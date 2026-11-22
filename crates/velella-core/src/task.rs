use serde::{Deserialize, Serialize};

/// A background agent task, defined as code in `.velella/tasks/*.yaml`.
///
/// This format is one of velella's public standards: committed to git,
/// reviewable, shareable, replayable in CI. Unknown fields are rejected so
/// typos can never silently change supervision semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskSpec {
    pub name: String,
    /// Agent adapter to drive, e.g. `claude-code`, `codex`.
    pub agent: String,
    /// Optional native CLI requests; absent values inherit the CLI defaults.
    #[serde(default, skip_serializing_if = "LaunchOptions::is_empty")]
    pub launch: LaunchOptions,
    /// Base git ref to branch the task worktree from.
    #[serde(default = "default_base")]
    pub base: String,
    /// `auto` = one git worktree per task (default).
    #[serde(default = "default_worktree")]
    pub worktree: String,
    /// Setup script making the worktree runnable (deps, DB, .env).
    #[serde(default)]
    pub env: Option<String>,
    /// The coding instruction handed to the agent as its first message.
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub policy: Policy,
    /// Machine-checkable completion criteria. The verifier runs these after
    /// the agent reports work; failures are fed back for bounded retries.
    #[serde(default)]
    pub done: Vec<DoneCheck>,
    #[serde(default)]
    pub verify: Verify,
    #[serde(default)]
    pub notify: Vec<String>,
    #[serde(default = "default_on_done")]
    pub on_done: OnDone,
}

fn default_base() -> String {
    "main".into()
}

fn default_worktree() -> String {
    "auto".into()
}

fn default_on_done() -> OnDone {
    OnDone::Review
}

/// Invocation-local requests, not a claim of model or account availability.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

impl LaunchOptions {
    pub fn is_empty(&self) -> bool {
        self.model.is_none() && self.effort.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[serde(default)]
    pub execution: crate::execution::Policy,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default)]
    pub budget: Budget,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            execution: Default::default(),
            permissions: Permissions::Ask,
            budget: Budget::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum Permissions {
    /// Everything sandboxed; risky ops auto-approved inside the sandbox.
    Sandbox,
    /// Default: risky operations raise a human gate.
    #[default]
    Ask,
    /// Allow all. Only sane inside a disposable container/worktree.
    Allow,
}


#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    /// Hard token budget for the whole task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    /// Wall-clock cap, human string such as `2h`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_clock: Option<String>,
}

/// Externally tagged so YAML reads `- run: pnpm test --run`.
///
/// serde_yaml 0.9 would demand an explicit YAML tag (`!Run …`) for enum
/// variants; the task format is a human-facing public standard, so the map
/// form is the contract and de/serialization is hand-rolled around it.
#[derive(Debug, Clone, PartialEq)]
pub enum DoneCheck {
    /// Run a command in the task worktree; exit 0 = pass.
    Run(String),
    /// Guardrail: fail verification if the diff touches more than N files.
    DiffMaxFiles(usize),
}

impl<'de> Deserialize<'de> for DoneCheck {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Run { run: String },
            DiffMaxFiles { diff_max_files: usize },
        }
        match Raw::deserialize(deserializer)? {
            Raw::Run { run } => Ok(DoneCheck::Run(run)),
            Raw::DiffMaxFiles { diff_max_files } => Ok(DoneCheck::DiffMaxFiles(diff_max_files)),
        }
    }
}

