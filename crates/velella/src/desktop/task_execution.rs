use super::*;
#[path = "task_creation.rs"]
mod creation;
#[path = "task_feedback.rs"]
mod feedback;

// An immutable execution context owns no daemon state lock. A task gate remains
// held by the caller until its result is merged, including partial failures.
pub(super) struct TaskExecution {
    pub root: PathBuf,
    pub executable: PathBuf,
    pub projects: Vec<Project>,
}
impl TaskExecution {
    fn project(&self,id:&str)->Result<Project>{self.projects.iter().find(|p|p.id==id).cloned().context("project not found")}
    pub(super) fn persist(&self,task:&Task)->Result<()>{save(&self.project(&task.project_id)?,task)}
    pub(super) fn descriptor(&self, task: &Task, command: Option<&str>, resume: bool) -> Result<Descriptor> {
        let launch_args = if command.is_none() {
            crate::native_launch::args(&task.spec.agent, &task.spec.launch)?
        } else {
            Vec::new()
        };
        let id = uuid::Uuid::now_v7().to_string();
        let directory = self
            .project(&task.project_id)?
            .path
            .join(".velella/sessions")
            .join(&id);
        std::fs::create_dir_all(&directory)?;
        let endpoint = ipc::endpoint(&self.root, &id);
        let sandbox = task_sandbox::configure(&self.root, &self.executable, &self.project(&task.project_id)?, task, &id, command.is_none())?;
        task_sandbox::validate(&sandbox, &task.worktree)?;
        let mut env = HashMap::new();
        env.insert("PATH".into(), self.launch_path()?);
        // ConPTY hosts an interactive TUI; do not inherit a daemon launcher's
        // TERM=dumb sentinel, which makes Codex decline interactive mode.
        env.insert("TERM".into(), "xterm-256color".into());
        if sandbox.policy.is_sandboxed() { env.insert("GIT_OPTIONAL_LOCKS".into(), "0".into()); }
        if let Some(inbox) = &sandbox.hook_inbox {
            env.insert("VELELLA_HOOK_INBOX".into(), inbox.to_string_lossy().into_owned());
        } else if let Some(hook_endpoint) = &sandbox.hook_endpoint {
            env.insert("VELELLA_SESSION_ENDPOINT".into(), hook_endpoint.clone());
        } else if !sandbox.policy.is_sandboxed() {
            env.insert("VELELLA_SESSION_ENDPOINT".into(), endpoint.clone());
        }
        let (program, mut args, adapter): (String, Vec<String>, &str) = if let Some(command) = command {
            if cfg!(windows) {
                (
                    "powershell".into(),
                    vec![
                        "-NoLogo".into(),
                        "-NoProfile".into(),
                        "-Command".into(),
                        command.into(),
                    ],
                    "generic",
                )
            } else {
                ("sh".into(), vec!["-c".into(), command.into()], "generic")
            }
        } else if task.spec.agent == "claude-code" {
            let hooks = if sandbox.policy.is_sandboxed() { hook_settings_for(&sandbox.hook_command) } else { hook_settings() };
            let settings = sandbox.settings_directory.as_ref().unwrap_or(&directory).join("claude-settings.json");
            ipc::atomic_json(&settings, &json!({"hooks":hooks}))?;
            let mut args = vec![
                "--settings".into(),
                settings.to_string_lossy().into_owned(),
                "--permission-mode".into(),
                "acceptEdits".into(),
            ];
            if resume {
                args.extend([
                    "--resume".into(),
                    task.native_id
                        .clone()
                        .context("native session ID unavailable")?,
                ]);
            } else {
                args.extend(["--session-id".into(), id.clone()]);
            }
            for command in &task.allowed_commands {
                args.extend(["--allowedTools".into(), format!("Bash({command})")]);
            }
            ("claude".into(), args, "claude-code")
        } else {
            let mut args = Vec::new();
            if resume {
                args.extend([
                    "resume".into(),
                    task.native_id
                        .clone()
                        .context("native session ID unavailable")?,
                ]);
            }
            args.extend([
                "--sandbox".into(),
                "workspace-write".into(),
                "--ask-for-approval".into(),
                "on-request".into(),
            ]);
            for event in [
                "SessionStart",
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "PermissionRequest",
                "Stop",
                "Interrupt",
            ] {
                args.extend(["-c".into(),format!("hooks.{event}=[{{hooks=[{{type=\"command\",command={}}}]}}]", serde_json::to_string(&sandbox.hook_command)?)]);
            }
            ("codex".into(), args, "codex")
        };
        args.extend(launch_args);
        if command.is_none() && !resume {
            args.push(format!("{}\nWork only on this task in the current worktree. Leave changes for human review. Do not commit, push, merge, or deploy.\nVerification commands:\n{}",task.spec.instruction.as_deref().unwrap_or(""),task.checks.iter().map(|c|c.command.as_str()).collect::<Vec<_>>().join("\n")));
        }
        let program = if command.is_none() {
            task_sandbox::agent_program(&program, env.get("PATH").map(String::as_str).unwrap_or(""), &sandbox.policy)?
        } else { program };
        let mut desc = Descriptor {
            id,
            endpoint,
            spawn: SpawnRequest { sandbox: sandbox.policy,
                program,
                args,
                cwd: Some(task.worktree.to_string_lossy().into_owned()),
                env,
                adapter: Some(adapter.into()),
            },
            directory,
        };
        if command.is_none() && !desc.spawn.sandbox.is_sandboxed() {
            let workspace=if task.direct {format!("main:{}",task.project_id)} else {format!("task:{}",task.id)};
            crate::browser_agent::inject(&mut desc,&self.executable,&self.root,&workspace)?;
        }
        Ok(desc)
    }

