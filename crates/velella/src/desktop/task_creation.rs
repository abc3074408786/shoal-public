//! Managed-task write-ahead effects. The caller holds the task gate, not Service.
use super::*;
use crate::workbench::{CreationOperation, CreationStepState as State, Workbench, Workspace};
use sha2::{Digest, Sha256};

impl TaskExecution {
    fn creation(&self, task: &Task) -> Result<Option<(Workbench, CreationOperation)>> {
        let Some(id) = &task.creation_id else { return Ok(None) };
        let store = Workbench::open(&self.root)?;
        let operation = store.creation(id, None)?.context("task creation journal missing")?;
        let frozen: Task = serde_json::from_value(operation.frozen["task"].clone())?;
        // Runtime fields evolve, but the execution contract must not.
        anyhow::ensure!(frozen.id == task.id && frozen.creation_id == task.creation_id
            && frozen.project_id == task.project_id && frozen.worktree == task.worktree
            && frozen.branch == task.branch && frozen.base_oid == task.base_oid
            && frozen.direct == task.direct && frozen.parent_workspace_id == task.parent_workspace_id
            && serde_json::to_value(&frozen.spec)? == serde_json::to_value(&task.spec)?
            && serde_json::to_value(&frozen.setup_snapshot)? == serde_json::to_value(&task.setup_snapshot)?
            && frozen.allowed_commands == task.allowed_commands,
            "task no longer matches its frozen creation contract");
        Ok(Some((store, operation)))
    }

    pub(super) fn setup_script(&self, task: &Task) -> Result<String> {
        self.creation(task)?;
        match &task.setup_snapshot {
            Some(snapshot) => {
                anyhow::ensure!(snapshot.digest == format!("{:x}", Sha256::digest(snapshot.script.as_bytes())),
                    "frozen setup digest mismatch");
                Ok(if snapshot.decision == SetupPolicy::Run { snapshot.script.clone() } else { String::new() })
            }
            None => {
                anyhow::ensure!(task.creation_id.is_none(), "managed task setup snapshot missing");
                Ok(self.project(&task.project_id)?.setup)
            }
        }
    }

    fn checkout_proof(&self, task: &Task) -> Result<Value> {
        let project = self.project(&task.project_id)?;
        anyhow::ensure!(task.worktree.is_dir(), "task checkout is missing; no unknown Git effect was replayed");
        if task.direct {
            anyhow::ensure!(dunce::canonicalize(&task.worktree)? == dunce::canonicalize(&project.path)?,
                "task folder does not match its project");
            return Ok(json!({"path":task.worktree,"kind":"folder"}));
        }
        anyhow::ensure!(git(&task.worktree, &["branch", "--show-current"])?.trim() == task.branch,
            "retained checkout branch does not match frozen task");
        // Checking the top level also rejects an ordinary subdirectory of the main checkout.
        anyhow::ensure!(dunce::canonicalize(git(&task.worktree, &["rev-parse", "--show-toplevel"])?.trim())?
            == dunce::canonicalize(&task.worktree)?, "retained path is not a worktree root");
        let common = git(&task.worktree, &["rev-parse", "--git-common-dir"])?;
        let expected = git(&project.path, &["rev-parse", "--git-common-dir"])?;
        let common = dunce::canonicalize(task.worktree.join(common.trim()))?;
        anyhow::ensure!(common == dunce::canonicalize(project.path.join(expected.trim()))?,
            "retained checkout belongs to another repository");
        anyhow::ensure!(git(&task.worktree, &["rev-parse", "HEAD"])?.trim() == task.base_oid,
            "retained checkout HEAD does not match frozen task");
        Ok(json!({"path":task.worktree,"branch":task.branch,"head":task.base_oid,"common_dir":common}))
    }

    fn add_checkout(&self, task: &Task) -> Result<()> {
        anyhow::ensure!(!task.direct, "项目文件夹不存在，请检查路径");
        git(&self.project(&task.project_id)?.path, &["worktree", "add", "-b", &task.branch,
            task.worktree.to_str().context("worktree path")?, &task.base_oid])?;
        Ok(())
    }

    pub(super) fn prepare_checkout(&self, task: &Task) -> Result<()> {
        let Some((store, mut operation)) = self.creation(task)? else {
            if !task.worktree.exists() { self.add_checkout(task)?; }
            self.checkout_proof(task)?;
            return Ok(());
        };
        let fresh = operation.steps[0].state == State::Pending;
        if fresh {
            operation.steps[0].state = State::Running;
            operation = store.save_creation(&operation)?;
        }
        anyhow::ensure!(matches!(operation.steps[0].state, State::Running | State::Uncertain | State::Succeeded),
            "checkout is not recoverable automatically");
        let proof = (|| {
            // Only this call's newly persisted intent may perform Git. Recovery
            // proves a retained checkout; it never repeats an unknown worktree add.
            if fresh && !task.worktree.exists() { self.add_checkout(task)?; }
            self.checkout_proof(task)
        })();
        let proof = match proof {
            Ok(proof) => proof,
            Err(error) => {
                if operation.steps[0].state == State::Running {
                    operation.steps[0].state = State::Uncertain;
                    operation.steps[0].error = Some(error.to_string());
                    store.save_creation(&operation)?;
                }
                return Err(error);
            }
        };
        let project = self.project(&task.project_id)?;
        let workspace = Workspace {
            id: if task.direct { format!("main:{}", task.project_id) } else { format!("task:{}", task.id) },
            project_id: task.project_id.clone(),
            name: if task.direct { project.name } else { task.spec.name.clone() },
            path: task.worktree.clone(),
            kind: if task.direct { "folder" } else { "task" }.into(),
            archived: false, favorite: false, position: 0,
            parent_workspace_id: task.parent_workspace_id.clone(),
        };
        if let Some(existing) = store.workspaces()?.iter().find(|w| w.id == workspace.id) {
            anyhow::ensure!(existing.project_id == workspace.project_id
                && dunce::canonicalize(&existing.path)? == dunce::canonicalize(&workspace.path)?,
                "registered workspace does not match retained checkout");
        } else {
            store.put_workspace(&workspace).context("checkout retained; workspace registration failed")?;
        }
        if operation.steps[0].state != State::Succeeded {
            operation.steps[0].state = State::Succeeded;
            operation.steps[0].receipt = Some(proof);
            operation.steps[0].error = None;
            store.save_creation(&operation)?;
        }
        Ok(())
    }

