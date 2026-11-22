//! Native configuration boundaries verified against installed CLI loaders.
//! These checks do not approve trust, authenticate, or modify account settings.
use super::*;

fn file_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 1024 * 1024,
        "native policy must be a bounded ordinary file: {}", path.display());
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 1024 * 1024, "native policy exceeds bounds: {}", path.display());
    Ok(Some(bytes))
}

fn stamp(bytes: &Option<Vec<u8>>) -> Value {
    bytes.as_ref().map(|bytes| json!(format!("{:x}", Sha256::digest(bytes)))).unwrap_or(Value::Null)
}

#[cfg(windows)]
fn codex_servers(desc: &crate::host::Descriptor) -> Result<Vec<Value>> {
    let mut command = crate::process::command(&desc.spawn.program);
    command.current_dir(desc.spawn.cwd.as_deref().context("Codex role has no workspace")?);
    command.envs(desc.spawn.env.iter().filter(|(key, _)| !key.starts_with("VELELLA_")));
    // Only native configuration arguments belong to the offline query; never
    // pass prompts or resume actions. This command does not connect MCP servers.
    let mut args = desc.spawn.args.iter();
    while let Some(arg) = args.next() {
        if matches!(arg.as_str(), "-c" | "--config" | "-p" | "--profile") {
            command.arg(arg).arg(args.next().context("incomplete Codex configuration argument")?);
        }
    }
    command.args(["mcp", "list", "--json"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn().context("cannot read effective Codex MCP configuration")?;
    let group = match crate::process::ChildGroup::attach(child.id()) {
        Ok(group) => group,
        Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error); }
    };
    let mut pipe = child.stdout.take().context("Codex configuration output missing")?;
    let reader = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut kept = Vec::new(); let mut buffer = [0_u8; 8192];
        loop {
            let count = pipe.read(&mut buffer)?; if count == 0 { break; }
            let remaining = (1024 * 1024 + 1_usize).saturating_sub(kept.len());
            kept.extend_from_slice(&buffer[..count.min(remaining)]);
        }
        Ok(kept)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => { let _ = group.kill(); let _ = child.wait(); break None; }
        }
    };
    drop(group);
    let bytes = reader.join().map_err(|_| anyhow::anyhow!("Codex configuration reader failed"))??;
    anyhow::ensure!(status.is_some_and(|status| status.success()), "effective Codex MCP configuration query failed or exceeded 5 seconds; no role was started");
    anyhow::ensure!(bytes.len() <= 1024 * 1024, "Codex MCP configuration exceeds bounds");
    // Do not put native output (which can contain credentials) into errors,
    // receipts, or logs. Only selected proxy configurations are persisted.
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("Codex MCP configuration query did not return a supported server list"))
}

#[cfg(not(windows))]
fn codex_servers(_desc: &crate::host::Descriptor) -> Result<Vec<Value>> {
    anyhow::bail!("bounded Codex native MCP configuration verification requires the supported Windows host")
}

fn verify_codex_servers(actual: &[Value], expected: &Value) -> Result<()> {
    let selected = expected.as_object().context("selected Codex MCP configuration is missing")?;
    let mut seen = std::collections::HashSet::new();
    for server in actual {
        let name = server["name"].as_str().context("Codex MCP server name is missing")?;
        anyhow::ensure!(seen.insert(name), "Codex MCP configuration contains duplicate server names");
        let enabled = server["enabled"].as_bool().context("Codex MCP enabled state is missing")?;
        if let Some(binding) = selected.get(name) {
            let transport = &server["transport"];
            anyhow::ensure!(enabled && transport["type"] == "stdio" && transport["command"] == binding["command"] && transport["args"] == binding["args"]
                && (transport["env"].is_null() || transport["env"].as_object().is_some_and(|env| env.is_empty()))
                && (transport["env_vars"].is_null() || transport["env_vars"].as_array().is_some_and(|vars| vars.is_empty()))
                && transport["cwd"].is_null(),
                "selected Codex MCP server {name} has an inherited transport or policy conflict; use a non-conflicting server ID or adjust configuration explicitly");
        } else {
            anyhow::ensure!(!enabled, "an unselected Codex MCP server is enabled; create a new role session");
        }
    }
    anyhow::ensure!(selected.keys().all(|name| seen.contains(name.as_str())), "Codex did not load every selected MCP server");
    Ok(())
}

