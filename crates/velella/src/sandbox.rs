//! Explicit execution policy. Native preserves legacy behavior; isolated never
//! falls back to native. Linux is the first backend, not a cross-platform promise.
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub use velella_core::execution::{Network, Policy};

#[derive(Debug, Serialize)]
pub struct Capabilities {
    pub platform: &'static str,
    pub backend: &'static str,
    pub available: bool,
    pub reason: Option<String>,
    pub filesystem_allowlist: bool,
    pub process_namespace: bool,
    pub network_modes: Vec<&'static str>,
    pub resource_limits: bool,
    pub credential_broker: bool,
    pub restricted: RestrictedCapabilities,
}

#[derive(Debug, Serialize)]
pub struct RestrictedCapabilities {
    pub platform: &'static str,
    pub backend: &'static str,
    pub available: bool,
    pub reason: Option<String>,
    pub filesystem_allowlist: bool,
    pub process_namespace: bool,
    pub network_modes: Vec<&'static str>,
    pub experimental: bool,
}

fn backend(platform: &str) -> Result<&'static str> {
    match platform {
        "linux" => Ok("bubblewrap"),
        "windows" => {
            anyhow::bail!("Windows isolated backend is not implemented; native mode is available")
        }
        "macos" => {
            anyhow::bail!("macOS isolated backend is not implemented; native mode is available")
        }
        _ => anyhow::bail!("isolated backend is not implemented for {platform}"),
    }
}

// Fixed system executable: never resolve a sandbox launcher from task PATH.
const BWRAP: &str = "/usr/bin/bwrap";

pub fn capabilities() -> Capabilities {
    let result = backend(std::env::consts::OS).and_then(|_| probe());
    let available = result.is_ok();
    let restricted = crate::restricted::probe();
    let restricted_available = restricted.is_ok();
    Capabilities {
        platform: std::env::consts::OS,
        backend: if cfg!(target_os = "linux") {
            "bubblewrap"
        } else {
            "unimplemented"
        },
        available,
        reason: result.err().map(|e| format!("{e:#}")),
        filesystem_allowlist: available,
        process_namespace: available,
        network_modes: if available {
            vec!["denied", "host"]
        } else {
            vec![]
        },
        resource_limits: false,
        credential_broker: false,
        restricted: RestrictedCapabilities {
            platform: std::env::consts::OS,
            backend: "landlock-seccomp",
            available: restricted_available,
            reason: restricted.err().map(|e| format!("{e:#}")),
            filesystem_allowlist: restricted_available,
            process_namespace: false,
            network_modes: if restricted_available { vec!["denied"] } else { vec![] },
            experimental: true,
        },
    }
}