impl Serialize for DoneCheck {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            DoneCheck::Run(cmd) => map.serialize_entry("run", cmd)?,
            DoneCheck::DiffMaxFiles(n) => map.serialize_entry("diff_max_files", n)?,
        }
        map.end()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct Verify {
    /// Bounded feedback loop size: how many times a failed verification is
    /// fed back to the agent before escalating to a human gate.
    #[serde(default)]
    pub retries: u32,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnDone {
    /// Keep all work local until a person chooses a delivery action.
    Review,
    /// Open a PR against `base`.
    Pr,
    /// Merge locally into `base`.
    LocalMerge,
}

impl TaskSpec {
    pub fn from_yaml_str(yaml: &str) -> Result<Self, crate::CoreError> {
        Ok(serde_yaml::from_str(yaml)?)
    }

    pub fn to_yaml(&self) -> Result<String, crate::CoreError> {
        Ok(serde_yaml::to_string(self)?)
    }
}

/// Parse a human wall-clock cap like `90s`, `30m`, `2h`, `1h30m`.
pub fn parse_wall_clock(s: &str) -> Option<std::time::Duration> {
    let mut total = std::time::Duration::ZERO;
    let mut num = String::new();
    let mut any = false;
    for c in s.trim().chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() {
            num.push(c);
            continue;
        }
        match c {
            's' | 'm' | 'h' => {
                let n: u64 = num.parse().ok()?;
                num.clear();
                total += match c {
                    's' => std::time::Duration::from_secs(n),
                    'm' => std::time::Duration::from_secs(n * 60),
                    _ => std::time::Duration::from_secs(n * 3600),
                };
                any = true;
            }
            ' ' => {
                if !num.is_empty() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    any.then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_policy_survives_yaml_and_legacy_defaults_are_native() {
        let task = TaskSpec::from_yaml_str("name: isolated\nagent: codex\npolicy:\n  execution:\n    mode: isolated\n    network: denied\n    read_only: [/opt/toolchain]\n").unwrap();
        assert!(task.policy.execution.is_isolated());
        assert_eq!(TaskSpec::from_yaml_str(&task.to_yaml().unwrap()).unwrap(), task);
        let legacy = TaskSpec::from_yaml_str("name: legacy\nagent: codex\n").unwrap();
        assert!(!legacy.policy.execution.is_isolated());
        assert!(TaskSpec::from_yaml_str("name: typo\nagent: codex\npolicy:\n  execution:\n    mode: isolated\n    netwrok: host\n").is_err());
    }

    const EXAMPLE: &str = "\
name: upgrade-vite-6
agent: claude-code
base: main
worktree: auto
env: .velella/env/setup.sh
policy:
  permissions: ask
  budget:
    tokens: 5000000
    wall_clock: 2h
done:
  - run: pnpm test --run
  - run: pnpm build
  - diff_max_files: 30
verify:
  retries: 2
notify: [desktop, slack]
on_done: pr
";

    #[test]
    fn parses_the_reference_example() {
        let t = TaskSpec::from_yaml_str(EXAMPLE).unwrap();
        assert_eq!(t.name, "upgrade-vite-6");
        assert_eq!(t.agent, "claude-code");
        assert_eq!(t.base, "main");
        assert_eq!(t.env.as_deref(), Some(".velella/env/setup.sh"));
        assert_eq!(t.policy.permissions, Permissions::Ask);
        assert_eq!(t.policy.budget.tokens, Some(5_000_000));
        assert_eq!(t.policy.budget.wall_clock.as_deref(), Some("2h"));
        assert_eq!(
            t.done,
            vec![
                DoneCheck::Run("pnpm test --run".into()),
                DoneCheck::Run("pnpm build".into()),
                DoneCheck::DiffMaxFiles(30),
            ]
        );
        assert_eq!(t.verify.retries, 2);
        assert_eq!(t.notify, vec!["desktop".to_string(), "slack".to_string()]);
        assert_eq!(t.on_done, OnDone::Pr);
    }

    #[test]
    fn defaults_fill_in() {
        let t = TaskSpec::from_yaml_str("name: t\nagent: claude-code\n").unwrap();
        assert_eq!(t.base, "main");
        assert_eq!(t.worktree, "auto");
        assert_eq!(t.policy.permissions, Permissions::Ask);
        assert_eq!(t.verify.retries, 0);
        assert_eq!(t.on_done, OnDone::Review);
        assert!(t.done.is_empty());
        assert!(t.env.is_none());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let err = TaskSpec::from_yaml_str("name: t\nagent: claude-code\nbogus: 1\n");
        assert!(err.is_err(), "typos must not silently change semantics");
    }

    #[test]
    fn bad_permissions_rejected() {
        let err = TaskSpec::from_yaml_str("name: t\nagent: a\npolicy:\n  permissions: yolo\n");
        assert!(err.is_err());
    }

    #[test]
    fn yaml_roundtrip() {
        let t = TaskSpec::from_yaml_str(EXAMPLE).unwrap();
        let s = t.to_yaml().unwrap();
        let t2 = TaskSpec::from_yaml_str(&s).unwrap();
        assert_eq!(t, t2);
    }

    #[test]
    fn instruction_is_optional_but_parsed() {
        let t = TaskSpec::from_yaml_str("name: t\nagent: a\ninstruction: |\n  do the thing\n").unwrap();
        assert_eq!(t.instruction.as_deref(), Some("do the thing\n"));
    }

    #[test]
    fn launch_options_yaml_roundtrip() {
        let task = TaskSpec::from_yaml_str(
            "name: tuned\nagent: codex\nlaunch:\n  model: custom/model-v2\n  effort: xhigh\n",
        ).unwrap();
        let value = serde_json::to_value(&task).unwrap();
        assert_eq!(value["launch"], serde_json::json!({"model":"custom/model-v2","effort":"xhigh"}));
        assert_eq!(TaskSpec::from_yaml_str(&task.to_yaml().unwrap()).unwrap(), task);
    }

    #[test]
    fn launch_options_preserve_defaults_and_omit_absent_values() {
        let legacy = TaskSpec::from_yaml_str("name: legacy\nagent: codex\n").unwrap();
        for launch in ["", "launch: {}\n", "launch:\n  model: null\n  effort: null\n"] {
            let task = TaskSpec::from_yaml_str(&format!("name: legacy\nagent: codex\n{launch}")).unwrap();
            assert_eq!(task, legacy);
            assert!(!task.to_yaml().unwrap().contains("launch:"));
            assert_eq!(TaskSpec::from_yaml_str(&task.to_yaml().unwrap()).unwrap(), legacy);
        }
        for (yaml, expected) in [
            ("  model: custom/model\n", serde_json::json!({"model":"custom/model"})),
            ("  effort: high\n", serde_json::json!({"effort":"high"})),
        ] {
            let task = TaskSpec::from_yaml_str(&format!("name: t\nagent: claude-code\nlaunch:\n{yaml}")).unwrap();
            assert_eq!(serde_json::to_value(&task).unwrap()["launch"], expected);
            assert_eq!(TaskSpec::from_yaml_str(&task.to_yaml().unwrap()).unwrap(), task);
        }
    }

    #[test]
    fn launch_options_reject_unknown_fields_and_wrong_types() {
        for launch in ["{modle: custom}", "{efort: high}", "{model: [custom]}", "{effort: {value: high}}", "[]"] {
            assert!(TaskSpec::from_yaml_str(&format!("name: t\nagent: codex\nlaunch: {launch}\n")).is_err());
        }
    }

    #[test]
    fn wall_clock_parsing() {
        use std::time::Duration;
        assert_eq!(parse_wall_clock("2h"), Some(Duration::from_secs(7200)));
        assert_eq!(parse_wall_clock("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(parse_wall_clock("90s"), Some(Duration::from_secs(90)));
        assert_eq!(parse_wall_clock("1h30m"), Some(Duration::from_secs(5400)));
        assert_eq!(parse_wall_clock("abc"), None);
        assert_eq!(parse_wall_clock("h"), None);
    }
}
