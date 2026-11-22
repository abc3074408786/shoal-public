//! Optional workbench services. Schedules share the existing task supervisor.
use super::*;
use rusqlite::OptionalExtension;
use crate::workbench::{Session, Workspace};
#[path = "execution_hosts.rs"]
mod execution_hosts;
#[path = "automation_management.rs"]
mod automation_management;

pub(super) struct Capabilities { db: Connection }
impl Capabilities {
    pub(super) fn hybrid_profile(&self,id:&str)->Result<Value>{
        let profile:SshProfile=serde_json::from_value(self.get("profiles",id)?)?;profile.validate()?;
        anyhow::ensure!(profile.local_port.is_none()&&profile.remote_port.is_none(),"混合执行请使用未配置额外端口转发的 SSH 配置");
        Ok(serde_json::to_value(profile)?)
    }
    pub(super) fn bound_hybrid_profile(&self,id:&str,fingerprint:&str)->Result<Value>{
        let profile=self.hybrid_profile(id)?;
        let actual=crate::hybrid::tools::digest(&serde_json::to_vec(&profile)?);
        anyhow::ensure!(actual==fingerprint,"SSH 配置已变化，请重新确认远程角色");
        Ok(profile)
    }
    pub fn open(root: &Path) -> Result<Self> {
        let existed=root.join("capabilities.db").try_exists()?;
        let db=Connection::open(root.join("capabilities.db"))?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        execution_hosts::migrate(&db,root,existed)?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS profiles(id TEXT PRIMARY KEY, document TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS automations(id TEXT PRIMARY KEY, document TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS runs(id TEXT PRIMARY KEY, automation_id TEXT NOT NULL, task_id TEXT NOT NULL UNIQUE, started_at INTEGER NOT NULL, state TEXT NOT NULL, error TEXT NOT NULL DEFAULT '');")?;
        let capabilities=Self{db};
        capabilities.migrate_automation_management()?;
        Ok(capabilities)
    }
    fn documents(&self,table:&str)->Result<Vec<Value>> {
        let mut stmt=self.db.prepare(&format!("SELECT document FROM {table} ORDER BY rowid"))?;
        let rows=stmt.query_map([],|r|r.get::<_,String>(0))?;
        rows.map(|row|Ok(serde_json::from_str(&row?)?)).collect()
    }
    fn put(&self,table:&str,value:&Value)->Result<()> {
        self.db.execute(&format!("INSERT INTO {table}(id,document) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET document=excluded.document"),params![value["id"].as_str().context("id required")?,serde_json::to_string(value)?])?;
        Ok(())
    }
    fn get(&self,table:&str,id:&str)->Result<Value> {
        let text:String=self.db.query_row(&format!("SELECT document FROM {table} WHERE id=?1"),[id],|r|r.get(0)).optional()?.context("record not found")?;
        Ok(serde_json::from_str(&text)?)
    }
    fn runs(&self)->Result<Vec<Value>> {
        let mut stmt=self.db.prepare("SELECT id,automation_id,task_id,started_at,state,error FROM runs ORDER BY started_at DESC,rowid DESC LIMIT 200")?;
        let rows=stmt.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"automation_id":r.get::<_,String>(1)?,"task_id":r.get::<_,String>(2)?,"started_at":r.get::<_,i64>(3)?,"state":r.get::<_,String>(4)?,"error":r.get::<_,String>(5)?})))?;
        Ok(rows.collect::<std::result::Result<Vec<_>,_>>()?)
    }
    // Reserve a task identity and advance the schedule atomically, before creating it.
    fn claim(&mut self,a:&Value,now:i64,scheduled:bool)->Result<(String,String)> {
        let run=uuid::Uuid::now_v7().to_string();let task=uuid::Uuid::now_v7().to_string();
        let tx=self.db.transaction()?;
        if scheduled {
            let mut next=a.clone();
            next["next_run"]=json!(advance_due(a["next_run"].as_i64().context("next run")?,a["interval_minutes"].as_i64().unwrap_or(0)*60,now));
            if a["interval_minutes"]==0 { next["enabled"]=json!(false); }
            tx.execute("UPDATE automations SET document=?1 WHERE id=?2",params![serde_json::to_string(&next)?,a["id"].as_str()])?;
        }
        tx.execute("INSERT INTO runs(id,automation_id,task_id,started_at,state) VALUES(?1,?2,?3,?4,'dispatching')",params![run,a["id"].as_str(),task,now])?;
        tx.commit()?;Ok((run,task))
    }
}

