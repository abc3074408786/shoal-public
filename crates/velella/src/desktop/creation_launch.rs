//! Server-owned ordinary launch and initial input, with observation-only recovery.
use super::*;
use crate::workbench::{CreationOperation, CreationStepState, Workbench, Workspace};
use workbench_commands::native_prompt::{self, PromptMode};
use CreationStepState::{Failed, Pending, Running, Skipped, Succeeded, Uncertain};

pub(super) fn freeze(req: &Value, workspace: &str) -> Result<Option<Value>> {
    creation_setup::validate_execution(req, SetupPolicy::Skip)?;
    let launch = req.get("launch").map(|v| v.as_str().context("launch must be a string"))
        .transpose()?.unwrap_or("none");
    // Keep this allowlist in step with workbench_commands::workspace_launch.
    anyhow::ensure!(matches!(launch, "none" | "terminal" | "codex" | "claude-code" | "gemini" | "opencode" | "omp" | "kimi" | "zcode"), "unsupported workspace launch");
    let options: velella_core::task::LaunchOptions = serde_json::from_value(
        req.get("launch_options").cloned().unwrap_or(json!({})))?;
    crate::native_launch::args(launch, &options)?;
    let permission=crate::terminal_permissions::requested(req)?;
    crate::terminal_permissions::plan(launch,permission)?;
    let prompt = native_prompt::requested(req)?;
    anyhow::ensure!((options.is_empty() && prompt.is_none())
        || matches!(launch, "codex" | "claude-code"), "launch_options and initial_prompt require native Codex or Claude");
    if launch == "none" { return Ok(None); }
    let execution: crate::sandbox::Policy = serde_json::from_value(
        req.get("execution").cloned().unwrap_or(json!({"mode":"native"})))?;
    anyhow::ensure!(!execution.is_isolated(), "ordinary terminal launch does not support isolated execution");
    if execution.is_restricted() {
        anyhow::ensure!(permission=="inherit", "restricted launch cannot override permissions");
        anyhow::ensure!(launch == "codex" && options.is_empty() && prompt.is_none(), "restricted launch requires Codex with inherited options and manual input");
        anyhow::ensure!(req["execution"].as_object().is_some_and(|v| v.len() == 1), "restricted terminal paths and network policy are owned by the execution host");
    }
    let mut request = json!({"cmd":"workbench.terminal.open", "workspace_id":workspace,
        "request_id":uuid::Uuid::now_v7().to_string(), "launch":launch, "launch_options":options,
        "sandbox":{"mode":if execution.is_restricted() {"restricted"} else {"native"}}});
    if permission!="inherit" {request["permission_mode"]=json!(permission);}
    if let Some(prompt) = prompt {
        request["initial_prompt"] = serde_json::to_value(prompt)?;
        request["feedback_request_id"] = json!(uuid::Uuid::now_v7().to_string());
    }
    Ok(Some(request))
}

fn launch_request(operation: &CreationOperation) -> Result<&Value> {
    let req = &operation.frozen["launch_request"];
    anyhow::ensure!(req.is_object() && req["cmd"] == "workbench.terminal.open"
        && req["workspace_id"].as_str() == Some(operation.request_id.as_str()), "invalid frozen launch request");
    let id = req["request_id"].as_str().context("frozen session ID missing")?;
    uuid::Uuid::parse_str(id)?;
    anyhow::ensure!(id != operation.request_id, "launch requires a separate session ID");
    Ok(req)
}

fn current_workspace(service: &Service, operation: &CreationOperation) -> Result<Workspace> {
    let frozen: Workspace = serde_json::from_value(operation.frozen["workspace"].clone())?;
    anyhow::ensure!(frozen.id == operation.request_id, "creation workspace identity mismatch");
    let current = service.workbench.workspace(&frozen.id)?;
    anyhow::ensure!(current.path == frozen.path && current.project_id == frozen.project_id
        && current.kind == frozen.kind, "workspace no longer matches its creation receipt");
    let project = service.project(&current.project_id)?;
    if let Some(path) = operation.frozen.get("project_path") {
        anyhow::ensure!(serde_json::to_value(&project.path)? == *path, "project path changed after creation");
    }
    anyhow::ensure!(!current.archived && current.path.is_dir(), "workspace is archived or unavailable");
    service.ensure_workspace_prepared(&current.id)?;
    service.ensure_workspace_writable(&current)?;
    Ok(current)
}

struct LaunchExecution {
    operation: CreationOperation,
    executable: PathBuf,
    spawn: bool,
}

fn prepare(service: &mut Service, id: &str) -> Result<Option<LaunchExecution>> {
    prepare_with(service, id, &mut |service, req| service.prepare_workbench_launch(req).map(|(created, _)| created))
}

fn prepare_with(
    service: &mut Service, id: &str,
    register: &mut impl FnMut(&mut Service, &Value) -> Result<Option<Descriptor>>,
) -> Result<Option<LaunchExecution>> {
    let Some(operation) = service.workbench.creation(id, None)? else { return Ok(None); };
    if !creation_setup::is_ordinary(&operation) { return Ok(None); }
    let mut operation = creation_setup::settle_skips(&service.workbench, operation)?;
    if operation.steps[0].state != Succeeded || !matches!(operation.steps[1].state, Succeeded | Skipped)
        || !matches!(operation.steps[2].state, Pending | Running | Uncertain) { return Ok(None); }
    let req = launch_request(&operation)?.clone();
    let mut spawn = false;
    if operation.steps[2].state == Pending {
        let created = match current_workspace(service, &operation).and_then(|_| register(service, &req)) {
            Ok(created) => created,
            Err(error) => {
                let descriptor = service.workbench.session(id, req["request_id"].as_str().unwrap())?
                    .map(|session| session.descriptor);
                if let Some(desc) = &descriptor {
                    anyhow::ensure!(req["request_id"] == desc.id, "launch descriptor identity mismatch");
                }
                let registered = descriptor.is_some();
                operation.steps[2].state = Running;
                operation.steps[2].descriptor = descriptor;
                operation.steps[2].receipt = Some(json!({"state":"preparation_intent", "session_id":req["request_id"],
                    "no_spawn":true, "registration_only":registered}));
                operation.steps[2].error = Some(if registered {
                    format!("launch preparation failed; existing session registration, observe original host only: {error:#}")
                } else {
                    format!("launch preparation failed before session registration: {error:#}")
                });
                operation = service.workbench.save_creation(&operation)?;
                operation.steps[2].state = if registered { Uncertain } else { Failed };
                operation = service.workbench.save_creation(&operation)?;
                return Ok(registered.then(|| LaunchExecution { operation, executable: service.executable.clone(), spawn:false }));
            }
        };
        spawn = created.is_some();
        let desc = match created {
            Some(desc) => desc,
            None => service.workbench.session(id, req["request_id"].as_str().unwrap())?
                .context("registered creation session missing")?.descriptor,
        };
        anyhow::ensure!(req["request_id"] == desc.id, "launch descriptor identity mismatch");
        operation.steps[2].state = Running;
        operation.steps[2].descriptor = Some(desc);
        operation.steps[2].receipt = Some(json!({"session_id":req["request_id"], "registration_only":!spawn}));
        operation = service.workbench.save_creation(&operation)?;
        if !spawn {
            // The journal can attach a descriptor only at Pending -> Running.
            // Close that bookkeeping transition immediately; it grants no spawn.
            operation.steps[2].state = Uncertain;
            operation.steps[2].error = Some("existing session registration; observe original host only".into());
            operation = service.workbench.save_creation(&operation)?;
        } else {
            let sid = req["request_id"].as_str().unwrap();
            service.terminal_inflight.insert(format!("session:{sid}"));
            service.terminal_inflight.insert(format!("starting:{sid}"));
        }
    }
    Ok(Some(LaunchExecution { operation, executable: service.executable.clone(), spawn }))
}

