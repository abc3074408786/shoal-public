//! Fixed, per-invocation defaults for ordinary interactive terminals only.
//! These are native CLI policies, not an application security boundary.
use anyhow::{Context, Result};
use serde_json::Value;
use std::{collections::HashMap, ffi::OsStr};

pub const MODE_ENV: &str = "VELELLA_TERMINAL_PERMISSION_MODE";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan { pub args: Vec<String>, pub env: HashMap<String, String> }

pub fn requested(req: &Value) -> Result<&str> {
    req.get("permission_mode").map(|v| v.as_str().context("permission_mode must be a string"))
        .unwrap_or(Ok("inherit"))
}

pub fn saved(env: &HashMap<String, String>) -> &str {
    env.get(MODE_ENV).map(String::as_str).unwrap_or("inherit")
}

pub fn program(agent: &str) -> Result<&'static str> {
    Ok(match agent { "codex"=>"codex", "claude-code"=>"claude", "gemini"=>"gemini",
        "opencode"=>"opencode", "omp"=>"omp", "kimi"=>"kimi", "zcode"=>"zcode-free", "minimax"=>"mcode",
        _=>anyhow::bail!("此终端不支持默认权限设置") })
}

pub fn agent(program: &str) -> Result<&'static str> {
    Ok(match program { "codex"=>"codex", "claude"=>"claude-code", "gemini"=>"gemini",
        "opencode"=>"opencode", "omp"=>"omp", "kimi"=>"kimi", "zcode-free"=>"zcode", "mcode"=>"minimax",
        _=>anyhow::bail!("saved permissions require a supported native CLI program") })
}

