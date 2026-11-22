//! User-local IPC, with an owner-only Windows pipe DACL / Unix mode 0600.
use anyhow::{Context, Result};
use interprocess::local_socket::{
    prelude::*, GenericFilePath, GenericNamespaced, Listener, ListenerOptions, Stream,
};
use serde_json::Value;
use std::{
    hash::{Hash, Hasher},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub fn data_dir() -> PathBuf {
    std::env::var_os("VELELLA_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join("Velella")
        })
}

pub fn endpoint(root: &Path, name: &str) -> String {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let normalized = std::fs::canonicalize(root).unwrap_or_else(|_| {
        if root.is_absolute() {
            root.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(root))
                .unwrap_or_else(|_| root.to_path_buf())
        }
    });
    normalized
        .to_string_lossy()
        .replace('/', "\\")
        .to_lowercase()
        .hash(&mut hash);
    let tag = format!("velella-{:x}-{name}", hash.finish());
    if cfg!(windows) {
        tag
    } else {
        root.join(format!("{name}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

fn name(endpoint: &str) -> std::io::Result<interprocess::local_socket::Name<'_>> {
    if cfg!(windows) {
        endpoint.to_ns_name::<GenericNamespaced>()
    } else {
        endpoint.to_fs_name::<GenericFilePath>()
    }
}

/// Claim a desktop endpoint, recovering a socket left behind by a crashed GUI.
/// Serialize startup so two new desktops cannot both remove/rebind the socket.
pub fn listen_recovering(endpoint: &str) -> Result<Listener> {
    #[cfg(unix)]
    {
        use fs2::FileExt;
        use std::os::unix::{fs::{FileTypeExt, MetadataExt}, net::UnixStream};
        let path = Path::new(endpoint);
        let lock = std::fs::OpenOptions::new().read(true).write(true).create(true)
            .truncate(false).open(path.with_extension("startup.lock"))?;
        lock.lock_exclusive()?;
        match listen(endpoint) {
            Ok(listener) => return Ok(listener),
            Err(error) => {
                let before = std::fs::symlink_metadata(path)?;
                // Never unlink regular files, symlinks, or another owner's socket.
                if !before.file_type().is_socket() || before.uid() != lock.metadata()?.uid() {
                    return Err(error);
                }
                match UnixStream::connect(path) {
                    Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {}
                    _ => return Err(error),
                }
                let after = std::fs::symlink_metadata(path)?;
                anyhow::ensure!(before.dev() == after.dev() && before.ino() == after.ino(),
                    "desktop socket changed during recovery");
                std::fs::remove_file(path)?;
                return listen(endpoint);
            }
        }
    }
    #[cfg(not(unix))]
    listen(endpoint)
}

pub fn listen(endpoint: &str) -> Result<Listener> {
    let options = ListenerOptions::new().name(name(endpoint)?);
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::{
            local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
        };
        // Owner Rights SID refers to the creator/owner, not all logged-in users.
        let sddl = widestring::U16CString::from_str("D:P(A;;GA;;;OW)")?;
        options.security_descriptor(SecurityDescriptor::deserialize(&sddl)?)
    };
    #[cfg(unix)]
    let options = {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        options.mode(0o600)
    };
    Ok(options.create_sync()?)
}

fn transfer(stream: &mut Stream, outgoing: Option<&Value>) -> Result<Value> {
    transfer_with_timeout(stream,outgoing,Duration::from_secs(45))
}

fn transfer_with_timeout(stream: &mut Stream, outgoing: Option<&Value>, timeout:Duration) -> Result<Value> {
    stream.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    if let Some(value) = outgoing {
        let mut data = serde_json::to_vec(value)?;
        data.push(b'\n');
        let mut offset = 0;
        while offset < data.len() {
            // Keep a single nonblocking write within interprocess's default
            // 512-byte Windows pipe buffer; large atomic writes can make no
            // progress even while the peer is actively reading.
            let end = if cfg!(windows) { (offset + 512).min(data.len()) } else { data.len() };
            match stream.write(&data[offset..end]) {
                // PIPE_NOWAIT may successfully write zero bytes while its
                // buffer is full. Retain the offset until the reader drains it.
                Ok(0) if cfg!(windows) => {
                    anyhow::ensure!(Instant::now() < deadline, "IPC write timed out");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Ok(0) => anyhow::bail!("IPC closed while writing"),
                Ok(n) => offset += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    anyhow::ensure!(Instant::now() < deadline, "IPC write timed out");
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    let mut data = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            // Windows PIPE_NOWAIT can report a zero-byte read while the pipe
            // is connected but empty. Only the bounded deadline is a failure.
            Ok(0) => {
                #[cfg(windows)]
                {
                    use std::os::windows::io::{AsHandle, AsRawHandle};
                    let Stream::NamedPipe(pipe) = &*stream;
                    // interprocess maps both an empty PIPE_NOWAIT read and a
                    // broken pipe to Ok(0). Peek preserves the Windows error,
                    // distinguishing a live idle peer from an exited host.
                    let ok = unsafe {
                        windows_sys::Win32::System::Pipes::PeekNamedPipe(
                            pipe.as_handle().as_raw_handle(), std::ptr::null_mut(), 0,
                            std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(),
                        )
                    };
                    if ok == 0 { return Err(std::io::Error::last_os_error()).context("IPC peer disconnected"); }
                }
                #[cfg(not(windows))]
                anyhow::bail!("IPC peer disconnected");
                anyhow::ensure!(Instant::now() < deadline, "IPC response timed out");
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(n) => {
                data.extend_from_slice(&chunk[..n]);
                anyhow::ensure!(data.len() <= 8 * 1024 * 1024, "IPC message too large");
                if data.ends_with(b"\n") {
                    return Ok(serde_json::from_slice(&data)?);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                anyhow::ensure!(Instant::now() < deadline, "IPC read timed out");
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

pub fn request(endpoint: &str, value: &Value) -> Result<Value> {
    request_with_timeout(endpoint,value,Duration::from_secs(45))
}

pub fn request_with_timeout(endpoint: &str, value: &Value, timeout:Duration) -> Result<Value> {
    let mut stream =
        Stream::connect(name(endpoint)?).context("background service is not connected")?;
    transfer_with_timeout(&mut stream, Some(value),timeout)
}

pub fn handle(mut stream: Stream, handler: impl FnOnce(Value) -> Result<Value>) {
    let response = transfer(&mut stream, None).and_then(handler);
    let value =
        response.unwrap_or_else(|e| serde_json::json!({"ok":false,"error":format!("{e:#}")}));
    let _ = stream.set_nonblocking(false);
    let _ = writeln!(stream, "{value}");
}

pub fn atomic_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    atomic_write(path, &serde_json::to_vec_pretty(value)?)
}

/// Replace a file only after its complete new content has reached disk. The
/// temporary name is unique so independent workspaces (and concurrent saves)
/// never contend for a shared `.tmp` file.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = path.file_name().context("atomic save path has no filename")?.to_string_lossy();
    let temp = path.with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::now_v7()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
    file.write_all(content)?;
    file.sync_all()?;
    drop(file);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        // Rust's filesystem calls support extended paths, but MoveFileExW
        // needs their verbatim spelling when a session/receipt path is long.
        let from_path = std::fs::canonicalize(&temp)?;
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let to_path = std::fs::canonicalize(parent)?.join(path.file_name().context("atomic save filename")?);
        let from: Vec<u16> = from_path.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = to_path.as_os_str().encode_wide().chain(Some(0)).collect();
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                0x1 | 0x8,
            )
        };
        anyhow::ensure!(
            ok != 0,
            "atomic state save: {}",
            std::io::Error::last_os_error()
        );
    }
    #[cfg(not(windows))]
    std::fs::rename(temp, path)?;
    Ok(())
}

pub fn spawn_hidden(command: &mut std::process::Command) -> Result<std::process::Child> {
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00000200); // no window, independent process group
    }
    Ok(command.spawn()?)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn disconnected_peer_does_not_hold_request_until_deadline() {
        let endpoint = format!("velella-disconnect-test-{}", uuid::Uuid::now_v7());
        let listener = listen(&endpoint).unwrap();
        let server = std::thread::spawn(move || {
            let mut stream = listener.accept().unwrap();
            transfer(&mut stream, None).unwrap();
            // Model a host exiting after receiving status, before replying.
        });
        let started = Instant::now();
        let result = request(&endpoint, &serde_json::json!({"cmd":"status"}));
        server.join().unwrap();
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(2), "dead peer stalled RPC for {:?}: {result:?}", started.elapsed());
    }

    #[test]
    fn large_hook_payload_round_trips_over_named_pipe() {
        let endpoint = format!("velella-hook-test-{}", uuid::Uuid::now_v7());
        let listener = listen(&endpoint).unwrap();
        let payload = serde_json::json!({"cmd":"hook","event":{
            "hook_event_name":"UserPromptSubmit", "prompt":"中文 probe ".repeat(8192)
        }});
        let expected = payload.clone();
        let server = std::thread::spawn(move || {
            let stream = listener.accept().unwrap();
            handle(stream, |received| {
                assert_eq!(received, expected);
                Ok(serde_json::json!({"ok":true}))
            });
        });
        let result = request(&endpoint, &payload);
        assert!(result.is_ok(), "large native hook failed: {result:?}");
        assert_eq!(result.unwrap()["ok"], true);
        server.join().unwrap();
    }

    #[test]
    fn endpoint_normalizes_existing_root_spelling() {
        let root = std::env::temp_dir().join(format!("velella-ipc-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let slash_spelling = PathBuf::from(root.to_string_lossy().replace('\\', "/"));
        assert_eq!(
            endpoint(&root, "daemon"),
            endpoint(&slash_spelling, "daemon")
        );
        std::fs::remove_dir(&root).unwrap();
    }

    #[test]
    fn atomic_write_replaces_content_without_a_shared_tmp_name() {
        let root = std::env::temp_dir().join(format!("velella-atomic-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("note.txt");
        atomic_write(&target, b"first").unwrap();
        atomic_write(&target, b"second \xE4\xB8\xAD\xE6\x96\x87").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), "second 中文".as_bytes());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn atomic_write_supports_deep_session_receipt_paths() {
        let root=std::env::temp_dir().join(format!("velella-long-path-{}",uuid::Uuid::now_v7()));
        let target=root.join("workspace-".repeat(10)).join("session-".repeat(10)).join("receipt.json");
        atomic_write(&target,b"first").unwrap();
        atomic_write(&target,b"second").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(),b"second");
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(all(test, unix))]
mod recovery_tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    fn root() -> PathBuf {
        let path = std::env::temp_dir().join(format!("vl-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn desktop_recovers_socket_left_by_dead_process() {
        let root = root();
        let path = root.join("browser.sock");
        drop(UnixListener::bind(&path).unwrap()); // Leaves the same inode as a crashed owner.
        assert!(path.exists());
        let listener = listen_recovering(path.to_str().unwrap()).unwrap();
        assert!(std::os::unix::net::UnixStream::connect(&path).is_ok());
        drop(listener);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn desktop_preserves_live_owner_and_non_socket_files() {
        let root = root();
        let path = root.join("browser.sock");
        let listener = UnixListener::bind(&path).unwrap();
        assert!(listen_recovering(path.to_str().unwrap()).is_err());
        assert!(std::os::unix::net::UnixStream::connect(&path).is_ok());
        drop(listener);
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "keep").unwrap();
        assert!(listen_recovering(path.to_str().unwrap()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep");
        std::fs::remove_file(&path).unwrap();
        let target = root.join("target");
        std::fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(listen_recovering(path.to_str().unwrap()).is_err());
        assert!(std::fs::symlink_metadata(&path).unwrap().file_type().is_symlink());
        std::fs::remove_dir_all(root).unwrap();
    }
}
