//! PTY session management: the daemon owns agent sessions, not the UI.
//!
//! A session outlives any client: `velella session spawn` hands the PTY to
//! the daemon, the CLI can exit, and the agent keeps running. Clients come
//! back to the same bytes through `session.output`, inject input through
//! `session.send`, and read the normalized state the classifier will feed.
//!
//! Idle detection is output-quiescence for now (`idle_secs`); screen-based
//! `tui-idle` classification (spinner/prompt patterns, structured hooks)
//! attaches here in the adapter iteration.

use anyhow::Context;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Instant;
use velella_core::ids::SessionId;
use velella_core::state::SessionState;
pub(crate) mod recording;
mod terminal_queries;

/// Capped byte buffer with absolute offsets, so clients can poll
/// incrementally (`since`) and learn when their window was truncated.
#[derive(Debug)]
pub struct OutputBuffer {
    start: u64,
    data: Vec<u8>,
    cap: usize,
}

impl OutputBuffer {
    pub fn new(cap: usize) -> Self {
        Self {
            start: 0,
            data: Vec::new(),
            cap,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
        let overflow = self.data.len().saturating_sub(self.cap);
        if overflow > 0 {
            self.data.drain(..overflow);
            self.start += overflow as u64;
        }
    }

    /// End offset: `start + len` = how many bytes have ever been produced.
    pub fn end(&self) -> u64 {
        self.start + self.data.len() as u64
    }

    /// Returns (actual_from, bytes). `from` clamps up to the buffer start
    /// when the caller's offset has been truncated away.
    pub fn read_since(&self, since: u64) -> (u64, &[u8]) {
        let from = since.max(self.start).min(self.end());
        let idx = (from - self.start) as usize;
        (from, &self.data[idx..])
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SpawnRequest {
    #[serde(default)]
    pub sandbox: crate::sandbox::Policy,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Adapter whose screen patterns classify this session's state.
    #[serde(default)]
    pub adapter: Option<String>,
}

#[derive(Debug)]
pub struct SessionSnapshot {
    pub id: SessionId,
    pub state: SessionState,
    pub program: String,
    pub exit: Option<i32>,
    pub idle_secs: u64,
    pub bytes: u64,
    pub started_at: chrono::DateTime<chrono::Utc>,
}

struct SessionInner {
    state: SessionState,
    exit: Option<i32>,
    buffer: OutputBuffer,
    last_output: Instant,
    killed: bool,
    /// Offset up to which the classifier has already rendered a verdict;
    /// classification is incremental over the byte stream (screen-emulator
    /// approximation) so stale dialogs never re-match.
    classified_until: u64,
}

pub struct Session {
    pub id: SessionId,
    pub program: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    adapter_name: String,
    inner: std::sync::Mutex<SessionInner>,
    writer: std::sync::Mutex<Box<dyn Write + Send>>,
    killer: std::sync::Mutex<Box<dyn ChildKiller + Send + Sync>>,
    #[allow(dead_code)] // kept alive: resize/pty queries arrive with the TUI
    master: std::sync::Mutex<Box<dyn MasterPty + Send>>,
    recording: std::sync::Mutex<Option<recording::Recording>>,
    #[cfg(windows)]
    group: crate::process::ChildGroup,
    #[cfg(target_os = "linux")]
    restricted_group: Option<crate::process::RestrictedGroup>,
    controller_cleanup: Option<std::sync::Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>>,
}

impl Session {
    /// Observation only: bind a native provider receipt to this PTY's tree.
    pub(crate) fn owns_process(&self,pid:u32)->bool {
        #[cfg(windows)] {self.group.owns(pid)}
        #[cfg(target_os="linux")] {
            let mut current=pid;
            for _ in 0..64 {let Ok(stat)=std::fs::read_to_string(format!("/proc/{current}/stat"))else{return false};let Some((_,fields))=stat.rsplit_once(") ")else{return false};let Some(parent)=fields.split_whitespace().nth(1).and_then(|s|s.parse::<u32>().ok())else{return false};if parent==std::process::id(){return true}if parent<=1||parent==current{return false}current=parent;}false
        }
        #[cfg(not(any(windows,target_os="linux")))] {let _=pid;false}
    }
    pub fn resize(&self, rows: u16, cols: u16) -> anyhow::Result<()> {
        anyhow::ensure!(rows > 0 && cols > 0 && rows <= 1000 && cols <= 1000, "invalid terminal size");
        // Older viewers can still send FitAddon's hidden-element minimum.
        if rows<=1 || cols<=2 {return Ok(());}
        // Serialize journal boundaries with output writes. A resize never
        // overtakes bytes already committed to terminal.bin.
        let mut recording=self.recording.lock().unwrap();
        self.master.lock().unwrap().resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        if let Some(recording)=recording.as_mut(){recording.resize(rows,cols,false)?;}
        Ok(())
    }

    pub fn raw_output(&self, since: u64) -> (u64, u64, Vec<u8>) {
        let g = self.inner.lock().unwrap();
        let (from, bytes) = g.buffer.read_since(since);
        (from, g.buffer.end(), bytes.to_vec())
    }
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        let g = self.inner.lock().unwrap();
        SessionSnapshot {
            id: self.id.clone(),
            state: g.state,
            program: self.program.clone(),
            exit: g.exit,
            idle_secs: g.last_output.elapsed().as_secs(),
            bytes: g.buffer.end(),
            started_at: self.started_at,
        }
    }

    pub fn output_since(&self, since: u64) -> (SessionState, u64, u64, String) {
        let g = self.inner.lock().unwrap();
        let (from, bytes) = g.buffer.read_since(since);
        let data = String::from_utf8_lossy(bytes).into_owned();
        (g.state, from, g.buffer.end(), data)
    }

    pub fn send_text(&self, text: &str, enter: bool) -> anyhow::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(text.as_bytes())?;
        if enter {
            // ConPTY and unix PTYs both translate \r into the line submit.
            writer.write_all(b"\r")?;
        }
        writer.flush()?;
        Ok(())
    }

