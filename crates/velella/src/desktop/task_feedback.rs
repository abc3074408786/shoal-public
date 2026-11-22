use super::*;
use crate::workbench::Workbench;

const UNAVAILABLE: &str = "原 Agent 已退出或状态未知，未发送反馈；请明确恢复会话并完成登录/授权，或在原终端手动输入。";
const UNCERTAIN: &str = "终端输入结果未确认，已暂停自动执行；请检查原会话，不会自动重发。";

fn workspace(task: &Task) -> String {
    if task.direct { format!("main:{}", task.project_id) } else { format!("task:{}", task.id) }
}

fn request_id(task: &Task, req: &Value) -> Result<String> {
    anyhow::ensure!(req["task_id"].as_str() == Some(task.id.as_str()), "feedback belongs to another task");
    Ok(uuid::Uuid::parse_str(req["request_id"].as_str().context("feedback request ID required")?)?.to_string())
}

fn public(receipt: &Value) -> Value {
    // Allowlist only: neither private text nor arbitrary host error/output is a
    // public receipt. An interrupted intent is never evidence of submission.
    let mut value = json!({});
    for key in ["request_id", "task_id", "session_id", "state", "created_at", "input_sequence", "error"] {
        if let Some(field) = receipt.get(key) { value[key] = field.clone(); }
    }
    if !matches!(value["state"].as_str(), Some("submitted" | "rejected")) {
        value["state"] = json!("uncertain");
        value["error"] = json!(UNCERTAIN);
    }
    value
}

impl TaskExecution {
    // Called under the task gate BEFORE current phase/folder/capacity/descriptor
    // validation. Status and replays use the already-open store and do not even
    // register legacy sessions, save a task, probe a host or construct a launch.
    pub(in crate::desktop) fn feedback_response(store: &Workbench, task: &Task, req: &Value) -> Result<Option<Value>> {
        let id = request_id(task, req)?;
        let query = req["cmd"] == "task.feedback.status";
        let text = if query { req.get("text").map(|v| v.as_str().context("feedback text required")).transpose()? }
            else { Some(req["text"].as_str().context("feedback text required")?) };
        if let Some(receipt) = store.feedback(&workspace(task), &id)? {
            anyhow::ensure!(receipt["kind"] == "task.feedback" && receipt["task_id"] == task.id
                && text.is_none_or(|text| receipt["text"] == text), "request ID belongs to different feedback");
            return Ok(Some(json!({"ok":true,"receipt":public(&receipt)})));
        }
        Ok(query.then(|| json!({"ok":true,"receipt":null})))
    }

