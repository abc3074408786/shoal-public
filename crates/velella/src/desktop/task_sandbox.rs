//! Task-owned resources for sandboxed execution. No host credentials or control
//! sockets are exposed. Agents use an event-only socket or a private file inbox.
use super::*;
use crate::sandbox::{self, Policy};
use anyhow::Context as _;

pub(super) struct Context {
    pub policy: Policy,
    pub hook_command: String,
    pub hook_endpoint: Option<String>,
    pub hook_inbox: Option<PathBuf>,
    pub settings_directory: Option<PathBuf>,
}

fn private_directory(path: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(path)?;
    let canonical = path.canonicalize()?;
    anyhow::ensure!(
        canonical == path,
        "sandbox state must not contain symlink aliases"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(canonical)
}

pub(super) fn configure(
    root: &Path,
    executable: &Path,
    project: &Project,
    task: &Task,
    session: &str,
    agent: bool,
) -> Result<Context> {
    let mut policy = task.spec.policy.execution.clone();
    if !policy.is_sandboxed() {
        return Ok(Context {
            policy,
            hook_command: "velella-runtime hook".into(),
            hook_endpoint: None,
            hook_inbox: None,
            settings_directory: None,
        });
    }
    anyhow::ensure!(!task.direct, "isolated tasks require a Git worktree");
    // IDs become filesystem components, never accept an arbitrary persisted path.
    uuid::Uuid::parse_str(&task.id).context("invalid sandbox task ID")?;
    uuid::Uuid::parse_str(session).context("invalid sandbox session ID")?;
    let root = root.canonicalize()?;
    let restricted = policy.is_restricted();
    let (read_only, writable, home) = match &mut policy {
        Policy::Isolated { read_only, writable, home, .. } | Policy::Restricted { read_only, writable, home, .. } => (read_only, writable, home),
        Policy::Native {} => unreachable!(),
    };
    anyhow::ensure!(home.is_none(), "task HOME is managed by Velella");
    for path in read_only.iter().chain(writable.iter()) {
        let path = path.canonicalize()?;
        anyhow::ensure!(
            !path.starts_with(&root) && !root.starts_with(&path),
            "task mounts must not expose Velella runtime state"
        );
    }
    let state = private_directory(&root.join("sandbox-tasks").join(&task.id))?;
    *home = Some(private_directory(&state.join("home"))?);
    // Worktree .git points into the main repository. Read-only access enables
    // status/diff without granting writes to branches, hooks or sibling worktrees.
    let git_dir = PathBuf::from(
        git(
            &project.path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?
        .trim(),
    )
    .canonicalize()?;
    if !read_only.contains(&git_dir) {
        read_only.push(git_dir);
    }
    let mut result = Context {
        policy: Policy::default(),
        hook_command: String::new(),
        hook_endpoint: None,
        hook_inbox: None,
        settings_directory: None,
    };
    if agent {
        let tools = private_directory(&state.join("tools"))?;
        let runtime = tools.join("velella-runtime");
        if !runtime.exists() {
            let temporary = tools.join(format!("runtime-{}.tmp", uuid::Uuid::now_v7()));
            std::fs::copy(executable, &temporary).context("stage sandbox hook runtime")?;
            std::fs::rename(&temporary, &runtime)?;
        }
        anyhow::ensure!(
            runtime.canonicalize()? == runtime,
            "sandbox runtime must not be a symlink"
        );
        // A short, per-session path keeps Unix socket names within platform limits.
        let support = private_directory(&root.join("sb").join(session))?;
        let settings = if restricted { private_directory(&support.join("settings"))? } else { support.clone() };
        if restricted {
            let inbox = private_directory(&support.join("inbox"))?;
            writable.push(inbox.clone());
            result.hook_inbox = Some(inbox);
        } else {
            let endpoint = support.join("h.sock").to_string_lossy().into_owned();
            anyhow::ensure!(endpoint.len() < 104, "sandbox hook socket path is too long; use a shorter runtime data directory");
            result.hook_endpoint = Some(endpoint);
        }
        result.hook_command = format!(
            "'{}' hook",
            runtime.to_string_lossy().replace('\'', "'\"'\"'")
        );
        result.settings_directory = Some(settings.clone());
        read_only.extend([tools, settings]);
    }
    result.policy = policy;
    Ok(result)
}

pub(super) fn agent_program(program: &str, path: &str, policy: &Policy) -> Result<String> {
    let read_only = match policy {
        Policy::Isolated { read_only, .. } | Policy::Restricted { read_only, .. } => read_only,
        Policy::Native {} => return Ok(program.into()),
    };
    let executable = std::env::split_paths(path)
        .map(|dir| dir.join(program))
        .find(|p| p.is_file())
        .with_context(|| format!("{program} is not installed on PATH"))?
        .canonicalize()?;
    let system = ["/usr", "/bin", "/sbin", "/lib", "/lib64"]
        .iter()
        .any(|p| executable.starts_with(p));
    anyhow::ensure!(system || read_only.iter().any(|p| executable.starts_with(p)),
        "agent executable {} is outside the sandbox; add its installation directory to policy.execution.read_only", executable.display());
    Ok(executable.to_string_lossy().into_owned())
}

pub(super) fn validate(context: &Context, cwd: &Path) -> Result<()> {
    sandbox::validate(
        &context.policy,
        cwd.to_str().context("task cwd must be UTF-8")?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::Network;

    struct Fixture {
        root: PathBuf,
        project: Project,
        task: Task,
        runtime: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("vl-sb-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(root.join("data")).unwrap();
            let root = root.canonicalize().unwrap();
            let repo = root.join("repo");
            std::fs::create_dir(&repo).unwrap();
            git(&repo, &["init", "-q"]).unwrap();
            let id = uuid::Uuid::now_v7().to_string();
            let worktree = repo.join(".velella/worktrees").join(&id);
            std::fs::create_dir_all(&worktree).unwrap();
            let project = Project {
                id: "p".into(),
                path: repo,
                name: "fixture".into(),
                setup: "echo setup".into(),
                default_base: None,
                setup_default: SetupPolicy::Skip,
                direct: false,
                terminals: vec![],
            };
            let mut spec =
                TaskSpec::from_yaml_str("name: fixture\nagent: codex\ninstruction: test\n")
                    .unwrap();
            spec.policy.execution = Policy::Isolated {
                read_only: vec![],
                writable: vec![],
                network: Network::Denied,
                home: None,
            };
            let task = Task {
                id,
                project_id: "p".into(),
                spec,
                phase: Phase::Queued,
                worktree,
                branch: "fixture".into(),
                base_oid: String::new(),
                agent: None,
                command: None,
                native_id: None,
                deadline: None,
                created_at: String::new(),
                reason: String::new(),
                verified: false,
                checks: vec![],
                retries: 0,
                observed_turns: 0,
                status: None,
                pr_url: None,
                allowed_commands: vec![],
                shell: None,
                direct: false,
                delivery: None,
                parent_workspace_id: None,
                setup_snapshot: None,
                creation_id: None,
            };
            let runtime = root.join("runtime");
            std::fs::write(&runtime, "fixture runtime").unwrap();
            Self {
                root,
                project,
                task,
                runtime,
            }
        }
        fn execution(&self) -> task_execution::TaskExecution {
            task_execution::TaskExecution {
                root: self.root.join("data"),
                executable: self.runtime.clone(),
                projects: vec![self.project.clone()],
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn command_descriptors_preserve_isolation_and_task_home_across_phases() {
        let f = Fixture::new();
        let execution = f.execution();
        let setup = execution
            .descriptor(&f.task, Some("echo setup"), false)
            .unwrap();
        let check = execution
            .descriptor(&f.task, Some("echo check"), false)
            .unwrap();
        let shell = execution.descriptor(&f.task, Some(""), false).unwrap();
        assert!(setup.spawn.sandbox.is_isolated());
        assert_eq!(setup.spawn.sandbox, check.spawn.sandbox);
        assert_eq!(setup.spawn.sandbox, shell.spawn.sandbox);
        assert!(!setup.spawn.env.contains_key("VELELLA_SESSION_ENDPOINT"));
        let Policy::Isolated {
            home: Some(home),
            read_only,
            ..
        } = &setup.spawn.sandbox
        else {
            panic!("persistent HOME required")
        };
        assert!(home.is_dir());
        assert!(read_only.contains(&f.project.path.join(".git")));
        std::fs::write(home.join("session-state"), "retained").unwrap();
        let recovered = execution
            .descriptor(&f.task, Some("echo recovered"), true)
            .unwrap();
        assert_eq!(setup.spawn.sandbox, recovered.spawn.sandbox);
        assert_eq!(
            std::fs::read_to_string(home.join("session-state")).unwrap(),
            "retained"
        );
        let persisted = serde_json::to_vec(&f.task).unwrap();
        let restored: Task = serde_json::from_slice(&persisted).unwrap();
        assert_eq!(restored.spec.policy.execution, f.task.spec.policy.execution);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn agent_support_is_read_only_scoped_and_separate_from_command_sessions() {
        let f = Fixture::new();
        let session = uuid::Uuid::now_v7().to_string();
        let ctx = configure(
            &f.root.join("data"),
            &f.runtime,
            &f.project,
            &f.task,
            &session,
            true,
        )
        .unwrap();
        validate(&ctx, &f.task.worktree).unwrap();
        let support = ctx.settings_directory.as_ref().unwrap();
        let Policy::Isolated {
            read_only,
            writable,
            home,
            ..
        } = &ctx.policy
        else {
            panic!()
        };
        assert!(read_only.contains(support));
        assert!(writable.is_empty());
        assert!(home.is_some());
        assert!(ctx.hook_endpoint.as_ref().unwrap().ends_with("/h.sock"));
        assert!(!read_only.contains(&f.root.join("data")));
        assert!(ctx.hook_command.ends_with("' hook"));
        let hooks = hook_settings_for(&ctx.hook_command);
        assert_eq!(hooks["Stop"][0]["hooks"][0]["command"], ctx.hook_command);
        let command_ctx = configure(
            &f.root.join("data"),
            &f.runtime,
            &f.project,
            &f.task,
            &uuid::Uuid::now_v7().to_string(),
            false,
        )
        .unwrap();
        assert!(command_ctx.hook_endpoint.is_none());
        assert!(!serde_json::to_string(&command_ctx.policy)
            .unwrap()
            .contains("/sb/"));
    }

    fn restricted_fixture() -> Fixture {
        let mut f = Fixture::new();
        f.task.spec.policy.execution = Policy::Restricted { read_only: vec![], writable: vec![], network: Network::Denied, home: None };
        f
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn restricted_command_phases_preserve_policy_home_and_no_hook_transport() {
        let f = restricted_fixture();
        let execution = f.execution();
        let setup = execution.descriptor(&f.task, Some("echo setup"), false).unwrap();
        let verify = execution.descriptor(&f.task, Some("echo verify"), false).unwrap();
        let shell = execution.descriptor(&f.task, Some(""), false).unwrap();
        let resumed = execution.descriptor(&f.task, Some("echo resumed"), true).unwrap();
        assert!(setup.spawn.sandbox.is_restricted());
        for desc in [&setup, &verify, &shell, &resumed] {
            assert_eq!(desc.spawn.sandbox, setup.spawn.sandbox);
            assert!(!desc.spawn.env.contains_key("VELELLA_SESSION_ENDPOINT"));
            assert!(!desc.spawn.env.contains_key("VELELLA_HOOK_INBOX"));
        }
        let Policy::Restricted { home: Some(home), read_only, .. } = &setup.spawn.sandbox else { panic!("task HOME required") };
        assert_eq!(home, &f.root.join("data/sandbox-tasks").join(&f.task.id).join("home"));
        assert!(read_only.contains(&f.project.path.join(".git")));
        assert!(home.is_dir());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn restricted_agent_gets_only_private_inbox_and_read_only_support() {
        let f = restricted_fixture();
        let session = uuid::Uuid::now_v7().to_string();
        let ctx = configure(&f.root.join("data"), &f.runtime, &f.project, &f.task, &session, true).unwrap();
        validate(&ctx, &f.task.worktree).unwrap();
        let inbox = ctx.hook_inbox.as_ref().unwrap();
        let settings = ctx.settings_directory.as_ref().unwrap();
        let Policy::Restricted { read_only, writable, home: Some(home), .. } = &ctx.policy else { panic!() };
        assert_eq!(inbox, &f.root.join("data/sb").join(session).join("inbox"));
        assert_eq!(writable, &[inbox.clone()]);
        assert!(read_only.contains(settings));
        assert!(read_only.contains(&home.parent().unwrap().join("tools")));
        assert!(!read_only.iter().chain(writable).any(|p| p == &f.root.join("data")));
        assert!(ctx.hook_endpoint.is_none());
        assert!(ctx.hook_command.ends_with("' hook"));
        let command = configure(&f.root.join("data"), &f.runtime, &f.project, &f.task, &uuid::Uuid::now_v7().to_string(), false).unwrap();
        assert!(command.hook_inbox.is_none());
        assert!(command.settings_directory.is_none());
        assert!(!serde_json::to_string(&command.policy).unwrap().contains("/sb/"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn restricted_rejects_user_home_runtime_access_and_direct_tasks() {
        for scenario in ["home", "runtime", "direct"] {
            let mut f = restricted_fixture();
            if scenario == "direct" { f.task.direct = true; }
            let Policy::Restricted { home, writable, .. } = &mut f.task.spec.policy.execution else { unreachable!() };
            if scenario == "home" { *home = Some(f.root.clone()); }
            if scenario == "runtime" { writable.push(f.root.join("data")); }
            assert!(configure(&f.root.join("data"), &f.runtime, &f.project, &f.task, &uuid::Uuid::now_v7().to_string(), false).is_err(), "{scenario}");
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn rejects_runtime_mounts_and_direct_tasks() {
        let mut f = Fixture::new();
        if let Policy::Isolated { writable, .. } = &mut f.task.spec.policy.execution {
            writable.push(f.root.join("data"));
        }
        assert!(configure(
            &f.root.join("data"),
            &f.runtime,
            &f.project,
            &f.task,
            &uuid::Uuid::now_v7().to_string(),
            false
        )
        .is_err());
        f.task.direct = true;
        assert!(configure(
            &f.root.join("data"),
            &f.runtime,
            &f.project,
            &f.task,
            &uuid::Uuid::now_v7().to_string(),
            false
        )
        .is_err());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn unavailable_backend_refuses_before_worktree_or_setup_mutation() {
        let mut f = Fixture::new();
        if sandbox::capabilities().available {
            return;
        }
        std::fs::remove_dir(&f.task.worktree).unwrap();
        let error = f.execution().prepare(&mut f.task).unwrap_err();
        assert!(format!("{error:#}").contains("refusing native fallback"));
        assert!(!f.task.worktree.exists());
        assert_eq!(f.task.phase, Phase::Queued);
        assert!(f.task.command.is_none());
    }
    #[test]
    #[ignore = "requires permitted Linux namespaces and VELELLA_TEST_RUNTIME"]
    #[cfg(target_os = "linux")]
    fn live_task_pipeline_with_fixture_agent() {
        use std::os::unix::fs::PermissionsExt;
        assert!(sandbox::capabilities().available, "real sandbox required");
        let runtime = PathBuf::from(std::env::var_os("VELELLA_TEST_RUNTIME").expect("built runtime required"));
        let mut f = Fixture::new();
        let tools = f.root.join("tools");
        std::fs::create_dir(&tools).unwrap();
        f.runtime = tools.join("velella-runtime");
        std::fs::copy(runtime, &f.runtime).unwrap();
        let agent = tools.join("claude");
        std::fs::write(&agent, include_str!("task_sandbox_fixture.py")).unwrap();
        std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
        if let Policy::Isolated { read_only, .. } = &mut f.task.spec.policy.execution {
            read_only.push(tools);
        }
        f.task.spec.agent = "claude-code".into();
        f.task.spec.verify.retries = 0;
        f.project.setup = "printf prepared > setup-ok; printf retained > \"$HOME/setup-state\"".into();
        f.task.checks = vec![Check { command: "test \"$(cat agent-result)\" = verified && test \"$(cat \"$HOME/setup-state\")\" = retained".into(), passed: None, exit: None, output: String::new() }];
        git(&f.project.path, &["-c", "user.name=Sandbox Test", "-c", "user.email=sandbox@example.invalid", "commit", "--allow-empty", "-m", "fixture"]).unwrap();
        f.task.base_oid = git(&f.project.path, &["rev-parse", "HEAD"]).unwrap().trim().into();
        std::fs::remove_dir(&f.task.worktree).unwrap();
        let execution = f.execution();
        struct Hosts(Vec<Descriptor>);
        impl Drop for Hosts {
            fn drop(&mut self) { for desc in &self.0 { let _ = stop_host(desc); } }
        }
        let mut hosts = Hosts(vec![]);
        execution.prepare(&mut f.task).unwrap();
        let deadline = Instant::now() + Duration::from_secs(40);
        while f.task.phase != Phase::Review && Instant::now() < deadline {
            let mut observations = HashMap::new();
            for desc in [&f.task.command, &f.task.agent, &f.task.shell].into_iter().flatten() {
                if !hosts.0.iter().any(|d| d.id == desc.id) { hosts.0.push(desc.clone()); }
                observations.insert(desc.id.clone(), get_status(desc).map_err(|e| format!("{e:#}")));
            }
            execution.advance(&mut f.task, &observations).unwrap();
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(f.task.phase, Phase::Review, "{}", f.task.reason);
        assert!(f.task.verified);
        assert_eq!(f.task.checks[0].passed, Some(true));
        assert!(f.task.native_id.is_some());
        execution.stop(&f.task).unwrap();
        f.task.phase = Phase::NeedsRecovery;
        // Simulate daemon recovery from the persisted representation.
        let mut restored: Task = serde_json::from_slice(&serde_json::to_vec(&f.task).unwrap()).unwrap();
        execution.command(&mut restored, &json!({"cmd":"task.resume"})).unwrap();
        hosts.0.push(restored.agent.clone().unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        while !restored.worktree.join("resumed-ok").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(std::fs::read_to_string(restored.worktree.join("resumed-ok")).unwrap(), restored.native_id.clone().unwrap());
        assert!(restored.agent.as_ref().unwrap().spawn.sandbox.is_isolated());
        assert!(get_status(restored.agent.as_ref().unwrap()).unwrap().human_control);
        execution.stop(&restored).unwrap();
    }

}