    /// Recent output as text, for the classifier's screen tail.
    pub fn recent_text(&self, max_bytes: u64) -> String {
        let g = self.inner.lock().unwrap();
        let (_, bytes) = g.buffer.read_since(g.buffer.end().saturating_sub(max_bytes));
        String::from_utf8_lossy(bytes).into_owned()
    }

    /// Classifier bookmark: bytes before this offset already have a verdict.
    pub fn classified_until(&self) -> u64 {
        self.inner.lock().unwrap().classified_until
    }

    pub fn advance_classified(&self, end: u64) {
        self.inner.lock().unwrap().classified_until = end;
    }

    /// Apply a classifier suggestion — the state machine still guards it.
    /// Returns the applied state if a transition happened.
    pub fn apply_classified(&self, next: SessionState) -> Option<SessionState> {
        let mut g = self.inner.lock().unwrap();
        if g.state != next && g.state.can_transition_to(next) {
            g.state = next;
            Some(next)
        } else {
            None
        }
    }

    /// Orderly kill. The waiter thread archives the exit and moves the state
    /// to `stopped`; we never flip state from two places.
    pub fn kill(&self) -> anyhow::Result<()> {
        self.inner.lock().unwrap().killed = true;
        #[cfg(target_os = "linux")]
        if let Some(group) = &self.restricted_group {
            return if let Some(cleanup) = &self.controller_cleanup {
                group.kill_after(|| cleanup())
            } else { group.kill() };
        }
        #[cfg(windows)]
        self.group.kill()?;
        match self.killer.lock().unwrap().kill() {
            Ok(()) => Ok(()),
            // Windows reports a completed kill as os error 0 ("success").
            Err(e) if e.raw_os_error() == Some(0) => Ok(()),
            Err(e) => Err(anyhow::anyhow!("kill failed: {e}")),
        }
    }
}

#[derive(Default)]
pub struct SessionManager {
    sessions: HashMap<SessionId, std::sync::Arc<Session>>,
}

impl SessionManager {
    pub fn spawn(&mut self, req: &SpawnRequest) -> anyhow::Result<SessionId> {
        self.spawn_logged(req, None)
    }

    pub fn spawn_logged(&mut self, req: &SpawnRequest, log: Option<std::fs::File>) -> anyhow::Result<SessionId> {
        self.spawn_recorded(req,log.map(|file|recording::Recording::new(file,None)).transpose()?)
    }

    pub(crate) fn spawn_recorded(&mut self, req:&SpawnRequest, recording:Option<recording::Recording>)->anyhow::Result<SessionId> {
        let launch = crate::sandbox::prepare(&req.sandbox, &req.program, &req.args, req.cwd.as_deref(), &req.env)?;
        self.spawn_launch_recorded(req, &launch, recording, None)
    }

