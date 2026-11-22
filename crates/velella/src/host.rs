use crate::{ipc, sessions::{Session, SessionManager, SpawnRequest}};
use anyhow::{Context, Result};
use base64::Engine;
use fs2::FileExt;
use interprocess::local_socket::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs::{File, OpenOptions}, io::{Read, Seek, SeekFrom}, path::{Path, PathBuf}, sync::{Arc, Mutex}, time::Duration};

pub(crate) const INITIAL_PROMPT_ENV: &str = "VELELLA_INITIAL_PROMPT";
pub(crate) const INITIAL_PROMPT_CLOSED: &str = "terminal-initial-prompt-closed.json";

#[derive(Clone, Serialize, Deserialize)]
pub struct Descriptor {
    pub id: String,
    pub endpoint: String,
    pub spawn: SpawnRequest,
    pub directory: PathBuf,
}

impl Descriptor {
    // Retain only a tombstone for old persisted sessions; never launch them as PTYs.
    pub(crate) fn is_legacy_chat(&self) -> bool {
        self.spawn.adapter.as_deref() == Some("codex-chat")
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Status {
    pub pid: u32,
    pub state: String,
    pub native_id: Option<String>,
    pub hooks_ready: bool,
    /// The local hybrid worker can accept a framed task; this is not evidence
    /// that a remote model has started or authenticated.
    #[serde(default)] pub hybrid_worker_ready: bool,
    pub turns: u64,
    pub human_control: bool,
    #[serde(default)]
    pub input_protocol: u8,
    // This host answers cursor/primary DA queries without a desktop viewer.
    #[serde(default)]
    pub terminal_query_protocol: u8,
    #[serde(default)]
    pub typed_control: bool,
    #[serde(default)]
    pub permission_input: bool,
    #[serde(default)]
    pub input_sequence: Option<u64>,
    // Advertises the atomic readiness + sequence guard (old hosts fail closed).
    #[serde(default)]
    pub input_ready_protocol: u8,
    #[serde(default)]
    pub submission_protocol: u8,
    #[serde(skip)]
    pub(crate) pending_submission: Option<crate::submission_receipts::Receipt>,
    #[serde(default)]
    pub(crate) initial_input_request: Option<String>,
    #[serde(default)]
    pub(crate) initial_prompt_blocked: bool,
    pub last_event: Option<Value>,
    pub exit: Option<i32>,
    pub bytes: u64,
    #[serde(default)]
    pub(crate) context_usage: Option<crate::collaboration::context_usage::Receipt>,
}

#[cfg(target_os = "linux")]
#[path = "hook_inbox.rs"]
mod inbox;

struct Host { session: Arc<Session>, status: Mutex<Status>, desc: Descriptor }

pub fn run(path: &Path) -> Result<()> {
    let desc: Descriptor = serde_json::from_slice(&std::fs::read(path)?)?;
    anyhow::ensure!(!desc.is_legacy_chat(), "standalone Codex chat has been removed; saved session data is retained");
    let restricted_terminal = desc.spawn.adapter.as_deref() == Some(crate::terminal_sandbox::ADAPTER);
    if restricted_terminal { crate::restricted_controller::validate_launch(&desc)?; }
    std::fs::create_dir_all(&desc.directory)?;
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(desc.directory.join("owner.lock"))?;
    lock.try_lock_exclusive().context("session already owned")?;
    // The owner lock above serializes recovery. A host exits via process::exit,
    // so its filesystem socket can outlive the process and listener destructor.
    let listener = ipc::listen_recovering(&desc.endpoint)?;
    let hook_listener = if desc.spawn.sandbox.is_isolated() {
        if let Some(endpoint) = desc.spawn.env.get("VELELLA_SESSION_ENDPOINT") {
            anyhow::ensure!(endpoint != &desc.endpoint, "sandbox hooks must not use the control socket");
            let parent = Path::new(endpoint).parent().context("hook socket directory required")?;
            let crate::sandbox::Policy::Isolated { read_only, .. } = &desc.spawn.sandbox else { unreachable!() };
            anyhow::ensure!(read_only.iter().any(|p| p == parent), "hook socket requires a dedicated read-only mount");
            Some(ipc::listen(endpoint)?)
        } else { None }
    } else { None };
    #[cfg(target_os = "linux")]
    let hook_inbox = if desc.spawn.sandbox.is_restricted() {
        anyhow::ensure!(!desc.spawn.env.contains_key("VELELLA_SESSION_ENDPOINT"), "restricted tasks must not receive a session control socket");
        if let Some(path) = desc.spawn.env.get("VELELLA_HOOK_INBOX") {
            let path = Path::new(path);
            let crate::sandbox::Policy::Restricted { writable, .. } = &desc.spawn.sandbox else { unreachable!() };
            anyhow::ensure!(writable.iter().any(|p| p == path), "hook inbox requires a dedicated writable directory");
            anyhow::ensure!(path != desc.directory && !desc.directory.starts_with(path), "hook inbox must not expose session state");
            Some(inbox::Inbox::open(path)?)
        } else { None }
    } else { None };
    let context_usage=crate::collaboration::context_usage::read(&desc.directory,&desc.id)?;
    let log = crate::sessions::recording::Recording::open(&desc.directory)?;
    let mut manager = SessionManager::default();
    let id = if restricted_terminal {
        let (launch, bridge) = crate::terminal_sandbox::prepare(&desc)?;
        manager.spawn_controller_recorded(&desc.spawn, &launch, Some(log),
            Arc::new(move || bridge.stop()))?
    } else { manager.spawn_recorded(&desc.spawn, Some(log))? };
    let host = Arc::new(Host {
        session: manager.get(&id.0).unwrap(),
        status: Mutex::new(Status { pid: std::process::id(), state: "starting".into(), input_protocol:1,terminal_query_protocol:u8::from(cfg!(windows)),input_ready_protocol:1,input_sequence:Some(0), submission_protocol:u8::from(desc.spawn.env.get(crate::submission_receipts::TRACKING_ENV).is_some_and(|v|v=="1")), context_usage, ..Status::default() }),
        desc,
    });
    if let Some(listener) = hook_listener {
        let host = host.clone();
        // Sequential, bounded IPC handling: the sandbox cannot create arbitrary
        // host worker threads or reach terminal/control methods through this port.
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                ipc::handle(stream, |request| {
                    let request = sandbox_hook_request(&request)?;
                    dispatch(&host, request)
                });
            }
        });
    }
    #[cfg(target_os = "linux")]
    if let Some(inbox) = hook_inbox {
        let host = host.clone();
        std::thread::spawn(move || {
            while host.session.snapshot().exit.is_none() {
                if let Err(error) = inbox.drain(|request| {
                    let request = sandbox_hook_request(&request)?;
                    dispatch(&host, request).map(|_| ())
                }) { eprintln!("hook inbox: {error:#}"); }
                std::thread::sleep(Duration::from_millis(25));
            }
        });
    }
    {
        let host = host.clone();
        std::thread::spawn(move || loop {
            let snap = host.session.snapshot();
            let input_sequence=host.status.lock().unwrap().input_sequence.unwrap_or(0);
            let native=if snap.exit.is_none(){crate::terminal_restore::poll_native(&host.desc,|pid|host.session.owns_process(pid),input_sequence).ok().flatten()}else{None};
            {
                let mut status = host.status.lock().unwrap();
                if let Some(id)=native{status.native_id=Some(id);}
                status.exit = snap.exit;
                status.bytes = snap.bytes;
                if snap.exit.is_some() { status.state = snap.state.as_str().into(); }
                // Screen classification is advisory and can never complete a task.
                // Opt-in initial prompts require target hook evidence, not text
                // that might be an onboarding, login or permission menu.
                if !status.hooks_ready && snap.exit.is_none() && !host.desc.spawn.env.contains_key(INITIAL_PROMPT_ENV) {
                    let adapter = host.desc.spawn.adapter.as_deref().unwrap_or("generic");
                    if let Some(spec) = crate::adapter::lookup(if adapter == crate::terminal_sandbox::ADAPTER { "codex" } else { adapter }) {
                        if let Some(state) = spec.classify(&crate::adapter::strip_ansi(&host.session.recent_text(8192)), snap.idle_secs) {
                            status.state = state.as_str().into();
                        }
                    }
                }
                if let Err(e) = ipc::atomic_json(&host.desc.directory.join("status.json"), &*status) { eprintln!("host state save: {e}"); }
            }
            if snap.exit.is_some() {
                // The transcript/status remain readable without a resident host.
                std::thread::sleep(Duration::from_secs(2));
                std::process::exit(0);
            }
            std::thread::sleep(Duration::from_millis(300));
        });
    }
    for stream in listener.incoming() {
        let stream = stream?;
        let host = host.clone();
        std::thread::spawn(move || ipc::handle(stream, |v| dispatch(&host, v)));
    }
    Ok(())
}