    pub(super) fn start(&self, desc: &Descriptor) -> Result<()> {
        start_host(&self.executable,desc)
    }

    fn launch_path(&self) -> Result<String> {
        let mut paths = vec![self.executable.parent().context("runtime executable directory")?.to_path_buf()];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        Ok(std::env::join_paths(paths)?.to_string_lossy().into_owned())
    }

    fn validate_launch(&self, task: &Task, resume: bool) -> Result<()> {
        if task.spec.launch.is_empty() { return Ok(()); }
        // Use the same PATH as the descriptor, including runtime-local CLIs.
        // Descriptor construction stays synthetic; probing belongs at boundaries.
        let program = if task.spec.agent == "claude-code" { "claude" } else { "codex" };
        crate::native_launch::validated_args_for_program(
            &task.spec.agent, &task.spec.launch, program, resume,
            Some(std::ffi::OsStr::new(&self.launch_path()?)),
        )?;
        Ok(())
    }

    pub(super) fn prepare(&self, task: &mut Task) -> Result<()> {
        crate::sandbox::ensure_available(&task.spec.policy.execution)?;
        self.setup_script(task)?; // Reject a corrupt snapshot before any Git or host effect.
        self.validate_launch(task, false)?;
        task.deadline = Some(renewed_deadline(task)?);
        task.phase = Phase::Preparing;
        task.reason = if task.direct { "准备在原文件夹执行" } else { "准备独立工作目录" }.into();
        self.persist(task)?;
        self.prepare_checkout(task)?;
        self.prepare_setup(task, None)
    }

    pub(super) fn launch_agent(&self, task: &mut Task, resume: bool) -> Result<()> {
        self.validate_launch(task, resume)?;
        if !resume && !self.initial_launch_intent(task, None)? { return Ok(()); }
        let desc = self.descriptor(task, None, resume)?;
        if !resume && !self.initial_launch_intent(task, Some(&desc))? { return Ok(()); }
        task.agent = Some(desc.clone());
        task.status = None; // The previous host snapshot cannot describe this new process.
        task.command = None;
        task.phase = Phase::Running;
        task.observed_turns = 0;
        task.verified = false;
        task.reason = "等待 Agent 启动；首次连接请在终端完成信任与登录检查".into();
        let result = self.persist(task).and_then(|_| self.start(&desc));
        if !resume { self.initial_launch_result(task, &result)?; }
        if result.is_err() { task.phase = Phase::NeedsRecovery; }
        result?;
        if resume {
            call_host(&desc, json!({"cmd":"control","human":true}))?;
            task.reason = "原生会话已恢复，请在终端检查现场后继续".into();
        }
        Ok(())
    }

