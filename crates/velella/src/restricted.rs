//! Explicit Linux Landlock + seccomp execution mode.
//!
//! This backend restricts file access and denies addressable sockets; it permits
//! anonymous UNIX stream/seqpacket pairs for internal runtime IPC. It does not create
//! PID or mount namespaces, hide filesystem metadata, or provide resource limits.
//! All irreversible restrictions are installed in a fresh launcher process.
use crate::sandbox::{Launch, Policy};
use anyhow::{Context, Result};
use std::collections::HashMap;

const PROBE_RECEIPT: &str = "velella-restricted-probe-v3:filesystem-and-network-denied";

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[path = "restricted_fs.rs"]
mod filesystem;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[path = "restricted_tools.rs"]
mod tools;

pub fn prepare(
    policy: &Policy,
    program: &str,
    args: &[String],
    cwd: Option<&str>,
    env: &HashMap<String, String>,
) -> Result<Launch> {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        linux::prepare(policy, program, args, cwd, env)
    }
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        let _ = (policy, program, args, cwd, env);
        anyhow::bail!("restricted execution requires Linux x86_64 or aarch64")
    }
}

pub fn validate(policy: &Policy, cwd: &str) -> Result<()> {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        linux::validated(policy, cwd).map(|_| ())
    }
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        let _ = (policy, cwd);
        anyhow::bail!("restricted execution requires Linux x86_64 or aarch64")
    }
}

/// Probe in a new executable, never the daemon thread that requested capability
/// information. A successful ABI query alone is insufficient.
pub fn probe() -> Result<()> {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let mut child = Command::new(std::env::current_exe()?)
            .arg("restricted-probe")
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("start restricted capability probe")?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if child.try_wait()?.is_some() {
                let output = child.wait_with_output()?;
                anyhow::ensure!(
                    output.status.success(),
                    "restricted capability probe failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
                anyhow::ensure!(
                    String::from_utf8_lossy(&output.stdout).trim() == PROBE_RECEIPT,
                    "restricted capability probe returned no valid verification receipt"
                );
                return Ok(());
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("restricted capability probe timed out");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        anyhow::bail!("restricted execution requires Linux x86_64 or aarch64")
    }
}

/// Internal launcher entry point. The serialized specification is revalidated
/// here because the filesystem may have changed after descriptor creation.
pub fn run_encoded(spec: &str) -> Result<()> {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        linux::run_encoded(spec)
    }
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        let _ = spec;
        anyhow::bail!("restricted execution requires Linux x86_64 or aarch64")
    }
}

pub fn run_probe() -> Result<()> {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        linux::run_probe()
    }
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        anyhow::bail!("restricted execution requires Linux x86_64 or aarch64")
    }
}