fn dispatch(host: &Host, req: Value) -> Result<Value> {
    if matches!(req["cmd"].as_str(), Some("input" | "resize" | "control" | "kill")) {
        if let Some(expected) = req.get("expected_host_pid") {
            anyhow::ensure!(expected.as_u64() == Some(u64::from(std::process::id())),
                "终端已重新启动，旧终端请求未执行");
        }
    }
    match req["cmd"].as_str().unwrap_or("") {
        "status" => Ok(json!({"ok":true,"status":host.status.lock().unwrap().clone()})),
        "output" => read_output(&host.desc, req["since"].as_u64().unwrap_or(0)),
        "input" => {
            let mut status = host.status.lock().unwrap();
            anyhow::ensure!(status.exit.is_none(), "session has exited");
            let automated = req["automated"].as_bool().unwrap_or(false);
            anyhow::ensure!(!automated || !status.human_control, "human has terminal control");
            if automated {crate::collaboration::context_usage::guard(status.context_usage.as_ref())?;}
            let text = req["text"].as_str().context("input requires text")?;
            let enter = req["enter"].as_bool().unwrap_or(false);
            let hybrid_worker=host.desc.spawn.adapter.as_deref()==Some("hybrid-orchestration-worker");
            if hybrid_worker {
                anyhow::ensure!(automated&&enter&&!text.is_empty()&&text.len()<=65536,"远程编排工作会话只接收受控任务输入");
                anyhow::ensure!(status.hybrid_worker_ready&&status.state=="waiting_input"&&!status.permission_input,"远程工作会话尚未准备好接收任务");
            }
            let kind=req["input_kind"].as_str().unwrap_or("legacy");
            anyhow::ensure!(["legacy","user","protocol","pointer"].contains(&kind),"unknown input kind");
            for flag in ["initial","require_input_ready"] {
                anyhow::ensure!(req.get(flag).is_none_or(Value::is_boolean),"{flag} must be a boolean");
            }
            let expected=req.get("expected_input_sequence").filter(|v|!v.is_null()).map(|v|v.as_u64().context("expected_input_sequence must be an unsigned integer")).transpose()?;
            let initial_request=if req["initial"]==true {
                anyhow::ensure!(host.desc.spawn.env.contains_key(INITIAL_PROMPT_ENV)&&!host.desc.spawn.sandbox.is_sandboxed()
                    &&!host.desc.directory.join(INITIAL_PROMPT_CLOSED).exists(),"initial prompt is unavailable or recovered; use manual input");
                anyhow::ensure!(req["require_input_ready"]==true&&kind=="user"&&!automated,"initial input requires the readiness guard");
                let id=req["request_id"].as_str().context("initial request ID required")?;
                uuid::Uuid::parse_str(id)?;
                anyhow::ensure!(expected==Some(if enter{1}else{0}) && if enter{text.is_empty()}else{!text.is_empty()},"invalid initial input phase");
                Some(id)
            }else{None};
            if req["require_input_ready"]==true {
                require_input_ready(&status,expected.context("readiness guard requires an input sequence")?,initial_request)?;
            }
            if kind=="protocol" || kind=="pointer" {
                anyhow::ensure!(!automated&&!enter&&if kind=="pointer"{crate::terminal_input::is_pointer_report(text)}else{crate::terminal_input::is_protocol_reply(text)},"invalid terminal non-driving input");
                host.session.send_text(text,false)?;
                return Ok(json!({"ok":true}));
            }
            let submission=crate::submission_receipts::prepare(&host.desc,&status,&req)?;
            advance_input_sequence(&mut status,expected,enter||!text.is_empty()&&!crate::terminal_input::is_pointer_report(text)&&!crate::terminal_input::is_protocol_reply(text))?;
            if let Some(id)=initial_request { status.initial_input_request=Some(id.into()); }
            if kind=="user" {
                anyhow::ensure!(!automated,"user input cannot be automated");
                status.permission_input|=status.state=="waiting_permission";
                status.human_control=true;
                status.typed_control=true;
            }
            if hybrid_worker {
                let framed=base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
                // Persist the uncertainty boundary before writing to the PTY:
                // an error may mean a partial frame was already delivered.
                send_hybrid_input(&mut status,&host.desc.directory,&host.desc.id,||host.session.send_text(&framed,true))?;
            } else if automated && enter && !text.is_empty() && matches!(host.desc.spawn.adapter.as_deref(), Some("claude-code" | "codex" | "codex-restricted-terminal")) {
                // Agent TUIs batch rapid PTY input as paste. Flush the text first
                // and submit separately, as a real user would after pasting.
                host.session.send_text(text, false)?;
                std::thread::sleep(Duration::from_millis(500));
                host.session.send_text("", true)?;
            } else {
                host.session.send_text(text, enter)?;
            }
            if enter { status.state = "input_pending".into(); }
            if let Some(receipt)=submission {crate::submission_receipts::accepted(&host.desc,&mut status,receipt)?;}
            // This acknowledges PTY input only, not model acceptance or a turn.
            Ok(json!({"ok":true,"input_sequence":status.input_sequence}))
        }
        "control" => {
            let mut status=host.status.lock().unwrap();
            if host.desc.spawn.env.contains_key(INITIAL_PROMPT_ENV) {status.initial_prompt_blocked=true;}
            status.human_control = req["human"].as_bool().unwrap_or(true);
            status.typed_control=false;
            status.permission_input=false;
            Ok(json!({"ok":true}))
        }
        "resize" => {
            let rows = u16::try_from(req["rows"].as_u64().context("rows")?)?;
            let cols = u16::try_from(req["cols"].as_u64().context("cols")?)?;
            host.session.resize(rows, cols)?;
            Ok(json!({"ok":true}))
        }
        "kill" => { host.session.kill()?; Ok(json!({"ok":true})) }
        "context_usage" => {
            let mut status=host.status.lock().unwrap();
            record_context_usage(&host.desc,&mut status,&req["payload"])?;
            Ok(json!({"ok":true,"context_usage":status.context_usage}))
        }
        "hybrid_worker_event" => {
            anyhow::ensure!(host.desc.spawn.adapter.as_deref()==Some("hybrid-orchestration-worker"),"not a hybrid orchestration worker");
            let mut status=host.status.lock().unwrap();anyhow::ensure!(status.exit.is_none(),"worker has exited");
            apply_hybrid_worker_event(&mut status,&req)?;
            ipc::atomic_json(&host.desc.directory.join("status.json"),&*status)?;Ok(json!({"ok":true}))
        }
        "hook" => {
            let event = &req["event"];
            if !crate::terminal_restore::hook_is_bound(&host.desc,event){return Ok(json!({"ok":true}));}
            let mut status = host.status.lock().unwrap();
            if event.get("agent_id").is_some_and(|v| !v.is_null()) { return Ok(json!({"ok":true})); }
            let initial=host.desc.spawn.env.contains_key(INITIAL_PROMPT_ENV);
            if initial || host.desc.spawn.env.contains_key(crate::submission_receipts::TRACKING_ENV) {
                let native=event["session_id"].as_str().filter(|id|!id.is_empty()&&id.len()<=256).context("initial prompt hook requires native session identity")?;
                anyhow::ensure!(status.native_id.as_deref().is_none_or(|id|id==native),"hook belongs to another native session");
                if let Some(cwd)=event["cwd"].as_str() {
                    anyhow::ensure!(dunce::canonicalize(cwd)?==dunce::canonicalize(host.desc.spawn.cwd.as_deref().context("session cwd required")?)?,"hook working directory mismatch");
                }
            }
            if event.get("context_usage").is_some() && event["hook_event_name"]!="SessionStart" {
                anyhow::ensure!(status.native_id.as_deref().is_none_or(|id|Some(id)==event["session_id"].as_str()),"context source belongs to another native session");
            }
            if host.desc.spawn.env.contains_key("VELELLA_RESTORE_DIR") {
                // Existing role hooks already arrive on this invocation's
                // endpoint. Observe them without rewriting role configuration.
                let _=crate::terminal_restore::observe_hook(&host.desc,event);
            }
            if let Some(id) = event["session_id"].as_str() { status.native_id = Some(id.into()); }
            if event.get("context_usage").is_some() {
                record_context_usage(&host.desc,&mut status,event)?;
            }
            crate::submission_receipts::observe(&host.desc,&mut status,event)?;
            apply_hook_status(&mut status,event,initial);
            status.last_event = Some(event.clone());
            ipc::atomic_json(&host.desc.directory.join("status.json"), &*status)?;
            Ok(json!({"ok":true}))
        }
        _ => anyhow::bail!("unknown session command"),
    }
}

