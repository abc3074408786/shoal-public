//! Platform-independent execution policy shared by tasks and sessions.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Policy {
    Native {},
    Isolated {
        #[serde(default)]
        read_only: Vec<PathBuf>,
        #[serde(default)]
        writable: Vec<PathBuf>,
        #[serde(default)]
        network: Network,
        /// Optional dedicated persistent HOME. Never the host user HOME.
        #[serde(default)]
        home: Option<PathBuf>,
    },
    /// Experimental Linux file-access and syscall restrictions. Unlike
    /// Isolated, this does not create mount or PID namespaces.
    Restricted {
        #[serde(default)]
        read_only: Vec<PathBuf>,
        #[serde(default)]
        writable: Vec<PathBuf>,
        #[serde(default)]
        network: Network,
        #[serde(default)]
        home: Option<PathBuf>,
    },
}

impl Policy {
    pub fn is_isolated(&self) -> bool {
        matches!(self, Self::Isolated { .. })
    }
    pub fn is_restricted(&self) -> bool {
        matches!(self, Self::Restricted { .. })
    }
    pub fn is_sandboxed(&self) -> bool {
        !matches!(self, Self::Native {})
    }
}

impl Default for Policy {
    fn default() -> Self {
        Self::Native {}
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Network {
    #[default]
    Denied,
    /// Full host networking, including localhost. This is not a domain allowlist.
    Host,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restricted_is_explicit_and_distinct_from_namespace_isolation() {
        let policy: Policy = serde_json::from_str(r#"{"mode":"restricted"}"#).unwrap();
        assert!(policy.is_restricted() && policy.is_sandboxed());
        assert!(!policy.is_isolated());
        assert!(matches!(policy, Policy::Restricted { network: Network::Denied, home: None, .. }));
        let encoded = serde_json::to_value(&policy).unwrap();
        assert_eq!(encoded["mode"], "restricted");
        assert!(serde_json::from_str::<Policy>(r#"{"mode":"restricted","netwrok":"host"}"#).is_err());
        assert!(!Policy::default().is_sandboxed());
    }
}