fn advance_due(due:i64,interval:i64,now:i64)->i64 {
    if interval<=0 {return due;}
    due+((now.saturating_sub(due).max(0)/interval)+1)*interval
}

#[derive(Serialize,Deserialize)]
struct SshProfile {
    #[serde(default)] id:String,
    name:String, host:String,
    #[serde(default)] user:String,
    #[serde(default)] port:Option<u16>,
    #[serde(default)] identity:String,
    #[serde(default)] directory:String,
    #[serde(default)] local_port:Option<u16>,
    #[serde(default)] remote_port:Option<u16>,
}
impl SshProfile {
    fn validate(&self)->Result<()> {
        anyhow::ensure!(!self.name.trim().is_empty()&&self.name.len()<=160,"主机名称不能为空（最多160字节）");
        anyhow::ensure!(!self.host.is_empty()&&self.host.len()<=253&&!self.host.starts_with('-')&&self.host.bytes().all(|c|c.is_ascii_alphanumeric()||b".-_:[]".contains(&c)),"主机或 SSH 别名格式不正确");
        anyhow::ensure!(self.user.len()<=128&&!self.user.starts_with('-')&&self.user.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c)),"用户名格式不正确");
        anyhow::ensure!(self.port!=Some(0)&&self.local_port!=Some(0)&&self.remote_port!=Some(0),"端口范围是1到65535");
        anyhow::ensure!(self.local_port.is_some()==self.remote_port.is_some(),"转发需要同时填写本地端口和远端端口");
        anyhow::ensure!(self.directory.len()<=1024&&!self.directory.chars().any(char::is_control),"远端目录包含无效字符");
        if !self.identity.is_empty(){anyhow::ensure!(Path::new(&self.identity).is_absolute()&&Path::new(&self.identity).is_file(),"密钥引用必须是已存在的本地绝对文件路径");}
        Ok(())
    }
    fn args(&self,diagnostic:bool)->Vec<String> {
        let mut args=vec![if diagnostic{"-G"}else{"-tt"}.into(),"-o".into(),"StrictHostKeyChecking=ask".into(),"-o".into(),"ServerAliveInterval=30".into(),"-o".into(),"ServerAliveCountMax=3".into(),"-o".into(),"ConnectTimeout=10".into(),"-o".into(),"ExitOnForwardFailure=yes".into()];
        if let Some(port)=self.port{args.extend(["-p".into(),port.to_string()]);}
        if !self.user.is_empty(){args.extend(["-l".into(),self.user.clone()]);}
        if !self.identity.is_empty(){args.extend(["-i".into(),self.identity.clone()]);}
        if let (Some(local),Some(remote))=(self.local_port,self.remote_port){args.extend(["-L".into(),format!("127.0.0.1:{local}:127.0.0.1:{remote}")]);}
        args.push(self.host.clone());
        if !diagnostic&&!self.directory.is_empty(){args.push(format!("cd -- {} && exec \"${{SHELL:-/bin/sh}}\" -l",shell_quote(&self.directory)));}
        args
    }
}
fn shell_quote(s:&str)->String {format!("'{}'",s.replace('\'',"'\"'\"'"))}

