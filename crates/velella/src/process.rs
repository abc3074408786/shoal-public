//! The session host owns the entire child tree. Losing the host closes its job.
#[cfg(windows)]
pub struct ChildGroup(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for ChildGroup {}
#[cfg(windows)]
unsafe impl Sync for ChildGroup {}
#[cfg(windows)]
impl ChildGroup {
    pub fn attach(pid: u32) -> anyhow::Result<Self> {
        use windows_sys::Win32::{Foundation::CloseHandle, System::{JobObjects::*, Threading::*}};
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            anyhow::ensure!(!job.is_null(), "create process job: {}", std::io::Error::last_os_error());
            let owner = Self(job);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            anyhow::ensure!(SetInformationJobObject(job, JobObjectExtendedLimitInformation, &limits as *const _ as _, std::mem::size_of_val(&limits) as u32) != 0, "set job limits: {}", std::io::Error::last_os_error());
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            anyhow::ensure!(!process.is_null(), "open owned process: {}", std::io::Error::last_os_error());
            let ok = AssignProcessToJobObject(job, process);
            CloseHandle(process);
            anyhow::ensure!(ok != 0, "assign owned process to job: {}", std::io::Error::last_os_error());
            Ok(owner)
        }
    }
    pub fn kill(&self) -> anyhow::Result<()> {
        anyhow::ensure!(unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1) } != 0, "stop process tree: {}", std::io::Error::last_os_error());
        Ok(())
    }
    pub(crate) fn owns(&self,pid:u32)->bool {
        use windows_sys::Win32::{Foundation::CloseHandle,System::{JobObjects::IsProcessInJob,Threading::{OpenProcess,PROCESS_QUERY_LIMITED_INFORMATION}}};
        unsafe {let process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,pid);if process.is_null(){return false}let mut owned=0;let ok=IsProcessInJob(process,self.0,&mut owned);CloseHandle(process);ok!=0&&owned!=0}
    }
}
#[cfg(windows)]
impl Drop for ChildGroup { fn drop(&mut self) { unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0); } } }

#[allow(dead_code)]
pub fn command(program: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; cmd.creation_flags(0x08000000); }
    cmd
}

/// The PTY child is the leader of an otherwise private process group. Restricted
/// children cannot change groups/sessions. Keep the leader unreaped until the
/// group is closed so neither cancel nor cleanup can signal a reused PID.
#[cfg(target_os = "linux")]
pub(crate) struct RestrictedGroup {
    pid: libc::pid_t,
    active: std::sync::Mutex<bool>,
    cleanup_started: std::sync::atomic::AtomicBool,
}

#[cfg(target_os = "linux")]
impl RestrictedGroup {
    pub(crate) fn attach(pid: u32) -> anyhow::Result<Self> {
        let pid = libc::pid_t::try_from(pid)?;
        anyhow::ensure!(pid > 1 && pid != unsafe { libc::getpgrp() }, "restricted child must own a private process group");
        anyhow::ensure!(unsafe { libc::getpgid(pid) } == pid, "restricted child is not its process group leader");
        // Confirm this is our unreaped direct child without consuming its status.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT | libc::WNOHANG) };
        anyhow::ensure!(result == 0, "restricted group child is not owned: {}", std::io::Error::last_os_error());
        Ok(Self { pid, active: std::sync::Mutex::new(true), cleanup_started: std::sync::atomic::AtomicBool::new(false) })
    }

    fn signal(&self) -> anyhow::Result<()> {
        let result = unsafe { libc::kill(-self.pid, libc::SIGKILL) };
        if result < 0 {
            let error = std::io::Error::last_os_error();
            anyhow::ensure!(error.raw_os_error() == Some(libc::ESRCH), "stop restricted process group: {error}");
        }
        Ok(())
    }

    pub(crate) fn kill(&self) -> anyhow::Result<()> {
        let active = self.active.lock().unwrap();
        if *active { self.signal()?; }
        Ok(())
    }

