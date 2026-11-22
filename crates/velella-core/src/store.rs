use crate::error::CoreError;
use crate::event::{Event, EventKind};
use crate::ids::TaskId;
use crate::state::TaskState;
use crate::task::TaskSpec;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS tasks (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  spec_yaml  TEXT NOT NULL,
  state      TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  worktree   TEXT,
  session_id TEXT,
  pr_url     TEXT
);
CREATE TABLE IF NOT EXISTS events (
  seq     INTEGER PRIMARY KEY AUTOINCREMENT,
  id      TEXT NOT NULL,
  ts      TEXT NOT NULL,
  kind    TEXT NOT NULL,
  subject TEXT NOT NULL,
  payload TEXT NOT NULL
);
";

/// One queued/known task as stored.
#[derive(Debug, Clone)]
pub struct TaskRecord {
    pub id: TaskId,
    pub name: String,
    pub state: TaskState,
    pub spec: TaskSpec,
    pub created_at: String,
    pub updated_at: String,
    /// Bound worktree path once the runner created it.
    pub worktree: Option<String>,
    /// Bound agent session once spawned.
    pub session_id: Option<String>,
    /// PR / compare link once delivery ran.
    pub pr_url: Option<String>,
}

/// Raw task row straight from SQLite, before spec parsing.
type TaskRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// SQLite-backed task repo + append-only event log. The event log is the
/// product's spine: UIs, notifications, automations and orchestration all
/// consume the same stream.
pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        // VACUUM INTO includes the WAL and produces a consistent backup. Never
        // overwrite user_version before inspecting the existing schema.
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let has_tasks: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='tasks')", [], |r| r.get(0),
        )?;
        if has_tasks && version < 3 {
            let backup = path.with_extension(format!("db.pre-v3-{}.bak", Utc::now().timestamp_millis()));
            conn.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])?;
        }
        if !has_tasks {
            conn.execute_batch(SCHEMA)?;
            conn.execute_batch("PRAGMA user_version = 3;")?;
        }
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self, CoreError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch("PRAGMA user_version = 3;")?;
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Databases created before the worktree/session (v2) and pr_url (v3)
    /// columns. Fresh databases already carry everything and user_version 3.
    fn migrate(conn: &Connection) -> Result<(), CoreError> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 2 {
            conn.execute_batch(
                "ALTER TABLE tasks ADD COLUMN worktree TEXT;
                 ALTER TABLE tasks ADD COLUMN session_id TEXT;",
            )?;
        }
        if version < 3 {
            conn.execute_batch(
                "ALTER TABLE tasks ADD COLUMN pr_url TEXT;
                 PRAGMA user_version = 3;",
            )?;
        }
        Ok(())
    }

    /// Queue a task spec. Duplicate names are rejected: task names are the
    /// human handle used in review, gates and automations.
    pub fn queue_task(&self, spec: &TaskSpec) -> Result<TaskId, CoreError> {
        let id = TaskId::generate();
        let now = Utc::now().to_rfc3339();
        let spec_yaml = spec.to_yaml()?;
        let inserted = self.conn.execute(
            "INSERT INTO tasks (id, name, spec_yaml, state, created_at, updated_at, worktree, session_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL)",
            params![id.0, spec.name, spec_yaml, TaskState::Queued.as_str(), now, now],
        );
        if let Err(e) = inserted {
            if e.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                return Err(CoreError::TaskExists(spec.name.clone()));
            }
            return Err(e.into());
        }
        self.append_event(
            EventKind::TaskQueued,
            &id,
            serde_json::json!({ "name": spec.name, "agent": spec.agent }),
        )?;
        Ok(id)
    }

    pub fn get_task(&self, name: &str) -> Result<Option<TaskRecord>, CoreError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, name, spec_yaml, state, created_at, updated_at, worktree, session_id, pr_url
                 FROM tasks WHERE name = ?1",
                params![name],
                Self::map_task_row,
            )
            .optional()?;
        row.map(Self::build_record).transpose()
    }

    pub fn list_tasks(&self) -> Result<Vec<TaskRecord>, CoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, spec_yaml, state, created_at, updated_at, worktree, session_id, pr_url
             FROM tasks ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([], Self::map_task_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter().map(Self::build_record).collect()
    }

    /// Move a task's state, enforcing the normalized state machine. Emits a
    /// `TaskStateChanged` event on success.
    pub fn set_task_state(
        &self,
        id: &TaskId,
        from: TaskState,
        to: TaskState,
    ) -> Result<(), CoreError> {
        if !from.can_transition_to(to) {
            return Err(CoreError::IllegalTransition(from.as_str().into(), to.as_str().into()));
        }
        let now = Utc::now().to_rfc3339();
        let changed = self.conn.execute(
            "UPDATE tasks SET state = ?1, updated_at = ?2 WHERE id = ?3 AND state = ?4",
            params![to.as_str(), now, id.0, from.as_str()],
        )?;
        if changed == 0 {
            return Err(CoreError::TaskNotFound(id.to_string()));
        }
        self.append_event(
            EventKind::TaskStateChanged,
            id,
            serde_json::json!({ "from": from.as_str(), "to": to.as_str() }),
        )?;
        Ok(())
    }

    /// Bind the runner's worktree path and agent session to a task.
    pub fn set_task_binding(
        &self,
        id: &TaskId,
        worktree: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<(), CoreError> {
        let changed = self.conn.execute(
            "UPDATE tasks SET worktree = ?1, session_id = ?2, updated_at = ?3 WHERE id = ?4",
            params![worktree, session_id, Utc::now().to_rfc3339(), id.0],
        )?;
        if changed == 0 {
            return Err(CoreError::TaskNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Record the delivery link (PR or compare URL) for a task.
    pub fn set_task_pr_url(&self, id: &TaskId, url: &str) -> Result<(), CoreError> {
        let changed = self.conn.execute(
            "UPDATE tasks SET pr_url = ?1, updated_at = ?2 WHERE id = ?3",
            params![url, Utc::now().to_rfc3339(), id.0],
        )?;
        if changed == 0 {
            return Err(CoreError::TaskNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Append one event; seq is assigned by the log, never by callers.
    pub fn append_event(
        &self,
        kind: EventKind,
        subject: &dyn std::fmt::Display,
        payload: serde_json::Value,
    ) -> Result<Event, CoreError> {
        let id = uuid::Uuid::now_v7().to_string();
        let ts = Utc::now();
        self.conn.execute(
            "INSERT INTO events (id, ts, kind, subject, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                ts.to_rfc3339(),
                serde_json::to_string(&kind)?,
                subject.to_string(),
                payload.to_string(),
            ],
        )?;
        Ok(Event {
            seq: self.conn.last_insert_rowid(),
            id,
            ts,
            kind,
            subject: subject.to_string(),
            payload,
        })
    }

    pub fn events_for(&self, subject: &str) -> Result<Vec<Event>, CoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT seq, id, ts, kind, subject, payload FROM events WHERE subject = ?1 ORDER BY seq")?;
        let rows = stmt.query_map(params![subject], |row| {
            let ts: String = row.get(2)?;
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                DateTime::parse_from_rfc3339(&ts)
                    .map(|t| t.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            2,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    }),
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (seq, id, ts, kind, subject, payload) = r?;
            out.push(Event {
                seq,
                id,
                ts: ts?,
                kind: serde_json::from_str(&kind)?,
                subject,
                payload: serde_json::from_str(&payload)?,
            });
        }
        Ok(out)
    }

    /// Raw task row straight from SQLite, before spec parsing.
    fn map_task_row(
        row: &rusqlite::Row<'_>,
    ) -> rusqlite::Result<TaskRow> {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
            row.get(8)?,
        ))
    }

    #[allow(clippy::type_complexity)]
    fn build_record(row: TaskRow) -> Result<TaskRecord, CoreError> {
        let (id, name, spec_yaml, state, created_at, updated_at, worktree, session_id, pr_url) = row;
        Ok(TaskRecord {
            id: TaskId(id),
            name,
            state: TaskState::from_str(&state)?,
            spec: TaskSpec::from_yaml_str(&spec_yaml)?,
            created_at,
            updated_at,
            worktree,
            session_id,
            pr_url,
        })
    }
}

impl TaskState {
    fn from_str(s: &str) -> Result<Self, CoreError> {
        serde_json::from_str(&format!("\"{s}\"")).map_err(|e| {
            CoreError::IllegalTransition(s.into(), format!("unknown task state: {e}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str) -> TaskSpec {
        TaskSpec::from_yaml_str(&format!("name: {name}\nagent: claude-code\n")).unwrap()
    }

    #[test]
    fn legacy_database_is_backed_up_before_versioned_migration() {
        let root=std::env::temp_dir().join(format!("velella-migration-{}",uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let path=root.join("state.db");
        let conn=Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE tasks(id TEXT PRIMARY KEY,name TEXT NOT NULL UNIQUE,spec_yaml TEXT NOT NULL,state TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE events(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL,ts TEXT NOT NULL,kind TEXT NOT NULL,subject TEXT NOT NULL,payload TEXT NOT NULL); PRAGMA user_version=1;").unwrap();
        conn.execute("INSERT INTO tasks VALUES('tsk_old','old','name: old\nagent: claude-code\non_done: pr\n','queued','2026-10-11','2026-10-11')",[]).unwrap();
        drop(conn);
        let store=Store::open(&path).unwrap();
        let task=store.get_task("old").unwrap().unwrap();
        assert_eq!(task.spec.on_done,crate::task::OnDone::Pr);
        assert!(task.worktree.is_none());
        let backups:Vec<_>=std::fs::read_dir(&root).unwrap().filter_map(Result::ok).filter(|e|e.path().extension().is_some_and(|v|v=="bak")).collect();
        assert_eq!(backups.len(),1);
        let backup=Connection::open(backups[0].path()).unwrap();
        assert_eq!(backup.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    }


    #[test]
    fn queue_and_list_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        let id = store.queue_task(&spec("alpha")).unwrap();
        assert!(id.0.starts_with("tsk_"));

        let tasks = store.list_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "alpha");
        assert_eq!(tasks[0].state, TaskState::Queued);
        assert_eq!(tasks[0].spec.agent, "claude-code");
    }

    #[test]
    fn duplicate_names_rejected() {
        let store = Store::open_in_memory().unwrap();
        store.queue_task(&spec("alpha")).unwrap();
        let err = store.queue_task(&spec("alpha")).unwrap_err();
        assert!(matches!(err, CoreError::TaskExists(_)));
    }

    #[test]
    fn state_transitions_are_enforced_and_logged() {
        let store = Store::open_in_memory().unwrap();
        let id = store.queue_task(&spec("alpha")).unwrap();

        // illegal jump is rejected
        let err = store
            .set_task_state(&id, TaskState::Queued, TaskState::Merged)
            .unwrap_err();
        assert!(matches!(err, CoreError::IllegalTransition(_, _)));

        // legal path Queued -> Running -> Verifying
        store.set_task_state(&id, TaskState::Queued, TaskState::Running).unwrap();
        store.set_task_state(&id, TaskState::Running, TaskState::Verifying).unwrap();

        let task = store.get_task("alpha").unwrap().unwrap();
        assert_eq!(task.state, TaskState::Verifying);

        let events = store.events_for(&id.to_string()).unwrap();
        let kinds: Vec<_> = events.iter().map(|e| e.kind).collect();
        assert!(kinds.contains(&EventKind::TaskQueued));
        assert!(kinds.contains(&EventKind::TaskStateChanged));
        // seq strictly increases
        for w in events.windows(2) {
            assert!(w[1].seq > w[0].seq);
        }
    }

    #[test]
    fn optimistic_state_guard() {
        let store = Store::open_in_memory().unwrap();
        let id = store.queue_task(&spec("alpha")).unwrap();
        store.set_task_state(&id, TaskState::Queued, TaskState::Running).unwrap();
        // legal transition but a stale from-state: the row-level guard must catch it
        let err = store
            .set_task_state(&id, TaskState::Queued, TaskState::Running)
            .unwrap_err();
        assert!(matches!(err, CoreError::TaskNotFound(_)));
    }

    #[test]
    fn event_kinds_roundtrip_through_json() {
        let store = Store::open_in_memory().unwrap();
        store
            .append_event(EventKind::WorkerLost, &"ses_x", serde_json::json!({"hint": "host died"}))
            .unwrap();
        let events = store.events_for("ses_x").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EventKind::WorkerLost);
        assert_eq!(events[0].payload["hint"], "host died");
    }
}
