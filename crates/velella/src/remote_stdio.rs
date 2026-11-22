//! Framed, target-local transport for an independently prestarted daemon.
//!
//! This module never starts a daemon, creates a data root, or selects a caller's
//! IPC endpoint. Persistence is deliberately unverified on both supported OSes.
//! The SSH owner must impose partial-frame/channel deadlines and close stdin on
//! cancellation. EOF detaches outstanding workers; process exit does not stop
//! the daemon and cannot cancel a business request already delivered to it.
use crate::ipc;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    ffi::OsString,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

const PROTOCOL: u64 = 1;
const MAX_ID: u64 = 9_007_199_254_740_991;
const MAX_REQUEST: usize = 2 * 1024 * 1024;
const MAX_RESPONSE: usize = 8 * 1024 * 1024;
const MAX_ERROR: usize = 512;
const MAX_WORKERS: usize = 8;
const IDENTITY_FILE: &str = "server-identity.json";
const MAX_IDENTITY_FILE: u64 = 4096;
const PERSISTENCE: &str = "prestarted_unverified";
const INCOMPATIBLE: &str =
    "incompatible daemon: independently prestart a runtime supporting runtime.identity protocol 1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredIdentity {
    schema: u64,
    server_id: String,
}

fn valid_server_id(value: &str) -> bool {
    value.len() == 36
        && uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}

fn read_identity_file(root: &Path) -> Result<StoredIdentity> {
    let path = root.join(IDENTITY_FILE);
    anyhow::ensure!(
        std::fs::symlink_metadata(&path)
            .context("server identity is missing or unreadable")?
            .file_type()
            .is_file(),
        "server identity must be a regular file"
    );
    let file = File::open(path).context("server identity is unreadable")?;
    anyhow::ensure!(
        file.metadata()?.len() <= MAX_IDENTITY_FILE,
        "server identity is too large"
    );
    let mut bytes = Vec::new();
    file.take(MAX_IDENTITY_FILE + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_IDENTITY_FILE,
        "server identity is too large"
    );
    let stored: StoredIdentity =
        serde_json::from_slice(&bytes).context("server identity is corrupt")?;
    anyhow::ensure!(stored.schema == 1, "unsupported server identity schema");
    anyhow::ensure!(
        valid_server_id(&stored.server_id),
        "server identity UUID is corrupt"
    );
    Ok(stored)
}

/// Call only at daemon startup while holding its existing exclusive root lock.
/// Missing identities are initialized atomically; invalid existing files are
/// never replaced, including files from a future schema.
pub(crate) fn initialize_identity(root: &Path) -> Result<()> {
    let root = canonical_root(root)?;
    match std::fs::symlink_metadata(root.join(IDENTITY_FILE)) {
        Ok(_) => {
            read_identity_file(&root)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ipc::atomic_json(
                &root.join(IDENTITY_FILE),
                &StoredIdentity {
                    schema: 1,
                    server_id: uuid::Uuid::now_v7().to_string(),
                },
            )?;
        }
        Err(error) => return Err(error).context("cannot inspect server identity"),
    }
    Ok(())
}

fn target_os() -> Result<&'static str> {
    match std::env::consts::OS {
        "linux" => Ok("linux"),
        "windows" => Ok("windows"),
        _ => anyhow::bail!("remote-stdio supports only Linux and Windows targets"),
    }
}

fn validate_root_path(root: &Path) -> Result<&str> {
    let text = root.to_str().context("data_root must be valid UTF-8")?;
    anyhow::ensure!(
        !text.trim().is_empty() && !text.chars().any(char::is_control),
        "data_root must be nonempty and contain no control characters"
    );
    anyhow::ensure!(
        root.is_absolute(),
        "data_root must be an absolute target-OS path"
    );
    Ok(text)
}

fn canonical_root(root: &Path) -> Result<PathBuf> {
    // Canonicalization runs only on the machine hosting this process. In
    // particular, the Electron client must not canonicalize a remote path.
    let root = dunce::canonicalize(root).context(
        "data_root is unavailable; independently prestart the daemon for this directory",
    )?;
    validate_root_path(&root)?;
    anyhow::ensure!(root.is_dir(), "data_root must be an existing directory");
    Ok(root)
}

