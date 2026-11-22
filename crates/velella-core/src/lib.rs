//! velella-core: domain types for the velella agent task runtime.
//!
//! Everything that is true about velella regardless of UI lives here: the
//! `.velella` task format, the normalized state machines both the screen
//! parser and structured protocols fold into, and the append-only event log
//! that UIs, notifications, automations and DAG orchestration all consume.

pub mod config;
pub mod execution;
pub mod error;
pub mod event;
pub mod ids;
pub mod state;
pub mod store;
pub mod task;

pub use error::CoreError;
pub use state::{SessionState, TaskState};
