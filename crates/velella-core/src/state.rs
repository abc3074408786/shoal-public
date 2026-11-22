use serde::{Deserialize, Serialize};
use std::fmt;

/// Normalized state of one agent session (a PTY driving one agent CLI).
///
/// Every driver signal — screen parsing, hooks events, structured protocols,
/// process exit — folds into this single machine. Structured signals only
/// accelerate it; the screen remains the arbiter of record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Spawning,
    Running,
    WaitingPermission,
    WaitingInput,
    Verifying,
    Done,
    Failed,
    Stopped,
    /// Worker fate uncertain (crash, host loss). Reconcile, never assume.
    Fenced,
}

impl SessionState {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionState::Spawning => "spawning",
            SessionState::Running => "running",
            SessionState::WaitingPermission => "waiting_permission",
            SessionState::WaitingInput => "waiting_input",
            SessionState::Verifying => "verifying",
            SessionState::Done => "done",
            SessionState::Failed => "failed",
            SessionState::Stopped => "stopped",
            SessionState::Fenced => "fenced",
        }
    }

    pub fn can_transition_to(self, next: SessionState) -> bool {
        use SessionState::*;
        matches!(
            (self, next),
            (Spawning, Running)
                | (Spawning, Failed)
                | (Spawning, Stopped)
                | (Running, WaitingPermission)
                | (Running, WaitingInput)
                | (Running, Verifying)
                | (Running, Done)
                | (Running, Failed)
                | (Running, Stopped)
                | (Running, Fenced)
                | (WaitingPermission, Running)
                | (WaitingPermission, Done)
                | (WaitingPermission, Failed)
                | (WaitingPermission, Stopped)
                | (WaitingPermission, Fenced)
                // Human-facing dialog swaps (approve -> next prompt) without
                // the agent ever going back to work.
                | (WaitingPermission, WaitingInput)
                | (WaitingInput, WaitingPermission)
                | (WaitingInput, Running)
                | (WaitingInput, Done)
                | (WaitingInput, Failed)
                | (WaitingInput, Stopped)
                | (WaitingInput, Fenced)
                | (Verifying, Running)
                | (Verifying, Done)
                | (Verifying, Failed)
                | (Verifying, Stopped)
                | (Fenced, Stopped)
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, SessionState::Done | SessionState::Failed | SessionState::Stopped)
    }
}

impl fmt::Display for SessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Normalized state of one task (the unit of delegation: worktree × agent ×
/// policy × budget × done-criteria).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Queued,
    Running,
    Verifying,
    InReview,
    Merged,
    Escalated,
    Failed,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskState::Queued => "queued",
            TaskState::Running => "running",
            TaskState::Verifying => "verifying",
            TaskState::InReview => "in_review",
            TaskState::Merged => "merged",
            TaskState::Escalated => "escalated",
            TaskState::Failed => "failed",
        }
    }

    /// Verifying → Running is the bounded feedback loop: verification failed
    /// and the failure is fed back to the agent. Escalated is the human gate.
    pub fn can_transition_to(self, next: TaskState) -> bool {
        use TaskState::*;
        matches!(
            (self, next),
            (Queued, Running)
                | (Running, Verifying)
                | (Running, Escalated)
                | (Running, Failed)
                | (Verifying, Running)
                | (Verifying, InReview)
                | (Verifying, Escalated)
                | (Verifying, Failed)
                | (InReview, Merged)
                | (InReview, Escalated)
                | (InReview, Failed)
                | (Escalated, Running)
                | (Escalated, InReview)
                | (Escalated, Failed)
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, TaskState::Merged | TaskState::Failed)
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_happy_path() {
        let path = [
            SessionState::Spawning,
            SessionState::Running,
            SessionState::WaitingPermission,
            SessionState::Running,
            SessionState::WaitingInput,
            SessionState::Running,
            SessionState::Done,
        ];
        for w in path.windows(2) {
            assert!(
                w[0].can_transition_to(w[1]),
                "{} -> {} must be legal",
                w[0],
                w[1]
            );
        }
        assert!(SessionState::Done.is_terminal());
    }

    #[test]
    fn agent_turn_cycle_is_legal() {
        // The classifier's bread and butter: running <-> waiting, then done.
        assert!(SessionState::Running.can_transition_to(SessionState::WaitingInput));
        assert!(SessionState::WaitingInput.can_transition_to(SessionState::Running));
    }

    #[test]
    fn session_fencing_semantics() {
        // Orderly, observed stop is legal; UNCERTAIN death must fence.
        assert!(SessionState::Running.can_transition_to(SessionState::Stopped));
        assert!(SessionState::Running.can_transition_to(SessionState::Fenced));
        // A fenced worker is reconciled, never assumed: Fenced -> Stopped only.
        assert!(SessionState::Fenced.can_transition_to(SessionState::Stopped));
        assert!(!SessionState::Fenced.can_transition_to(SessionState::Running));
    }

    #[test]
    fn session_illegal_shortcuts() {
        assert!(!SessionState::Spawning.can_transition_to(SessionState::Done));
        assert!(!SessionState::Done.can_transition_to(SessionState::Running));
        assert!(!SessionState::Failed.can_transition_to(SessionState::Running));
    }

    #[test]
    fn task_feedback_loop_and_gate() {
        for pair in [
            (TaskState::Queued, TaskState::Running),
            (TaskState::Running, TaskState::Verifying),
            // verify failed -> feed back to the agent
            (TaskState::Verifying, TaskState::Running),
            (TaskState::Running, TaskState::Verifying),
            (TaskState::Verifying, TaskState::InReview),
            (TaskState::InReview, TaskState::Merged),
        ] {
            assert!(pair.0.can_transition_to(pair.1), "{:?} -> {:?}", pair.0, pair.1);
        }
        // budget/gate escalation paths
        assert!(TaskState::Running.can_transition_to(TaskState::Escalated));
        assert!(TaskState::Verifying.can_transition_to(TaskState::Escalated));
        assert!(TaskState::Escalated.can_transition_to(TaskState::Running));
        assert!(TaskState::Merged.is_terminal());
        assert!(TaskState::Failed.is_terminal());
        assert!(!TaskState::Merged.can_transition_to(TaskState::Running));
    }

    #[test]
    fn serde_snake_case_roundtrip() {
        let s = serde_json::to_string(&TaskState::InReview).unwrap();
        assert_eq!(s, r#""in_review""#);
        let back: TaskState = serde_json::from_str(&s).unwrap();
        assert_eq!(back, TaskState::InReview);
    }
}
