use super::*;
use base64::Engine;
use crate::workbench::{Layout, Session, Workspace};
use std::io::Write;

#[path = "native_prompt.rs"]
pub(super) mod native_prompt;

impl Service {
    pub(super) fn workbench_command(&mut self, req: Value) -> Result<Value> {
        let cmd = req["cmd"].as_str().unwrap_or("");
        if cmd == "workbench.snapshot" {
            return Ok(json!({"ok":true,"protocol":1,"projects":self.projects,"workspaces":self.workbench.workspaces()?,"tasks":self.tasks,"backups":self.workbench.backups()?,"source_links":crate::task_sources::Store::open(&self.root)?.links()?}));
        }
        if cmd == "workbench.create" {
            return self.create_workbench(req);
        }
        if cmd == "workbench.creation.status" {
            let id=req["request_id"].as_str().context("request_id required")?;
            let operation=self.workbench.creation(id,None)?;
            let retry_ready=operation.as_ref().is_some_and(|op|creation_setup::is_ordinary(op)&&op.steps[1].state==crate::workbench::CreationStepState::Failed&&op.steps[1].descriptor.as_ref().is_some_and(|desc|host::confirmed_dead(&desc.directory)));
            return Ok(json!({"ok":true,"creation":operation,"retry_ready":retry_ready}));
        }
        if cmd == "workbench.creation.retry" {
            return self.retry_workspace_setup(&req);
        }
        let id=req["workspace_id"].as_str().context("workspace required")?;
        if cmd=="workbench.source.attach" {
            let project=match self.workbench.workspace(id) {
                Ok(w)=>w.project_id,
                Err(_) => self.tasks.iter().find(|t|format!("task:{}",t.id)==id).map(|t|t.project_id.clone()).context("任务工作区不存在")?,
            };
            crate::task_sources::Store::open(&self.root)?.attach(req["source_key"].as_str().context("source task required")?,id,&project)?;
            return Ok(json!({"ok":true}));
        }
        let mut w=self.workbench.workspace(id)?;
        let project=self.project(&w.project_id)?;
        if cmd=="workbench.layout" {return Ok(json!({"ok":true,"layout":self.workbench.layout(id)?,"split_layout_version":1}));}
        if cmd=="workbench.layout.save" {
            let layout: Layout=serde_json::from_value(req["layout"].clone())?;
            return Ok(json!({"ok":true,"layout":self.workbench.save_layout(id,layout)?}));
        }
        if cmd=="workbench.events" {return Ok(json!({"ok":true,"events":self.workbench.events(id,req["since"].as_i64().unwrap_or(0))?}));}
        if cmd=="workbench.sessions" {
            let sessions:Vec<_>=self.workbench.sessions(Some(id))?.iter().map(|s|terminal_io::session_row(s,self.terminal_inflight.contains(&format!("starting:{}",s.id)))).collect();
            return Ok(json!({"ok":true,"sessions":sessions}));
        }
        if cmd=="workbench.update" {
            if let Some(name)=req["name"].as_str(){w.name=name.trim().into();}
            if let Some(favorite)=req["favorite"].as_bool(){w.favorite=favorite;}
            if let Some(position)=req["position"].as_i64(){w.position=position;}
            if let Some(parent)=req.get("parent_workspace_id") {
                w.parent_workspace_id=if parent.is_null(){None}else{Some(parent.as_str().context("parent_workspace_id must be a string or null")?.to_owned())};
            }
            if let Some(archived)=req["archived"].as_bool(){
                if archived {
                    anyhow::ensure!(req["confirmed"]==true,"review workspace state before archiving");
                    anyhow::ensure!(!self.workbench.sessions(Some(id))?.iter().any(|s|self.terminal_inflight.contains(&format!("session:{}",s.id))||!host::confirmed_dead(&s.descriptor.directory)),"workspace has live or starting sessions; stop them explicitly before archiving");
                }
                w.archived=archived;
            }
            self.workbench.put_workspace(&w)?;
            self.workbench.record_event(id,"workspace.updated",json!({"archived":w.archived,"favorite":w.favorite,"position":w.position}))?;
            return Ok(json!({"ok":true,"workspace":w}));
        }
        anyhow::ensure!(!w.archived,"restore the archived workspace before using it");
        let root=dunce::canonicalize(&w.path).context("workspace folder is unavailable")?;
        if cmd=="workbench.feedback.status" || cmd=="workbench.feedback.send" {
            // All writes must pass through the session queue with IPC outside
            // Service's mutex. No alternate send path may bypass its guards.
            anyhow::ensure!(cmd=="workbench.feedback.status","feedback must be sent through the terminal queue");
            let request_id=uuid::Uuid::parse_str(req["request_id"].as_str().context("request ID required")?)?.to_string();
            let existing=self.workbench.feedback(id,&request_id)?;
            return Ok(json!({"ok":true,"receipt":existing.map(feedback_public)}));
        }
        if cmd=="workbench.terminal.open" {
            let (desc,response)=self.prepare_workbench_launch(&req)?;
            if let Some(desc)=desc{self.start(&desc)?;}
            return Ok(response);
        }
        if cmd.starts_with("workbench.terminal.") {
            let sid=req["session_id"].as_str().context("session required")?;
            let s=self.workbench.sessions(Some(id))?.into_iter().find(|s|s.id==sid).context("session does not belong to workspace")?;
            let op=cmd.trim_start_matches("workbench.terminal.");
            if op=="restore.list"||op=="restore.select" {
                anyhow::ensure!(!w.archived&&crate::terminal_restore::needs_selection(&s),"this terminal does not need an initial session selection");
                if op=="restore.list"{return Ok(json!({"ok":true,"sessions":crate::terminal_restore::candidates(&s.descriptor)?}));}
                anyhow::ensure!(req["confirmed"]==true,"initial session selection requires confirmation");
                let native=req["native_id"].as_str().context("native session required")?;
                crate::terminal_restore::bind(&s.descriptor,native)?;
                let mut selected=s;selected.native_id=Some(native.into());self.workbench.put_session(&selected)?;
                return Ok(json!({"ok":true}));
            }
            if op=="output" {anyhow::ensure!(!s.descriptor.is_legacy_chat(),"native chat has no terminal output");return host::read_output(&s.descriptor,req["since"].as_u64().unwrap_or(0));}
            if op=="stop" {
                anyhow::ensure!(req["confirmed"]==true,"explicit process stop confirmation required");
                if let Some(tid)=s.task_id {return self.task_command(json!({"cmd":"task.cancel","task_id":tid}));}
                ipc::atomic_json(&s.descriptor.directory.join("terminal-stop-requested.json"),&json!({"requested":true}))?;
                stop_host(&s.descriptor)?;self.workbench.record_event(&w.id,"terminal.stop_requested",json!({"session_id":s.id}))?;return Ok(json!({"ok":true}));
            }
            if op=="recover" {
                let (desc,response)=self.prepare_workbench_launch(&req)?;
                if let Some(desc)=desc{self.start(&desc)?;}
                self.workbench.record_event(&w.id,"terminal.recovered",json!({"session_id":s.id}))?;
                return Ok(response);
            }
            anyhow::ensure!(["input","resize","control"].contains(&op),"unknown terminal operation");
            anyhow::ensure!(!s.descriptor.is_legacy_chat(),"standalone Codex chat has been removed; saved session data is retained");
            if op=="input" && s.task_id.is_none() {self.ensure_workspace_writable(&w)?;}
            if let Some(tid)=s.task_id {
                let task=self.tasks.iter().find(|t|t.id==tid).context("task not found")?;
                let target=if task.agent.as_ref().is_some_and(|d|d.id==sid){"agent"}else if task.shell.as_ref().is_some_and(|d|d.id==sid){"shell"}else if task.command.as_ref().is_some_and(|d|d.id==sid){"command"}else{anyhow::bail!("historical task session is read-only")};
                let mut forward=req.clone();forward["cmd"]=json!(format!("terminal.{op}"));forward["task_id"]=json!(tid);forward["target"]=json!(target);
                return self.task_command(forward);
            }
            let mut forward=req.clone();forward["cmd"]=json!(op);forward["automated"]=json!(false);
            if op=="input" && req["input_kind"]=="pointer" {
                anyhow::ensure!(req["enter"]!=true&&crate::terminal_input::is_pointer_report(req["text"].as_str().unwrap_or("")),"invalid terminal pointer report");
                forward["input_kind"]=json!("legacy");
            }
            return call_host(&s.descriptor,forward);
        }
        match cmd {
            "workbench.files" => Ok(json!({"ok":true,"files":folder_files(&root)?})),
            "workbench.search" => {
                let query=req["query"].as_str().context("search query required")?.trim();
                anyhow::ensure!(!query.is_empty() && query.chars().count()<=200,"search query must contain 1 to 200 characters");
                let needle=query.to_lowercase();
                let mut scanned=0u64;
                let mut matches=Vec::new();
                for relative in folder_files(&root)? {
                    if matches.len()>=200 || scanned>=8*1024*1024 { break; }
                    let path=contained_file(&root,&relative)?;
                    let size=path.metadata()?.len();
                    if size>2*1024*1024 || scanned.saturating_add(size)>8*1024*1024 { continue; }
                    let Ok(content)=std::fs::read_to_string(&path) else { continue; };
                    scanned+=size;
                    for (line,excerpt) in content.lines().enumerate() {
                        if excerpt.to_lowercase().contains(&needle) {
                            matches.push(json!({"path":relative,"line":line+1,"excerpt":excerpt.chars().take(300).collect::<String>()}));
                            if matches.len()>=200 { break; }
                        }
                    }
                }
                Ok(json!({"ok":true,"matches":matches,"truncated":matches.len()>=200 || scanned>=8*1024*1024}))
            }
            "workbench.file.read" => {
                editable_path(req["path"].as_str().context("path required")?)?;
                let path=contained_file(&root,req["path"].as_str().context("path required")?)?;
                editable_canonical_path(&root,&path)?;
                anyhow::ensure!(path.is_file() && path.metadata()?.len()<=2*1024*1024,"text editor supports files up to 2 MiB");
                Ok(json!({"ok":true,"content":std::fs::read_to_string(path)?}))
            }
            "workbench.file.asset" => {
                editable_path(req["path"].as_str().context("path required")?)?;
                let path=contained_file(&root,req["path"].as_str().context("path required")?)?;
                editable_canonical_path(&root,&path)?;
                anyhow::ensure!(path.is_file() && path.metadata()?.len()<=10*1024*1024,"preview supports files up to 10 MiB");
                let extension=path.extension().and_then(|x|x.to_str()).unwrap_or("").to_ascii_lowercase();
                let mime=match extension.as_str() {
                    "png"=>"image/png", "jpg"|"jpeg"=>"image/jpeg", "gif"=>"image/gif", "webp"=>"image/webp", "svg"=>"image/svg+xml", "pdf"=>"application/pdf",
                    _=>anyhow::bail!("preview supports PNG, JPEG, GIF, WebP, SVG, and PDF"),
                };
                Ok(json!({"ok":true,"mime":mime,"data":base64::engine::general_purpose::STANDARD.encode(std::fs::read(path)?)}))
            }
            "workbench.file.create" => {
                self.ensure_workspace_writable(&w)?;
                let relative=req["path"].as_str().context("path required")?;
                let path=creatable_file(&root,relative)?;
                anyhow::ensure!(!path.exists(),"file already exists; use an explicit save after reviewing it");
                let content=req["content"].as_str().unwrap_or("");
                anyhow::ensure!(content.len()<=2*1024*1024,"text editor supports files up to 2 MiB");
                let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
                file.write_all(content.as_bytes())?;
                file.sync_all()?;
                self.workbench.record_event(&w.id,"file.created",json!({"path":relative}))?;
                Ok(json!({"ok":true}))
            }
            "workbench.file.rename" => {
                self.ensure_workspace_writable(&w)?;
                let relative=req["path"].as_str().context("path required")?;
                editable_path(relative)?;
                let path=contained_file(&root,relative)?;
                editable_canonical_path(&root,&path)?;
                anyhow::ensure!(path.is_file(),"only files can be renamed from this workbench");
                let next_relative=req["new_path"].as_str().context("new path required")?;
                let next=creatable_file(&root,next_relative)?;
                anyhow::ensure!(!next.exists(),"destination file already exists");
                std::fs::rename(path,next)?;
                self.workbench.record_event(&w.id,"file.renamed",json!({"path":relative,"new_path":next_relative}))?;
                Ok(json!({"ok":true}))
            }
            "workbench.file.delete" => {
                self.ensure_workspace_writable(&w)?;
                anyhow::ensure!(req["confirmed"]==true,"explicit file deletion confirmation required");
                let relative=req["path"].as_str().context("path required")?;
                editable_path(relative)?;
                let path=contained_file(&root,relative)?;
                editable_canonical_path(&root,&path)?;
                anyhow::ensure!(path.is_file(),"only files can be deleted from this workbench");
                std::fs::remove_file(path)?;
                self.workbench.record_event(&w.id,"file.deleted",json!({"path":relative}))?;
                Ok(json!({"ok":true}))
            }
            "workbench.file.write" => {
                self.ensure_workspace_writable(&w)?;
                editable_path(req["path"].as_str().context("path required")?)?;
                let path=contained_file(&root,req["path"].as_str().context("path required")?)?;
                editable_canonical_path(&root,&path)?;
                let previous=req["previous"].as_str().context("previous content required")?;
                anyhow::ensure!(std::fs::read_to_string(&path)?==previous,"file changed on disk; reload before saving");
                ipc::atomic_write(&path,req["content"].as_str().context("content required")?.as_bytes())?;
                self.workbench.record_event(&w.id,"file.saved",json!({"path":req["path"]}))?;
                Ok(json!({"ok":true}))
            }
            "workbench.git" => {
                if project.direct {return Ok(json!({"ok":true,"git":false,"status":"普通文件夹","diff":"","staged":""}));}
                Ok(json!({"ok":true,"git":true,"status":git(&root,&["status","--short"] )?,"diff":git(&root,&["diff","--no-ext-diff","--no-color"] )?,"staged":git(&root,&["diff","--cached","--no-ext-diff","--no-color"] )?,"branch":git(&root,&["branch","--show-current"])?}))
            }
            "workbench.git.stage" | "workbench.git.unstage" => {
                self.ensure_workspace_writable(&w)?;
                anyhow::ensure!(!project.direct,"Git repository required");
                let path=req["path"].as_str().context("path required")?;
                editable_path(path)?;
                // Validate lexical path too: deleted files cannot canonicalize.
                let p=Path::new(path);
                anyhow::ensure!(!path.is_empty() && p.components().all(|c|matches!(c,std::path::Component::Normal(_))) && !p.is_absolute(),"invalid relative path");
                if cmd.ends_with("unstage") {git(&root,&["--literal-pathspecs","reset","HEAD","--",path])?;self.workbench.record_event(&w.id,"git.unstaged",json!({"path":path}))?;} else {git(&root,&["--literal-pathspecs","add","--",path])?;self.workbench.record_event(&w.id,"git.staged",json!({"path":path}))?;}
                Ok(json!({"ok":true}))
            }
            "workbench.git.commit" => {
                self.ensure_workspace_writable(&w)?;
                anyhow::ensure!(!project.direct && req["confirmed"]==true,"explicit local commit confirmation required");
                let message=req["message"].as_str().context("commit message required")?.trim();
                anyhow::ensure!(!message.is_empty(),"commit message required");
                anyhow::ensure!(!git(&root,&["diff","--cached","--name-only"] )?.trim().is_empty(),"stage changes before committing");
                let output=git(&root,&["commit","-m",message])?;
                self.workbench.record_event(&w.id,"git.committed",json!({"message":message}))?;
                Ok(json!({"ok":true,"output":output}))
            }
            _ => anyhow::bail!("unknown workbench operation"),
        }
    }