fn resolve_data_root_with(
    explicit: Option<&str>,
    environment: impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf> {
    let root = if let Some(explicit) = explicit {
        PathBuf::from(explicit)
    } else if let Some(configured) = environment("VELELLA_DATA_DIR") {
        PathBuf::from(configured)
    } else {
        let key = if target_os()? == "windows" {
            "LOCALAPPDATA"
        } else {
            "HOME"
        };
        let base = PathBuf::from(
            environment(key)
                .context("data_root is required: target data directory environment is unset")?,
        );
        validate_root_path(&base)?;
        base.join("Velella")
    };
    validate_root_path(&root)?;
    canonical_root(&root)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RuntimeIdentity {
    protocol: u64,
    server_id: String,
    data_root: String,
    os: String,
    runtime_version: String,
    persistence: String,
}

impl RuntimeIdentity {
    fn result(&self) -> Value {
        json!({
            "protocol": self.protocol,
            "server_id": self.server_id,
            "data_root": self.data_root,
            "os": self.os,
            "runtime_version": self.runtime_version,
            "persistence": self.persistence,
            "capabilities": ["identity-v1", "multiplex-v1"],
        })
    }
}

/// Read-only daemon command. This never initializes or repairs the identity.
pub(crate) fn identity(root: &Path) -> Result<Value> {
    let root = canonical_root(root)?;
    let stored = read_identity_file(&root)?;
    Ok(json!({
        "ok": true,
        "protocol": PROTOCOL,
        "server_id": stored.server_id,
        "data_root": validate_root_path(&root)?,
        "os": target_os()?,
        "runtime_version": env!("CARGO_PKG_VERSION"),
        "persistence": PERSISTENCE,
    }))
}

// This is the only production IPC adapter. A path and a business object cannot
// redirect it to a browser, host, arbitrary socket, or named pipe endpoint.
fn local_request(root: &Path, request: &Value) -> Result<Value> {
    let endpoint = ipc::endpoint(root, "daemon");
    if request["cmd"] == "runtime.identity" {
        ipc::request_with_timeout(&endpoint, request, Duration::from_secs(5))
    } else {
        // Existing IPC bounds each business transfer to 45 seconds / 8 MiB.
        // Do not retry: a timeout does not prove the operation was not executed.
        ipc::request(&endpoint, request)
    }
}

fn query_identity(
    root: &Path,
    request: &impl Fn(&Path, &Value) -> Result<Value>,
) -> Result<RuntimeIdentity> {
    let mut response = request(root, &json!({"cmd": "runtime.identity"}))
        .map_err(|_| anyhow::anyhow!("daemon identity unavailable; independently prestart a compatible daemon (no automatic startup)"))?;
    anyhow::ensure!(response["ok"] == true, INCOMPATIBLE);
    response.as_object_mut().context(INCOMPATIBLE)?.remove("ok");
    let identity: RuntimeIdentity =
        serde_json::from_value(response).map_err(|_| anyhow::anyhow!(INCOMPATIBLE))?;
    anyhow::ensure!(
        identity.protocol == PROTOCOL
            && valid_server_id(&identity.server_id)
            && identity.data_root == validate_root_path(root)?
            && identity.os == target_os()?
            && identity.persistence == PERSISTENCE
            && !identity.runtime_version.trim().is_empty()
            && identity.runtime_version.len() <= 128
            && !identity.runtime_version.chars().any(char::is_control),
        INCOMPATIBLE
    );
    Ok(identity)
}

fn verify_identity(
    expected: &RuntimeIdentity,
    request: &impl Fn(&Path, &Value) -> Result<Value>,
) -> Result<()> {
    let current = query_identity(Path::new(&expected.data_root), request)?;
    anyhow::ensure!(
        current == *expected,
        "daemon identity changed; open a new connection and confirm again"
    );
    Ok(())
}

// An omitted optional string is permitted; explicit null is not a string.
fn optional_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(tag = "method", deny_unknown_fields)]
enum Envelope {
    #[serde(rename = "hello")]
    Hello {
        id: u64,
        protocol: u64,
        #[serde(default, deserialize_with = "optional_string")]
        data_root: Option<String>,
    },
    #[serde(rename = "connect")]
    Connect {
        id: u64,
        protocol: u64,
        server_id: String,
        data_root: String,
        confirm: bool,
    },
    #[serde(rename = "rpc")]
    Rpc {
        id: u64,
        protocol: u64,
        server_id: String,
        data_root: String,
        request: Map<String, Value>,
    },
}

impl Envelope {
    fn id(&self) -> u64 {
        match self {
            Self::Hello { id, .. } | Self::Connect { id, .. } | Self::Rpc { id, .. } => *id,
        }
    }

    fn protocol(&self) -> u64 {
        match self {
            Self::Hello { protocol, .. }
            | Self::Connect { protocol, .. }
            | Self::Rpc { protocol, .. } => *protocol,
        }
    }
}

/// A clean EOF is allowed only before the next header. Every other short read
/// is fatal. No allocation is made from an unchecked wire length.
fn read_frame(reader: &mut impl Read) -> Result<Option<Vec<u8>>> {
    let mut header = [0u8; 4];
    loop {
        match reader.read(&mut header[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error).context("cannot read frame header"),
        }
    }
    reader
        .read_exact(&mut header[1..])
        .context("truncated frame header")?;
    let length = u32::from_be_bytes(header) as usize;
    anyhow::ensure!(
        length > 0 && length <= MAX_REQUEST,
        "request frame length must be 1..=2 MiB"
    );
    let mut bytes = vec![0; length];
    reader
        .read_exact(&mut bytes)
        .context("truncated frame body")?;
    Ok(Some(bytes))
}

fn decode_envelope(bytes: &[u8]) -> Result<Envelope> {
    // Deserialize directly, not through Value: duplicate envelope fields must
    // not be silently collapsed before strict schema validation.
    serde_json::from_slice(bytes).context("invalid JSON or remote envelope")
}

#[derive(Default)]
struct Connection {
    last_id: u64,
    hello_seen: bool,
    identity: Option<RuntimeIdentity>,
    connected: bool,
}

enum Action {
    Reply(Value),
    Forward(RpcJob),
}

struct RpcJob {
    id: u64,
    identity: RuntimeIdentity,
    request: Value,
}

impl RpcJob {
    fn execute(self, request: &impl Fn(&Path, &Value) -> Result<Value>) -> Result<Value> {
        verify_identity(&self.identity, request)?;
        request(Path::new(&self.identity.data_root), &self.request).map_err(|_| {
            anyhow::anyhow!("daemon RPC failed; execution outcome may be unknown (not retried)")
        })
    }
}

impl Connection {
    fn matches_identity(&self, server_id: &str, data_root: &str) -> Result<&RuntimeIdentity> {
        let identity = self
            .identity
            .as_ref()
            .context("hello must succeed before connect or rpc")?;
        validate_root_path(Path::new(data_root))?;
        anyhow::ensure!(
            identity.server_id == server_id,
            "server_id does not match hello"
        );
        // Do not canonicalize aliases or normalize spellings supplied by the
        // peer: confirmation and every RPC must echo the exact hello root.
        anyhow::ensure!(
            identity.data_root == data_root,
            "data_root does not exactly match hello"
        );
        Ok(identity)
    }

    fn prepare(
        &mut self,
        envelope: Envelope,
        request: &impl Fn(&Path, &Value) -> Result<Value>,
    ) -> Result<Action> {
        let id = envelope.id();
        anyhow::ensure!(id > 0 && id <= MAX_ID, "id must be a positive safe integer");
        anyhow::ensure!(id > self.last_id, "ids must be strictly increasing");
        self.last_id = id;
        anyhow::ensure!(
            envelope.protocol() == PROTOCOL,
            "unsupported remote protocol; expected 1"
        );
        match envelope {
            Envelope::Hello { data_root, .. } => {
                anyhow::ensure!(
                    !self.hello_seen && !self.connected,
                    "hello is allowed only once before connect"
                );
                self.hello_seen = true;
                let root =
                    resolve_data_root_with(data_root.as_deref(), |key| std::env::var_os(key))?;
                let identity = query_identity(&root, request)?;
                let result = identity.result();
                self.identity = Some(identity);
                Ok(Action::Reply(result))
            }
            Envelope::Connect {
                server_id,
                data_root,
                confirm,
                ..
            } => {
                anyhow::ensure!(!self.connected, "connection is already confirmed");
                anyhow::ensure!(confirm, "connect requires confirm:true");
                let identity = self.matches_identity(&server_id, &data_root)?;
                verify_identity(identity, request)?;
                let result = identity.result();
                self.connected = true;
                Ok(Action::Reply(result))
            }
            Envelope::Rpc {
                server_id,
                data_root,
                request,
                ..
            } => {
                anyhow::ensure!(self.connected, "connect must succeed before rpc");
                let identity = self.matches_identity(&server_id, &data_root)?.clone();
                if let Some(command) = request.get("cmd").and_then(Value::as_str) {
                    anyhow::ensure!(
                        !command.starts_with("runtime.") && !command.starts_with("remote."),
                        "runtime.* and remote.* commands are not allowed inside rpc"
                    );
                }
                Ok(Action::Forward(RpcJob {
                    id,
                    identity,
                    request: Value::Object(request),
                }))
            }
        }
    }
}

fn sanitize_error(message: &str) -> String {
    let mut sanitized = String::new();
    for character in message.chars().filter(|c| {
        !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    }) {
        if sanitized.len() + character.len_utf8() > MAX_ERROR {
            break;
        }
        sanitized.push(character);
    }
    if sanitized.is_empty() {
        sanitized.push_str("remote request failed");
    }
    sanitized
}

fn error_response(id: u64, message: &str) -> Value {
    json!({"id": id, "ok": false, "error": sanitize_error(message)})
}

struct BoundedBuffer(Vec<u8>);

impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_RESPONSE - self.0.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "response exceeds 8 MiB limit",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn bounded_json(value: &Value) -> Result<Vec<u8>> {
    let mut buffer = BoundedBuffer(Vec::new());
    serde_json::to_writer(&mut buffer, value)?;
    Ok(buffer.0)
}

fn encode_response(id: u64, result: Result<Value>) -> Result<Vec<u8>> {
    let response = match result {
        Ok(result) => json!({"id": id, "ok": true, "result": result}),
        Err(error) => error_response(id, &error.to_string()),
    };
    bounded_json(&response)
        .or_else(|_| bounded_json(&error_response(id, "response exceeds 8 MiB limit")))
}

struct FrameWriter<W> {
    output: Mutex<W>,
    failed: AtomicBool,
}

impl<W: Write> FrameWriter<W> {
    fn new(output: W) -> Self {
        Self {
            output: Mutex::new(output),
            failed: AtomicBool::new(false),
        }
    }

    fn reply(&self, id: u64, result: Result<Value>) -> Result<()> {
        let bytes = encode_response(id, result)?;
        let mut output = self
            .output
            .lock()
            .map_err(|_| anyhow::anyhow!("stdout frame writer is unavailable"))?;
        anyhow::ensure!(
            !self.failed.load(Ordering::Acquire),
            "stdout frame writer is closed"
        );
        let written = (|| -> std::io::Result<()> {
            output.write_all(&(bytes.len() as u32).to_be_bytes())?;
            output.write_all(&bytes)?;
            output.flush()
        })();
        if written.is_err() {
            self.failed.store(true, Ordering::Release);
        }
        written.context("cannot write response frame")
    }
}

struct WorkerPermit(Arc<AtomicUsize>);

impl WorkerPermit {
    fn acquire(active: &Arc<AtomicUsize>) -> Result<Self> {
        active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                if count < MAX_WORKERS {
                    Some(count + 1)
                } else {
                    None
                }
            })
            .map_err(|_| {
                anyhow::anyhow!("too many in-flight RPCs; connection closed (maximum 8)")
            })?;
        Ok(Self(active.clone()))
    }
}

