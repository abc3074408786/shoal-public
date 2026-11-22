use crate::adapter;
use crate::notify;
use crate::runner::{self, Deps};
use crate::sessions::{SessionManager, SpawnRequest};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use velella_core::config::Config;
use velella_core::event::EventKind;
use velella_core::store::Store;

/// Headless runtime. One process owns the task store and every PTY session;
/// clients (CLI, TUI, CI) speak newline-delimited JSON, one request per
/// connection in this slice. A supervisor classifies sessions every second;
/// the runner executes queued tasks end to end.
pub fn run(listen: Option<String>, db: Option<PathBuf>) -> anyhow::Result<()> {
    let _ownership = velella::desktop::lock_project(&std::env::current_dir()?)?;
    let db = db.unwrap_or_else(|| PathBuf::from(".velella/state.db"));
    let config = load_config()?;
    let state = Arc::new(AppState {
        store: Arc::new(Mutex::new(Store::open(&db)?)),
        sessions: Arc::new(Mutex::new(SessionManager::default())),
        config,
    });

    let addr = match listen {
        Some(a) => a,
        None => state.config.listen.clone(),
    };
    let listener = TcpListener::bind(&addr)?;
    eprintln!("velella serve listening on {addr} (db: {})", db.display());

    {
        // supervisor: classifier -> legal transitions -> events -> notify
        let state = Arc::clone(&state);
        std::thread::spawn(move || supervise_loop(state));
    }
    {
        // runner: queued task -> worktree -> agent -> verified work
        let deps = Deps {
            store: Arc::clone(&state.store),
            sessions: Arc::clone(&state.sessions),
            config: state.config.clone(),
        };
        runner::spawn_thread(deps);
    }
    {
        // HTTP API: curl / phone shortcuts / webhooks
        let state = Arc::clone(&state);
        let addr = state.config.http_listen.clone();
        std::thread::spawn(move || {
            if let Err(e) = crate::httpapi::serve_loop(state, &addr) {
                eprintln!("[velella:http] {e}");
            }
        });
    }

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let state = Arc::clone(&state);
        std::thread::spawn(move || {
            let _ = handle_conn(stream, &state);
        });
    }
    Ok(())
}

pub(crate) struct AppState {
    pub(crate) store: Arc<Mutex<Store>>,
    pub(crate) sessions: Arc<Mutex<SessionManager>>,
    pub(crate) config: Config,
}

fn load_config() -> anyhow::Result<Config> {
    let path = Path::new(".velella/config.toml");
    if path.exists() {
        Ok(Config::parse(&std::fs::read_to_string(path)?)?)
    } else {
        Ok(Config::default())
    }
}

fn supervise_loop(state: Arc<AppState>) {
    let mut notified: HashSet<String> = HashSet::new();
    loop {
        std::thread::sleep(Duration::from_secs(1));
        supervise_once(&state, &mut notified);
    }
}

fn supervise_once(state: &AppState, notified: &mut HashSet<String>) {
    for session in state.sessions.lock().unwrap().iter() {
        let snap = session.snapshot();

        // Terminal states: notify once — a failed worker and a finished one
        // both deserve a ping (the "come review" moment).
        if snap.state.is_terminal() {
            if notified.insert(session.id.0.clone()) {
                notify::fire(&state.config, "session", &session.id.0, snap.state.as_str());
                let _ = state.store.lock().unwrap().append_event(
                    EventKind::SessionStateChanged,
                    &session.id,
                    serde_json::json!({ "state": snap.state.as_str(), "exit": snap.exit }),
                );
            }
            continue;
        }

        // Non-terminal: classifier suggests, the state machine disposes.
        // Classification is incremental: only bytes past the last verdict
        // are judged, so a dialog answered 10 seconds ago can't re-match.
        let Some(spec) = adapter::lookup(session.adapter_name()) else {
            continue;
        };
        if spec.waiting_permission.is_empty() && spec.waiting_input.is_empty() {
            continue;
        }
        let since = session.classified_until();
        let (_, _from, end, delta) = session.output_since(since);
        if end == since {
            continue; // no new bytes to judge
        }
        if let Some(suggested) = spec.classify(&adapter::strip_ansi(&delta), snap.idle_secs) {
            if suggested != snap.state {
                if let Some(applied) = session.apply_classified(suggested) {
                    session.advance_classified(end);
                    eprintln!("[velella:classify] {} {} -> {}", session.id, snap.state, applied);
                    let _ = state.store.lock().unwrap().append_event(
                        EventKind::SessionStateChanged,
                        &session.id,
                        serde_json::json!({
                            "from": snap.state.as_str(),
                            "to": applied.as_str(),
                            "by": "classifier",
                        }),
                    );
                    notify::fire(&state.config, "session", &session.id.0, applied.as_str());
                }
            }
        }
    }
}