    /// A trusted model controller owns the PTY; its tools use the separate
    /// fixed executor. This entry is internal and requires an explicit adapter.
    pub(crate) fn spawn_controller_recorded(
        &mut self, req: &SpawnRequest, launch: &crate::sandbox::Launch,
        recording: Option<recording::Recording>,
        cleanup: std::sync::Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    ) -> anyhow::Result<SessionId> {
        anyhow::ensure!(req.sandbox.is_restricted()
            && req.adapter.as_deref() == Some("codex-restricted-terminal")
            && launch.clear_env, "invalid restricted terminal controller launch");
        self.spawn_launch_recorded(req, launch, recording, Some(cleanup))
    }

    fn spawn_launch_recorded(
        &mut self, req: &SpawnRequest, launch: &crate::sandbox::Launch,
        recording: Option<recording::Recording>,
        controller_cleanup: Option<std::sync::Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>>,
    ) -> anyhow::Result<SessionId> {
        // A lease follows the actual child, not Session/history or GUI lifetime.
        // Acquiring before spawn also releases automatically on every startup error.
        let power = crate::power_continuity::Lease::acquire().context("CLI AC power protection")?;
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty")?;

        let mut child = match pair.slave.spawn_command(build_launch_cmd(launch, false)?) {
            Ok(c) => c,
            Err(e) if cfg!(windows) => {
                // npm-style CLIs are .cmd shims CreateProcess cannot execute
                // directly; route through the shell like PATHEXT does.
                eprintln!("[velella:pty] direct spawn failed ({e}); retrying via cmd /C");
                pair.slave
                    .spawn_command(build_launch_cmd(launch, true)?)
                    .context("spawn command (direct and via cmd /C)")?
            }
            Err(e) => return Err(e.context("spawn command")),
        };
        #[cfg(windows)]
        let group = match child.process_id().ok_or_else(|| anyhow::anyhow!("child PID unavailable")).and_then(crate::process::ChildGroup::attach) {
            Ok(group) => group,
            Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error); }
        };
        #[cfg(target_os = "linux")]
        let restricted_group = if req.sandbox.is_restricted() {
            let attached = child.process_id().ok_or_else(|| anyhow::anyhow!("restricted child PID unavailable"))
                .and_then(crate::process::RestrictedGroup::attach);
            match attached {
                Ok(group) => Some(group),
                Err(error) => {
                    // The direct child has not been reaped, so its PID is still owned.
                    if let Some(pid) = child.process_id() { unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL); } }
                    let _ = child.wait();
                    return Err(error);
                }
            }
        } else { None };
        drop(pair.slave);

        let handles = (|| -> anyhow::Result<_> {
            Ok((pair.master.take_writer().context("take writer")?, pair.master.try_clone_reader().context("clone reader")?))
        })();
        let (writer, reader) = match handles {
            Ok(handles) => handles,
            Err(error) => {
                #[cfg(target_os = "linux")]
                if let Some(group) = &restricted_group {
                    let _ = group.kill_after(|| controller_cleanup.as_ref().map(|cleanup| cleanup()).unwrap_or(Ok(())));
                    let _ = group.wait_and_finish();
                    let _ = child.wait();
                }
                return Err(error);
            }
        };
        let killer = child.clone_killer();

        let id = SessionId::generate();
        let session = std::sync::Arc::new(Session {
            id: id.clone(),
            program: format!("{} {}", req.program, req.args.join(" ")),
            started_at: chrono::Utc::now(),
            adapter_name: req.adapter.clone().unwrap_or_else(|| "generic".into()),
            inner: std::sync::Mutex::new(SessionInner {
                state: SessionState::Running,
                exit: None,
                buffer: OutputBuffer::new(256 * 1024),
                last_output: Instant::now(),
                killed: false,
                classified_until: 0,
            }),
            writer: std::sync::Mutex::new(writer),
            killer: std::sync::Mutex::new(killer),
            master: std::sync::Mutex::new(pair.master),
            recording: std::sync::Mutex::new(recording),
            #[cfg(windows)]
            group,
            #[cfg(target_os = "linux")]
            restricted_group,
            controller_cleanup,
        });

        {
            // reader thread: PTY bytes -> buffer
            let session = std::sync::Arc::clone(&session);
            std::thread::spawn(move || pump_output(reader, session));
        }
        {
            // waiter thread: sole writer of terminal session states
            let session = std::sync::Arc::clone(&session);
            std::thread::spawn(move || reap_child(child, session, power));
        }

        self.sessions.insert(id.clone(), session);
        Ok(id)
    }

    pub fn get(&self, id: &str) -> Option<std::sync::Arc<Session>> {
        self.sessions.get(&SessionId(id.to_string())).cloned()
    }

    pub fn list(&self) -> Vec<SessionSnapshot> {
        let mut v: Vec<_> = self.sessions.values().map(|s| s.snapshot()).collect();
        v.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        v
    }

    /// Snapshot of handles for the supervisor loop.
    pub fn iter(&self) -> Vec<std::sync::Arc<Session>> {
        self.sessions.values().cloned().collect()
    }
}