    pub(super) fn advance(&self, task: &mut Task,observations:&HashMap<String,std::result::Result<Status,String>>) -> Result<()> {
        if task
            .deadline
            .is_some_and(|d| chrono::Utc::now().timestamp() >= d)
        {
            self.stop(task)?;
            task.phase = Phase::NeedsAttention;
            task.reason = "任务时间上限已到，执行进程已停止；未完成".into();
            return Ok(());
        }
        if task.phase == Phase::Preparing {
            self.advance_preparing(task, observations)?;
        } else if task.phase == Phase::Running {
            let desc = task.agent.as_ref().context("agent not launched")?;
            let status = match observed_status(observations,desc) {
                Ok(status) => status,
                Err(error) => {
                    self.launch_observation_error(task, desc, &error.to_string())?;
                    return Err(error);
                }
            };
            self.prompt_observation(task, desc, &status)?;
            if status.native_id.is_some() {
                task.native_id = status.native_id.clone();
            }
            task.status = Some(status.clone());
            if status.exit.is_some() && status.turns <= task.observed_turns {
                task.phase = Phase::NeedsRecovery;
                task.reason = "Agent 退出，尚未收到完整回合事件".into();
            } else if status.human_control && status.exit.is_none() {
                task.reason = "人工接管中；交还执行后继续检查".into();
            } else if status.hooks_ready && status.turns > task.observed_turns {
                task.observed_turns = status.turns;
                self.begin_checks(task)?;
            } else if status.state == "waiting_permission" {
                task.reason = "需要确认，请打开终端查看目录信任、hooks 或权限请求".into();
            } else if !status.hooks_ready {
                task.reason = "等待 Agent 就绪；如长时间无变化，请打开终端检查启动提示".into();
            } else if status.state == "input_pending" {
                task.reason = "输入已发送，等待原生反馈；请检查终端现场".into();
            } else if status.state == "interrupted" {
                task.phase = Phase::NeedsAttention;
                task.reason = "原生会话等待输入或已中断；尚未收到完成事件，请检查终端".into();
            } else {
                task.reason = "Agent 正在执行".into();
            }
        } else if task.phase == Phase::Verifying {
            let desc = task.command.as_ref().context("verification interrupted")?;
            let status = observed_status(observations,desc)?;
            if let Some(code) = status.exit {
                let check = task
                    .checks
                    .iter_mut()
                    .find(|c| c.passed.is_none())
                    .context("check not found")?;
                check.passed = Some(code == 0);
                check.exit = Some(code);
                check.output = file_tail(&desc.directory.join("terminal.bin"));
                if code != 0 {
                    if task.retries < task.spec.verify.retries.min(1) {
                        let feedback=format!("Verification failed: {}\n{}\nDiagnose the failure and repair it. Do not repeat unchanged commands or push/merge.",check.command,check.output);
                        let agent = task.agent.clone().context("agent missing")?;
                        let s = observed_status(observations,&agent)?;
                        anyhow::ensure!(s.exit.is_none(), "agent exited before repair");
                        if s.human_control {
                            task.phase = Phase::NeedsAttention;
                            task.reason = "验证失败，人工接管期间未自动发送修复指令".into();
                            return Ok(());
                        }
                        task.retries += 1;
                        task.phase = Phase::NeedsAttention;
                        task.reason = "准备发送唯一一次修复；若中断请核对会话".into();
                        self.persist(task)?;
                        call_host(
                            &agent,
                            json!({"cmd":"input","text":feedback.replace('\n'," "),"enter":true,"automated":true}),
                        )?;
                        task.phase = Phase::Running;
                        task.command = None;
                        task.reason = "检查失败，已回馈一次修复".into();
                    } else {
                        task.phase = Phase::NeedsAttention;
                        task.reason = "验证失败，已停止自动重试".into();
                        task.command = None;
                    }
                } else {
                    self.next_check(task)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn begin_checks(&self, task: &mut Task) -> Result<()> {
        task.verified = false;
        for check in &mut task.checks {
            check.passed = None;
            check.exit = None;
            check.output.clear();
        }
        if task.checks.is_empty() {
            task.phase = Phase::Review;
            task.reason = "未配置验证命令：改动未验证，请人工审查".into();
            task.command = None;
            return Ok(());
        }
        task.phase = Phase::Verifying;
        self.next_check(task)
    }

    pub(super) fn next_check(&self, task: &mut Task) -> Result<()> {
        if let Some(check) = task.checks.iter().find(|c| c.passed.is_none()) {
            let desc = self.descriptor(task, Some(&check.command), false)?;
            task.command = Some(desc.clone());
            task.reason = format!("正在验证：{}", check.command);
            self.persist(task)?;
            self.start(&desc)?;
        } else {
            task.verified = true;
            task.phase = Phase::Review;
            task.reason = "验证通过，等待人工审查".into();
            task.command = None;
        }
        Ok(())
    }

    pub(super) fn stop(&self, task: &Task) -> Result<()> {
        if let Some(d) = &task.command {
            stop_host(d)?;
        }
        if let Some(d) = &task.agent {
            stop_host(d)?;
        }
        if let Some(d) = &task.shell {
            stop_host(d)?;
        }
        Ok(())
    }

    pub(super) fn command(&self,task:&mut Task,req:&Value)->Result<Value> {
        let response=match req["cmd"].as_str().unwrap_or("") {
            "terminal.shell" => {
                anyhow::ensure!(task.worktree.exists(), "task worktree is not ready");
                let needs_start=match task.shell.as_ref(){
                    None=>true,
                    Some(desc)=>match get_status(desc){
                        Ok(status)=>status.exit.is_some(),
                        Err(_) if host::confirmed_dead(&desc.directory)=>true,
                        Err(error)=>return Err(error.context("existing shell is unresponsive; no replacement was created")),
                    },
                };
                if needs_start {
                    let mut desc = self.descriptor(task, Some(""), false)?;
                    desc.spawn.args = if cfg!(windows) {
                        vec!["-NoLogo".into(), "-NoProfile".into()]
                    } else {
                        vec![]
                    };
                    task.shell = Some(desc.clone());
                    self.persist(task)?;
                    self.start(&desc)?;
                }
                json!({"ok":true})
            }
            "task.resume" => {
                self.resume_task(task, req)?;
                json!({"ok":true})
            }
            "task.verify" => {
                anyhow::ensure!(!task.phase.active(), "wait for the active task to settle");
                task.deadline = Some(renewed_deadline(task)?);
                self.begin_checks(task)?;
                json!({"ok":true})
            }
            "task.feedback" => self.send_task_feedback(task, req)?,
            "task.finish"=>{
                anyhow::ensure!(task.direct&&task.phase==Phase::Review,"只能结束待审查的普通文件夹任务");
                anyhow::ensure!(req["confirmed"]==true,"请确认已检查任务结果");
                self.stop(task)?;task.phase=Phase::Delivered;task.reason="任务已结束；文件保留在原目录".into();
                json!({"ok":true})
            },
            _=>anyhow::bail!("unsupported task execution"),
        };Ok(response)
    }
}

#[cfg(test)]
mod launch_tests {
    use super::*;
    use velella_core::task::LaunchOptions;

    struct Fixture {
        execution: TaskExecution,
        task: Task,
        root: PathBuf,
    }

    impl Fixture {
        fn new(agent: &str) -> Self {
            let root = std::env::temp_dir().join(format!("vl-task-launch-{}", uuid::Uuid::now_v7()));
            let project = root.join("project");
            let worktree = project.join("worktree");
            std::fs::create_dir_all(&worktree).unwrap();
            let spec = TaskSpec::from_yaml_str(&format!("name: fixture\nagent: {agent}\ninstruction: launch fixture\n")).unwrap();
            let task = serde_json::from_value(json!({
                "id":"task", "project_id":"project", "spec":spec, "phase":"queued",
                "worktree":worktree, "branch":"fixture", "base_oid":"base",
                "native_id":"native-session", "created_at":"", "reason":"",
                "verified":false, "checks":[], "retries":0, "observed_turns":0,
                "allowed_commands":["cargo test"]
            })).unwrap();
            let execution = TaskExecution {
                root: root.join("data"),
                // No native CLI is executed, and no browser resources are present.
                executable: root.join("runtime.exe"),
                projects: vec![Project {
                    id: "project".into(), path: project, name: "fixture".into(),
                    setup: String::new(), default_base: None, setup_default: SetupPolicy::Skip,
                    direct: false, terminals: vec![],
                }],
            };
            Self { execution, task, root }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    impl Fixture {
        fn install_help_fixture(&self, supported: bool) {
            let program = if self.task.spec.agent == "claude-code" { "claude" } else { "codex" };
            #[cfg(windows)]
            {
                let script = if supported {
                    "@echo off\r\nif \"%~1\"==\"resume\" (echo   --help & exit /b 0)\r\nif not \"%~1\"==\"--help\" exit /b 9\r\necho   --model\r\necho   -c\r\necho   --effort\r\nexit /b 0\r\n"
                } else {
                    "@echo off\r\necho   --help\r\nexit /b 0\r\n"
                };
                std::fs::write(self.root.join(format!("{program}.cmd")), script).unwrap();
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let script = if supported {
                    "#!/bin/sh\nif [ \"$1\" = resume ]; then printf '  --help\\n'; exit 0; fi\n[ \"$1\" = --help ] || exit 9\nprintf '  --model\\n  -c\\n  --effort\\n'\n"
                } else {
                    "#!/bin/sh\nprintf '  --help\\n'\n"
                };
                let path = self.root.join(program);
                std::fs::write(&path, script).unwrap();
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
    }

    #[test]
    fn capability_failure_precedes_prepare_and_launch_effects() {
        for agent in ["codex", "claude-code"] {
            let mut fixture = Fixture::new(agent);
            fixture.install_help_fixture(false);
            fixture.task.spec.launch.model = Some("custom/model".into());
            // A checkout that does not yet exist must stay absent on rejection.
            std::fs::remove_dir(&fixture.task.worktree).unwrap();
            let before = serde_json::to_value(&fixture.task).unwrap();
            let error = fixture.execution.prepare(&mut fixture.task).unwrap_err();
            assert!(error.to_string().contains("does not advertise --model"), "{error:#}");
            assert!(!fixture.task.worktree.exists());
            assert!(!fixture.root.join("project/.velella").exists());
            assert_eq!(serde_json::to_value(&fixture.task).unwrap(), before);
            for resume in [false, true] {
                let error = fixture.execution.launch_agent(&mut fixture.task, resume).unwrap_err();
                assert!(error.to_string().contains("does not advertise --model"), "{error:#}");
                assert!(!fixture.root.join("project/.velella").exists());
                assert_eq!(serde_json::to_value(&fixture.task).unwrap(), before);
            }
        }
    }

    #[test]
    fn capability_revalidation_uses_descriptor_path_and_actual_invocation_mode() {
        for agent in ["codex", "claude-code"] {
            let mut fixture = Fixture::new(agent);
            fixture.install_help_fixture(true);
            fixture.task.spec.launch = LaunchOptions { model: Some("custom/model".into()), effort: Some("high".into()) };
            fixture.execution.validate_launch(&fixture.task, false).unwrap();
            if agent == "codex" {
                // This fixture supports top-level flags, but not resume flags.
                let error = fixture.execution.launch_agent(&mut fixture.task, true).unwrap_err();
                assert!(error.to_string().contains("does not advertise --model"), "{error:#}");
            } else {
                fixture.execution.validate_launch(&fixture.task, true).unwrap();
            }
            // Do not cache evidence from prepare/a previous successful probe.
            fixture.install_help_fixture(false);
            let error = fixture.execution.launch_agent(&mut fixture.task, false).unwrap_err();
            assert!(error.to_string().contains("does not advertise --model"), "{error:#}");
            assert!(!fixture.root.join("project/.velella").exists());
        }
    }

    #[test]
    fn empty_execution_options_do_not_even_construct_a_probe_path() {
        let mut fixture = Fixture::new("custom");
        fixture.execution.executable = PathBuf::new();
        assert!(fixture.execution.launch_path().is_err());
        for resume in [false, true] {
            fixture.execution.validate_launch(&fixture.task, resume).unwrap();
        }
    }

    #[test]
    fn launch_options_apply_before_prompt_and_on_resume_without_changing_native_policy() {
        for agent in ["codex", "claude-code"] {
            let mut fixture = Fixture::new(agent);
            fixture.task.spec.launch = LaunchOptions { model: Some("custom/model".into()), effort: Some("high".into()) };
            let launch = crate::native_launch::args(agent, &fixture.task.spec.launch).unwrap();
            for resume in [false, true] {
                let desc = fixture.execution.descriptor(&fixture.task, None, resume).unwrap();
                let args = &desc.spawn.args;
                let start = args.iter().position(|arg| arg == "--model").unwrap();
                assert_eq!(&args[start..start + launch.len()], launch.as_slice());
                assert_eq!(args.iter().filter(|arg| *arg == "--model").count(), 1);
                if resume {
                    assert_eq!(start + launch.len(), args.len());
                    assert!(args.windows(2).any(|pair| pair == [if agent == "codex" { "resume" } else { "--resume" }, "native-session"]));
                } else {
                    assert_eq!(start + launch.len() + 1, args.len());
                    assert!(args.last().unwrap().starts_with("launch fixture\nWork only on this task"));
                }
                assert!(!desc.spawn.sandbox.is_sandboxed());
                if agent == "codex" {
                    assert!(args.windows(2).any(|pair| pair == ["--sandbox", "workspace-write"]));
                    assert!(args.windows(2).any(|pair| pair == ["--ask-for-approval", "on-request"]));
                    assert_eq!(args.iter().filter(|arg| arg.starts_with("hooks.")).count(), 7);
                } else {
                    assert!(args.windows(2).any(|pair| pair == ["--permission-mode", "acceptEdits"]));
                    assert!(args.windows(2).any(|pair| pair == ["--allowedTools", "Bash(cargo test)"]));
                    let settings = args.iter().position(|arg| arg == "--settings").unwrap();
                    let value: Value = serde_json::from_slice(&std::fs::read(&args[settings + 1]).unwrap()).unwrap();
                    assert_eq!(value["hooks"], hook_settings());
                }
            }
        }
    }

    #[test]
    fn launch_options_leave_legacy_defaults_and_command_descriptors_unchanged() {
        for agent in ["codex", "claude-code"] {
            let mut fixture = Fixture::new(agent);
            for resume in [false, true] {
                let desc = fixture.execution.descriptor(&fixture.task, None, resume).unwrap();
                assert!(!desc.spawn.args.iter().any(|arg| arg == "--model" || arg == "--effort" || arg.starts_with("model_reasoning_effort=")));
            }
            fixture.task.spec.launch = LaunchOptions { model: Some("--invalid".into()), effort: Some("invalid".into()) };
            let desc = fixture.execution.descriptor(&fixture.task, Some("fixture command"), false).unwrap();
            let expected: Vec<&str> = if cfg!(windows) {
                vec!["-NoLogo", "-NoProfile", "-Command", "fixture command"]
            } else {
                vec!["-c", "fixture command"]
            };
            assert_eq!(desc.spawn.args, expected);
            assert_eq!(desc.spawn.adapter.as_deref(), Some("generic"));
        }
    }

    #[test]
    fn launch_options_fail_before_creating_a_session_for_invalid_or_unsupported_requests() {
        for (agent, model) in [("codex", "--invalid"), ("claude-code", ""), ("gemini", "custom/model")] {
            let mut fixture = Fixture::new(agent);
            fixture.task.spec.launch.model = Some(model.into());
            for resume in [false, true] {
                assert!(fixture.execution.descriptor(&fixture.task, None, resume).is_err());
                assert!(!fixture.root.join("project/.velella/sessions").exists());
            }
        }
    }
}