fn prepare_hybrid_input(status:&mut Status,directory:&Path)->Result<()> {
    status.hybrid_worker_ready=false;
    status.state="input_pending".into();
    ipc::atomic_json(&directory.join("status.json"),status)
}

fn send_hybrid_input(status:&mut Status,directory:&Path,session_id:&str,send:impl FnOnce()->Result<()>)->Result<()> {
    prepare_hybrid_input(status,directory)?;
    // Invalidate the previous turn's completion before the next frame can be
    // consumed, including if the host dies before the worker launches SSH.
    ipc::atomic_json(&directory.join("hybrid-turn-state.json"),&json!({"state":"uncertain","session_id":session_id}))?;
    if let Err(error)=send() {
        status.state="interrupted".into();
        let persist=ipc::atomic_json(&directory.join("status.json"),status);
        anyhow::bail!("remote worker input outcome is uncertain; do not replay automatically: {error}; status persistence error: {:?}",persist.err());
    }
    Ok(())
}

fn apply_hybrid_worker_event(status:&mut Status,req:&Value)->Result<()> {
    match req["event"].as_str() {
        Some("ready")=>{anyhow::ensure!(status.state=="starting","worker readiness already recorded");status.hybrid_worker_ready=true;status.state="waiting_input".into();},
        Some("running")=>{anyhow::ensure!(!status.hybrid_worker_ready&&status.state=="input_pending","worker has no accepted input");status.state="running".into();},
        Some("completed")=>{
            anyhow::ensure!(status.state=="running","worker has no running turn");let native=req["native_id"].as_str().context("native identity required")?;uuid::Uuid::parse_str(native)?;
            anyhow::ensure!(status.native_id.as_deref().is_none_or(|old|old==native),"remote native identity changed");status.native_id=Some(native.into());status.turns+=1;status.hybrid_worker_ready=true;status.state="waiting_input".into();
        },
        Some("uncertain")=>{status.hybrid_worker_ready=false;status.state="interrupted".into();},
        _=>anyhow::bail!("invalid hybrid worker event"),
    }Ok(())
}