pub fn plan(agent: &str, mode: &str) -> Result<Plan> {
    if mode == "inherit" { return Ok(Plan::default()); }
    let args: &[&str] = match (agent, mode) {
        ("codex", "ask")=>&["-a","on-request","-s","read-only","-c","approvals_reviewer=user"],
        ("codex", "auto")=>&["--approve-for-me"],
        ("codex", "full-access")=>&["--dangerously-bypass-approvals-and-sandbox"],
        ("claude-code", "ask")=>&["--permission-mode","manual"],
        ("claude-code", "accept-edits")=>&["--permission-mode","acceptEdits"],
        ("claude-code", "auto")=>&["--permission-mode","auto"],
        ("claude-code", "full-access")=>&["--dangerously-skip-permissions"],
        ("gemini", "full-access")|("kimi", "yolo")=>&["--yolo"],
        ("opencode", "ask")=>&[],
        ("opencode", "auto")|("kimi", "auto")=>&["--auto"],
        ("omp", "always-ask-cached")=>&["--approval-mode","always-ask-cached"],
        ("omp", "always-ask")=>&["--approval-mode","always-ask"],
        ("omp", "write")=>&["--approval-mode","write"],
        ("omp", "yolo")=>&["--approval-mode","yolo"],
        ("zcode", "guarded")=>&["--mode","guarded"],
        ("zcode", "build")=>&["--mode","build"],
        ("zcode", "edit")=>&["--mode","edit"],
        ("zcode", "yolo")=>&["--mode","yolo"],
        _=>anyhow::bail!("Agent `{agent}` 不支持权限模式 `{mode}`"),
    };
    let mut env = HashMap::from([(MODE_ENV.into(), mode.into())]);
    if agent == "opencode" && mode == "ask" {
        env.insert("OPENCODE_PERMISSION".into(), r#"{"*":"ask"}"#.into());
    }
    Ok(Plan { args: args.iter().map(|s| (*s).into()).collect(), env })
}

pub fn check_capabilities(agent: &str, mode: &str, help: &str) -> Result<()> {
    let plan = plan(agent, mode)?;
    if mode == "inherit" { return Ok(()); }
    for flag in plan.args.iter().filter(|v| v.starts_with('-')) {
        anyhow::ensure!(crate::native_launch::declares_flag(help, flag),
            "当前 `{agent}` CLI 不支持权限参数 {flag}；请选择沿用 CLI 设置或升级该工具");
    }
    let value = match (agent, mode) {
        ("claude-code", "ask")=>Some("manual"), ("claude-code", "accept-edits")=>Some("acceptEdits"),
        ("claude-code", "auto")=>Some("auto"), ("omp", _) | ("zcode", _)=>Some(mode), _=>None,
    };
    if let Some(value) = value {
        anyhow::ensure!(help.split(|c: char| !c.is_alphanumeric() && c != '-').any(|v| v == value),
            "当前 `{agent}` CLI 未声明权限模式 `{value}`");
    }
    if agent == "opencode" && mode == "ask" {
        anyhow::ensure!(crate::native_launch::declares_flag(help, "--auto"), "当前 OpenCode 版本不支持已验证的权限配置协议");
    }
    Ok(())
}

pub fn validate(agent: &str, mode: &str, actual_program: &str, resume: bool, path: Option<&OsStr>) -> Result<()> {
    plan(agent, mode)?;
    if mode == "inherit" { return Ok(()); }
    anyhow::ensure!(actual_program == program(agent)?, "cannot apply permissions to a custom CLI program");
    let help = crate::native_launch::permission_probe(actual_program, resume, false, path)?;
    check_capabilities(agent, mode, &help)?;
    if agent == "opencode" && mode == "ask" {
        let version = crate::native_launch::permission_probe(actual_program, false, true, path)?;
        let parts: Vec<u32> = version.trim().split('.').map(str::parse).collect::<std::result::Result<_,_>>()
            .context("无法验证 OpenCode 权限配置版本")?;
        anyhow::ensure!(parts.len()==3 && parts.as_slice() >= [1,19,5].as_slice(), "OpenCode 默认询问需要 1.19.5 或更新版本");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_invalid_requests_never_escalate() {
        assert_eq!(requested(&serde_json::json!({})).unwrap(), "inherit");
        for value in [Value::Null, serde_json::json!({}), serde_json::json!(1)] {
            assert!(requested(&serde_json::json!({"permission_mode":value})).is_err());
        }
        for agent in ["codex","claude-code","gemini","opencode","omp","kimi","zcode","terminal"] {
            assert_eq!(plan(agent,"inherit").unwrap(),Plan::default());
            for mode in ["", "YOLO", "plan", "--yolo", "yolo;echo bad", "yolo\n"] {assert!(plan(agent,mode).is_err());}
        }
        assert!(plan("omp","auto").is_err());
        assert!(plan("codex","yolo").is_err());
    }
    #[test]
    fn permissions_are_fixed_invocation_local_fields() {
        assert_eq!(plan("codex","ask").unwrap().args,["-a","on-request","-s","read-only","-c","approvals_reviewer=user"]);
        assert_eq!(plan("omp","write").unwrap().args,["--approval-mode","write"]);
        assert_eq!(plan("zcode","guarded").unwrap().args,["--mode","guarded"]);
        let open=plan("opencode","ask").unwrap();
        assert!(open.args.is_empty());
        assert_eq!(open.env["OPENCODE_PERMISSION"],r#"{"*":"ask"}"#);
        assert_eq!(saved(&open.env),"ask");
        assert!(!plan("opencode","auto").unwrap().env.contains_key("OPENCODE_PERMISSION"));
    }
    #[test]
    fn capabilities_fail_closed() {
        assert!(check_capabilities("claude-code","ask","  --permission-mode <mode>  choices: default, auto").is_err());
        assert!(check_capabilities("claude-code","ask","  --permission-mode <mode>  choices: manual, auto").is_ok());
        assert!(check_capabilities("omp","write","  --help  See --approval-mode write").is_err());
        assert!(check_capabilities("omp","write","  --approval-mode=<value>  always-ask|always-ask-cached|write|yolo").is_ok());
        assert!(check_capabilities("zcode","plan","  --mode <mode>  plan").is_err());
        assert!(validate("codex","ask","custom-codex",false,None).is_err());
    }
}
