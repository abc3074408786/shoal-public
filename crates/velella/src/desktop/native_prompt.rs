//! Opt-in launch contract and observation hooks, never native policy overrides.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct InitialPrompt {
    pub text: String,
    pub mode: PromptMode,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(in crate::desktop) enum PromptMode { Draft, Submit }

pub(in crate::desktop) fn requested(req: &Value) -> Result<Option<InitialPrompt>> {
    req.get("initial_prompt").map(|value| {
        let prompt: InitialPrompt = serde_json::from_value(value.clone())?;
        anyhow::ensure!(!prompt.text.trim().is_empty() && prompt.text.len() <= 65536
            && prompt.text.chars().all(|c| !c.is_control() || matches!(c, '\n' | '\t')),
            "initial_prompt contains unsupported control characters or is too large");
        Ok(prompt)
    }).transpose()
}

pub(in crate::desktop) fn saved(desc: &Descriptor) -> Result<Option<InitialPrompt>> {
    desc.spawn.env.get(host::INITIAL_PROMPT_ENV)
        .map(|value| serde_json::from_str(value).context("invalid saved initial_prompt")).transpose()
}

pub(in crate::desktop) fn validate_feedback(desc: &Descriptor, text: &str, draft: bool) -> Result<()> {
    let prompt = saved(desc)?.context("session has no initial_prompt contract")?;
    anyhow::ensure!(prompt.text == text && (prompt.mode == PromptMode::Draft) == draft,
        "initial feedback does not match the launch text or mode");
    anyhow::ensure!(!desc.spawn.sandbox.is_sandboxed(), "initial_prompt requires native execution");
    anyhow::ensure!(!desc.directory.join(host::INITIAL_PROMPT_CLOSED).exists(),
        "recovered initial prompts require manual handling and are never resent");
    Ok(())
}

// Called after activity/role configuration, so it preserves their settings and
// handlers. The command has no interpolated paths or user text; PATH is already
// bound to the runtime by activity::attach. It emits no permission decision.
pub(super) fn attach(desc: &mut Descriptor) -> Result<()> {
    anyhow::ensure!(!desc.spawn.sandbox.is_sandboxed() && matches!(desc.spawn.program.as_str(), "codex" | "claude"),
        "initial_prompt requires native Codex or Claude");
    desc.spawn.env.insert("VELELLA_SESSION_ENDPOINT".into(), desc.endpoint.clone());
    if desc.spawn.program == "claude" {
        let index = desc.spawn.args.iter().position(|arg| arg == "--settings");
        let mut settings: Value = match index {
            Some(i) => serde_json::from_slice(&std::fs::read(desc.spawn.args.get(i + 1).context("settings path")?)?)?,
            None => json!({}),
        };
        let object = settings.as_object_mut().context("settings object")?;
        let hooks = object.entry("hooks").or_insert_with(|| json!({})).as_object_mut().context("hook settings object")?;
        for (event, entries) in hook_settings().as_object().unwrap() {
            let current = hooks.entry(event.clone()).or_insert_with(|| json!([])).as_array_mut().context("hook array")?;
            // Existing collaboration hooks already report to this same endpoint.
            if !current.iter().any(has_status_hook) { current.extend(entries.as_array().unwrap().iter().cloned()); }
        }
        let path = desc.directory.join("initial-prompt-settings.json");
        ipc::atomic_json(&path, &settings)?;
        if let Some(i) = index { desc.spawn.args[i + 1] = path.to_string_lossy().into_owned(); }
        else { desc.spawn.args.extend(["--settings".into(), path.to_string_lossy().into_owned()]); }
    } else {
        for event in ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "PermissionRequest", "Stop", "Interrupt"] {
            let key = format!("hooks.{event}=");
            let hook = "{hooks=[{type=\"command\",command=\"velella-runtime hook\"}]}";
            if let Some(i) = desc.spawn.args.iter().position(|arg| arg.starts_with(&key)) {
                let parsed: toml::Value = toml::from_str(&desc.spawn.args[i])?;
                let entries = serde_json::to_value(&parsed["hooks"][event])?;
                if entries.as_array().context("inline hook array")?.iter().any(has_status_hook) { continue; }
                let old = desc.spawn.args[i].strip_suffix(']').context("inline hook array")?;
                desc.spawn.args[i] = format!("{old},{hook}]");
            } else { desc.spawn.args.extend(["-c".into(), format!("{key}[{hook}]")]); }
        }
    }
    Ok(())
}