// Used for preflight as well, but the authoritative call is above while the
// status mutex remains held through the write. Permission hooks cannot race it.
pub(crate) fn require_input_ready(status:&Status,expected:u64,initial_request:Option<&str>)->Result<()> {
    anyhow::ensure!(status.exit.is_none()&&status.input_protocol==1&&status.input_ready_protocol==1,
        "host does not support guarded input");
    anyhow::ensure!(status.state=="waiting_input"&&!status.permission_input,"Agent is not safely waiting for input; inspect its native terminal");
    anyhow::ensure!(status.input_sequence==Some(expected),"terminal has newer input; prompt was not sent");
    if let Some(id)=initial_request {
        anyhow::ensure!(status.hooks_ready&&status.native_id.is_some()&&!status.initial_prompt_blocked,"initial prompt has no safe target readiness evidence");
        if expected==0 {
            anyhow::ensure!(!status.human_control&&!status.typed_control&&status.initial_input_request.is_none(),"terminal already has human input or control");
        } else {
            anyhow::ensure!(expected==1&&status.initial_input_request.as_deref()==Some(id)&&status.human_control&&status.typed_control,
                "initial paste ownership changed; submit manually");
        }
    }
    Ok(())
}

fn apply_hook_status(status:&mut Status,event:&Value,initial:bool) {
    let name=event["hook_event_name"].as_str().unwrap_or("");
    release_submitted_input(status,name);
    match name {
        // A boot hook can precede login, trust and selection menus. For opt-in
        // initial prompts it proves only that hooks are connected, never ready.
        "SessionStart" => {status.hooks_ready=true;if !initial{status.state="waiting_input".into();}}
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" => status.state="running".into(),
        "PermissionRequest" => {status.state="waiting_permission".into();status.initial_prompt_blocked|=initial;}
        "Notification" if event["notification_type"]=="permission_prompt" => {status.state="waiting_permission".into();status.initial_prompt_blocked|=initial;}
        // Claude's idle_prompt is explicit target input readiness. Codex builds
        // without an idle signal stay pending/manual; agent_needs_input is not
        // sufficient because it may be an approval or an unknown menu.
        "Notification" if initial&&event["notification_type"]=="idle_prompt" => {
            if status.hooks_ready&&!status.initial_prompt_blocked&&status.state=="starting"&&status.input_sequence==Some(0) {
                status.state="waiting_input".into();
            }
        }
        "Notification" if event["notification_type"]=="idle_prompt" => {
            // Preserve readiness already proven by a completed native turn.
            // An idle notification alone cannot establish a safe input target.
            let confirmed_idle=status.turns>0&&status.hooks_ready
                &&status.native_id.as_deref().is_some_and(|id|!id.is_empty())
                &&!status.human_control&&!status.typed_control
                &&status.input_sequence.is_some_and(|sequence|require_input_ready(status,sequence,None).is_ok());
            if !confirmed_idle {status.state="interrupted".into();}
        }
        "Notification" if event["notification_type"]=="agent_needs_input" => status.state="interrupted".into(),
        "Stop" => {status.turns+=1;status.state="waiting_input".into();}
        "Interrupt" => {status.state="interrupted".into();status.initial_prompt_blocked|=initial;}
        _=>{},
    }
}

