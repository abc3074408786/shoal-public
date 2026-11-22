//! Invocation-local tool discovery for the two desktop Agent launch paths.
use crate::{host::Descriptor, ipc};
use anyhow::{Context, Result};
use serde_json::json;
use std::path::{Path, PathBuf};

pub fn resources(runtime: &Path) -> PathBuf {
    runtime.parent().unwrap_or(Path::new(".")).join("browser-agent/server.mjs")
}

pub fn inject(desc: &mut Descriptor, runtime: &Path, data: &Path, workspace: &str) -> Result<()> {
    // Orchestrated roles have an explicit MCP catalog. This also covers recovery.
    if desc.spawn.env.contains_key("VELELLA_ROLE_CONFIG_FINGERPRINT") {
        anyhow::ensure!(!desc.spawn.env.contains_key("VELELLA_BROWSER_WORKSPACE"), "role session has a legacy browser injection; create a new session");
        return Ok(());
    }
    if !matches!(desc.spawn.program.as_str(), "claude" | "codex") || !resources(runtime).is_file() {
        return Ok(());
    }
    if let Some(bound) = desc.spawn.env.get("VELELLA_BROWSER_WORKSPACE") {
        anyhow::ensure!(bound == workspace, "browser tool binding belongs to another workspace");
        return Ok(());
    }
    let args = json!(["browser-mcp", data, workspace]);
    if desc.spawn.program == "claude" {
        let config = desc.directory.join("browser-mcp.json");
        ipc::atomic_json(&config, &json!({"mcpServers":{"velella_browser":{"type":"stdio","command":runtime,"args":args}}}))?;
        desc.spawn.args.extend(["--mcp-config".into(), config.to_string_lossy().into_owned()]);
    } else {
        // JSON strings/arrays are also valid TOML strings/arrays. No shell quoting.
        let config = format!("mcp_servers.velella_browser={{command={},args={},startup_timeout_sec=30,tool_timeout_sec=60}}", json!(runtime), args);
        desc.spawn.args.extend(["-c".into(), config]);
    }
    desc.spawn.env.insert("VELELLA_BROWSER_WORKSPACE".into(), workspace.into());
    Ok(())
}

pub fn serve(data: &Path, workspace: &str) -> Result<()> {
    let runtime = std::env::current_exe()?;
    let script = resources(&runtime);
    anyhow::ensure!(script.is_file(), "browser_agent_missing: browser-agent/server.mjs must be shipped beside the runtime");
    let status = std::process::Command::new("node")
        .arg(script).arg(&runtime).arg(data).arg(workspace)
        .env("CHROME_DEVTOOLS_MCP_NO_USAGE_STATISTICS", "1")
        .env("CHROME_DEVTOOLS_MCP_NO_UPDATE_CHECKS", "1")
        .status().context("browser_agent_node_unavailable: install a supported Node.js on PATH")?;
    anyhow::ensure!(status.success(), "browser_agent_failed: {status}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::SpawnRequest;
    use std::collections::HashMap;
    #[test]
    fn orchestrated_role_does_not_gain_an_unselected_browser_server() {
        let root=std::env::temp_dir().join(format!("velella-role-browser-{}",uuid::Uuid::now_v7()));std::fs::create_dir_all(root.join("browser-agent")).unwrap();std::fs::write(root.join("browser-agent/server.mjs"),"").unwrap();
        for program in ["codex","claude"] {
            let mut desc=Descriptor{id:"s".into(),endpoint:"e".into(),directory:root.clone(),spawn:SpawnRequest{sandbox:Default::default(),program:program.into(),args:vec![],cwd:None,env:HashMap::from([("VELELLA_ROLE_CONFIG_FINGERPRINT".into(),"fixture".into())]),adapter:None}};
            inject(&mut desc,&root.join("runtime.exe"),&root,"w").unwrap();assert!(desc.spawn.args.is_empty());assert!(!desc.spawn.env.contains_key("VELELLA_BROWSER_WORKSPACE"));
            desc.spawn.env.insert("VELELLA_BROWSER_WORKSPACE".into(),"w".into());assert!(inject(&mut desc,&root.join("runtime.exe"),&root,"w").is_err());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invocation_scope_resume_idempotence_and_shell_exclusion() {
        let root=std::env::temp_dir().join(format!("velella-agent-{}",uuid::Uuid::now_v7()));
        std::fs::create_dir_all(root.join("browser-agent")).unwrap();
        std::fs::write(root.join("browser-agent/server.mjs"), "").unwrap();
        let runtime=root.join("runtime.exe");
        for program in ["claude", "codex", "powershell"] {
            let mut d=Descriptor{id:"s".into(),endpoint:"e".into(),directory:root.clone(),spawn:SpawnRequest{ sandbox: Default::default(),program:program.into(),args:vec!["resume".into(),"native-session".into()],cwd:None,env:HashMap::new(),adapter:None}};
            inject(&mut d,&runtime,&root,"workspace-a").unwrap();
            let once=d.spawn.args.clone();
            inject(&mut d,&runtime,&root,"workspace-a").unwrap();
            assert_eq!(d.spawn.args,once);
            assert_eq!(&d.spawn.args[..2], &["resume","native-session"]);
            if program=="powershell" { assert_eq!(once.len(),2); }
            else {
                assert_eq!(once.len(),4);
                assert!(inject(&mut d,&runtime,&root,"workspace-b").is_err());
                assert!(!once.iter().any(|a|a.contains("bypassPermissions")||a.contains("dangerously")));
            }
        }
        let config:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("browser-mcp.json")).unwrap()).unwrap();
        assert_eq!(config["mcpServers"]["velella_browser"]["args"][2],"workspace-a");
        std::fs::remove_dir_all(root).unwrap();
    }
}
