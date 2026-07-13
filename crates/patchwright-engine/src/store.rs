use crate::{GitHubAccount, GitHubRepository, GitHubRepositorySnapshot};
use anyhow::{Context, Result};
use patchwright_core::{Task, TaskId};
use rusqlite::{Connection, OptionalExtension, params};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
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
        #[cfg(unix)]
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("restrict event database permissions {}", path.display()))?;
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
             CREATE TABLE IF NOT EXISTS github_account (
                 singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                 payload TEXT NOT NULL,
                 synced_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS github_snapshots (
                 full_name TEXT PRIMARY KEY,
                 repository_payload TEXT NOT NULL,
                 snapshot_payload TEXT NOT NULL,
                 synced_at TEXT NOT NULL
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

    pub fn save_github_account(&self, account: &GitHubAccount) -> Result<()> {
        self.connection.execute(
            "INSERT INTO github_account(singleton, payload, synced_at) VALUES (1, ?1, datetime('now'))
             ON CONFLICT(singleton) DO UPDATE SET payload=excluded.payload, synced_at=excluded.synced_at",
            [serde_json::to_string(account)?],
        )?;
        Ok(())
    }

    pub fn replace_github_snapshot(&self, snapshot: &GitHubRepositorySnapshot) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO github_snapshots(full_name, repository_payload, snapshot_payload, synced_at)
             VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(full_name) DO UPDATE SET repository_payload=excluded.repository_payload,
                 snapshot_payload=excluded.snapshot_payload, synced_at=excluded.synced_at",
            params![
                snapshot.repository.full_name,
                serde_json::to_string(&snapshot.repository)?,
                serde_json::to_string(snapshot)?,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn github_account(&self) -> Result<Option<GitHubAccount>> {
        let payload: Option<String> = self
            .connection
            .query_row(
                "SELECT payload FROM github_account WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        payload
            .map(|value| serde_json::from_str(&value).context("decode GitHub account"))
            .transpose()
    }

    pub fn github_repositories(&self) -> Result<Vec<GitHubRepository>> {
        let mut statement = self.connection.prepare(
            "SELECT repository_payload FROM github_snapshots ORDER BY full_name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let payload = row?;
            serde_json::from_str(&payload).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    payload.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .context("load GitHub repositories")
    }

    pub fn github_last_synced_at(&self) -> Result<Option<String>> {
        self.connection
            .query_row("SELECT MAX(synced_at) FROM github_snapshots", [], |row| {
                row.get(0)
            })
            .context("load latest GitHub sync time")
    }

    pub fn github_repository(&self, full_name: &str) -> Result<Option<GitHubRepositorySnapshot>> {
        let payload: Option<String> = self
            .connection
            .query_row(
                "SELECT snapshot_payload FROM github_snapshots WHERE full_name = ?1",
                [full_name],
                |row| row.get(0),
            )
            .optional()?;
        payload
            .map(|value| serde_json::from_str(&value).context("decode GitHub snapshot"))
            .transpose()
    }
}
