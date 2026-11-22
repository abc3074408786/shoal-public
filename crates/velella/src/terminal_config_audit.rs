//! Read-only, bounded preflight for a restricted terminal's Codex configuration.
//!
//! A real app-server eagerly connects configured executors. The audit therefore
//! uses an exact private profile copy and an environment registry with *no*
//! executors. It never starts a thread, invokes a model, or consumes the live
//! restricted adapter's one-use PID receipt.
use anyhow::Result;
use std::{path::Path, process::Command};

pub(crate) fn audit(
    command: Command,
    directory: &Path,
    work: &Path,
    expected_profile: &str,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    { linux::audit(command, directory, work, expected_profile) }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (command, directory, work, expected_profile);
        anyhow::bail!("restricted terminal configuration audit requires Linux")
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use anyhow::Context;
    use serde_json::{json, Map, Value};
    use std::{
        fs::{self, DirBuilder, OpenOptions},
        io::{Read, Write},
        os::{
            fd::AsRawFd,
            unix::{
                fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
                process::CommandExt,
            },
        },
        path::PathBuf,
        process::{Child, Stdio},
        time::{Duration, Instant},
    };

    const PROFILE_LIMIT: usize = 1024 * 1024;
    const FRAME_LIMIT: usize = 4 * 1024 * 1024;
    const OUTPUT_LIMIT: usize = 8 * 1024 * 1024;
    const AUDIT_TIMEOUT: Duration = Duration::from_secs(8);

    struct PrivateDirectory(PathBuf);
    impl Drop for PrivateDirectory {
        fn drop(&mut self) {
            // This newly created directory is outside both tool-writable roots.
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct AuditProcess {
        child: Child,
        group: crate::process::RestrictedGroup,
    }
    impl Drop for AuditProcess {
        fn drop(&mut self) {
            // Do not poll/reap the leader before group cleanup: its PID pins the
            // process-group identity, including on protocol/timeout failures.
            let _ = self.group.kill();
            let _ = self.group.wait_and_finish();
            let _ = self.child.wait();
        }
    }

    fn private_directory(path: &Path) -> Result<()> {
        let meta = fs::symlink_metadata(path).context("inspect private audit directory")?;
        anyhow::ensure!(
            meta.is_dir() && meta.uid() == unsafe { libc::geteuid() }
                && meta.mode() & 0o777 == 0o700 && path.canonicalize()? == path,
            "configuration audit requires a canonical private directory"
        );
        Ok(())
    }

    fn read_private(path: &Path) -> Result<Vec<u8>> {
        let file = OpenOptions::new().read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path).context("open private configuration for audit")?;
        let meta = file.metadata()?;
        anyhow::ensure!(
            meta.is_file() && meta.uid() == unsafe { libc::geteuid() }
                && meta.nlink() == 1 && meta.mode() & 0o777 == 0o600
                && meta.len() <= PROFILE_LIMIT as u64,
            "configuration audit requires a private regular profile"
        );
        let mut bytes = Vec::new();
        file.take(PROFILE_LIMIT as u64 + 1).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() <= PROFILE_LIMIT, "audit profile exceeds size limit");
        Ok(bytes)
    }

    fn write_private_new(path: &Path, bytes: &[u8]) -> Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path).context("create private audit file")?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    }

    fn parse_profile(source: &str) -> Result<Value> {
        anyhow::ensure!(source.len() <= PROFILE_LIMIT, "audit profile exceeds size limit");
        let table: toml::Table = toml::from_str(source)
            .map_err(|_| anyhow::anyhow!("audit profile is not valid TOML"))?;
        serde_json::to_value(table).context("encode audit profile")
    }

    // The pinned CLI splits override *paths* on dots without parsing quoted
    // segments. Whole tables (particularly projects with dotted path keys) are
    // valid override values. Reject ambiguous paths instead of normalizing them.
    fn insert_override(root: &mut Value, path: &str, value: Value) -> Result<()> {
        let parts: Vec<_> = path.split('.').collect();
        anyhow::ensure!(
            parts.iter().all(|part| !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')),
            "unsupported configuration override path in audit"
        );
        let mut node = root;
        for part in &parts[..parts.len() - 1] {
            let table = node.as_object_mut().context("conflicting audit override paths")?;
            node = table.entry((*part).to_owned()).or_insert_with(|| json!({}));
            anyhow::ensure!(node.is_object(), "conflicting audit override paths");
        }
        node.as_object_mut().context("invalid audit override table")?
            .insert(parts[parts.len() - 1].to_owned(), value);
        Ok(())
    }

    fn command_overrides(command: &Command) -> Result<Value> {
        let mut result = json!({});
        let mut args = command.get_args();
        while let Some(arg) = args.next() {
            let arg = arg.to_str().context("audit arguments must be UTF-8")?;
            if arg == "--strict-config" { continue; }
            anyhow::ensure!(arg == "-c" || arg == "--config",
                "audit command must contain only fixed configuration overrides");
            let value = args.next().and_then(|s| s.to_str())
                .context("audit configuration override is missing")?;
            anyhow::ensure!(value.len() <= PROFILE_LIMIT, "audit override exceeds size limit");
            let (path, raw) = value.split_once('=').context("invalid audit override")?;
            let parsed: toml::Table = toml::from_str(&format!("value={raw}"))
                .map_err(|_| anyhow::anyhow!("audit override is not valid TOML"))?;
            anyhow::ensure!(parsed.len() == 1, "audit override contains extra TOML keys");
            insert_override(&mut result, path.trim(),
                serde_json::to_value(parsed.get("value").context("audit override has no value")?)?)?;
        }
        anyhow::ensure!(result.as_object().is_some_and(|m| !m.is_empty()),
            "restricted terminal requires explicit fixed CLI overrides");
        Ok(result)
    }

    fn merged(mut lower: Value, higher: &Value) -> Value {
        match (&mut lower, higher) {
            (Value::Object(low), Value::Object(high)) => {
                for (key, value) in high {
                    let old = low.remove(key).unwrap_or(Value::Null);
                    low.insert(key.clone(), merged(old, value));
                }
            }
            (low, high) => *low = high.clone(),
        }
        lower
    }

    fn require_expected_subset(actual: &Value, expected: &Value) -> Result<()> {
        if let Some(expected) = expected.as_object() {
            let actual = actual.as_object().context("effective configuration shape changed")?;
            for (key, value) in expected {
                require_expected_subset(
                    actual.get(key).context("effective configuration omitted a locked value")?,
                    value,
                )?;
            }
            Ok(())
        } else {
            anyhow::ensure!(actual == expected, "effective configuration changed a locked value");
            Ok(())
        }
    }

    fn empty_or_disabled(value: Option<&Value>) -> bool {
        match value {
            None | Some(Value::Null) => true,
            Some(Value::Object(map)) => map.values().all(|v| matches!(v, Value::Null)),
            Some(Value::Array(items)) => items.is_empty(),
            _ => false,
        }
    }

    fn check_effective(actual: &Value, expected: &Value) -> Result<()> {
        require_expected_subset(actual, expected)?;
        // Packaged defaults are omitted from config/read's public layer list.
        // Do not allow hidden layers to add command-backed integrations.
        for key in ["mcp_servers", "hooks", "notify"] {
            anyhow::ensure!(empty_or_disabled(actual.get(key)),
                "restricted terminal rejects additional executable configuration");
        }
        // Config/read adds null/default fields when serializing a provider.
        // Permit those inert defaults, but no additional provider or active
        // field (notably command-backed auth, headers or URL parameters).
        if let Some(providers) = expected.get("model_providers").and_then(Value::as_object) {
            let observed = actual.get("model_providers").and_then(Value::as_object)
                .context("effective model-provider configuration missing")?;
            anyhow::ensure!(observed.len() == providers.len(),
                "effective model-provider configuration changed");
            for (id, wanted) in providers {
                let wanted = wanted.as_object().context("invalid selected provider")?;
                let observed = observed.get(id).and_then(Value::as_object)
                    .context("effective selected provider missing")?;
                for (key, value) in observed {
                    if wanted.contains_key(key) { continue; }
                    anyhow::ensure!(value.is_null()
                        || (["requires_openai_auth", "supports_websockets",
                            "supports_standalone_web_search"].contains(&key.as_str())
                            && *value == Value::Bool(false)),
                        "effective model-provider configuration gained an active value");
                }
            }
        } else {
            anyhow::ensure!(empty_or_disabled(actual.get("model_providers")),
                "unexpected model-provider configuration");
        }
        Ok(())
    }

    fn verify_layers(
        response: &Value,
        copied_profile: &Path,
        expected_profile: &Value,
        expected_cli: &Value,
    ) -> Result<usize> {
        let layers = response.get("layers").and_then(Value::as_array)
            .context("Codex omitted audit configuration layers")?;
        let mut users = 0;
        let mut flags = 0;
        let mut projects = 0;
        for layer in layers {
            let kind = layer["name"]["type"].as_str().context("unknown audit configuration layer")?;
            let disabled = layer["disabledReason"].as_str().is_some_and(|s| !s.is_empty());
            let config = layer.get("config").filter(|c| c.is_object())
                .context("invalid audit configuration layer")?;
            if kind == "project" {
                projects += 1;
                anyhow::ensure!(disabled, "project-local configuration is enabled");
            }
            if disabled { continue; }
            match kind {
                "user" => {
                    anyhow::ensure!(layer["name"]["file"] == json!(copied_profile)
                        && layer["name"]["profile"].is_null() && config == expected_profile,
                        "audit loaded an unexpected user profile");
                    users += 1;
                }
                "sessionFlags" => {
                    anyhow::ensure!(config == expected_cli, "audit CLI overrides changed");
                    flags += 1;
                }
                _ => anyhow::ensure!(config.as_object().is_some_and(Map::is_empty),
                    "restricted terminal rejects configuration outside its private profile"),
            }
        }
        anyhow::ensure!(users == 1 && flags == 1, "audit profile or CLI layer is missing or duplicated");
        check_effective(response.get("config").context("Codex omitted effective configuration")?,
            &merged(expected_profile.clone(), expected_cli))?;
        Ok(projects)
    }

    fn nonblocking(fd: i32) -> Result<()> {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        anyhow::ensure!(flags >= 0
            && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0,
            "cannot configure bounded audit transport");
        Ok(())
    }

    fn wait_ready(fd: i32, events: i16, deadline: Instant) -> Result<()> {
        loop {
            let remaining = deadline.checked_duration_since(Instant::now())
                .context("Codex configuration audit timed out")?;
            let timeout = remaining.as_millis().clamp(1, 100) as i32;
            let mut poll = libc::pollfd { fd, events, revents: 0 };
            let status = unsafe { libc::poll(&mut poll, 1, timeout) };
            if status > 0 { return Ok(()); }
            if status < 0 {
                anyhow::ensure!(std::io::Error::last_os_error().kind()
                    == std::io::ErrorKind::Interrupted, "audit transport poll failed");
            }
        }
    }

    struct Wire {
        buffered: Vec<u8>,
        received: usize,
        records: usize,
        deadline: Instant,
    }
    impl Wire {
        fn send(&self, child: &mut Child, message: Value) -> Result<()> {
            let mut bytes = serde_json::to_vec(&message)?;
            bytes.push(b'\n');
            anyhow::ensure!(bytes.len() <= FRAME_LIMIT, "audit request exceeds frame limit");
            let pipe = child.stdin.as_mut().context("audit stdin is unavailable")?;
            let mut offset = 0;
            while offset < bytes.len() {
                wait_ready(pipe.as_raw_fd(), libc::POLLOUT, self.deadline)?;
                match pipe.write(&bytes[offset..]) {
                    Ok(0) => anyhow::bail!("audit stdin closed"),
                    Ok(count) => offset += count,
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::Interrupted) => {}
                    Err(_) => anyhow::bail!("audit request could not be written"),
                }
            }
            Ok(())
        }

        fn response(&mut self, child: &mut Child, wanted: u64) -> Result<Value> {
            loop {
                anyhow::ensure!(Instant::now() < self.deadline, "Codex configuration audit timed out");
                while let Some(newline) = self.buffered.iter().position(|b| *b == b'\n') {
                    anyhow::ensure!(newline <= FRAME_LIMIT, "audit response frame exceeds limit");
                    let line = self.buffered.drain(..=newline).collect::<Vec<_>>();
                    self.records += 1;
                    anyhow::ensure!(self.records <= 256, "audit emitted too many records");
                    let value: Value = serde_json::from_slice(&line)
                        .map_err(|_| anyhow::anyhow!("Codex emitted an invalid audit record"))?;
                    anyhow::ensure!(!(value.get("id").is_some() && value.get("method").is_some()),
                        "Codex requested an unsupported audit callback");
                    if value["id"] == json!(wanted) {
                        anyhow::ensure!(value.get("error").is_none(), "Codex rejected configuration audit");
                        return value.get("result").cloned().context("audit response omitted its result");
                    }
                }
                anyhow::ensure!(self.buffered.len() <= FRAME_LIMIT, "audit response frame exceeds limit");
                let pipe = child.stdout.as_mut().context("audit stdout is unavailable")?;
                wait_ready(pipe.as_raw_fd(), libc::POLLIN, self.deadline)?;
                let mut chunk = [0u8; 8192];
                match pipe.read(&mut chunk) {
                    Ok(0) => anyhow::bail!("Codex closed the configuration audit"),
                    Ok(count) => {
                        self.received += count;
                        anyhow::ensure!(self.received <= OUTPUT_LIMIT, "audit response exceeds total limit");
                        self.buffered.extend_from_slice(&chunk[..count]);
                    }
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::Interrupted) => {}
                    Err(_) => anyhow::bail!("audit response could not be read"),
                }
            }
        }

        fn rpc(&mut self, child: &mut Child, id: u64, method: &str, params: Value) -> Result<Value> {
            self.send(child, json!({"id":id,"method":method,"params":params}))?;
            self.response(child, id)
        }
    }

    pub(super) fn audit(
        mut command: Command,
        directory: &Path,
        work: &Path,
        expected_profile: &str,
    ) -> Result<()> {
        private_directory(directory)?;
        let controller = directory.join("controller");
        private_directory(&controller)?;
        let live_home = controller.join("codex");
        private_directory(&live_home)?;
        let live_profile = live_home.join("config.toml");
        anyhow::ensure!(work.is_absolute() && work.canonicalize()? == work
            && !directory.starts_with(work) && !work.starts_with(directory),
            "audit workspace and private state must be separate");
        anyhow::ensure!(command.get_current_dir() == Some(controller.as_path()),
            "audit must start in its private controller directory");
        anyhow::ensure!(read_private(&live_profile)? == expected_profile.as_bytes(),
            "private controller profile changed before audit");
        let expected = parse_profile(expected_profile)?;
        let overrides = command_overrides(&command)?;
        let temporary = directory.join(format!("config-audit-{}", uuid::Uuid::now_v7()));
        DirBuilder::new().mode(0o700).create(&temporary)?;
        let temporary = PrivateDirectory(temporary);
        let copied_profile = temporary.0.join("config.toml");
        write_private_new(&copied_profile, expected_profile.as_bytes())?;
        write_private_new(&temporary.0.join("environments.toml"),
            b"include_local = false\ndefault = \"none\"\n")?;

        // stderr may contain provider errors or config values. It is deliberately
        // discarded; all failures below are fixed, non-secret diagnostics.
        command.env("CODEX_HOME", &temporary.0).arg("app-server")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .process_group(0);
        let deadline = Instant::now() + AUDIT_TIMEOUT;
        let mut child = command.spawn().context("start Codex configuration audit")?;
        let group = match crate::process::RestrictedGroup::attach(child.id()) {
            Ok(group) => group,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.context("own Codex audit process group"));
            }
        };
        let mut process = AuditProcess { child, group };
        nonblocking(process.child.stdin.as_ref().context("audit stdin missing")?.as_raw_fd())?;
        nonblocking(process.child.stdout.as_ref().context("audit stdout missing")?.as_raw_fd())?;
        let mut wire = Wire { buffered: Vec::new(), received: 0, records: 0, deadline };
        wire.rpc(&mut process.child, 1, "initialize", json!({
            "clientInfo":{"name":"velella_configuration_audit","version":"1"},
            "capabilities":{"experimentalApi":true}
        }))?;
        wire.send(&mut process.child, json!({"method":"initialized"}))?;
        let response = wire.rpc(&mut process.child, 2, "config/read",
            json!({"cwd":work,"includeLayers":true}))?;
        let projects = verify_layers(&response, &copied_profile, &expected, &overrides)?;
        let requirements = wire.rpc(&mut process.child, 3, "configRequirements/read", json!({}))?;
        anyhow::ensure!(requirements.get("requirements") == Some(&Value::Null),
            "restricted terminal rejects managed configuration requirements");
        anyhow::ensure!(read_private(&copied_profile)? == expected_profile.as_bytes()
            && read_private(&live_profile)? == expected_profile.as_bytes(),
            "private controller profile changed during audit");
        drop(process);
        drop(temporary);
        let receipt = json!({
            "verified":true,
            "project_layers":projects,
            "project_layers_disabled":projects,
            "private_profile_unchanged":true,
            "cli_layer_verified":true,
            "requirements_absent":true,
            "audit_executors_disabled":true
        });
        crate::terminal_recovery::write_atomic(&directory.join("config-audit.json"), &serde_json::to_vec(&receipt)?)?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn response(profile: &Value, flags: &Value) -> Value {
            json!({
                "config":merged(profile.clone(),flags),
                "layers":[
                    {"name":{"type":"sessionFlags"},"config":flags,"disabledReason":null},
                    {"name":{"type":"user","file":"/private/copy/config.toml","profile":null},
                        "config":profile,"disabledReason":null},
                    {"name":{"type":"project","dotCodexFolder":"/work/.codex"},
                        "config":{"model":"hostile"},"disabledReason":"untrusted"}
                ]
            })
        }

        #[test]
        fn terminal_audit_requires_disabled_project_and_exact_layers() {
            let profile = json!({"model":"fixture","features":{"hooks":false}});
            let flags = json!({"features":{"hooks":false}});
            let valid = response(&profile, &flags);
            let check = |value: &Value| verify_layers(value,
                Path::new("/private/copy/config.toml"), &profile, &flags);
            assert_eq!(check(&valid).unwrap(), 1);
            let mut changed = valid.clone();
            changed["layers"][2]["disabledReason"] = Value::Null;
            assert!(check(&changed).is_err());
            let mut changed = valid.clone();
            changed["layers"].as_array_mut().unwrap().push(json!({
                "name":{"type":"system"},"config":{"mcp_servers":{"escape":{"command":"/bin/sh"}}}
            }));
            assert!(check(&changed).is_err());
            let mut changed = valid.clone();
            changed["layers"][0]["config"]["features"]["hooks"] = json!(true);
            assert!(check(&changed).is_err());
            let mut changed = valid;
            changed["config"]["features"]["hooks"] = json!(true);
            assert!(check(&changed).is_err());
        }

        #[test]
        fn terminal_audit_detects_hidden_executable_defaults_and_provider_changes() {
            let expected = json!({"model_providers":{"fixture":{"base_url":"http://127.0.0.1"}}});
            assert!(check_effective(&expected, &expected).is_ok());
            let mut defaults = expected.clone();
            defaults["model_providers"]["fixture"]["auth"] = Value::Null;
            defaults["model_providers"]["fixture"]["supports_standalone_web_search"] = json!(false);
            assert!(check_effective(&defaults, &expected).is_ok());
            defaults["model_providers"]["fixture"]["auth"] = json!({"command":"/bin/false"});
            assert!(check_effective(&defaults, &expected).is_err());
            for key in ["mcp_servers", "hooks", "notify"] {
                let mut actual = expected.clone();
                actual[key] = json!(["unexpected"]);
                assert!(check_effective(&actual, &expected).is_err());
            }
            let mut actual = expected.clone();
            actual["model_providers"]["other"] = json!({"command":"unexpected"});
            assert!(check_effective(&actual, &expected).is_err());
        }

        #[test]
        fn terminal_audit_parses_whole_project_table_without_splitting_path_keys() {
            let mut command = Command::new("/bin/false");
            command.args(["-c", "features.hooks=false",
                "-c", "projects={\"/work.with.dots\"={trust_level=\"untrusted\"}}"]);
            let parsed = command_overrides(&command).unwrap();
            assert_eq!(parsed["projects"]["/work.with.dots"]["trust_level"], "untrusted");
            assert_eq!(parsed["features"]["hooks"], false);
            command.args(["-c", "projects.\"/bad.path\".trust_level=\"untrusted\""]);
            assert!(command_overrides(&command).is_err());
        }

        #[test]
        fn terminal_audit_private_files_reject_links() {
            use std::os::unix::fs::symlink;
            let path = std::env::temp_dir().join(format!("vl-audit-test-{}", uuid::Uuid::now_v7()));
            DirBuilder::new().mode(0o700).create(&path).unwrap();
            let owned = PrivateDirectory(path);
            let source = owned.0.join("source");
            write_private_new(&source, b"private").unwrap();
            let link = owned.0.join("link");
            symlink(&source, &link).unwrap();
            assert!(read_private(&link).is_err());
            fs::hard_link(&source, owned.0.join("hardlink")).unwrap();
            assert!(read_private(&source).is_err());
        }

        #[test]
        fn terminal_audit_transport_bounds_stalled_and_oversized_children() {
            for script in [
                "import time;time.sleep(30)",
                "import os,time;os.write(1,b'x'*(4*1024*1024+8192));time.sleep(30)",
            ] {
                let child = Command::new("/usr/bin/python3").args(["-I", "-c", script])
                    .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
                    .process_group(0).spawn().unwrap();
                let pid = child.id();
                let group = crate::process::RestrictedGroup::attach(pid).unwrap();
                let mut process = AuditProcess { child, group };
                nonblocking(process.child.stdout.as_ref().unwrap().as_raw_fd()).unwrap();
                let start = Instant::now();
                let mut wire = Wire { buffered: Vec::new(), received: 0, records: 0,
                    deadline: start + Duration::from_millis(250) };
                assert!(wire.response(&mut process.child, 1).is_err());
                drop(process);
                assert!(start.elapsed() < Duration::from_secs(2));
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                assert_eq!(unsafe { libc::waitid(libc::P_PID, pid, &mut info,
                    libc::WEXITED | libc::WNOWAIT | libc::WNOHANG) }, -1);
                assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ECHILD));
            }
        }

        #[test]
        #[ignore = "requires explicitly supplied pinned CLI; uses disposable configuration only"]
        fn terminal_audit_actual_cli_uses_no_live_executor() {
            let cli = std::env::var_os("VELELLA_AUDIT_TEST_CODEX")
                .expect("set VELELLA_AUDIT_TEST_CODEX to the verified native CLI");
            let root = std::env::temp_dir().join(format!("vl-audit-cli-{}", uuid::Uuid::now_v7()));
            DirBuilder::new().mode(0o700).create(&root).unwrap();
            let root = PrivateDirectory(root);
            let state = root.0.join("state");
            let work = root.0.join("work");
            let controller = state.join("controller");
            let home = controller.join("home");
            let codex = controller.join("codex");
            for path in [&state, &work, &controller, &home, &codex] {
                DirBuilder::new().mode(0o700).create(path).unwrap();
            }
            let profile = format!(r#"
model = "mock-model"
model_provider = "fixture"
approval_policy = "never"
sandbox_mode = "danger-full-access"
check_for_update_on_startup = false
web_search = "disabled"
[features]
hooks = false
plugins = false
apps = false
[analytics]
enabled = false
[feedback]
enabled = false
[otel]
exporter = "none"
trace_exporter = "none"
metrics_exporter = "none"
[model_providers.fixture]
name = "No-model audit fixture"
base_url = "http://127.0.0.1:9/v1"
wire_api = "responses"
requires_openai_auth = false
request_max_retries = 0
stream_max_retries = 0
[projects.{}]
trust_level = "untrusted"
"#, json!(work).to_string());
            write_private_new(&codex.join("config.toml"), profile.as_bytes()).unwrap();
            // If the audit accidentally used the live registry, this would
            // create an owned marker, without performing any outside action.
            let marker = state.join("unexpected-live-executor");
            let registry = format!(
                "include_local=false\ndefault=\"trap\"\n[[environments]]\nid=\"trap\"\nprogram=\"/usr/bin/touch\"\nargs=[{}]\n",
                json!(marker)
            );
            write_private_new(&codex.join("environments.toml"), registry.as_bytes()).unwrap();
            let mut command = Command::new(cli);
            command.env_clear().env("PATH", "/usr/bin:/bin").env("LANG", "C.UTF-8")
                .env("HOME", &home).env("CODEX_HOME", &codex).env("TMPDIR", &controller)
                .current_dir(&controller).args(["-c", "features.hooks=false"]);
            audit(command, &state, &work, &profile).unwrap();
            assert!(!marker.exists());
            assert!(!state.join("exec-server.pid.json").exists());
            let receipt: Value = serde_json::from_slice(
                &read_private(&state.join("config-audit.json")).unwrap()).unwrap();
            assert_eq!(receipt["verified"], true);
            assert_eq!(receipt["audit_executors_disabled"], true);
            assert!(!fs::read_dir(&state).unwrap().any(|entry| {
                entry.unwrap().file_name().to_string_lossy().starts_with("config-audit-")
            }));
        }
    }
}
