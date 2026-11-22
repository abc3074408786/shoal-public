//! Compatibility suite — the moat, engineered.
//!
//! Two halves:
//! 1. **Fixture replay** (every `cargo test`, every CI push): real screens
//!    recorded from agent CLIs live in `crates/velella/fixtures/`, replayed
//!    through the classifier with asserted verdicts. Agent ships a new
//!    screen → update or add a fixture, CI tells you if the patterns held.
//! 2. **Live probes** (`velella compat`): when real CLIs are installed,
//!    drive them for real — PTY round-trip, claude boot, controlled
//!    instruction submit (gated behind VELELLA_COMPAT_LIVE=1, it costs
//!    tokens and needs API access).

use crate::adapter::{self, strip_ansi};
use crate::sessions::{contains_dsr, SessionManager, SpawnRequest};
use serde::Deserialize;
use std::time::{Duration, Instant};
use velella_core::state::SessionState;

// ---------------- fixture replay ----------------

#[derive(Debug, Deserialize)]
struct FixtureIndex {
    fixtures: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    file: String,
    adapter: String,
    idle_secs: u64,
    /// waiting_permission | waiting_input | none
    expect: String,
    #[serde(default)]
    contains_dsr: bool,
}

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn load_fixtures() -> anyhow::Result<Vec<(Fixture, String)>> {
    let index: FixtureIndex =
        serde_yaml::from_str(&std::fs::read_to_string(fixtures_dir().join("index.yaml"))?)?;
    index
        .fixtures
        .into_iter()
        .map(|fx| {
            let bytes = std::fs::read(fixtures_dir().join(&fx.file))?;
            Ok((fx, String::from_utf8_lossy(&bytes).into_owned()))
        })
        .collect()
}

/// Replay every fixture through its adapter. Returns failure lines; an
/// empty vec means the whole suite held.
pub fn replay_fixtures() -> anyhow::Result<Vec<String>> {
    let mut failures = Vec::new();
    for (fx, raw) in load_fixtures()? {
        if fx.contains_dsr && !contains_dsr(raw.as_bytes()) {
            failures.push(format!("{}: expected a DSR query in the stream", fx.file));
        }
        let Some(spec) = adapter::lookup(&fx.adapter) else {
            failures.push(format!("{}: unknown adapter `{}`", fx.file, fx.adapter));
            continue;
        };
        let got = spec.classify(&strip_ansi(&raw), fx.idle_secs);
        let want = match fx.expect.as_str() {
            "waiting_permission" => Some(SessionState::WaitingPermission),
            "waiting_input" => Some(SessionState::WaitingInput),
            _ => None,
        };
        if got != want {
            failures.push(format!(
                "{}: expected {:?}, got {:?} — agent screen drifted, update the fixture or the patterns",
                fx.file, want, got
            ));
        }
    }
    Ok(failures)
}

// ---------------- live probes ----------------

#[derive(Debug, PartialEq)]
enum Status {
    Pass,
    Fail,
    Skip,
}

struct Outcome {
    name: &'static str,
    status: Status,
    detail: String,
    secs: u64,
}

impl Outcome {
    fn pass(name: &'static str, detail: impl Into<String>, secs: u64) -> Self {
        Self { name, status: Status::Pass, detail: detail.into(), secs }
    }
    fn fail(name: &'static str, detail: impl Into<String>, secs: u64) -> Self {
        Self { name, status: Status::Fail, detail: detail.into(), secs }
    }
    fn skip(name: &'static str, detail: impl Into<String>) -> Self {
        Self { name, status: Status::Skip, detail: detail.into(), secs: 0 }
    }
}

fn wait_terminal(mgr: &mut SessionManager, id: &str, secs: u64) -> Option<(SessionState, Option<i32>, String)> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if let Some(session) = mgr.get(id) {
            let snap = session.snapshot();
            if snap.state.is_terminal() {
                let (_, _, _, data) = session.output_since(0);
                return Some((snap.state, snap.exit, data));
            }
        } else {
            return None;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    None
}