    /// Stop the controller before its executor cleanup so it cannot issue
    /// another tool request while independently grouped tool children exit.
    pub(crate) fn kill_after(&self, cleanup: impl FnOnce() -> anyhow::Result<()>) -> anyhow::Result<()> {
        let active = self.active.lock().unwrap();
        if !*active { return Ok(()); }
        let cleaned = if !self.cleanup_started.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let stopped = (|| -> anyhow::Result<()> {
                let stopped = unsafe { libc::kill(self.pid, libc::SIGSTOP) };
                if stopped < 0 {
                    let error = std::io::Error::last_os_error();
                    anyhow::ensure!(error.raw_os_error() == Some(libc::ESRCH), "stop owned controller: {error}");
                    return Ok(());
                }
                // Signal delivery is asynchronous. Keep the PID unreaped and
                // confirm stopped/exited before draining the independent tools.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                loop {
                    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                    let result = unsafe { libc::waitid(libc::P_PID, self.pid as u32, &mut info,
                        libc::WSTOPPED | libc::WEXITED | libc::WNOWAIT | libc::WNOHANG) };
                    if result == 0 && unsafe { info.si_pid() } == self.pid { return Ok(()); }
                    anyhow::ensure!(result == 0 || std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted,
                        "wait for owned controller stop: {}", std::io::Error::last_os_error());
                    anyhow::ensure!(std::time::Instant::now() < deadline, "owned controller did not stop");
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            })();
            stopped.and_then(|()| cleanup())
        } else { Ok(()) };
        // Always close the owned group, including when executor cleanup fails.
        let killed = self.signal();
        cleaned.and(killed)
    }

    /// The sole reaper must call this before child.wait(). WNOWAIT retains the
    /// leader's zombie PID while we kill descendants and disable later signals.
    pub(crate) fn wait_and_finish(&self) -> anyhow::Result<()> {
        self.wait_and_finish_after(|| Ok(()))
    }

    pub(crate) fn wait_and_finish_after(&self, cleanup: impl FnOnce() -> anyhow::Result<()>) -> anyhow::Result<()> {
        let result = loop {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            if unsafe { libc::waitid(libc::P_PID, self.pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT) } == 0 { break Ok(()); }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted { break Err(error); }
        };
        let mut active = self.active.lock().unwrap();
        // An unexpected external reaper means ownership is no longer provable.
        // Disable signaling before returning that error; never guess at a PID.
        let finish = match result {
            Ok(()) if *active => {
                let cleaned = if !self.cleanup_started.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    cleanup()
                } else { Ok(()) };
                let killed = self.signal();
                cleaned.and(killed)
            },
            Ok(()) => Ok(()),
            Err(error) => Err(anyhow::anyhow!("wait for restricted group leader: {error}")),
        };
        *active = false;
        finish
    }
}

#[cfg(all(test, target_os = "linux"))]
mod restricted_group_tests {
    use super::*;
    use std::{os::unix::process::CommandExt, path::PathBuf, process::{Child, Command, Stdio}, time::{Duration, Instant}};

    struct OwnedFixture { child: Child, group: RestrictedGroup, root: PathBuf }
    impl Drop for OwnedFixture {
        fn drop(&mut self) {
            let _ = self.group.kill();
            let _ = self.group.wait_and_finish();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn running(pid: i32) -> bool {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { return false; };
        stat.rsplit_once(") ").is_some_and(|(_, fields)| !fields.starts_with('Z') && !fields.starts_with('X'))
    }

    fn process_state(pid: i32) -> Option<char> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat.rsplit_once(") ")?.1.chars().next()
    }