fn build_cmd(req: &SpawnRequest, via_shell: bool) -> anyhow::Result<CommandBuilder> {
    let launch = crate::sandbox::prepare(&req.sandbox, &req.program, &req.args, req.cwd.as_deref(), &req.env)?;
    build_launch_cmd(&launch, via_shell)
}

fn build_launch_cmd(launch: &crate::sandbox::Launch, via_shell: bool) -> anyhow::Result<CommandBuilder> {
    anyhow::ensure!(!via_shell || !launch.clear_env, "isolated execution cannot fall back to a host shell");
    let req = launch;
    let mut cmd = if via_shell {
        let mut c = CommandBuilder::new("cmd");
        let mut argv = vec!["/C".to_string(), req.program.clone()];
        argv.extend(req.args.iter().cloned());
        c.args(argv.iter());
        c
    } else {
        let mut c = CommandBuilder::new(&req.program);
        c.args(req.args.iter());
        c
    };
    if let Some(cwd) = &req.cwd {
        cmd.cwd(PathBuf::from(cwd));
    }
    if req.clear_env { cmd.env_clear(); }
    // This child owns a real PTY rendered by xterm. A headless launcher may
    // carry NO_COLOR=1 / TERM=dumb; those describe the launcher, not this PTY.
    // Explicit per-session preferences below still take precedence.
    cmd.env_remove("NO_COLOR");
    cmd.env_remove("FORCE_COLOR");
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    for (k, v) in &req.env {
        cmd.env(k, v);
    }
    Ok(cmd)
}

/// ConPTY's conhost queries the cursor position at startup; console apps
/// stall before their first write until the terminal answers. Fixture-tested
/// in compat.rs — if you touch this, run `velella compat`.
pub(crate) const DSR_QUERY: &[u8] = b"\x1b[6n";
pub(crate) const DSR_REPLY: &[u8] = b"\x1b[1;1R";

pub(crate) fn contains_dsr(chunk: &[u8]) -> bool {
    chunk.windows(DSR_QUERY.len()).any(|w| w == DSR_QUERY)
}

fn pump_output(mut reader: Box<dyn Read + Send>, session: std::sync::Arc<Session>) {
    let mut buf = [0u8; 4096];
    let mut queries = terminal_queries::TerminalQueries::new(cfg!(windows));
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                eprintln!("[velella:pty] reader eof");
                break;
            }
            Ok(n) => {
                let chunk = &buf[..n];
                let logged={let mut recording=session.recording.lock().unwrap();
                    recording.as_mut().map(|log|log.write(chunk)).transpose()};
                if logged.is_err() {
                        let _ = session.kill();
                        break;
                }
                let replies = queries.replies(chunk);
                {
                    let mut g = session.inner.lock().unwrap();
                    g.buffer.push(chunk);
                    g.last_output = Instant::now();
                }
                if !replies.is_empty() {
                    if let Ok(mut w) = session.writer.lock() {
                        let _ = w.write_all(&replies);
                        let _ = w.flush();
                    }
                }
            }
            Err(e) => {
                eprintln!("[velella:pty] reader error: {e}");
                break;
            }
        }
    }
}