impl Service {
    pub(super) fn capability_command(&mut self,req:Value)->Result<Value> {
        let cmd=req["cmd"].as_str().unwrap_or("");
        if cmd.starts_with("execution_hosts.") { return self.capabilities.execution_host_command(&req); }
        if matches!(cmd,"automation.archived"|"automation.runs"|"automation.remove"|"automation.restore") { return self.automation_management_command(req); }
        match cmd {
            "ssh.list"=>Ok(json!({"ok":true,"profiles":self.capabilities.documents("profiles")?})),
            "ssh.hybrid.preview"=>{
                let id=req["id"].as_str().context("SSH profile required")?;
                let profile=self.capabilities.hybrid_profile(id)?;
                let fingerprint=crate::hybrid::tools::digest(&serde_json::to_vec(&profile)?);
                Ok(json!({"ok":true,"profile":profile,"ssh_profile_id":id,"ssh_profile_fingerprint":fingerprint}))
            }
            "ssh.hybrid.target.preview"=>{
                let id=req["id"].as_str().context("SSH profile required")?;
                let profile=self.capabilities.hybrid_profile(id)?;
                let fingerprint=crate::hybrid::tools::digest(&serde_json::to_vec(&profile)?);
                let binding=crate::hybrid::preview_remote_binding(&profile)?;
                Ok(json!({"ok":true,"profile":profile,"ssh_profile_id":id,"ssh_profile_fingerprint":fingerprint,"target":binding["target"],"provider":binding["provider"]}))
            }
            "ssh.save"=>{
                let mut p:SshProfile=serde_json::from_value(req["profile"].clone())?;p.validate()?;
                if p.id.is_empty(){p.id=uuid::Uuid::now_v7().to_string();}else{self.capabilities.get("profiles",&p.id)?;}
                let value=serde_json::to_value(p)?;self.capabilities.put("profiles",&value)?;Ok(json!({"ok":true,"profile":value}))
            }
            "ssh.delete"=>{
                anyhow::ensure!(req["confirmed"]==true,"请确认删除主机配置；已连接会话不会停止");
                self.capabilities.db.execute("DELETE FROM profiles WHERE id=?1",[req["id"].as_str().context("id required")?])?;Ok(json!({"ok":true}))
            }
            "ssh.inspect"|"ssh.connect"=>{
                let profile:SshProfile=serde_json::from_value(self.capabilities.get("profiles",req["id"].as_str().context("id required")?)?)?;profile.validate()?;
                let current_profile=serde_json::to_value(&profile)?;
                if cmd=="ssh.inspect" { return Ok(json!({"ok":true,"profile":current_profile,"program":"ssh","args":profile.args(false),"trust":"OpenSSH StrictHostKeyChecking=ask","remote_recovery":"远端进程连续性取决于远端 tmux/screen 等工具；重新连接会新建 SSH 进程"})); }
                anyhow::ensure!(req["confirmed"]==true,"请确认连接目标和端口转发");
                anyhow::ensure!(req["reviewed_profile"]==current_profile,"SSH 配置已改变，请重新检查并确认连接目标");
                self.workbench.sync_legacy(&self.projects,&self.tasks)?;
                let w=self.workbench.workspace(req["workspace_id"].as_str().context("workspace required")?)?;
                anyhow::ensure!(!w.archived,"请先恢复工作区");self.ensure_workspace_writable(&w)?;
                anyhow::ensure!(self.workbench.sessions(Some(&w.id))?.iter().filter(|s|!host::confirmed_dead(&s.descriptor.directory)).count()<32,"工作区会话已达上限");
                let project=self.project(&w.project_id)?;let id=uuid::Uuid::now_v7().to_string();let directory=project.path.join(".velella/sessions").join(&id);
                std::fs::create_dir_all(&directory)?;
                let desc=Descriptor{id:id.clone(),endpoint:ipc::endpoint(&self.root,&id),directory,spawn:SpawnRequest{ sandbox: Default::default(),program:"ssh".into(),args:profile.args(false),cwd:Some(w.path.to_string_lossy().into_owned()),env:HashMap::new(),adapter:Some("generic".into())}};
                self.workbench.put_session(&Session{id:id.clone(),workspace_id:w.id.clone(),title:format!("SSH · {}",profile.name),kind:"terminal".into(),task_id:None,native_id:None,descriptor:desc.clone()})?;
                self.start(&desc)?;self.workbench.record_event(&w.id,"ssh.connected",json!({"session_id":id,"profile_id":profile.id,"status":"process_started"}))?;
                Ok(json!({"ok":true,"session_id":id,"workspace_id":w.id}))
            }
            "ssh.diagnose"=>{
                let profile:SshProfile=serde_json::from_value(self.capabilities.get("profiles",req["id"].as_str().context("id required")?)?)?;profile.validate()?;
                // `ssh -G` expands local OpenSSH configuration without establishing a
                // transport. Keep the output bounded because it is shown in the UI.
                let output=std::process::Command::new("ssh").args(profile.args(true)).output().context("无法启动系统 OpenSSH；请确认 ssh.exe 可用")?;
                let mut text=String::from_utf8_lossy(if output.status.success(){&output.stdout}else{&output.stderr}).into_owned();
                const LIMIT:usize=32*1024;
                let truncated=text.len()>LIMIT;
                if truncated {text.truncate(LIMIT);text.push_str("\n… 输出已截断");}
                Ok(json!({"ok":true,"profile":profile,"exit_code":output.status.code(),"output":text,"truncated":truncated,"network":false}))
            }
            "automation.list"=>Ok(json!({"ok":true,"automations":self.capabilities.documents("automations")?,"runs":self.capabilities.runs()?})),
            "automation.save"=>{
                anyhow::ensure!(req["confirmed"]==true,"请确认自动执行的目标、权限、日程和项目准备脚本");
                let mut a=req["automation"].clone();let p=self.project(a["project_id"].as_str().context("project required")?)?;
                anyhow::ensure!(!p.direct,"自动化需要 Git 项目，以便每次运行使用独立工作树");
                let explicit_base=a["spec"].get("base").cloned();
                let spec:TaskSpec=serde_json::from_value(a["spec"].clone())?;
                crate::native_launch::args(&spec.agent,&spec.launch)?;
                anyhow::ensure!(["claude-code","codex"].contains(&spec.agent.as_str())&&spec.policy.permissions==Permissions::Ask&&spec.on_done==OnDone::Review,"自动化只允许 Claude/Codex 原生权限与人工审查交付");
                anyhow::ensure!(!spec.name.trim().is_empty()&&spec.name.len()<=160&&spec.instruction.as_ref().is_some_and(|s|!s.trim().is_empty()),"请输入名称和执行目标");
                anyhow::ensure!(spec.env.is_none()&&spec.policy.budget.tokens.is_none()&&spec.verify.retries<=1,"使用项目准备脚本，最多一次修复，不支持硬Token上限");
                let cap=parse_wall_clock(spec.policy.budget.wall_clock.as_deref().unwrap_or("8h")).context("时间上限格式不正确")?;
                anyhow::ensure!(cap.as_secs()>0&&cap.as_secs()<=604800,"时间上限须为1秒到7天");
                anyhow::ensure!(spec.done.iter().all(|d|matches!(d,velella_core::task::DoneCheck::Run(_))),"只支持执行命令验证");
                let interval=a["interval_minutes"].as_i64().context("interval required")?;
                anyhow::ensure!((0..=525600).contains(&interval),"间隔须为0（一次）或1到525600分钟");
                let due=a["next_run"].as_i64().context("next run required")?;
                anyhow::ensure!(due>0&&due<4102444800,"请输入2100年前的有效开始时间");
                anyhow::ensure!(a["enabled"].is_boolean(),"enabled required");
                creation_base::resolve(&p.path,explicit_base.as_ref().and_then(Value::as_str),p.default_base.as_deref())?;
                let id=a["id"].as_str().filter(|s|!s.is_empty()).map(str::to_owned).unwrap_or_else(||uuid::Uuid::now_v7().to_string());
                if a["id"].as_str().is_some_and(|s|!s.is_empty()){self.capabilities.get("automations",&id)?;}
                a["id"]=json!(id);a["project_setup"]=json!(p.setup);a["spec"]=serde_json::to_value(spec)?;
                a["project_base_policy"]=json!({"default_base":p.default_base,"setup_default":p.setup_default});
                if explicit_base.is_none(){a["spec"].as_object_mut().unwrap().remove("base");}
                self.capabilities.put("automations",&a)?;Ok(json!({"ok":true,"automation":a}))
            }
            "automation.pause"=>{
                let mut a=self.capabilities.get("automations",req["id"].as_str().context("id required")?)?;
                // Resuming requires the same reviewed save contract, never this shortcut.
                a["enabled"]=json!(false);self.capabilities.put("automations",&a)?;Ok(json!({"ok":true}))
            }
            "automation.run"=>{
                anyhow::ensure!(req["confirmed"]==true,"请确认立即运行一次");
                let a=self.capabilities.get("automations",req["id"].as_str().context("id required")?)?;
                let id=self.dispatch_automation(&a,false)?;Ok(json!({"ok":true,"task_id":id}))
            }
            "worktree.list"=>{
                self.workbench.sync_legacy(&self.projects,&self.tasks)?;
                let p=self.project(req["project_id"].as_str().context("project required")?)?;
                anyhow::ensure!(!p.direct,"Git project required");
                let branches=git(&p.path,&["for-each-ref","--format=%(refname:short)","refs/heads","refs/remotes","refs/tags"])?;
                let base=match creation_base::resolve(&p.path,None,p.default_base.as_deref()){Ok(base)=>json!({"resolved":base}),Err(error)=>json!({"error":error.to_string()})};
                Ok(json!({"ok":true,"branches":branches.lines().collect::<Vec<_>>(),"default_base":base,"checkouts":parse_worktrees(&git(&p.path,&["worktree","list","--porcelain","-z"])?),"workspaces":self.workbench.workspaces()?.into_iter().filter(|w|w.project_id==p.id).collect::<Vec<_>>()}))
            }
            "worktree.import"=>{
                let p=self.project(req["project_id"].as_str().context("project required")?)?;
                anyhow::ensure!(!p.direct,"Git project required");
                let path=dunce::canonicalize(req["path"].as_str().context("path required")?)?;
                let checkouts=parse_worktrees(&git(&p.path,&["worktree","list","--porcelain","-z"])?);
                anyhow::ensure!(checkouts.iter().any(|r|r["path"].as_str().and_then(|s|dunce::canonicalize(s).ok()).as_ref()==Some(&path)),"该目录不是此项目的已注册 Git 工作树");
                if let Some(w)=self.workbench.workspaces()?.into_iter().find(|w|dunce::canonicalize(&w.path).ok().as_ref()==Some(&path)){
                    anyhow::ensure!(w.project_id==p.id,"该工作树已归属其他项目，请从其所属项目打开");
                    return Ok(json!({"ok":true,"workspace":w}));
                }
                let name=req["name"].as_str().unwrap_or("导入的工作树").trim();anyhow::ensure!(!name.is_empty()&&name.len()<=240,"请输入工作区名称");
                let w=Workspace{id:uuid::Uuid::now_v7().to_string(),project_id:p.id,path,name:name.into(),kind:"worktree".into(),archived:false,favorite:false,position:0,parent_workspace_id:None};
                self.workbench.put_workspace(&w)?;Ok(json!({"ok":true,"workspace":w}))
            }
            _=>anyhow::bail!("unknown capability operation"),
        }
    }

