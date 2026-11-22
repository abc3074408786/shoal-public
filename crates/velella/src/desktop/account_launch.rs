//! Account directory selection is invocation-local. No credentials are read,
//! copied, removed or returned by this module.
use super::*;

pub(super) fn home(req:&Value,agent:&str)->Result<Option<String>> {
    let Some(value)=req.get("account_home") else{return Ok(None)};
    anyhow::ensure!(matches!(agent,"codex"|"claude-code"),"account directories require Codex or Claude Code");
    let text=value.as_str().context("account_home must be an absolute directory string")?;
    anyhow::ensure!(!text.trim().is_empty()&&text.len()<=4096&&!text.chars().any(char::is_control),"invalid account directory");
    let path=Path::new(text);
    anyhow::ensure!(path.is_absolute()&&path.is_dir(),"account directory must be an existing absolute directory on the execution host");
    Ok(Some(dunce::canonicalize(path)?.to_str().context("account directory must be UTF-8")?.into()))
}

pub(super) struct Selection {pub home:Option<String>,pub login:bool}
impl Selection {
    pub fn requested(req:&Value,agent:&str,restricted:bool,collaboration:bool)->Result<Self>{
        let selected=Self{home:home(req,agent)?,login:if let Some(action)=req.get("account_action"){anyhow::ensure!(action=="login","unsupported account action");true}else{false}};
        if selected.home.is_some()||selected.login {
            anyhow::ensure!(matches!(agent,"codex"|"claude-code")&&!restricted&&!collaboration&&req["role_config"].is_null(),"account selection requires an ordinary native Codex or Claude terminal");
        }
        if selected.login {
            let options:velella_core::task::LaunchOptions=serde_json::from_value(req.get("launch_options").cloned().unwrap_or(json!({})))?;
            anyhow::ensure!(options.is_empty()&&crate::terminal_permissions::requested(req)?=="inherit"&&req.get("initial_prompt").is_none()&&req["track_submission"]!=true,"login terminals cannot request model, permission, prompt or submission overrides");
        }
        Ok(selected)
    }
    pub fn apply(&self,desc:&mut Descriptor,agent:&str){
        if let Some(home)=&self.home {
            desc.spawn.env.insert(if agent=="codex"{"CODEX_HOME"}else{"CLAUDE_CONFIG_DIR"}.into(),home.clone());
            desc.spawn.env.insert("VELELLA_ACCOUNT_HOME".into(),home.clone());
        }
        if self.login {desc.spawn.args=if agent=="codex"{vec!["login".into()]}else{vec!["auth".into(),"login".into()]};desc.spawn.env.insert("VELELLA_ACCOUNT_ACTION".into(),"login".into());}
    }
    pub fn ensure_saved(&self,desc:&Descriptor)->Result<()> {
        anyhow::ensure!(desc.spawn.env.get("VELELLA_ACCOUNT_HOME")==self.home.as_ref(),"创建请求已绑定另一个账号目录，请新建会话");
        anyhow::ensure!(desc.spawn.env.get("VELELLA_ACCOUNT_ACTION").map(String::as_str)==if self.login{Some("login")}else{None},"创建请求已绑定另一账号操作，请新建会话");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_selection_rejects_invalid_or_managed_requests(){
        for req in [json!({"account_home":null}),json!({"account_home":"relative"}),json!({"account_action":"logout"}),json!({"account_action":"login","permission_mode":"full-access"}),json!({"account_action":"login","initial_prompt":"x"})]{assert!(Selection::requested(&req,"codex",false,false).is_err());}
        assert!(Selection::requested(&json!({"account_action":"login"}),"codex",true,false).is_err());
        assert!(Selection::requested(&json!({"account_action":"login"}),"claude-code",false,true).is_err());
        assert!(Selection::requested(&json!({"account_action":"login"}),"omp",false,false).is_err());
        assert!(Selection::requested(&json!({}),"omp",false,false).is_ok());
    }
    #[test]
    fn profile_is_pinned_and_login_uses_fixed_argv(){
        let root=std::env::temp_dir().join(format!("velella-account-test-{}",uuid::Uuid::now_v7()));std::fs::create_dir(&root).unwrap();
        let req=json!({"account_home":root,"account_action":"login"});
        let selection=Selection::requested(&req,"codex",false,false).unwrap();
        let mut desc=Descriptor{id:"test".into(),endpoint:"unused".into(),directory:root.clone(),spawn:SpawnRequest{sandbox:Default::default(),program:"codex".into(),args:vec![],cwd:None,env:HashMap::new(),adapter:None}};
        selection.apply(&mut desc,"codex");assert_eq!(desc.spawn.args,vec!["login"]);assert_eq!(desc.spawn.env["CODEX_HOME"],selection.home.clone().unwrap());assert!(selection.ensure_saved(&desc).is_ok());
        assert!(Selection{home:None,login:true}.ensure_saved(&desc).is_err());assert!(Selection{home:selection.home.clone(),login:false}.ensure_saved(&desc).is_err());
        selection.apply(&mut desc,"claude-code");assert_eq!(desc.spawn.args,vec!["auth","login"]);assert!(desc.spawn.env.contains_key("CLAUDE_CONFIG_DIR"));
        std::fs::remove_dir(root).unwrap();
    }
}