fn handle_conn(stream: std::net::TcpStream, state: &AppState) -> std::io::Result<()> {
    let mut line = String::new();
    {
        let mut reader = BufReader::new(&stream);
        if reader.read_line(&mut line)? == 0 {
            return Ok(()); // probe with no payload
        }
    }
    let response = match serde_json::from_str::<serde_json::Value>(line.trim()) {
        Ok(req) => dispatch(req, state),
        Err(e) => err(format!("bad request: {e}")),
    };
    let mut writer = stream.try_clone()?;
    writeln!(writer, "{response}")?;
    Ok(())
}

fn err(msg: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "ok": false, "error": msg.into() })
}

pub(crate) fn dispatch(req: serde_json::Value, state: &AppState) -> serde_json::Value {
    let cmd = req.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
    match cmd {
        "status" => {
            let n = state.sessions.lock().unwrap().list().len();
            serde_json::json!({
                "ok": true,
                "service": "velella",
                "version": env!("CARGO_PKG_VERSION"),
                "sessions": n,
            })
        }
        "task.list" => {
            let store = state.store.lock().unwrap();
            match store.list_tasks() {
                Ok(tasks) => {
                    let items: Vec<serde_json::Value> = tasks
                        .iter()
                        .map(|t| {
                            serde_json::json!({
                                "id": t.id.0,
                                "name": t.name,
                                "state": t.state.as_str(),
                                "agent": t.spec.agent,
                                "pr_url": t.pr_url,
                            })
                        })
                        .collect();
                    serde_json::json!({ "ok": true, "tasks": items })
                }
                Err(e) => err(e.to_string()),
            }
        }
        "task.events" => {
            let Some(name) = req.get("name").and_then(|v| v.as_str()) else {
                return err("task.events needs { name }");
            };
            let store = state.store.lock().unwrap();
            let Some(task) = store.get_task(name).ok().flatten() else {
                return err(format!("task `{name}` not found"));
            };
            match store.events_for(&task.id.0) {
                Ok(events) => {
                    let items: Vec<serde_json::Value> = events
                        .iter()
                        .map(|e| {
                            serde_json::json!({
                                "seq": e.seq,
                                "kind": e.kind,
                                "payload": e.payload,
                                "ts": e.ts.to_rfc3339(),
                            })
                        })
                        .collect();
                    serde_json::json!({ "ok": true, "events": items })
                }
                Err(e) => err(e.to_string()),
            }
        }
        "sandbox.capabilities" => serde_json::json!({"ok":true,"sandbox":crate::sandbox::capabilities()}),
        "session.spawn" => {
            let parsed: Result<SpawnRequest, _> = serde_json::from_value(req.clone());
            let Ok(spawn) = parsed else {
                return err("session.spawn needs { program, args?, cwd?, env?, adapter? }");
            };
            // NOTE: bind outside `match` — a lock temporary in the scrutinee
            // lives until the match ends and would deadlock the arms.
            let spawned = state.sessions.lock().unwrap().spawn(&spawn);
            match spawned {
                Ok(id) => {
                    let state_str = state
                        .sessions
                        .lock()
                        .unwrap()
                        .get(&id.0)
                        .map(|s| s.snapshot().state.as_str())
                        .unwrap_or("?")
                        .to_string();
                    serde_json::json!({ "ok": true, "session_id": id.0, "state": state_str })
                }
                Err(e) => err(format!("spawn failed: {e}")),
            }
        }
        "session.list" => {
            let snaps = state.sessions.lock().unwrap().list();
            let items: Vec<serde_json::Value> = snaps
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "id": s.id.0,
                        "state": s.state.as_str(),
                        "program": s.program,
                        "exit": s.exit,
                        "idle_secs": s.idle_secs,
                        "bytes": s.bytes,
                        "started_at": s.started_at.to_rfc3339(),
                    })
                })
                .collect();
            serde_json::json!({ "ok": true, "sessions": items })
        }
        "session.output" => {
            let Some(id) = req.get("id").and_then(|v| v.as_str()) else {
                return err("session.output needs { id }");
            };
            let since = req.get("since").and_then(|v| v.as_u64()).unwrap_or(0);
            let Some(session) = state.sessions.lock().unwrap().get(id) else {
                return err(format!("session `{id}` not found"));
            };
            let snap = session.snapshot();
            let (state_, from, end, data) = session.output_since(since);
            let (_, _, bytes) = session.raw_output(since);
            use base64::Engine;
            serde_json::json!({
                "ok": true,
                "id": id,
                "state": state_.as_str(),
                "exit": snap.exit,
                "idle_secs": snap.idle_secs,
                "from": from,
                "end": end,
                "data": data,
                "data_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
            })
        }
        "session.resize" => {
            let Some(session) = req["id"].as_str().and_then(|id| state.sessions.lock().unwrap().get(id)) else { return err("session not found"); };
            let rows = req["rows"].as_u64().and_then(|v| u16::try_from(v).ok()).unwrap_or(0);
            let cols = req["cols"].as_u64().and_then(|v| u16::try_from(v).ok()).unwrap_or(0);
            match session.resize(rows, cols) { Ok(()) => serde_json::json!({"ok":true}), Err(e) => err(e.to_string()) }
        }
        "session.send" => {
            let (Some(id), Some(text)) = (
                req.get("id").and_then(|v| v.as_str()),
                req.get("text").and_then(|v| v.as_str()),
            ) else {
                return err("session.send needs { id, text }");
            };
            let enter = req.get("enter").and_then(|v| v.as_bool()).unwrap_or(true);
            let Some(session) = state.sessions.lock().unwrap().get(id) else {
                return err(format!("session `{id}` not found"));
            };
            match session.send_text(text, enter) {
                Ok(()) => serde_json::json!({ "ok": true, "id": id }),
                Err(e) => err(format!("send failed: {e}")),
            }
        }
        "session.kill" => {
            let Some(id) = req.get("id").and_then(|v| v.as_str()) else {
                return err("session.kill needs { id }");
            };
            let Some(session) = state.sessions.lock().unwrap().get(id) else {
                return err(format!("session `{id}` not found"));
            };
            match session.kill() {
                Ok(()) => serde_json::json!({ "ok": true, "id": id }),
                Err(e) => err(format!("kill failed: {e}")),
            }
        }
        "task.verify" => {
            let Some(name) = req.get("name").and_then(|v| v.as_str()) else {
                return err("task.verify needs { name }");
            };
            let deps = Deps {
                store: Arc::clone(&state.store),
                sessions: Arc::clone(&state.sessions),
                config: state.config.clone(),
            };
            match runner::verify_gated(&deps, name) {
                Ok(msg) => serde_json::json!({ "ok": true, "message": msg }),
                Err(e) => err(e.to_string()),
            }
        }
        "task.clean" => {
            let all = req.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
            let deps = Deps {
                store: Arc::clone(&state.store),
                sessions: Arc::clone(&state.sessions),
                config: state.config.clone(),
            };
            match runner::clean_tasks(&deps, all) {
                Ok(report) => serde_json::json!({ "ok": true, "report": report }),
                Err(e) => err(e.to_string()),
            }
        }
        "task.pr" => {
            let Some(name) = req.get("name").and_then(|v| v.as_str()) else {
                return err("task.pr needs { name }");
            };
            let deps = Deps {
                store: Arc::clone(&state.store),
                sessions: Arc::clone(&state.sessions),
                config: state.config.clone(),
            };
            match runner::pr_for(&deps, name) {
                Ok(msg) => serde_json::json!({ "ok": true, "message": msg }),
                Err(e) => err(e.to_string()),
            }
        }
        other => err(format!("unknown cmd: {other}")),
    }
}