    fn uncertain(&self, task: &Task, index: usize, error: &str) -> Result<()> {
        if let Some((store, mut operation)) = self.creation(task)? {
            if operation.steps[index].state == State::Running {
                operation.steps[index].state = State::Uncertain;
                operation.steps[index].error = Some(error.into());
                store.save_creation(&operation)?;
            }
        }
        Ok(())
    }

    fn start_setup(&self, task: &mut Task, script: &str) -> Result<()> {
        let desc = self.descriptor(task, Some(script), false)?;
        if let Some((store, mut operation)) = self.creation(task)? {
            anyhow::ensure!(operation.steps[1].state == State::Pending, "setup already attempted; reconcile its saved descriptor");
            operation.steps[1].state = State::Running;
            operation.steps[1].descriptor = Some(desc.clone());
            store.save_creation(&operation)?;
        }
        task.command = Some(desc.clone());
        task.phase = Phase::Preparing;
        task.reason = "正在执行已冻结的准备脚本".into();
        // Intent precedes both the task copy and the host start. Any failure
        // after intent is unknown, even if no owner.lock exists yet.
        let result = self.persist(task).and_then(|_| self.start(&desc));
        if let Err(error) = &result {
            self.uncertain(task, 1, &error.to_string())?;
            task.phase = Phase::NeedsRecovery;
        }
        result
    }

    pub(super) fn advance_preparing(&self, task: &mut Task, observations: &HashMap<String, std::result::Result<Status, String>>) -> Result<()> {
        if let Some((_, operation)) = self.creation(task)? {
            if operation.steps[0].state != State::Succeeded { self.prepare_checkout(task)?; }
            if !matches!(operation.steps[1].state, State::Running | State::Uncertain) {
                return self.prepare_setup(task, None);
            }
            task.command = Some(operation.steps[1].descriptor.clone().context("setup journal descriptor missing")?);
        }
        let desc = task.command.as_ref().context("setup was interrupted")?;
        // A journal descriptor may predate the persisted task copy and hence
        // be absent from this tick's observations. Probe only that saved host.
        let status = if observations.contains_key(&desc.id) { observed_status(observations, desc) }
            else { get_status(desc) };
        if self.setup_observation(task, status)? == Some(0) { self.launch_agent(task, false)?; }
        Ok(())
    }