impl Drop for WorkerPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn run_frames<R, W, F>(reader: &mut R, writer: Arc<FrameWriter<W>>, request: Arc<F>) -> Result<()>
where
    R: Read,
    W: Write + Send + 'static,
    F: Fn(&Path, &Value) -> Result<Value> + Send + Sync + 'static,
{
    let mut connection = Connection::default();
    let active = Arc::new(AtomicUsize::new(0));
    loop {
        let Some(bytes) = read_frame(reader)? else {
            // No joins and no writer lock on EOF, even with RPCs in flight.
            return Ok(());
        };
        anyhow::ensure!(
            !writer.failed.load(Ordering::Acquire),
            "stdout frame writer is closed"
        );
        let envelope = decode_envelope(&bytes)?;
        let id = envelope.id();
        // Invalid/ambiguous IDs, framing and schema errors close without an
        // uncorrelatable frame. Other protocol rejections get one error frame.
        anyhow::ensure!(
            id > connection.last_id && id <= MAX_ID,
            "invalid or non-increasing request id"
        );
        let action = match connection.prepare(envelope, &*request) {
            Ok(action) => action,
            Err(error) => {
                writer.reply(id, Err(error))?;
                anyhow::bail!("remote request rejected; connection closed");
            }
        };
        match action {
            Action::Reply(result) => writer.reply(id, Ok(result))?,
            Action::Forward(job) => {
                // There is no pending-job queue. Overload closes immediately,
                // without waiting behind workers that may be blocked on stdout.
                let permit = WorkerPermit::acquire(&active)?;
                let writer = writer.clone();
                let request = request.clone();
                std::thread::Builder::new()
                    .name("remote-rpc".into())
                    .spawn(move || {
                        let _permit = permit;
                        let id = job.id;
                        if writer.reply(id, job.execute(&*request)).is_err() {
                            eprintln!("remote-stdio: response writer closed");
                        }
                    })
                    .context("cannot start bounded RPC worker")?;
                // Drop the JoinHandle immediately: EOF must not wait for IPC.
            }
        }
    }
}