    pub(super) fn native_launch_validation(&self,req:&Value)->Result<Option<NativeLaunchValidation>> {
        let cmd=req["cmd"].as_str().unwrap_or("");
        if !matches!(cmd,"workbench.create"|"workbench.terminal.open"|"workbench.terminal.recover") {return Ok(None)}
        if cmd=="workbench.terminal.recover" {
            anyhow::ensure!(req.get("permission_mode").is_none()&&req.get("sandbox").is_none()&&req.get("launch_options").is_none()&&req.get("initial_prompt").is_none()&&req.get("account_home").is_none()&&req.get("account_action").is_none(),"恢复会话不能修改权限、执行模式、模型、推理强度、账号目录或初始提示词");
            let workspace=req["workspace_id"].as_str().context("workspace required")?;
            let w=self.workbench.workspace(workspace)?;
            self.project(&w.project_id)?;
            self.ensure_workspace_prepared(workspace)?;
            anyhow::ensure!(!w.archived&&w.path.is_dir(),"workspace is archived or unavailable");
            self.ensure_workspace_writable(&w)?;
            let s=self.workbench.session(workspace,req["session_id"].as_str().context("session required")?)?.context("session does not belong to workspace")?;
            anyhow::ensure!(s.task_id.is_none()&&s.kind=="terminal","use the original task or creation controls to recover this session");
            if req["automatic"]==true {anyhow::ensure!(automatic_recovery_allowed(&s,req),"此会话暂不能自动恢复，请检查原会话恢复信息");}
            ensure_terminal_recovery_supported(&s.descriptor)?;
            anyhow::ensure!(host::confirmed_dead(&s.descriptor.directory),"terminal is still running or its ownership cannot be confirmed");
            let mut desc=s.descriptor.clone();crate::terminal_restore::resume(&mut desc,s.native_id.as_deref())?;
            return NativeLaunchValidation::saved(&desc)
        }
        let requested=req.get("request_id").map(|id|->Result<_>{Ok(uuid::Uuid::parse_str(id.as_str().context("request_id must be a UUID string")?)?)}).transpose()?.map(|id|id.to_string());
        // Replays retain the existing comparison path, without consulting today's CLI.
        if cmd=="workbench.create" {
            if let Some(id)=&requested {if self.workbench.creation(id,None)?.is_some(){return Ok(None)}}
            if self.root.join("task-sources.db").exists() {
                let sources=crate::task_sources::Store::open(&self.root)?;
                if let Some(id)=&requested {if sources.saved_workspace_request(id)?.is_some(){return Ok(None)}}
                if let (Some(key),Some(project))=(req["source_key"].as_str(),req["project_id"].as_str()) {
                    if sources.linked_workspace(key,project)?.is_some(){return Ok(None)}
                }
            }
        } else if let Some(id)=&requested {
            if self.workbench.sessions(None)?.iter().any(|s|&s.id==id){return Ok(None)}
        }
        let options:velella_core::task::LaunchOptions=serde_json::from_value(req.get("launch_options").cloned().unwrap_or(json!({})))?;
        let permission=crate::terminal_permissions::requested(req)?;
        if options.is_empty()&&permission=="inherit"{return Ok(None)}
        let launch=req["launch"].as_str().unwrap_or(if cmd=="workbench.create"{"none"}else{"terminal"});
        crate::native_launch::args(launch,&options)?;
        crate::terminal_permissions::plan(launch,permission)?;
        anyhow::ensure!(permission=="inherit"||req["collaboration"]!=true,"协作终端的权限由角色策略管理");
        let (program,_,_)=workspace_launch(launch)?;
        if cmd=="workbench.create" {
            let project=self.project(req["project_id"].as_str().context("project required")?)?;
            anyhow::ensure!(!project.direct,"ordinary folders have one primary workspace");
            creation_setup::validate_execution(req,SetupPolicy::Skip)?;
            let execution:crate::sandbox::Policy=serde_json::from_value(req.get("execution").cloned().unwrap_or(json!({"mode":"native"})))?;
            anyhow::ensure!(!execution.is_sandboxed(),"native launch options require native execution");
        } else {
            let workspace=req["workspace_id"].as_str().context("workspace required")?;
            let w=self.workbench.workspace(workspace)?;
            self.project(&w.project_id)?;
            self.ensure_workspace_prepared(workspace)?;
            anyhow::ensure!(!w.archived&&w.path.is_dir(),"workspace is archived or unavailable");
            self.ensure_workspace_writable(&w)?;
            anyhow::ensure!(!requested_restricted_terminal(req,launch)?,"受限终端的模型与推理强度只能继承执行主机配置");
            anyhow::ensure!(req["role_config"].is_null(),"角色配置与原生模型参数不能同时设置");
        }
        let mut paths=vec![self.executable.parent().context("runtime directory")?.to_path_buf()];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        let path=std::env::join_paths(paths)?.to_string_lossy().into_owned();
        Ok(Some(NativeLaunchValidation{launch:launch.into(),options,permission:permission.into(),program,resume:false,path:Some(path)}))
    }