fn record_context_usage(desc:&Descriptor,status:&mut Status,payload:&Value)->Result<()> {
    let provider=desc.spawn.env.get("VELELLA_PROVIDER").context("context source has no role provider")?;
    let native=payload["session_id"].as_str().context("context source has no native session")?;
    anyhow::ensure!(status.native_id.as_deref().is_none_or(|id|id==native),"context source belongs to another native session");
    let value=if provider=="omp" {&payload["context_usage"]} else {payload};
    let receipt=crate::collaboration::context_usage::observe(provider,&desc.id,native,value,status.context_usage.as_ref())?;
    ipc::atomic_json(&desc.directory.join("context-usage.json"),&receipt)?;
    status.context_usage=Some(receipt);
    Ok(())
}

fn sandbox_hook_request(request: &Value) -> Result<Value> {
    anyhow::ensure!(request["cmd"] == "hook", "sandbox bridge accepts hook events only");
    let event = &request["event"];
    let name = event["hook_event_name"].as_str().context("hook event name required")?;
    anyhow::ensure!(["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "PermissionRequest", "Notification", "Stop", "Interrupt"].contains(&name), "unsupported sandbox hook event");
    let mut clean = serde_json::Map::new();
    clean.insert("hook_event_name".into(), json!(name));
    for field in ["session_id", "agent_id", "notification_type"] {
        if let Some(value) = event.get(field) {
            anyhow::ensure!(value.is_null() || value.as_str().is_some_and(|s| s.len() <= 256), "invalid hook metadata");
            clean.insert(field.into(), value.clone());
        }
    }
    Ok(json!({"cmd":"hook", "event":clean}))
}

fn release_submitted_input(status:&mut Status,event:&str) {
    if status.typed_control && (event=="UserPromptSubmit" || (status.permission_input&&["PreToolUse","PostToolUse"].contains(&event))) {
        status.human_control=false;status.typed_control=false;status.permission_input=false;
    }
}

fn advance_input_sequence(status:&mut Status,expected:Option<u64>,driving:bool)->Result<()> {
    if let Some(expected)=expected{anyhow::ensure!(status.input_sequence==Some(expected),"终端已有新输入，任务草稿未粘贴，请手动处理");}
    if driving{status.input_sequence=status.input_sequence.map(|n|n.saturating_add(1));}Ok(())
}

fn read_hook_event() -> Result<Value> {
    let mut data = Vec::new();
    std::io::stdin().take(1024 * 1024).read_to_end(&mut data)?;
    Ok(serde_json::from_slice(&data)?)
}

pub fn hook(endpoint: &str) -> Result<()> {
    ipc::request(endpoint, &json!({"cmd":"hook","event":read_hook_event()?}))?;
    // Empty output does not override any native permission decision.
    Ok(())
}

pub fn hook_inbox(path: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let request = sandbox_hook_request(&json!({"cmd":"hook", "event":read_hook_event()?}))?;
        inbox::Inbox::open(path)?.submit(&request)
    }
    #[cfg(not(target_os = "linux"))]
    { let _ = path; anyhow::bail!("file hook inbox is supported only on Linux") }
}

pub fn confirmed_dead(directory: &Path) -> bool {
    match OpenOptions::new().read(true).write(true).open(directory.join("owner.lock")) {
        // A descriptor is persisted before its host starts. If launch never
        // reached `host::run`, no owner lock exists and there cannot be an
        // executor to terminate; callers may explicitly recover or close it.
        Err(error) if error.kind()==std::io::ErrorKind::NotFound => true,
        Err(_) => false,
        Ok(file) => file.try_lock_exclusive().is_ok(),
    }
}

pub fn read_output(desc:&Descriptor,since:u64)->Result<Value> {
    let mut file=File::open(desc.directory.join("terminal.bin"))?;
    let end=file.metadata()?.len();let from=since.min(end);
    file.seek(SeekFrom::Start(from))?;let mut bytes=Vec::new();file.take((end-from).min(128*1024)).read_to_end(&mut bytes)?;
    let page_end=from+bytes.len() as u64;
    let geometry=crate::sessions::recording::page(&desc.directory,from,page_end);
    let geometry_error=geometry.as_ref().err().map(|error|error.to_string());
    Ok(json!({"ok":true,"from":from,"end":page_end,"total":end,"geometry":geometry.unwrap_or_default(),"geometry_error":geometry_error,"data":base64::engine::general_purpose::STANDARD.encode(bytes)}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn hybrid_input_is_marked_pending_before_delivery_and_fails_closed_on_storage_error(){
        let root=std::env::temp_dir().join(format!("hybrid-input-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&root).unwrap();
        let mut status=Status{state:"waiting_input".into(),hybrid_worker_ready:true,..Status::default()};
        prepare_hybrid_input(&mut status,&root).unwrap();
        let saved:Status=serde_json::from_slice(&std::fs::read(root.join("status.json")).unwrap()).unwrap();
        assert_eq!(saved.state,"input_pending");assert!(!saved.hybrid_worker_ready);
        let missing=root.join("missing").join("child");
        let mut failed=Status{state:"waiting_input".into(),hybrid_worker_ready:true,..Status::default()};
        // A regular file prevents the persistence helper from creating its parent.
        std::fs::write(root.join("missing"),b"file").unwrap();
        assert!(prepare_hybrid_input(&mut failed,&missing).is_err());
        assert_eq!(failed.state,"input_pending");assert!(!failed.hybrid_worker_ready);
        let mut failed_send=Status{state:"waiting_input".into(),hybrid_worker_ready:true,..Status::default()};
        let id=uuid::Uuid::new_v4().to_string();
        ipc::atomic_json(&root.join("hybrid-turn-state.json"),&json!({"state":"complete","session_id":id})).unwrap();
        assert!(send_hybrid_input(&mut failed_send,&root,&id,||{let state:Value=serde_json::from_slice(&std::fs::read(root.join("hybrid-turn-state.json"))?)?;assert_eq!(state["state"],"uncertain");anyhow::bail!("partial write fixture")}).is_err());
        let saved:Status=serde_json::from_slice(&std::fs::read(root.join("status.json")).unwrap()).unwrap();
        assert_eq!(saved.state,"interrupted");assert!(!saved.hybrid_worker_ready);
        let mut blocked=Status{state:"waiting_input".into(),hybrid_worker_ready:true,..Status::default()};
        let mut sent=false;
        assert!(send_hybrid_input(&mut blocked,&missing,&id,||{sent=true;Ok(())}).is_err());assert!(!sent);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn hybrid_worker_status_requires_order_and_same_native_identity(){
        let mut status=Status{state:"starting".into(),..Status::default()};let id=uuid::Uuid::new_v4().to_string();
        assert!(apply_hybrid_worker_event(&mut status,&json!({"event":"completed","native_id":id})).is_err());
        apply_hybrid_worker_event(&mut status,&json!({"event":"ready"})).unwrap();assert!(status.hybrid_worker_ready);assert!(!status.hooks_ready);assert!(status.native_id.is_none());
        assert!(apply_hybrid_worker_event(&mut status,&json!({"event":"running"})).is_err());
        status.hybrid_worker_ready=false;status.state="input_pending".into();apply_hybrid_worker_event(&mut status,&json!({"event":"running"})).unwrap();
        apply_hybrid_worker_event(&mut status,&json!({"event":"completed","native_id":id})).unwrap();assert_eq!(status.native_id.as_deref(),Some(id.as_str()));assert!(status.hybrid_worker_ready);
        status.hybrid_worker_ready=false;status.state="input_pending".into();apply_hybrid_worker_event(&mut status,&json!({"event":"running"})).unwrap();
        assert!(apply_hybrid_worker_event(&mut status,&json!({"event":"completed","native_id":uuid::Uuid::new_v4().to_string()})).is_err());
        apply_hybrid_worker_event(&mut status,&json!({"event":"uncertain"})).unwrap();assert!(!status.hybrid_worker_ready);assert_eq!(status.state,"interrupted");
        assert!(apply_hybrid_worker_event(&mut status,&json!({"event":"ready"})).is_err());
    }
    #[test] fn native_context_host_rejects_other_sessions_and_persists_only_metrics(){
        let root=std::env::temp_dir().join(format!("velella-context-host-{}",uuid::Uuid::now_v7()));std::fs::create_dir(&root).unwrap();
        let desc=Descriptor{id:"desktop-session".into(),endpoint:"unused".into(),directory:root.clone(),spawn:SpawnRequest{sandbox:Default::default(),program:"unused".into(),args:vec![],cwd:None,env:std::collections::HashMap::from([("VELELLA_PROVIDER".into(),"claude-code".into())]),adapter:None}};
        let mut status=Status{native_id:Some("native".into()),state:"running".into(),..Status::default()};
        let mut payload=json!({"session_id":"other","secret":"must-not-persist","context_window":{"context_window_size":200000,"current_usage":{"input_tokens":80000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}});
        assert!(record_context_usage(&desc,&mut status,&payload).is_err());assert!(!root.join("context-usage.json").exists());
        payload["session_id"]=json!("native");record_context_usage(&desc,&mut status,&payload).unwrap();
        assert_eq!(status.state,"running");assert!(!status.hooks_ready);
        let bytes=std::fs::read_to_string(root.join("context-usage.json")).unwrap();assert!(!bytes.contains("must-not-persist"));
        let recovered=crate::collaboration::context_usage::read(&root,"desktop-session").unwrap().unwrap();
        assert!(crate::collaboration::context_usage::guard(Some(&recovered)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn sandbox_hook_protocol_rejects_control_over_real_ipc() {
        let root = std::env::temp_dir().join(format!("vl-hook-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let endpoint = root.join("h.sock").to_string_lossy().into_owned();
        let listener = ipc::listen(&endpoint).unwrap();
        let worker = std::thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                ipc::handle(stream.unwrap(), |request| Ok(json!({"ok":true,"request":sandbox_hook_request(&request)?})));
            }
        });
        let denied = ipc::request(&endpoint, &json!({"cmd":"input","text":"touch outside"})).unwrap();
        assert_eq!(denied["ok"], false);
        let accepted = ipc::request(&endpoint, &json!({"cmd":"hook","event":{"hook_event_name":"SessionStart","session_id":"fixture"}})).unwrap();
        assert_eq!(accepted["ok"], true);
        assert_eq!(accepted["request"]["event"]["session_id"], "fixture");
        worker.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sandbox_bridge_rejects_control_and_strips_payloads() {
        for command in ["input", "kill", "resize", "control", "output", "rpc"] {
            assert!(sandbox_hook_request(&json!({"cmd":command,"event":{"hook_event_name":"Stop"}})).is_err());
        }
        assert!(sandbox_hook_request(&json!({"cmd":"hook","event":{"hook_event_name":"Execute"}})).is_err());
        assert!(sandbox_hook_request(&json!({"cmd":"hook","event":{"hook_event_name":"Stop","session_id":42}})).is_err());
        let result = sandbox_hook_request(&json!({"cmd":"hook","event":{"hook_event_name":"Stop","session_id":"s","tool_input":{"command":"bad"},"permissions":"allow"}})).unwrap();
        assert_eq!(result, json!({"cmd":"hook","event":{"hook_event_name":"Stop","session_id":"s"}}));
    }
    #[test]
    fn initial_prompt_boot_and_permissions_never_prove_readiness() {
        let mut status=Status{state:"starting".into(),input_protocol:1,input_ready_protocol:1,input_sequence:Some(0),native_id:Some("native".into()),..Status::default()};
        let event=|name:&str|json!({"hook_event_name":name,"session_id":"native"});
        apply_hook_status(&mut status,&event("SessionStart"),true);
        assert!(status.hooks_ready);assert_eq!(status.state,"starting");assert_eq!(status.turns,0);
        assert!(require_input_ready(&status,0,Some("request")).is_err());
        apply_hook_status(&mut status,&json!({"hook_event_name":"Notification","notification_type":"idle_prompt"}),true);
        require_input_ready(&status,0,Some("request")).unwrap();
        apply_hook_status(&mut status,&event("PermissionRequest"),true);
        apply_hook_status(&mut status,&event("SessionStart"),true);
        assert_eq!(status.state,"waiting_permission");
        // Even later idle/Stop events cannot erase an initial permission barrier.
        apply_hook_status(&mut status,&event("Stop"),true);
        assert!(require_input_ready(&status,0,Some("request")).is_err());
        let mut ordinary=Status::default();apply_hook_status(&mut ordinary,&event("SessionStart"),false);
        assert_eq!(ordinary.state,"waiting_input","no-prompt lifecycle is unchanged");
    }

    #[test]
    fn initial_prompt_atomic_guard_rejects_competition_and_wrong_paste_owner() {
        let ready=Status{state:"waiting_input".into(),input_protocol:1,input_ready_protocol:1,input_sequence:Some(0),hooks_ready:true,native_id:Some("native".into()),..Status::default()};
        require_input_ready(&ready,0,Some("request")).unwrap();
        for field in ["sequence","human","typed","permission","protocol","state"] {
            let mut status=ready.clone();
            match field {"sequence"=>status.input_sequence=Some(1),"human"=>status.human_control=true,"typed"=>status.typed_control=true,"permission"=>status.permission_input=true,"protocol"=>status.input_ready_protocol=0,_=>status.state="unknown".into()}
            assert!(require_input_ready(&status,0,Some("request")).is_err(),"{field}");
        }
        let mut pasted=ready;pasted.input_sequence=Some(1);pasted.human_control=true;pasted.typed_control=true;pasted.initial_input_request=Some("request".into());
        require_input_ready(&pasted,1,Some("request")).unwrap();
        assert!(require_input_ready(&pasted,1,Some("other")).is_err());
        apply_hook_status(&mut pasted,&json!({"hook_event_name":"PermissionRequest"}),true);
        assert!(require_input_ready(&pasted,1,Some("request")).is_err());
        assert_eq!(pasted.input_sequence,Some(1),"a rejected write does not consume sequence");
    }

    #[test]fn automatic_draft_rejects_input_that_arrived_after_readiness_check(){
        let mut status=Status{input_sequence:Some(0),..Status::default()};
        advance_input_sequence(&mut status,None,false).unwrap();assert_eq!(status.input_sequence,Some(0));
        advance_input_sequence(&mut status,None,true).unwrap();assert_eq!(status.input_sequence,Some(1));
        assert!(advance_input_sequence(&mut status,Some(0),true).is_err());assert_eq!(status.input_sequence,Some(1));
        assert!(advance_input_sequence(&mut Status::default(),Some(0),true).is_err());
    }
    #[test] fn only_confirmed_submission_releases_typed_control(){
        let mut s=Status{human_control:true,typed_control:true,..Status::default()};
        release_submitted_input(&mut s,"PostToolUse");assert!(s.human_control);
        release_submitted_input(&mut s,"Stop");assert!(s.human_control);
        release_submitted_input(&mut s,"UserPromptSubmit");assert!(!s.human_control);
        s.human_control=true;s.typed_control=true;s.permission_input=true;
        release_submitted_input(&mut s,"PostToolUse");assert!(!s.human_control);
        s.human_control=true;s.typed_control=false;
        release_submitted_input(&mut s,"UserPromptSubmit");assert!(s.human_control);
    }

    #[test]
    fn post_turn_idle_notification_preserves_guarded_readiness() {
        let mut status=Status{state:"running".into(),input_protocol:1,input_ready_protocol:1,input_sequence:Some(3),
            native_id:Some("native".into()),hooks_ready:true,..Status::default()};
        apply_hook_status(&mut status,&json!({"hook_event_name":"Stop"}),false);
        apply_hook_status(&mut status,&json!({"hook_event_name":"Notification","notification_type":"idle_prompt"}),false);
        assert_eq!(status.state,"waiting_input");
        require_input_ready(&status,3,None).unwrap();
        assert_eq!(status.input_sequence,Some(3));
        assert_eq!(status.turns,1);
    }

    #[test]
    fn idle_notification_cannot_create_readiness_or_override_driving_input() {
        let ready=Status{state:"waiting_input".into(),input_protocol:1,input_ready_protocol:1,input_sequence:Some(3),
            native_id:Some("native".into()),hooks_ready:true,turns:1,..Status::default()};
        let event=json!({"hook_event_name":"Notification","notification_type":"idle_prompt"});
        for state in ["starting","running","waiting_permission","input_pending","interrupted"] {
            let mut status=ready.clone();status.state=state.into();
            apply_hook_status(&mut status,&event,false);
            assert_ne!(status.state,"waiting_input","{state}");
        }
        for missing in ["turn","native","hooks","human","typed","permission","protocol"] {
            let mut status=ready.clone();
            match missing {"turn"=>status.turns=0,"native"=>status.native_id=None,"hooks"=>status.hooks_ready=false,
                "human"=>status.human_control=true,"typed"=>status.typed_control=true,"permission"=>status.permission_input=true,
                _=>status.input_ready_protocol=0}
            apply_hook_status(&mut status,&event,false);
            assert_ne!(status.state,"waiting_input","{missing}");
        }
        let mut status=ready;
        apply_hook_status(&mut status,&json!({"hook_event_name":"Notification","notification_type":"agent_needs_input"}),false);
        assert_ne!(status.state,"waiting_input","ambiguous notifications remain unconfirmed");
    }

    #[test]
    fn descriptor_without_a_started_host_is_confirmed_dead() {
        let directory=std::env::temp_dir().join(format!("velella-unstarted-host-test-{}",uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        assert!(confirmed_dead(&directory));
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