pub(super) fn configure_codex(desc: &mut crate::host::Descriptor, servers: &Value) -> Result<()> {
    let selected = servers.as_object().context("Codex MCP servers must be an object")?;
    let inherited = codex_servers(desc)?;
    let mut enabled = servers.clone();
    for server in enabled.as_object_mut().unwrap().values_mut() { server["enabled"] = json!(true); }
    for server in inherited {
        let name = server["name"].as_str().context("Codex MCP server name is missing")?;
        if !selected.contains_key(name) {
            enabled.as_object_mut().unwrap().insert(name.into(), json!({"enabled":false}));
        }
    }
    // One table is essential: a later top-level CLI override discards earlier
    // per-name overrides within the CLI layer. TOML table keys also preserve
    // names containing dots, unlike Codex's simple dotted-path argument parser.
    desc.spawn.args.extend(["-c".into(), format!("mcp_servers={}", to_toml(&enabled)?)]);
    verify_codex_servers(&codex_servers(desc)?, servers)?;
    desc.spawn.env.insert("VELELLA_CODEX_MCP_EXPECTED".into(), servers.to_string());
    Ok(())
}

pub(super) fn check_kimi_project(cwd: &Path) -> Result<()> {
    // Kimi 0.36.1 walks to the nearest .git (directory or worktree pointer).
    let cwd = dunce::canonicalize(cwd).context("Kimi workspace is unavailable")?;
    let mut project = cwd.clone();
    loop {
        if project.join(".git").try_exists()? { break; }
        if !project.pop() { project = cwd.clone(); break; }
    }
    for path in [project.join(".mcp.json"), cwd.join(".kimi-code/mcp.json")] {
        let Some(bytes) = file_bytes(&path)? else { continue };
        if bytes.iter().all(u8::is_ascii_whitespace) { continue; }
        let value: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("cannot parse Kimi project MCP configuration: {}", path.display()))?;
        anyhow::ensure!(value.is_object(), "Kimi project MCP configuration must be an object: {}", path.display());
        if let Some(servers) = value.get("mcpServers") {
            anyhow::ensure!(servers.as_object().is_some_and(|servers| servers.is_empty()),
                "project MCP configuration is not allowed for a role with an isolated trusted catalog: {}", path.display());
        }
    }
    Ok(())
}

fn check_gemini_policy(value: &Value, names: &[String], path: &Path) -> Result<()> {
    anyhow::ensure!(value.is_object(), "Gemini policy must be an object: {}", path.display());
    anyhow::ensure!(value.get("mcp").map_or(true, Value::is_object), "Gemini mcp policy must be an object: {}", path.display());
    for key in ["allowed", "excluded"] {
        let Some(list) = value.get("mcp").and_then(|mcp| mcp.get(key)) else { continue };
        let list = list.as_array().with_context(|| format!("Gemini mcp.{key} must be an array: {}", path.display()))?;
        let normalized = list.iter().map(|item| item.as_str().map(|s| s.trim().to_lowercase()).context("Gemini MCP policy entries must be strings"))
            .collect::<Result<Vec<_>>>()?;
        for name in names {
            let present = normalized.contains(&name.to_lowercase());
            anyhow::ensure!(if key == "allowed" { present } else { !present },
                "selected MCP server {name} conflicts with existing Gemini mcp.{key}: {}; adjust the role or the policy explicitly", path.display());
        }
    }
    Ok(())
}