    // Persist launch intent and authorize it under the state lock. The daemon
    // dispatcher waits for the host outside that lock, under its session gate.
    pub(super) fn prepare_workbench_launch(&mut self,req:&Value)->Result<(Option<Descriptor>,Value)> {
        let workspace=req["workspace_id"].as_str().context("workspace required")?;
        let w=self.workbench.workspace(workspace)?;
        let project=self.project(&w.project_id)?;
        self.ensure_workspace_prepared(workspace)?;
        anyhow::ensure!(!w.archived,"restore the archived workspace before using it");
        let root=dunce::canonicalize(&w.path).context("workspace folder is unavailable")?;
        self.ensure_workspace_writable(&w)?;
        if req["cmd"]=="workbench.terminal.recover" {
            anyhow::ensure!(req.get("permission_mode").is_none()&&req.get("sandbox").is_none()&&req.get("launch_options").is_none()&&req.get("initial_prompt").is_none()&&req.get("account_home").is_none()&&req.get("account_action").is_none(),"恢复会话不能修改权限、执行模式、模型、推理强度、账号目录或初始提示词");
            let sid=req["session_id"].as_str().context("session required")?;
            let mut s=self.workbench.sessions(Some(workspace))?.into_iter().find(|s|s.id==sid).context("session does not belong to workspace")?;
            anyhow::ensure!(!s.descriptor.spawn.env.contains_key("VELELLA_SKILL_ARGV_PROMPT"),"此技能任务使用启动参数提交，恢复不能重放任务。请使用该工具原生会话恢复入口。");
            anyhow::ensure!(s.task_id.is_none(),"task-owned sessions must be recovered with the task controls");
            anyhow::ensure!(s.kind=="terminal","准备脚本等命令会话只能通过原创建流程确认重试");
            if req["automatic"]==true {
                anyhow::ensure!(automatic_recovery_allowed(&s,req),"此会话暂不能自动恢复，请检查原会话恢复信息");
            }
            if s.descriptor.spawn.sandbox.is_restricted() {
                anyhow::ensure!(w.kind=="worktree"&&!project.direct&&!self.tasks.iter().any(|task|task.worktree==w.path),"受限终端只能在原有、未被任务占用的 Git 工作树中恢复");
                anyhow::ensure!(s.descriptor.spawn.cwd.as_deref()==root.to_str(),"恢复目录与原会话工作树不匹配");
            }
            ensure_terminal_recovery_supported(&s.descriptor)?;
            crate::collaboration::providers::verify_mcp_bindings(&s.descriptor)?;
            anyhow::ensure!(host::confirmed_dead(&s.descriptor.directory),"terminal is still running or its ownership cannot be confirmed");
            let native=crate::terminal_restore::supported(&s.descriptor);
            if native{s.native_id=crate::terminal_restore::resume(&mut s.descriptor,s.native_id.as_deref())?;crate::terminal_restore::attach(&mut s.descriptor,&self.executable)?;}
            if s.descriptor.spawn.env.contains_key(host::INITIAL_PROMPT_ENV) {
                // Keep the immutable contract for launch replay, but permanently
                // close initial delivery before a recovered host resets sequence.
                ipc::atomic_json(&s.descriptor.directory.join(host::INITIAL_PROMPT_CLOSED),&json!({"recovered":true}))?;
            }
            if req["automatic"]==true {
                // Durable claim under the session gate: a crash, reload or lost
                // response must never turn recovery into an endless spawn loop.
                if native{crate::terminal_restore::claim(&s.descriptor)?;}else{
                    let previous=std::fs::read(s.descriptor.directory.join("status.json")).ok().and_then(|v|serde_json::from_slice::<Status>(&v).ok());
                    ipc::atomic_json(&s.descriptor.directory.join("terminal-auto-recovery.json"),&json!({"attempted":true,"previous_host_pid":previous.map(|s|s.pid),"at":chrono::Utc::now().to_rfc3339()}))?;
                }
                if req["on_open"]==true {
                    match std::fs::remove_file(s.descriptor.directory.join("terminal-stop-requested.json")){Ok(())=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.into())}
                }
            } else {
                for name in ["terminal-auto-recovery.json","terminal-stop-requested.json"] {
                    match std::fs::remove_file(s.descriptor.directory.join(name)){Ok(())=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.into())}
                }
            }
            terminal_io::clear_input_marker(&s.descriptor.directory)?;
            if !s.descriptor.spawn.sandbox.is_sandboxed()&&s.descriptor.spawn.adapter.as_deref()!=Some("hybrid-codex") {crate::browser_agent::inject(&mut s.descriptor,&self.executable,&self.root,workspace)?;}
            self.workbench.put_session(&s)?;
            return Ok((Some(s.descriptor),json!({"ok":true,"session_id":sid})));
        }
        let launch=req["launch"].as_str().unwrap_or("terminal");
        let (program,mut args,title)=workspace_launch(launch)?;
        let requested_role=if req["role_config"].is_null(){None}else{Some(serde_json::from_value::<crate::collaboration::orchestration::v2::RoleDefinition>(req["role_config"].clone())?)};
        let remote_role=requested_role.as_ref().filter(|role|role.remote_codex.is_some());
        anyhow::ensure!(remote_role.is_none()||launch=="hybrid-codex","远程角色不能回退到本机启动");
        let launch_options:velella_core::task::LaunchOptions=serde_json::from_value(req.get("launch_options").cloned().unwrap_or(json!({})))?;
        let hybrid_profile=if launch=="hybrid-codex"{
            anyhow::ensure!(launch_options.is_empty()&&crate::terminal_permissions::requested(req)?=="inherit"&&req["account_home"].is_null()&&req["account_action"].is_null()&&req["skill_prompt"].is_null(),"混合执行使用远端 Codex 配置，不能附加本机账号或权限覆盖");
            let id=req["ssh_profile_id"].as_str().context("SSH profile required")?;
            if let Some(role)=remote_role {
                anyhow::ensure!(req["collaboration"]==true,"远程角色需要受控协作会话");crate::collaboration::providers::validate_remote(role)?;
                crate::collaboration::providers::validate(role)?;
                let remote=role.remote_codex.as_ref().unwrap();anyhow::ensure!(remote.ssh_profile_id==id,"远程角色 SSH 配置不匹配");
                Some(self.capabilities.bound_hybrid_profile(id,&remote.ssh_profile_fingerprint)?)
            }else{
                anyhow::ensure!(req["collaboration"]!=true&&requested_role.is_none(),"普通远程终端不能附加协作角色");Some(self.capabilities.hybrid_profile(id)?)
            }
        }else{anyhow::ensure!(req["ssh_profile_id"].is_null(),"SSH profile is only accepted for hybrid Codex");None};
        let requested_args=crate::native_launch::args(launch,&launch_options)?;
        let permission=crate::terminal_permissions::requested(req)?;
        let permission_plan=crate::terminal_permissions::plan(launch,permission)?;
        let restricted=requested_restricted_terminal(req,launch)?;
        anyhow::ensure!(permission=="inherit"||(!restricted&&req["collaboration"]!=true&&req["role_config"].is_null()),"受限、协作或角色终端使用自身权限策略，不能覆盖默认权限");
        anyhow::ensure!(!restricted||launch_options.is_empty(),"受限终端的模型与推理强度只能继承执行主机配置");
        let initial_prompt=native_prompt::requested(req)?;
        let skill_prompt=req.get("skill_prompt").map(|v|v.as_str().context("skill_prompt must be a string")).transpose()?;
        anyhow::ensure!(skill_prompt.is_none()||(!restricted&&req["collaboration"]!=true&&req["role_config"].is_null()&&initial_prompt.is_none()&&req["account_action"].is_null()),"skill prompt requires an ordinary native session without another initial prompt");
        anyhow::ensure!(req.get("track_submission").is_none_or(Value::is_boolean),"track_submission must be a boolean");
        let track_submission=req["track_submission"]==true;
        anyhow::ensure!(!track_submission||(!restricted&&matches!(launch,"codex"|"claude-code")),"submission tracking requires native Codex or Claude");
        anyhow::ensure!(initial_prompt.is_none()||(!restricted&&matches!(launch,"codex"|"claude-code")),"initial_prompt requires native Codex or Claude; restricted prompts require manual input");
        let collaboration=req["collaboration"]==true;
        let account=super::account_launch::Selection::requested(req,launch,restricted,collaboration)?;
        anyhow::ensure!(req["role_config"].is_null()||launch_options.is_empty(),"角色配置与原生模型参数不能同时设置");
        args.extend(requested_args);
        args.extend(permission_plan.args);
        anyhow::ensure!(!collaboration || (!restricted && (crate::collaboration::providers::program(launch).is_ok()||remote_role.is_some())),"collaboration requires a supported native CLI terminal");
        if restricted {
            anyhow::ensure!(w.kind=="worktree"&&!project.direct&&!self.tasks.iter().any(|task|task.worktree==w.path),"受限工具仅支持非任务占用的 Git 工作树，请新建工作树后重试");
        }
        let requested=req["request_id"].as_str().map(uuid::Uuid::parse_str).transpose().context("invalid launch request ID")?.map(|id|id.to_string());
        if let Some(role)=&requested_role {if role.remote_codex.is_some(){crate::collaboration::providers::validate_remote(role)?;}else{crate::collaboration::providers::validate(role)?;}}
        let role_fingerprint=requested_role.as_ref().map(crate::collaboration::providers::role_fingerprint).transpose()?;
        if let Some(ref sid)=requested {
            if let Some(existing)=self.workbench.sessions(None)?.iter().find(|s|&s.id==sid) {
                ensure_matching_launch(existing,workspace,title,restricted,role_fingerprint.as_deref())?;
                if let Some(profile)=&hybrid_profile{
                    let saved=crate::hybrid::load(&existing.descriptor.directory)?;
                    anyhow::ensure!(saved.profile==*profile&&saved.project_root==root,"混合执行请求已绑定另一 SSH 配置或项目目录");
                }
                account.ensure_saved(&existing.descriptor)?;
                let saved_options:velella_core::task::LaunchOptions=existing.descriptor.spawn.env.get("VELELLA_NATIVE_LAUNCH_OPTIONS").map(|text|serde_json::from_str(text)).transpose()?.unwrap_or_default();
                anyhow::ensure!(saved_options==launch_options,"创建请求已绑定另一模型或推理强度，请新建会话");
                anyhow::ensure!(crate::terminal_permissions::saved(&existing.descriptor.spawn.env)==permission,"创建请求已绑定另一权限模式，请新建会话");
                anyhow::ensure!(native_prompt::saved(&existing.descriptor)?==initial_prompt,"launch request belongs to another initial_prompt text or mode");
                anyhow::ensure!(existing.descriptor.spawn.env.get("VELELLA_SKILL_ARGV_PROMPT").map(String::as_str)==skill_prompt,"launch request belongs to another skill prompt");
                anyhow::ensure!(existing.descriptor.spawn.env.contains_key(crate::submission_receipts::TRACKING_ENV)==track_submission,"launch request belongs to another submission tracking mode");
                crate::collaboration::providers::verify_mcp_bindings(&existing.descriptor)?;
                anyhow::ensure!(existing.descriptor.spawn.env.contains_key("VELELLA_COLLABORATION_SKILL")==collaboration,"launch request belongs to another collaboration mode");
                return Ok((None,json!({"ok":true,"session_id":sid,"preflight":crate::collaboration::providers::preflight_receipt(&existing.descriptor)})));
            }
        }
        anyhow::ensure!(self.workbench.sessions(Some(workspace))?.iter().filter(|s|self.terminal_inflight.contains(&format!("session:{}",s.id))||!host::confirmed_dead(&s.descriptor.directory)).count()<32,"workspace has 32 live or unconfirmed sessions; stop unused sessions first");
        let id=requested.unwrap_or_else(||uuid::Uuid::now_v7().to_string());
        if let Some(prompt)=skill_prompt {args.extend(crate::native_launch::skill_prompt_args(launch,prompt)?);}
        let directory=if restricted {restricted_terminal_directory(&self.root,&self.executable,&root,&id)?}else{project.path.join(".velella/sessions").join(&id)};
        if !restricted {std::fs::create_dir_all(&directory)?;}
        let sandbox=if restricted {crate::sandbox::Policy::Restricted {read_only:vec![],writable:vec![],network:crate::sandbox::Network::Denied,home:Some(directory.join("tool-home"))}}else{Default::default()};
        let mut desc=Descriptor{id:id.clone(),endpoint:ipc::endpoint(&self.root,&id),directory,spawn:SpawnRequest{ sandbox,
            program,args,cwd:Some(root.to_string_lossy().into_owned()),
            env:HashMap::from([("TERM".into(),"xterm-256color".into()),("PATH".into(),std::env::var("PATH").unwrap_or_default())]),adapter:Some(if restricted{crate::terminal_sandbox::ADAPTER}else{"generic"}.into())}};
        if let Some(profile)=hybrid_profile{
            let config=crate::hybrid::Config{session_id:id.clone(),workspace_id:workspace.into(),project_root:root.clone(),directory:desc.directory.clone(),profile_fingerprint:crate::hybrid::tools::digest(&serde_json::to_vec(&profile)?),profile,collaboration_data:if remote_role.is_some(){Some(dunce::canonicalize(&self.root)?)}else{None},collaboration_writes:remote_role.is_some_and(|role|role.writes())};
            ipc::atomic_json(&desc.directory.join("hybrid.json"),&config)?;
            desc.spawn.program=self.executable.to_string_lossy().into_owned();
            desc.spawn.args=vec![if remote_role.is_some(){"hybrid-worker"}else{"hybrid-session"}.into(),desc.directory.to_string_lossy().into_owned()];
            desc.spawn.adapter=Some(if remote_role.is_some(){"hybrid-orchestration-worker"}else{"hybrid-codex"}.into());
        }
        if permission!="inherit" {
            let mut paths=vec![self.executable.parent().context("runtime directory")?.to_path_buf()];
            paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
            desc.spawn.env.insert("PATH".into(),std::env::join_paths(paths)?.to_string_lossy().into_owned());
        }
        if !launch_options.is_empty(){desc.spawn.env.insert("VELELLA_NATIVE_LAUNCH_OPTIONS".into(),serde_json::to_string(&launch_options)?);}
        if let Some(prompt)=skill_prompt {desc.spawn.env.insert("VELELLA_SKILL_ARGV_PROMPT".into(),prompt.into());}
        desc.spawn.env.extend(permission_plan.env);
        account.apply(&mut desc,launch);
        if collaboration {if remote_role.is_some(){super::collaboration_commands::prepare_hybrid_member(&mut desc,&self.executable,&self.root)?;}else{super::collaboration_commands::prepare_native_member(&mut desc,&self.executable,&self.root)?;}}
        let mut role_preflight=Value::Null;
        if let Some(role)=requested_role {
            anyhow::ensure!(collaboration,"role configuration requires a collaboration session");
            if role.remote_codex.is_some(){
                anyhow::ensure!(launch=="hybrid-codex","remote role launch mismatch");
                ipc::atomic_json(&desc.directory.join("hybrid-role.json"),&serde_json::to_value(&role)?)?;
                desc.spawn.env.insert("VELELLA_ROLE_CONFIG_FINGERPRINT".into(),crate::collaboration::providers::role_fingerprint(&role)?);
                desc.spawn.env.insert("VELELLA_PROVIDER".into(),"codex".into());desc.spawn.env.insert("VELELLA_RUNTIME_EXE".into(),desc.spawn.program.clone());
                desc.spawn.env.insert("VELELLA_HYBRID_MODEL".into(),crate::collaboration::providers::effective_model(&role));
                crate::hybrid::verify_role_binding(&desc)?;
                role_preflight=json!({"version":1,"provider":"codex","launcher":{"status":"remote_unverified"},"cli_version":{"status":"not_observed"},"model":{"requested":crate::collaboration::providers::effective_model(&role),"status":"configured_unverified"},"execution_mode":{"requested":"standard","status":"configured_unverified"},"native_ready":"not_verified"});
            }else{
                anyhow::ensure!(role.provider==launch,"role provider does not match launch");
                role_preflight=crate::collaboration::providers::configure(&mut desc,&self.executable,&self.root,&role)?;
            }
        }
        if !restricted&&!account.login {crate::terminal_activity::attach(&mut desc,&self.executable)?;}
        if !restricted&&!account.login {crate::terminal_restore::attach(&mut desc,&self.executable)?;}
        if track_submission {
            desc.spawn.env.insert(crate::submission_receipts::TRACKING_ENV.into(),"1".into());
            native_prompt::attach(&mut desc)?;
        }
        if let Some(prompt)=initial_prompt {
            desc.spawn.env.insert(host::INITIAL_PROMPT_ENV.into(),serde_json::to_string(&prompt)?);
            native_prompt::attach(&mut desc)?;
        }
        if restricted {crate::restricted_controller::validate_launch(&desc)?;}else if !account.login&&launch!="hybrid-codex"{crate::browser_agent::inject(&mut desc,&self.executable,&self.root,workspace)?;}
        self.workbench.put_session(&Session{id:id.clone(),workspace_id:workspace.into(),title:title.into(),kind:"terminal".into(),task_id:None,native_id:None,descriptor:desc.clone()})?;
        self.workbench.record_event(workspace,"terminal.launch_requested",json!({"session_id":id}))?;
        Ok((Some(desc),json!({"ok":true,"session_id":id,"preflight":role_preflight})))
    }

    pub(super) fn ensure_workspace_writable(&self,w:&Workspace)->Result<()> {
        // Existing task service remains authoritative for task-owned roots.
        anyhow::ensure!(!self.tasks.iter().any(|t|t.worktree==w.path && !matches!(t.phase,Phase::Queued|Phase::Cancelled|Phase::Delivered)),"workspace is owned by a task; use the task controls until it ends");
        Ok(())
    }
}