    pub(super) fn send_task_feedback(&self, task: &mut Task, req: &Value) -> Result<Value> {
        let mut store = Workbench::open(&self.root)?;
        if let Some(response) = Self::feedback_response(&store, task, req)? { return Ok(response); }
        let id = request_id(task, req)?;
        let text = req["text"].as_str().context("feedback text required")?;
        anyhow::ensure!(!text.trim().is_empty() && text.len() <= 65536
            && text.chars().all(|c| !c.is_control() || matches!(c, '\n' | '\t')),
            "feedback must be nonempty, at most 65536 bytes, with only LF/tab control characters");
        let wid = workspace(task);
        // Use the original task, not a modified descriptor, for legacy identity.
        store.sync_legacy(&self.projects, std::slice::from_ref(task))?;
        let mut receipt = json!({"kind":"task.feedback","request_id":id,"task_id":task.id,
            "text":text,"state":"sending","created_at":chrono::Utc::now().to_rfc3339()});
        let ready = (|| -> Result<_> {
            anyhow::ensure!(matches!(task.phase, Phase::Review | Phase::NeedsAttention), "task is not ready for review feedback");
            let w = store.workspace(&wid)?;
            anyhow::ensure!(!w.archived && w.path.is_dir(), "workspace is archived or unavailable");
            let agent = task.agent.clone().context(UNAVAILABLE)?;
            anyhow::ensure!(!agent.is_legacy_chat(), "saved chat is read-only; use a native terminal");
            // No launch or control transition is permitted on this route.
            let status = get_status(&agent).map_err(|_| anyhow::anyhow!(UNAVAILABLE))?;
            anyhow::ensure!(status.exit.is_none() && status.pid != 0, UNAVAILABLE);
            anyhow::ensure!(!status.human_control && !status.typed_control,
                "请先在终端明确交还人工控制，再发送任务反馈。未改变人工控制。");
            let sequence = status.input_sequence.context("host cannot guard the input sequence; use manual input")?;
            host::require_input_ready(&status, sequence, None)?;
            let acknowledged = sequence.checked_add(1).context("input sequence exhausted; use manual input")?;
            anyhow::ensure!(!agent.directory.join("terminal-input-pending.json").exists(),
                "上一条输入仍未确认；请检查原会话，本次反馈未发送。");
            Ok((agent, status, sequence, acknowledged, renewed_deadline(task)?))
        })();
        let (agent, status, sequence, acknowledged, deadline) = match ready {
            Ok(ready) => ready,
            Err(error) => {
                // These errors are local guard messages, never host output.
                receipt["state"] = json!("rejected");
                receipt["error"] = json!(error.to_string());
                store.begin_feedback(&wid, &id, &receipt)?;
                return Ok(json!({"ok":true,"receipt":public(&receipt)}));
            }
        };
        receipt["session_id"] = json!(agent.id);
        receipt["expected_input_sequence"] = json!(sequence);
        receipt["expected_host_pid"] = json!(status.pid);
        store.begin_feedback(&wid, &id, &receipt)?;
        // A crash during/after input leaves a durable paused task and a sending
        // intent. Neither can cause automatic delivery on restart.
        task.phase = Phase::NeedsAttention;
        task.verified = false;
        task.reason = UNCERTAIN.into();
        self.persist(task)?;
        // Explicit legacy kind is the typed protocol's managed/automated input:
        // `user` would take human control. Keep the native guard and control
        // state intact; bracketed paste preserves LF/tab without extra submits.
        let input = json!({"cmd":"input","text":format!("\u{1b}[200~{text}\u{1b}[201~"),
            "enter":true,"automated":true,"input_kind":"legacy","require_input_ready":true,
            "expected_input_sequence":sequence,"expected_host_pid":status.pid,"request_id":id});
        let sent = terminal_io::ordered_host_request(&agent, &input);
        let acknowledged = sent.as_ref().is_ok_and(|value| value["ok"] == true
            && value["input_sequence"].as_u64() == Some(acknowledged));
        if acknowledged {
            receipt["state"] = json!("submitted");
            receipt["input_sequence"] = json!(sequence + 1);
        } else {
            // Even ok:false can follow a partial PTY write. Without specific
            // no-effect evidence every post-send failure is uncertain.
            receipt["state"] = json!("uncertain");
            receipt["error"] = json!(UNCERTAIN);
        }
        // Store acknowledged delivery before the task save: a save interruption
        // must not make the same request eligible for another PTY write.
        store.finish_feedback(&wid, &id, &receipt)?;
        if acknowledged {
            task.phase = Phase::Running;
            task.deadline = Some(deadline);
            task.command = None;
            task.reason = "审查意见已提交到终端（PTY 输入已确认），不代表 Agent 已接受或开始执行".into();
            self.persist(task)?;
        }
        Ok(json!({"ok":true,"receipt":public(&receipt)}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    struct Fixture {
        service: Arc<Mutex<Service>>,
        desc: Descriptor,
        root: PathBuf,
        _cleanup: Cleanup,
    }
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }
    impl Fixture {
        fn new(direct: bool) -> Self {
            let root = std::env::temp_dir().join(format!("velella-task-feedback-{}", uuid::Uuid::now_v7()));
            let data = root.join("data");
            let project = root.join("project");
            let directory = root.join("session");
            for path in [&data, &project, &directory] { std::fs::create_dir_all(path).unwrap(); }
            let desc = Descriptor { id:"agent".into(), endpoint:ipc::endpoint(&data,"feedback"), directory,
                spawn:SpawnRequest { sandbox:Default::default(), program:"codex".into(), args:vec![],
                    cwd:None, env:HashMap::new(), adapter:Some("codex".into()) } };
            let task: Task = serde_json::from_value(json!({"id":"t","project_id":"p",
                "spec":TaskSpec::from_yaml_str("name: fixture\nagent: codex").unwrap(),
                "phase":"review","worktree":project,"branch":"fixture","base_oid":"base",
                "agent":desc,"created_at":"","reason":"fixture review","verified":true,
                "checks":[],"retries":0,"observed_turns":0,"allowed_commands":[],"direct":direct})).unwrap();
            let service = Service { root:data.clone(), executable:root.join("DO-NOT-LAUNCH"),
                projects:vec![Project { id:"p".into(), path:project, name:"fixture".into(),
                    setup:String::new(), default_base:None, setup_default:SetupPolicy::Skip, direct, terminals:vec![] }],
                tasks:vec![task], project_locks:HashMap::new(), workbench:Workbench::open(&data).unwrap(),
                capabilities:capabilities::Capabilities::open(&data).unwrap(), terminal_gates:HashMap::new(),
                terminal_inflight:Default::default(), terminal_epoch:0, collaboration:Arc::new(Mutex::new(None)) };
            service.persist(&service.tasks[0]).unwrap();
            Self { service:Arc::new(Mutex::new(service)), desc, _cleanup:Cleanup(root.clone()), root }
        }
        fn request(&self) -> Value {
            json!({"cmd":"task.feedback","task_id":"t","request_id":uuid::Uuid::now_v7().to_string(),"text":"private review\nwith\ttabs"})
        }
        fn call(&self, req: &Value) -> Result<Value> { terminal_io::dispatch(&self.service, req.clone()) }
        fn saved(&self) -> Task {
            let state = self.service.lock().unwrap();
            let document: String = db(&state.projects[0]).unwrap().query_row("SELECT document FROM desktop_tasks WHERE id='t'", [], |r| r.get(0)).unwrap();
            serde_json::from_str(&document).unwrap()
        }
        fn reopen(&self) {
            let saved = self.saved();
            let mut state = self.service.lock().unwrap();
            state.tasks[0] = saved;
            state.workbench = Workbench::open(&state.root).unwrap();
        }
        fn receipt(&self, req: &Value) -> Value {
            let state = self.service.lock().unwrap();
            state.workbench.feedback(&workspace(&state.tasks[0]), req["request_id"].as_str().unwrap()).unwrap().unwrap()
        }
    }

    struct Peer {
        endpoint: String,
        thread: Option<std::thread::JoinHandle<()>>,
        calls: Arc<Mutex<Vec<Value>>>,
        writes: Arc<Mutex<usize>>,
        release: Option<mpsc::Sender<()>>,
    }
    impl Peer {
        fn new(f: &Fixture, case: &'static str) -> Self {
            let listener = ipc::listen(&f.desc.endpoint).unwrap();
            let calls = Arc::new(Mutex::new(vec![]));
            let writes = Arc::new(Mutex::new(0));
            let recorded = calls.clone();
            let written = writes.clone();
            let observed = f.service.clone();
            let (release, wait) = mpsc::channel();
            let thread = std::thread::spawn(move || {
                let mut status = Status { pid:77, state:"waiting_input".into(), hooks_ready:true,
                    native_id:Some("native".into()), input_protocol:1, input_ready_protocol:1,
                    input_sequence:Some(5), ..Status::default() };
                match case {
                    "ended" => status.exit = Some(0),
                    "human" => status.human_control = true,
                    "typed" => status.typed_control = true,
                    "permission" => status.state = "waiting_permission".into(),
                    "permission_input" => status.permission_input = true,
                    "starting" => status.state = "starting".into(),
                    "unknown" => status.state = "unknown".into(),
                    "untyped" => status.input_protocol = 0,
                    "unguarded" => status.input_ready_protocol = 0,
                    "no_sequence" => status.input_sequence = None,
                    "exhausted" => status.input_sequence = Some(u64::MAX),
                    _ => {}
                }
                loop {
                    let mut stop = false;
                    ipc::handle(listener.accept().unwrap(), |req| {
                        if req["cmd"] == "fixture.stop" { stop = true; return Ok(json!({"ok":true})); }
                        let state = observed.try_lock().expect("feedback host I/O held the global Service mutex");
                        assert!(state.terminal_gates["task:t"].try_lock().is_err(), "feedback released the task gate");
                        recorded.lock().unwrap().push(req.clone());
                        if req["cmd"] == "status" { return Ok(json!({"ok":true,"status":status})); }
                        assert_eq!(req["cmd"], "input", "feedback must never launch/control a host");
                        assert_eq!(req["automated"], true);
                        assert_eq!(req["input_kind"], "legacy");
                        assert_eq!(req["require_input_ready"], true);
                        assert_eq!(req["expected_host_pid"], 77);
                        assert_eq!(req["expected_input_sequence"], 5);
                        assert_eq!(req["text"], "\u{1b}[200~private review\nwith\ttabs\u{1b}[201~");
                        let receipt = state.workbench.feedback(&workspace(&state.tasks[0]), req["request_id"].as_str().unwrap()).unwrap().unwrap();
                        assert_eq!(receipt["state"], "sending", "intent must precede input");
                        let document: String = db(&state.projects[0]).unwrap().query_row("SELECT document FROM desktop_tasks WHERE id='t'", [], |r| r.get(0)).unwrap();
                        let paused: Task = serde_json::from_str(&document).unwrap();
                        assert_eq!(paused.phase, Phase::NeedsAttention);
                        assert!(!paused.verified);
                        drop(state);
                        match case {
                            "race_sequence" => status.input_sequence = Some(6),
                            "race_permission" => status.permission_input = true,
                            "race_human" => status.human_control = true,
                            "race_pid" => status.pid = 88,
                            _ => {}
                        }
                        anyhow::ensure!(req["expected_host_pid"] == status.pid, "fixture host restarted");
                        anyhow::ensure!(!status.human_control, "fixture human control");
                        host::require_input_ready(&status, 5, None)?;
                        *written.lock().unwrap() += 1;
                        status.input_sequence = Some(6);
                        status.state = "input_pending".into();
                        match case {
                            "lost_ack" => { wait.recv().unwrap(); }
                            "missing_ack" => return Ok(json!({"ok":true})),
                            "wrong_ack" => return Ok(json!({"ok":true,"input_sequence":99})),
                            "partial_error" => return Ok(json!({"ok":false,"error":"private review echoed by PTY error"})),
                            _ => {}
                        }
                        Ok(json!({"ok":true,"input_sequence":6}))
                    });
                    if stop { break; }
                }
            });
            Self { endpoint:f.desc.endpoint.clone(), thread:Some(thread), calls, writes, release:Some(release) }
        }
    }
    impl Drop for Peer {
        fn drop(&mut self) {
            if let Some(release) = self.release.take() { let _ = release.send(()); }
            let _ = ipc::request(&self.endpoint, &json!({"cmd":"fixture.stop"}));
            self.thread.take().unwrap().join().unwrap();
        }
    }

    #[test]
    fn managed_feedback_submitted_replay_survives_phase_descriptor_and_reopen() {
        for direct in [false, true] {
            let f = Fixture::new(direct);
            let peer = Peer::new(&f, "ready");
            let request = f.request();
            let submitted = f.call(&request).unwrap();
            assert_eq!(submitted["receipt"]["state"], "submitted");
            assert!(!submitted.to_string().contains("private review"));
            assert!(submitted["receipt"]["accepted"].is_null());
            assert_eq!(f.saved().phase, Phase::Running);
            assert!(!f.saved().verified);
            f.reopen();
            { let mut state = f.service.lock().unwrap();
                state.tasks[0].phase = Phase::Delivered;
                state.tasks[0].agent = None;
                // Replays must bypass folder/capacity/launch validation too.
                state.tasks[0].spec.launch.model = Some("--invalid".into());
                let mut w = state.workbench.workspace(&workspace(&state.tasks[0])).unwrap();
                w.archived = true; state.workbench.put_workspace(&w).unwrap();
                state.persist(&state.tasks[0]).unwrap();
            }
            let before = serde_json::to_value(f.saved()).unwrap();
            assert_eq!(f.call(&request).unwrap(), submitted);
            let query = json!({"cmd":"task.feedback.status","task_id":"t","request_id":request["request_id"]});
            assert_eq!(f.call(&query).unwrap(), submitted);
            assert_eq!(serde_json::to_value(f.saved()).unwrap(), before);
            assert_eq!(peer.calls.lock().unwrap().len(), 2);
            assert_eq!(*peer.writes.lock().unwrap(), 1);
            assert_eq!(f.receipt(&request)["text"], request["text"]);
        }
    }

    #[test]
    fn managed_feedback_rejects_cross_content_task_and_global_namespace_collision() {
        for direct in [false, true] {
            let f = Fixture::new(direct);
            let _peer = Peer::new(&f, "ready");
            let request = f.request(); f.call(&request).unwrap();
            let mut changed = request.clone(); changed["text"] = json!("different");
            assert!(f.call(&changed).is_err());
            changed["cmd"] = json!("task.feedback.status"); assert!(f.call(&changed).is_err());
            { let mut state = f.service.lock().unwrap();
                let mut other = state.tasks[0].clone(); other.id = "other".into(); state.tasks.push(other);
            }
            let mut wrong = request.clone(); wrong["task_id"] = json!("other");
            assert!(f.call(&wrong).is_err());
            wrong["cmd"] = json!("task.feedback.status"); assert!(f.call(&wrong).is_err());
            let collision = f.request();
            { let state = f.service.lock().unwrap();
                state.workbench.begin_feedback(&workspace(&state.tasks[0]), collision["request_id"].as_str().unwrap(),
                    &json!({"session_id":"agent","text":collision["text"],"state":"submitted"})).unwrap();
            }
            assert!(f.call(&collision).is_err(), "workbench feedback ID cannot become task feedback");
        }
    }

    #[test]
    fn managed_feedback_readiness_guards_reject_without_effects() {
        for case in ["ended", "human", "typed", "permission", "permission_input", "starting", "unknown", "untyped", "unguarded", "no_sequence", "exhausted"] {
            let f = Fixture::new(false); let peer = Peer::new(&f, case);
            let request = f.request();
            let before = serde_json::to_value(f.saved()).unwrap();
            let result = f.call(&request).unwrap();
            assert_eq!(result["receipt"]["state"], "rejected", "{case}");
            if case == "ended" { assert!(result["receipt"]["error"].as_str().unwrap().contains("恢复")); }
            assert_eq!(f.call(&request).unwrap(), result);
            assert_eq!(serde_json::to_value(f.saved()).unwrap(), before, "{case}");
            assert_eq!(peer.calls.lock().unwrap().len(), 1, "{case}");
            assert_eq!(*peer.writes.lock().unwrap(), 0);
            assert!(!f.root.join("project/.velella/sessions").exists());
        }
        let f = Fixture::new(false); // No listening host: do not recreate it.
        let result = f.call(&f.request()).unwrap();
        assert_eq!(result["receipt"]["state"], "rejected");
        assert!(result["receipt"]["error"].as_str().unwrap().contains("恢复"));
        assert!(f.saved().verified);
    }

    #[test]
    fn managed_feedback_ambiguous_input_and_races_pause_without_redelivery() {
        for case in ["lost_ack", "missing_ack", "wrong_ack", "partial_error", "race_sequence", "race_permission", "race_human", "race_pid"] {
            let f = Fixture::new(false); let peer = Peer::new(&f, case);
            let request = f.request(); let result = f.call(&request).unwrap();
            assert_eq!(result["receipt"]["state"], "uncertain", "{case}");
            assert!(!result.to_string().contains("private review"));
            assert_eq!(f.saved().phase, Phase::NeedsAttention); assert!(!f.saved().verified);
            f.reopen();
            assert_eq!(f.call(&request).unwrap(), result);
            assert_eq!(f.call(&json!({"cmd":"task.feedback.status","task_id":"t","request_id":request["request_id"]})).unwrap(), result);
            assert_eq!(peer.calls.lock().unwrap().len(), 2, "{case}");
            assert_eq!(*peer.writes.lock().unwrap(), usize::from(!case.starts_with("race_")));
        }
    }

    #[test]
    fn managed_feedback_status_is_read_only_including_interrupted_intent() {
        let f = Fixture::new(false);
        let request = f.request();
        let query = json!({"cmd":"task.feedback.status","task_id":"t","request_id":request["request_id"]});
        assert!(f.call(&query).unwrap()["receipt"].is_null());
        { let mut state = f.service.lock().unwrap();
            assert!(state.workbench.workspaces().unwrap().is_empty(), "status registered a workspace");
            assert_eq!(state.terminal_epoch, 0);
            let projects = state.projects.clone(); let tasks = state.tasks.clone();
            state.workbench.sync_legacy(&projects, &tasks).unwrap();
            state.workbench.begin_feedback("task:t", request["request_id"].as_str().unwrap(),
                &json!({"kind":"task.feedback","request_id":request["request_id"],"task_id":"t","text":request["text"],"state":"sending"})).unwrap();
        }
        f.reopen();
        let before = serde_json::to_value(f.saved()).unwrap();
        let result = f.call(&query).unwrap();
        assert_eq!(result["receipt"]["state"], "uncertain");
        assert_eq!(f.call(&request).unwrap(), result);
        assert_eq!(f.receipt(&request)["state"], "sending", "read rewrote a durable intent");
        assert_eq!(serde_json::to_value(f.saved()).unwrap(), before);
        assert_eq!(f.service.lock().unwrap().terminal_epoch, 0);
        assert!(!f.desc.directory.join("terminal-input-pending.json").exists());
    }

    #[test]
    fn managed_feedback_validates_uuid_text_and_controls_before_host_io() {
        let f = Fixture::new(false);
        for text in [String::new(), " \n\t".into(), "x".repeat(65537), "x\r".into(), "x\0".into(), "x\u{1b}".into(), "x\u{7f}".into()] {
            let mut req = f.request(); req["text"] = json!(text); assert!(f.call(&req).is_err());
        }
        let mut req = f.request(); req["request_id"] = json!("not-a-uuid"); assert!(f.call(&req).is_err());
        assert!(f.service.lock().unwrap().workbench.workspaces().unwrap().is_empty());
        let mut req = f.request(); req["text"] = json!("x".repeat(65536));
        assert_eq!(f.call(&req).unwrap()["receipt"]["state"], "rejected", "valid maximum reaches unavailable-host preflight");
    }

    #[test]
    fn managed_feedback_receipt_outlives_task_save_and_receipt_finish_failures() {
        for fail_receipt in [false, true] {
            let f = Fixture::new(false); let peer = Peer::new(&f, "ready");
            let request = f.request();
            let conn = if fail_receipt {
                rusqlite::Connection::open(f.root.join("data/workbench.db")).unwrap()
            } else { db(&f.service.lock().unwrap().projects[0]).unwrap() };
            conn.execute_batch(if fail_receipt {
                "CREATE TRIGGER fail_receipt BEFORE UPDATE ON feedback_deliveries BEGIN SELECT RAISE(ABORT,'fixture receipt save failure'); END;"
            } else {
                "CREATE TRIGGER fail_running BEFORE UPDATE ON desktop_tasks WHEN json_extract(NEW.document,'$.phase')='running' BEGIN SELECT RAISE(ABORT,'fixture task save failure'); END;"
            }).unwrap();
            assert!(f.call(&request).is_err());
            f.reopen();
            let replay = f.call(&request).unwrap();
            assert_eq!(replay["receipt"]["state"], if fail_receipt {"uncertain"} else {"submitted"});
            assert!(!f.saved().phase.active()); assert!(!f.saved().verified);
            assert_eq!(peer.calls.lock().unwrap().len(), 2);
            assert_eq!(*peer.writes.lock().unwrap(), 1);
        }
    }
}
