//! OpenCode 1.18 role-local config and lifecycle. No account writes go back to the host.
use super::*;

const FLAGS: &[(&str, &str)] = &[
    ("OPENCODE_DISABLE_PROJECT_CONFIG", "1"),
    ("OPENCODE_DISABLE_EXTERNAL_SKILLS", "1"),
    // PURE also disables our explicit lifecycle plugin. Built-in authentication
    // plugins are part of the executable and are needed for existing OAuth logins.
    ("OPENCODE_PURE", "0"), ("OPENCODE_DISABLE_DEFAULT_PLUGINS", "0"),
    ("OPENCODE_DISABLE_AUTOUPDATE", "1"),
    ("OPENCODE_CONFIG_CONTENT", "{}"), ("OPENCODE_AUTH_CONTENT", ""),
    ("OPENCODE_PERMISSION", "{}"), ("OPENCODE_PLUGIN_META_FILE", ""),
    ("OPENCODE_WORKSPACE_ID", ""),
];
const ROOTS: &[(&str, &str)] = &[
    ("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"),
    ("XDG_CACHE_HOME", "cache"), ("XDG_STATE_HOME", "state"),
    ("OPENCODE_TEST_HOME", "home"), ("OPENCODE_TEST_MANAGED_CONFIG_DIR", "managed"),
    ("OPENCODE_CONFIG_DIR", "config/opencode"),
    ("OPENCODE_DB", "data/opencode/opencode.db"),
    ("OPENCODE_TUI_CONFIG", "config/opencode/tui.json"),
];

pub(super) fn auth_source() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(||
        PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default()).join(".local/share"))
        .join("opencode/auth.json")
}

fn capture_auth(source: &Path, target: &Path) -> Result<()> {
    let value = if source.try_exists()? {
        let meta = std::fs::symlink_metadata(source)?;
        anyhow::ensure!(meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 1024*1024, "OpenCode credentials must be a bounded ordinary file");
        serde_json::from_slice::<Value>(&std::fs::read(source)?).context("OpenCode credentials are not valid JSON")?
    } else { json!({}) };
    let mut selected = serde_json::Map::new();
    for (id, item) in value.as_object().context("OpenCode credentials must be an object")? {
        // wellknown records fetch remote configuration before local configuration.
        if matches!(item["type"].as_str(), Some("api" | "oauth")) { selected.insert(id.clone(),item.clone()); }
    }
    std::fs::create_dir_all(target.parent().context("auth parent missing")?)?;
    crate::ipc::atomic_json(target, &Value::Object(selected))
}