fn check_pty_echo(mgr: &mut SessionManager) -> Outcome {
    let t0 = Instant::now();
    let marker = format!("vel-compat-{}", std::process::id());
    #[cfg(windows)]
    let req = SpawnRequest { sandbox: Default::default(),
        program: "cmd".into(),
        args: vec!["/C".into(), "echo".into(), marker.clone()],
        cwd: None,
        env: Default::default(),
        adapter: None,
    };
    #[cfg(not(windows))]
    let req = SpawnRequest { sandbox: Default::default(),
        program: "sh".into(),
        args: vec!["-c".into(), format!("echo {marker}")],
        cwd: None,
        env: Default::default(),
        adapter: None,
    };
    let Ok(id) = mgr.spawn(&req) else {
        return Outcome::fail("pty-echo", "spawn failed", t0.elapsed().as_secs());
    };
    match wait_terminal(mgr, &id.0, 20) {
        Some((SessionState::Done, Some(0), out)) if out.contains(&marker) => {
            Outcome::pass("pty-echo", "round-trip ok", t0.elapsed().as_secs())
        }
        Some((state, exit, _)) => Outcome::fail(
            "pty-echo",
            format!("state {state}, exit {exit:?}"),
            t0.elapsed().as_secs(),
        ),
        other => Outcome::fail("pty-echo", format!("timeout/lost: {other:?}"), t0.elapsed().as_secs()),
    }
}

fn check_claude_version(mgr: &mut SessionManager) -> Outcome {
    let t0 = Instant::now();
    let req = SpawnRequest { sandbox: Default::default(),
        program: "claude".into(),
        args: vec!["--version".into()],
        cwd: None,
        env: Default::default(),
        adapter: None,
    };
    let id = match mgr.spawn(&req) {
        Ok(id) => id,
        Err(_) => return Outcome::skip("claude-version", "claude not on PATH"),
    };
    match wait_terminal(mgr, &id.0, 60) {
        Some((SessionState::Done, Some(0), out)) if strip_ansi(&out).contains("Claude Code") => {
            let v = strip_ansi(&out)
                .lines()
                .find(|l| l.contains("Claude Code"))
                .unwrap_or("")
                .trim()
                .to_string();
            Outcome::pass("claude-version", v, t0.elapsed().as_secs())
        }
        Some((state, exit, out)) => Outcome::fail(
            "claude-version",
            format!("state {state}, exit {exit:?}, tail: {}", last_line(&out)),
            t0.elapsed().as_secs(),
        ),
        None => Outcome::fail("claude-version", "timeout", t0.elapsed().as_secs()),
    }
}