fn parse_gemini_settings(bytes: &[u8], path: &Path) -> Result<Value> {
    // Gemini uses strip-json-comments followed by JSON.parse, without trailing
    // comma support. Preserve byte offsets and all content inside JSON strings.
    let mut stripped = bytes.to_vec();
    let mut index = 0; let mut in_string = false; let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped { escaped = false; }
            else if byte == b'\\' { escaped = true; }
            else if byte == b'"' { in_string = false; }
            index += 1; continue;
        }
        if byte == b'"' { in_string = true; index += 1; continue; }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            while index < bytes.len() && !matches!(bytes[index], b'\n' | b'\r') {
                stripped[index] = b' '; index += 1;
            }
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            stripped[index..index + 2].fill(b' '); index += 2;
            let mut closed = false;
            while index < bytes.len() {
                if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    stripped[index..index + 2].fill(b' '); index += 2; closed = true; break;
                }
                if !matches!(bytes[index], b'\n' | b'\r') { stripped[index] = b' '; }
                index += 1;
            }
            anyhow::ensure!(closed, "unterminated comment in Gemini policy: {}", path.display());
        } else { index += 1; }
    }
    let value: Value = serde_json::from_slice(&stripped)
        .with_context(|| format!("cannot parse Gemini policy JSONC: {}", path.display()))?;
    anyhow::ensure!(value.is_object(), "Gemini settings must be an object: {}", path.display());
    Ok(value)
}

pub(super) fn gemini_settings(path: &Path) -> Result<Value> {
    match file_bytes(path)? { Some(bytes) => parse_gemini_settings(&bytes, path), None => Ok(json!({})) }
}

fn capture_gemini_policies(paths: &[PathBuf], names: &[String]) -> Result<Vec<Value>> {
    let mut bindings = Vec::new();
    for path in paths {
        let bytes = file_bytes(path)?;
        if let Some(bytes) = &bytes {
            let value = parse_gemini_settings(bytes, path)?;
            check_gemini_policy(&value, names, path)?;
        }
        bindings.push(json!({"path":path,"sha256":stamp(&bytes)}));
    }
    Ok(bindings)
}

pub(super) fn configure_gemini(desc: &mut crate::host::Descriptor, base: &Path, config: &mut Value, servers: &Value) -> Result<()> {
    let names = servers.as_object().context("Gemini MCP servers must be an object")?.keys().cloned().collect::<Vec<_>>();
    anyhow::ensure!(!names.is_empty() && names.iter().any(|name| name == "velella"), "Gemini requires the control MCP server");
    let defaults = std::env::var_os("GEMINI_CLI_SYSTEM_DEFAULTS_PATH").map(PathBuf::from)
        .unwrap_or_else(|| base.parent().unwrap_or(Path::new(".")).join("system-defaults.json"));
    let home = std::env::var_os("GEMINI_CLI_HOME").or_else(|| std::env::var_os("USERPROFILE")).or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from).context("Gemini user home is unavailable")?;
    let cwd = PathBuf::from(desc.spawn.cwd.as_deref().context("Gemini role has no workspace")?);
    let bindings = capture_gemini_policies(&[base.to_owned(), defaults.clone(), home.join(".gemini/settings.json"), cwd.join(".gemini/settings.json")], &names)?;
    // CLI's explicit allowlist prevents an empty cross-scope intersection from
    // becoming an unrestricted list. Existing allow/exclude policies above must
    // permit every selected name; this never broadens a known policy.
    desc.spawn.args.extend(["--allowed-mcp-server-names".into(), names.join(",")]);
    config["mcp"]["allowed"] = json!(names);
    // Moving the system settings file must not relocate its sibling defaults.
    desc.spawn.env.insert("GEMINI_CLI_SYSTEM_DEFAULTS_PATH".into(), defaults.to_string_lossy().into_owned());
    desc.spawn.env.insert("VELELLA_NATIVE_POLICY_BINDINGS".into(), serde_json::to_string(&bindings)?);
    Ok(())
}