fn reconcile_launch(store: &Workbench, mut operation: CreationOperation, observation: Result<Status>) -> Result<CreationOperation> {
    if !matches!(operation.steps[2].state, Running | Uncertain) { return Ok(operation); }
    let step = &mut operation.steps[2];
    match observation {
        Ok(status) if status.pid > 0 || status.exit.is_some() => {
            step.state = if status.exit.is_some_and(|exit| exit != 0) { Failed } else { Succeeded };
            step.error = (step.state == Failed).then(|| "original launch exited unsuccessfully; no automatic restart".into());
            step.receipt = Some(json!({"session_id":step.descriptor.as_ref().map(|d| &d.id),
                "state":if status.exit.is_some() {"exited"} else {"started"}, "status":status}));
        }
        observation => {
            if step.state == Uncertain { return Ok(operation); }
            step.state = Uncertain;
            step.error = Some(match observation {
                Err(error) => format!("launch not confirmed; original host only, never respawned: {error:#}"),
                Ok(_) => "original host status has no process evidence".into(),
            });
        }
    }
    let operation = store.save_creation(&operation)?;
    creation_setup::settle_skips(store, operation)
}

fn feedback_request(operation: &CreationOperation) -> Result<Value> {
    let launch = launch_request(operation)?;
    let prompt = native_prompt::requested(launch)?.context("frozen prompt missing")?;
    let id = launch["feedback_request_id"].as_str().context("frozen feedback ID missing")?;
    uuid::Uuid::parse_str(id)?;
    anyhow::ensure!(id != operation.request_id && launch["request_id"] != id, "feedback requires a separate ID");
    Ok(json!({"cmd":"workbench.feedback.send", "workspace_id":operation.request_id,
        "session_id":launch["request_id"], "request_id":id, "text":prompt.text,
        "initial":true, "draft":prompt.mode == PromptMode::Draft, "expected_input_sequence":0, "source":null}))
}

fn matching_feedback(receipt: &Value, req: &Value) -> bool {
    ["request_id", "session_id", "text", "initial", "draft", "expected_input_sequence", "source"]
        .iter().all(|key| receipt[*key] == req[*key])
}

fn reconcile_prompt(store: &Workbench, mut operation: CreationOperation) -> Result<CreationOperation> {
    if !matches!(operation.steps[3].state, Running | Uncertain) { return Ok(operation); }
    let req = feedback_request(&operation)?;
    let receipt = store.feedback(&operation.request_id, req["request_id"].as_str().unwrap())?;
    let submitted = receipt.as_ref().is_some_and(|receipt| matching_feedback(receipt, &req)
        && receipt["state"] == "submitted" && receipt["input_sequence"] == 2);
    if !submitted && operation.steps[3].state == Uncertain { return Ok(operation); }
    operation.steps[3].state = if submitted { Succeeded } else { Uncertain };
    let missing = receipt.is_none();
    operation.steps[3].receipt = Some(match receipt {
        Some(receipt) if matching_feedback(&receipt, &req) => workbench_commands::feedback_public(receipt),
        _ => json!({"request_id":req["request_id"], "session_id":req["session_id"], "state":"uncertain", "receipt_missing":missing, "receipt_conflict":!missing}),
    });
    operation.steps[3].error = (!submitted).then(|| "initial input acknowledgement is unknown; inspect durable feedback, never resend".into());
    store.save_creation(&operation)
}

fn claim_prompt(store: &Workbench, mut operation: CreationOperation, req: &Value) -> Result<CreationOperation> {
    let desc = operation.steps[2].descriptor.clone().context("launch descriptor missing")?;
    anyhow::ensure!(req["session_id"] == desc.id, "prompt descriptor identity mismatch");
    operation.steps[3].state = Running;
    operation.steps[3].descriptor = Some(desc);
    operation.steps[3].receipt = Some(json!({"state":"intent", "request_id":req["request_id"],
        "session_id":req["session_id"], "initial":true, "expected_input_sequence":0}));
    store.save_creation(&operation)
}

fn save_draft(store: &Workbench, mut operation: CreationOperation, req: &Value) -> Result<()> {
    let id = req["request_id"].as_str().unwrap();
    let receipt = match store.feedback(&operation.request_id, id)? {
        Some(receipt) => {
            anyhow::ensure!(matching_feedback(&receipt, req) && receipt["state"] == "draft_saved"
                && receipt["stored_only"] == true, "draft ID belongs to another delivery");
            receipt
        }
        None => {
            let mut receipt = req.clone();
            receipt.as_object_mut().unwrap().remove("cmd");
            receipt["state"] = json!("draft_saved");
            receipt["stored_only"] = json!(true);
            receipt["created_at"] = json!(chrono::Utc::now().to_rfc3339());
            store.begin_feedback(&operation.request_id, id, &receipt)?;
            receipt
        }
    };
    operation.steps[3].state = Skipped;
    operation.steps[3].receipt = Some(workbench_commands::feedback_public(receipt));
    store.save_creation(&operation)?;
    Ok(())
}

