//! Stop only the pinned executor belonging to this restricted terminal.
#[cfg(target_os = "linux")]
pub(crate) fn stop_adapter(directory: &std::path::Path) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::{os::fd::{AsRawFd, FromRawFd, OwnedFd}, time::Duration};
    let path = directory.join("exec-server.pid.json");
    if !path.exists() { return Ok(()); }
    let receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let pid = receipt["pid"].as_i64().filter(|p| *p > 1 && *p <= i32::MAX as i64).context("invalid adapter PID")? as i32;
    let identity = || -> Option<String> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        Some(stat.rsplit_once(") ")?.1.split_whitespace().nth(19)?.to_string())
    };
    if identity().as_deref() != receipt["start"].as_str() { return Ok(()); }
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) { return Ok(()); }
        return Err(error.into());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    if identity().as_deref() != receipt["start"].as_str() { return Ok(()); }
    let sent = unsafe { libc::syscall(libc::SYS_pidfd_send_signal, fd.as_raw_fd(), libc::SIGTERM, std::ptr::null::<libc::siginfo_t>(), 0) };
    anyhow::ensure!(sent == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH), "signal restricted adapter");
    let deadline = std::time::Instant::now() + Duration::from_secs(6);
    loop {
        let mut poll = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let result = unsafe { libc::poll(&mut poll, 1, 100) };
        if result > 0 { return Ok(()); }
        anyhow::ensure!(std::time::Instant::now() < deadline, "restricted adapter cleanup timed out");
        anyhow::ensure!(result >= 0 || std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted, "wait for restricted adapter cleanup");
    }
}
