//! Ordinary workspace setup: journal intent before spawning, observe without replay.
use super::*;
use crate::workbench::{CreationOperation, CreationStepState, Session, Workbench, Workspace};
use CreationStepState::{Failed, Pending, Running, Skipped, Succeeded, Uncertain};

pub(super) fn is_ordinary(operation: &CreationOperation) -> bool {
    operation.frozen["workspace"].is_object() && operation.frozen.get("task").is_none()
}

pub(super) fn validate_execution(req: &Value, decision: SetupPolicy) -> Result<()> {
    anyhow::ensure!(req.get("sandbox").is_none(), "创建工作区使用 execution 声明执行模式，不能忽略 sandbox 字段");
    let policy: crate::sandbox::Policy = req.get("execution").cloned()
        .map(serde_json::from_value).transpose().context("invalid execution policy")?
        .unwrap_or_default();
    anyhow::ensure!(decision != SetupPolicy::Run || !policy.is_sandboxed(),
        "普通工作区准备脚本仅支持 native 执行；受限终端请明确选择 skip");
    Ok(())
}

fn snapshot(operation: &CreationOperation) -> Result<SetupSnapshot> {
    anyhow::ensure!(is_ordinary(operation), "not an ordinary workspace creation");
    serde_json::from_value(operation.frozen["setup"].clone()).context("frozen setup missing")
}

fn workspace(operation: &CreationOperation) -> Result<Workspace> {
    anyhow::ensure!(is_ordinary(operation), "not an ordinary workspace creation");
    let workspace: Workspace = serde_json::from_value(operation.frozen["workspace"].clone())?;
    anyhow::ensure!(workspace.id == operation.request_id, "workspace creation identity mismatch");
    Ok(workspace)
}

/// Legacy creations without frozen launch intent retain their skip behavior.
pub(super) fn settle_skips(store: &Workbench, mut operation: CreationOperation) -> Result<CreationOperation> {
    anyhow::ensure!(is_ordinary(&operation), "not an ordinary workspace creation");
    if operation.steps[0].state != Succeeded { return Ok(operation); }
    if operation.steps[1].state == Pending {
        let setup = snapshot(&operation)?;
        if setup.decision == SetupPolicy::Skip || setup.script.trim().is_empty() {
            operation.steps[1].state = Skipped;
            operation = store.save_creation(&operation)?;
        }
    }
    if matches!(operation.steps[1].state, Succeeded | Skipped) {
        if !operation.frozen["launch_request"].is_object() {
            for index in 2..operation.steps.len() {
                if operation.steps[index].state == Pending {
                    operation.steps[index].state = Skipped;
                    operation = store.save_creation(&operation)?;
                }
            }
        } else if operation.steps[2].state == Succeeded && operation.steps[3].state == Pending
            && operation.frozen["launch_request"].get("initial_prompt").is_none() {
            operation.steps[3].state = Skipped;
            operation = store.save_creation(&operation)?;
        }
    }
    Ok(operation)
}

// Only a newly persisted Pending -> Running transition can set launch=true.
// A recovered Running/Uncertain descriptor is always observation-only.
struct SetupExecution {
    operation: CreationOperation,
    executable: PathBuf,
    launch: bool,
}

