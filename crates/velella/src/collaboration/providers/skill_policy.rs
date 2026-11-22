//! Selected skill snapshots are supplied as role context on providers that can
//! disable all native discovery. This is not a filesystem security sandbox.
use super::*;
use std::collections::HashMap;

pub(super) const POLICY: &str = "selected_snapshot_context_v1";
const LIMIT: usize = 1024 * 1024;

pub(super) fn instructions(role: &RoleDefinition, bindings: &[Value]) -> Result<String> {
    let mut text = format!("{}\n不派生任何子 Agent。只通过 Velella 的持久任务协议领取及回报。不得自行接受或派发任务。完成回报后停止。\n选定技能的固定快照（含引用资源；完整阅读SKILL.md并按其相对路径使用资源）：{}",
        role.instruction, bindings.iter().filter_map(|s|s["path"].as_str()).collect::<Vec<_>>().join("\n"));
    if matches!(role.provider.as_str(), "codex" | "claude-code" | "gemini" | "opencode" | "omp") {
        text.push_str("\n本角色关闭原生自动技能目录和技能命令。以下为选定技能的完整内容，适用时按技能执行；相对资源路径以每项注明的快照目录为基准。\n");
        for binding in bindings {
            let path = Path::new(binding["path"].as_str().context("skill snapshot path missing")?);
            let body = std::fs::read_to_string(path).context("selected SKILL.md must contain UTF-8 text")?;
            text.push_str(&format!("\n--- 选定技能：{}；资源目录：{} ---\n{}\n--- 技能结束 ---\n", path.display(), path.parent().context("skill root missing")?.display(), body));
            anyhow::ensure!(text.len() <= LIMIT, "选定技能正文合计超过 1 MiB，请减少本角色技能数量；未截断或启动角色");
        }
    }
    anyhow::ensure!(text.len() <= LIMIT, "role instructions exceed 1 MiB");
    Ok(text)
}

pub(super) fn configure_gemini(config: &mut Value) -> Result<()> {
    if config["skills"].is_null() { config["skills"] = json!({}); }
    config["skills"].as_object_mut().context("Gemini skills settings must be an object")?.insert("enabled".into(), json!(false));
    if config["tools"].is_null() { config["tools"] = json!({}); }
    let tools = config["tools"].as_object_mut().context("Gemini tools settings must be an object")?;
    let excludes = tools.entry("exclude").or_insert(json!([])).as_array_mut().context("Gemini tools.exclude must be an array")?;
    if !excludes.iter().any(|value| value == "activate_skill") { excludes.push(json!("activate_skill")); }
    Ok(())
}

pub(super) fn receipt(provider: &str) -> &'static str {
    match provider {
        "claude-code" | "gemini" | "omp" => "selected_snapshot_context; native_skill_discovery_disabled; authenticated_workflow_unverified",
        "opencode" => "selected_snapshot_context; native_skill_access_denied; authenticated_workflow_unverified",
        "codex" => "selected_snapshot_context; native_catalog_disabled_at_launch_and_recovery; running_discovery_changes_unverified; authenticated_workflow_unverified",
        "kimi" => "selected_directory; builtin_and_plugin_catalog_unverified",
        _ => "instruction_only",
    }
}

fn verified_instruction(env: &HashMap<String,String>) -> Result<String> {
    anyhow::ensure!(env.get("VELELLA_SKILL_POLICY").map(String::as_str) == Some(POLICY), "legacy role lacks selected skill context policy; create a new session");
    verify_snapshots(env)?;
    let path = Path::new(env.get("VELELLA_ROLE_FILE").context("role instruction file missing")?);
    // Read once, then hash the same bytes emitted to the model.
    let meta = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= LIMIT as u64, "role instruction file must be a bounded ordinary file");
    let mut bytes = Vec::new(); std::fs::File::open(path)?.take(LIMIT as u64 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= LIMIT && env.get("VELELLA_ROLE_INSTRUCTION_SHA256") == Some(&format!("{:x}",Sha256::digest(&bytes))), "role skill instructions changed; create a new session");
    String::from_utf8(bytes).context("role instructions are not UTF-8")
}

fn verify_snapshots(env: &HashMap<String,String>) -> Result<()> {
    let Some(bindings) = env.get("VELELLA_SKILL_BINDINGS") else {
        anyhow::ensure!(!env.contains_key("VELELLA_ROLE_CONFIG_FINGERPRINT") && !env.contains_key("VELELLA_SKILL_POLICY"), "legacy role lacks skill resource bindings; create a new session");
        return Ok(());
    };
    for binding in serde_json::from_str::<Vec<Value>>(bindings)? {
        let path = Path::new(binding["path"].as_str().context("skill snapshot path missing")?);
        anyhow::ensure!(Some(skill_digest(path)?.as_str()) == binding["digest"].as_str(), "selected skill resources changed; create a new session");
    }
    Ok(())
}