    fn eventually(mut check: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if check() { return true; }
            if Instant::now() >= deadline { return false; }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn owned_fixture(cancel: bool) -> (OwnedFixture, i32) {
            let root = std::env::temp_dir().join(format!("vl-group-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir(&root).unwrap();
            let script = r#"import os, pathlib, signal, sys, time
child = os.fork()
if child == 0:
    signal.signal(signal.SIGHUP, signal.SIG_IGN)
    pending = pathlib.Path(sys.argv[1], 'ready.tmp')
    pending.write_text(str(os.getpid()))
    pending.rename(pathlib.Path(sys.argv[1], 'ready'))
    time.sleep(60)
    os._exit(0)
while not pathlib.Path(sys.argv[1], 'ready').exists():
    time.sleep(.01)
if sys.argv[2] == 'cancel':
    signal.signal(signal.SIGHUP, signal.SIG_IGN)
    time.sleep(60)
os._exit(0)
"#;
            let child = Command::new("/usr/bin/python3").args(["-c", script]).arg(&root).arg(if cancel { "cancel" } else { "exit" })
                .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn().unwrap();
            let group = RestrictedGroup::attach(child.id()).unwrap();
            let fixture = OwnedFixture { child, group, root };
            let deadline = Instant::now() + Duration::from_secs(5);
            while !fixture.root.join("ready").exists() && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(10)); }
            let pid: i32 = std::fs::read_to_string(fixture.root.join("ready")).unwrap().parse().unwrap();
            assert!(running(pid), "fixed descendant must be alive before cleanup");
            (fixture, pid)
    }

    #[test]
    fn restricted_group_cancel_and_natural_exit_remove_background_children() {
        for cancel in [true, false] {
            let (mut fixture, pid) = owned_fixture(cancel);
            if cancel { fixture.group.kill().unwrap(); }
            fixture.group.wait_and_finish().unwrap();
            fixture.child.wait().unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            while running(pid) && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(10)); }
            assert!(!running(pid), "background descendant survived group cleanup");
            assert!(!*fixture.group.active.lock().unwrap());
            fixture.group.kill().unwrap(); // repeated cancel never signals a reaped leader
        }
    }

    #[test]
    fn kill_after_stops_the_native_leader_before_cleanup_and_kills_on_callback_error() {
        for fail_cleanup in [false, true] {
            let (mut fixture, descendant) = owned_fixture(true);
            let leader = fixture.child.id() as i32;
            let mut called = 0;
            let mut stopped_at_callback = false;
            let mut descendant_alive_at_callback = false;
            let result = fixture.group.kill_after(|| {
                called += 1;
                // Read-only observation: do not consume wait status from the sole reaper.
                stopped_at_callback = process_state(leader) == Some('T');
                descendant_alive_at_callback = running(descendant);
                if fail_cleanup { anyhow::bail!("fixed executor cleanup failure"); }
                Ok(())
            });
            // Finish and reap before assertions, so an assertion cannot poison the
            // active mutex or leave a stopped fixture behind during unwinding.
            fixture.group.wait_and_finish().unwrap();
            let status = fixture.child.wait().unwrap();
            let descendant_gone = eventually(|| !running(descendant));
            assert_eq!(called, 1);
            assert!(stopped_at_callback, "cleanup ran while the native controller could still run");
            assert!(descendant_alive_at_callback, "group was killed before cleanup ran");
            assert_eq!(result.is_err(), fail_cleanup);
            if let Err(error) = result { assert!(error.to_string().contains("fixed executor cleanup failure")); }
            assert!(!status.success(), "cancelled fixture should have been killed");
            assert!(descendant_gone, "callback failure must not bypass group cleanup");
        }
    }