pub(super) fn verify(desc: &crate::host::Descriptor) -> Result<()> {
    if desc.spawn.env.get("VELELLA_PROVIDER").is_some_and(|provider| provider == "kimi") {
        check_kimi_project(Path::new(desc.spawn.cwd.as_deref().context("Kimi role has no workspace")?))?;
    }
    if let Some(bindings) = desc.spawn.env.get("VELELLA_NATIVE_POLICY_BINDINGS") {
        for binding in serde_json::from_str::<Vec<Value>>(bindings)? {
            let path = Path::new(binding["path"].as_str().context("native policy path is missing")?);
            anyhow::ensure!(stamp(&file_bytes(path)?) == binding["sha256"],
                "native MCP policy changed; create a new session: {}", path.display());
        }
    }
    if desc.spawn.env.get("VELELLA_PROVIDER").is_some_and(|provider| provider == "codex") {
        let expected: Value = serde_json::from_str(desc.spawn.env.get("VELELLA_CODEX_MCP_EXPECTED")
            .context("legacy Codex role has no effective native MCP verification; create a new session")?)?;
        verify_codex_servers(&codex_servers(desc)?, &expected)?;
    }
    if desc.spawn.env.get("VELELLA_PROVIDER").is_some_and(|provider| provider == "gemini") {
        anyhow::ensure!(desc.spawn.env.contains_key("VELELLA_NATIVE_POLICY_BINDINGS"), "legacy Gemini role has no native MCP policy binding; create a new session");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("velella-native-policy-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap(); root
    }
    #[test]
    fn codex_effective_catalog_rejects_inheritance_and_transport_drift() {
        let selected = json!({"velella":{"command":"runtime","args":["control"]}});
        let control = json!({"name":"velella","enabled":true,"transport":{"type":"stdio","command":"runtime","args":["control"],"env":null,"env_vars":[],"cwd":null}});
        verify_codex_servers(&[control.clone()], &selected).unwrap();
        assert!(verify_codex_servers(&[control.clone(),json!({"name":"other","enabled":true})], &selected).is_err());
        verify_codex_servers(&[control.clone(),json!({"name":"other","enabled":false})], &selected).unwrap();
        let mut altered = control.clone(); altered["transport"]["env"] = json!({"UNEXPECTED":"value"});
        assert!(verify_codex_servers(&[altered], &selected).is_err());
        let mut disabled = control; disabled["enabled"] = json!(false);
        assert!(verify_codex_servers(&[disabled], &selected).is_err());
        assert!(verify_codex_servers(&[], &selected).is_err());
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "requires installed Codex; explicitly run for native configuration acceptance"]
    fn codex_native_catalog_fixture() {
        let root = root(); let cwd = root.join("project"); std::fs::create_dir_all(&cwd).unwrap();
        let config = "[mcp_servers.unselected]\ncommand=\"velella-fixture-never-execute\"\n";
        std::fs::write(root.join("config.toml"),config).unwrap();
        let mut desc = crate::host::Descriptor { id:"fixture".into(), endpoint:"fixture".into(), directory:root.clone(), spawn:crate::sessions::SpawnRequest {
            sandbox:Default::default(),program:"codex".into(),args:vec![],cwd:Some(cwd.to_string_lossy().into_owned()),
            env:std::collections::HashMap::from([("CODEX_HOME".into(),root.to_string_lossy().into_owned()),("VELELLA_PROVIDER".into(),"codex".into())]),adapter:None }};
        let selected = json!({"velella":{"command":"velella-fixture-selected","args":["control"]}});
        configure_codex(&mut desc,&selected).unwrap(); verify(&desc).unwrap();
        let actual = codex_servers(&desc).unwrap();
        assert!(actual.iter().any(|server| server["name"] == "unselected" && server["enabled"] == false));
        assert_eq!(std::fs::read_to_string(root.join("config.toml")).unwrap(),config);
        std::fs::write(root.join("config.toml"),format!("{config}[mcp_servers.new_after_capture]\ncommand=\"velella-fixture-never-execute\"\n")).unwrap();
        assert!(verify(&desc).unwrap_err().to_string().contains("unselected"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn kimi_checks_nearest_git_root_from_subdirectory_and_again_on_resume() {
        let root = root(); let cwd = root.join("src/nested");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(root.join(".git"), "gitdir: fixture").unwrap();
        check_kimi_project(&cwd).unwrap();
        let desc = crate::host::Descriptor { id:"fixture".into(), endpoint:"fixture".into(), directory:root.clone(), spawn:crate::sessions::SpawnRequest {
            sandbox:Default::default(),program:"kimi".into(),args:vec![],cwd:Some(cwd.to_string_lossy().into_owned()),
            env:std::collections::HashMap::from([("VELELLA_PROVIDER".into(),"kimi".into())]),adapter:None }};
        verify(&desc).unwrap();
        std::fs::write(root.join(".mcp.json"), r#"{"mcpServers":{"unselected":{"command":"fixture"}}}"#).unwrap();
        assert!(check_kimi_project(&cwd).is_err()); assert!(verify(&desc).is_err());
        std::fs::write(root.join(".mcp.json"), r#"{"mcpServers":false}"#).unwrap();
        assert!(check_kimi_project(&cwd).is_err());
        std::fs::write(root.join(".mcp.json"), " \n").unwrap(); check_kimi_project(&cwd).unwrap();
        std::fs::create_dir_all(cwd.join(".kimi-code")).unwrap();
        std::fs::write(cwd.join(".kimi-code/mcp.json"), r#"{"mcpServers":{"another":{"command":"fixture"}}}"#).unwrap();
        assert!(check_kimi_project(&cwd).is_err()); std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn gemini_rejects_policy_conflicts_and_binds_absent_files() {
        let root = root(); let path = root.join("settings.json"); let names = vec!["velella".into(), "docs".into()];
        let absent = capture_gemini_policies(&[path.clone()], &names).unwrap();
        assert!(absent[0]["sha256"].is_null());
        let mut desc = crate::host::Descriptor { id:"fixture".into(), endpoint:"fixture".into(), directory:root.clone(), spawn:crate::sessions::SpawnRequest {
            sandbox:Default::default(),program:"gemini".into(),args:vec![],cwd:Some(root.to_string_lossy().into_owned()),
            env:std::collections::HashMap::from([("VELELLA_NATIVE_POLICY_BINDINGS".into(),serde_json::to_string(&absent).unwrap())]),adapter:None }};
        verify(&desc).unwrap();
        std::fs::write(&path,r#"{"mcp":{"allowed":["velella","docs"]}}"#).unwrap();
        assert!(verify(&desc).is_err());
        let captured = capture_gemini_policies(&[path.clone()], &names).unwrap();
        desc.spawn.env.insert("VELELLA_NATIVE_POLICY_BINDINGS".into(),serde_json::to_string(&captured).unwrap()); verify(&desc).unwrap();
        for policy in [json!({"mcp":{"allowed":[]}}), json!({"mcp":{"allowed":["velella"]}}), json!({"mcp":{"excluded":["DOCS"]}}), json!({"mcp":{"allowed":false}})] {
            std::fs::write(&path,policy.to_string()).unwrap();
            assert!(capture_gemini_policies(&[path.clone()], &names).is_err()); assert!(verify(&desc).is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn gemini_policy_jsonc_preserves_strings_and_fingerprints_raw_source() {
        let root = root(); let path = root.join("settings.json");
        let source = br#"{ // selected servers
            "mcp": {"allowed": ["velella", /* selection */ "docs"]},
            "url": "https://example.test/a/*literal*/",
            "quoted": "a\"//still a string",
            "backslash": "\\"
        }"#;
        std::fs::write(&path,source).unwrap();
        let parsed = gemini_settings(&path).unwrap();
        assert_eq!(parsed["url"],"https://example.test/a/*literal*/");
        assert_eq!(parsed["quoted"],"a\"//still a string");
        assert_eq!(parsed["backslash"],"\\");
        let bindings = capture_gemini_policies(&[path.clone()], &["velella".into(),"docs".into()]).unwrap();
        assert_eq!(bindings[0]["sha256"],json!(format!("{:x}",Sha256::digest(source))));
        assert!(parse_gemini_settings(br#"{"mcp": {},}"#,&path).is_err());
        assert!(parse_gemini_settings(br#"{} /* unterminated"#,&path).is_err());
        assert!(parse_gemini_settings(br#"{"mcp": // interrupted"#,&path).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