fn prepare(service: &Service, id: &str) -> Result<Option<SetupExecution>> {
    let Some(operation) = service.workbench.creation(id, None)? else { return Ok(None); };
    if !is_ordinary(&operation) { return Ok(None); }
    let mut operation = settle_skips(&service.workbench, operation)?;
    if operation.steps[0].state != Succeeded || !matches!(operation.steps[1].state, Pending | Running | Uncertain) {
        return Ok(None);
    }
    let frozen_workspace = workspace(&operation)?;
    let current = service.workbench.workspace(&frozen_workspace.id)?;
    anyhow::ensure!(current.path == frozen_workspace.path && current.project_id == frozen_workspace.project_id,
        "workspace no longer matches its creation receipt");
    let launch = operation.steps[1].state == Pending;
    if launch {
        let setup = snapshot(&operation)?;
        validate_execution(&operation.request, setup.decision)?;
        anyhow::ensure!(operation.request["reviewed_setup"].as_str() == Some(setup.script.as_str()),
            "请审阅创建记录中的准备脚本后再执行");
        let id = uuid::Uuid::now_v7().to_string();
        let desc = Descriptor {
            endpoint: ipc::endpoint(&service.root, &id),
            // Keep logs outside the new checkout; never delete them on failure.
            directory: service.project(&current.project_id)?.path.join(".velella/sessions").join(&id),
            id,
            spawn: SpawnRequest {
                program: if cfg!(windows) { "powershell" } else { "sh" }.into(),
                args: if cfg!(windows) {
                    vec!["-NoLogo".into(), "-NoProfile".into(), "-Command".into(), setup.script]
                } else { vec!["-c".into(), setup.script] },
                cwd: Some(frozen_workspace.path.to_string_lossy().into_owned()),
                env: HashMap::from([("TERM".into(), "xterm-256color".into())]),
                adapter: Some("generic".into()),
                sandbox: Default::default(),
            },
        };
        operation.steps[1].state = Running;
        operation.steps[1].descriptor = Some(desc);
        operation = service.workbench.save_creation(&operation)?;
    }
    if let Some(desc) = &operation.steps[1].descriptor {
        // Recover metadata after a crash between the journal and session insert,
        // but never recover by spawning. Preserve an existing user session title.
        if service.workbench.session(&current.id, &desc.id)?.is_none() {
            service.workbench.put_session(&Session {
                id: desc.id.clone(), workspace_id: current.id, title: format!("准备脚本 · {}", current.name),
                kind: "command".into(), task_id: None, native_id: None, descriptor: desc.clone(),
            })?;
        }
    }
    Ok(Some(SetupExecution { operation, executable: service.executable.clone(), launch }))
}

fn reconcile(store: &Workbench, mut operation: CreationOperation, observation: Result<Status>) -> Result<CreationOperation> {
    if !matches!(operation.steps[1].state, Running | Uncertain) { return Ok(operation); }
    let step = &mut operation.steps[1];
    match observation {
        Ok(status) => {
            let Some(exit) = status.exit else { return Ok(operation); };
            step.state = if exit == 0 { Succeeded } else { Failed };
            step.receipt = Some(json!({"exit_code":exit,"status":status}));
            step.error = (exit != 0).then(|| format!("准备脚本退出码 {exit}；工作目录与日志已保留"));
        }
        Err(error) => {
            if step.state == Uncertain { return Ok(operation); }
            step.state = Uncertain;
            step.error = Some(format!("准备脚本结果不确定；不会自动重试：{error:#}"));
        }
    }
    let operation = store.save_creation(&operation)?;
    settle_skips(store, operation)
}

fn tick_one(service: &Arc<Mutex<Service>>, id: &str) -> Result<()> {
    let key = format!("creation:{id}");
    let gate = { service.lock().unwrap().terminal_gates.entry(key).or_default().clone() };
    // Serialize an in-flight launch with polling and explicit retries. Never
    // wait on this gate while owning the Service lock.
    let Ok(_guard) = gate.try_lock() else { return Ok(()); };
    let execution = { prepare(&service.lock().unwrap(), id)? };
    let Some(execution) = execution else { return Ok(()); };
    let observation = if let Some(desc) = &execution.operation.steps[1].descriptor {
        let started = if execution.launch {
            std::fs::create_dir_all(&desc.directory).map_err(anyhow::Error::from)
                .and_then(|()| start_host(&execution.executable, desc))
        } else { Ok(()) };
        // A start timeout is not proof that the process failed. Even an error
        // must be reconciled from its original host rather than spawning again.
        get_status(desc).map_err(|error| match started {
            Err(start) => error.context(format!("setup launch was not confirmed: {start:#}")),
            Ok(()) => error,
        })
    } else { Err(anyhow::anyhow!("running setup has no durable descriptor")) };
    let state = service.lock().unwrap();
    let Some(current) = state.workbench.creation(id, None)? else { return Ok(()); };
    if current.revision == execution.operation.revision {
        reconcile(&state.workbench, current, observation)?;
    }
    Ok(())
}