    #[test]
    fn natural_exit_holds_the_waitable_leader_until_cleanup_and_removes_descendants() {
        for fail_cleanup in [false, true] {
            let (mut fixture, descendant) = owned_fixture(false);
            let leader = fixture.child.id() as i32;
            let mut called = 0;
            let mut zombie_at_callback = false;
            let mut waitable_at_callback = false;
            let mut descendant_alive_at_callback = false;
            let result = fixture.group.wait_and_finish_after(|| {
                called += 1;
                zombie_at_callback = process_state(leader) == Some('Z');
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                let waited = unsafe {
                    libc::waitid(libc::P_PID, leader as libc::id_t, &mut info,
                        libc::WEXITED | libc::WNOWAIT | libc::WNOHANG)
                };
                waitable_at_callback = waited == 0 && unsafe { info.si_pid() } == leader;
                descendant_alive_at_callback = running(descendant);
                if fail_cleanup { anyhow::bail!("fixed natural-exit cleanup failure"); }
                Ok(())
            });
            let zombie_before_reap = process_state(leader) == Some('Z');
            let status = fixture.child.wait().unwrap();
            let descendant_gone = eventually(|| !running(descendant));
            assert_eq!(called, 1);
            assert!(zombie_at_callback && waitable_at_callback, "callback lost the unreaped leader identity");
            assert!(descendant_alive_at_callback, "descendants must remain until cleanup is called");
            assert!(zombie_before_reap, "wait_and_finish_after must not reap the leader");
            assert!(status.success(), "sole child waiter must still receive the original natural exit");
            assert_eq!(result.is_err(), fail_cleanup);
            assert!(descendant_gone, "natural-exit cleanup must remove background descendants");
            assert!(!*fixture.group.active.lock().unwrap());
        }
    }

    #[test]
    fn repeated_kill_after_does_not_repeat_cleanup_before_or_after_the_sole_reaper() {
        for fail_cleanup in [false, true] {
            let (mut fixture, descendant) = owned_fixture(true);
            let calls = std::cell::Cell::new(0);
            let first = fixture.group.kill_after(|| {
                calls.set(calls.get() + 1);
                if fail_cleanup { anyhow::bail!("fixed first cleanup failure"); }
                Ok(())
            });
            // This window precedes wait_and_finish_after: the leader still reserves
            // its PID, but cancellation has already attempted executor cleanup.
            let second = fixture.group.kill_after(|| { calls.set(calls.get() + 1); Ok(()) });
            let finished = fixture.group.wait_and_finish_after(|| { calls.set(calls.get() + 1); Ok(()) });
            fixture.child.wait().unwrap();
            let descendant_gone = eventually(|| !running(descendant));
            let after_reap = fixture.group.kill_after(|| { calls.set(calls.get() + 1); Ok(()) });
            assert_eq!(first.is_err(), fail_cleanup);
            assert!(second.is_ok() && finished.is_ok() && after_reap.is_ok());
            assert_eq!(calls.get(), 1, "cancel and natural reaper must share one cleanup attempt");
            assert!(descendant_gone);
            assert!(!*fixture.group.active.lock().unwrap());
        }
    }

    #[test]
    fn inactive_group_never_stops_or_kills_a_new_owned_pid_identity() {
        let (mut finished, _) = owned_fixture(true);
        finished.group.kill_after(|| Ok(())).unwrap();
        finished.group.wait_and_finish().unwrap();
        finished.child.wait().unwrap();
        let (sentinel, sentinel_descendant) = owned_fixture(true);
        let sentinel_pid = sentinel.child.id() as i32;
        // Deterministically model a stale numeric PID now belonging to a different
        // private fixture. Do not force OS PID reuse or signal any unrelated PID.
        let stale = RestrictedGroup {
            pid: sentinel_pid,
            active: std::sync::Mutex::new(*finished.group.active.lock().unwrap()),
            cleanup_started: std::sync::atomic::AtomicBool::new(false),
        };
        let mut calls = 0;
        stale.kill_after(|| { calls += 1; Ok(()) }).unwrap();
        stale.kill_after(|| { calls += 1; Ok(()) }).unwrap();
        stale.kill().unwrap();
        let stayed_live = (0..5).all(|_| {
            std::thread::sleep(Duration::from_millis(10));
            !matches!(process_state(sentinel_pid), None | Some('T' | 't' | 'Z' | 'X'))
                && running(sentinel_descendant)
        });
        assert_eq!(calls, 0, "inactive handle must not attempt executor cleanup");
        assert!(stayed_live, "inactive handle signalled the replacement PID owner");
        // Both fixtures' Drop implementations close their own groups and remove
        // their temporary trees; stale has no reaper and must not wait on sentinel.
    }

    #[test]
    fn restricted_group_rejects_the_host_process_group() {
        assert!(RestrictedGroup::attach(unsafe { libc::getpgrp() } as u32).is_err());
    }
}