fn base_args(network: Network) -> Vec<String> {
    let mut args: Vec<String> = [
        "--unshare-user",
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-uts",
        "--die-with-parent",
        "--new-session",
        "--cap-drop",
        "ALL",
        "--clearenv",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
        "--dir",
        "/home/velella",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    if network == Network::Denied {
        args.push("--unshare-net".into());
    }
    // Only system runtime files are visible by default, never the host root/HOME.
    for path in [
        "/usr",
        "/bin",
        "/sbin",
        "/lib",
        "/lib64",
        "/etc/ld.so.cache",
        "/etc/ssl/certs",
    ] {
        if Path::new(path).exists() {
            args.extend(["--ro-bind".into(), path.into(), path.into()]);
        }
    }
    if network == Network::Host {
        for path in ["/etc/resolv.conf", "/etc/hosts", "/etc/nsswitch.conf"] {
            if Path::new(path).exists() {
                args.extend(["--ro-bind".into(), path.into(), path.into()]);
            }
        }
    }
    args
}

pub fn ensure_available(policy: &Policy) -> Result<()> {
    if policy.is_restricted() {
        if let Policy::Restricted { network, .. } = policy {
            anyhow::ensure!(*network == Network::Denied, "restricted execution only supports denied networking");
        }
        return crate::restricted::probe().context("restricted execution unavailable; refusing native fallback");
    }
    if !policy.is_isolated() {
        return Ok(());
    }
    backend(std::env::consts::OS)?;
    probe().context("isolated execution unavailable; refusing native fallback")
}

fn probe() -> Result<()> {
    let mut args = base_args(Network::Denied);
    args.extend(["--".into(), "/bin/true".into()]);
    // No inherited loader variables or credentials even in the launcher probe.
    let mut child = Command::new(BWRAP)
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("cannot start /usr/bin/bwrap")?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if child.try_wait()?.is_some() {
            let output = child.wait_with_output()?;
            anyhow::ensure!(
                output.status.success(),
                "sandbox probe failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("sandbox probe timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: HashMap<String, String>,
    pub clear_env: bool,
}

fn directory(path: &Path) -> Result<PathBuf> {
    anyhow::ensure!(
        path.is_absolute(),
        "sandbox paths must be absolute: {}",
        path.display()
    );
    let resolved = path
        .canonicalize()
        .with_context(|| format!("sandbox path unavailable: {}", path.display()))?;
    anyhow::ensure!(resolved.is_dir(), "sandbox mount must be a directory");
    // Reject symlink aliases instead of changing what a policy means silently.
    anyhow::ensure!(
        resolved == path,
        "sandbox paths must be canonical: {}",
        path.display()
    );
    for reserved in [
        "/usr",
        "/bin",
        "/sbin",
        "/lib",
        "/lib64",
        "/etc",
        "/proc",
        "/dev",
        "/home/velella",
    ] {
        let reserved = Path::new(reserved);
        anyhow::ensure!(
            !resolved.starts_with(reserved) && !reserved.starts_with(&resolved),
            "sandbox mount overlaps reserved path: {}",
            path.display()
        );
    }
    anyhow::ensure!(
        resolved != Path::new("/tmp"),
        "cannot expose host /tmp; use a task subdirectory"
    );
    Ok(resolved)
}

fn isolated_args(
    policy: &Policy,
    program: &str,
    argv: &[String],
    cwd: Option<&str>,
    env: &HashMap<String, String>,
) -> Result<Vec<String>> {
    let Policy::Isolated {
        read_only,
        writable,
        network,
        home,
    } = policy
    else {
        anyhow::bail!("isolated policy required")
    };
    let cwd = directory(Path::new(
        cwd.context("isolated execution requires an explicit cwd")?,
    ))?;
    let mut mounts: Vec<(PathBuf, bool)> = vec![(cwd.clone(), true)];
    for (paths, write) in [(read_only, false), (writable, true)] {
        for path in paths {
            mounts.push((directory(path)?, write));
        }
    }
    // Disallow overlapping mounts: nested rebinds otherwise make policy order significant.
    for (i, (path, _)) in mounts.iter().enumerate() {
        for (other, _) in mounts.iter().take(i) {
            anyhow::ensure!(
                !path.starts_with(other) && !other.starts_with(path),
                "overlapping sandbox mounts: {} and {}",
                path.display(),
                other.display()
            );
        }
    }
    let home = home.as_deref().map(directory).transpose()?;
    if let Some(home) = &home {
        for (path, _) in &mounts {
            anyhow::ensure!(
                !path.starts_with(home) && !home.starts_with(path),
                "sandbox HOME overlaps another mount"
            );
        }
    }
    let mut args = base_args(*network);
    if let Some(home) = home {
        args.extend([
            "--bind".into(),
            home.to_str().context("HOME path must be UTF-8")?.into(),
            "/home/velella".into(),
        ]);
    }
    for (path, write) in mounts {
        let path = path
            .to_str()
            .context("sandbox paths must be UTF-8")?
            .to_owned();
        args.extend([
            if write { "--bind" } else { "--ro-bind" }.into(),
            path.clone(),
            path,
        ]);
    }
    let mut child_env = env.clone();
    child_env
        .entry("PATH".into())
        .or_insert_with(|| "/usr/local/bin:/usr/bin:/bin".into());
    child_env.insert("HOME".into(), "/home/velella".into());
    child_env.insert("TMPDIR".into(), "/tmp".into());
    child_env
        .entry("TERM".into())
        .or_insert_with(|| "xterm-256color".into());
    child_env
        .entry("COLORTERM".into())
        .or_insert_with(|| "truecolor".into());
    // Values belong inside the sandbox, never in the launcher's environment.
    let mut entries: Vec<_> = child_env.into_iter().collect();
    entries.sort();
    for (key, value) in entries {
        anyhow::ensure!(
            !key.is_empty() && !key.contains(['=', '\0']) && !value.contains('\0'),
            "invalid sandbox environment variable"
        );
        args.extend(["--setenv".into(), key, value]);
    }
    args.extend([
        "--chdir".into(),
        cwd.to_str().context("cwd must be UTF-8")?.into(),
        "--".into(),
        program.into(),
    ]);
    args.extend_from_slice(argv);
    Ok(args)
}

/// Validate a policy without running a process (used during descriptor creation).
pub fn validate(policy: &Policy, cwd: &str) -> Result<()> {
    if policy.is_restricted() {
        return crate::restricted::validate(policy, cwd);
    }
    if policy.is_isolated() {
        isolated_args(policy, "/bin/true", &[], Some(cwd), &HashMap::new())?;
    }
    Ok(())
}

pub fn prepare(
    policy: &Policy,
    program: &str,
    args: &[String],
    cwd: Option<&str>,
    env: &HashMap<String, String>,
) -> Result<Launch> {
    if policy.is_restricted() {
        return crate::restricted::prepare(policy, program, args, cwd, env);
    }
    if matches!(policy, Policy::Native {}) {
        return Ok(Launch {
            program: program.into(),
            args: args.to_vec(),
            cwd: cwd.map(str::to_owned),
            env: env.clone(),
            clear_env: false,
        });
    }
    backend(std::env::consts::OS)?;
    let args = isolated_args(policy, program, args, cwd, env)?;
    probe().context("isolated execution unavailable; refusing native fallback")?;
    Ok(Launch {
        program: BWRAP.into(),
        args,
        cwd: None,
        env: HashMap::new(),
        clear_env: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> Policy {
        Policy::Isolated {
            read_only: vec![],
            writable: vec![],
            network: Network::Denied,
            home: None,
        }
    }
    #[test]
    #[ignore = "requires permitted Linux namespaces and a system-installed real Codex CLI"]
    #[cfg(target_os = "linux")]
    fn real_codex_starts_without_login_inside_isolation() {
        assert!(capabilities().available, "real sandbox required");
        let executable = std::env::var("VELELLA_TEST_CODEX")
            .expect("explicit path to the real Codex executable required");
        let root = std::env::temp_dir().join(format!("vl-cli-{}", uuid::Uuid::now_v7()));
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
        }
        std::fs::create_dir(&root).unwrap();
        let _cleanup = Cleanup(root.clone());
        let work = root.join("work");
        let home = root.join("home");
        let host_auth = root.join("host-auth");
        for path in [&work, &home, &host_auth] { std::fs::create_dir(path).unwrap(); }
        let work = work.canonicalize().unwrap();
        let isolated = Policy::Isolated {
            read_only: vec![], writable: vec![], network: Network::Denied,
            home: Some(home.canonicalize().unwrap()),
        };
        let run = |arguments: &[&str]| {
            let launch = prepare(&isolated, &executable,
                &arguments.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
                work.to_str(), &HashMap::new()).unwrap();
            let mut child = Command::new(launch.program).args(launch.args)
                .env_clear()
                // Synthetic values verify that launcher env cannot select host auth.
                .env("CODEX_HOME", &host_auth)
                .env("OPENAI_API_KEY", "fixture-must-not-be-inherited")
                .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                if child.try_wait().unwrap().is_some() { break; }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("Codex startup command timed out");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let output = child.wait_with_output().unwrap();
            let text = format!("{}{}", String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr));
            (output.status, text)
        };
        let (status, version) = run(&["--version"]);
        assert!(status.success() && version.contains("codex"), "{version}");
        eprintln!("Real sandboxed CLI version: {}", version.trim());
        let (status, help) = run(&["exec", "--help"]);
        assert!(status.success() && help.contains("--sandbox"), "{help}");
        let (status, auth) = run(&["login", "status"]);
        assert!(!status.success() && auth.to_lowercase().contains("not logged in"), "{auth}");
        assert!(!home.join(".codex/auth.json").exists(), "smoke test must not create credentials");
        assert_eq!(std::fs::read_dir(&host_auth).unwrap().count(), 0);
        eprintln!("Real CLI startup/help/auth status passed with denied network and no login.");
    }
    #[test]
    fn unsupported_platforms_fail_explicitly() {
        assert!(backend("windows")
            .unwrap_err()
            .to_string()
            .contains("not implemented"));
        assert!(backend("macos").is_err());
        assert!(backend("linux").is_ok());
    }
    #[test]
    fn unknown_policy_fields_and_network_rejected() {
        assert!(serde_json::from_str::<Policy>(r#"{"mode":"isolated","netwrok":"host"}"#).is_err());
        assert!(
            serde_json::from_str::<Policy>(r#"{"mode":"isolated","network":"allowlist"}"#).is_err()
        );
        assert!(serde_json::from_str::<Policy>(r#"{"mode":"native","writable":["/"]}"#).is_err());
    }
    #[test]
    fn native_preserves_command_and_environment() {
        let env = HashMap::from([("EXPLICIT".into(), "value".into())]);
        let result = prepare(
            &Policy::Native {},
            "tool",
            &["a b".into()],
            Some("relative"),
            &env,
        )
        .unwrap();
        assert_eq!(result.args, ["a b"]);
        assert_eq!(result.env, env);
        assert!(!result.clear_env);
    }
    #[test]
    fn unsafe_mounts_rejected_before_launch() {
        assert!(isolated_args(&policy(), "true", &[], Some("/"), &HashMap::new()).is_err());
        assert!(isolated_args(&policy(), "true", &[], Some("relative"), &HashMap::new()).is_err());
        assert!(isolated_args(&policy(), "true", &[], None, &HashMap::new()).is_err());
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn policy_is_explicit_and_rejects_overlapping_mounts() {
        let root = std::env::temp_dir().join(format!("velella-policy-{}", uuid::Uuid::now_v7()));
        let work = root.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let work = work.canonicalize().unwrap();
        let env = HashMap::from([
            ("HOME".into(), "/host/home".into()),
            ("EXPLICIT".into(), "a b;$x".into()),
        ]);
        let args = isolated_args(
            &policy(),
            "/bin/sh",
            &["-c".into(), "echo ok".into()],
            work.to_str(),
            &env,
        )
        .unwrap();
        assert!(args.iter().any(|a| a == "--unshare-net"));
        assert!(args
            .windows(3)
            .any(|a| a == ["--setenv", "HOME", "/home/velella"]));
        assert!(args
            .windows(3)
            .any(|a| a == ["--setenv", "EXPLICIT", "a b;$x"]));
        assert!(!args.iter().any(|a| a == "/host/home"));
        let overlap = Policy::Isolated {
            read_only: vec![work.clone()],
            writable: vec![],
            network: Network::Host,
            home: None,
        };
        assert!(isolated_args(&overlap, "true", &[], work.to_str(), &env).is_err());
        let host = Policy::Isolated {
            read_only: vec![],
            writable: vec![],
            network: Network::Host,
            home: None,
        };
        let host_args = isolated_args(&host, "true", &[], work.to_str(), &env).unwrap();
        assert!(!host_args.iter().any(|a| a == "--unshare-net"));
        std::os::unix::fs::symlink(&work, root.join("alias")).unwrap();
        assert!(directory(&root.join("alias")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn live_confinement_or_explicit_unavailable() {
        let root = std::env::temp_dir().join(format!("velella-sandbox-{}", uuid::Uuid::now_v7()));
        let work = root.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let secret = root.join("outside");
        std::fs::write(&secret, "private").unwrap();
        let work = work.canonicalize().unwrap();
        let script = format!("test ! -e '{}' && test \"$HOME\" = /home/velella && test -z \"$SSH_AUTH_SOCK\" && echo allowed > result", secret.display());
        let result = prepare(
            &policy(),
            "/bin/sh",
            &["-c".into(), script],
            work.to_str(),
            &HashMap::new(),
        );
        if capabilities().available {
            result.unwrap();
            // These are temporary fixtures, never host credentials or user files.
            let readonly = root.join("readonly");
            let cache = root.join("cache");
            let home = root.join("home");
            for path in [&readonly, &cache, &home] {
                std::fs::create_dir(path).unwrap();
            }
            std::fs::write(readonly.join("reference"), "unchanged").unwrap();
            std::os::unix::fs::symlink(&secret, work.join("escape")).unwrap();
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let env = HashMap::from([
                ("CASE_SECRET".into(), secret.to_string_lossy().into_owned()),
                (
                    "CASE_READONLY".into(),
                    readonly.to_string_lossy().into_owned(),
                ),
                ("CASE_CACHE".into(), cache.to_string_lossy().into_owned()),
                (
                    "CASE_PORT".into(),
                    listener.local_addr().unwrap().port().to_string(),
                ),
                (
                    "CASE_PID_NS".into(),
                    std::fs::read_link("/proc/self/ns/pid")
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                ),
            ]);
            let mut persistent = Policy::Isolated {
                read_only: vec![readonly.clone()],
                writable: vec![cache.clone()],
                network: Network::Denied,
                home: Some(home.clone()),
            };
            let script = r#"
import errno, os, pathlib, socket
assert os.environ['HOME'] == '/home/velella'
assert 'VELELLA_HOST_ONLY_FIXTURE' not in os.environ
assert not pathlib.Path(os.environ['CASE_SECRET']).exists()
assert not pathlib.Path('escape').exists()
assert os.readlink('/proc/self/ns/pid') != os.environ['CASE_PID_NS']
reference = pathlib.Path(os.environ['CASE_READONLY']) / 'reference'
assert reference.read_text() == 'unchanged'
try:
    reference.write_text('forbidden')
except OSError as error:
    assert error.errno in (errno.EROFS, errno.EACCES, errno.EPERM)
else:
    raise AssertionError('read-only mount was writable')
try:
    sock = socket.create_connection(('127.0.0.1', int(os.environ['CASE_PORT'])), timeout=1)
except OSError:
    pass
else:
    sock.close()
    raise AssertionError('denied network reached host localhost')
pathlib.Path('result').write_text('allowed')
(pathlib.Path(os.environ['CASE_CACHE']) / 'result').write_text('allowed')
pathlib.Path('/home/velella/session-state').write_text('retained')
pathlib.Path('/tmp/ephemeral').write_text('temporary')
"#;
            let run = |policy: &Policy, script: &str| {
                let launch = prepare(
                    policy,
                    "/usr/bin/python3",
                    &["-c".into(), script.into()],
                    work.to_str(),
                    &env,
                )
                .unwrap();
                let output = Command::new(launch.program)
                    .args(launch.args)
                    .env_clear()
                    .env("VELELLA_HOST_ONLY_FIXTURE", "must-not-be-inherited")
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            };
            run(&persistent, script);
            assert_eq!(
                std::fs::read_to_string(readonly.join("reference")).unwrap(),
                "unchanged"
            );
            assert!(work.join("result").exists());
            assert!(cache.join("result").exists());
            if let Policy::Isolated { network, .. } = &mut persistent {
                *network = Network::Host;
            }
            run(
                &persistent,
                r#"
import os, pathlib, socket
assert pathlib.Path('/home/velella/session-state').read_text() == 'retained'
assert not pathlib.Path('/tmp/ephemeral').exists()
with socket.create_connection(('127.0.0.1', int(os.environ['CASE_PORT'])), timeout=1) as sock:
    sock.sendall(b'explicit-host-network')
"#,
            );
        } else {
            assert!(
                std::env::var_os("VELELLA_REQUIRE_SANDBOX").is_none(),
                "this validation run requires a functioning sandbox backend"
            );
            let error = result.err().expect("must fail closed");
            assert!(format!("{error:#}").contains("refusing native fallback"));
            assert!(!work.join("result").exists());
            eprintln!("live confinement unavailable; verified refusal: {error:#}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