pub(super) fn verify(desc: &crate::host::Descriptor) -> Result<()> {
    verify_snapshots(&desc.spawn.env)?;
    match desc.spawn.env.get("VELELLA_PROVIDER").map(String::as_str) {
        Some("codex") => {
            let instruction=verified_instruction(&desc.spawn.env)?;
            let expected=format!("developer_instructions={}",to_toml(&json!(instruction))?);
            anyhow::ensure!(desc.spawn.args.windows(2).filter(|pair|matches!(pair[0].as_str(),"-c"|"--config")&&pair[1].starts_with("developer_instructions=")).last().is_some_and(|pair|pair[1]==expected),"Codex role context override changed");
            codex_skills::verify(desc)?;
        }
        Some("claude-code") => {
            verified_instruction(&desc.spawn.env)?;
            anyhow::ensure!(desc.spawn.args.iter().any(|arg| arg == "--disable-slash-commands") &&
                desc.spawn.args.windows(2).any(|pair| pair[0] == "--append-system-prompt-file" && Some(&pair[1]) == desc.spawn.env.get("VELELLA_ROLE_FILE")),
                "Claude role is missing its selected skill configuration");
        }
        Some("gemini") => {
            verified_instruction(&desc.spawn.env)?;
            let path = desc.spawn.env.get("GEMINI_CLI_SYSTEM_SETTINGS_PATH").context("Gemini role settings missing")?;
            let config = native_policy::gemini_settings(Path::new(path))?;
            anyhow::ensure!(config["skills"]["enabled"] == false && config["tools"]["exclude"].as_array().is_some_and(|items| items.iter().any(|item| item == "activate_skill")), "Gemini role no longer disables native skill discovery");
        }
        Some("opencode") => {
            verified_instruction(&desc.spawn.env)?;
            opencode_policy::verify(desc)?;
        }
        Some("omp") => {
            verified_instruction(&desc.spawn.env)?;
            omp_policy::verify(desc)?;
        }
        _ => {},
    }
    Ok(())
}

