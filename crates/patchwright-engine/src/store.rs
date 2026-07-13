use anyhow::{Context, Result};
use patchwright_core::{Task, TaskId};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

pub struct EventStore {
    connection: Connection,
}

impl EventStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create database directory {}", parent.display()))?;
        }
        let connection = Connection::open(path)
            .with_context(|| format!("open event database {}", path.display()))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.execute_batch(
            "BEGIN;
             CREATE TABLE IF NOT EXISTS tasks (
                 id TEXT PRIMARY KEY,
                 payload TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS task_events (
                 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                 task_id TEXT NOT NULL,
                 summary TEXT NOT NULL,
                 payload TEXT NOT NULL,
                 occurred_at TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS task_events_task_id ON task_events(task_id, sequence);
             CREATE TABLE IF NOT EXISTS deliveries (
                 key TEXT PRIMARY KEY,
                 result TEXT,
                 claimed_at TEXT NOT NULL,
                 completed_at TEXT
             );
             COMMIT;",
        )?;
        Ok(Self { connection })
    }

    pub fn save_task(&self, task: &Task, summary: &str) -> Result<()> {
        let payload = serde_json::to_string(task)?;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO tasks(id, payload, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET payload=excluded.payload, updated_at=excluded.updated_at",
            params![task.id.to_string(), payload, task.updated_at.to_rfc3339()],
        )?;
        transaction.execute(
            "INSERT INTO task_events(task_id, summary, payload, occurred_at) VALUES (?1, ?2, ?3, ?4)",
            params![task.id.to_string(), summary, payload, task.updated_at.to_rfc3339()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn load_task(&self, id: TaskId) -> Result<Option<Task>> {
        let payload: Option<String> = self
            .connection
            .query_row(
                "SELECT payload FROM tasks WHERE id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        payload
            .map(|value| serde_json::from_str(&value).context("decode persisted task"))
            .transpose()
    }

    pub fn timeline(&self, id: TaskId) -> Result<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM task_events WHERE task_id = ?1 ORDER BY sequence ASC")?;
        let rows = statement.query_map([id.to_string()], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .context("load task timeline")
    }

    pub fn claim_delivery(&self, key: &str) -> Result<bool> {
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO deliveries(key, claimed_at) VALUES (?1, datetime('now'))",
            [key],
        )?;
        Ok(inserted == 1)
    }

    pub fn complete_delivery(&self, key: &str, result: &str) -> Result<()> {
        let updated = self.connection.execute(
            "UPDATE deliveries SET result = ?2, completed_at = datetime('now') WHERE key = ?1",
            params![key, result],
        )?;
        anyhow::ensure!(updated == 1, "delivery key was not claimed");
        Ok(())
    }

    pub fn delivery_result(&self, key: &str) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT result FROM deliveries WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()
            .context("load delivery result")
    }
}