    pub(super) fn setup_observation(&self, task: &mut Task, status: Result<Status>) -> Result<Option<i32>> {
        let mut journal = self.creation(task)?;
        if let Some((_, operation)) = &journal {
            let step = &operation.steps[1];
            anyhow::ensure!(matches!(step.state, State::Running | State::Uncertain), "setup is not awaiting an exit");
            let desc = step.descriptor.as_ref().context("setup journal descriptor missing")?;
            anyhow::ensure!(task.command.as_ref().is_some_and(|d| d.id == desc.id), "setup observation descriptor mismatch");
        }
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                self.uncertain(task, 1, &error.to_string())?;
                task.phase = Phase::NeedsRecovery;
                task.reason = "准备脚本状态未知；未自动重试".into();
                return Err(error);
            }
        };
        if let Some(code) = status.exit {
            if let Some((store, operation)) = &mut journal {
                operation.steps[1].state = if code == 0 { State::Succeeded } else { State::Failed };
                operation.steps[1].receipt = Some(json!({"kind":"setup_exit","exit":code,"descriptor_id":task.command.as_ref().map(|d| &d.id)}));
                operation.steps[1].error = (code != 0).then(|| format!("setup exited with {code}"));
                store.save_creation(operation)?;
            }
            if code != 0 {
                task.phase = Phase::NeedsAttention;
                let output = task.command.as_ref().map(|d| file_tail(&d.directory.join("terminal.bin"))).unwrap_or_default();
                task.reason = format!("环境准备失败（退出码 {code}）；重试前需要明确确认并审阅脚本：{output}");
            }
        }
        Ok(status.exit)
    }

    fn confirm_setup_retry(&self, task: &Task, req: &Value) -> Result<()> {
        self.setup_script(task)?; // Includes digest and frozen-contract verification.
        let script = match &task.setup_snapshot {
            Some(snapshot) => snapshot.script.clone(),
            None => self.project(&task.project_id)?.setup,
        };
        anyhow::ensure!(req["retry_setup"] == true && req["confirmed"] == true
            && req["reviewed_setup"].as_str() == Some(script.as_str()),
            "setup retry requires retry_setup, explicit confirmation, and review of the exact setup script");
        // Do not manufacture reviewed_at for a historical legacy execution.
        Ok(())
    }

    pub(super) fn prepare_setup(&self, task: &mut Task, retry: Option<&Value>) -> Result<()> {
        let script = self.setup_script(task)?;
        if let Some((store, mut operation)) = self.creation(task)? {
            if matches!(operation.steps[1].state, State::Running | State::Uncertain) {
                let desc = operation.steps[1].descriptor.clone().context("setup journal descriptor missing")?;
                task.command = Some(desc.clone());
                let exit = self.setup_observation(task, get_status(&desc))?;
                anyhow::ensure!(exit.is_some(), "old setup has not confirmed exit; it was not stopped or restarted");
                operation = store.creation(&operation.request_id, None)?.context("creation missing")?;
            }
            match operation.steps[1].state {
                State::Succeeded | State::Skipped => return self.launch_agent(task, false),
                State::Failed => {
                    self.confirm_setup_retry(task, retry.context("setup failed; explicit reviewed retry required")?)?;
                    let desc = operation.steps[1].descriptor.as_ref().context("setup descriptor missing")?;
                    anyhow::ensure!(host::confirmed_dead(&desc.directory), "old setup owner has not confirmed exit; no retry performed");
                    let step = &mut operation.steps[1];
                    step.state = State::Pending;
                    step.attempt = step.attempt.checked_add(1).context("setup attempt overflow")?;
                    step.descriptor = None;
                    step.receipt = None;
                    step.error = None;
                    operation = store.save_creation(&operation)?;
                }
                State::Pending => {}
                _ => anyhow::bail!("setup outcome remains unknown; no retry performed"),
            }
            if script.trim().is_empty() {
                operation.steps[1].state = State::Skipped;
                operation.steps[1].receipt = Some(json!({"kind":"setup_skipped","decision":task.setup_snapshot.as_ref().map(|s| s.decision)}));
                store.save_creation(&operation)?;
                return self.launch_agent(task, false);
            }
        }
        if script.trim().is_empty() { self.launch_agent(task, false) } else { self.start_setup(task, &script) }
    }

    /// Returns false when an initial launch already has durable intent. Such a
    /// descriptor is inspected, never spawned again (including after a crash).
    pub(super) fn initial_launch_intent(&self, task: &mut Task, desc: Option<&Descriptor>) -> Result<bool> {
        let Some((store, mut operation)) = self.creation(task)? else { return Ok(true) };
        if operation.steps[2].state != State::Pending {
            self.recover_initial_launch(task)?;
            return Ok(false);
        }
        if let Some(desc) = desc {
            operation.steps[2].state = State::Running;
            operation.steps[2].descriptor = Some(desc.clone());
            store.save_creation(&operation)?;
        }
        Ok(true)
    }

    pub(super) fn initial_launch_result(&self, task: &Task, result: &Result<()>) -> Result<()> {
        if let Err(error) = result { return self.uncertain(task, 2, &error.to_string()); }
        if let Some((store, mut operation)) = self.creation(task)? {
            operation.steps[2].state = State::Succeeded;
            operation.steps[2].receipt = Some(json!({"kind":"host_started","descriptor_id":task.agent.as_ref().map(|d| &d.id),"model_acceptance":false}));
            operation.steps[2].error = None;
            operation = store.save_creation(&operation)?;
            Self::prompt_intent(&store, operation)?;
        }
        Ok(())
    }

    fn prompt_intent(store: &Workbench, mut operation: CreationOperation) -> Result<CreationOperation> {
        if operation.steps[3].state == State::Pending {
            operation.steps[3].state = State::Running;
            operation.steps[3].descriptor = operation.steps[2].descriptor.clone();
            operation.steps[3].receipt = Some(json!({"delivery":"argv","acceptance":"unconfirmed"}));
            operation = store.save_creation(&operation)?;
        }
        Ok(operation)
    }

    pub(super) fn launch_observation_error(&self, task: &Task, desc: &Descriptor, error: &str) -> Result<()> {
        if let Some((_, operation)) = self.creation(task)? {
            if operation.steps[2].descriptor.as_ref().is_some_and(|d| d.id == desc.id) {
                self.uncertain(task, 2, error)?;
                if operation.steps[2].state == State::Succeeded { self.uncertain(task, 3, error)?; }
            }
        }
        Ok(())
    }

    pub(super) fn prompt_observation(&self, task: &Task, desc: &Descriptor, status: &Status) -> Result<()> {
        let Some((store, mut operation)) = self.creation(task)? else { return Ok(()) };
        if !operation.steps[2].descriptor.as_ref().is_some_and(|d| d.id == desc.id) { return Ok(()) }
        if matches!(operation.steps[2].state, State::Running | State::Uncertain) {
            // Covers a crash between start_host returning and its journal save.
            operation.steps[2].state = State::Succeeded;
            operation.steps[2].receipt = Some(json!({"kind":"host_status","descriptor_id":desc.id,"exit":status.exit,"model_acceptance":false}));
            operation.steps[2].error = None;
            operation = store.save_creation(&operation)?;
        }
        if operation.steps[2].state != State::Succeeded { return Ok(()) }
        let mut operation = Self::prompt_intent(&store, operation)?;
        if !matches!(operation.steps[3].state, State::Running | State::Uncertain) { return Ok(()) }
        let event = status.last_event.as_ref().and_then(|event| event.get("hook_event_name")).and_then(Value::as_str);
        let evidence = if status.turns > 0 { Some("turn_completed") }
            else if event == Some("UserPromptSubmit") { Some("UserPromptSubmit") } else { None };
        if let Some(evidence) = evidence {
            operation.steps[3].state = State::Succeeded;
            operation.steps[3].receipt = Some(json!({"delivery":"argv","evidence":evidence,"descriptor_id":desc.id,"turns":status.turns}));
            operation.steps[3].error = None;
            store.save_creation(&operation)?;
        } else if status.exit.is_some() && operation.steps[3].state == State::Running {
            operation.steps[3].state = State::Uncertain;
            operation.steps[3].error = Some("host exited without an observed prompt/turn receipt".into());
            store.save_creation(&operation)?;
        }
        Ok(())
    }

    fn recover_initial_launch(&self, task: &mut Task) -> Result<()> {
        let (store, mut operation) = self.creation(task)?.context("creation missing")?;
        let desc = operation.steps[2].descriptor.clone().context("launch descriptor missing; no replay allowed")?;
        let status = match get_status(&desc) {
            Ok(status) => status,
            Err(error) => {
                self.uncertain(task, 2, &error.to_string())?;
                task.agent = Some(desc);
                task.phase = Phase::NeedsRecovery;
                return Err(error.context("initial launch outcome unknown; no replacement was spawned"));
            }
        };
        if matches!(operation.steps[2].state, State::Running | State::Uncertain) {
            operation.steps[2].state = State::Succeeded;
            operation.steps[2].receipt = Some(json!({"kind":"host_status","descriptor_id":desc.id,"exit":status.exit,"model_acceptance":false}));
            operation.steps[2].error = None;
            store.save_creation(&operation)?;
        }
        self.prompt_observation(task, &desc, &status)?;
        task.agent = Some(desc);
        task.command = None;
        task.native_id = status.native_id.clone().or_else(|| task.native_id.clone());
        task.phase = if status.exit.is_some() { Phase::NeedsRecovery } else { Phase::Running };
        task.reason = "已找回原启动描述符；未重发首次指令".into();
        task.status = Some(status);
        self.persist(task)
    }

    pub(super) fn resume_task(&self, task: &mut Task, req: &Value) -> Result<()> {
        anyhow::ensure!(matches!(task.phase, Phase::NeedsRecovery | Phase::NeedsAttention | Phase::Cancelled),
            "task does not need recovery");
        let journal = self.creation(task)?;
        if task.agent.is_some() {
            if let Some(desc) = &task.command {
                anyhow::ensure!(get_status(desc)?.exit.is_some(), "old command has not confirmed exit; it was not stopped");
            }
        }
        if let Some((_, operation)) = &journal {
            if operation.steps[2].state != State::Pending {
                let initial = operation.steps[2].descriptor.as_ref().context("launch descriptor missing")?;
                let needs_receipt = matches!(operation.steps[2].state, State::Running | State::Uncertain);
                if task.agent.is_none() || (needs_receipt && task.agent.as_ref().is_some_and(|d| d.id == initial.id)) {
                    self.recover_initial_launch(task)?;
                    // Reattach a live initial host; never kill it merely to repeat launch.
                    if task.status.as_ref().is_some_and(|s| s.exit.is_none()) {
                        task.deadline = Some(renewed_deadline(task)?);
                        return Ok(());
                    }
                }
            }
        }
        if journal.as_ref().is_some_and(|(_, op)| op.steps[2].state == State::Pending) {
            anyhow::ensure!(task.agent.is_none() && task.native_id.is_none(), "agent exists before journal launch; inspect task state");
            task.deadline = Some(renewed_deadline(task)?);
            self.prepare_checkout(task)?;
            return self.prepare_setup(task, Some(req));
        }
        // A setup or verification process must report its exit. A missing lock
        // is not proof that its command never ran, nor permission to rerun it.
        let command_exit = task.command.as_ref().map(|desc| {
            get_status(desc)?.exit.context("old command has not confirmed exit; it was not stopped")
        }).transpose();
        if command_exit.is_err() && task.agent.is_none() {
            self.uncertain(task, 1, "old setup has no confirmed exit")?;
        }
        let command_exit = command_exit?;
        if let Some(desc) = task.agent.clone() {
            let status = get_status(&desc).context("old agent status unknown; no replacement was spawned")?;
            self.prompt_observation(task, &desc, &status)?;
            task.native_id = status.native_id.or_else(|| task.native_id.clone());
            anyhow::ensure!(task.native_id.is_some(), "native session ID missing; inspect terminal history before creating another task");
            if status.exit.is_none() {
                stop_host(&desc)?;
                anyhow::ensure!(get_status(&desc)?.exit.is_some(), "old agent has not confirmed exit");
            }
            task.deadline = Some(renewed_deadline(task)?);
            return self.launch_agent(task, true);
        }
        task.deadline = Some(renewed_deadline(task)?);
        if task.native_id.is_some() {
            // Legacy records can retain a native ID without a host descriptor.
            self.launch_agent(task, true)
        } else if command_exit == Some(0) {
            // Known successful legacy setup is not rerun after a project edit.
            self.launch_agent(task, false)
        } else {
            let script = self.setup_script(task)?;
            if command_exit.is_some() || !script.trim().is_empty() { self.confirm_setup_retry(task, req)?; }
            if let Some(desc) = &task.command {
                anyhow::ensure!(host::confirmed_dead(&desc.directory), "old setup owner has not confirmed exit; no retry performed");
            }
            self.prepare_checkout(task)?;
            self.prepare_setup(task, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        execution: TaskExecution,
        task: Task,
        root: PathBuf,
    }
    impl Fixture {
        fn new(script: &str, decision: SetupPolicy, managed: bool) -> Self {
            let root = std::env::temp_dir().join(format!("velella-task-creation-{}", uuid::Uuid::now_v7()));
            let project = root.join("project");
            let data = root.join("data");
            std::fs::create_dir_all(&project).unwrap();
            std::fs::create_dir_all(&data).unwrap();
            let id = uuid::Uuid::now_v7().to_string();
            let spec = TaskSpec::from_yaml_str("name: fixture\nagent: codex\ninstruction: first instruction must not repeat\n").unwrap();
            let task = serde_json::from_value(json!({
                "id":id,"project_id":"p","spec":spec,"phase":"queued","worktree":project,
                "branch":"fixture-task","base_oid":"","direct":true,"created_at":"","reason":"",
                "verified":false,"checks":[],"retries":0,"observed_turns":0,"allowed_commands":[],
                "setup_snapshot":{"decision":decision,"script":script,"digest":format!("{:x}",Sha256::digest(script.as_bytes())),"reviewed_at":null}
            })).unwrap();
            let execution = TaskExecution {
                root:data, executable:root.join("deliberately-missing-runtime.exe"),
                projects:vec![Project {id:"p".into(),path:project,name:"fixture".into(),setup:script.into(),
                    default_base:None,setup_default:decision,direct:true,terminals:vec![]}],
            };
            let mut fixture = Self {execution,task,root};
            if managed { fixture.freeze(); }
            fixture
        }
        fn freeze(&mut self) {
            self.task.creation_id = Some(self.task.id.clone());
            self.store().accept_creation(&self.task.id, &json!({"cmd":"task.create"}), &json!({"task":self.task})).unwrap();
        }
        fn store(&self) -> Workbench { Workbench::open(&self.execution.root).unwrap() }
        fn operation(&self) -> CreationOperation { self.store().creation(&self.task.id, None).unwrap().unwrap() }
        fn setup_running(&mut self) -> Descriptor {
            self.execution.prepare_checkout(&self.task).unwrap();
            let desc = self.execution.descriptor(&self.task, Some(&self.execution.setup_script(&self.task).unwrap()), false).unwrap();
            let mut operation = self.operation();
            operation.steps[1].state = State::Running;
            operation.steps[1].descriptor = Some(desc.clone());
            self.store().save_creation(&operation).unwrap();
            self.task.command = Some(desc.clone());
            self.task.phase = Phase::Preparing;
            desc
        }
        fn launch_running(&mut self) -> Descriptor {
            self.execution.prepare_checkout(&self.task).unwrap();
            let mut operation = self.operation();
            operation.steps[1].state = State::Skipped;
            self.store().save_creation(&operation).unwrap();
            let desc = self.execution.descriptor(&self.task, None, false).unwrap();
            self.execution.initial_launch_intent(&mut self.task, Some(&desc)).unwrap();
            desc
        }
        fn status(desc: &Descriptor, exit: Option<i32>, native: Option<&str>) {
            // Saved, fixture-only host evidence. No IPC server, CLI or daemon is run.
            ipc::atomic_json(&desc.directory.join("status.json"), &Status {
                exit,native_id:native.map(str::to_owned),state:"exited".into(),..Status::default()
            }).unwrap();
        }
        fn session_count(&self) -> usize {
            std::fs::read_dir(self.execution.projects[0].path.join(".velella/sessions")).map(|s|s.count()).unwrap_or(0)
        }
        fn git_fixture() -> Self {
            for name in ["GIT_DIR","GIT_COMMON_DIR","GIT_WORK_TREE","GIT_INDEX_FILE","GIT_OBJECT_DIRECTORY","GIT_ALTERNATE_OBJECT_DIRECTORIES","GIT_NAMESPACE"] {
                assert!(std::env::var_os(name).is_none(), "unset {name} for isolated Git fixtures");
            }
            let mut fixture = Self::new("", SetupPolicy::Skip, false);
            let project = fixture.execution.projects[0].path.clone();
            git(&project, &["init", "--quiet", "--template=", "-b", "main"]).unwrap();
            let oid = fixture.commit("initial");
            git(&project, &["update-ref", "refs/heads/main", &oid]).unwrap();
            fixture.task.direct = false;
            fixture.task.base_oid = oid;
            fixture.task.worktree = project.join("retained");
            fixture.execution.projects[0].direct = false;
            fixture.freeze();
            fixture
        }
        fn commit(&self, message: &str) -> String {
            let project = &self.execution.projects[0].path;
            let tree = git(project, &["mktree"]).unwrap();
            // Plumbing only: no checkout hooks, user config changes or production refs.
            git(project, &["-c","user.name=Task Creation Fixture","-c","user.email=fixture@example.invalid",
                "commit-tree",tree.trim(),"-m",message]).unwrap().trim().into()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.root); }
    }

    #[test]
    fn frozen_script_and_skip_ignore_project_edits_and_reject_corrupt_digest() {
        for (decision, expected) in [(SetupPolicy::Run,"original"),(SetupPolicy::Skip,"")] {
            let mut f = Fixture::new("original", decision, true);
            f.execution.projects[0].setup = "edited after acceptance".into();
            assert_eq!(f.execution.setup_script(&f.task).unwrap(), expected);
            f.task.setup_snapshot.as_mut().unwrap().script = "tampered".into();
            assert!(f.execution.setup_script(&f.task).is_err());
        }
        let mut f = Fixture::new("original", SetupPolicy::Run, false);
        f.task.setup_snapshot.as_mut().unwrap().digest = "invalid".into();
        assert!(f.execution.setup_script(&f.task).is_err());
        f.task.setup_snapshot = None;
        f.execution.projects[0].setup = "legacy first prepare".into();
        assert_eq!(f.execution.setup_script(&f.task).unwrap(), "legacy first prepare");
    }

    #[test]
    fn prepare_journals_frozen_setup_before_failed_host_start_without_replay() {
        let mut f = Fixture::new("original", SetupPolicy::Run, true);
        f.execution.projects[0].setup = "changed".into();
        // The nonexistent runtime fails spawn. This does NOT prove the general
        // launch failure case had no effects, so it must still be Uncertain.
        assert!(f.execution.prepare(&mut f.task).is_err());
        let op = f.operation();
        assert_eq!(op.steps[0].state, State::Succeeded);
        assert_eq!(op.steps[1].state, State::Uncertain);
        assert_eq!(op.steps[2].state, State::Pending);
        let desc = op.steps[1].descriptor.as_ref().unwrap();
        assert_eq!(desc.spawn.args.last().unwrap(), "original");
        let conn = db(&f.execution.projects[0]).unwrap();
        let document: String = conn.query_row("SELECT document FROM desktop_tasks WHERE id=?1", [&f.task.id], |row| row.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&document).unwrap()["command"]["id"], desc.id);
        drop(conn);
        assert!(f.execution.prepare(&mut f.task).is_err());
        assert_eq!(f.operation().steps[1].descriptor.as_ref().unwrap().id, desc.id);
        assert_eq!(f.session_count(), 1);
    }

    #[test]
    fn skipped_or_empty_setup_reaches_launch_without_a_setup_descriptor() {
        for (script, decision) in [("do not run",SetupPolicy::Skip),(" \n",SetupPolicy::Run)] {
            let mut f = Fixture::new(script, decision, true);
            f.execution.projects[0].setup = "new setup must not run".into();
            assert!(f.execution.prepare(&mut f.task).is_err()); // missing fixture runtime
            let op = f.operation();
            assert_eq!(op.steps[1].state, State::Skipped);
            assert!(op.steps[1].descriptor.is_none());
            assert_eq!(op.steps[2].state, State::Uncertain);
            assert!(op.steps[2].descriptor.as_ref().unwrap().spawn.args.last().unwrap().starts_with("first instruction"));
            assert_eq!(f.session_count(), 1);
        }
    }

    #[test]
    fn setup_success_is_durable_before_launch_and_is_never_rerun() {
        let mut f = Fixture::new("frozen", SetupPolicy::Run, true);
        let setup = f.setup_running();
        f.execution.projects[0].setup = "edited".into();
        let observations = HashMap::from([(setup.id.clone(), Ok(Status {exit:Some(0),..Status::default()}))]);
        assert!(f.execution.advance(&mut f.task, &observations).is_err()); // launch cannot spawn
        let op = f.operation();
        assert_eq!(op.steps[1].state, State::Succeeded);
        assert_eq!(op.steps[1].receipt.as_ref().unwrap()["exit"], 0);
        assert_eq!(op.steps[2].state, State::Uncertain);
        assert_eq!(op.steps[1].descriptor.as_ref().unwrap().id, setup.id);
        f.task.phase = Phase::NeedsRecovery;
        assert!(f.execution.resume_task(&mut f.task, &json!({"retry_setup":true,"confirmed":true,"reviewed_setup":"frozen"})).is_err());
        assert_eq!(f.session_count(), 2);
        assert_eq!(f.operation().steps[1].attempt, 0);
    }

    #[test]
    fn failed_setup_waits_for_original_owner_before_retry() {
        use fs2::FileExt;
        use interprocess::local_socket::prelude::*;
        for managed in [true, false] {
            let mut f = Fixture::new("frozen", SetupPolicy::Run, managed);
            let setup = if managed { f.setup_running() } else {
                let desc = f.execution.descriptor(&f.task, Some("frozen"), false).unwrap();
                f.task.command = Some(desc.clone());
                desc
            };
            if managed {
                f.execution.setup_observation(&mut f.task, Ok(Status {exit:Some(9),..Status::default()})).unwrap();
            }
            Fixture::status(&setup, Some(9), None);
            f.task.phase = Phase::NeedsAttention;
            let owner = std::fs::File::create(setup.directory.join("owner.lock")).unwrap();
            owner.lock_exclusive().unwrap();
            let peer = (!managed).then(|| {
                let listener = ipc::listen(&setup.endpoint).unwrap();
                std::thread::spawn(move || ipc::handle(listener.accept().unwrap(), |req| {
                    assert_eq!(req["cmd"], "status");
                    Ok(json!({"ok":true,"status":Status {exit:Some(9),..Status::default()}}))
                }))
            });
            let error = f.execution.resume_task(&mut f.task, &json!({"retry_setup":true,"confirmed":true,"reviewed_setup":"frozen"})).unwrap_err();
            if let Some(peer) = peer { peer.join().unwrap(); }
            assert!(error.to_string().contains("owner"), "managed={managed}: {error:#}");
            assert_eq!(f.task.command.as_ref().unwrap().id, setup.id);
            assert_eq!(f.session_count(), 1);
            if managed { assert_eq!(f.operation().steps[1].attempt, 0); }
        }
    }

    #[test]
    fn failed_setup_requires_exact_review_and_explicit_retry_then_new_attempt() {
        let mut f = Fixture::new("frozen", SetupPolicy::Run, true);
        let setup = f.setup_running();
        f.execution.setup_observation(&mut f.task, Ok(Status {exit:Some(9),..Status::default()})).unwrap();
        f.execution.projects[0].setup = "edited".into();
        for req in [json!({}),json!({"retry_setup":true,"reviewed_setup":"frozen"}),
            json!({"confirmed":true,"reviewed_setup":"frozen"}),
            json!({"retry_setup":true,"confirmed":true,"reviewed_setup":"edited"})] {
            assert!(f.execution.resume_task(&mut f.task, &req).is_err());
            assert_eq!(f.operation().steps[1].state, State::Failed);
            assert_eq!(f.operation().steps[1].attempt, 0);
            assert_eq!(f.session_count(), 1);
        }
        assert!(f.execution.resume_task(&mut f.task, &json!({"retry_setup":true,"confirmed":true,"reviewed_setup":"frozen"})).is_err());
        let op = f.operation();
        assert_eq!(op.steps[1].attempt, 1);
        assert_eq!(op.steps[1].state, State::Uncertain); // new fixture spawn fails
        assert_ne!(op.steps[1].descriptor.as_ref().unwrap().id, setup.id);
        assert_eq!(op.steps[1].descriptor.as_ref().unwrap().spawn.args.last().unwrap(), "frozen");
        assert!(op.steps[1].receipt.is_none()); // previous failed exit evidence cleared
        assert_eq!(op.steps[2].state, State::Pending);
    }

    #[test]
    fn uncertain_setup_uses_saved_descriptor_and_only_resolves_a_known_exit() {
        for exit in [0,7] {
            let mut f = Fixture::new("frozen", SetupPolicy::Run, true);
            let setup = f.setup_running();
            assert!(f.execution.setup_observation(&mut f.task, Err(anyhow::anyhow!("fixture disconnected"))).is_err());
            f.task.command = None; // crash before the task copy saved its descriptor
            f.task.phase = Phase::NeedsRecovery;
            let req = json!({"retry_setup":true,"confirmed":true,"reviewed_setup":"frozen"});
            assert!(f.execution.resume_task(&mut f.task, &req).is_err());
            Fixture::status(&setup, None, None); // saved nonterminal status is not proof of exit
            assert!(f.execution.resume_task(&mut f.task, &req).is_err());
            assert_eq!(f.operation().steps[1].state, State::Uncertain);
            assert_eq!(f.operation().steps[1].attempt, 0);
            assert_eq!(f.session_count(), 1);
            Fixture::status(&setup, Some(exit), None);
            assert!(f.execution.resume_task(&mut f.task, &json!({})).is_err());
            let op = f.operation();
            assert_eq!(op.steps[1].state, if exit == 0 {State::Succeeded} else {State::Failed});
            assert_eq!(op.steps[1].descriptor.as_ref().unwrap().id, setup.id);
            assert_eq!(op.steps[1].attempt, 0);
            assert_eq!(f.session_count(), if exit == 0 {2} else {1});
        }
    }

    #[test]
    fn interrupted_task_copy_recovers_completed_setup_without_requiring_host_again() {
        let mut f = Fixture::new("frozen", SetupPolicy::Run, true);
        f.setup_running();
        f.execution.setup_observation(&mut f.task, Ok(Status {exit:Some(0),..Status::default()})).unwrap();
        f.task.command = None;
        f.task.phase = Phase::Preparing;
        assert!(f.execution.advance(&mut f.task, &HashMap::new()).is_err()); // missing launch runtime
        assert_eq!(f.operation().steps[1].state, State::Succeeded);
        assert_eq!(f.operation().steps[2].state, State::Uncertain);
        assert_eq!(f.session_count(), 2);
    }

    #[test]
    fn unknown_initial_launch_never_creates_a_replacement_or_retransmits_prompt() {
        let mut f = Fixture::new("", SetupPolicy::Skip, true);
        let desc = f.launch_running();
        f.task.phase = Phase::NeedsRecovery;
        for _ in 0..2 {
            assert!(f.execution.resume_task(&mut f.task, &json!({})).is_err());
            assert_eq!(f.operation().steps[2].state, State::Uncertain);
            assert_eq!(f.task.agent.as_ref().unwrap().id, desc.id);
            assert_eq!(f.session_count(), 1);
        }
    }

    #[test]
    fn host_receipt_and_session_start_do_not_claim_prompt_acceptance() {
        for evidence in ["UserPromptSubmit","Stop"] {
            let mut f = Fixture::new("", SetupPolicy::Skip, true);
            let desc = f.launch_running();
            f.task.agent = Some(desc.clone());
            // Synthetic host-start result tests journal semantics only, not native acceptance.
            f.execution.initial_launch_result(&f.task, &Ok(())).unwrap();
            let op = f.operation();
            assert_eq!(op.steps[2].receipt.as_ref().unwrap()["model_acceptance"], false);
            assert_eq!(op.steps[3].receipt.as_ref().unwrap()["delivery"], "argv");
            let mut status = Status {hooks_ready:true,last_event:Some(json!({"hook_event_name":"SessionStart"})),..Status::default()};
            f.execution.prompt_observation(&f.task, &desc, &status).unwrap();
            assert_eq!(f.operation().steps[3].state, State::Running);
            status.last_event = Some(json!({"hook_event_name":evidence}));
            status.turns = if evidence == "Stop" {1} else {0};
            f.execution.prompt_observation(&f.task, &desc, &status).unwrap();
            assert_eq!(f.operation().steps[3].state, State::Succeeded);
        }
    }

    #[test]
    fn managed_resume_preserves_native_id_and_omits_first_instruction() {
        let mut f = Fixture::new("", SetupPolicy::Skip, true);
        let initial = f.launch_running();
        Fixture::status(&initial, Some(0), Some("native-session"));
        f.task.phase = Phase::NeedsRecovery;
        assert!(f.execution.resume_task(&mut f.task, &json!({})).is_err()); // missing resume runtime
        let resumed = f.task.agent.as_ref().unwrap();
        assert_ne!(resumed.id, initial.id);
        assert_eq!(f.task.native_id.as_deref(), Some("native-session"));
        assert!(resumed.spawn.args.windows(2).any(|a| a == ["resume","native-session"]));
        assert!(!resumed.spawn.args.iter().any(|a| a.contains("first instruction")));
        let op = f.operation();
        assert_eq!(op.steps[2].descriptor.as_ref().unwrap().id, initial.id);
        assert_eq!(op.steps[3].state, State::Uncertain);
        assert_eq!(f.session_count(), 2);
    }

    #[test]
    fn legacy_retry_needs_current_review_and_never_invents_historical_approval() {
        let mut f = Fixture::new("old", SetupPolicy::Run, false);
        f.task.setup_snapshot = None;
        let old = f.execution.descriptor(&f.task, Some("old"), false).unwrap();
        f.task.command = Some(old.clone());
        f.task.phase = Phase::NeedsAttention;
        f.execution.projects[0].setup = "current".into();
        Fixture::status(&old, Some(1), None);
        for req in [json!({}),json!({"retry_setup":true,"confirmed":true,"reviewed_setup":"old"})] {
            assert!(f.execution.resume_task(&mut f.task, &req).is_err());
            assert_eq!(f.session_count(), 1);
        }
        assert!(f.execution.resume_task(&mut f.task, &json!({"retry_setup":true,"confirmed":true,"reviewed_setup":"current"})).is_err());
        assert_eq!(f.task.command.as_ref().unwrap().spawn.args.last().unwrap(), "current");
        assert!(f.task.setup_snapshot.is_none());
        assert_eq!(f.session_count(), 2);
    }

    #[test]
    fn legacy_success_or_unknown_setup_is_not_rerun_after_project_edit() {
        for exit in [Some(0),None] {
            let mut f = Fixture::new("old", SetupPolicy::Run, false);
            f.task.setup_snapshot = None;
            let old = f.execution.descriptor(&f.task, Some("old"), false).unwrap();
            f.task.command = Some(old.clone());
            f.task.phase = Phase::NeedsRecovery;
            f.execution.projects[0].setup = "edited".into();
            Fixture::status(&old, exit, None);
            assert!(f.execution.resume_task(&mut f.task, &json!({"retry_setup":true,"confirmed":true,"reviewed_setup":"edited"})).is_err());
            if exit.is_some() {
                assert!(f.task.command.is_none());
                assert!(f.task.agent.is_some());
            } else {
                assert_eq!(f.task.command.as_ref().unwrap().id, old.id);
                assert!(f.task.agent.is_none());
            }
            assert!(f.task.setup_snapshot.is_none());
        }
    }

    #[test]
    fn intent_precedes_failed_task_save_for_setup_and_initial_launch() {
        for decision in [SetupPolicy::Run,SetupPolicy::Skip] {
            let mut f = Fixture::new("frozen", decision, true);
            f.execution.prepare_checkout(&f.task).unwrap();
            // Make only this fixture's task DB unopenable. The journal remains writable.
            std::fs::create_dir_all(f.execution.projects[0].path.join(".velella/state.db")).unwrap();
            assert!(f.execution.prepare_setup(&mut f.task, None).is_err());
            let op = f.operation();
            let index = if decision == SetupPolicy::Run {1} else {2};
            assert_eq!(op.steps[index].state, State::Uncertain);
            let desc = op.steps[index].descriptor.as_ref().unwrap();
            assert!(!desc.directory.join("launch.json").exists()); // start_host was never reached
        }
    }

    #[test]
    fn observed_host_reconciles_interrupted_launch_but_not_prompt_from_session_start() {
        let mut f = Fixture::new("", SetupPolicy::Skip, true);
        let desc = f.launch_running();
        f.task.agent = Some(desc.clone());
        f.task.phase = Phase::Running;
        let missing = HashMap::from([(desc.id.clone(), Err("fixture disconnected".into()))]);
        assert!(f.execution.advance(&mut f.task, &missing).is_err());
        assert_eq!(f.operation().steps[2].state, State::Uncertain);
        let started = Status {hooks_ready:true,last_event:Some(json!({"hook_event_name":"SessionStart"})),..Status::default()};
        let observations = HashMap::from([(desc.id.clone(), Ok(started))]);
        f.execution.advance(&mut f.task, &observations).unwrap();
        assert_eq!(f.operation().steps[2].state, State::Succeeded);
        assert_eq!(f.operation().steps[3].state, State::Running);
        assert_eq!(f.session_count(), 1);
        // A different native-resume host cannot accept the initial argv prompt.
        let mut resumed = desc.clone();
        resumed.id = "different-host".into();
        f.execution.prompt_observation(&f.task, &resumed, &Status {turns:1,..Status::default()}).unwrap();
        assert_eq!(f.operation().steps[3].state, State::Running);
    }

    #[test]
    fn fresh_checkout_is_created_from_frozen_head() {
        let f = Fixture::git_fixture();
        let newer = f.commit("new project HEAD");
        git(&f.execution.projects[0].path, &["update-ref","refs/heads/main",&newer]).unwrap();
        f.execution.prepare_checkout(&f.task).unwrap();
        assert_eq!(git(&f.task.worktree, &["rev-parse","HEAD"]).unwrap().trim(), f.task.base_oid);
        assert_eq!(f.operation().steps[0].state, State::Succeeded);
        assert_eq!(f.store().workspaces().unwrap().len(), 1);
    }

    #[test]
    fn checkout_recovers_proven_retained_worktree_without_duplicate_registration() {
        let f = Fixture::git_fixture();
        let mut op = f.operation();
        op.steps[0].state = State::Running;
        f.store().save_creation(&op).unwrap();
        f.execution.add_checkout(&f.task).unwrap(); // emulate crash after Git, before registration
        for _ in 0..2 { f.execution.prepare_checkout(&f.task).unwrap(); }
        let op = f.operation();
        assert_eq!(op.steps[0].state, State::Succeeded);
        assert_eq!(op.steps[0].receipt.as_ref().unwrap()["head"], f.task.base_oid);
        assert_eq!(f.store().workspaces().unwrap().len(), 1);
        let worktrees = git(&f.execution.projects[0].path, &["worktree","list","--porcelain"]).unwrap();
        assert_eq!(worktrees.matches("branch refs/heads/fixture-task").count(), 1);
    }

    #[test]
    fn checkout_recovery_rejects_wrong_branch_head_repository_and_missing_effect() {
        for mismatch in ["branch","head","repository","missing"] {
            let f = Fixture::git_fixture();
            let mut op = f.operation();
            op.steps[0].state = State::Running;
            f.store().save_creation(&op).unwrap();
            if mismatch == "repository" {
                std::fs::create_dir(&f.task.worktree).unwrap();
                git(&f.task.worktree, &["init","--quiet","--template=","-b",&f.task.branch]).unwrap();
            } else if mismatch != "missing" {
                f.execution.add_checkout(&f.task).unwrap();
                if mismatch == "branch" {
                    git(&f.task.worktree, &["symbolic-ref","HEAD","refs/heads/wrong"]).unwrap();
                } else {
                    let changed = f.commit("changed HEAD");
                    git(&f.task.worktree, &["update-ref","HEAD",&changed]).unwrap();
                }
            }
            assert!(f.execution.prepare_checkout(&f.task).is_err(), "{mismatch}");
            assert_eq!(f.operation().steps[0].state, State::Uncertain);
            assert!(f.store().workspaces().unwrap().is_empty());
            if mismatch == "missing" {
                assert!(!f.task.worktree.exists());
                assert!(git(&f.execution.projects[0].path, &["show-ref","--verify","refs/heads/fixture-task"]).is_err());
            }
        }
    }
}