pub(super) struct NativeLaunchValidation {
    launch:String,
    options:velella_core::task::LaunchOptions,
    permission:String,
    program:String,
    resume:bool,
    path:Option<String>,
}
impl NativeLaunchValidation {
    pub(super) fn validate(self)->Result<()> {
        crate::native_launch::validated_args_for_program(&self.launch,&self.options,&self.program,self.resume,self.path.as_deref().map(std::ffi::OsStr::new))?;
        crate::terminal_permissions::validate(&self.launch,&self.permission,&self.program,self.resume,self.path.as_deref().map(std::ffi::OsStr::new))?;
        Ok(())
    }
    fn saved(desc:&Descriptor)->Result<Option<Self>> {
        let options:velella_core::task::LaunchOptions=desc.spawn.env.get("VELELLA_NATIVE_LAUNCH_OPTIONS").map(|text|serde_json::from_str(text)).transpose()?.unwrap_or_default();
        let permission=crate::terminal_permissions::saved(&desc.spawn.env);
        if options.is_empty()&&permission=="inherit"{return Ok(None)}
        anyhow::ensure!(!desc.spawn.sandbox.is_sandboxed()&&!desc.is_legacy_chat(),"saved launch options require a native terminal");
        let launch=crate::terminal_permissions::agent(&desc.spawn.program)?;
        crate::native_launch::args(launch,&options)?;
        crate::terminal_permissions::plan(launch,permission)?;
        Ok(Some(Self{launch:launch.into(),options,permission:permission.into(),program:desc.spawn.program.clone(),resume:desc.spawn.args.first().is_some_and(|arg|arg=="resume"),path:desc.spawn.env.get("PATH").cloned()}))
    }
}