fn has_status_hook(entry: &Value) -> bool {
    // A matcher or asynchronous observer is not equivalent to a synchronous,
    // unconditional host status hook (especially at permission boundaries).
    entry.get("matcher").is_none() && entry["hooks"].as_array().is_some_and(|hooks|
        hooks.iter().any(|hook| hook["type"] == "command" && hook["command"] == "velella-runtime hook" && hook["async"] != true))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initial_prompt_status_hooks_preserve_activity_policy_and_parameters() {
        let directory=std::env::temp_dir().join(format!("vl-initial-hooks-{}",uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut desc=Descriptor{id:"fixture".into(),endpoint:"fixture-endpoint".into(),directory:directory.clone(),spawn:SpawnRequest{sandbox:Default::default(),program:"claude".into(),args:vec![],cwd:Some(directory.to_string_lossy().into_owned()),env:HashMap::new(),adapter:Some("generic".into())}};
        let original=json!({"permissions":{"deny":["Bash(rm *)"]},"statusLine":{"type":"command","command":"existing-context"},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"existing-stop","timeout":17}]}]}});
        let path=directory.join("settings.json");ipc::atomic_json(&path,&original).unwrap();
        desc.spawn.args=vec!["--model".into(),"fixture-model".into(),"--settings".into(),path.to_string_lossy().into_owned(),"--mcp-config".into(),"unchanged-mcp.json".into()];
        crate::terminal_activity::attach(&mut desc,&std::env::current_exe().unwrap()).unwrap();
        attach(&mut desc).unwrap();attach(&mut desc).unwrap();
        let index=desc.spawn.args.iter().position(|arg|arg=="--settings").unwrap();
        let settings:Value=serde_json::from_slice(&std::fs::read(&desc.spawn.args[index+1]).unwrap()).unwrap();
        assert_eq!(settings["permissions"],original["permissions"]);assert_eq!(settings["statusLine"],original["statusLine"]);
        assert_eq!(settings["hooks"]["Stop"][0],original["hooks"]["Stop"][0]);
        assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(),3,"existing, activity and exactly one status hook");
        assert_eq!(&desc.spawn.args[..2],["--model","fixture-model"]);
        assert_eq!(&desc.spawn.args[index+2..],["--mcp-config","unchanged-mcp.json"]);
        assert_eq!(desc.spawn.env["VELELLA_SESSION_ENDPOINT"],desc.endpoint);
        assert!(!desc.spawn.env.contains_key("VELELLA_COLLABORATION_SKILL"));
        desc.spawn.program="codex".into();desc.spawn.args=vec!["--model".into(),"fixture-model".into(),"-c".into(),"hooks.Stop=[{hooks=[{type=\"command\",command=\"existing-stop\",timeout=17}]}]".into()];
        crate::terminal_activity::attach(&mut desc,&std::env::current_exe().unwrap()).unwrap();
        attach(&mut desc).unwrap();let once=desc.spawn.args.clone();attach(&mut desc).unwrap();assert_eq!(once,desc.spawn.args);
        let stop:toml::Value=toml::from_str(desc.spawn.args.iter().find(|arg|arg.starts_with("hooks.Stop=")).unwrap()).unwrap();
        assert_eq!(stop["hooks"]["Stop"].as_array().unwrap().len(),3);
        assert_eq!(stop["hooks"]["Stop"][0]["hooks"][0]["timeout"].as_integer(),Some(17));
        assert_eq!(stop["hooks"]["Stop"][1]["hooks"][0]["async"].as_bool(),Some(true));
        for name in ["SessionStart","UserPromptSubmit"] {assert!(desc.spawn.args.iter().any(|arg|arg.starts_with(&format!("hooks.{name}="))&&arg.contains("velella-runtime hook")));}
        assert!(!desc.spawn.args.iter().any(|arg|arg.starts_with("hooks.Notification=")));
        assert_eq!(desc.spawn.adapter.as_deref(),Some("generic"));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn initial_prompt_schema_is_narrow() {
        for value in [Value::Null, json!({}), json!({"text":"x","mode":"auto"}),
            json!({"text":7,"mode":"draft"}), json!({"text":"x","mode":false}),
            json!({"text":"x","mode":"draft","permission":"allow"}),
            json!({"text":"x\r","mode":"draft"}), json!({"text":"\u{1b}[31m","mode":"submit"}),
            json!({"text":"x".repeat(65537),"mode":"draft"})] {
            assert!(requested(&json!({"initial_prompt":value})).is_err());
        }
        assert!(requested(&json!({})).unwrap().is_none());
        assert!(requested(&json!({"initial_prompt":{"text":"x\n\t","mode":"submit"}})).unwrap().is_some());
    }
}