/// Fresh-process tool entry point for the fixed-policy exec-server adapter.
/// Filesystem operations and command cwd resolution happen only after confinement.
pub fn run_tool(encoded: &str) -> Result<()> {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    {
        linux::run_tool(encoded)
    }
    #[cfg(not(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        let _ = encoded;
        anyhow::bail!("restricted execution requires Linux x86_64 or aarch64")
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod linux {
    use super::*;
    use crate::sandbox::Network;
    use serde::{Deserialize, Serialize};
    use std::{
        ffi::CString,
        fs, io,
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::{ffi::OsStrExt, fs::{DirBuilderExt, PermissionsExt}, process::CommandExt},
        },
        path::{Path, PathBuf},
        process::Command,
    };

    const SYSTEM_PATH: &str = "/usr/local/bin:/usr/bin:/bin";
    const READ: u64 = (1 << 0) | (1 << 2) | (1 << 3);
    // ABI 3 is the minimum: includes REFER and TRUNCATE, not just open/write.
    const HANDLED: u64 = (1 << 15) - 1;
    const WRITE: u64 = HANDLED & !((1 << 6) | (1 << 11));
    const FILE_READ: u64 = 1 << 2;
    const FILE_WRITE: u64 = (1 << 1) | (1 << 14);
    const SYSTEM_DIRS: &[&str] = &["/usr", "/bin", "/sbin", "/lib", "/lib64"];

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Spec {
        policy: Policy,
        program: String,
        args: Vec<String>,
        cwd: String,
        env: HashMap<String, String>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ToolSpec {
        policy: Policy,
        // Fixed writable work root, never the command's requested subdirectory.
        cwd: String,
        env: HashMap<String, String>,
        operation: ToolOperation,
    }

    #[derive(Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    enum ToolOperation {
        Process { argv: Vec<String>, cwd: String },
        Filesystem { method: String },
    }

    pub(super) struct Validated {
        cwd: PathBuf,
        home: PathBuf,
        rules: Vec<(PathBuf, u64)>,
        tool_bin: Option<PathBuf>,
    }

    fn canonical_directory(path: &Path) -> Result<PathBuf> {
        anyhow::ensure!(path.is_absolute(), "restricted paths must be absolute");
        let canonical = path
            .canonicalize()
            .with_context(|| format!("restricted directory unavailable: {}", path.display()))?;
        anyhow::ensure!(
            canonical == path && canonical.is_dir(),
            "restricted paths must be canonical directories: {}",
            path.display()
        );
        // Allowing ancestors of these locations would expose unrelated host state.
        for reserved in [
            "/usr", "/bin", "/sbin", "/lib", "/lib64", "/etc", "/dev", "/proc", "/sys", "/run",
        ] {
            let reserved = Path::new(reserved);
            anyhow::ensure!(
                !canonical.starts_with(reserved) && !reserved.starts_with(&canonical),
                "restricted directory overlaps a system path: {}",
                path.display()
            );
        }
        for broad in ["/tmp", "/var", "/home", "/root"] {
            anyhow::ensure!(
                canonical != Path::new(broad),
                "restricted directory is too broad: {}",
                path.display()
            );
        }
        Ok(canonical)
    }

    fn base_rules() -> Result<Vec<(PathBuf, u64)>> {
        let mut rules = Vec::new();
        for path in SYSTEM_DIRS {
            if let Ok(path) = Path::new(path).canonicalize() {
                if !rules.iter().any(|(existing, _)| existing == &path) {
                    rules.push((path, READ));
                }
            }
        }
        anyhow::ensure!(!rules.is_empty(), "system runtime directories unavailable");
        for path in ["/etc/ld.so.cache", "/etc/ssl/certs"] {
            if let Ok(path) = Path::new(path).canonicalize() {
                let rights = if path.is_dir() { READ } else { FILE_READ };
                rules.push((path, rights));
            }
        }
        // Rules on individual devices must omit directory-only rights.
        for (path, rights) in [
            ("/dev/null", FILE_READ | FILE_WRITE),
            ("/dev/zero", FILE_READ | FILE_WRITE),
            ("/dev/urandom", FILE_READ),
            ("/dev/random", FILE_READ),
            ("/dev/tty", FILE_READ | FILE_WRITE),
        ] {
            if let Ok(path) = Path::new(path).canonicalize() {
                rules.push((path, rights));
            }
        }
        Ok(rules)
    }

    pub(super) fn validated(policy: &Policy, cwd: &str) -> Result<Validated> {
        let Policy::Restricted {
            read_only,
            writable,
            network,
            home,
        } = policy
        else {
            anyhow::bail!("restricted policy required")
        };
        anyhow::ensure!(
            *network == Network::Denied,
            "restricted execution currently supports network=denied only"
        );
        anyhow::ensure!(
            read_only.len() + writable.len() <= 64,
            "too many restricted directory rules"
        );
        let cwd = canonical_directory(Path::new(cwd))?;
        let home = canonical_directory(
            home.as_deref()
                .context("restricted execution requires a dedicated HOME")?,
        )?;
        let mut explicit = vec![(cwd.clone(), WRITE), (home.clone(), WRITE)];
        for (paths, rights) in [(read_only, READ), (writable, WRITE)] {
            for path in paths {
                explicit.push((canonical_directory(path)?, rights));
            }
        }
        for (index, (path, _)) in explicit.iter().enumerate() {
            for (other, _) in explicit.iter().take(index) {
                anyhow::ensure!(
                    !path.starts_with(other) && !other.starts_with(path),
                    "overlapping restricted directories: {} and {}",
                    path.display(),
                    other.display()
                );
            }
        }
        let mut rules = base_rules()?;
        rules.extend(explicit);
        Ok(Validated { cwd, home, rules, tool_bin: None })
    }

    fn include_tools(mut validated: Validated) -> Result<Validated> {
        let writable: Vec<_> = validated.rules.iter()
            .filter(|(_, rights)| rights & (1 << 1) != 0)
            .map(|(path, _)| path.as_path()).collect();
        if let Some(directory) = super::tools::prepare(&validated.home, &writable)? {
            validated.rules.push((directory.clone(), READ));
            validated.tool_bin = Some(directory);
        }
        Ok(validated)
    }

    fn search_path(validated: &Validated) -> Result<String> {
        Ok(match &validated.tool_bin {
            Some(directory) => format!("{}:{SYSTEM_PATH}",
                directory.to_str().context("trusted tool directory must be UTF-8")?),
            None => SYSTEM_PATH.to_owned(),
        })
    }

    fn tmp_directory(home: &Path) -> Result<PathBuf> {
        let tmp = home.join(".tmp");
        // Set permissions in mkdir itself. A separate path-based chmod before
        // confinement could follow a concurrently substituted .tmp symlink.
        match fs::DirBuilder::new().mode(0o700).create(&tmp) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error).context("create restricted TMPDIR"),
        }
        anyhow::ensure!(
            tmp.canonicalize()? == tmp && tmp.is_dir(),
            "restricted TMPDIR must not be a symlink"
        );
        Ok(tmp)
    }

    fn clean_environment(
        input: &HashMap<String, String>,
        validated: &Validated,
        tmp: &Path,
    ) -> Result<HashMap<String, String>> {
        // No host credentials, loader overrides, shell startup files or host
        // control endpoints. Only explicit, non-credential runtime variables.
        let mut env = HashMap::new();
        for key in [
            "TERM",
            "COLORTERM",
            "LANG",
            "LC_ALL",
            "LC_CTYPE",
            "GIT_OPTIONAL_LOCKS",
            "VELELLA_HOOK_INBOX",
        ] {
            if let Some(value) = input.get(key) {
                anyhow::ensure!(
                    !value.contains('\0'),
                    "invalid restricted environment value"
                );
                env.insert(key.into(), value.clone());
            }
        }
        if let Some(inbox) = env.get("VELELLA_HOOK_INBOX") {
            let inbox = canonical_directory(Path::new(inbox))?;
            anyhow::ensure!(
                validated
                    .rules
                    .iter()
                    .any(|(path, rights)| *rights == WRITE && inbox.starts_with(path)),
                "hook inbox must be inside a writable restricted directory"
            );
        }
        env.insert("PATH".into(), search_path(validated)?);
        // OpenSSL-based tools (including Node) must start without opening host
        // configuration. Keep default providers; do not import caller settings.
        env.insert("OPENSSL_CONF".into(), "/dev/null".into());
        env.insert(
            "HOME".into(),
            validated
                .home
                .to_str()
                .context("HOME must be UTF-8")?
                .into(),
        );
        env.insert(
            "TMPDIR".into(),
            tmp.to_str().context("TMPDIR must be UTF-8")?.into(),
        );
        env.entry("LANG".into()).or_insert_with(|| "C.UTF-8".into());
        env.entry("TERM".into())
            .or_insert_with(|| "xterm-256color".into());
        env.entry("COLORTERM".into())
            .or_insert_with(|| "truecolor".into());
        Ok(env)
    }

    fn executable(program: &str, validated: &Validated) -> Result<String> {
        anyhow::ensure!(
            !program.is_empty() && !program.contains('\0'),
            "invalid restricted executable"
        );
        let path = if Path::new(program).is_absolute() {
            PathBuf::from(program)
        } else {
            anyhow::ensure!(
                !program.contains('/'),
                "restricted executable must be absolute or a bare name"
            );
            search_path(validated)?
                .split(':')
                .map(|dir| Path::new(dir).join(program))
                .find(|path| path.is_file())
                .with_context(|| {
                    format!("restricted executable is not installed on the system PATH: {program}")
                })?
        };
        let path = path
            .canonicalize()
            .context("resolve restricted executable")?;
        let metadata = path.metadata()?;
        anyhow::ensure!(
            metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
            "restricted executable must be an executable file"
        );
        anyhow::ensure!(
            validated
                .rules
                .iter()
                .any(|(root, rights)| rights & 1 != 0 && path.starts_with(root)),
            "restricted executable is outside permitted directories: {}",
            path.display()
        );
        Ok(path
            .to_str()
            .context("restricted executable must be UTF-8")?
            .into())
    }

    pub(super) fn prepare(
        policy: &Policy,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        env: &HashMap<String, String>,
    ) -> Result<Launch> {
        let validated = include_tools(validated(
            policy,
            cwd.context("restricted execution requires an explicit cwd")?,
        )?)?;
        let tmp = tmp_directory(&validated.home)?;
        let spec = Spec {
            policy: policy.clone(),
            program: executable(program, &validated)?,
            args: args.to_vec(),
            cwd: validated.cwd.to_str().context("cwd must be UTF-8")?.into(),
            env: clean_environment(env, &validated, &tmp)?,
        };
        anyhow::ensure!(
            !spec.args.iter().any(|arg| arg.contains('\0')),
            "restricted argument contains NUL"
        );
        // The real capability probe includes the seccomp policy, not only ABI.
        super::probe().context("restricted execution unavailable; refusing native fallback")?;
        Ok(Launch {
            program: std::env::current_exe()?
                .to_str()
                .context("runtime path must be UTF-8")?
                .into(),
            args: vec!["restricted-exec".into(), serde_json::to_string(&spec)?],
            cwd: None,
            env: HashMap::new(),
            clear_env: true,
        })
    }

    fn single_threaded() -> Result<()> {
        let count = fs::read_dir("/proc/self/task")
            .context("inspect launcher threads")?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .len();
        anyhow::ensure!(
            count == 1,
            "restricted launcher must run before starting threads"
        );
        Ok(())
    }

    #[repr(C)]
    struct RulesetAttr {
        handled_access_fs: u64,
    }
    #[repr(C, packed)]
    struct PathAttr {
        allowed_access: u64,
        parent_fd: i32,
    }

    fn landlock(rules: &[(PathBuf, u64)]) -> Result<()> {
        let abi = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<u8>(),
                0,
                1,
            )
        };
        anyhow::ensure!(
            abi >= 3,
            "Landlock ABI >= 3 required, found {abi}: {}",
            io::Error::last_os_error()
        );
        let attr = RulesetAttr {
            handled_access_fs: HANDLED,
        };
        let fd = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                &attr,
                size_of::<RulesetAttr>(),
                0,
            )
        };
        anyhow::ensure!(
            fd >= 0,
            "create Landlock ruleset: {}",
            io::Error::last_os_error()
        );
        let ruleset = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        for (path, rights) in rules {
            let c_path = CString::new(path.as_os_str().as_bytes())?;
            let fd = unsafe {
                libc::open(
                    c_path.as_ptr(),
                    libc::O_PATH | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            anyhow::ensure!(
                fd >= 0,
                "open Landlock rule {}: {}",
                path.display(),
                io::Error::last_os_error()
            );
            let object = unsafe { OwnedFd::from_raw_fd(fd) };
            let attr = PathAttr {
                allowed_access: *rights,
                parent_fd: object.as_raw_fd(),
            };
            let result = unsafe {
                libc::syscall(
                    libc::SYS_landlock_add_rule,
                    ruleset.as_raw_fd(),
                    1,
                    &attr,
                    0,
                )
            };
            anyhow::ensure!(
                result == 0,
                "add Landlock rule {}: {}",
                path.display(),
                io::Error::last_os_error()
            );
        }
        let result = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
        anyhow::ensure!(
            result == 0,
            "set no_new_privs: {}",
            io::Error::last_os_error()
        );
        let result =
            unsafe { libc::syscall(libc::SYS_landlock_restrict_self, ruleset.as_raw_fd(), 0) };
        anyhow::ensure!(
            result == 0,
            "enforce Landlock: {}",
            io::Error::last_os_error()
        );
        Ok(())
    }

    fn drop_capabilities() -> Result<()> {
        #[repr(C)]
        struct Header {
            version: u32,
            pid: i32,
        }
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Data {
            effective: u32,
            permitted: u32,
            inheritable: u32,
        }
        let header = Header {
            version: 0x2008_0522,
            pid: 0,
        };
        let data = [Data {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let result = unsafe { libc::syscall(libc::SYS_capset, &header, data.as_ptr()) };
        anyhow::ensure!(
            result == 0,
            "drop launcher capabilities: {}",
            io::Error::last_os_error()
        );
        Ok(())
    }

    // Classic BPF, with explicit native architecture and x32 rejection. Checking
    // syscall numbers without the architecture check permits alternate-ABI bypasses.
    const LD_ABS: u16 = 0x20;
    const JEQ: u16 = 0x15;
    const JSET: u16 = 0x45;
    const AND: u16 = 0x54;
    const RET: u16 = 0x06;
    const ALLOW: u32 = 0x7fff_0000;
    const DENY: u32 = 0x0005_0000 | libc::EPERM as u32;
    const KILL: u32 = 0x8000_0000;
    fn insn(code: u16, jt: u8, jf: u8, k: u32) -> libc::sock_filter {
        libc::sock_filter { code, jt, jf, k }
    }
    fn deny(filter: &mut Vec<libc::sock_filter>, syscall: libc::c_long) {
        filter.extend([insn(JEQ, 0, 1, syscall as u32), insn(RET, 0, 0, DENY)]);
    }

    fn filter() -> Vec<libc::sock_filter> {
        #[cfg(target_arch = "x86_64")]
        const ARCH: u32 = 0xc000_003e;
        #[cfg(target_arch = "aarch64")]
        const ARCH: u32 = 0xc000_00b7;
        let mut filter = vec![
            insn(LD_ABS, 0, 0, 4),
            insn(JEQ, 1, 0, ARCH),
            insn(RET, 0, 0, KILL),
            insn(LD_ABS, 0, 0, 0),
        ];
        #[cfg(target_arch = "x86_64")]
        filter.extend([insn(JSET, 0, 1, 0x4000_0000), insn(RET, 0, 0, KILL)]);
        // Runtime signalling and process setup use anonymous UNIX pairs. Such
        // pairs have no address and cannot reach host listeners. Permit only STREAM/SEQPACKET,
        // protocol=0 with optional NONBLOCK/CLOEXEC flags. socket/connect/bind,
        // descriptor passing via sendmsg and external socket connections stay denied.
        let pair_branch = [
            insn(LD_ABS, 0, 0, 16),
            insn(JEQ, 1, 0, libc::AF_UNIX as u32),
            insn(RET, 0, 0, DENY),
            insn(LD_ABS, 0, 0, 24),
            insn(
                AND,
                0,
                0,
                !(libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC) as u32,
            ),
            insn(JEQ, 2, 0, libc::SOCK_STREAM as u32),
            insn(JEQ, 1, 0, libc::SOCK_SEQPACKET as u32),
            insn(RET, 0, 0, DENY),
            insn(LD_ABS, 0, 0, 32),
            insn(JEQ, 1, 0, 0),
            insn(RET, 0, 0, DENY),
            insn(RET, 0, 0, ALLOW),
        ];
        filter.push(insn(
            JEQ,
            0,
            pair_branch.len() as u8,
            libc::SYS_socketpair as u32,
        ));
        filter.extend(pair_branch);
        // Pointer arguments to clone3 cannot be safely inspected by classic BPF.
        // ENOSYS lets libc fall back to clone, whose flags we can inspect.
        filter.extend([
            insn(JEQ, 0, 1, libc::SYS_clone3 as u32),
            insn(RET, 0, 0, 0x0005_0000 | libc::ENOSYS as u32),
        ]);
        let namespace_flags = (libc::CLONE_NEWNS
            | libc::CLONE_NEWCGROUP
            | libc::CLONE_NEWUTS
            | libc::CLONE_NEWIPC
            | libc::CLONE_NEWUSER
            | libc::CLONE_NEWPID
            | libc::CLONE_NEWNET
            | libc::CLONE_PARENT
            | libc::CLONE_UNTRACED) as u32;
        filter.extend([
            insn(JEQ, 0, 4, libc::SYS_clone as u32),
            insn(LD_ABS, 0, 0, 16),
            insn(JSET, 0, 1, namespace_flags),
            insn(RET, 0, 0, DENY),
            insn(RET, 0, 0, ALLOW),
        ]);
        // prlimit64(pid=0) concerns only the caller. Other PIDs must not be changed.
        filter.extend([
            insn(JEQ, 0, 4, libc::SYS_prlimit64 as u32),
            insn(LD_ABS, 0, 0, 16),
            insn(JEQ, 0, 1, 0),
            insn(RET, 0, 0, ALLOW),
            insn(RET, 0, 0, DENY),
        ]);
        // F_SETOWN/F_SETSIG can deliver signals without kill(2). Keep only fd,
        // file-status, advisory-lock and pipe-size operations.
        let safe_fcntl = [
            libc::F_DUPFD,
            libc::F_DUPFD_CLOEXEC,
            libc::F_GETFD,
            libc::F_SETFD,
            libc::F_GETFL,
            libc::F_SETFL,
            libc::F_GETLK,
            libc::F_SETLK,
            libc::F_SETLKW,
            libc::F_GETPIPE_SZ,
            libc::F_SETPIPE_SZ,
        ];
        filter.push(insn(
            JEQ,
            0,
            (2 * safe_fcntl.len() + 2) as u8,
            libc::SYS_fcntl as u32,
        ));
        filter.push(insn(LD_ABS, 0, 0, 24));
        for value in safe_fcntl {
            filter.extend([insn(JEQ, 0, 1, value as u32), insn(RET, 0, 0, ALLOW)]);
        }
        filter.push(insn(RET, 0, 0, DENY));
        // Landlock ABI 3/4 does not restrict device/file ioctls. Permit only the
        // terminal queries/updates used by our task's inherited PTY and fd flags.
        let safe_ioctls = [
            libc::TCGETS as u32,
            libc::TCSETS as u32,
            libc::TCSETSW as u32,
            libc::TCSETSF as u32,
            libc::TIOCGWINSZ as u32,
            libc::TIOCSWINSZ as u32,
            libc::TIOCGPGRP as u32,
            libc::TIOCSPGRP as u32,
            libc::TIOCGSID as u32,
            libc::FIONREAD as u32,
            libc::FIONBIO as u32,
            libc::FIOCLEX as u32,
            libc::FIONCLEX as u32,
            libc::TIOCOUTQ as u32,
        ];
        filter.push(insn(
            JEQ,
            0,
            (2 * safe_ioctls.len() + 2) as u8,
            libc::SYS_ioctl as u32,
        ));
        filter.push(insn(LD_ABS, 0, 0, 24));
        for value in safe_ioctls {
            filter.extend([insn(JEQ, 0, 1, value), insn(RET, 0, 0, ALLOW)]);
        }
        filter.push(insn(RET, 0, 0, DENY));
        // Deny addressable network/UNIX communication, io_uring alternatives,
        // external-process mutation, namespaces, and unmediated metadata writes.
        for syscall in [
            libc::SYS_socket,
            libc::SYS_connect,
            libc::SYS_bind,
            libc::SYS_listen,
            libc::SYS_accept,
            libc::SYS_accept4,
            libc::SYS_sendto,
            libc::SYS_sendmsg,
            libc::SYS_sendmmsg,
            libc::SYS_recvfrom,
            libc::SYS_recvmsg,
            libc::SYS_recvmmsg,
            libc::SYS_setsockopt,
            libc::SYS_io_uring_setup,
            libc::SYS_io_uring_enter,
            libc::SYS_io_uring_register,
            libc::SYS_ptrace,
            libc::SYS_process_vm_readv,
            libc::SYS_process_vm_writev,
            libc::SYS_pidfd_open,
            libc::SYS_pidfd_getfd,
            libc::SYS_pidfd_send_signal,
            libc::SYS_process_mrelease,
            libc::SYS_kill,
            libc::SYS_tkill,
            libc::SYS_tgkill,
            libc::SYS_rt_sigqueueinfo,
            libc::SYS_rt_tgsigqueueinfo,
            // The outside runtime owns the PTY process group. Descendants must
            // not detach from it, so cancelling the task can stop all of them.
            libc::SYS_setsid,
            libc::SYS_setpgid,
            libc::SYS_mount,
            libc::SYS_umount2,
            libc::SYS_pivot_root,
            libc::SYS_chroot,
            libc::SYS_setns,
            libc::SYS_unshare,
            libc::SYS_fsopen,
            libc::SYS_fsconfig,
            libc::SYS_fsmount,
            libc::SYS_move_mount,
            libc::SYS_open_tree,
            libc::SYS_mount_setattr,
            libc::SYS_open_by_handle_at,
            libc::SYS_name_to_handle_at,
            libc::SYS_fchmod,
            libc::SYS_fchmodat,
            libc::SYS_fchmodat2,
            libc::SYS_fchown,
            libc::SYS_fchownat,
            libc::SYS_utimensat,
            libc::SYS_setxattr,
            libc::SYS_lsetxattr,
            libc::SYS_fsetxattr,
            libc::SYS_removexattr,
            libc::SYS_lremovexattr,
            libc::SYS_fremovexattr,
            libc::SYS_shmget,
            libc::SYS_shmat,
            libc::SYS_shmctl,
            libc::SYS_semget,
            libc::SYS_semop,
            libc::SYS_semtimedop,
            libc::SYS_semctl,
            libc::SYS_msgget,
            libc::SYS_msgsnd,
            libc::SYS_msgrcv,
            libc::SYS_msgctl,
            libc::SYS_mq_open,
            libc::SYS_mq_unlink,
            libc::SYS_mq_timedsend,
            libc::SYS_mq_timedreceive,
            libc::SYS_mq_notify,
            libc::SYS_mq_getsetattr,
            libc::SYS_keyctl,
            libc::SYS_add_key,
            libc::SYS_request_key,
            libc::SYS_bpf,
            libc::SYS_perf_event_open,
            libc::SYS_userfaultfd,
            libc::SYS_setpriority,
            libc::SYS_ioprio_set,
            libc::SYS_sched_setparam,
            libc::SYS_sched_setscheduler,
            libc::SYS_sched_setattr,
            libc::SYS_sched_setaffinity,
            libc::SYS_acct,
            libc::SYS_swapon,
            libc::SYS_swapoff,
            libc::SYS_reboot,
            libc::SYS_kexec_load,
            libc::SYS_init_module,
            libc::SYS_finit_module,
            libc::SYS_delete_module,
            libc::SYS_quotactl,
        ] {
            deny(&mut filter, syscall);
        }
        #[cfg(target_arch = "x86_64")]
        for syscall in [
            libc::SYS_chmod,
            libc::SYS_chown,
            libc::SYS_lchown,
            libc::SYS_utime,
            libc::SYS_utimes,
            libc::SYS_futimesat,
            libc::SYS_iopl,
            libc::SYS_ioperm,
        ] {
            deny(&mut filter, syscall);
        }
        // New xattr, mount, file-attribute and chroot variants have the same
        // generic numbers on both supported architectures (Linux UAPI tables).
        // Old kernels return EPERM instead of ENOSYS.
        for syscall in [463, 466, 467, 469, 472] {
            deny(&mut filter, syscall);
        }
        filter.push(insn(RET, 0, 0, ALLOW));
        filter
    }

    fn seccomp() -> Result<()> {
        let filter = filter();
        let program = libc::sock_fprog {
            len: filter.len() as u16,
            filter: filter.as_ptr() as *mut _,
        };
        let result =
            unsafe { libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &program) };
        anyhow::ensure!(
            result == 0,
            "install restricted syscall filter: {}",
            io::Error::last_os_error()
        );
        Ok(())
    }

    fn enforce(rules: &[(PathBuf, u64)]) -> Result<()> {
        single_threaded()?;
        for fd in 0..=2 {
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            let result = unsafe { libc::fstat(fd, stat.as_mut_ptr()) };
            anyhow::ensure!(
                result == 0,
                "inspect restricted standard descriptor {fd}: {}",
                io::Error::last_os_error()
            );
            let stat = unsafe { stat.assume_init() };
            let kind = stat.st_mode & libc::S_IFMT;
            let is_null = kind == libc::S_IFCHR && stat.st_rdev == libc::makedev(1, 3);
            let is_terminal = kind == libc::S_IFCHR && unsafe { libc::isatty(fd) } == 1;
            anyhow::ensure!(
                kind == libc::S_IFIFO || is_null || is_terminal,
                "restricted stdin/stdout/stderr must be pipes, terminals or /dev/null; descriptor {fd} is not safe"
            );
        }
        // No stale host handles, socket connections or inherited directory FDs.
        let result = unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0) };
        anyhow::ensure!(
            result == 0,
            "close inherited descriptors: {}",
            io::Error::last_os_error()
        );
        drop_capabilities()?;
        landlock(rules)?;
        seccomp()?;
        Ok(())
    }

    pub(super) fn run_encoded(encoded: &str) -> Result<()> {
        anyhow::ensure!(
            encoded.len() <= 2 * 1024 * 1024,
            "restricted specification is too large"
        );
        let spec: Spec =
            serde_json::from_str(encoded).context("invalid restricted specification")?;
        let validated = include_tools(validated(&spec.policy, &spec.cwd)?)?;
        let program = executable(&spec.program, &validated)?;
        let tmp = tmp_directory(&validated.home)?;
        let env = clean_environment(&spec.env, &validated, &tmp)?;
        anyhow::ensure!(
            !spec.args.iter().any(|arg| arg.contains('\0')),
            "restricted argument contains NUL"
        );
        // No fs/path/environment preparation remains after restrictions except
        // chdir + exec, both checked by Landlock against the same kernel objects.
        let mut command = Command::new(program);
        command
            .args(spec.args)
            .env_clear()
            .envs(env)
            .current_dir(&validated.cwd);
        enforce(&validated.rules)?;
        Err(command.exec()).context("execute restricted command")
    }

    pub(super) fn run_tool(encoded: &str) -> Result<()> {
        anyhow::ensure!(
            encoded.len() <= 2 * 1024 * 1024,
            "restricted tool specification is too large"
        );
        let spec: ToolSpec =
            serde_json::from_str(encoded).context("invalid restricted tool specification")?;
        let validated = include_tools(validated(&spec.policy, &spec.cwd)?)?;
        let tmp = tmp_directory(&validated.home)?;
        let env = clean_environment(&spec.env, &validated, &tmp)?;
        // No operation-supplied path has been resolved or opened. The same
        // enforcement code closes inherited handles and installs both filters.
        enforce(&validated.rules)?;
        // enforce verified that this is a single-threaded fresh process.
        // Filesystem helpers stay in-process, so scrub their environment too.
        for (key, _) in std::env::vars_os() {
            std::env::remove_var(key);
        }
        for (key, value) in &env {
            std::env::set_var(key, value);
        }
        // The adapter blocks these across Popen/ownership registration. No
        // unconfined child operation runs before restoring ordinary signals.
        let mut signals = unsafe { std::mem::zeroed::<libc::sigset_t>() };
        unsafe {
            libc::sigemptyset(&mut signals);
            libc::sigaddset(&mut signals, libc::SIGTERM);
            libc::sigaddset(&mut signals, libc::SIGINT);
        }
        anyhow::ensure!(
            unsafe { libc::sigprocmask(libc::SIG_UNBLOCK, &signals, std::ptr::null_mut()) } == 0,
            "unblock restricted helper signals: {}",
            io::Error::last_os_error()
        );
        match spec.operation {
            ToolOperation::Process { argv, cwd } => {
                anyhow::ensure!(
                    !argv.is_empty()
                        && argv.len() <= 512
                        && !argv[0].is_empty()
                        && !argv.iter().any(|arg| arg.contains('\0'))
                        && serde_json::to_vec(&argv)?.len() <= 64 * 1024,
                    "invalid restricted tool argv"
                );
                anyhow::ensure!(
                    Path::new(&cwd).is_absolute() && !cwd.contains('\0'),
                    "restricted tool cwd must be absolute"
                );
                let cwd = Path::new(&cwd).canonicalize()
                    .context("resolve restricted tool cwd")?;
                anyhow::ensure!(
                    cwd.starts_with(&validated.cwd) && cwd.is_dir(),
                    "tool cwd must stay inside the fixed work directory"
                );
                // Relative argv[0] is resolved from this cwd by exec, after
                // confinement. The operation cannot enlarge the writable root.
                let mut command = Command::new(&argv[0]);
                command.args(&argv[1..]).env_clear().envs(env).current_dir(cwd);
                Err(command.exec()).context("execute restricted tool command")
            }
            ToolOperation::Filesystem { method } => super::filesystem::run(&method),
        }
    }

    pub(super) fn run_probe() -> Result<()> {
        let rules = base_rules()?;
        enforce(&rules)?;
        fs::read_dir("/usr").context("restricted probe cannot read permitted runtime")?;
        let error =
            fs::read("/etc/passwd").expect_err("probe must deny files outside permitted runtime");
        anyhow::ensure!(
            error.raw_os_error() == Some(libc::EACCES),
            "restricted probe did not produce the expected file-access denial"
        );
        for domain in [libc::AF_INET, libc::AF_INET6, libc::AF_UNIX] {
            let fd = unsafe { libc::socket(domain, libc::SOCK_STREAM, 0) };
            if fd >= 0 {
                unsafe {
                    libc::close(fd);
                }
                anyhow::bail!("restricted probe unexpectedly allowed a socket");
            }
            anyhow::ensure!(
                io::Error::last_os_error().raw_os_error() == Some(libc::EPERM),
                "restricted probe did not prove socket denial"
            );
        }
        for kind in [libc::SOCK_STREAM, libc::SOCK_SEQPACKET] {
            let mut pair = [-1; 2];
            let result = unsafe {
                libc::socketpair(
                    libc::AF_UNIX,
                    kind | libc::SOCK_CLOEXEC,
                    0,
                    pair.as_mut_ptr(),
                )
            };
            anyhow::ensure!(
                result == 0,
                "restricted probe cannot create an anonymous UNIX pair: {}",
                io::Error::last_os_error()
            );
            let first = unsafe { OwnedFd::from_raw_fd(pair[0]) };
            let second = unsafe { OwnedFd::from_raw_fd(pair[1]) };
            let payload = [0x41u8];
            let mut received = [0u8];
            anyhow::ensure!(
                unsafe { libc::write(first.as_raw_fd(), payload.as_ptr().cast(), 1) } == 1,
                "restricted probe cannot write its anonymous pair"
            );
            anyhow::ensure!(
                unsafe { libc::read(second.as_raw_fd(), received.as_mut_ptr().cast(), 1) } == 1
                    && received == payload,
                "restricted probe cannot read its anonymous pair"
            );
            let result = unsafe { libc::connect(first.as_raw_fd(), std::ptr::null(), 0) };
            anyhow::ensure!(
                result == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::EPERM),
                "restricted probe did not prove connect denial"
            );
        }
        println!("{PROBE_RECEIPT}");
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // Interpret the small subset of classic BPF used above. This exercises
        // architecture and argument branches without restricting test-runner threads.
        fn decision(arch: u32, syscall: u32, args: [u64; 6]) -> u32 {
            let mut data = [0u32; 16];
            data[0] = syscall;
            data[1] = arch;
            for (index, value) in args.into_iter().enumerate() {
                data[4 + index * 2] = value as u32;
                data[5 + index * 2] = (value >> 32) as u32;
            }
            let filter = filter();
            let mut a = 0;
            let mut pc = 0;
            loop {
                let instruction = filter[pc];
                pc += 1;
                match instruction.code {
                    LD_ABS => a = data[instruction.k as usize / 4],
                    JEQ => {
                        pc += if a == instruction.k {
                            instruction.jt
                        } else {
                            instruction.jf
                        } as usize
                    }
                    JSET => {
                        pc += if a & instruction.k != 0 {
                            instruction.jt
                        } else {
                            instruction.jf
                        } as usize
                    }
                    AND => a &= instruction.k,
                    RET => return instruction.k,
                    _ => panic!("unsupported test BPF instruction"),
                }
            }
        }
        #[cfg(target_arch = "x86_64")]
        const ARCH: u32 = 0xc000_003e;
        #[cfg(target_arch = "aarch64")]
        const ARCH: u32 = 0xc000_00b7;

        #[test]
        fn seccomp_rejects_alternate_abis_and_new_communication() {
            assert_eq!(decision(0x4000_0003, libc::SYS_read as u32, [0; 6]), KILL);
            #[cfg(target_arch = "x86_64")]
            assert_eq!(
                decision(ARCH, 0x4000_0000 | libc::SYS_socket as u32, [0; 6]),
                KILL
            );
            for syscall in [
                libc::SYS_socket,
                libc::SYS_socketpair,
                libc::SYS_connect,
                libc::SYS_io_uring_setup,
                libc::SYS_pidfd_getfd,
                libc::SYS_kill,
                libc::SYS_setsid,
                libc::SYS_setpgid,
                libc::SYS_fchmodat,
                libc::SYS_utimensat,
                libc::SYS_keyctl,
            ] {
                assert_eq!(decision(ARCH, syscall as u32, [0; 6]), DENY, "{syscall}");
            }
            assert_eq!(decision(ARCH, libc::SYS_read as u32, [0; 6]), ALLOW);
            assert_eq!(decision(ARCH, libc::SYS_execve as u32, [0; 6]), ALLOW);
        }

        #[test]
        fn seccomp_allows_only_anonymous_unix_stream_and_seqpacket_pairs() {
            for flags in [
                0,
                libc::SOCK_NONBLOCK,
                libc::SOCK_CLOEXEC,
                libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            ] {
                for kind in 0..=15 {
                    assert_eq!(
                        decision(
                            ARCH,
                            libc::SYS_socketpair as u32,
                            [libc::AF_UNIX as u64, (kind | flags) as u64, 0, 0, 0, 0]
                        ),
                        if kind == libc::SOCK_STREAM || kind == libc::SOCK_SEQPACKET {
                            ALLOW
                        } else {
                            DENY
                        }
                    );
                }
            }
            for args in [
                [libc::AF_INET as u64, libc::SOCK_STREAM as u64, 0, 0, 0, 0],
                [libc::AF_INET6 as u64, libc::SOCK_STREAM as u64, 0, 0, 0, 0],
                [libc::AF_UNIX as u64, libc::SOCK_DGRAM as u64, 0, 0, 0, 0],
                [libc::AF_UNIX as u64, 0, 0, 0, 0, 0],
                [
                    libc::AF_UNIX as u64,
                    (libc::SOCK_STREAM | 0x2000) as u64,
                    0,
                    0,
                    0,
                    0,
                ],
                [libc::AF_UNIX as u64, libc::SOCK_STREAM as u64, 1, 0, 0, 0],
                [
                    libc::AF_UNIX as u64,
                    libc::SOCK_SEQPACKET as u64,
                    1,
                    0,
                    0,
                    0,
                ],
            ] {
                assert_eq!(decision(ARCH, libc::SYS_socketpair as u32, args), DENY);
            }
            for syscall in [
                libc::SYS_socket,
                libc::SYS_connect,
                libc::SYS_bind,
                libc::SYS_listen,
                libc::SYS_accept,
                libc::SYS_accept4,
                libc::SYS_sendmsg,
                libc::SYS_recvmsg,
            ] {
                assert_eq!(
                    decision(ARCH, syscall as u32, [libc::AF_UNIX as u64, 1, 0, 0, 0, 0]),
                    DENY
                );
            }
        }

        #[test]
        fn seccomp_supports_threads_but_denies_namespaces_and_file_ioctls() {
            assert_eq!(
                decision(ARCH, libc::SYS_clone3 as u32, [0; 6]),
                0x0005_0000 | libc::ENOSYS as u32
            );
            for flag in [libc::CLONE_NEWUSER, libc::CLONE_NEWNS, libc::CLONE_PARENT] {
                assert_eq!(
                    decision(ARCH, libc::SYS_clone as u32, [flag as u64, 0, 0, 0, 0, 0]),
                    DENY
                );
            }
            assert_eq!(
                decision(
                    ARCH,
                    libc::SYS_clone as u32,
                    [
                        (libc::CLONE_VM | libc::CLONE_THREAD | libc::CLONE_SIGHAND) as u64,
                        0,
                        0,
                        0,
                        0,
                        0
                    ]
                ),
                ALLOW
            );
            assert_eq!(decision(ARCH, libc::SYS_prlimit64 as u32, [0; 6]), ALLOW);
            assert_eq!(
                decision(ARCH, libc::SYS_prlimit64 as u32, [123, 0, 0, 0, 0, 0]),
                DENY
            );
            assert_eq!(
                decision(
                    ARCH,
                    libc::SYS_ioctl as u32,
                    [1, libc::TIOCGWINSZ as u64, 0, 0, 0, 0]
                ),
                ALLOW
            );
            assert_eq!(
                decision(ARCH, libc::SYS_ioctl as u32, [1, 0x40086602, 0, 0, 0, 0]),
                DENY
            );
            assert_eq!(
                decision(
                    ARCH,
                    libc::SYS_fcntl as u32,
                    [1, libc::F_GETFD as u64, 0, 0, 0, 0]
                ),
                ALLOW
            );
            assert_eq!(
                decision(
                    ARCH,
                    libc::SYS_fcntl as u32,
                    [1, libc::F_SETOWN as u64, 0, 0, 0, 0]
                ),
                DENY
            );
        }

        #[test]
        fn restricted_paths_and_environment_fail_closed() {
            let root =
                std::env::temp_dir().join(format!("vl-restricted-policy-{}", uuid::Uuid::now_v7()));
            fs::create_dir(&root).unwrap();
            struct Cleanup(PathBuf);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    let _ = fs::remove_dir_all(&self.0);
                }
            }
            let _cleanup = Cleanup(root.clone());
            let work = root.join("work");
            let home = root.join("home");
            for path in [&work, &home] {
                fs::create_dir(path).unwrap();
            }
            let policy = Policy::Restricted {
                read_only: vec![],
                writable: vec![],
                network: Network::Denied,
                home: Some(home.clone()),
            };
            let checked = validated(&policy, work.to_str().unwrap()).unwrap();
            assert!(validated(&policy, "/").is_err());
            let mut invalid = policy.clone();
            if let Policy::Restricted { writable, .. } = &mut invalid {
                writable.push(work.clone());
            }
            assert!(validated(&invalid, work.to_str().unwrap()).is_err());
            if let Policy::Restricted { network, .. } = &mut invalid {
                *network = Network::Host;
            }
            assert!(validated(&invalid, work.to_str().unwrap()).is_err());
            let tmp = tmp_directory(&home).unwrap();
            let env = clean_environment(
                &HashMap::from([
                    ("OPENAI_API_KEY".into(), "synthetic".into()),
                    ("CODEX_HOME".into(), "/host/auth".into()),
                    ("LD_PRELOAD".into(), "/host/inject.so".into()),
                    ("OPENSSL_CONF".into(), "/host/openssl.cnf".into()),
                    ("PATH".into(), "/host/tools".into()),
                    ("HOME".into(), "/host/home".into()),
                    ("TERM".into(), "xterm".into()),
                ]),
                &checked,
                &tmp,
            )
            .unwrap();
            assert_eq!(env.get("PATH").unwrap(), SYSTEM_PATH);
            assert_eq!(env.get("OPENSSL_CONF").unwrap(), "/dev/null");
            assert_eq!(env.get("HOME").unwrap(), home.to_str().unwrap());
            assert!(
                !env.contains_key("OPENAI_API_KEY")
                    && !env.contains_key("CODEX_HOME")
                    && !env.contains_key("LD_PRELOAD")
            );
            assert_eq!(env.get("TERM").unwrap(), "xterm");
            assert!(executable("../bin/sh", &checked).is_err());
        }
    }
}