pub(super) fn validate_saved_launch(desc:&Descriptor)->Result<()> {
    if let Some(validation)=NativeLaunchValidation::saved(desc)? {validation.validate()?;}
    Ok(())
}

pub(super) fn session_execution_mode(policy:&crate::sandbox::Policy)->&'static str {
    match policy {crate::sandbox::Policy::Native {}=>"native",crate::sandbox::Policy::Isolated {..}=>"isolated",crate::sandbox::Policy::Restricted {..}=>"restricted"}
}

fn requested_restricted_terminal(req:&Value,launch:&str)->Result<bool> {
    let Some(value)=req.get("sandbox") else{return Ok(false)};
    let object=value.as_object().context("sandbox 必须只包含 mode 字段")?;
    anyhow::ensure!(object.len()==1&&object.contains_key("mode"),"sandbox 必须只包含 mode 字段，目录与网络策略由后台管理");
    match object["mode"].as_str() {
        Some("native")=>Ok(false),
        Some("restricted")=>{anyhow::ensure!(launch=="codex","受限工具模式仅用于 Codex 终端");Ok(true)},
        _=>anyhow::bail!("执行模式必须为 native 或 restricted"),
    }
}

fn ensure_matching_launch(existing:&Session,workspace:&str,title:&str,restricted:bool,role_fingerprint:Option<&str>)->Result<()> {
    anyhow::ensure!(existing.workspace_id==workspace&&existing.title==title,"launch request belongs to another workspace or action");
    anyhow::ensure!(session_execution_mode(&existing.descriptor.spawn.sandbox)==if restricted{"restricted"}else{"native"},"创建请求已绑定另一执行模式，请新建会话");
    anyhow::ensure!(existing.descriptor.spawn.env.get("VELELLA_ROLE_CONFIG_FINGERPRINT").map(String::as_str)==role_fingerprint,"创建请求已绑定另一角色配置，请新建会话");
    Ok(())
}