/// Called outside the supervisor's task/state lock; no host IPC owns that lock.
pub(super) fn tick(service: &Arc<Mutex<Service>>) -> Result<()> {
    let ids = { service.lock().unwrap().workbench.creation_operations()?.into_iter()
        .filter(is_ordinary).map(|operation| operation.request_id).collect::<Vec<_>>() };
    let mut first_error = None;
    for id in ids {
        if let Err(error) = tick_one(service, &id) {
            if first_error.is_none() { first_error = Some(error.context(format!("workspace setup {id}"))); }
        }
    }
    first_error.map_or(Ok(()), Err)
}

impl Service {
    pub(super) fn ensure_workspace_prepared(&self, id: &str) -> Result<()> {
        // Main/task workspace identifiers are deliberately outside this journal
        // lookup. Ordinary creation uses the workspace UUID as its receipt ID.
        if uuid::Uuid::parse_str(id).is_err() { return Ok(()); }
        let Some(operation) = self.workbench.creation(id, None)? else { return Ok(()); };
        if !is_ordinary(&operation) { return Ok(()); }
        workspace(&operation)?;
        anyhow::ensure!(operation.steps[0].state == Succeeded && matches!(operation.steps[1].state, Succeeded | Skipped),
            "工作区准备尚未成功或跳过（{:?}）；请查看准备脚本日志，失败后明确重试", operation.steps[1].state);
        Ok(())
    }