fn stdout_file() -> Result<File> {
    // Bypass Rust's global buffered stdout: process teardown must not flush a
    // worker's partial frame or wait for its stdout lock after stdin reaches EOF.
    // Cloning this handle neither creates a file nor changes its permissions.
    #[cfg(windows)]
    {
        use std::os::windows::io::AsHandle;
        Ok(File::from(
            std::io::stdout().as_handle().try_clone_to_owned()?,
        ))
    }
    #[cfg(unix)]
    {
        use std::os::fd::AsFd;
        Ok(File::from(std::io::stdout().as_fd().try_clone_to_owned()?))
    }
    #[cfg(not(any(windows, unix)))]
    anyhow::bail!("remote-stdio supports only Linux and Windows targets")
}

pub(crate) fn run() -> Result<()> {
    target_os()?;
    let stdin = std::io::stdin();
    run_frames(
        &mut stdin.lock(),
        Arc::new(FrameWriter::new(stdout_file()?)),
        Arc::new(local_request),
    )
    .map_err(|error| anyhow::anyhow!(sanitize_error(&error.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct TestRoot(PathBuf);
    impl TestRoot {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("velella-remote-test-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir(&root).unwrap();
            Self(dunce::canonicalize(root).unwrap())
        }
        fn text(&self) -> &str {
            self.0.to_str().unwrap()
        }
        fn reply(&self) -> Value {
            json!({"ok":true,"protocol":1,"server_id":"018f07b9-8438-7abc-8abc-123456789abc",
                "data_root":self.text(),"os":target_os().unwrap(),
                "runtime_version":"test-runtime","persistence":PERSISTENCE})
        }
    }
    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn envelope(value: Value) -> Envelope {
        decode_envelope(&serde_json::to_vec(&value).unwrap()).unwrap()
    }
    fn hello(id: u64, root: &TestRoot) -> Envelope {
        envelope(json!({"id":id,"method":"hello","protocol":1,"data_root":root.text()}))
    }
    fn connect(id: u64, identity: &Value) -> Envelope {
        envelope(json!({"id":id,"method":"connect","protocol":1,
            "server_id":identity["server_id"],"data_root":identity["data_root"],"confirm":true}))
    }
    fn rpc(id: u64, identity: &Value, request: Value) -> Envelope {
        envelope(json!({"id":id,"method":"rpc","protocol":1,
            "server_id":identity["server_id"],"data_root":identity["data_root"],"request":request}))
    }
    fn frame(bytes: &[u8]) -> Vec<u8> {
        let mut framed = (bytes.len() as u32).to_be_bytes().to_vec();
        framed.extend_from_slice(bytes);
        framed
    }
    fn wire(value: Value) -> Vec<u8> {
        frame(&serde_json::to_vec(&value).unwrap())
    }

    struct SplitReader {
        bytes: Cursor<Vec<u8>>,
        chunk: usize,
    }
    impl Read for SplitReader {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let length = out.len().min(self.chunk);
            self.bytes.read(&mut out[..length])
        }
    }

    #[test]
    fn codec_split_headers_bodies_and_consecutive_frames() {
        let bytes = br#"{"id":1,"method":"hello","protocol":1}"#;
        for chunk in 1..=bytes.len() + 4 {
            let mut reader = SplitReader {
                bytes: Cursor::new([frame(bytes), frame(bytes)].concat()),
                chunk,
            };
            assert_eq!(read_frame(&mut reader).unwrap().unwrap(), bytes);
            assert_eq!(read_frame(&mut reader).unwrap().unwrap(), bytes);
            assert!(read_frame(&mut reader).unwrap().is_none());
        }
    }

    #[test]
    fn codec_rejects_truncation_zero_and_oversize_before_body() {
        let framed = frame(br#"{"id":1,"method":"hello","protocol":1}"#);
        for length in 1..framed.len() {
            assert!(
                read_frame(&mut &framed[..length]).is_err(),
                "length {length}"
            );
        }
        for length in [0, MAX_REQUEST as u32 + 1, u32::MAX] {
            assert!(read_frame(&mut &length.to_be_bytes()[..]).is_err());
        }
        let maximum = vec![b' '; MAX_REQUEST];
        assert_eq!(
            read_frame(&mut Cursor::new(frame(&maximum)))
                .unwrap()
                .unwrap()
                .len(),
            MAX_REQUEST
        );
    }

    #[test]
    fn envelope_rejects_invalid_json_utf8_unknown_and_duplicate_fields() {
        for invalid in [
            br#"{"# as &[u8], b"\xff", b"null", b"[]",
            br#"{"id":1,"method":"hello","protocol":1}{}"#,
            br#"{"id":1,"method":"hello","protocol":1,"endpoint":"arbitrary"}"#,
            br#"{"id":1,"id":2,"method":"hello","protocol":1}"#,
            br#"{"id":1,"method":"hello","method":"hello","protocol":1}"#,
            br#"{"id":1,"method":"hello","protocol":1,"protocol":1}"#,
            br#"{"id":1,"method":"hello","protocol":1,"data_root":null}"#,
            br#"{"id":1,"method":"hello","protocol":1,"confirm":true}"#,
            br#"{"id":1,"method":"unknown","protocol":1}"#,
            br#"{"id":1.5,"method":"hello","protocol":1}"#,
            br#"{"id":-1,"method":"hello","protocol":1}"#,
            br#"{"id":"1","method":"hello","protocol":1}"#,
            br#"{"id":1,"method":"hello","protocol":"1"}"#,
            br#"{"id":1,"method":"rpc","protocol":1,"server_id":"s","data_root":"/","request":[]}"#,
            br#"{"id":1,"method":"connect","protocol":1,"server_id":"s","data_root":"/","confirm":true,"request":{}}"#,
        ] {
            assert!(decode_envelope(invalid).is_err(), "accepted {}", String::from_utf8_lossy(invalid));
        }
    }

    #[test]
    fn fatal_envelopes_never_forward_and_protocol_errors_are_framed() {
        for value in [
            json!({"id":1,"method":"hello","protocol":2}),
            json!({"id":0,"method":"hello","protocol":1}),
            json!({"id":MAX_ID+1,"method":"hello","protocol":1}),
            json!({"id":1,"method":"hello","protocol":1,"endpoint":"other"}),
        ] {
            let writer = Arc::new(FrameWriter::new(Vec::new()));
            let fake = |_: &Path, _: &Value| -> Result<Value> { panic!("must not forward") };
            assert!(run_frames(
                &mut Cursor::new(wire(value.clone())),
                writer.clone(),
                Arc::new(fake)
            )
            .is_err());
            let bytes = writer.output.lock().unwrap();
            if value["protocol"] == 2 {
                let mut reader = Cursor::new(bytes.as_slice());
                let response: Value =
                    serde_json::from_slice(&read_frame(&mut reader).unwrap().unwrap()).unwrap();
                assert_eq!(response["id"], 1);
                assert_eq!(response["ok"], false);
                assert!(response["error"].as_str().unwrap().contains("protocol"));
                assert!(read_frame(&mut reader).unwrap().is_none());
            } else {
                assert!(bytes.is_empty());
            }
        }
    }

    #[test]
    fn ids_are_safe_strictly_increasing_with_constant_state() {
        let root = TestRoot::new();
        let identity = root.reply();
        let fake = |_: &Path, _: &Value| Ok(identity.clone());
        for bad in [0, MAX_ID + 1, u64::MAX] {
            assert!(Connection::default()
                .prepare(hello(bad, &root), &fake)
                .is_err());
        }
        let mut connection = Connection::default();
        connection.prepare(hello(10, &root), &fake).unwrap();
        assert!(connection.prepare(connect(10, &identity), &fake).is_err());
        assert!(connection.prepare(connect(9, &identity), &fake).is_err());
        connection
            .prepare(connect(MAX_ID, &identity), &fake)
            .unwrap();
        assert!(connection
            .prepare(rpc(MAX_ID, &identity, json!({})), &fake)
            .is_err());
    }

    #[test]
    fn handshake_order_confirmation_protocol_and_exact_identity() {
        let root = TestRoot::new();
        let identity = root.reply();
        let calls = AtomicUsize::new(0);
        let fake = |_: &Path, request: &Value| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request, &json!({"cmd":"runtime.identity"}));
            Ok(identity.clone())
        };
        assert!(Connection::default()
            .prepare(connect(1, &identity), &fake)
            .is_err());
        assert!(Connection::default()
            .prepare(rpc(1, &identity, json!({})), &fake)
            .is_err());
        let mut connection = Connection::default();
        let Action::Reply(result) = connection.prepare(hello(1, &root), &fake).unwrap() else {
            panic!()
        };
        assert_eq!(
            result["capabilities"],
            json!(["identity-v1", "multiplex-v1"])
        );
        assert_eq!(result["persistence"], PERSISTENCE);
        assert!(connection.prepare(hello(2, &root), &fake).is_err());
        assert!(connection
            .prepare(rpc(3, &identity, json!({})), &fake)
            .is_err());
        let wrong = json!({"server_id":uuid::Uuid::now_v7().to_string(),"data_root":root.text()});
        assert!(connection.prepare(connect(4, &wrong), &fake).is_err());
        let wrong =
            json!({"server_id":identity["server_id"],"data_root":format!("{}/.",root.text())});
        assert!(connection.prepare(connect(5, &wrong), &fake).is_err());
        let mut unconfirmed = connect(6, &identity);
        if let Envelope::Connect { confirm, .. } = &mut unconfirmed {
            *confirm = false;
        }
        assert!(connection.prepare(unconfirmed, &fake).is_err());
        let mut incompatible = connect(7, &identity);
        if let Envelope::Connect { protocol, .. } = &mut incompatible {
            *protocol = 2;
        }
        assert!(connection.prepare(incompatible, &fake).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let Action::Reply(confirmed) = connection.prepare(connect(8, &identity), &fake).unwrap()
        else {
            panic!()
        };
        assert_eq!(confirmed, result);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(connection.prepare(connect(9, &identity), &fake).is_err());
        assert!(connection.prepare(hello(10, &root), &fake).is_err());
        assert!(connection
            .prepare(rpc(11, &wrong, json!({"cmd":"snapshot"})), &fake)
            .is_err());
    }

    #[test]
    fn roots_use_target_environment_without_fallback_or_creation() {
        let root = TestRoot::new();
        let no_env = |_: &str| None;
        for invalid in ["", " ", "relative", ".", "../elsewhere", "bad\npath", "\0"] {
            assert!(resolve_data_root_with(Some(invalid), no_env).is_err());
        }
        let missing = root.0.join("never-create");
        assert!(resolve_data_root_with(missing.to_str(), no_env).is_err());
        assert!(!missing.exists());
        assert!(resolve_data_root_with(None, no_env).is_err());
        let env = |key: &str| {
            if key == "VELELLA_DATA_DIR" {
                Some(root.0.clone().into_os_string())
            } else {
                None
            }
        };
        assert_eq!(resolve_data_root_with(None, env).unwrap(), root.0);
        assert_eq!(
            resolve_data_root_with(Some(root.text()), |_| Some(OsString::from("relative")))
                .unwrap(),
            root.0
        );
        assert!(resolve_data_root_with(None, |_| Some(OsString::new())).is_err());
        let base_key = if cfg!(windows) {
            "LOCALAPPDATA"
        } else {
            "HOME"
        };
        let env = |key: &str| {
            if key == base_key {
                Some(root.0.clone().into_os_string())
            } else {
                None
            }
        };
        assert!(resolve_data_root_with(None, env).is_err());
        assert!(!root.0.join("Velella").exists());
        std::fs::create_dir(root.0.join("Velella")).unwrap();
        assert_eq!(
            resolve_data_root_with(None, env).unwrap(),
            root.0.join("Velella")
        );
        let env = |key: &str| match key {
            "VELELLA_DATA_DIR" => Some(OsString::from("relative")),
            k if k == base_key => Some(root.0.clone().into_os_string()),
            _ => None,
        };
        assert!(resolve_data_root_with(None, env).is_err());
    }

    #[test]
    fn hello_missing_or_old_daemon_is_read_only_and_cannot_be_repeated() {
        let root = TestRoot::new();
        for response in [
            json!({"ok":false,"error":"unknown command"}),
            json!({"ok":true,"protocol":1}),
        ] {
            let mut connection = Connection::default();
            let fake = |_: &Path, _: &Value| Ok(response.clone());
            assert!(connection.prepare(hello(1, &root), &fake).is_err());
            assert!(connection.prepare(hello(2, &root), &fake).is_err());
        }
        let fake = |_: &Path, _: &Value| anyhow::bail!("not running");
        let error = Connection::default()
            .prepare(hello(1, &root), &fake)
            .err()
            .unwrap();
        assert!(error.to_string().contains("independently prestart"));
        assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 0);
    }

    #[test]
    fn connect_and_each_rpc_recheck_identity_and_forward_original_once() {
        let root = TestRoot::new();
        let identity = root.reply();
        let calls = Mutex::new(Vec::new());
        let live = Mutex::new(identity.clone());
        let fake = |path: &Path, request: &Value| {
            assert_eq!(path, root.0.as_path());
            calls.lock().unwrap().push(request.clone());
            if request["cmd"] == "runtime.identity" {
                Ok(live.lock().unwrap().clone())
            } else {
                Ok(json!({"ok":false,"error":"business rejection"}))
            }
        };
        let mut connection = Connection::default();
        connection.prepare(hello(1, &root), &fake).unwrap();
        live.lock().unwrap()["server_id"] = json!(uuid::Uuid::now_v7().to_string());
        assert!(connection.prepare(connect(2, &identity), &fake).is_err());
        assert!(!connection.connected);
        *live.lock().unwrap() = identity.clone();
        connection.prepare(connect(3, &identity), &fake).unwrap();
        let original = json!({"cmd":"snapshot","opaque":{"nested":[1,"x",true]}});
        let Action::Forward(job) = connection
            .prepare(rpc(4, &identity, original.clone()), &fake)
            .unwrap()
        else {
            panic!()
        };
        let result = job.execute(&fake).unwrap();
        assert_eq!(result, json!({"ok":false,"error":"business rejection"}));
        let response: Value =
            serde_json::from_slice(&encode_response(4, Ok(result)).unwrap()).unwrap();
        assert_eq!(response["ok"], true);
        assert_eq!(response["result"]["ok"], false);
        assert_eq!(calls.lock().unwrap().last(), Some(&original));
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|v| v["cmd"] == "snapshot")
                .count(),
            1
        );
        for (offset, (field, value)) in [
            ("server_id", json!(uuid::Uuid::now_v7().to_string())),
            ("protocol", json!(2)),
            ("runtime_version", json!("replacement")),
            ("data_root", json!("relative")),
            ("persistence", json!("verified")),
            ("os", json!("other")),
        ]
        .into_iter()
        .enumerate()
        {
            *live.lock().unwrap() = identity.clone();
            live.lock().unwrap()[field] = value;
            let Action::Forward(job) = connection
                .prepare(rpc(5 + offset as u64, &identity, original.clone()), &fake)
                .unwrap()
            else {
                panic!()
            };
            assert!(job.execute(&fake).is_err());
        }
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|v| v["cmd"] == "snapshot")
                .count(),
            1
        );
        for (id, command) in [
            (20, "runtime.identity"),
            (21, "runtime.stop"),
            (22, "remote.connect"),
        ] {
            assert!(connection
                .prepare(rpc(id, &identity, json!({"cmd":command})), &fake)
                .is_err());
        }
    }

    #[test]
    fn business_failure_is_never_retried() {
        let root = TestRoot::new();
        let identity = root.reply();
        let calls = AtomicUsize::new(0);
        let fake = |_: &Path, request: &Value| {
            if request["cmd"] == "runtime.identity" {
                return Ok(identity.clone());
            }
            calls.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("timeout after delivery")
        };
        let mut connection = Connection::default();
        connection.prepare(hello(1, &root), &fake).unwrap();
        connection.prepare(connect(2, &identity), &fake).unwrap();
        let Action::Forward(job) = connection
            .prepare(rpc(3, &identity, json!({"cmd":"mutate"})), &fake)
            .unwrap()
        else {
            panic!()
        };
        assert!(job
            .execute(&fake)
            .unwrap_err()
            .to_string()
            .contains("not retried"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn bounded_responses_include_envelope_and_sanitize_errors() {
        let encoded = encode_response(1, Ok(json!({"data":"x".repeat(MAX_RESPONSE)}))).unwrap();
        assert!(encoded.len() < 1024);
        let value: Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(value["id"], 1);
        assert_eq!(value["ok"], false);
        assert!(value["error"].as_str().unwrap().contains("8 MiB"));
        let empty = encode_response(2, Ok(json!(""))).unwrap().len();
        assert_eq!(
            encode_response(2, Ok(json!("x".repeat(MAX_RESPONSE - empty))))
                .unwrap()
                .len(),
            MAX_RESPONSE
        );
        let encoded = encode_response(
            3,
            Err(anyhow::anyhow!("\n\x1b\0\u{202e}{}", "中".repeat(2048))),
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&encoded).unwrap();
        let error = value["error"].as_str().unwrap();
        assert!(error.len() <= MAX_ERROR);
        assert!(!error.chars().any(char::is_control));
        assert!(!error.contains('\u{202e}'));
    }

    #[test]
    fn identity_persists_and_read_only_identity_does_not_initialize() {
        let root = TestRoot::new();
        assert!(identity(&root.0).is_err());
        assert!(!root.0.join(IDENTITY_FILE).exists());
        initialize_identity(&root.0).unwrap();
        let before = std::fs::read(root.0.join(IDENTITY_FILE)).unwrap();
        let first = identity(&root.0).unwrap();
        initialize_identity(&root.0).unwrap();
        assert_eq!(identity(&root.0).unwrap(), first);
        assert_eq!(std::fs::read(root.0.join(IDENTITY_FILE)).unwrap(), before);
        assert!(valid_server_id(first["server_id"].as_str().unwrap()));
        assert_eq!(first["data_root"], root.text());
        assert_eq!(first["protocol"], 1);
        assert_eq!(first["runtime_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(first["persistence"], PERSISTENCE);
    }

    #[test]
    fn corrupt_future_and_oversized_identity_is_never_replaced() {
        let root = TestRoot::new();
        let path = root.0.join(IDENTITY_FILE);
        for corrupt in [
            Vec::new(),
            b"not-json".to_vec(),
            b"\xff".to_vec(),
            serde_json::to_vec(&json!({"schema":2,"server_id":uuid::Uuid::now_v7().to_string()}))
                .unwrap(),
            br#"{"schema":1,"server_id":"not-a-uuid"}"#.to_vec(),
            br#"{"schema":1,"server_id":"00000000-0000-0000-0000-000000000000"}"#.to_vec(),
            serde_json::to_vec(
                &json!({"schema":1,"server_id":uuid::Uuid::now_v7().to_string(),"unknown":true}),
            )
            .unwrap(),
            vec![b' '; MAX_IDENTITY_FILE as usize + 1],
        ] {
            std::fs::write(&path, &corrupt).unwrap();
            assert!(identity(&root.0).is_err());
            assert!(initialize_identity(&root.0).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), corrupt);
        }
    }

    #[test]
    fn worker_limit_has_no_queue_and_releases_slots() {
        let active = Arc::new(AtomicUsize::new(0));
        let mut permits = Vec::new();
        for _ in 0..MAX_WORKERS {
            permits.push(WorkerPermit::acquire(&active).unwrap());
        }
        assert!(WorkerPermit::acquire(&active).is_err());
        permits.pop();
        permits.push(WorkerPermit::acquire(&active).unwrap());
        drop(permits);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn concurrent_writer_keeps_frames_whole() {
        let writer = Arc::new(FrameWriter::new(Vec::new()));
        let threads: Vec<_> = (1..=MAX_WORKERS as u64)
            .map(|id| {
                let writer = writer.clone();
                std::thread::spawn(move || {
                    writer
                        .reply(id, Ok(json!({"body":"x".repeat(4096)})))
                        .unwrap()
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let bytes = writer.output.lock().unwrap();
        let mut reader = Cursor::new(bytes.as_slice());
        let mut ids = Vec::new();
        while let Some(frame) = read_frame(&mut reader).unwrap() {
            let value: Value = serde_json::from_slice(&frame).unwrap();
            assert_eq!(value["result"]["body"].as_str().unwrap().len(), 4096);
            ids.push(value["id"].as_u64().unwrap());
        }
        ids.sort_unstable();
        assert_eq!(ids, (1..=MAX_WORKERS as u64).collect::<Vec<_>>());
    }

    #[test]
    fn eof_and_overload_return_while_rpc_workers_are_blocked() {
        use std::sync::{mpsc, Condvar};
        for count in [1, MAX_WORKERS + 1] {
            let root = TestRoot::new();
            let identity = root.reply();
            let mut input =
                wire(json!({"id":1,"method":"hello","protocol":1,"data_root":root.text()}));
            input.extend(wire(json!({"id":2,"method":"connect","protocol":1,
                "server_id":identity["server_id"],"data_root":root.text(),"confirm":true})));
            for i in 0..count {
                input.extend(wire(json!({"id":i+3,"method":"rpc","protocol":1,
                    "server_id":identity["server_id"],"data_root":root.text(),"request":{"cmd":"snapshot"}})));
            }
            let gate = Arc::new((Mutex::new(false), Condvar::new()));
            let worker_gate = gate.clone();
            let (done_tx, done_rx) = mpsc::channel();
            let fake = move |_: &Path, request: &Value| {
                if request["cmd"] == "runtime.identity" {
                    return Ok(identity.clone());
                }
                let (lock, ready) = &*worker_gate;
                let _guard = ready
                    .wait_while(lock.lock().unwrap(), |released| !*released)
                    .unwrap();
                done_tx.send(()).unwrap();
                Ok(json!({"ok":true}))
            };
            let (returned_tx, returned_rx) = mpsc::channel();
            let runner = std::thread::spawn(move || {
                let result = run_frames(
                    &mut Cursor::new(input),
                    Arc::new(FrameWriter::new(Vec::new())),
                    Arc::new(fake),
                );
                returned_tx.send(result.is_ok()).unwrap();
            });
            let returned = returned_rx.recv_timeout(Duration::from_secs(3));
            // Release on failure as well, so this test never strands workers.
            *gate.0.lock().unwrap() = true;
            gate.1.notify_all();
            runner.join().unwrap();
            assert_eq!(returned.unwrap(), count == 1);
            for _ in 0..count.min(MAX_WORKERS) {
                done_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            }
        }
    }
}