    fn automation_busy(&self,a:&Value)->Result<bool>{
        let mut stmt=self.capabilities.db.prepare("SELECT task_id FROM runs WHERE automation_id=?1")?;
        let ids=stmt.query_map([a["id"].as_str()],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
        Ok(ids.iter().any(|id|self.tasks.iter().any(|t|*id==t.id&&!matches!(t.phase,Phase::Cancelled|Phase::Delivered))))
    }
    fn dispatch_automation(&mut self,a:&Value,scheduled:bool)->Result<String>{
        anyhow::ensure!(!self.automation_busy(a)?,"上次运行仍在执行或等待处理，请先在任务中完成审查/停止");
        let p=self.project(a["project_id"].as_str().context("project required")?)?;
        anyhow::ensure!(a["project_setup"].as_str()==Some(p.setup.as_str()),"项目准备脚本已改变，请重新审阅并保存自动化");
        if let Some(policy)=a.get("project_base_policy") {
            anyhow::ensure!(*policy==json!({"default_base":p.default_base,"setup_default":p.setup_default}),"项目基准或准备策略已改变，请重新审阅并保存自动化");
        }
        let (run,id)=self.capabilities.claim(a,chrono::Utc::now().timestamp(),scheduled)?;
        match self.create_task_with_id(json!({"project_id":a["project_id"],"spec":a["spec"],"setup":"run","reviewed_setup":a["project_setup"]}),id.clone()){ 
            Ok(_)=>{self.capabilities.db.execute("UPDATE runs SET state='queued' WHERE id=?1",[run])?;Ok(id)}
            Err(e)=>{self.capabilities.db.execute("UPDATE runs SET state='failed',error=?1 WHERE id=?2",params![e.to_string(),run])?;Err(e)}
        }
    }
    pub(super) fn tick_automations(&mut self)->Result<()> {
        self.sync_automation_runs()?;
        let now=chrono::Utc::now().timestamp();
        for mut a in self.capabilities.documents("automations")? {
            if a["enabled"]!=true||a["next_run"].as_i64().unwrap_or(i64::MAX)>now {continue;}
            if self.automation_busy(&a)? {continue;}
            if let Err(e)=self.dispatch_automation(&a,true){a=self.capabilities.get("automations",a["id"].as_str().unwrap())?;a["enabled"]=json!(false);a["last_error"]=json!(e.to_string());self.capabilities.put("automations",&a)?;}
        }
        Ok(())
    }
}

fn parse_worktrees(text:&str)->Vec<Value>{
    let mut rows=Vec::new();let mut row=json!({});
    for part in text.split('\0'){
        if part.is_empty(){if row.get("path").is_some(){rows.push(row);row=json!({});}continue;}
        if let Some(path)=part.strip_prefix("worktree "){row["path"]=json!(path);}
        else if let Some(branch)=part.strip_prefix("branch "){row["branch"]=json!(branch.trim_start_matches("refs/heads/"));}
        else if let Some(oid)=part.strip_prefix("HEAD "){row["head"]=json!(oid);}
        else if part.starts_with("locked"){row["locked"]=json!(true);}
        else if part=="detached"{row["detached"]=json!(true);}
    }
    if row.get("path").is_some(){rows.push(row);}rows
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn hybrid_profile_binding_rejects_changed_or_missing_profile(){
        let root=std::env::temp_dir().join(format!("velella-profile-binding-{}",uuid::Uuid::new_v4()));std::fs::create_dir_all(&root).unwrap();let c=Capabilities::open(&root).unwrap();
        let id=uuid::Uuid::new_v4().to_string();let mut profile=json!({"id":id,"name":"fixture","host":"agent","user":"","port":22,"identity":"","directory":"","local_port":null,"remote_port":null});c.put("profiles",&profile).unwrap();
        let saved=c.hybrid_profile(&id).unwrap();let stamp=crate::hybrid::tools::digest(&serde_json::to_vec(&saved).unwrap());assert_eq!(c.bound_hybrid_profile(&id,&stamp).unwrap(),saved);
        profile["host"]=json!("different");c.put("profiles",&profile).unwrap();assert!(c.bound_hybrid_profile(&id,&stamp).is_err());assert!(c.bound_hybrid_profile(&uuid::Uuid::new_v4().to_string(),&stamp).is_err());
        drop(c);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]fn offline_schedule_skips_backlog_without_drift(){assert_eq!(advance_due(100,60,289),340);assert_eq!(advance_due(100,0,999),100);assert_eq!(advance_due(100,60,100),160);}
    #[test]fn ssh_rejects_options_and_preserves_remote_quoting(){let mut p:SshProfile=serde_json::from_value(json!({"name":"test","host":"-oProxyCommand=bad"})).unwrap();assert!(p.validate().is_err());p.host="dev.example".into();p.directory="/home/a'b $(whoami)".into();assert!(p.validate().is_ok());let a=p.args(false);assert!(a.contains(&"StrictHostKeyChecking=ask".into()));assert!(a.last().unwrap().contains("'/home/a'\"'\"'b $(whoami)'"));let diagnose=p.args(true);assert_eq!(diagnose[0],"-G");assert!(!diagnose.iter().any(|part|part.starts_with("cd --")));}
    #[test]fn porcelain_paths_are_not_split_on_newlines(){let r=parse_worktrees("worktree C:/a b\n中文\0HEAD abc\0branch refs/heads/main\0\0worktree C:/two\0detached\0\0");assert_eq!(r.len(),2);assert_eq!(r[0]["path"],"C:/a b\n中文");assert_eq!(r[1]["detached"],true);}
    #[test]fn schedule_claim_persists_before_dispatch(){let root=std::env::temp_dir().join(format!("velella-schedule-{}",uuid::Uuid::now_v7()));std::fs::create_dir_all(&root).unwrap();let mut c=Capabilities::open(&root).unwrap();let a=json!({"id":"a","enabled":true,"next_run":100,"interval_minutes":1});c.put("automations",&a).unwrap();let (_,id)=c.claim(&a,280,true).unwrap();drop(c);let c=Capabilities::open(&root).unwrap();assert_eq!(c.get("automations","a").unwrap()["next_run"],340);assert_eq!(c.runs().unwrap()[0]["task_id"],id);drop(c);std::fs::remove_dir_all(root).unwrap();}
}