pub(super) fn configure(desc: &mut crate::host::Descriptor, folder: &Path, role: &RoleDefinition, instruction: &str, servers: &Value, auth: &Path) -> Result<()> {
    let private = folder.join("opencode-home");
    if !private.try_exists()? {
        private_directory(&private)?;
        capture_auth(auth, &private.join("data/opencode/auth.json"))?;
        crate::ipc::atomic_json(&private.join("velella-binding.json"), &json!({"fingerprint":role_fingerprint(role)?,"state":"ready"}))?;
    } else {
        let binding = json_file(&private.join("velella-binding.json"))?;
        anyhow::ensure!(binding["fingerprint"] == role_fingerprint(role)? && binding["state"] == "ready", "OpenCode private capture is incomplete or belongs to another role; create a new session");
    }
    for (key, relative) in ROOTS {
        let path = private.join(relative);
        if matches!(*key,"OPENCODE_DB"|"OPENCODE_TUI_CONFIG") {std::fs::create_dir_all(path.parent().context("private file parent missing")?)?;}else{std::fs::create_dir_all(&path)?;}
        desc.spawn.env.insert((*key).into(),path.to_string_lossy().into_owned());
    }
    for (key,value) in FLAGS { desc.spawn.env.insert((*key).into(),(*value).into()); }
    let mut permission = json!({});
    if !role.tools.is_empty() {
        permission["*"] = json!("deny");
        for tool in &role.tools { permission[tool] = json!("ask"); }
        for name in servers.as_object().context("MCP map missing")?.keys() { permission[format!("{name}_*")] = json!("ask"); }
    }
    // Apply these last so saved plans selecting the old skill checkbox cannot
    // re-enable native discovery or delegate outside the durable workflow.
    permission["task"] = json!("deny"); permission["skill"] = json!({"*":"deny"});
    let plugin = folder.join("lifecycle.mjs");
    std::fs::write(&plugin, include_str!("opencode_lifecycle.mjs"))?;
    let url = reqwest::Url::from_file_path(&plugin).map_err(|_| anyhow::anyhow!("OpenCode plugin path must be absolute"))?.to_string();
    let mcp = servers.as_object().unwrap().iter().map(|(name,server)| {
        let mut command = vec![server["command"].clone()]; command.extend(server["args"].as_array().into_iter().flatten().cloned());
        (name.clone(), json!({"type":"local","command":command,"enabled":true}))
    }).collect::<serde_json::Map<_,_>>();
    // OpenCode otherwise writes this field at load time, invalidating our
    // immutable launch-file digest before the first recovery.
    let config = json!({"$schema":"https://opencode.ai/config.json","plugin":[url],"mcp":mcp,"autoupdate":false,"share":"disabled",
        "agent":{"velella":{"mode":"primary","prompt":instruction,"permission":permission}},"default_agent":"velella"});
    let path = folder.join("opencode.json"); crate::ipc::atomic_json(&path,&config)?;
    desc.spawn.env.insert("OPENCODE_CONFIG".into(),path.to_string_lossy().into_owned());
    desc.spawn.adapter = Some("generic".into());
    Ok(())
}

pub(super) fn verify(desc: &crate::host::Descriptor) -> Result<()> {
    for (key,value) in FLAGS { anyhow::ensure!(desc.spawn.env.get(*key).map(String::as_str) == Some(*value), "OpenCode isolated flag changed: {key}"); }
    let private = desc.directory.join("role/opencode-home");
    for (key,relative) in ROOTS { anyhow::ensure!(desc.spawn.env.get(*key).map(PathBuf::from) == Some(private.join(relative)), "OpenCode isolated directory changed: {key}"); }
    let path = desc.directory.join("role/opencode.json");
    anyhow::ensure!(desc.spawn.env.get("OPENCODE_CONFIG").map(PathBuf::from) == Some(path.clone()), "OpenCode role config changed");
    let config = json_file(&path)?;
    let url = reqwest::Url::from_file_path(desc.directory.join("role/lifecycle.mjs")).map_err(|_| anyhow::anyhow!("plugin path must be absolute"))?.to_string();
    anyhow::ensure!(config["plugin"] == json!([url]) && config["default_agent"] == "velella" && config["agent"]["velella"]["permission"]["task"] == "deny" && config["agent"]["velella"]["permission"]["skill"]["*"] == "deny", "OpenCode role lost lifecycle or skill/delegation restrictions");
    Ok(())
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn private_auth_excludes_remote_config_and_preserves_source() {
        let root=std::env::temp_dir().join(format!("velella-auth-{}",uuid::Uuid::now_v7()));std::fs::create_dir_all(&root).unwrap();
        let source=root.join("source.json");let bytes=br#"{"openai":{"type":"oauth","access":"synthetic","refresh":"synthetic","expires":1},"docs":{"type":"wellknown","key":"ENV","token":"synthetic"}}"#;
        std::fs::write(&source,bytes).unwrap();let target=root.join("private/auth.json");capture_auth(&source,&target).unwrap();
        let result=json_file(&target).unwrap();assert!(result.get("docs").is_none());assert_eq!(result["openai"]["access"],"synthetic");assert_eq!(std::fs::read(source).unwrap(),bytes);std::fs::remove_dir_all(root).unwrap();
    }
}