pub(super) fn ensure_terminal_recovery_supported(desc:&Descriptor)->Result<()> {
    anyhow::ensure!(!desc.is_legacy_chat(),"standalone Codex chat has been removed; saved session data is retained");
    anyhow::ensure!(desc.spawn.adapter.as_deref()!=Some("hybrid-orchestration-worker"),"远程编排会话需要核对未完成任务，不能直接恢复或重放");
    if desc.spawn.sandbox.is_restricted() { crate::terminal_recovery::thread(desc)?; }
    Ok(())
}

fn automatic_recovery_allowed(session:&Session,request:&Value)->bool {
    if request["on_open"]==true {crate::terminal_restore::open_allowed(session)}
    else {automatic_shell_recovery_allowed(session)||crate::terminal_restore::automatic_allowed(session)}
}
pub(super) fn automatic_shell_recovery_allowed(session:&Session)->bool {
    let desc=&session.descriptor;
    if session.task_id.is_some()||session.kind!="terminal"||desc.spawn.sandbox.is_sandboxed()
        ||desc.spawn.adapter.as_deref()!=Some("generic")
        ||desc.directory.join("terminal-stop-requested.json").exists(){return false}
    let Ok((program,args,_))=workspace_launch("terminal") else{return false};
    if desc.spawn.program!=program||desc.spawn.args!=args||!host::confirmed_dead(&desc.directory){return false}
    // Only a previously started interactive shell with a missing owner is
    // eligible. Never replay launch commands, Agent prompts or a normal exit.
    let status_path=desc.directory.join("status.json");
    let Some(status)=std::fs::read(&status_path).ok().and_then(|v|serde_json::from_slice::<Status>(&v).ok())else{return false};
    if status.pid==0||status.exit.is_some(){return false}
    let claim_path=desc.directory.join("terminal-auto-recovery.json");
    if !claim_path.exists(){return true}
    let Some(claim)=std::fs::read(claim_path).ok().and_then(|v|serde_json::from_slice::<Value>(&v).ok())else{return false};
    let previous=claim["previous_host_pid"].as_u64();
    if previous==Some(u64::from(status.pid))||status.input_protocol!=1{return false}
    // A recovery claim belongs to a startup, not the lifetime of a tab. Only a
    // different host with actual input or sustained heartbeats can rearm it.
    // Compare the saved heartbeat time, never today's clock: simply waiting or
    // reloading must not turn a failed launch into another automatic attempt.
    if previous.is_some()&&status.input_sequence.is_some_and(|n|n>0){return true}
    let Some(at)=claim["at"].as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok())else{return false};
    let Some(modified)=std::fs::metadata(status_path).ok().and_then(|m|m.modified().ok())else{return false};
    let heartbeat:chrono::DateTime<chrono::Utc>=modified.into();
    // Legacy claims have no PID. A later healthy persisted heartbeat is the
    // migration evidence; absent that evidence they remain safely blocked.
    heartbeat.signed_duration_since(at).num_seconds()>=5
}

