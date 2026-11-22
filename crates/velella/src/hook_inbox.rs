//! A Linux, event-only bridge for tasks whose seccomp policy rejects all sockets.
//! All operations stay relative to a directory descriptor opened before the child.
use anyhow::{Context, Result};
use fs2::FileExt;
use serde_json::Value;
use std::{ffi::{CStr, CString}, fs::File, io::{Read, Write}, os::{fd::{AsRawFd, FromRawFd}, unix::{ffi::OsStrExt, fs::MetadataExt}}, path::Path, time::{Duration, Instant}};

const MAX_EVENT: u64 = 4096;
const MAX_BATCH: usize = 32;
const MAX_SCAN: usize = 256;

pub(super) struct Inbox { directory: File }

impl Inbox {
    pub(super) fn open(path: &Path) -> Result<Self> {
        anyhow::ensure!(path.is_absolute() && path.canonicalize()? == path, "hook inbox must be a canonical directory");
        let name = CString::new(path.as_os_str().as_bytes())?;
        let fd = unsafe { libc::open(name.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
        anyhow::ensure!(fd >= 0, "cannot open hook inbox: {}", std::io::Error::last_os_error());
        let directory = unsafe { File::from_raw_fd(fd) };
        let meta = directory.metadata()?;
        anyhow::ensure!(meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0, "hook inbox must be private and owned by this user");
        Ok(Self { directory })
    }

    fn open_file(&self, name: &str, create: bool) -> Result<File> {
        anyhow::ensure!(!name.is_empty() && !name.contains('/') && name != "." && name != "..", "invalid inbox filename");
        let name = CString::new(name)?;
        let flags = libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK | if create { libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL } else { libc::O_RDONLY };
        let fd = unsafe { libc::openat(self.directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
        if fd < 0 { return Err(std::io::Error::last_os_error().into()); }
        let file = unsafe { File::from_raw_fd(fd) };
        let meta = file.metadata()?;
        anyhow::ensure!(meta.is_file() && meta.nlink() == 1 && meta.len() <= MAX_EVENT, "invalid hook event file");
        Ok(file)
    }

    fn remove(&self, name: &str) {
        if let Ok(name) = CString::new(name) { unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0); } }
    }

    fn atomic(&self, name: &str, content: &[u8]) -> Result<()> {
        anyhow::ensure!(content.len() <= MAX_EVENT as usize, "hook event too large");
        let temp = format!("{}.pending", uuid::Uuid::now_v7());
        let result = (|| {
            let mut file = self.open_file(&temp, true)?;
            file.write_all(content)?;
            file.sync_all()?;
            drop(file);
            let from = CString::new(temp.as_str())?;
            let to = CString::new(name)?;
            // rename changes the directory entry itself; it never follows a target symlink.
            let rc = unsafe { libc::renameat(self.directory.as_raw_fd(), from.as_ptr(), self.directory.as_raw_fd(), to.as_ptr()) };
            anyhow::ensure!(rc == 0, "cannot publish hook event: {}", std::io::Error::last_os_error());
            Ok(())
        })();
        if result.is_err() { self.remove(&temp); }
        result
    }

    fn bytes(&self, name: &str) -> Result<Vec<u8>> {
        let file = self.open_file(name, false)?;
        let mut data = Vec::new();
        (&file).take(MAX_EVENT + 1).read_to_end(&mut data)?;
        let meta = file.metadata()?;
        anyhow::ensure!(data.len() <= MAX_EVENT as usize && meta.is_file() && meta.nlink() == 1, "invalid hook event file");
        Ok(data)
    }

    fn events(&self) -> Result<Vec<String>> {
        // Opening '.' creates a fresh directory offset, unlike dup(fd).
        let dot = c".";
        let fd = unsafe { libc::openat(self.directory.as_raw_fd(), dot.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC) };
        anyhow::ensure!(fd >= 0, "cannot scan hook inbox");
        let stream = unsafe { libc::fdopendir(fd) };
        if stream.is_null() { unsafe { libc::close(fd); } anyhow::bail!("cannot scan hook inbox"); }
        struct Directory(*mut libc::DIR);
        impl Drop for Directory { fn drop(&mut self) { unsafe { libc::closedir(self.0); } } }
        let stream = Directory(stream);
        let mut events = Vec::new();
        for _ in 0..MAX_SCAN {
            let entry = unsafe { libc::readdir(stream.0) };
            if entry.is_null() { break; }
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_string_lossy();
            if let Some(id) = name.strip_suffix(".event") {
                if uuid::Uuid::parse_str(id).is_ok() { events.push(name.into_owned()); }
            }
        }
        events.sort_unstable();
        events.truncate(MAX_BATCH);
        Ok(events)
    }

    pub(super) fn drain(&self, mut dispatch: impl FnMut(Value) -> Result<()>) -> Result<usize> {
        let names = self.events()?;
        let count = names.len();
        for name in names {
            let result = self.bytes(&name)
                .and_then(|bytes| serde_json::from_slice(&bytes).context("invalid hook event JSON"))
                .and_then(&mut dispatch);
            self.remove(&name);
            let ack = format!("{}.ack", name.trim_end_matches(".event"));
            self.atomic(&ack, if result.is_ok() { b"ok" } else { b"rejected" })?;
        }
        Ok(count)
    }

    pub(super) fn submit(&self, request: &Value) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match self.directory.try_lock_exclusive() {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => return Err(error).context("hook event submission lock unavailable"),
            }
        }
        // The lock serializes helpers. Each helper waits for dispatch acknowledgement
        // before the next hook can publish, preserving SessionStart -> Stop ordering.
        let id = uuid::Uuid::now_v7();
        let event = format!("{id}.event");
        let ack = format!("{id}.ack");
        let result = (|| {
            self.atomic(&event, &serde_json::to_vec(request)?)?;
            loop {
                match self.bytes(&ack) {
                    Ok(bytes) => { anyhow::ensure!(bytes == b"ok", "hook event rejected"); return Ok(()); },
                    Err(error) if error.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => {},
                    Err(error) => return Err(error),
                }
                anyhow::ensure!(Instant::now() < deadline, "hook event acknowledgement timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
        })();
        self.remove(&event);
        self.remove(&ack);
        let _ = FileExt::unlock(&self.directory);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::{PermissionsExt, symlink};
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self { let p = std::env::temp_dir().join(format!("vl-inbox-{}", uuid::Uuid::now_v7())); std::fs::create_dir(&p).unwrap(); std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap(); Self(p) }
        fn event(&self) -> String { format!("{}.event", uuid::Uuid::now_v7()) }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
    #[test]
    fn inbox_acknowledges_events_in_order_and_rejects_control() {
        let f = Fixture::new(); let inbox = Inbox::open(&f.0).unwrap(); let path = f.0.clone();
        let producer = std::thread::spawn(move || { let client = Inbox::open(&path).unwrap(); for event in ["SessionStart", "Stop"] { client.submit(&json!({"cmd":"hook", "event":{"hook_event_name":event}})).unwrap(); } assert!(client.submit(&json!({"cmd":"input", "text":"untrusted"})).is_err()); });
        let mut events = Vec::new(); let deadline = Instant::now() + Duration::from_secs(5);
        while !producer.is_finished() && Instant::now() < deadline { inbox.drain(|request| { let request = super::super::sandbox_hook_request(&request)?; events.push(request["event"]["hook_event_name"].as_str().unwrap().to_string()); Ok(()) }).unwrap(); std::thread::sleep(Duration::from_millis(5)); }
        assert!(producer.is_finished()); producer.join().unwrap(); assert_eq!(events, ["SessionStart", "Stop"]);
        assert_eq!(std::fs::read_dir(&f.0).unwrap().count(), 0);
    }
    #[test]
    fn inbox_rejects_symlink_fifo_hardlink_oversize_and_malformed() {
        let f = Fixture::new(); let inbox = Inbox::open(&f.0).unwrap(); let private = Fixture::new(); let secret = private.0.join("secret"); std::fs::write(&secret, "private").unwrap();
        let link = f.event(); symlink(&secret, f.0.join(&link)).unwrap();
        let fifo = f.event(); let name = CString::new(f.0.join(&fifo).as_os_str().as_bytes()).unwrap(); assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let hard = f.event(); std::fs::hard_link(&secret, f.0.join(&hard)).unwrap();
        let large = f.event(); std::fs::write(f.0.join(&large), vec![b'x'; MAX_EVENT as usize + 1]).unwrap();
        let malformed = f.event(); std::fs::write(f.0.join(&malformed), b"{bad").unwrap();
        let mut called = false; assert_eq!(inbox.drain(|_| { called = true; Ok(()) }).unwrap(), 5); assert!(!called);
        assert_eq!(std::fs::read_to_string(secret).unwrap(), "private");
        for name in [link,fifo,hard,large,malformed] { assert_eq!(std::fs::read(f.0.join(name.replace(".event", ".ack"))).unwrap(), b"rejected"); }
        assert!(inbox.open_file("../secret", false).is_err());
    }
    #[test]
    fn inbox_keeps_pinned_directory_if_path_is_replaced() {
        let f = Fixture::new(); let inbox = Inbox::open(&f.0).unwrap(); let original = f.0.with_extension("original"); std::fs::rename(&f.0, &original).unwrap(); std::fs::create_dir(&f.0).unwrap();
        let event = f.event(); std::fs::write(f.0.join(&event), b"untrusted replacement").unwrap();
        std::fs::write(original.join(&event), b"{\"cmd\":\"hook\"}").unwrap();
        let mut seen = false; inbox.drain(|request| { assert_eq!(request["cmd"], "hook"); seen = true; Ok(()) }).unwrap(); assert!(seen);
        assert_eq!(std::fs::read(f.0.join(event)).unwrap(), b"untrusted replacement"); std::fs::remove_dir_all(original).unwrap();
    }
    #[test]
    fn inbox_ack_does_not_follow_a_preexisting_symlink() {
        let f = Fixture::new(); let inbox = Inbox::open(&f.0).unwrap(); let private = Fixture::new();
        let secret = private.0.join("secret"); std::fs::write(&secret, b"private").unwrap();
        let name = f.event(); let ack = name.replace(".event", ".ack");
        std::fs::write(f.0.join(&name), b"{}").unwrap(); symlink(&secret, f.0.join(&ack)).unwrap();
        inbox.drain(|_| Ok(())).unwrap();
        assert_eq!(std::fs::read(&secret).unwrap(), b"private"); assert_eq!(std::fs::read(f.0.join(ack)).unwrap(), b"ok");
        let alias = private.0.join("alias"); symlink(&f.0, &alias).unwrap(); assert!(Inbox::open(&alias).is_err());
    }
    #[test]
    fn inbox_drain_work_is_bounded() {
        let f = Fixture::new(); let inbox = Inbox::open(&f.0).unwrap(); for _ in 0..MAX_BATCH + 5 { std::fs::write(f.0.join(f.event()), b"{}").unwrap(); }
        let mut count = 0; assert_eq!(inbox.drain(|_| { count += 1; Ok(()) }).unwrap(), MAX_BATCH); assert_eq!(count, MAX_BATCH);
    }
}