fn check_claude_boot(mgr: &mut SessionManager) -> Outcome {
    let t0 = Instant::now();
    let dir = std::env::temp_dir().join(format!("vel-compat-boot-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let req = SpawnRequest { sandbox: Default::default(),
        program: "claude".into(),
        args: vec![],
        cwd: Some(dir.to_string_lossy().into_owned()),
        env: Default::default(),
        adapter: Some("claude-code".into()),
    };
    let Ok(id) = mgr.spawn(&req) else {
        return Outcome::skip("claude-boot", "claude not on PATH");
    };
    // compat runs without the daemon's supervisor — classify in-place
    let spec = adapter::lookup("claude-code").expect("claude-code builtin");
    let _verdict = String::new();
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut result = Outcome::fail("claude-boot", "timeout before a classified screen", 120);
    while Instant::now() < deadline {
        if let Some(session) = mgr.get(&id.0) {
            let snap = session.snapshot();
            if snap.state.is_terminal() {
                result = Outcome::fail(
                    "claude-boot",
                    format!("agent exited: {}", snap.state),
                    t0.elapsed().as_secs(),
                );
                break;
            }
            let tail = strip_ansi(&session.recent_text(8192));
            match spec.classify(&tail, snap.idle_secs) {
                Some(SessionState::WaitingPermission) => {
                    result = Outcome::pass(
                        "claude-boot",
                        "trust dialog (waiting_permission)",
                        t0.elapsed().as_secs(),
                    );
                    break;
                }
                Some(SessionState::WaitingInput) => {
                    result = Outcome::pass(
                        "claude-boot",
                        "main prompt (waiting_input)",
                        t0.elapsed().as_secs(),
                    );
                    break;
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    if let Some(session) = mgr.get(&id.0) {
        let _ = session.kill();
    }
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// The heavyweight: real instruction, real tokens, real API access.
/// Opt-in via VELELLA_COMPAT_LIVE=1.
fn check_instruction_submit(mgr: &mut SessionManager) -> Outcome {
    if std::env::var("VELELLA_COMPAT_LIVE").as_deref() != Ok("1") {
        return Outcome::skip("instruction-submit", "set VELELLA_COMPAT_LIVE=1 (burns tokens; needs API access)");
    }
    let t0 = Instant::now();
    let dir = std::env::temp_dir().join(format!("vel-compat-live-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let req = SpawnRequest { sandbox: Default::default(),
        program: "claude".into(),
        args: vec![],
        cwd: Some(dir.to_string_lossy().into_owned()),
        env: Default::default(),
        adapter: Some("claude-code".into()),
    };
    let Ok(id) = mgr.spawn(&req) else {
        return Outcome::skip("instruction-submit", "claude not on PATH");
    };
    // answer trust dialogs, wait for a settled prompt — classify in-place
    // (no supervisor in the compat process)
    let spec = adapter::lookup("claude-code").expect("claude-code builtin");
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut settled = false;
    while Instant::now() < deadline {
        if let Some(session) = mgr.get(&id.0) {
            let snap = session.snapshot();
            if snap.state.is_terminal() {
                return Outcome::fail("instruction-submit", format!("agent exited: {}", snap.state), t0.elapsed().as_secs());
            }
            let tail = strip_ansi(&session.recent_text(8192));
            if spec.classify(&tail, snap.idle_secs) == Some(SessionState::WaitingPermission) {
                let _ = session.send_text("1", true);
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            if spec.classify(&tail, snap.idle_secs) == Some(SessionState::WaitingInput)
                && snap.idle_secs >= 8
            {
                settled = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    if !settled {
        if let Some(s) = mgr.get(&id.0) {
            let _ = s.kill();
        }
        return Outcome::fail("instruction-submit", "no settled prompt in 180s", t0.elapsed().as_secs());
    }
    let Some(session) = mgr.get(&id.0) else {
        return Outcome::fail("instruction-submit", "session lost", t0.elapsed().as_secs());
    };
    let before = session.snapshot().bytes;
    let _ = session.send_text("Reply with exactly one word: pong", false);
    std::thread::sleep(Duration::from_millis(400));
    let _ = session.send_text("", true);
    let mut pong = false;
    while Instant::now() < t0 + Duration::from_secs(240) {
        if let Some(session) = mgr.get(&id.0) {
            let (_, _, end, data) = session.output_since(before);
            if data.contains("pong") {
                pong = true;
                break;
            }
            let _ = end;
            if session.snapshot().state.is_terminal() {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(700));
    }
    if let Some(s) = mgr.get(&id.0) {
        let _ = s.kill();
    }
    let _ = std::fs::remove_dir_all(&dir);
    if pong {
        Outcome::pass("instruction-submit", "agent replied", t0.elapsed().as_secs())
    } else {
        Outcome::fail("instruction-submit", "no reply with pong", t0.elapsed().as_secs())
    }
}

fn last_line(s: &str) -> String {
    s.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
}

/// Run all live probes; prints a table and returns the process exit code.
pub fn run_live() -> anyhow::Result<i32> {
    let mut mgr = SessionManager::default();
    let results = vec![
        check_pty_echo(&mut mgr),
        check_claude_version(&mut mgr),
        check_claude_boot(&mut mgr),
        check_instruction_submit(&mut mgr),
    ];
    println!("velella compat");
    let (mut pass, mut fail, mut skip) = (0, 0, 0);
    for r in &results {
        let mark = match r.status {
            Status::Pass => {
                pass += 1;
                "✓"
            }
            Status::Fail => {
                fail += 1;
                "✗"
            }
            Status::Skip => {
                skip += 1;
                "-"
            }
        };
        let secs = if r.secs > 0 { format!("{:>5}s", r.secs) } else { "     ".to_string() };
        println!("  {mark} {:<20} {:>5}  {}", r.name, secs, r.detail);
    }
    println!("{pass} passed, {skip} skipped, {fail} failed");
    // fixture replay rides along on every live run
    match replay_fixtures() {
        Ok(f) if f.is_empty() => println!("  ✓ fixture replay       all patterns held"),
        Ok(f) => {
            fail += 1;
            for line in f {
                println!("  ✗ fixture: {line}");
            }
        }
        Err(e) => {
            fail += 1;
            println!("  ✗ fixture replay failed: {e}");
        }
    }
    Ok(i32::from(fail > 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_screens_classify_as_recorded() {
        let failures = replay_fixtures().expect("fixtures load");
        assert!(failures.is_empty(), "fixture drift:\n{}", failures.join("\n"));
    }

    #[test]
    fn dsr_detection() {
        assert!(contains_dsr(b"\x1b[6n"));
        assert!(contains_dsr(b"banner\r\n\x1b[6n rest"));
        assert!(!contains_dsr(b"nothing here"));
        assert!(!contains_dsr(b"\x1b[61n lookalike"));
    }
}