fn private_terminal_directory(path:&Path)->Result<PathBuf> {
    #[cfg(target_os="linux")]
    {
        use std::os::unix::fs::{DirBuilderExt,MetadataExt};
        match std::fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(())=>{},Err(error) if error.kind()==std::io::ErrorKind::AlreadyExists=>{},Err(error)=>return Err(error).context("create private terminal directory"),
        }
        let metadata=std::fs::symlink_metadata(path)?;
        anyhow::ensure!(metadata.is_dir()&&!metadata.file_type().is_symlink()&&metadata.uid()==unsafe{libc::geteuid()}&&metadata.mode()&0o777==0o700,"终端控制目录必须由当前用户独占且权限为 0700");
        let canonical=dunce::canonicalize(path)?;
        anyhow::ensure!(canonical==path,"终端控制目录不能包含符号链接");
        Ok(canonical)
    }
    #[cfg(not(target_os="linux"))]
    {let _=path;anyhow::bail!("受限终端目前仅支持 Linux")}
}

fn restricted_terminal_directory(data:&Path,runtime:&Path,work:&Path,id:&str)->Result<PathBuf> {
    anyhow::ensure!(cfg!(target_os="linux"),"受限终端目前仅支持 Linux");
    uuid::Uuid::parse_str(id).context("invalid restricted terminal session ID")?;
    let data=dunce::canonicalize(data)?;
    let runtime=dunce::canonicalize(runtime)?;
    anyhow::ensure!(!data.starts_with(work)&&!work.starts_with(&data)&&!runtime.starts_with(work),"受限终端的运行时和控制数据必须位于工作树之外");
    // Keep the existing on-disk location so restricted terminal recovery needs no migration.
    let parent=private_terminal_directory(&data.join("sandbox-chats"))?;
    let directory=private_terminal_directory(&parent.join(id))?;
    private_terminal_directory(&directory.join("tool-home"))?;
    Ok(directory)
}

fn workspace_launch(launch:&str)->Result<(String,Vec<String>,&'static str)> {
    match launch {
        "terminal"=>Ok((if cfg!(windows){"powershell"}else{"sh"}.into(),if cfg!(windows){vec!["-NoLogo".into(),"-NoProfile".into()]}else{vec![]},if cfg!(windows){"PowerShell"}else{"Shell"})),
        "codex"=>Ok(("codex".into(),vec![],"Codex")),
        "hybrid-codex"=>Ok(("velella-runtime".into(),vec![],"远程 Codex · 本地项目")),
        "claude-code"=>Ok(("claude".into(),vec![],"Claude")),
        "minimax"=>Ok(("mcode".into(),vec![],"MiniMax Code")),
        "omp"=>Ok(("omp".into(),vec![],"Oh My Pi (OMP)")),
        "gemini"=>Ok(("gemini".into(),vec![],"Gemini CLI")),
        "opencode"=>Ok(("opencode".into(),vec![],"OpenCode")),
        "kimi"=>Ok(("kimi".into(),vec![],"Kimi")),
        "zcode"=>Ok(("zcode-free".into(),vec![],"ZCode")),
        _=>anyhow::bail!("不支持的启动操作"),
    }
}

pub(super) fn wait_feedback_paste(directory:&std::path::Path)->Result<u64>{
    let log=directory.join("terminal.bin");
    let started=std::time::Instant::now();
    let mut changed=started;
    let mut bytes=std::fs::metadata(&log)?.len();
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let current=std::fs::metadata(&log)?.len();
        if current!=bytes {bytes=current;changed=std::time::Instant::now();}
        if started.elapsed()>=Duration::from_millis(1500) && changed.elapsed()>=Duration::from_millis(1000){
            return Ok(started.elapsed().as_millis() as u64);
        }
        anyhow::ensure!(started.elapsed()<Duration::from_secs(10),"Agent input has not settled; inspect the original session before submitting");
    }
}

pub(super) fn feedback_public(mut receipt:Value)->Value {
    if let Some(object)=receipt.as_object_mut(){
        object.remove("text");
        // A delivery receipt is not evidence of a model turn. No status reads
        // infer acceptance from SessionStart or upgrade an interrupted send.
        object.remove("accepted");object.remove("turn_started");
        if object.get("state")==Some(&json!("sending")){object.insert("state".into(),json!("uncertain"));}
    }
    receipt
}

