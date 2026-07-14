use crate::{
    CancellationState, GitHubAccount, GitHubRepository, GitHubRepositorySnapshot, Job,
    JobCheckpoint, JobId, JobState, TaskCheckpoint,
    codex::service::CodexRuntimeApproval,
    codex::session::{CodexEventDraft, CodexEventRecord, CodexSessionRecord, CodexSessionStatus},
    jobs::validate_summary,
};
use anyhow::{Context, Result, bail};
use patchwright_core::{
    Approval, QueueDecision, RepositoryBinding, RepositoryBindingId, Task, TaskContract, TaskId,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Serialize, de::DeserializeOwned};
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
        let mut connection = Connection::open(path)
            .with_context(|| format!("open event database {}", path.display()))?;
        #[cfg(unix)]
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("restrict event database permissions {}", path.display()))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        apply_migrations(&mut connection)?;
        let store = Self { connection };
        store.recover_interrupted_jobs()?;
        Ok(store)
    }

    pub fn schema_versions(&self) -> Result<Vec<u32>> {
        let mut statement = self
            .connection
            .prepare("SELECT version FROM schema_migrations ORDER BY version")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .context("load schema migration versions")
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

    pub fn save_task_with_checkpoint(
        &self,
        task: &Task,
        summary: &str,
        checkpoint: &TaskCheckpoint,
    ) -> Result<()> {
        anyhow::ensure!(
            task.id == checkpoint.task_id,
            "checkpoint task does not match"
        );
        anyhow::ensure!(
            task.state == checkpoint.state,
            "checkpoint state does not match"
        );
        validate_summary(checkpoint.summary.clone())?;
        let payload = serde_json::to_string(task)?;
        let checkpoint_payload = serde_json::to_string(checkpoint)?;
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
        transaction.execute(
            "INSERT INTO task_checkpoints(id, task_id, state, summary, payload, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                checkpoint.id.to_string(),
                checkpoint.task_id.to_string(),
                enum_name(checkpoint.state),
                checkpoint.summary,
                checkpoint_payload,
                checkpoint.occurred_at.to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn task_checkpoints(&self, task_id: TaskId) -> Result<Vec<TaskCheckpoint>> {
        let mut statement = self.connection.prepare(
            "SELECT payload FROM task_checkpoints WHERE task_id = ?1 ORDER BY occurred_at, id",
        )?;
        let rows = statement.query_map([task_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| decode_json_row(row, "task checkpoint"))
            .collect::<Result<Vec<_>>>()
    }

    pub fn checkpoint_codex_session(
        &self,
        session: &mut CodexSessionRecord,
        kind: &str,
        summary: &str,
    ) -> Result<()> {
        self.append_codex_event(session, CodexEventDraft::status(kind, summary))
    }

    pub fn append_codex_event(
        &self,
        session: &mut CodexSessionRecord,
        draft: CodexEventDraft,
    ) -> Result<()> {
        let kind = validate_codex_event_kind(&draft.kind)?;
        let summary = validate_summary(draft.summary)?;
        let content = draft
            .content
            .map(validate_codex_event_content)
            .transpose()?;
        for (value, field) in [
            (draft.thread_id.as_deref(), "thread id"),
            (draft.turn_id.as_deref(), "turn id"),
            (draft.item_id.as_deref(), "item id"),
        ] {
            if let Some(value) = value {
                validate_codex_identity(value, field)?;
            }
        }
        let transaction = self.connection.unchecked_transaction()?;
        let next_sequence: u64 = transaction.query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM codex_events WHERE task_id = ?1",
            [session.task_id.to_string()],
            |row| row.get(0),
        )?;
        let occurred_at = chrono::Utc::now();
        let mut next_session = session.clone();
        next_session.last_sequence = next_sequence;
        next_session.updated_at = occurred_at;
        let event = CodexEventRecord {
            task_id: session.task_id,
            process_generation: session.process_generation,
            sequence: next_sequence,
            kind: kind.clone(),
            summary: summary.clone(),
            thread_id: draft.thread_id,
            turn_id: draft.turn_id,
            item_id: draft.item_id,
            content,
            occurred_at,
        };
        upsert_codex_session(&transaction, &next_session)?;
        transaction.execute(
            "INSERT INTO codex_events(
                 task_id, sequence, process_generation, kind, summary, payload, occurred_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.task_id.to_string(),
                event.sequence,
                event.process_generation.to_string(),
                event.kind,
                event.summary,
                serde_json::to_string(&event)?,
                event.occurred_at.to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        *session = next_session;
        Ok(())
    }

    pub fn codex_session(&self, task_id: TaskId) -> Result<Option<CodexSessionRecord>> {
        self.load_json_optional(
            "SELECT payload FROM codex_sessions WHERE task_id = ?1",
            &task_id.to_string(),
            "Codex session",
        )
    }

    pub fn codex_events(&self, task_id: TaskId, after: u64) -> Result<Vec<CodexEventRecord>> {
        let mut statement = self.connection.prepare(
            "SELECT payload FROM codex_events
             WHERE task_id = ?1 AND sequence > ?2 ORDER BY sequence",
        )?;
        let rows = statement.query_map(params![task_id.to_string(), after], |row| {
            row.get::<_, String>(0)
        })?;
        rows.map(|row| decode_json_row(row, "Codex event"))
            .collect::<Result<Vec<_>>>()
    }

    pub fn save_codex_runtime_approval(&self, approval: &CodexRuntimeApproval) -> Result<()> {
        self.connection.execute("INSERT INTO codex_runtime_approvals(id, task_id, process_generation, request_id, state, payload, expires_at) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET state=excluded.state,payload=excluded.payload,expires_at=excluded.expires_at", params![approval.id.to_string(), approval.task_id.to_string(), approval.process_generation.to_string(), serde_json::to_string(&approval.request_id)?, enum_name(approval.state), serde_json::to_string(approval)?, approval.expires_at.to_rfc3339()])?;
        Ok(())
    }

    pub fn codex_runtime_approval(&self, id: uuid::Uuid) -> Result<Option<CodexRuntimeApproval>> {
        self.load_json_optional(
            "SELECT payload FROM codex_runtime_approvals WHERE id = ?1",
            &id.to_string(),
            "Codex runtime approval",
        )
    }

    pub fn codex_runtime_approvals(&self, task_id: TaskId) -> Result<Vec<CodexRuntimeApproval>> {
        let mut statement = self.connection.prepare("SELECT payload FROM codex_runtime_approvals WHERE task_id = ?1 ORDER BY expires_at, id")?;
        let rows = statement.query_map([task_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| decode_json_row(row, "Codex runtime approval"))
            .collect()
    }

    pub fn enter_implementing_with_codex(
        &self,
        task: &Task,
        checkpoint: &TaskCheckpoint,
        session: &CodexSessionRecord,
    ) -> Result<()> {
        anyhow::ensure!(
            task.id == checkpoint.task_id,
            "checkpoint task does not match"
        );
        anyhow::ensure!(
            task.state == checkpoint.state,
            "checkpoint state does not match"
        );
        anyhow::ensure!(
            task.state == patchwright_core::TaskState::Implementing,
            "task is not entering implementing"
        );
        anyhow::ensure!(
            session.task_id == task.id,
            "Codex session task does not match"
        );
        anyhow::ensure!(
            session.status == CodexSessionStatus::Ready && session.thread_id.is_some(),
            "Codex session is not ready"
        );
        validate_summary(checkpoint.summary.clone())?;
        let transaction = self.connection.unchecked_transaction()?;
        let persisted_task: String = transaction.query_row(
            "SELECT payload FROM tasks WHERE id = ?1",
            [task.id.to_string()],
            |row| row.get(0),
        )?;
        let persisted_task: Task = serde_json::from_str(&persisted_task)?;
        anyhow::ensure!(
            persisted_task.state == patchwright_core::TaskState::Preparing,
            "persisted task is not prepared"
        );
        let persisted_session: String = transaction.query_row(
            "SELECT payload FROM codex_sessions WHERE task_id = ?1",
            [task.id.to_string()],
            |row| row.get(0),
        )?;
        let persisted_session: CodexSessionRecord = serde_json::from_str(&persisted_session)?;
        anyhow::ensure!(
            persisted_session.process_generation == session.process_generation
                && persisted_session.status == CodexSessionStatus::Ready
                && persisted_session.thread_id == session.thread_id,
            "persisted Codex session does not match the ready generation"
        );

        let task_payload = serde_json::to_string(task)?;
        let checkpoint_payload = serde_json::to_string(checkpoint)?;
        transaction.execute(
            "UPDATE tasks SET payload = ?2, updated_at = ?3 WHERE id = ?1",
            params![
                task.id.to_string(),
                task_payload,
                task.updated_at.to_rfc3339()
            ],
        )?;
        transaction.execute(
            "INSERT INTO task_events(task_id, summary, payload, occurred_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                task.id.to_string(),
                checkpoint.summary,
                task_payload,
                task.updated_at.to_rfc3339()
            ],
        )?;
        transaction.execute(
            "INSERT INTO task_checkpoints(id, task_id, state, summary, payload, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                checkpoint.id.to_string(),
                checkpoint.task_id.to_string(),
                enum_name(checkpoint.state),
                checkpoint.summary,
                checkpoint_payload,
                checkpoint.occurred_at.to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn save_repository_binding(&self, binding: &RepositoryBinding) -> Result<()> {
        self.connection.execute(
            "INSERT INTO repository_bindings(id, full_name, payload, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET full_name=excluded.full_name,
                 payload=excluded.payload, updated_at=excluded.updated_at",
            params![
                binding.id().to_string(),
                binding.full_name(),
                serde_json::to_string(binding)?,
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn repository_binding(&self, id: RepositoryBindingId) -> Result<Option<RepositoryBinding>> {
        self.load_json_optional(
            "SELECT payload FROM repository_bindings WHERE id = ?1",
            &id.to_string(),
            "repository binding",
        )
    }

    pub fn repository_binding_by_full_name(
        &self,
        full_name: &str,
    ) -> Result<Option<RepositoryBinding>> {
        self.load_json_optional(
            "SELECT payload FROM repository_bindings WHERE full_name = ?1",
            full_name,
            "repository binding",
        )
    }

    pub fn save_task_contract(&self, contract: &TaskContract) -> Result<()> {
        self.connection.execute(
            "INSERT INTO task_contracts(task_id, binding_id, version, payload, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(task_id) DO UPDATE SET binding_id=excluded.binding_id,
                 version=excluded.version, payload=excluded.payload, updated_at=excluded.updated_at",
            params![
                contract.task_id().to_string(),
                contract.repository_binding_id().to_string(),
                contract.version(),
                serde_json::to_string(contract)?,
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn task_contract(&self, task_id: TaskId) -> Result<Option<TaskContract>> {
        self.load_json_optional(
            "SELECT payload FROM task_contracts WHERE task_id = ?1",
            &task_id.to_string(),
            "task contract",
        )
    }

    pub fn save_approval(&self, approval: &Approval) -> Result<()> {
        self.connection.execute(
            "INSERT INTO approvals(id, task_id, capability, action_digest, payload, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET payload=excluded.payload,
                 action_digest=excluded.action_digest, expires_at=excluded.expires_at",
            params![
                approval.id().to_string(),
                approval.fingerprint().task_id().to_string(),
                enum_name(approval.capability()),
                approval.fingerprint().digest_sha256(),
                serde_json::to_string(approval)?,
                approval.expires_at().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn approval(&self, id: uuid::Uuid) -> Result<Option<Approval>> {
        self.load_json_optional(
            "SELECT payload FROM approvals WHERE id = ?1",
            &id.to_string(),
            "approval",
        )
    }

    pub fn approval_action_digest(&self, id: uuid::Uuid) -> Result<Option<String>> {
        self.connection
            .query_row(
                "SELECT action_digest FROM approvals WHERE id = ?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .context("load approval action digest")
    }

    fn load_json_optional<T: DeserializeOwned>(
        &self,
        statement: &str,
        key: &str,
        label: &'static str,
    ) -> Result<Option<T>> {
        let payload: Option<String> = self
            .connection
            .query_row(statement, [key], |row| row.get(0))
            .optional()?;
        payload
            .map(|value| serde_json::from_str(&value).with_context(|| format!("decode {label}")))
            .transpose()
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

    pub fn tasks(&self) -> Result<Vec<Task>> {
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM tasks ORDER BY updated_at DESC, id ASC")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| decode_json_row(row, "task"))
            .collect::<Result<Vec<_>>>()
    }

    pub fn create_converted_task(
        &self,
        task: &Task,
        contract: &TaskContract,
        source_key: &str,
    ) -> Result<(Task, bool)> {
        anyhow::ensure!(
            task.id == contract.task_id(),
            "task contract ID does not match"
        );
        anyhow::ensure!(
            task.repository_binding_id == Some(contract.repository_binding_id()),
            "task contract binding does not match"
        );
        anyhow::ensure!(!source_key.is_empty(), "source key is required");
        let transaction = self.connection.unchecked_transaction()?;
        let existing_task_id: Option<String> = transaction
            .query_row(
                "SELECT task_id FROM task_sources WHERE source_key = ?1",
                [source_key],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing_task_id) = existing_task_id {
            let payload: String = transaction.query_row(
                "SELECT payload FROM tasks WHERE id = ?1",
                [existing_task_id],
                |row| row.get(0),
            )?;
            let existing = serde_json::from_str(&payload).context("decode converted task")?;
            transaction.rollback()?;
            return Ok((existing, false));
        }
        let task_payload = serde_json::to_string(task)?;
        let occurred_at = task.updated_at.to_rfc3339();
        transaction.execute(
            "INSERT INTO tasks(id, payload, updated_at) VALUES (?1, ?2, ?3)",
            params![task.id.to_string(), task_payload, occurred_at],
        )?;
        transaction.execute(
            "INSERT INTO task_events(task_id, summary, payload, occurred_at)
             VALUES (?1, 'task created from GitHub snapshot', ?2, ?3)",
            params![task.id.to_string(), task_payload, occurred_at],
        )?;
        transaction.execute(
            "INSERT INTO task_contracts(task_id, binding_id, version, payload, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                task.id.to_string(),
                contract.repository_binding_id().to_string(),
                contract.version(),
                serde_json::to_string(contract)?,
                occurred_at,
            ],
        )?;
        transaction.execute(
            "INSERT INTO task_sources(source_key, task_id, created_at) VALUES (?1, ?2, ?3)",
            params![source_key, task.id.to_string(), occurred_at],
        )?;
        transaction.commit()?;
        Ok((task.clone(), true))
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

    pub fn create_job(&self, job: &Job) -> Result<()> {
        let checkpoint_payload = job
            .checkpoint
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO jobs(
                 id, kind, task_id, state, cancellation_state, summary, checkpoint_payload,
                 created_at, updated_at, generation
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                job.id.to_string(),
                enum_name(job.kind),
                job.task_id.map(|value| value.to_string()),
                enum_name(job.state),
                enum_name(job.cancellation),
                job.summary,
                checkpoint_payload,
                job.created_at.to_rfc3339(),
                job.updated_at.to_rfc3339(),
                job.generation,
            ],
        )?;
        transaction.execute(
            "INSERT INTO job_events(job_id, state, summary, checkpoint_payload, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                job.id.to_string(),
                enum_name(job.state),
                job.summary,
                checkpoint_payload,
                job.created_at.to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn transition_job(
        &self,
        id: JobId,
        expected: JobState,
        next: JobState,
        cancellation: CancellationState,
        summary: &str,
        checkpoint: Option<&JobCheckpoint>,
    ) -> Result<bool> {
        if !expected.permits(next) {
            bail!("invalid job transition: {expected:?} -> {next:?}");
        }
        let summary = validate_summary(summary.to_owned())?;
        let transaction = self.connection.unchecked_transaction()?;
        let current: Option<(String, Option<String>, u64)> = transaction
            .query_row(
                "SELECT state, checkpoint_payload, generation FROM jobs WHERE id = ?1",
                [id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((current_state, existing_checkpoint, generation)) = current else {
            transaction.rollback()?;
            return Ok(false);
        };
        if current_state != enum_name(expected) {
            transaction.rollback()?;
            return Ok(false);
        }
        let checkpoint_payload = checkpoint
            .map(serde_json::to_string)
            .transpose()?
            .or(existing_checkpoint);
        let occurred_at = chrono::Utc::now();
        let updated = transaction.execute(
            "UPDATE jobs SET state = ?2, cancellation_state = ?3, summary = ?4,
                 checkpoint_payload = ?5, updated_at = ?6, generation = ?7
             WHERE id = ?1 AND state = ?8 AND generation = ?9",
            params![
                id.to_string(),
                enum_name(next),
                enum_name(cancellation),
                summary,
                checkpoint_payload,
                occurred_at.to_rfc3339(),
                generation + 1,
                enum_name(expected),
                generation,
            ],
        )?;
        if updated != 1 {
            transaction.rollback()?;
            return Ok(false);
        }
        transaction.execute(
            "INSERT INTO job_events(job_id, state, summary, checkpoint_payload, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id.to_string(),
                enum_name(next),
                summary,
                checkpoint_payload,
                occurred_at.to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn job(&self, id: JobId) -> Result<Option<Job>> {
        self.connection
            .query_row(
                "SELECT kind, task_id, state, cancellation_state, summary, checkpoint_payload,
                        created_at, updated_at, generation
                 FROM jobs WHERE id = ?1",
                [id.to_string()],
                |row| {
                    let kind: String = row.get(0)?;
                    let task_id: Option<String> = row.get(1)?;
                    let state: String = row.get(2)?;
                    let cancellation: String = row.get(3)?;
                    let summary: String = row.get(4)?;
                    let checkpoint: Option<String> = row.get(5)?;
                    let created_at: String = row.get(6)?;
                    let updated_at: String = row.get(7)?;
                    let generation: u64 = row.get(8)?;
                    Ok((
                        kind,
                        task_id,
                        state,
                        cancellation,
                        summary,
                        checkpoint,
                        created_at,
                        updated_at,
                        generation,
                    ))
                },
            )
            .optional()?
            .map(|row| decode_job(id, row))
            .transpose()
    }

    pub fn job_timeline(&self, id: JobId) -> Result<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT summary FROM job_events WHERE job_id = ?1 ORDER BY sequence")?;
        let rows = statement.query_map([id.to_string()], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .context("load job timeline")
    }

    fn recover_interrupted_jobs(&self) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        let recoverable = {
            let mut statement = transaction.prepare(
                "SELECT id, generation FROM jobs
                 WHERE state IN ('running', 'cancelling') ORDER BY id",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for (id, generation) in recoverable {
            let occurred_at = chrono::Utc::now().to_rfc3339();
            let summary = "engine restart interrupted job";
            let updated = transaction.execute(
                "UPDATE jobs SET state = 'interrupted', summary = ?2, updated_at = ?3,
                     generation = ?4 WHERE id = ?1 AND generation = ?5
                     AND state IN ('running', 'cancelling')",
                params![id, summary, occurred_at, generation + 1, generation],
            )?;
            if updated == 1 {
                transaction.execute(
                    "INSERT INTO job_events(job_id, state, summary, checkpoint_payload, occurred_at)
                     SELECT id, 'interrupted', ?2, checkpoint_payload, ?3 FROM jobs WHERE id = ?1",
                    params![id, summary, occurred_at],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
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
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(full_name) DO UPDATE SET repository_payload=excluded.repository_payload,
                 snapshot_payload=excluded.snapshot_payload, synced_at=excluded.synced_at",
            params![
                snapshot.repository.full_name,
                serde_json::to_string(&snapshot.repository)?,
                serde_json::to_string(snapshot)?,
                chrono::Utc::now().to_rfc3339(),
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

    pub fn github_work_items(&self) -> Result<Vec<crate::GitHubWorkItem>> {
        let mut statement = self.connection.prepare(
            "SELECT snapshot_payload FROM github_snapshots ORDER BY full_name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut items = Vec::new();
        for row in rows {
            let snapshot: GitHubRepositorySnapshot = decode_json_row(row, "GitHub snapshot")?;
            items.extend(snapshot.work_items);
        }
        items.sort_by(|left, right| {
            left.repository_full_name
                .cmp(&right.repository_full_name)
                .then_with(|| left.number.cmp(&right.number))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(items)
    }

    pub fn github_last_synced_at(&self) -> Result<Option<String>> {
        self.connection
            .query_row("SELECT MAX(synced_at) FROM github_snapshots", [], |row| {
                row.get(0)
            })
            .context("load latest GitHub sync time")
    }

    pub fn replace_queue_decisions(&self, decisions: &[QueueDecision]) -> Result<()> {
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute("DELETE FROM queue_decisions", [])?;
        for (position, decision) in decisions.iter().enumerate() {
            transaction.execute(
                "INSERT INTO queue_decisions(repository_full_name, pull_request_number, position, payload, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    decision.repository_full_name,
                    decision.number,
                    position,
                    serde_json::to_string(decision)?,
                    chrono::Utc::now().to_rfc3339(),
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn queue_decisions(&self) -> Result<Vec<QueueDecision>> {
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM queue_decisions ORDER BY position")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| serde_json::from_str(&row?).context("decode persisted queue decision"))
            .collect()
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

    pub fn github_repository_with_snapshot_at(
        &self,
        full_name: &str,
    ) -> Result<Option<(GitHubRepositorySnapshot, chrono::DateTime<chrono::Utc>)>> {
        let row: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT snapshot_payload, synced_at FROM github_snapshots WHERE full_name = ?1",
                [full_name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(payload, synced_at)| {
            let snapshot = serde_json::from_str(&payload).context("decode GitHub snapshot")?;
            let snapshot_at = parse_persisted_timestamp(&synced_at)?;
            Ok((snapshot, snapshot_at))
        })
        .transpose()
    }
}

type PersistedJobRow = (
    String,
    Option<String>,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    u64,
);

fn decode_job(id: JobId, row: PersistedJobRow) -> Result<Job> {
    Ok(Job {
        id,
        kind: decode_enum(&row.0, "job kind")?,
        task_id: row
            .1
            .map(|value| value.parse().context("decode job task id"))
            .transpose()?,
        state: decode_enum(&row.2, "job state")?,
        cancellation: decode_enum(&row.3, "job cancellation state")?,
        summary: validate_summary(row.4)?,
        checkpoint: row
            .5
            .map(|value| serde_json::from_str(&value).context("decode job checkpoint"))
            .transpose()?,
        created_at: row.6.parse().context("decode job creation time")?,
        updated_at: row.7.parse().context("decode job update time")?,
        generation: row.8,
    })
}

fn enum_name<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .expect("enum serialization")
        .as_str()
        .expect("enum serializes as a string")
        .to_owned()
}

fn decode_enum<T: DeserializeOwned>(value: &str, label: &'static str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .with_context(|| format!("decode {label}"))
}

fn decode_json_row<T: DeserializeOwned>(
    row: std::result::Result<String, rusqlite::Error>,
    label: &'static str,
) -> Result<T> {
    let payload = row?;
    serde_json::from_str(&payload).with_context(|| format!("decode {label}"))
}

fn parse_persisted_timestamp(value: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(value) {
        return Ok(timestamp.with_timezone(&chrono::Utc));
    }
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .map(|timestamp| timestamp.and_utc())
        .context("decode persisted timestamp")
}

fn validate_codex_event_kind(value: &str) -> Result<String> {
    let value = value.trim();
    anyhow::ensure!(
        !value.is_empty() && value.len() <= 64 && !value.chars().any(char::is_control),
        "invalid Codex event kind"
    );
    Ok(value.to_owned())
}

fn validate_codex_identity(value: &str, field: &'static str) -> Result<()> {
    anyhow::ensure!(
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control),
        "invalid Codex {field}"
    );
    Ok(())
}

fn validate_codex_event_content(value: String) -> Result<String> {
    anyhow::ensure!(
        value.len() <= 64 * 1024 && !value.contains('\0'),
        "invalid Codex event content"
    );
    Ok(value)
}

fn upsert_codex_session(
    transaction: &rusqlite::Transaction<'_>,
    session: &CodexSessionRecord,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO codex_sessions(
             task_id, process_generation, status, thread_id, last_turn_id,
             last_sequence, payload, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(task_id) DO UPDATE SET
             process_generation=excluded.process_generation,
             status=excluded.status,
             thread_id=excluded.thread_id,
             last_turn_id=excluded.last_turn_id,
             last_sequence=excluded.last_sequence,
             payload=excluded.payload,
             updated_at=excluded.updated_at",
        params![
            session.task_id.to_string(),
            session.process_generation.to_string(),
            enum_name(session.status),
            session.thread_id,
            session.last_turn_id,
            session.last_sequence,
            serde_json::to_string(session)?,
            session.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn apply_migrations(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
             version INTEGER PRIMARY KEY,
             applied_at TEXT NOT NULL
         );",
    )?;
    let applied = {
        let mut statement =
            transaction.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
        let rows = statement.query_map([], |row| row.get::<_, u32>(0))?;
        rows.collect::<std::result::Result<std::collections::HashSet<_>, _>>()?
    };
    for (version, sql) in MIGRATIONS {
        if applied.contains(version) {
            continue;
        }
        transaction
            .execute_batch(sql)
            .with_context(|| format!("apply schema migration {version}"))?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
            params![version, chrono::Utc::now().to_rfc3339()],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

const MIGRATIONS: &[(u32, &str)] = &[
    (
        1,
        "CREATE TABLE IF NOT EXISTS tasks (
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
         );",
    ),
    (
        2,
        "CREATE TABLE IF NOT EXISTS repository_bindings (
             id TEXT PRIMARY KEY,
             full_name TEXT NOT NULL UNIQUE,
             payload TEXT NOT NULL,
             updated_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS task_contracts (
             task_id TEXT PRIMARY KEY,
             binding_id TEXT NOT NULL,
             version INTEGER NOT NULL,
             payload TEXT NOT NULL,
             updated_at TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS task_contracts_binding_id
             ON task_contracts(binding_id, task_id);
         CREATE TABLE IF NOT EXISTS approvals (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL,
             capability TEXT NOT NULL,
             action_digest TEXT NOT NULL,
             payload TEXT NOT NULL,
             expires_at TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS approvals_task_id ON approvals(task_id, expires_at);
         CREATE TABLE IF NOT EXISTS jobs (
             id TEXT PRIMARY KEY,
             kind TEXT NOT NULL,
             task_id TEXT,
             state TEXT NOT NULL,
             cancellation_state TEXT NOT NULL,
             summary TEXT NOT NULL,
             checkpoint_payload TEXT,
             created_at TEXT NOT NULL,
             updated_at TEXT NOT NULL,
             generation INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS jobs_state ON jobs(state, updated_at);
         CREATE TABLE IF NOT EXISTS job_events (
             sequence INTEGER PRIMARY KEY AUTOINCREMENT,
             job_id TEXT NOT NULL,
             state TEXT NOT NULL,
             summary TEXT NOT NULL,
             checkpoint_payload TEXT,
             occurred_at TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS job_events_job_id ON job_events(job_id, sequence);
         CREATE TABLE IF NOT EXISTS task_checkpoints (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL,
             state TEXT NOT NULL,
             summary TEXT NOT NULL,
             payload TEXT NOT NULL,
             occurred_at TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS task_checkpoints_task_id
             ON task_checkpoints(task_id, occurred_at);",
    ),
    (
        3,
        "CREATE TABLE IF NOT EXISTS task_sources (
             source_key TEXT PRIMARY KEY,
             task_id TEXT NOT NULL UNIQUE,
             created_at TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS task_sources_task_id ON task_sources(task_id);",
    ),
    (
        4,
        "CREATE TABLE IF NOT EXISTS codex_sessions (
             task_id TEXT PRIMARY KEY,
             process_generation TEXT NOT NULL,
             status TEXT NOT NULL,
             thread_id TEXT,
             last_turn_id TEXT,
             last_sequence INTEGER NOT NULL,
             payload TEXT NOT NULL,
             updated_at TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS codex_sessions_status
             ON codex_sessions(status, updated_at);
         CREATE TABLE IF NOT EXISTS codex_events (
             task_id TEXT NOT NULL,
             sequence INTEGER NOT NULL,
             process_generation TEXT NOT NULL,
             kind TEXT NOT NULL,
             summary TEXT NOT NULL,
             payload TEXT NOT NULL,
             occurred_at TEXT NOT NULL,
             PRIMARY KEY(task_id, sequence)
         );
         CREATE INDEX IF NOT EXISTS codex_events_generation
             ON codex_events(task_id, process_generation, sequence);",
    ),
    (5, "CREATE TABLE IF NOT EXISTS codex_runtime_approvals (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL,
             process_generation TEXT NOT NULL,
             request_id TEXT NOT NULL,
             state TEXT NOT NULL,
             payload TEXT NOT NULL,
             expires_at TEXT NOT NULL
         );
         CREATE UNIQUE INDEX IF NOT EXISTS codex_runtime_approvals_request ON codex_runtime_approvals(task_id, process_generation, request_id);
         CREATE INDEX IF NOT EXISTS codex_runtime_approvals_task ON codex_runtime_approvals(task_id, state, expires_at);"),
    (6, "CREATE TABLE IF NOT EXISTS queue_decisions (
             repository_full_name TEXT NOT NULL,
             pull_request_number INTEGER NOT NULL,
             position INTEGER NOT NULL,
             payload TEXT NOT NULL,
             updated_at TEXT NOT NULL,
             PRIMARY KEY(repository_full_name, pull_request_number)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS queue_decisions_position ON queue_decisions(position);")
];
