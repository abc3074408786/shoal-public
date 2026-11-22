//! Agent adapters: data-driven screen patterns that fold raw PTY output
//! into the normalized SessionState.
//!
//! Agent CLIs ship weekly and their screens drift, so the patterns live in
//! data, not code: built-ins ship with the binary, and `.velella/adapters/
//! <name>.yaml` overrides them without a rebuild (the compat test-suite
//! keeps them honest). Structured signals (hooks, ACP) accelerate this
//! later; the screen remains the arbiter of record.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;
use velella_core::state::SessionState;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterSpec {
    pub name: String,
    /// Output quiescence (seconds) required before a prompt pattern is
    /// trusted — screens that just scrolled must not look like prompts.
    #[serde(default = "default_quiet")]
    pub quiet_secs: u64,
    /// Patterns meaning the agent is actively working; checked first so an
    /// always-visible prompt (e.g. claude-code's input box) never reads as
    /// idle while the spinner line is on screen.
    #[serde(default)]
    pub running: Vec<String>,
    /// Patterns meaning the agent is asking permission.
    #[serde(default)]
    pub waiting_permission: Vec<String>,
    /// Patterns meaning the agent sits at its prompt awaiting input.
    #[serde(default)]
    pub waiting_input: Vec<String>,
    /// How to launch this agent for a task. The adapter owns this knowledge:
    /// on Windows npm CLIs are shim scripts CreateProcess cannot execute
    /// directly, so the daemon falls back through the shell itself.
    #[serde(default)]
    pub launch: Option<LaunchSpec>,
    /// Settled-quiet seconds the runner demands before trusting a
    /// waiting_input during task boot: agents render their UI in frames with
    /// mid-boot pauses, and a half-drawn screen must not read as ready.
    #[serde(default = "default_boot_settle")]
    pub boot_settle_secs: u64,
    /// Known one-time boot dialogs and their keystroke answers (e.g. the
    /// folder-trust check). The runner answers these during boot so a task
    /// can run unattended; anything else waits for a human.
    #[serde(default)]
    pub boot: Vec<BootAnswer>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootAnswer {
    /// Pattern (whitespace-insensitive) identifying the dialog.
    pub when: String,
    /// Keystrokes to send when it is on screen.
    pub send: String,
}

/// Whitespace-insensitive containment, the matching primitive for screens
/// drawn with cursor repositioning.
pub fn normalize_contains(hay: &str, needle: &str) -> bool {
    normalize(hay).contains(&normalize(needle))
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchSpec {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

fn default_quiet() -> u64 {
    4
}

fn default_boot_settle() -> u64 {
    8
}

impl AdapterSpec {
    /// Suggest a state from the ANSI-stripped screen tail. `None` keeps the
    /// current state; the state machine still guards every application.
    ///
    /// Matching is whitespace-insensitive on both sides: agents draw their
    /// screens with cursor repositioning, so the linear byte stream has the
    /// spaces eaten out of prompts ("❯1.Yes,Itrust…"). Patterns are written
    /// naturally and normalized the same way.
    pub fn classify(&self, tail: &str, idle_secs: u64) -> Option<SessionState> {
        if idle_secs < self.quiet_secs {
            return None; // output still moving — no verdict
        }
        let tail = normalize(tail);
        if self.running.iter().any(|p| tail.contains(&normalize(p))) {
            return None;
        }
        if self
            .waiting_permission
            .iter()
            .any(|p| tail.contains(&normalize(p)))
        {
            return Some(SessionState::WaitingPermission);
        }
        if self.waiting_input.iter().any(|p| tail.contains(&normalize(p))) {
            return Some(SessionState::WaitingInput);
        }
        None
    }
}

fn normalize(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Remove ANSI CSI/OSC sequences, keeping printable content (prompts, box
/// text, spinners) for pattern matching.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if ('\x40'..='\x7e').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if n == '\x07' {
                            break;
                        }
                        if n == '\x1b' {
                            chars.next(); // ST: ESC \
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        out.push(c);
    }
    out
}

const CLAUDE_CODE: &str = "\
name: claude-code
quiet_secs: 4
launch:
  program: claude
boot:
  - when: 'I trust this folder'
    send: '1'
  - when: 'one you trust?'
    send: '1'
running:
  - 'esc to interrupt'
waiting_permission:
  - 'Do you want to proceed?'
  - 'Would you like to proceed?'
  - '❯ 1. Yes'
  - 'one you trust?'
  - 'I trust this folder'
waiting_input:
  - '❯'
";

// Advisory boot gates only. Completion remains driven by structured hooks.
const CODEX: &str = "\
name: codex
waiting_permission:
  - 'Do you trust the contents of this directory?'
  - 'Hooks need review'
";

const GENERIC: &str = "\
name: generic
";

fn builtins() -> &'static HashMap<String, AdapterSpec> {
    static BUILTINS: OnceLock<HashMap<String, AdapterSpec>> = OnceLock::new();
    BUILTINS.get_or_init(|| {
        let mut m = HashMap::new();
        for yaml in [CLAUDE_CODE, CODEX, GENERIC] {
            let spec: AdapterSpec = serde_yaml::from_str(yaml)
                .expect("builtin adapter spec must parse");
            m.insert(spec.name.clone(), spec);
        }
        m
    })
}

/// Lookup: `.velella/adapters/<name>.yaml` overrides the built-in — agent
/// screens drift weekly and overrides must not wait for a release.
pub fn lookup(name: &str) -> Option<AdapterSpec> {
    let path = std::path::Path::new(".velella/adapters").join(format!("{name}.yaml"));
    if let Ok(yaml) = std::fs::read_to_string(&path) {
        return serde_yaml::from_str(&yaml)
            .map_err(|e| eprintln!("[velella:adapter] bad spec {path:?}: {e}"))
            .ok();
    }
    builtins().get(name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude() -> AdapterSpec {
        lookup("claude-code").unwrap()
    }

    #[test]
    fn builtin_specs_parse() {
        let c = claude();
        assert_eq!(c.quiet_secs, 4);
        assert!(c.waiting_permission.contains(&"Do you want to proceed?".to_string()));
        assert_eq!(lookup("generic").unwrap().waiting_input.len(), 0);
    }

    #[test]
    fn codex_boot_trust_requires_attention_but_never_implies_completion() {
        let spec = lookup("codex").unwrap();
        for screen in [
            "Do\x1b[6;6Hyou\x1b[6;10Htrust the contents of this directory? › 1. Yes, continue",
            "Hooks need review 7 hooks are new or changed. 1. Review hooks 2. Trust all and continue",
        ] {
            let text = strip_ansi(screen);
            assert_eq!(spec.classify(&text, 10), Some(SessionState::WaitingPermission));
            assert_eq!(spec.classify(&text, 0), None);
        }
        assert!(spec.boot.is_empty(), "trust choices must remain explicit");
        assert_eq!(spec.classify("Ask Codex to do anything", 10), None);
        assert_eq!(spec.classify("Task complete", 10), None);
    }

    #[test]
    fn strip_ansi_keeps_content_drops_sequences() {
        let raw = "\x1b]0;title\x07C:\\> \x1b[?25l\x1b[7;1H❯ \x1b[?25h";
        assert_eq!(strip_ansi(raw), "C:\\> ❯ ");
    }

    #[test]
    fn permission_prompt_beats_always_visible_input_prompt() {
        let spec = claude();
        let screen = "\u{276f} 1. Yes\n  2. Yes, and don't ask again\n  3. No\nDo you want to proceed?";
        assert_eq!(
            spec.classify(&strip_ansi(screen), 10),
            Some(SessionState::WaitingPermission)
        );
    }

    #[test]
    fn settled_prompt_reads_as_waiting_input() {
        let spec = claude();
        let screen = "\u{276f} type here";
        assert_eq!(
            spec.classify(&strip_ansi(screen), 10),
            Some(SessionState::WaitingInput)
        );
    }

    #[test]
    fn spinner_line_means_still_running() {
        let spec = claude();
        // prompt box is on screen, but the spinner line is too
        let screen = "✻ Cogitating… (esc to interrupt · ↑ to edit)\n\u{276f}";
        assert_eq!(spec.classify(&strip_ansi(screen), 10), None);
    }

    #[test]
    fn fresh_output_is_never_classified() {
        let spec = claude();
        let screen = "\u{276f}";
        assert_eq!(spec.classify(&strip_ansi(screen), 1), None);
    }

    #[test]
    fn cursor_repositioned_screens_still_match() {
        // Real Claude Code trust dialog as it arrives over ConPTY: spaces
        // eaten by cursor moves, no separators.
        let spec = claude();
        let screen = "safetycheck:Isthisaprojectyoucreatedoroneyoutrust?❯1.Yes,Itrustthisfolder2.No,exit";
        assert_eq!(
            spec.classify(&strip_ansi(screen), 10),
            Some(SessionState::WaitingPermission)
        );
    }

    #[test]
    fn generic_adapter_classifies_nothing() {
        let g = lookup("generic").unwrap();
        assert_eq!(g.classify("anything at all", 999), None);
    }
}