fn editable_path(relative:&str)->Result<()> {
    let path=Path::new(relative);
    anyhow::ensure!(!relative.is_empty() && !path.is_absolute() && path.components().all(|c|matches!(c,std::path::Component::Normal(_))),"invalid project-relative path");
    anyhow::ensure!(!path.components().any(|c| {let name=c.as_os_str().to_string_lossy(); name.eq_ignore_ascii_case(".git") || name.eq_ignore_ascii_case(".velella")}),"runtime and Git metadata are not editable workspace files");
    Ok(())
}

// `contained_file` permits safe links inside a workspace. Editing must apply
// the metadata rule after canonicalization too, otherwise `note.md` could be
// a link to `.velella/state.db` or `.git/config`.
fn editable_canonical_path(root:&Path,path:&Path)->Result<()> {
    let root=dunce::canonicalize(root)?;
    let relative=path.strip_prefix(&root).context("file leaves the workspace")?;
    anyhow::ensure!(!relative.components().any(|c| {
        let name=c.as_os_str().to_string_lossy();
        name.eq_ignore_ascii_case(".git") || name.eq_ignore_ascii_case(".velella")
    }),"runtime and Git metadata are not editable workspace files");
    Ok(())
}

fn creatable_file(root:&Path,relative:&str)->Result<PathBuf> {
    editable_path(relative)?;
    let root=dunce::canonicalize(root)?;
    let requested=root.join(relative);
    let parent=requested.parent().context("file path has no parent")?;
    let parent=dunce::canonicalize(parent).context("parent folder is unavailable")?;
    editable_canonical_path(&root,&parent)?;
    anyhow::ensure!(parent.starts_with(&root),"file leaves the workspace");
    Ok(parent.join(requested.file_name().context("file path has no filename")?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]fn worktree_launch_uses_native_cli_without_prompt_or_permission_overrides(){
        for launch in ["terminal","codex","claude-code"]{assert!(workspace_launch(launch).is_ok());}
        assert_eq!(workspace_launch("codex").unwrap().1,Vec::<String>::new());
        assert_eq!(workspace_launch("claude-code").unwrap().0,"claude");
        let (program,args,title)=workspace_launch("zcode").unwrap();
        assert_eq!(program,"zcode-free");
        assert!(args.is_empty());
        assert_eq!(title,"ZCode");
        assert!(crate::native_launch::args("zcode",&Default::default()).unwrap().is_empty());
        assert!(crate::native_launch::args("zcode",&serde_json::from_value(json!({"model":"paid"})).unwrap()).is_err());
        assert!(workspace_launch("cmd /c anything").is_err());
    }

    #[test]
    fn terminal_execution_request_is_narrow_and_native_compatible() {
        assert!(!requested_restricted_terminal(&json!({}),"terminal").unwrap());
        assert!(!requested_restricted_terminal(&json!({"sandbox":{"mode":"native"}}),"codex").unwrap());
        assert!(requested_restricted_terminal(&json!({"sandbox":{"mode":"restricted"}}),"codex-chat").is_err());
        for sandbox in [Value::Null,json!({}),json!({"mode":"isolated"}),json!({"mode":true}),json!({"mode":"restricted","home":"/tmp"}),json!({"mode":"restricted","network":"denied"}),json!({"mode":"native","writable":[]})] {
            assert!(requested_restricted_terminal(&json!({"sandbox":sandbox}),"codex").is_err());
        }
        assert!(requested_restricted_terminal(&json!({"sandbox":{"mode":"restricted"}}),"codex").unwrap());
        for launch in ["terminal","claude-code"] {assert!(requested_restricted_terminal(&json!({"sandbox":{"mode":"restricted"}}),launch).is_err());}
    }

    #[test]
    fn launch_idempotency_binds_mode_and_restricted_recovery_stays_closed() {
        let mut session=Session{id:"s".into(),workspace_id:"w".into(),title:"Codex".into(),kind:"terminal".into(),task_id:None,native_id:None,descriptor:Descriptor{id:"s".into(),endpoint:"unused".into(),directory:PathBuf::from("/owned/controller"),spawn:SpawnRequest{sandbox:Default::default(),program:"codex".into(),args:vec![],cwd:Some("/owned/work".into()),env:HashMap::new(),adapter:Some("generic".into())}}};
        assert!(ensure_matching_launch(&session,"w","Codex",false,None).is_ok());
        assert!(ensure_matching_launch(&session,"w","Codex",true,None).is_err());
        assert!(ensure_terminal_recovery_supported(&session.descriptor).is_ok());
        session.descriptor.spawn.sandbox=crate::sandbox::Policy::Restricted{read_only:vec![],writable:vec![],network:crate::sandbox::Network::Denied,home:Some(PathBuf::from("/owned/controller/tool-home"))};
        assert!(ensure_matching_launch(&session,"w","Codex",true,None).is_ok());
        assert!(ensure_matching_launch(&session,"w","Codex",false,None).is_err());
        assert!(ensure_matching_launch(&session,"other","Codex",true,None).is_err());
        session.descriptor.spawn.env.insert("VELELLA_ROLE_CONFIG_FINGERPRINT".into(),"sha256:configured".into());
        assert!(ensure_matching_launch(&session,"w","Codex",true,Some("sha256:other")).is_err());
        assert!(ensure_terminal_recovery_supported(&session.descriptor).is_err());
    }

    #[cfg(target_os="linux")]
    #[test]
    fn restricted_terminal_control_directory_is_private_and_outside_work() {
        use std::os::unix::fs::{MetadataExt,symlink};
        let base=std::env::temp_dir().join(format!("velella-terminal-layout-test-{}",uuid::Uuid::now_v7()));
        let data=base.join("data");let work=base.join("work");
        std::fs::create_dir_all(&data).unwrap();std::fs::create_dir(&work).unwrap();
        let runtime=base.join("runtime");std::fs::write(&runtime,b"fixture").unwrap();
        let id=uuid::Uuid::now_v7().to_string();
        let directory=restricted_terminal_directory(&data,&runtime,&work,&id).unwrap();
        for path in [&directory,&directory.join("tool-home")] {assert_eq!(std::fs::metadata(path).unwrap().mode()&0o777,0o700);assert!(!path.starts_with(&work));}
        assert_eq!(restricted_terminal_directory(&data,&runtime,&work,&id).unwrap(),directory);
        assert!(restricted_terminal_directory(&work,&runtime,&work,&id).is_err());
        let unsafe_runtime=work.join("runtime");std::fs::write(&unsafe_runtime,b"fixture").unwrap();
        assert!(restricted_terminal_directory(&data,&unsafe_runtime,&work,&id).is_err());
        let aliased_id=uuid::Uuid::now_v7().to_string();symlink(&work,data.join("sandbox-chats").join(&aliased_id)).unwrap();
        assert!(restricted_terminal_directory(&data,&runtime,&work,&aliased_id).is_err());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn canonical_metadata_target_is_not_editable() {
        let root=std::env::temp_dir().join(format!("velella-workbench-path-test-{}",uuid::Uuid::now_v7()));
        let metadata=root.join(".velella");
        std::fs::create_dir_all(&metadata).unwrap();
        let target=metadata.join("state.db");
        std::fs::write(&target,b"metadata").unwrap();
        // This is the resolved path produced by a workspace-internal symlink.
        assert!(editable_canonical_path(&root,&target).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

}