fn tick_one_with(
    service: &Arc<Mutex<Service>>, id: &str,
    start: &mut impl FnMut(&Path, &Descriptor) -> Result<()>,
    observe: &mut impl FnMut(&Descriptor) -> Result<Status>,
    send: &mut impl FnMut(&Arc<Mutex<Service>>, Value) -> Result<Value>,
) -> Result<()> {
    let gates = {
        let mut state = service.lock().unwrap();
        let Some(operation) = state.workbench.creation(id, None)? else { return Ok(()); };
        if !creation_setup::is_ordinary(&operation) || !operation.frozen["launch_request"].is_object()
            || operation.ready() { return Ok(()); }
        let sid = launch_request(&operation)?["request_id"].as_str().unwrap().to_owned();
        [format!("creation:{id}"), format!("workspace:{id}"), format!("session:{sid}")]
            .map(|key| state.terminal_gates.entry(key).or_default().clone())
    };
    let Ok(_creation) = gates[0].try_lock() else { return Ok(()); };
    let Ok(_workspace) = gates[1].try_lock() else { return Ok(()); };
    let Ok(session_guard) = gates[2].try_lock() else { return Ok(()); };
    let execution = { prepare(&mut service.lock().unwrap(), id)? };
    if let Some(execution) = execution {
        let observation = if let Some(desc) = &execution.operation.steps[2].descriptor {
            let started = if execution.spawn { start(&execution.executable, desc) } else { Ok(()) };
            observe(desc).map_err(|error| match started {
                Err(start) => error.context(format!("launch was not confirmed: {start:#}")),
                Ok(()) => error,
            })
        } else { Err(anyhow::anyhow!("running launch has no durable descriptor")) };
        let mut state = service.lock().unwrap();
        if execution.spawn {
            let sid = launch_request(&execution.operation)?["request_id"].as_str().unwrap();
            state.terminal_inflight.remove(&format!("starting:{sid}"));
            state.terminal_inflight.remove(&format!("session:{sid}"));
        }
        if let Some(current) = state.workbench.creation(id, None)? {
            if current.revision == execution.operation.revision {
                reconcile_launch(&state.workbench, current, observation)?;
            }
        }
    }
    let (operation, req, desc) = {
        let state = service.lock().unwrap();
        let Some(operation) = state.workbench.creation(id, None)? else { return Ok(()); };
        if operation.steps[2].state != Succeeded { return Ok(()); }
        if matches!(operation.steps[3].state, Running | Uncertain) {
            reconcile_prompt(&state.workbench, operation)?;
            return Ok(());
        }
        if operation.steps[3].state != Pending { return Ok(()); }
        current_workspace(&state, &operation)?;
        let req = feedback_request(&operation)?;
        if req["draft"] == true {
            save_draft(&state.workbench, operation, &req)?;
            return Ok(());
        }
        let desc = operation.steps[2].descriptor.clone().context("launch descriptor missing")?;
        let session = state.workbench.session(id, &desc.id)?.context("creation session missing")?;
        anyhow::ensure!(session.task_id.is_none() && serde_json::to_value(&session.descriptor)? == serde_json::to_value(&desc)?, "creation session changed; initial prompt not sent");
        anyhow::ensure!(native_prompt::saved(&desc)? == native_prompt::requested(launch_request(&operation)?)?, "initial prompt contract changed");
        if state.workbench.feedback(id, req["request_id"].as_str().unwrap())?.is_some() {
            let operation = claim_prompt(&state.workbench, operation, &req)?;
            reconcile_prompt(&state.workbench, operation)?;
            return Ok(());
        }
        (operation, req, desc)
    };
    // Login, trust, permission prompts and unknown readiness remain Pending.
    let Ok(status) = observe(&desc) else { return Ok(()); };
    if native_prompt::validate_feedback(&desc, req["text"].as_str().unwrap(), false).is_err()
        || operation.steps[2].receipt.as_ref().and_then(|r| r["status"]["pid"].as_u64()) != Some(u64::from(status.pid))
        || host::require_input_ready(&status, 0, req["request_id"].as_str()).is_err() { return Ok(()); }
    {
        let state = service.lock().unwrap();
        let workspace = current_workspace(&state, &operation)?;
        anyhow::ensure!(desc.spawn.cwd.as_deref() == dunce::canonicalize(&workspace.path)?.to_str(), "creation session working directory changed");
        claim_prompt(&state.workbench, operation, &req)?;
    }
    // Dispatch owns the session gate throughout paste + Enter, rechecking the
    // sequence/readiness at the host. Never hold Service or recursively lock it.
    drop(session_guard);
    let _response = send(service, req);
    let state = service.lock().unwrap();
    if let Some(current) = state.workbench.creation(id, None)? { reconcile_prompt(&state.workbench, current)?; }
    Ok(())
}