    pub(super) fn retry_workspace_setup(&mut self, req: &Value) -> Result<Value> {
        anyhow::ensure!(req["confirmed"] == true, "请明确确认重试准备脚本");
        let id = uuid::Uuid::parse_str(req["request_id"].as_str().context("request_id required")?)?.to_string();
        let gate = self.terminal_gates.entry(format!("creation:{id}")).or_default().clone();
        let _guard = gate.try_lock().map_err(|_| anyhow::anyhow!("准备脚本正在启动或检查，请稍后重试"))?;
        let mut operation = self.workbench.creation(&id, None)?.context("creation operation not found")?;
        let frozen_workspace = workspace(&operation)?;
        if let Some(workspace_id) = req.get("workspace_id") {
            anyhow::ensure!(workspace_id.as_str() == Some(frozen_workspace.id.as_str()), "retry workspace does not match request_id");
        }
        let setup = snapshot(&operation)?;
        anyhow::ensure!(setup.decision == SetupPolicy::Run && !setup.script.trim().is_empty(), "creation has no setup script to retry");
        anyhow::ensure!(req["reviewed_setup"].as_str() == Some(setup.script.as_str()), "请审阅创建记录中完全相同的准备脚本后再重试");
        validate_execution(&operation.request, setup.decision)?;
        validate_execution(req, setup.decision)?;
        anyhow::ensure!(matches!(operation.steps[1].state, Failed | Uncertain), "仅失败或结果不确定的准备脚本可以检查重试");
        let desc = operation.steps[1].descriptor.clone().context("setup descriptor missing; cannot prove old exit")?;
        let status = get_status(&desc).context("无法确认旧准备进程的实际退出；未重试")?;
        let exit = status.exit.context("旧准备进程仍在运行；未重试")?;
        if operation.steps[1].state == Uncertain {
            operation = reconcile(&self.workbench, operation, Ok(status))?;
            if operation.steps[1].state == Succeeded {
                return Ok(json!({"ok":true,"workspace":self.workbench.workspace(&id)?,"creation":operation,"reconciled":true}));
            }
        }
        anyhow::ensure!(exit != 0 && host::confirmed_dead(&desc.directory), "旧准备进程尚未确认停止；未重试");
        let step = &mut operation.steps[1];
        step.state = Pending;
        step.attempt = step.attempt.checked_add(1).context("setup attempt overflow")?;
        step.descriptor = None;
        step.receipt = None;
        step.error = None;
        operation = self.workbench.save_creation(&operation)?;
        Ok(json!({"ok":true,"workspace":self.workbench.workspace(&id)?,"creation":operation}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These are journal/state fixtures, not evidence of Git checkout or PTY
    // execution. Host observations are deliberately saved fake status files.
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("velella-ordinary-setup-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir(&root).unwrap();
            std::fs::create_dir(root.join("project")).unwrap();
            Self(root)
        }
        fn service(&self) -> Service {
            Service {
                root: self.0.clone(), executable: self.0.join("never-spawn-this-fixture"),
                projects: vec![Project {
                    id: "p".into(), path: self.0.join("project"), name: "fixture".into(),
                    setup: "exit 7".into(), default_base: None, setup_default: SetupPolicy::Skip,
                    direct: false, terminals: vec![],
                }],
                tasks: vec![], project_locks: HashMap::new(),
                workbench: Workbench::open(&self.0).unwrap(),
                capabilities: capabilities::Capabilities::open(&self.0).unwrap(),
                terminal_gates: HashMap::new(), terminal_inflight: Default::default(), terminal_epoch: 0,
                collaboration: Arc::new(Mutex::new(None)),
            }
        }
        fn accepted(&self, service: &Service, decision: SetupPolicy, script: &str) -> CreationOperation {
            let id = uuid::Uuid::now_v7().to_string();
            let workspace = Workspace {
                id: id.clone(), project_id: "p".into(), name: "original".into(),
                path: self.0.join(format!("workspace-{id}")), kind: "worktree".into(),
                archived: false, favorite: false, position: 0, parent_workspace_id: None,
            };
            std::fs::create_dir(&workspace.path).unwrap();
            service.workbench.put_workspace(&workspace).unwrap();
            let request = json!({"cmd":"workbench.create","request_id":id,"project_id":"p", "name":"original",
                "setup":decision,"reviewed_setup":script,"execution":{"mode":"native"}});
            let mut project = service.projects[0].clone();
            project.setup = script.into();
            let setup = creation::setup_snapshot(&project, &request, false).unwrap();
            let operation = service.workbench.accept_creation(&id, &request,
                &json!({"workspace":workspace,"setup":setup,"base":{"oid":"fixture-only"},"branch":"fixture-only"})).unwrap();
            let operation = transition(service, operation, 0, Running);
            transition(service, operation, 0, Succeeded)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    fn transition(service: &Service, mut operation: CreationOperation, index: usize, state: CreationStepState) -> CreationOperation {
        operation.steps[index].state = state;
        service.workbench.save_creation(&operation).unwrap()
    }
    fn load(service: &Service, id: &str) -> CreationOperation {
        service.workbench.creation(id, None).unwrap().unwrap()
    }
    fn document(operation: &CreationOperation) -> Value { serde_json::to_value(operation).unwrap() }
    fn saved_status(desc: &Descriptor, exit: Option<i32>) {
        std::fs::create_dir_all(&desc.directory).unwrap();
        ipc::atomic_json(&desc.directory.join("status.json"), &Status {
            state: if exit.is_some() { "exited" } else { "running" }.into(), exit, ..Status::default()
        }).unwrap();
    }
    fn retry_request(id: &str) -> Value {
        json!({"cmd":"workbench.creation.retry","request_id":id,"confirmed":true,"reviewed_setup":"exit 7"})
    }

    #[test]
    fn setup_review_and_execution_policy_are_validated_before_checkout() {
        let fixture = Fixture::new();
        let mut service = fixture.service();
        for execution in [Value::Null, json!({}), json!({"mode":"typo"}),
            json!({"mode":"native","extra":true}), json!({"mode":"restricted"}), json!({"mode":"isolated"})] {
            let id = uuid::Uuid::now_v7().to_string();
            let request = json!({"request_id":id,"project_id":"p","name":"bad", "setup":"run",
                "reviewed_setup":"exit 7","execution":execution});
            let error = service.create_workbench(request).unwrap_err().to_string();
            assert!(error.contains("invalid execution policy") || error.contains("仅支持 native"), "{error}");
            assert!(service.workbench.creation(&id, None).unwrap().is_none());
        }
        let project = service.projects[0].clone();
        for request in [json!({"setup":"run"}), json!({"setup":"run","reviewed_setup":"exit 7 "})] {
            assert!(creation::setup_snapshot(&project, &request, false).is_err());
        }
        assert!(creation::setup_snapshot(&project, &json!({"setup":"run","reviewed_setup":"exit 7"}), false).is_ok());
        assert!(creation::setup_snapshot(&project, &json!({"setup":"skip","execution":{"mode":"restricted"}}), false).is_ok());
        assert!(creation::setup_snapshot(&project, &json!({"setup":"skip","execution":null}), false).is_err());
        // Managed task execution remains outside ordinary policy validation.
        assert!(creation::setup_snapshot(&project, &json!({"setup":"run","execution":{"mode":"isolated"}}), true).is_ok());
    }

    #[test]
    fn replay_returns_pending_without_git_or_overwriting_user_metadata() {
        let fixture = Fixture::new();
        let mut service = fixture.service();
        let operation = fixture.accepted(&service, SetupPolicy::Run, "exit 7");
        let mut current = service.workbench.workspace(&operation.request_id).unwrap();
        let mut parent = current.clone();
        parent.id = uuid::Uuid::now_v7().to_string();
        service.workbench.put_workspace(&parent).unwrap();
        current.name = "user renamed".into();
        current.parent_workspace_id = Some(parent.id);
        service.workbench.put_workspace(&current).unwrap();
        // No Git repository exists, and the original project cannot be looked
        // up. Replaying the completed checkout must require neither.
        service.projects.clear();
        let response = service.create_workbench(operation.request.clone()).unwrap();
        assert_eq!(response["workspace"], serde_json::to_value(current).unwrap());
        assert_eq!(response["creation"], document(&operation));
        assert!(service.workbench.sessions(None).unwrap().is_empty());
        assert!(service.ensure_workspace_prepared(&operation.request_id).is_err());
    }

    #[test]
    fn skip_and_empty_setup_finish_without_a_descriptor_or_session() {
        let fixture = Fixture::new();
        let mut service = fixture.service();
        for (decision, script) in [(SetupPolicy::Skip, "exit 7"), (SetupPolicy::Run, "  \n")] {
            let operation = fixture.accepted(&service, decision, script);
            let result = service.create_workbench(operation.request.clone()).unwrap();
            assert_eq!(result["creation"]["steps"][1]["state"], "skipped");
            let operation = load(&service, &operation.request_id);
            assert!(operation.ready());
            assert!(operation.steps[1..].iter().all(|s| s.descriptor.is_none()));
            service.ensure_workspace_prepared(&operation.request_id).unwrap();
            assert!(prepare(&service, &operation.request_id).unwrap().is_none());
        }
        assert!(service.workbench.sessions(None).unwrap().is_empty());
    }

    #[test]
    fn pending_claim_saves_exact_descriptor_and_command_session_before_spawn() {
        let fixture = Fixture::new();
        let mut service = fixture.service();
        let operation = fixture.accepted(&service, SetupPolicy::Run, "exit 7");
        service.projects[0].setup = "changed configuration must not run".into();
        let execution = prepare(&service, &operation.request_id).unwrap().unwrap();
        assert!(execution.launch);
        let saved = load(&service, &operation.request_id);
        assert_eq!(saved.steps[1].state, Running);
        let desc = saved.steps[1].descriptor.as_ref().unwrap();
        assert!(!desc.directory.exists()); // No host or log directory was spawned.
        assert_eq!(desc.spawn.args.last().unwrap(), "exit 7");
        assert_eq!(desc.spawn.cwd.as_deref(), operation.frozen["workspace"]["path"].as_str());
        assert!(!desc.spawn.sandbox.is_sandboxed());
        assert_eq!(desc.spawn.program, if cfg!(windows) { "powershell" } else { "sh" });
        assert_eq!(desc.spawn.args, if cfg!(windows) {
            vec!["-NoLogo", "-NoProfile", "-Command", "exit 7"]
        } else { vec!["-c", "exit 7"] });
        let session = service.workbench.session(&operation.request_id, &desc.id).unwrap().unwrap();
        assert_eq!(session.kind, "command");
        assert!(session.task_id.is_none());
        assert!(!workbench_commands::automatic_shell_recovery_allowed(&session));
        assert_eq!(serde_json::to_value(session.descriptor).unwrap(), serde_json::to_value(desc).unwrap());
        let replay = prepare(&service, &operation.request_id).unwrap().unwrap();
        assert!(!replay.launch);
        assert_eq!(document(&replay.operation), document(&saved));
        assert!(service.tasks.is_empty());
    }

    #[test]
    fn journal_or_session_write_failure_never_authorizes_an_unrecorded_spawn() {
        let fixture = Fixture::new();
        let service = fixture.service();
        let operation = fixture.accepted(&service, SetupPolicy::Run, "exit 7");
        let conn = Connection::open(fixture.0.join("workbench.db")).unwrap();
        conn.execute_batch("CREATE TRIGGER reject_setup_journal BEFORE UPDATE ON creation_operations
            BEGIN SELECT RAISE(ABORT, 'fixture journal failure'); END;").unwrap();
        assert!(prepare(&service, &operation.request_id).is_err());
        assert_eq!(document(&load(&service, &operation.request_id)), document(&operation));
        assert!(service.workbench.sessions(None).unwrap().is_empty());
        conn.execute_batch("DROP TRIGGER reject_setup_journal;
            CREATE TRIGGER reject_setup_session BEFORE INSERT ON sessions
            BEGIN SELECT RAISE(ABORT, 'fixture session failure'); END;").unwrap();
        assert!(prepare(&service, &operation.request_id).is_err());
        let running = load(&service, &operation.request_id);
        assert_eq!(running.steps[1].state, Running);
        let desc = running.steps[1].descriptor.as_ref().unwrap();
        assert!(!desc.directory.exists());
        assert!(service.workbench.sessions(None).unwrap().is_empty());
        conn.execute_batch("DROP TRIGGER reject_setup_session;").unwrap();
        let recovered = prepare(&service, &operation.request_id).unwrap().unwrap();
        assert!(!recovered.launch);
        assert_eq!(document(&recovered.operation), document(&running));
        assert_eq!(service.workbench.sessions(None).unwrap().len(), 1);
        assert!(service.ensure_workspace_prepared(&operation.request_id).is_err());
    }

    #[test]
    fn state_polling_across_restart_accepts_only_actual_zero_exit() {
        let fixture = Fixture::new();
        let service = fixture.service();
        let operation = fixture.accepted(&service, SetupPolicy::Run, "exit 7");
        let execution = prepare(&service, &operation.request_id).unwrap().unwrap();
        let desc = execution.operation.steps[1].descriptor.as_ref().unwrap().clone();
        let unchanged = reconcile(&service.workbench, execution.operation.clone(), Ok(Status {
            state: "completed".into(), exit: None, ..Status::default()
        })).unwrap();
        assert_eq!(unchanged.steps[1].state, Running);
        drop(service);
        let service = Arc::new(Mutex::new(fixture.service()));
        saved_status(&desc, Some(0));
        tick(&service).unwrap();
        {
            let state = service.lock().unwrap();
            let completed = load(&state, &operation.request_id);
            assert!(completed.ready());
            assert_eq!(completed.steps[1].state, Succeeded);
            assert_eq!(completed.steps[1].receipt.as_ref().unwrap()["exit_code"], 0);
            state.ensure_workspace_prepared(&operation.request_id).unwrap();
        }
        tick(&service).unwrap();
        assert_eq!(service.lock().unwrap().workbench.sessions(None).unwrap().len(), 1);
    }

    #[test]
    fn missing_exit_becomes_uncertain_and_never_relaunches_on_replay_or_tick() {
        let fixture = Fixture::new();
        let service = fixture.service();
        let operation = fixture.accepted(&service, SetupPolicy::Run, "exit 7");
        let execution = prepare(&service, &operation.request_id).unwrap().unwrap();
        let desc = execution.operation.steps[1].descriptor.as_ref().unwrap().clone();
        saved_status(&desc, None);
        drop(service);
        let service = Arc::new(Mutex::new(fixture.service()));
        tick(&service).unwrap();
        let uncertain = load(&service.lock().unwrap(), &operation.request_id);
        assert_eq!(uncertain.steps[1].state, Uncertain);
        tick(&service).unwrap();
        let mut state = service.lock().unwrap();
        assert_eq!(document(&load(&state, &operation.request_id)), document(&uncertain));
        assert_eq!(state.create_workbench(operation.request.clone()).unwrap()["creation"], document(&uncertain));
        assert!(state.retry_workspace_setup(&retry_request(&operation.request_id)).is_err());
        assert!(state.ensure_workspace_prepared(&operation.request_id).is_err());
        assert_eq!(document(&load(&state, &operation.request_id)), document(&uncertain));
    }

    #[test]
    fn failed_retry_requires_exact_review_confirmation_and_dead_owner() {
        let fixture = Fixture::new();
        let service = fixture.service();
        let operation = fixture.accepted(&service, SetupPolicy::Run, "exit 7");
        let execution = prepare(&service, &operation.request_id).unwrap().unwrap();
        let desc = execution.operation.steps[1].descriptor.as_ref().unwrap().clone();
        saved_status(&desc, Some(7));
        ipc::atomic_json(&desc.directory.join("log-retained.json"), &json!({"fixture":"log"})).unwrap();
        let service = Arc::new(Mutex::new(service));
        tick(&service).unwrap();
        let failed = load(&service.lock().unwrap(), &operation.request_id);
        assert_eq!(failed.steps[1].state, Failed);
        assert_eq!(failed.steps[2].state, Pending);
        tick(&service).unwrap();
        assert_eq!(document(&load(&service.lock().unwrap(), &operation.request_id)), document(&failed));
        let mut state = service.lock().unwrap();
        assert!(state.ensure_workspace_prepared(&operation.request_id).is_err());
        let request = retry_request(&operation.request_id);
        for (field, value) in [("confirmed", json!(false)), ("reviewed_setup", json!("exit 7 ")),
            ("request_id", Value::Null), ("workspace_id", json!("other")), ("execution", json!({"mode":"restricted"}))] {
            let mut invalid = request.clone(); invalid[field] = value;
            assert!(state.retry_workspace_setup(&invalid).is_err());
            assert_eq!(document(&load(&state, &operation.request_id)), document(&failed));
        }
        // A saved exit alone cannot authorize retry while the owner is locked.
        let owner = OpenOptions::new().create(true).truncate(false).read(true).write(true)
            .open(desc.directory.join("owner.lock")).unwrap();
        owner.try_lock_exclusive().unwrap();
        assert!(state.retry_workspace_setup(&request).is_err());
        assert_eq!(document(&load(&state, &operation.request_id)), document(&failed));
        drop(owner);
        state.projects[0].setup = "current project settings changed".into();
        let response = state.retry_workspace_setup(&request).unwrap();
        let pending = load(&state, &operation.request_id);
        assert_eq!(response["creation"], document(&pending));
        assert_eq!(pending.steps[1].state, Pending);
        assert_eq!(pending.steps[1].attempt, 1);
        assert!(pending.steps[1].descriptor.is_none());
        assert!(pending.steps[1].receipt.is_none());
        assert!(pending.steps[1].error.is_none());
        assert_eq!(pending.frozen, operation.frozen);
        assert_eq!(pending.request, operation.request);
        assert!(state.retry_workspace_setup(&request).is_err());
        let next = prepare(&state, &operation.request_id).unwrap().unwrap();
        assert!(next.launch);
        let next_desc = next.operation.steps[1].descriptor.as_ref().unwrap();
        assert_ne!(next_desc.id, desc.id);
        assert_eq!(next_desc.spawn.args.last().unwrap(), "exit 7");
        assert_eq!(state.workbench.sessions(None).unwrap().len(), 2);
        assert!(desc.directory.join("log-retained.json").is_file());
    }

    #[test]
    fn uncertain_retry_reconciles_exit_before_allowing_a_new_attempt() {
        let fixture = Fixture::new();
        let mut service = fixture.service();
        for exit in [0, 7] {
            let operation = fixture.accepted(&service, SetupPolicy::Run, "exit 7");
            let execution = prepare(&service, &operation.request_id).unwrap().unwrap();
            let desc = execution.operation.steps[1].descriptor.as_ref().unwrap().clone();
            transition(&service, execution.operation, 1, Uncertain);
            assert!(service.retry_workspace_setup(&retry_request(&operation.request_id)).is_err());
            saved_status(&desc, Some(exit));
            service.retry_workspace_setup(&retry_request(&operation.request_id)).unwrap();
            let after = load(&service, &operation.request_id);
            assert_eq!(after.steps[1].state, if exit == 0 { Succeeded } else { Pending });
            assert_eq!(after.steps[1].attempt, if exit == 0 { 0 } else { 1 });
            assert_eq!(after.ready(), exit == 0);
            assert_eq!(after.frozen, operation.frozen);
        }
    }

    #[test]
    fn ordinary_supervisor_ignores_task_journals_and_retry_rejects_them() {
        let fixture = Fixture::new();
        let service = fixture.service();
        let id = uuid::Uuid::now_v7().to_string();
        let operation = service.workbench.accept_creation(&id, &json!({}),
            &json!({"task":{"id":id},"workspace":{"id":id}})).unwrap();
        assert!(prepare(&service, &id).unwrap().is_none());
        let service = Arc::new(Mutex::new(service));
        tick(&service).unwrap();
        let mut state = service.lock().unwrap();
        assert!(state.retry_workspace_setup(&retry_request(&id)).is_err());
        state.ensure_workspace_prepared("main:p").unwrap();
        state.ensure_workspace_prepared("task:anything").unwrap();
        state.ensure_workspace_prepared(&uuid::Uuid::now_v7().to_string()).unwrap();
        assert_eq!(document(&load(&state, &id)), document(&operation));
        assert!(state.workbench.sessions(None).unwrap().is_empty());
    }

    #[test]
    fn setup_gate_prevents_overlapping_poll_or_explicit_retry() {
        let fixture = Fixture::new();
        let mut service = fixture.service();
        let operation = fixture.accepted(&service, SetupPolicy::Run, "exit 7");
        let gate = service.terminal_gates.entry(format!("creation:{}", operation.request_id)).or_default().clone();
        let _guard = gate.lock().unwrap();
        let service = Arc::new(Mutex::new(service));
        tick(&service).unwrap();
        let mut state = service.lock().unwrap();
        assert_eq!(document(&load(&state, &operation.request_id)), document(&operation));
        assert!(state.retry_workspace_setup(&retry_request(&operation.request_id)).is_err());
    }
}
