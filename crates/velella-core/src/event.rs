use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One entry of the append-only log. Every consumer — UI, notifications,
/// automations, DAG orchestration — subscribes to the same stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub seq: i64,
    pub id: String,
    pub ts: DateTime<Utc>,
    pub kind: EventKind,
    /// Subject reference: a task id, a session id, or a task name.
    pub subject: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    TaskQueued,
    TaskStateChanged,
    SessionSpawned,
    SessionStateChanged,
    PermissionRequested,
    PermissionResolved,
    MessageQueued,
    VerifyStarted,
    VerifyPassed,
    VerifyFailed,
    GateCreated,
    GateResolved,
    WorkerLost,
    BudgetExceeded,
    TaskCompleted,
    TaskFailed,
}
