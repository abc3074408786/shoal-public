//! Own the private transport used by the unmodified native Codex terminal.
//! Only the user-shell RPC is adapted; all execution keeps the fixed policy.
use crate::{restricted_controller::Prepared, host::Descriptor};
use anyhow::Result;
use std::{path::Path, sync::Arc};

#[cfg(target_os = "linux")]
pub(crate) use linux::Bridge;

#[cfg(not(target_os = "linux"))]
pub(crate) struct Bridge;
#[cfg(not(target_os = "linux"))]
impl Bridge {
    pub(crate) fn endpoint(&self) -> String { String::new() }
    pub(crate) fn stop(&self) -> Result<()> { Ok(()) }
}

pub(crate) fn start(
    desc: &Descriptor, executable: &Path, args: &[String], prepared: &Prepared,
) -> Result<Arc<Bridge>> {
    #[cfg(target_os = "linux")]
    { linux::start(desc, executable, args, prepared) }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (desc, executable, args, prepared);
        anyhow::bail!("restricted terminal bridge requires Linux")
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use anyhow::Context;
    use serde_json::{json, Value};
    use std::{
        fs::{self, DirBuilder, OpenOptions},
        io::{Read, Write},
        os::{fd::{AsRawFd, FromRawFd, OwnedFd}, unix::{fs::{DirBuilderExt, OpenOptionsExt}, process::CommandExt}},
        path::PathBuf,
        process::{Child, Command, Stdio},
        sync::Mutex,
        time::{Duration, Instant},
    };

    const SCRIPT: &str = include_str!("../../../scripts/fixtures/restricted-terminal-bridge.py");

    struct PrivateDirectory(PathBuf);
    impl Drop for PrivateDirectory {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    /// Retain a kernel reference, never signal a numeric PID after it can be reused.
    struct PinnedProcess { pid: i32, start: String, fd: OwnedFd }
    impl PinnedProcess {
        fn pin(pid: i32, parent: u32) -> Result<Self> {
            anyhow::ensure!(pid > 1, "invalid bridge child PID");
            let before = Self::identity(pid).context("bridge child disappeared before ownership check")?;
            anyhow::ensure!(before.1 == parent, "bridge child has an unexpected parent");
            anyhow::ensure!(unsafe { libc::getpgid(pid) } == parent as i32,
                "bridge child must remain in its owned process group");
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
            anyhow::ensure!(fd >= 0, "pin bridge child: {}", std::io::Error::last_os_error());
            let fd = unsafe { OwnedFd::from_raw_fd(fd) };
            let after = Self::identity(pid).context("bridge child disappeared while pinning")?;
            anyhow::ensure!(before.1 == after.1 && before.2 == after.2, "bridge child identity changed");
            Ok(Self { pid, start: before.2, fd })
        }

        fn identity(pid: i32) -> Option<(char, u32, String)> {
            let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            let fields: Vec<_> = stat.rsplit_once(") ")?.1.split_whitespace().collect();
            Some((fields.first()?.chars().next()?, fields.get(1)?.parse().ok()?, fields.get(19)?.to_string()))
        }

        fn exited(&self) -> Result<bool> {
            let mut poll = libc::pollfd { fd: self.fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            let result = unsafe { libc::poll(&mut poll, 1, 0) };
            anyhow::ensure!(result >= 0, "poll bridge child: {}", std::io::Error::last_os_error());
            Ok(result > 0)
        }

        fn signal(&self, signal: i32) -> Result<()> {
            let result = unsafe { libc::syscall(libc::SYS_pidfd_send_signal, self.fd.as_raw_fd(),
                signal, std::ptr::null::<libc::siginfo_t>(), 0) };
            anyhow::ensure!(result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH),
                "signal owned bridge child: {}", std::io::Error::last_os_error());
            Ok(())
        }

        fn suspend(&self) -> Result<()> {
            self.signal(libc::SIGSTOP)?;
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if self.exited()? { return Ok(()); }
                if let Some((state, _, start)) = Self::identity(self.pid) {
                    if start == self.start && matches!(state, 'T' | 't' | 'Z' | 'X') { return Ok(()); }
                }
                anyhow::ensure!(Instant::now() < deadline, "app-server did not stop before executor cleanup");
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn terminate(&self) -> Result<()> {
            self.signal(libc::SIGTERM)?;
            let deadline = Instant::now() + Duration::from_secs(6);
            while !self.exited()? {
                anyhow::ensure!(Instant::now() < deadline, "shell executor cleanup timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(())
        }
    }

    struct Running {
        child: Child,
        group: crate::process::RestrictedGroup,
        app_server: Option<PinnedProcess>,
        shell_adapter: Option<PinnedProcess>,
        state: PathBuf,
        generation: String,
        published: bool,
        finished: bool,
    }

    impl Running {
        fn stop(&mut self) -> Result<()> {
            if self.finished { return Ok(()); }
            // Freeze both request producers before asking either executor to
            // drain independently grouped tools. The bridge stays unreaped.
            let cleanup = self.group.kill_after(|| {
                let freeze = self.app_server.as_ref().map_or(Ok(()), PinnedProcess::suspend);
                let shell = self.shell_adapter.as_ref().map_or(Ok(()), PinnedProcess::terminate);
                let model = crate::restricted_cleanup::stop_adapter(&self.state);
                freeze.and(shell).and(model)
            });
            let finish = self.group.wait_and_finish();
            let reaped = self.child.wait().context("reap terminal bridge");
            self.finished = true;
            let result = cleanup.and(finish).and(reaped.map(|_| ()));
            result?;
            if self.published {
                if let Some(shell) = &self.shell_adapter {
                    verify_cleanup(&self.state, shell.pid, &shell.start)?;
                }
                let model_path = self.state.join("exec-server.pid.json");
                if model_path.try_exists()? {
                    let model: Value = serde_json::from_slice(&crate::terminal_recovery::read_private(&model_path)?)?;
                    let pid = model["pid"].as_i64().filter(|p| *p > 1 && *p <= i32::MAX as i64)
                        .context("invalid model executor PID")? as i32;
                    let start = model["start"].as_str().context("model executor identity missing")?;
                    verify_cleanup(&self.state, pid, start)?;
                }
                crate::terminal_recovery::stopped(&self.state, &self.generation)?;
            }
            Ok(())
        }
    }

    impl Drop for Running {
        fn drop(&mut self) { let _ = self.stop(); }
    }

    pub(crate) struct Bridge {
        endpoint: String,
        running: Mutex<Option<Running>>,
        socket_directory: Mutex<Option<PrivateDirectory>>,
    }

    impl Bridge {
        pub(crate) fn endpoint(&self) -> String { self.endpoint.clone() }
        pub(crate) fn stop(&self) -> Result<()> {
            let mut running = self.running.lock().unwrap();
            let result = if let Some(mut process) = running.take() { process.stop() } else { Ok(()) };
            self.socket_directory.lock().unwrap().take();
            result
        }
    }

    impl Drop for Bridge {
        fn drop(&mut self) { let _ = self.stop(); }
    }

    fn private_new(path: &Path, bytes: &[u8]) -> Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(path)?;
        file.write_all(bytes)?;
        Ok(())
    }

    fn verify_cleanup(state: &Path, pid: i32, start: &str) -> Result<()> {
        let value: Value = serde_json::from_slice(&crate::terminal_recovery::read_private(
            &state.join(format!("exec-server-cleanup-{pid}.json")))?)
            .context("executor did not confirm complete process cleanup")?;
        anyhow::ensure!(value["schema"] == 1 && value["pid"] == pid
            && value["start"] == start && value["clean"] == true,
            "executor cleanup receipt does not match this terminal");
        Ok(())
    }

    fn ready(process: &mut Running) -> Result<Value> {
        let output = process.child.stdout.as_mut().context("bridge readiness pipe missing")?;
        let fd = output.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        anyhow::ensure!(flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0,
            "configure bridge readiness pipe");
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut bytes = Vec::new();
        loop {
            let mut buffer = [0u8; 2048];
            match output.read(&mut buffer) {
                Ok(0) => anyhow::bail!("restricted terminal bridge stopped during startup"),
                Ok(count) => bytes.extend_from_slice(&buffer[..count]),
                Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {},
                Err(error) => return Err(error).context("read bridge readiness"),
            }
            anyhow::ensure!(bytes.len() <= 8192, "bridge readiness exceeded size limit");
            if let Some(end) = bytes.iter().position(|byte| *byte == b'\n') {
                return serde_json::from_slice(&bytes[..end]).context("invalid bridge readiness");
            }
            anyhow::ensure!(Instant::now() < deadline, "restricted terminal bridge startup timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    pub(super) fn start(
        desc: &Descriptor, executable: &Path, args: &[String], prepared: &Prepared,
    ) -> Result<Arc<Bridge>> {
        // Short path avoids Linux's sockaddr_un length limit for deep workspaces.
        let socket_root = Path::new("/tmp").join(format!("vl-tui-{}", uuid::Uuid::now_v7()));
        DirBuilder::new().mode(0o700).create(&socket_root)?;
        let socket_directory = PrivateDirectory(socket_root);
        let socket = socket_directory.0.join("rpc.sock");
        let script = desc.directory.join("terminal-bridge.py");
        if prepared.resume_thread.is_some() {
            anyhow::ensure!(crate::terminal_recovery::read_private(&script)? == SCRIPT.as_bytes(),
                "受限终端桥接程序已经变化，请新建终端");
            // Invalidate the old success marker before any replacement child
            // starts. A crash before publishing its receipt cannot reuse it.
            crate::terminal_recovery::read_private(&desc.directory.join("terminal-stopped.json"))?;
            fs::remove_file(desc.directory.join("terminal-stopped.json"))?;
            let pid_receipt = desc.directory.join("exec-server.pid.json");
            if pid_receipt.try_exists()? {
                crate::terminal_recovery::read_private(&pid_receipt)?;
                fs::remove_file(pid_receipt)?;
            }
            fs::File::open(&desc.directory)?.sync_all()?;
        } else { private_new(&script, SCRIPT.as_bytes())?; }
        let generation = uuid::Uuid::now_v7().to_string();
        let mut command = Command::new("/usr/bin/python3");
        command.arg("-I").arg(script).arg("--socket").arg(&socket)
            .arg("--codex").arg(executable).arg("--config").arg(desc.directory.join("exec-server.json"))
            .arg("--generation").arg(&generation);
        if let Some(thread) = &prepared.resume_thread { command.arg("--resume-thread").arg(thread); }
        if let Some(source) = &prepared.chatgpt_auth_source {
            command.arg("--chatgpt-auth-source").arg(source);
        }
        for arg in args { command.arg(format!("--codex-arg={arg}")); }
        let mut child = command.current_dir(&prepared.cwd).env_clear().envs(&prepared.env)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .process_group(0).spawn().context("start restricted native-terminal bridge")?;
        let group = match crate::process::RestrictedGroup::attach(child.id()) {
            Ok(group) => group,
            Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error); }
        };
        let mut process = Running {
            child, group, app_server: None, shell_adapter: None, state: desc.directory.clone(), finished: false,
            generation: generation.clone(), published: false,
        };
        let receipt = ready(&mut process)?;
        anyhow::ensure!(receipt["ready"] == true && receipt["pid"].as_u64() == Some(process.child.id() as u64)
            && receipt["socket"].as_str() == socket.to_str(), "bridge readiness identity mismatch");
        for (key, slot) in [
            ("app_server_pid", &mut process.app_server), ("shell_adapter_pid", &mut process.shell_adapter),
        ] {
            let pid = receipt[key].as_i64().filter(|pid| *pid > 1 && *pid <= i32::MAX as i64)
                .context("bridge child receipt missing")? as i32;
            *slot = Some(PinnedProcess::pin(pid, process.child.id())?);
        }
        crate::terminal_recovery::write_atomic(&desc.directory.join("terminal-bridge.json"), &serde_json::to_vec_pretty(&json!({
            "socket": socket, "pid": process.child.id(),
            "generation": generation,
            "app_server_pid": process.app_server.as_ref().unwrap().pid,
            "shell_adapter_pid": process.shell_adapter.as_ref().unwrap().pid,
            "transport": "private-unix-websocket", "native_tui": true,
            "shell_execution": "fixed-restricted-executor",
            "existing_chatgpt_login": prepared.chatgpt_auth_source.is_some(),
        }))?)?;
        process.published = true;
        Ok(Arc::new(Bridge {
            endpoint: format!("unix://{}", socket.display()),
            running: Mutex::new(Some(process)), socket_directory: Mutex::new(Some(socket_directory)),
        }))
    }
}