pub(super) fn tick(service: &Arc<Mutex<Service>>) -> Result<()> {
    let ids = { service.lock().unwrap().workbench.creation_operations()?.into_iter()
        .filter(creation_setup::is_ordinary).map(|op| op.request_id).collect::<Vec<_>>() };
    let mut first_error = None;
    for id in ids {
        if let Err(error) = tick_one_with(service, &id, &mut start_host, &mut get_status, &mut terminal_io::dispatch) {
            if first_error.is_none() { first_error = Some(error.context(format!("workspace continuation {id}"))); }
        }
    }
    first_error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::Session;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("velella-creation-launch-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(root.join("project")).unwrap();
            Self(root)
        }
        fn service(&self) -> Service {
            Service {
                root: self.0.clone(), executable: self.0.join("never-execute-fixture"),
                projects: vec![Project { id:"p".into(), path:self.0.join("project"), name:"fixture".into(),
                    setup:"".into(), setup_default:SetupPolicy::Skip, default_base:None, direct:false, terminals:vec![] }],
                tasks:vec![], project_locks:HashMap::new(), workbench:Workbench::open(&self.0).unwrap(),
                capabilities:capabilities::Capabilities::open(&self.0).unwrap(), terminal_gates:HashMap::new(),
                terminal_inflight:Default::default(), terminal_epoch:0, collaboration:Arc::new(Mutex::new(None)),
            }
        }
        fn accepted(&self, service: &Service, mut req: Value) -> CreationOperation {
            let id = uuid::Uuid::now_v7().to_string();
            req["request_id"] = json!(id);
            req["cmd"] = json!("workbench.create");
            req["project_id"] = json!("p");
            req["name"] = json!("fixture");
            let workspace = Workspace { id:id.clone(), project_id:"p".into(), name:"fixture".into(),
                path:self.0.join(format!("workspace-{id}")), kind:"worktree".into(), archived:false,
                favorite:false, position:0, parent_workspace_id:None };
            std::fs::create_dir(&workspace.path).unwrap();
            service.workbench.put_workspace(&workspace).unwrap();
            let setup = creation::setup_snapshot(&service.projects[0], &req, false).unwrap();
            let launch = freeze(&req, &id).unwrap();
            let op = service.workbench.accept_creation(&id, &req, &json!({"workspace":workspace,
                "project_path":service.projects[0].path, "setup":setup, "launch_request":launch})).unwrap();
            let op = transition(service, op, 0, Running);
            transition(service, op, 0, Succeeded)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }
    fn transition(service: &Service, mut op: CreationOperation, index: usize, state: CreationStepState) -> CreationOperation {
        op.steps[index].state = state;
        service.workbench.save_creation(&op).unwrap()
    }
    fn load(service: &Arc<Mutex<Service>>, id: &str) -> CreationOperation {
        service.lock().unwrap().workbench.creation(id, None).unwrap().unwrap()
    }
    fn document(op: &CreationOperation) -> Value { serde_json::to_value(op).unwrap() }
    fn preparation_failed(state: &Service, op: &CreationOperation, error: &str) -> CreationOperation {
        let saved = state.workbench.creation(&op.request_id, None).unwrap().unwrap();
        assert_eq!(saved.request, op.request);
        assert_eq!(saved.frozen, op.frozen);
        assert_eq!(saved.steps[0].state, Succeeded);
        assert!(matches!(saved.steps[1].state, Succeeded | Skipped));
        assert_eq!(saved.steps[2].state, Failed);
        assert_eq!(saved.steps[2].attempt, 0);
        assert!(saved.steps[2].descriptor.is_none());
        assert!(saved.steps[2].error.as_ref().unwrap().contains(error));
        assert_eq!(saved.steps[2].receipt.as_ref().unwrap(), &json!({"state":"preparation_intent",
            "session_id":op.frozen["launch_request"]["request_id"], "no_spawn":true, "registration_only":false}));
        assert_eq!(saved.steps[3].state, Pending);
        assert!(!saved.ready());
        assert!(state.terminal_inflight.is_empty());
        saved
    }
    fn ready() -> Status {
        Status { pid:123, state:"waiting_input".into(), input_protocol:1, input_ready_protocol:1,
            input_sequence:Some(0), hooks_ready:true, native_id:Some("fixture-native".into()), ..Status::default() }
    }
    fn no_start(_: &Path, _: &Descriptor) -> Result<()> { panic!("unexpected spawn") }
    fn no_send(_: &Arc<Mutex<Service>>, _: Value) -> Result<Value> { panic!("unexpected input") }
    fn no_observe(_: &Descriptor) -> Result<Status> { panic!("unexpected host observation") }
    fn prompt(mode: &str) -> Value {
        json!({"launch":"codex", "initial_prompt":{"text":"fixture text", "mode":mode}})
    }
    fn stored_feedback(service: &Arc<Mutex<Service>>, req: &Value, state: &str) {
        let mut receipt = req.clone();
        receipt["state"] = json!(state);
        receipt["input_sequence"] = json!(2);
        service.lock().unwrap().workbench.begin_feedback(req["workspace_id"].as_str().unwrap(),
            req["request_id"].as_str().unwrap(), &receipt).unwrap();
    }

    #[test]
    fn creation_launch_contract_rejects_before_checkout_and_freezes_separate_ids() {
        let fixture = Fixture::new();
        let mut service = fixture.service();
        for mut req in [json!({"launch":null}), json!({"launch":{"program":"shell"}}),
            json!({"launch":"shell"}), json!({"launch":"arbitrary"}),
            json!({"launch_options":{"model":"m"}}), json!({"launch":"terminal","launch_options":{"model":"m"}}),
            json!({"launch":"gemini","initial_prompt":{"text":"x","mode":"submit"}}),
            json!({"launch":"codex","launch_options":{"argv":[]}}),
            json!({"launch":"codex","launch_options":{"model":"--help"}}),
            json!({"launch":"codex","initial_prompt":{"text":"x","mode":"unknown"}}),
            json!({"launch":"claude-code","execution":{"mode":"restricted"}}),
            json!({"launch":"codex","execution":{"mode":"isolated"}}),
            json!({"launch":"codex","execution":{"mode":"restricted","network":"denied"}}),
            json!({"launch":"codex","execution":{"mode":"restricted"},"launch_options":{"model":"m"}}),
            json!({"launch":"codex","execution":{"mode":"restricted"},"initial_prompt":{"text":"x","mode":"draft"}}),
            json!({"launch":"codex","sandbox":{"mode":"native"}})] {
            let id = uuid::Uuid::now_v7().to_string();
            req["request_id"] = json!(id); req["project_id"] = json!("p"); req["name"] = json!("fixture");
            let error = service.create_workbench(req).unwrap_err().to_string();
            assert!(!error.contains("Git") && !error.contains("git"), "{error}");
            assert!(service.workbench.creation(&id, None).unwrap().is_none());
        }
        for launch in ["terminal", "codex", "claude-code", "gemini", "opencode", "omp", "kimi", "zcode"] {
            assert!(freeze(&json!({"launch":launch}), "w").unwrap().is_some());
            assert!(freeze(&json!({"launch":launch,"launch_options":{}}), "w").unwrap().is_some());
        }
        assert!(freeze(&json!({}), "w").unwrap().is_none());
        assert!(freeze(&json!({"launch":"none"}), "w").unwrap().is_none());
        assert_eq!(freeze(&json!({"launch":"omp","permission_mode":"write"}),"w").unwrap().unwrap()["permission_mode"],"write");
        assert!(freeze(&json!({"launch":"codex","permission_mode":"full-access","execution":{"mode":"restricted"}}),"w").is_err());
        assert!(freeze(&json!({"launch":"zcode","permission_mode":"plan"}),"w").is_err());
        assert!(freeze(&json!({"launch":"codex","permission_mode":null}),"w").is_err());
        assert!(freeze(&json!({"launch":"codex","execution":{"mode":"restricted"}}), "w").is_ok());
        let op = fixture.accepted(&service, prompt("submit"));
        let req = launch_request(&op).unwrap();
        assert_ne!(req["request_id"], op.request_id);
        assert_ne!(req["feedback_request_id"], req["request_id"]);
        assert_ne!(req["feedback_request_id"], op.request_id);
        let frozen = op.frozen.clone();
        let replay = service.create_workbench(op.request.clone()).unwrap();
        assert_eq!(replay["recovered"], true);
        assert_eq!(replay["creation"]["frozen"], frozen);
        assert_eq!(replay["creation"]["steps"][2]["state"], "pending");
    }

    #[test]
    fn no_config_keeps_skips_and_failed_setup_blocks_launch() {
        let fixture = Fixture::new();
        let mut state = fixture.service();
        let plain = fixture.accepted(&state, json!({}));
        assert!(state.create_workbench(plain.request).unwrap()["creation"]["steps"].as_array().unwrap()[1..]
            .iter().all(|step| step["state"] == "skipped"));
        state.projects[0].setup = "exit 7".into();
        let op = fixture.accepted(&state, json!({"launch":"terminal","setup":"run","reviewed_setup":"exit 7"}));
        let op = transition(&state, op, 1, Running);
        let op = transition(&state, op, 1, Failed);
        let service = Arc::new(Mutex::new(state));
        tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
        assert_eq!(document(&load(&service, &op.request_id)), document(&op));
    }

    #[test]
    fn descriptor_and_session_are_durable_before_spawn_and_replay_never_spawns() {
        let fixture = Fixture::new();
        let state = fixture.service();
        let op = fixture.accepted(&state, json!({"launch":"terminal"}));
        let service = Arc::new(Mutex::new(state));
        let mut count = 0;
        tick_one_with(&service, &op.request_id, &mut |_, desc| {
            count += 1;
            let state = service.try_lock().expect("spawn must not own Service");
            let saved = state.workbench.creation(&op.request_id, None)?.unwrap();
            assert_eq!(saved.steps[2].state, Running);
            assert_eq!(serde_json::to_value(saved.steps[2].descriptor.as_ref().unwrap())?, serde_json::to_value(desc)?);
            assert_eq!(serde_json::to_value(state.workbench.session(&op.request_id, &desc.id)?.unwrap().descriptor)?, serde_json::to_value(desc)?);
            for key in [format!("starting:{}", desc.id), format!("session:{}", desc.id)] { assert!(state.terminal_inflight.contains(&key)); }
            Ok(())
        }, &mut |_| { assert!(service.try_lock().is_ok()); Ok(ready()) }, &mut no_send).unwrap();
        assert_eq!(count, 1);
        let saved = load(&service, &op.request_id);
        assert!(saved.ready());
        assert_eq!(saved.steps[3].state, Skipped);
        assert!(service.lock().unwrap().terminal_inflight.is_empty());
        service.lock().unwrap().projects.clear();
        let replay = service.lock().unwrap().create_workbench(op.request.clone()).unwrap();
        assert_eq!(replay["recovered"], true);
        assert_eq!(replay["creation"], document(&saved));
        let mut changed = op.request; changed["launch"] = json!("codex");
        assert!(service.lock().unwrap().create_workbench(changed).is_err());
        tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
    }

    #[test]
    fn prior_registration_and_failed_journal_write_are_observation_only() {
        let fixture = Fixture::new();
        let mut state = fixture.service();
        let op = fixture.accepted(&state, json!({"launch":"terminal"}));
        creation_setup::settle_skips(&state.workbench, op.clone()).unwrap();
        let conn = Connection::open(fixture.0.join("workbench.db")).unwrap();
        conn.execute_batch("CREATE TRIGGER reject_launch_journal BEFORE UPDATE ON creation_operations
            BEGIN SELECT RAISE(ABORT, 'fixture journal failure'); END;").unwrap();
        assert!(prepare(&mut state, &op.request_id).is_err());
        assert!(state.terminal_inflight.is_empty());
        let desc = state.workbench.sessions(Some(&op.request_id)).unwrap()[0].descriptor.clone();
        assert_eq!(state.workbench.creation(&op.request_id, None).unwrap().unwrap().steps[2].state, Pending);
        conn.execute_batch("DROP TRIGGER reject_launch_journal;").unwrap();
        let execution = prepare(&mut state, &op.request_id).unwrap().unwrap();
        assert!(!execution.spawn);
        assert_eq!(execution.operation.steps[2].state, Uncertain);
        assert_eq!(serde_json::to_value(&execution.operation.steps[2].descriptor).unwrap(), serde_json::to_value(Some(desc)).unwrap());
        let service = Arc::new(Mutex::new(state));
        tick_one_with(&service, &op.request_id, &mut no_start, &mut |_| anyhow::bail!("no host"), &mut no_send).unwrap();
        assert_eq!(load(&service, &op.request_id).steps[2].state, Uncertain);
        tick_one_with(&service, &op.request_id, &mut no_start, &mut |_| Ok(ready()), &mut no_send).unwrap();
        assert!(load(&service, &op.request_id).ready());
    }

    #[test]
    fn recovered_running_and_uncertain_never_restart_the_host() {
        let fixture = Fixture::new();
        let mut state = fixture.service();
        let op = fixture.accepted(&state, json!({"launch":"terminal"}));
        let execution = prepare(&mut state, &op.request_id).unwrap().unwrap();
        assert!(execution.spawn);
        drop(state);
        let service = Arc::new(Mutex::new(fixture.service()));
        for _ in 0..2 {
            tick_one_with(&service, &op.request_id, &mut no_start, &mut |_| anyhow::bail!("no original host"), &mut no_send).unwrap();
            assert_eq!(load(&service, &op.request_id).steps[2].state, Uncertain);
        }
        tick_one_with(&service, &op.request_id, &mut no_start, &mut |_| Ok(Status { exit:Some(7), ..ready() }), &mut no_send).unwrap();
        assert_eq!(load(&service, &op.request_id).steps[2].state, Failed);
        tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
    }

    #[test]
    fn draft_is_saved_without_readiness_paste_or_enter() {
        let fixture = Fixture::new();
        let state = fixture.service();
        let op = fixture.accepted(&state, prompt("draft"));
        let service = Arc::new(Mutex::new(state));
        let mut observations = 0;
        tick_one_with(&service, &op.request_id, &mut |_, _| Ok(()), &mut |_| {
            observations += 1;
            Ok(Status { state:"waiting_permission".into(), permission_input:true, ..ready() })
        }, &mut no_send).unwrap();
        assert_eq!(observations, 1);
        let saved = load(&service, &op.request_id);
        assert!(saved.ready());
        assert_eq!(saved.steps[3].state, Skipped);
        assert_eq!(saved.steps[3].receipt.as_ref().unwrap()["state"], "draft_saved");
        let receipt = service.lock().unwrap().workbench.feedback(&op.request_id,
            op.frozen["launch_request"]["feedback_request_id"].as_str().unwrap()).unwrap().unwrap();
        assert_eq!(receipt["text"], "fixture text");
        assert_eq!(receipt["stored_only"], true);
        assert!(receipt.get("input_sequence").is_none());
        tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
    }

    #[test]
    fn unsafe_readiness_and_permissions_leave_submit_pending() {
        let fixture = Fixture::new();
        let state = fixture.service();
        let op = fixture.accepted(&state, prompt("submit"));
        let service = Arc::new(Mutex::new(state));
        tick_one_with(&service, &op.request_id, &mut |_, _| Ok(()), &mut |_| Ok(Status { state:"starting".into(), ..ready() }), &mut no_send).unwrap();
        for field in ["permission", "blocked", "protocol", "sequence", "human", "typed", "hooks", "native", "exit", "pid", "unreachable"] {
            tick_one_with(&service, &op.request_id, &mut no_start, &mut |_| {
                let mut status = ready();
                match field {
                    "permission" => { status.state = "waiting_permission".into(); status.permission_input = true; }
                    "blocked" => status.initial_prompt_blocked = true,
                    "protocol" => status.input_ready_protocol = 0,
                    "sequence" => status.input_sequence = Some(1),
                    "human" => status.human_control = true,
                    "typed" => status.typed_control = true,
                    "hooks" => status.hooks_ready = false,
                    "native" => status.native_id = None,
                    "exit" => status.exit = Some(0),
                    "pid" => status.pid = 456,
                    _ => anyhow::bail!("unreachable"),
                }
                Ok(status)
            }, &mut no_send).unwrap();
            let saved = load(&service, &op.request_id);
            assert_eq!(saved.steps[3].state, Pending, "{field}");
            assert!(saved.steps[3].descriptor.is_none());
        }
    }

    #[test]
    fn submit_intent_precedes_dispatch_and_only_durable_acknowledgement_succeeds() {
        let fixture = Fixture::new();
        let state = fixture.service();
        let op = fixture.accepted(&state, prompt("submit"));
        let service = Arc::new(Mutex::new(state));
        let mut sends = 0;
        tick_one_with(&service, &op.request_id, &mut |_, _| Ok(()), &mut |_| Ok(ready()), &mut |service, req| {
            sends += 1;
            let saved = load(service, &op.request_id);
            assert_eq!(saved.steps[3].state, Running);
            assert_eq!(saved.steps[3].receipt.as_ref().unwrap()["state"], "intent");
            assert_eq!(req, feedback_request(&saved).unwrap());
            assert_eq!(serde_json::to_value(&saved.steps[2].descriptor)?, serde_json::to_value(&saved.steps[3].descriptor)?);
            let gate = service.lock().unwrap().terminal_gates[&format!("session:{}", req["session_id"].as_str().unwrap())].clone();
            let _session = gate.try_lock().expect("dispatch must be able to acquire the session queue");
            stored_feedback(service, &req, "submitted");
            anyhow::bail!("lost response after durable acknowledgement")
        }).unwrap();
        assert_eq!(sends, 1);
        assert!(load(&service, &op.request_id).ready());
        assert!(load(&service, &op.request_id).steps[3].receipt.as_ref().unwrap().get("accepted").is_none());
        tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
    }

    #[test]
    fn missing_feedback_receipt_stays_unknown_across_restart_never_transparent_replay() {
        let fixture = Fixture::new();
        let state = fixture.service();
        let op = fixture.accepted(&state, prompt("submit"));
        let service = Arc::new(Mutex::new(state));
        tick_one_with(&service, &op.request_id, &mut |_, _| Ok(()), &mut |_| Ok(ready()),
            &mut |_, _| Ok(json!({"ok":true,"receipt":{"state":"submitted"}}))).unwrap();
        let unknown = load(&service, &op.request_id);
        assert_eq!(unknown.steps[3].state, Uncertain);
        assert_eq!(unknown.steps[3].receipt.as_ref().unwrap()["receipt_missing"], true);
        drop(service);
        let service = Arc::new(Mutex::new(fixture.service()));
        for _ in 0..2 {
            tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
            assert_eq!(document(&load(&service, &op.request_id)), document(&unknown));
        }
        stored_feedback(&service, &feedback_request(&unknown).unwrap(), "submitted");
        tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
        assert!(load(&service, &op.request_id).ready());
    }

    #[test]
    fn interrupted_prompt_claim_reconciles_receipts_only_even_if_host_is_ready() {
        let fixture = Fixture::new();
        let mut state = fixture.service();
        let op = fixture.accepted(&state, prompt("submit"));
        let execution = prepare(&mut state, &op.request_id).unwrap().unwrap();
        let launched = reconcile_launch(&state.workbench, execution.operation, Ok(ready())).unwrap();
        claim_prompt(&state.workbench, launched.clone(), &feedback_request(&launched).unwrap()).unwrap();
        drop(state);
        let service = Arc::new(Mutex::new(fixture.service()));
        tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
        assert_eq!(load(&service, &op.request_id).steps[3].state, Uncertain);
    }

    #[test]
    fn interrupted_or_conflicting_feedback_is_never_inferred_from_host_readiness() {
        for outcome in ["sending", "uncertain", "wrong_sequence", "wrong_text"] {
            let fixture = Fixture::new();
            let state = fixture.service();
            let op = fixture.accepted(&state, prompt("submit"));
            let service = Arc::new(Mutex::new(state));
            tick_one_with(&service, &op.request_id, &mut |_, _| Ok(()), &mut |_| Ok(ready()), &mut |service, req| {
                let mut receipt = req.clone();
                receipt["state"] = json!(if outcome.starts_with("wrong") { "submitted" } else { outcome });
                receipt["input_sequence"] = json!(if outcome == "wrong_sequence" { 1 } else { 2 });
                if outcome == "wrong_text" { receipt["text"] = json!("another request"); }
                service.lock().unwrap().workbench.begin_feedback(&op.request_id, req["request_id"].as_str().unwrap(), &receipt)?;
                Ok(json!({"ok":true,"receipt":receipt}))
            }).unwrap();
            assert_eq!(load(&service, &op.request_id).steps[3].state, Uncertain, "{outcome}");
            tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
        }
    }

    #[test]
    fn draft_receipt_survives_crash_before_journal_without_pasting() {
        let fixture = Fixture::new();
        let mut state = fixture.service();
        let op = fixture.accepted(&state, prompt("draft"));
        let execution = prepare(&mut state, &op.request_id).unwrap().unwrap();
        let op = reconcile_launch(&state.workbench, execution.operation, Ok(ready())).unwrap();
        let req = feedback_request(&op).unwrap();
        let conn = Connection::open(fixture.0.join("workbench.db")).unwrap();
        conn.execute_batch("CREATE TRIGGER reject_draft_journal BEFORE UPDATE ON creation_operations
            BEGIN SELECT RAISE(ABORT, 'fixture journal failure'); END;").unwrap();
        assert!(save_draft(&state.workbench, op.clone(), &req).is_err());
        assert!(state.workbench.feedback(&op.request_id, req["request_id"].as_str().unwrap()).unwrap().is_some());
        conn.execute_batch("DROP TRIGGER reject_draft_journal;").unwrap();
        drop(state);
        let service = Arc::new(Mutex::new(fixture.service()));
        tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
        assert!(load(&service, &op.request_id).ready());
        assert_eq!(load(&service, &op.request_id).steps[3].receipt.as_ref().unwrap()["state"], "draft_saved");
    }

    #[test]
    fn failed_prompt_journal_write_never_dispatches_input() {
        let fixture = Fixture::new();
        let mut state = fixture.service();
        let op = fixture.accepted(&state, prompt("submit"));
        let execution = prepare(&mut state, &op.request_id).unwrap().unwrap();
        let op = reconcile_launch(&state.workbench, execution.operation, Ok(ready())).unwrap();
        let conn = Connection::open(fixture.0.join("workbench.db")).unwrap();
        conn.execute_batch("CREATE TRIGGER reject_prompt_journal BEFORE UPDATE ON creation_operations
            BEGIN SELECT RAISE(ABORT, 'fixture journal failure'); END;").unwrap();
        let service = Arc::new(Mutex::new(state));
        assert!(tick_one_with(&service, &op.request_id, &mut no_start, &mut |_| Ok(ready()), &mut no_send).is_err());
        assert_eq!(load(&service, &op.request_id).steps[3].state, Pending);
        assert!(service.lock().unwrap().workbench.feedback(&op.request_id,
            op.frozen["launch_request"]["feedback_request_id"].as_str().unwrap()).unwrap().is_none());
    }

    #[test]
    fn creation_workspace_and_session_gates_use_nonblocking_acquisition() {
        for key in ["creation", "workspace", "session"] {
            let fixture = Fixture::new();
            let mut state = fixture.service();
            let op = fixture.accepted(&state, json!({"launch":"terminal"}));
            let id = if key == "session" { op.frozen["launch_request"]["request_id"].as_str().unwrap() } else { &op.request_id };
            let gate = state.terminal_gates.entry(format!("{key}:{id}")).or_default().clone();
            let _guard = gate.lock().unwrap();
            let service = Arc::new(Mutex::new(state));
            tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
            assert_eq!(document(&load(&service, &op.request_id)), document(&op));
        }
    }

    #[test]
    fn archived_missing_or_changed_workspace_failure_is_visible_and_never_retried() {
        for change in ["archive", "missing_workspace", "missing_directory", "path", "project", "project_path"] {
            let fixture = Fixture::new();
            let mut state = fixture.service();
            let op = fixture.accepted(&state, json!({"launch":"terminal"}));
            let workspace = state.workbench.workspace(&op.request_id).unwrap();
            let mut changed = workspace.clone();
            let error = match change {
                "archive" => { changed.archived = true; "archived or unavailable" }
                "missing_directory" => { std::fs::remove_dir(&workspace.path).unwrap(); "archived or unavailable" }
                "path" => { changed.path = fixture.0.join("project"); "no longer matches" }
                "project" => { changed.project_id = "different".into(); "no longer matches" }
                "project_path" => { state.projects[0].path = fixture.0.clone(); "project path changed" }
                _ => "workspace not found",
            };
            state.workbench.put_workspace(&changed).unwrap();
            if change == "missing_workspace" {
                Connection::open(fixture.0.join("workbench.db")).unwrap()
                    .execute("DELETE FROM workspaces WHERE id=?1", [&op.request_id]).unwrap();
            }
            assert!(prepare_with(&mut state, &op.request_id, &mut |_, _| panic!("workspace must be checked first")).unwrap().is_none());
            let saved = preparation_failed(&state, &op, error);
            assert!(state.workbench.sessions(None).unwrap().is_empty());
            state.workbench.put_workspace(&workspace).unwrap();
            std::fs::create_dir_all(&workspace.path).unwrap();
            state.projects[0].path = fixture.0.join("project");
            assert_eq!(state.create_workbench(op.request.clone()).unwrap()["creation"], document(&saved));
            drop(state);
            let service = Arc::new(Mutex::new(fixture.service()));
            for _ in 0..2 {
                tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
                assert_eq!(document(&load(&service, &op.request_id)), document(&saved));
                assert!(service.lock().unwrap().workbench.sessions(None).unwrap().is_empty());
            }
        }
    }

    #[test]
    fn preflight_program_and_path_errors_persist_no_spawn_failure() {
        for error in ["unsupported launch program", "launch program not found", "preflight path unavailable"] {
            let fixture = Fixture::new();
            let mut state = fixture.service();
            let op = fixture.accepted(&state, json!({"launch":"terminal"}));
            let pending = creation_setup::settle_skips(&state.workbench, op.clone()).unwrap();
            let mut calls = 0;
            assert!(prepare_with(&mut state, &op.request_id, &mut |state, req| {
                calls += 1;
                assert_eq!(req, launch_request(&op).unwrap());
                assert_eq!(state.workbench.creation(&op.request_id, None)?.unwrap().steps[2].state, Pending);
                Err(anyhow::anyhow!("fixture preflight source")).context(error)
            }).unwrap().is_none());
            assert_eq!(calls, 1);
            let saved = preparation_failed(&state, &op, error);
            assert!(saved.steps[2].error.as_ref().unwrap().contains("fixture preflight source"));
            assert_eq!(saved.revision, pending.revision + 2);
            assert!(state.workbench.sessions(None).unwrap().is_empty());
            assert!(prepare_with(&mut state, &op.request_id, &mut |_, _| panic!("no implicit retry")).unwrap().is_none());
            drop(state);
            let service = Arc::new(Mutex::new(fixture.service()));
            for _ in 0..2 {
                tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
                let mut state = service.lock().unwrap();
                assert_eq!(state.create_workbench(op.request.clone()).unwrap()["creation"], document(&saved));
                assert!(state.workbench.sessions(None).unwrap().is_empty());
            }
        }
    }

    #[test]
    fn registration_surviving_preparation_error_keeps_original_descriptor_and_only_observes() {
        for failure in ["event", "archive", "missing_directory"] {
            let fixture = Fixture::new();
            let mut state = fixture.service();
            let op = fixture.accepted(&state, json!({"launch":"terminal"}));
            let pending = creation_setup::settle_skips(&state.workbench, op.clone()).unwrap();
            let conn = Connection::open(fixture.0.join("workbench.db")).unwrap();
            if failure == "event" {
                conn.execute_batch("CREATE TRIGGER reject_launch_event BEFORE INSERT ON events
                    WHEN NEW.kind='terminal.launch_requested'
                    BEGIN SELECT RAISE(ABORT, 'fixture event failure after registration'); END;").unwrap();
            } else {
                state.prepare_workbench_launch(launch_request(&op).unwrap()).unwrap();
                let mut workspace = state.workbench.workspace(&op.request_id).unwrap();
                if failure == "archive" {
                    workspace.archived = true;
                    state.workbench.put_workspace(&workspace).unwrap();
                } else { std::fs::remove_dir(&workspace.path).unwrap(); }
            }
            let execution = prepare(&mut state, &op.request_id).unwrap().unwrap();
            assert!(!execution.spawn);
            assert!(state.terminal_inflight.is_empty());
            let saved = execution.operation;
            assert_eq!(saved.revision, pending.revision + 2);
            assert_eq!(saved.steps[2].state, Uncertain);
            assert_eq!(saved.steps[2].attempt, 0);
            let error = saved.steps[2].error.as_ref().unwrap();
            assert!(error.contains(if failure == "event" { "fixture event failure after registration" } else { "archived or unavailable" }));
            assert!(error.contains("observe original host only"));
            assert_eq!(saved.steps[2].receipt.as_ref().unwrap()["no_spawn"], true);
            assert_eq!(saved.steps[2].receipt.as_ref().unwrap()["registration_only"], true);
            let original = state.workbench.session(&op.request_id, launch_request(&op).unwrap()["request_id"].as_str().unwrap())
                .unwrap().unwrap().descriptor;
            assert_eq!(serde_json::to_value(&saved.steps[2].descriptor).unwrap(), serde_json::to_value(Some(&original)).unwrap());
            assert_eq!(state.create_workbench(op.request.clone()).unwrap()["creation"], document(&saved));
            drop(conn);
            drop(state);
            let service = Arc::new(Mutex::new(fixture.service()));
            service.lock().unwrap().projects.clear();
            for _ in 0..2 {
                let execution = prepare_with(&mut service.lock().unwrap(), &op.request_id,
                    &mut |_, _| panic!("registered creation must never prepare again")).unwrap().unwrap();
                assert!(!execution.spawn);
                tick_one_with(&service, &op.request_id, &mut no_start, &mut |desc| {
                    assert!(service.try_lock().is_ok());
                    assert_eq!(serde_json::to_value(desc)?, serde_json::to_value(&original)?);
                    anyhow::bail!("original host unavailable")
                }, &mut no_send).unwrap();
                assert_eq!(document(&load(&service, &op.request_id)), document(&saved));
                let state = service.lock().unwrap();
                assert_eq!(state.workbench.sessions(None).unwrap().len(), 1);
                assert_eq!(serde_json::to_value(state.workbench.session(&op.request_id, &original.id).unwrap().unwrap().descriptor).unwrap(),
                    serde_json::to_value(&original).unwrap());
            }
            tick_one_with(&service, &op.request_id, &mut no_start, &mut |_| Ok(ready()), &mut no_send).unwrap();
            assert!(load(&service, &op.request_id).ready());
        }
    }

    #[test]
    fn interrupted_no_spawn_intent_is_never_prepared_or_given_a_descriptor_on_reopen() {
        let fixture = Fixture::new();
        let mut state = fixture.service();
        let op = fixture.accepted(&state, json!({"launch":"terminal"}));
        creation_setup::settle_skips(&state.workbench, op.clone()).unwrap();
        let conn = Connection::open(fixture.0.join("workbench.db")).unwrap();
        conn.execute_batch("CREATE TRIGGER reject_preparation_completion BEFORE UPDATE ON creation_operations
            WHEN json_extract(OLD.document, '$.steps[2].state')='running'
            BEGIN SELECT RAISE(ABORT, 'fixture completion write failure'); END;").unwrap();
        assert!(prepare_with(&mut state, &op.request_id, &mut |_, _| anyhow::bail!("fixture preflight failed")).is_err());
        let intent = state.workbench.creation(&op.request_id, None).unwrap().unwrap();
        assert_eq!(intent.steps[2].state, Running);
        assert!(intent.steps[2].descriptor.is_none());
        assert_eq!(intent.steps[2].receipt.as_ref().unwrap()["no_spawn"], true);
        assert!(intent.steps[2].error.as_ref().unwrap().contains("fixture preflight failed"));
        conn.execute_batch("DROP TRIGGER reject_preparation_completion;").unwrap();
        drop(conn);
        drop(state);
        let service = Arc::new(Mutex::new(fixture.service()));
        let execution = prepare_with(&mut service.lock().unwrap(), &op.request_id,
            &mut |_, _| panic!("running intent must not prepare again")).unwrap().unwrap();
        assert!(!execution.spawn);
        assert_eq!(document(&execution.operation), document(&intent));
        for _ in 0..2 {
            tick_one_with(&service, &op.request_id, &mut no_start, &mut no_observe, &mut no_send).unwrap();
            let saved = load(&service, &op.request_id);
            assert_eq!(saved.steps[2].state, Uncertain);
            assert!(saved.steps[2].descriptor.is_none());
            assert_eq!(saved.steps[2].receipt, intent.steps[2].receipt);
            assert_eq!(saved.steps[2].attempt, 0);
            assert!(service.lock().unwrap().workbench.sessions(None).unwrap().is_empty());
        }
    }

    #[test]
    fn corrupt_frozen_launch_identity_is_not_converted_to_preparation_failure() {
        let fixture = Fixture::new();
        let mut state = fixture.service();
        let op = fixture.accepted(&state, json!({"launch":"terminal"}));
        let pending = creation_setup::settle_skips(&state.workbench, op).unwrap();
        let conn = Connection::open(fixture.0.join("workbench.db")).unwrap();
        for (field, value) in [("cmd", json!("workbench.terminal.recover")),
            ("workspace_id", json!(uuid::Uuid::now_v7().to_string())), ("request_id", Value::Null),
            ("request_id", json!("invalid-session-id")), ("request_id", json!(pending.request_id))] {
            let mut corrupt = document(&pending);
            corrupt["frozen"]["launch_request"][field] = value;
            conn.execute("UPDATE creation_operations SET document=?2 WHERE id=?1",
                params![pending.request_id, corrupt.to_string()]).unwrap();
            assert!(prepare_with(&mut state, &pending.request_id, &mut |_, _| panic!("invalid identity must not prepare")).is_err());
            assert_eq!(document(&state.workbench.creation(&pending.request_id, None).unwrap().unwrap()), corrupt);
            assert!(state.workbench.sessions(None).unwrap().is_empty());
            assert!(state.terminal_inflight.is_empty());
        }
    }

    #[test]
    fn terminal_cap_failure_is_visible_without_registering_another_session() {
        let fixture = Fixture::new();
        let mut state = fixture.service();
        let op = fixture.accepted(&state, json!({"launch":"terminal"}));
        for _ in 0..32 {
            let id = uuid::Uuid::now_v7().to_string();
            state.workbench.put_session(&Session { id:id.clone(), workspace_id:op.request_id.clone(), title:"fixture".into(),
                kind:"terminal".into(), task_id:None, native_id:None,
                descriptor:Descriptor { id:id.clone(), endpoint:"fixture-only".into(), directory:fixture.0.join(&id),
                    spawn:SpawnRequest { program:"fixture".into(), args:vec![], cwd:None, env:HashMap::new(), adapter:None, sandbox:Default::default() } } }).unwrap();
            state.terminal_inflight.insert(format!("session:{id}"));
        }
        assert!(prepare(&mut state, &op.request_id).unwrap().is_none());
        assert_eq!(state.terminal_inflight.len(), 32);
        state.terminal_inflight.clear();
        preparation_failed(&state, &op, "32");
        assert_eq!(state.workbench.sessions(None).unwrap().len(), 32);
    }
}