pub(super) fn hook_output(event: &str, env: &HashMap<String,String>) -> Result<Value> {
    if event == "UserPromptSubmit" && env.get("VELELLA_PROVIDER").map(String::as_str) == Some("omp") { verified_instruction(env)?; }
    // Gemini maps BeforeAgent to our UserPromptSubmit lifecycle event. Inject on
    // every submitted task: its /clear path discards SessionStart hook context.
    if event != "UserPromptSubmit" || env.get("VELELLA_PROVIDER").map(String::as_str) != Some("gemini") { return Ok(json!({})); }
    // Native getAdditionalContext HTML-escapes angle brackets. JSON Unicode
    // escapes preserve the exact string through that sanitizer, including code.
    let context = serde_json::to_string(&json!({"velella_role_context":verified_instruction(env)?}))?
        .replace('<', "\\u003c").replace('>', "\\u003e");
    Ok(json!({"hookSpecificOutput":{"hookEventName":"BeforeAgent","additionalContext":context}}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_context_preserves_selected_body_and_resource_base_without_source_drift() {
        let root = std::env::temp_dir().join(format!("velella-skill-context-{}",uuid::Uuid::now_v7()));
        let source = root.join("source"); std::fs::create_dir_all(source.join("references")).unwrap();
        std::fs::write(source.join("SKILL.md"), "---\nname: selected\n---\nRead references/rules.txt").unwrap();
        std::fs::write(source.join("references/rules.txt"), "fixed rules").unwrap();
        let mut role: RoleDefinition = serde_json::from_value(super::super::super::orchestration::v2::seed("gemini","goal")["roles"][0].clone()).unwrap();
        role.skills.push(super::super::super::orchestration::v2::Skill {path:source.join("SKILL.md").to_string_lossy().into_owned(),digest:skill_digest(&source).unwrap()});
        let bindings = freeze_skills(&root.join("role"),&role).unwrap();
        std::fs::write(source.join("SKILL.md"), "unselected changed source").unwrap();
        let text = instructions(&role,&bindings).unwrap();
        assert!(text.contains("Read references/rules.txt") && text.contains("role") && text.contains("skill-0"));
        assert!(!text.contains("unselected changed source"));
        let env = HashMap::from([("VELELLA_SKILL_BINDINGS".into(),serde_json::to_string(&bindings).unwrap())]);
        verify_snapshots(&env).unwrap();
        let frozen = Path::new(bindings[0]["path"].as_str().unwrap()).parent().unwrap();
        std::fs::write(frozen.join("references/rules.txt"), "changed snapshot resource").unwrap();
        assert!(verify_snapshots(&env).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn gemini_skill_settings_preserve_other_restrictions_and_reject_invalid_shapes() {
        let mut config = json!({"skills":{"disabled":["blocked"]},"tools":{"exclude":["run_shell_command"]}});
        configure_gemini(&mut config).unwrap(); configure_gemini(&mut config).unwrap();
        assert_eq!(config["skills"]["enabled"],false);
        assert_eq!(config["skills"]["disabled"],json!(["blocked"]));
        assert_eq!(config["tools"]["exclude"],json!(["run_shell_command","activate_skill"]));
        assert!(configure_gemini(&mut json!({"tools":false})).is_err());
        assert!(configure_gemini(&mut json!({"skills":false})).is_err());
    }
    #[test]
    fn gemini_hook_supplies_each_task_context_and_rejects_tampering() {
        let root = std::env::temp_dir().join(format!("velella-skill-hook-{}",uuid::Uuid::now_v7())); std::fs::create_dir_all(&root).unwrap();
        let path = root.join("instructions.md"); std::fs::write(&path,"selected context").unwrap();
        let env = HashMap::from([("VELELLA_PROVIDER".into(),"gemini".into()),("VELELLA_SKILL_POLICY".into(),POLICY.into()),("VELELLA_SKILL_BINDINGS".into(),"[]".into()),("VELELLA_ROLE_FILE".into(),path.to_string_lossy().into_owned()),("VELELLA_ROLE_INSTRUCTION_SHA256".into(),launch_file_digest(&path).unwrap())]);
        for _ in 0..2 { let output = hook_output("UserPromptSubmit",&env).unwrap(); let context: Value = serde_json::from_str(output["hookSpecificOutput"]["additionalContext"].as_str().unwrap()).unwrap(); assert_eq!(context["velella_role_context"],"selected context"); }
        assert_eq!(hook_output("UserPromptSubmit",&env).unwrap()["hookSpecificOutput"]["hookEventName"],"BeforeAgent");
        assert_eq!(hook_output("SessionStart",&env).unwrap(),json!({}));
        assert_eq!(hook_output("Stop",&env).unwrap(),json!({}));
        std::fs::write(path,"changed context").unwrap(); assert!(hook_output("UserPromptSubmit",&env).is_err());
        let mut legacy = env; legacy.remove("VELELLA_SKILL_POLICY"); assert!(hook_output("UserPromptSubmit",&legacy).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn opencode_requires_a_private_catalog_and_denies_native_skills() {
        let root = std::env::temp_dir().join(format!("velella-opencode-policy-{}",uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let instructions = root.join("instructions.md"); std::fs::write(&instructions,"selected context").unwrap();
        let env = HashMap::from([
            ("VELELLA_PROVIDER".into(),"opencode".into()),("VELELLA_SKILL_POLICY".into(),POLICY.into()),("VELELLA_SKILL_BINDINGS".into(),"[]".into()),
            ("VELELLA_ROLE_FILE".into(),instructions.to_string_lossy().into_owned()),("VELELLA_ROLE_INSTRUCTION_SHA256".into(),launch_file_digest(&instructions).unwrap()),
        ]);
        let mut desc = crate::host::Descriptor {id:"fixture".into(),endpoint:"fixture".into(),directory:root.clone(),spawn:crate::sessions::SpawnRequest {sandbox:Default::default(),program:"opencode".into(),args:vec![],cwd:None,env:env.clone(),adapter:None}};
        let folder=root.join("role"); std::fs::create_dir_all(&folder).unwrap();
        let role:RoleDefinition=serde_json::from_value(super::super::super::orchestration::v2::seed("opencode","goal")["roles"][0].clone()).unwrap();
        opencode_policy::configure(&mut desc,&folder,&role,"selected context",&json!({}),&root.join("absent-auth.json")).unwrap();
        verify(&desc).unwrap();
        for (key,value) in [("OPENCODE_PURE","1"),("OPENCODE_CONFIG_DIR","outside"),("XDG_STATE_HOME","outside"),("OPENCODE_TEST_MANAGED_CONFIG_DIR","outside"),("OPENCODE_AUTH_CONTENT","{}")]{
            let mut changed=desc.clone();changed.spawn.env.insert(key.into(),value.into());assert!(verify(&changed).is_err(),"{key}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