fn reap_child(
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
    session: std::sync::Arc<Session>,
    power: crate::power_continuity::Lease,
) {
    eprintln!("[velella:pty] waiting for child exit");
    #[cfg(target_os = "linux")]
    let cleanup_failed = session.restricted_group.as_ref().is_some_and(|group| {
        if let Err(error) = group.wait_and_finish_after(|| {
            session.controller_cleanup.as_ref().map(|cleanup| cleanup()).unwrap_or(Ok(()))
        }) {
            eprintln!("[velella:pty] restricted group cleanup failed: {error:#}"); true
        } else { false }
    });
    #[cfg(not(target_os = "linux"))]
    let cleanup_failed = false;
    let status = child.wait();
    drop(power); // history and the output reader may outlive execution
    eprintln!("[velella:pty] child exited: {status:?}");
    let mut g = session.inner.lock().unwrap();
    let code = if cleanup_failed { -1 } else { status
        .ok()
        .map(|s| s.exit_code() as i32)
        .unwrap_or(-1) };
    g.exit = Some(code);
    let target = if g.killed {
        SessionState::Stopped
    } else if code == 0 {
        SessionState::Done
    } else {
        SessionState::Failed
    };
    // The classifier may already have moved us on; only apply legal moves.
    if g.state.can_transition_to(target) {
        g.state = target;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn legacy_descriptors_default_to_native_and_bad_policy_is_rejected() {
        let legacy: SpawnRequest = serde_json::from_str(r#"{"program":"sh"}"#).unwrap();
        assert_eq!(legacy.sandbox, crate::sandbox::Policy::Native {});
        assert!(serde_json::from_str::<SpawnRequest>(r#"{"program":"sh","sandbox":{"mode":"isloated"}}"#).is_err());
    }

    #[test]
    fn invalid_isolation_never_launches_command() {
        let mut manager = SessionManager::default();
        let request: SpawnRequest = serde_json::from_str(r#"{"program":"does-not-matter","sandbox":{"mode":"isolated"}}"#).unwrap();
        assert!(manager.spawn(&request).is_err());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn unavailable_isolation_never_executes_a_real_pty_command() {
        if crate::sandbox::capabilities().available { return; }
        let root = std::env::temp_dir().join(format!("velella-pty-refusal-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let req = SpawnRequest {
            sandbox: crate::sandbox::Policy::Isolated {
                read_only: vec![], writable: vec![],
                network: crate::sandbox::Network::Denied, home: None,
            },
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "printf launched > must-not-exist".into()],
            cwd: Some(root.to_string_lossy().into_owned()),
            env: HashMap::new(), adapter: None,
        };
        let mut manager = SessionManager::default();
        let error = manager.spawn(&req).expect_err("unavailable isolation must refuse launch");
        assert!(format!("{error:#}").contains("refusing native fallback"));
        assert!(manager.list().is_empty());
        assert!(!root.join("must-not-exist").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn interactive_pty_has_color_capabilities() {
        let req = SpawnRequest { sandbox: Default::default(),
            program: "powershell".into(), args: vec![], cwd: None,
            env: HashMap::new(), adapter: None,
        };
        for via_shell in [false, true] {
            let cmd = build_cmd(&req, via_shell).unwrap();
            assert_eq!(cmd.get_env("TERM"), Some(std::ffi::OsStr::new("xterm-256color")));
            assert_eq!(cmd.get_env("COLORTERM"), Some(std::ffi::OsStr::new("truecolor")));
            assert_eq!(cmd.get_env("NO_COLOR"), None);
            assert_eq!(cmd.get_env("FORCE_COLOR"), None);
        }
    }

    #[test]
    fn explicit_session_color_preferences_are_preserved() {
        let req = SpawnRequest { sandbox: Default::default(),
            program: "powershell".into(), args: vec![], cwd: None,
            env: HashMap::from([("NO_COLOR".into(), "1".into()), ("TERM".into(), "vt100".into())]),
            adapter: None,
        };
        let cmd = build_cmd(&req, false).unwrap();
        assert_eq!(cmd.get_env("NO_COLOR"), Some(std::ffi::OsStr::new("1")));
        assert_eq!(cmd.get_env("TERM"), Some(std::ffi::OsStr::new("vt100")));
    }

    #[test]
    fn output_buffer_offsets_and_truncation() {
        let mut b = OutputBuffer::new(8);
        b.push(b"12345678");
        assert_eq!(b.end(), 8);
        let (from, data) = b.read_since(0);
        assert_eq!((from, data), (0, b"12345678".as_slice()));

        // push past cap: head is truncated, offsets stay absolute
        b.push(b"9012");
        assert_eq!(b.end(), 12);
        let (from, data) = b.read_since(0);
        assert_eq!(from, 4, "head truncated");
        assert_eq!(data, b"56789012");

        // since == end -> empty; since beyond end clamps to end
        assert_eq!(b.read_since(12).1, b"");
        assert_eq!(b.read_since(999).1, b"");
        let (from, data) = b.read_since(10);
        assert_eq!((from, data), (10, &b"12"[..]));
    }

    #[test]
    #[cfg(windows)]
    fn power_continuity_follows_processes_not_history_and_cleans_failed_spawn() {
        use crate::power_continuity::{TEST_LOCK, test_on_ac, test_requests};
        let _serial = TEST_LOCK.lock().unwrap();
        let expected = usize::from(test_on_ac());
        let mut manager = SessionManager::default();
        let request = |command: &str, adapter: &str| SpawnRequest {
            sandbox: Default::default(), program: "powershell".into(),
            args: vec!["-NoProfile".into(), "-Command".into(), command.into()],
            cwd: None, env: HashMap::new(), adapter: Some(adapter.into()),
        };
        let long = manager.spawn(&request("1..600 | ForEach-Object { Write-Output ('tick-' + $_); Start-Sleep -Milliseconds 100 }", "generic")).unwrap();
        let short = manager.spawn(&request("Write-Output short-power-fixture", "unlisted-test-cli")).unwrap();
        assert_eq!(test_requests(), expected * 2);
        let wait = |id: &str| {
            let session = manager.get(id).unwrap();
            let end = Instant::now() + Duration::from_secs(30);
            while session.snapshot().exit.is_none() && Instant::now() < end { std::thread::sleep(Duration::from_millis(10)); }
            assert!(session.snapshot().exit.is_some(), "fixture did not exit");
            session
        };
        let history = wait(&short.0);
        assert_eq!(history.snapshot().state, SessionState::Done);
        assert_eq!(test_requests(), expected, "completed history must not keep the machine awake");
        let running = manager.get(&long.0).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !running.recent_text(4096).contains("tick-2") && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(20)); }
        assert!(running.recent_text(4096).contains("tick-2"), "other CLI must continue producing output");
        running.kill().unwrap();
        assert_eq!(wait(&long.0).snapshot().state, SessionState::Stopped);
        assert_eq!(test_requests(), 0, "explicit stop must release protection");
        let mut bad = request("Write-Output must-not-launch", "generic");
        bad.program = std::env::temp_dir().join(format!("missing-power-fixture-{}.exe", uuid::Uuid::now_v7())).to_string_lossy().into_owned();
        let mut launch = crate::sandbox::prepare(&bad.sandbox, &bad.program, &bad.args, None, &bad.env).unwrap();
        // portable-pty falls back from nonexistent cwd on Windows; instead use a
        // nonexistent program with shell fallback prohibited by a clear-env launch.
        launch.clear_env = true;
        assert!(manager.spawn_launch_recorded(&bad, &launch, None, None).is_err());
        assert_eq!(test_requests(), 0, "failed startup must release protection");
        assert_eq!(manager.list().len(), 2, "history remains without holding protection");
    }

    #[test]
    fn spawn_echo_session_reaches_terminal_state() {
        let mut mgr = SessionManager::default();
        #[cfg(windows)]
        let req = SpawnRequest { sandbox: Default::default(),
            program: "powershell".into(),
            args: vec!["-NoProfile".into(), "-Command".into(), "Write-Output hello-velella".into()],
            cwd: None,
            env: HashMap::new(),
            adapter: None,
        };
        #[cfg(not(windows))]
        let req = SpawnRequest { sandbox: Default::default(),
            program: "sh".into(),
            args: vec!["-c".into(), "echo hello-velella".into()],
            cwd: None,
            env: HashMap::new(),
            adapter: None,
        };
        let id = mgr.spawn(&req).unwrap();

        let mut snap = mgr.get(&id.0).unwrap().snapshot();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !snap.state.is_terminal() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
            snap = mgr.get(&id.0).unwrap().snapshot();
        }
        let (_, _, _, data) = mgr.get(&id.0).unwrap().output_since(0);
        assert_eq!(
            snap.state,
            SessionState::Done,
            "must exit cleanly; output so far: {data:?}"
        );
        assert_eq!(snap.exit, Some(0));
        assert!(data.contains("hello-velella"), "output: {data:?}");
    }
}
