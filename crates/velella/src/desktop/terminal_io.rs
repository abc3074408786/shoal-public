use super::*;

fn input_marker_lock(directory:&Path)->Arc<Mutex<()>>{
    static LOCKS:std::sync::OnceLock<Mutex<HashMap<PathBuf,Arc<Mutex<()>>>>>=std::sync::OnceLock::new();
    LOCKS.get_or_init(Default::default).lock().unwrap().entry(directory.into()).or_default().clone()
}
pub(super) fn clear_input_marker(directory:&Path)->Result<()>{
    let lock=input_marker_lock(directory);let _guard=lock.lock().unwrap();
    match std::fs::remove_file(directory.join("terminal-input-pending.json")){Ok(())=>Ok(()),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Ok(()),Err(e)=>Err(e.into())}
}
// Keep reading the original pipe after the interactive deadline. Until its
// acknowledgement arrives, later input/control cannot overtake it. A durable,
// content-free marker also protects this ordering after a daemon interruption.
pub(super) fn ordered_host_request(desc:&Descriptor,request:&Value)->Result<Value>{
    let input=request["cmd"]=="input";
    if !input&&request["cmd"]!="control"{
        let seconds=2;
        return ipc::request_with_timeout(&desc.endpoint,request,Duration::from_secs(seconds))
    }
    let marker=desc.directory.join("terminal-input-pending.json");
    let lock=input_marker_lock(&desc.directory);
    let id=uuid::Uuid::now_v7().to_string();
    {let _guard=lock.lock().unwrap();
        anyhow::ensure!(!marker.exists(),"上一条输入仍未确认，新输入未发送；请等待原响应，或明确停止并恢复该会话。");
        if input{ipc::atomic_json(&marker,&json!({"request_id":id,"created_at":chrono::Utc::now().to_rfc3339()}))?;}
    }
    if !input{return ipc::request_with_timeout(&desc.endpoint,request,Duration::from_secs(2))}
    let endpoint=desc.endpoint.clone();let request=request.clone();
    let (tx,rx)=std::sync::mpsc::channel();
    std::thread::spawn(move||{
        let result=ipc::request(&endpoint,&request).and_then(|value|{anyhow::ensure!(value["ok"].is_boolean(),"host did not acknowledge input");Ok(value)});
        if result.is_ok(){let _guard=lock.lock().unwrap();
            let current=std::fs::read(&marker).ok().and_then(|v|serde_json::from_slice::<Value>(&v).ok());
            if current.is_some_and(|value|value["request_id"]==id){let _=std::fs::remove_file(&marker);}
        }
        let _=tx.send(result);
    });
    rx.recv_timeout(Duration::from_secs(2)).context("input acknowledgement timed out; delivery remains unresolved")?
}

pub(super) fn dispatch(service:&Arc<Mutex<Service>>,req:Value)->Result<Value>{
    let cmd=req["cmd"].as_str().unwrap_or("");
    if matches!(cmd,"workbench.hybrid.status"|"workbench.hybrid.decide"){
        let desc={let state=service.lock().unwrap();let wid=req["workspace_id"].as_str().context("workspace required")?;
            let w=state.workbench.workspace(wid)?;anyhow::ensure!(!w.archived,"workspace is archived");
            let s=state.workbench.session(wid,req["session_id"].as_str().context("session required")?)?.context("session does not belong to workspace")?;
            anyhow::ensure!(matches!(s.descriptor.spawn.adapter.as_deref(),Some("hybrid-codex"|"hybrid-orchestration-worker")),"not a hybrid session");s.descriptor};
        let config=crate::hybrid::load(&desc.directory)?;
        if cmd=="workbench.hybrid.status"{return crate::hybrid::tools::status(&config)}
        anyhow::ensure!(req["approved"].is_boolean()&&req["confirmed"]==true,"explicit local operation decision required");
        return crate::hybrid::tools::decide(&config,req["operation_id"].as_str().context("operation required")?,req["digest"].as_str().context("digest required")?,req["approved"]==true)
    }
    if cmd.starts_with("orchestration.") {return super::orchestration::dispatch(service,req);}
    if cmd.starts_with("collab.") { return super::collaboration_commands::dispatch(service,req); }
    if cmd=="workbench.feedback.observe" {return observe_feedback(service,&req);}
    if cmd=="workbench.terminal.progress" {
        let (desc,saved)={let state=service.lock().unwrap();let wid=req["workspace_id"].as_str().context("workspace required")?;
            let w=state.workbench.workspace(wid)?;anyhow::ensure!(!w.archived,"workspace is archived");
            let s=state.workbench.session(wid,req["session_id"].as_str().context("session required")?)?.context("session does not belong to workspace")?;
            (s.descriptor,s.native_id)};
        return crate::terminal_progress::read(&desc,saved.as_deref());
    }
    if cmd=="workbench.terminal.activity" {
        let desc={let state=service.lock().unwrap();let wid=req["workspace_id"].as_str().context("workspace required")?;
            let w=state.workbench.workspace(wid)?;anyhow::ensure!(!w.archived,"workspace is archived");
            let s=state.workbench.session(wid,req["session_id"].as_str().context("session required")?)?.context("session does not belong to workspace")?;
            anyhow::ensure!(matches!(s.descriptor.spawn.program.as_str(),"codex"|"claude")&&!s.descriptor.is_legacy_chat(),"session has no terminal activity");s.descriptor};
        return crate::terminal_activity::read(&desc,req["since"].as_u64().unwrap_or(0));
    }
    if cmd.starts_with("sources.") {
        let (root,projects)={let state=service.lock().unwrap();
            if matches!(cmd,"sources.save"|"sources.discover") {if let Some(id)=req["source"]["project_id"].as_str().filter(|s|!s.is_empty()){state.project(id)?;}}
            (state.root.clone(),if cmd=="sources.sync"{state.projects.clone()}else{vec![]})
        };
        if cmd=="sources.sync" {return crate::task_sources::sync_repositories(&root,&projects);}
        return crate::task_sources::dispatch(&root,&req);
    }
    // Resolve the queue first, then acquire it WITHOUT holding service state.
    // Revalidate ownership after acquiring it; queued commands never carry a
    // stale authorization snapshot across a stop, recover or task transition.
    let gate={
        let mut state=service.lock().unwrap();
        state.terminal_gate(&req)?
    };
    let _guard=gate.as_ref().map(|(_,gate)|gate.lock().unwrap());
    if cmd=="workbench.feedback.send"{return send_feedback(service,&req)}
    let validation={service.lock().unwrap().native_launch_validation(&req)?};
    if let Some(validation)=validation {validation.validate()?;}
    let (operation,key,starting)={
        let mut state=service.lock().unwrap();
        let operation=state.prepare_terminal(&req)?;
        if matches!(cmd,"task.feedback"|"task.feedback.status") {
            if let Some(Operation::Ready(value))=&operation {return Ok(value.clone());}
        }
        if operation.is_none(){
            if matches!(cmd,"task.create"|"task.cancel"|"task.resume"|"task.verify"|"task.feedback"|"task.finish"|"task.commit"|"task.pr"|"terminal.shell"|"workbench.terminal.recover"){state.terminal_epoch=state.terminal_epoch.wrapping_add(1);}
            return state.dispatch(req)
        }
        let key=gate.as_ref().map(|(key,_)|key.clone());
        if let Some(key)=&key{state.terminal_inflight.insert(key.clone());}
        let operation=operation.unwrap();
        let starting=match &operation{Operation::Start{desc,..}=>Some(format!("starting:{}",desc.id)),_=>None};
        if let Some(key)=&starting{state.terminal_inflight.insert(key.clone());}
        (operation,key,starting)
    };
    let completion=operation.completion();
    let response=operation.run();
    if key.is_some()||completion.is_some()||starting.is_some(){
        let mut state=service.lock().unwrap();
        if let Some(key)=&starting{state.terminal_inflight.remove(key);}
        if let Some(key)=&key{state.terminal_inflight.remove(key);if key.starts_with("task:"){state.terminal_epoch=state.terminal_epoch.wrapping_add(1);}}
        if response.is_ok()||matches!(&completion,Some(Completion::Task(_))){if let Some(completion)=completion{completion.apply(&mut state)?;}}
        else if let Some(uncertain)=response.as_ref().err().and_then(|error|error.downcast_ref::<UncertainInput>()){
            Completion::Uncertain(uncertain.task.clone()).apply(&mut state)?;
        }
    }
    // Mutating operations are issued exactly once, even after an ambiguous
    // timeout. Reads retain the caller's absolute output cursor.
    response.with_context(||format!("{cmd} failed (input is never automatically resent)"))
}

fn observe_feedback(service:&Arc<Mutex<Service>>,req:&Value)->Result<Value> {
    let workspace=req["workspace_id"].as_str().context("workspace required")?;
    let sid=req["session_id"].as_str().context("session required")?;
    let id=uuid::Uuid::parse_str(req["request_id"].as_str().context("request ID required")?)?.to_string();
    let (receipt,desc)={let state=service.lock().unwrap();
        let w=state.workbench.workspace(workspace)?;anyhow::ensure!(!w.archived,"workspace is archived");
        let receipt=state.workbench.feedback(workspace,&id)?.context("feedback receipt not found")?;
        anyhow::ensure!(receipt["session_id"]==sid,"receipt belongs to another session");
        let session=state.workbench.session(workspace,sid)?.context("session does not belong to workspace")?;
        (receipt,session.descriptor)
    };
    let submission=match (receipt["submission_host_pid"].as_u64(),receipt["input_sequence"].as_u64()) {
        (Some(pid),Some(sequence)) if receipt["draft"]!=true => crate::submission_receipts::read(&desc,&id,pid,sequence)
            .unwrap_or_else(|_|json!({"state":"unconfirmed","turn_started_confirmed":false,"reason":"evidence_unavailable_or_mismatched"})),
        _=>json!({"state":"unsupported","turn_started_confirmed":false,"reason":"receipt_has_no_submission_tracking"}),
    };
    Ok(json!({"ok":true,"request_id":id,"receipt":workbench_commands::feedback_public(receipt),"submission":submission,
        "turn_started_confirmed":submission["turn_started_confirmed"]==true}))
}

fn send_feedback(service:&Arc<Mutex<Service>>,req:&Value)->Result<Value>{
    for flag in ["draft","initial"] {
        anyhow::ensure!(req.get(flag).is_none_or(Value::is_boolean),"{flag} must be a boolean");
    }
    let draft=req["draft"]==true;
    let initial=req["initial"]==true;
    let requested_sequence=req.get("expected_input_sequence").map(|v|v.as_u64().context("expected_input_sequence must be an unsigned integer")).transpose()?;
    anyhow::ensure!(!initial||requested_sequence==Some(0),"initial feedback requires expected_input_sequence 0");
    let workspace=req["workspace_id"].as_str().context("workspace required")?;
    let sid=req["session_id"].as_str().context("Agent session required")?;
    let request_id=uuid::Uuid::parse_str(req["request_id"].as_str().context("request ID required")?)?.to_string();
    let text=req["text"].as_str().context("feedback text required")?;
    anyhow::ensure!(!text.trim().is_empty()&&text.len()<=65536&&text.chars().all(|c|!c.is_control()||matches!(c,'\n'|'\t')),"feedback contains unsupported control characters or is too large");
    let target={let mut state=service.lock().unwrap();
        // This path only reconciles durable receipts; it performs no host I/O.
        state.workbench_command(json!({"cmd":"workbench.feedback.status","workspace_id":workspace,"request_id":request_id}))?;
        if let Some(receipt)=state.workbench.feedback(workspace,&request_id)?{
            anyhow::ensure!(receipt["session_id"]==sid&&receipt["text"]==text&&receipt["source"]==req["source"]
                &&(receipt["draft"]==true)==draft&&(receipt["initial"]==true)==initial
                // Old source-only draft receipts did not store their sequence.
                // They remain read-only replays, never another delivery.
                &&(receipt.get("expected_input_sequence").is_none()&&!initial||receipt["expected_input_sequence"]==json!(requested_sequence)),
                "request ID belongs to different feedback");
            return Ok(json!({"ok":true,"receipt":workbench_commands::feedback_public(receipt)}))
        }
        state.workbench.sessions(Some(workspace))?.into_iter().find(|s|s.id==sid).context("Agent does not belong to this workspace")?
    };
    let desc=&target.descriptor;
    anyhow::ensure!(matches!(desc.spawn.program.as_str(),"claude"|"codex"),"target is not a supported Agent session");
    anyhow::ensure!(!desc.is_legacy_chat(),"standalone Codex chat has been removed; saved session data is retained");
    let initial_claim=desc.directory.join("terminal-initial-prompt-feedback.json");
    if initial {
        workbench_commands::native_prompt::validate_feedback(desc,text,draft)?;
        anyhow::ensure!(!initial_claim.exists(),"initial prompt already has a delivery receipt; inspect it instead of resending");
    }
    let status=get_status(desc)?;
    anyhow::ensure!(status.exit.is_none()&&status.input_protocol==1,"Agent session is unavailable");
    anyhow::ensure!(status.state=="waiting_input"&&!status.permission_input,"请先在 Agent 终端完成登录和授权，等待输入就绪后再放入草稿");
    let expected=requested_sequence.or(status.input_sequence).context("host cannot guard the input sequence")?;
    anyhow::ensure!(status.input_sequence==Some(expected),"终端已有新输入，任务草稿未粘贴");
    if initial {host::require_input_ready(&status,expected,Some(&request_id))?;}
    anyhow::ensure!(req["require_submission_tracking"]!=true || (!draft&&status.submission_protocol==1&&status.native_id.is_some()),
        "submission tracking is unavailable; no input was sent");
    let mut receipt=json!({"request_id":request_id,"session_id":sid,"text":text,"source":req["source"],"draft":draft,"initial":initial,"expected_input_sequence":requested_sequence,"state":"sending","created_at":chrono::Utc::now().to_rfc3339()});
    if !draft&&status.submission_protocol==1&&status.native_id.is_some() {receipt["submission_host_pid"]=json!(status.pid);}
    {let state=service.lock().unwrap();let w=state.workbench.workspace(workspace)?;anyhow::ensure!(!w.archived&&w.path.is_dir(),"workspace is archived or unavailable");state.workbench.begin_feedback(workspace,&request_id,&receipt)?;}
    let send=(||->Result<()> {
        // The durable claim also blocks a different feedback ID after an
        // ambiguous response. Only explicit manual input may follow uncertainty.
        if initial {ipc::atomic_json(&initial_claim,&json!({"request_id":request_id}))?;}
        for (value,enter) in [(format!("\u{1b}[200~{text}\u{1b}[201~"),false),(String::new(),true)] {
            if draft&&enter{break;}
            let sequence=expected.checked_add(u64::from(enter)).context("input sequence exhausted")?;
            if enter {
                receipt["paste_wait_ms"]=json!(workbench_commands::wait_feedback_paste(&desc.directory)?);
                let fresh=get_status(desc)?;
                anyhow::ensure!(fresh.pid==status.pid,"host restarted after paste; submit manually");
                host::require_input_ready(&fresh,sequence,if initial{Some(&request_id)}else{None})?;
            }
            let mut input=json!({"cmd":"workbench.terminal.input","workspace_id":workspace,"session_id":sid,"text":value,"enter":enter,"input_kind":"user","expected_input_sequence":sequence,"require_input_ready":true,"initial":initial,"request_id":request_id});
            if enter&&!receipt["submission_host_pid"].is_null() {
                input["submission"]=json!({"request_id":request_id,"prompt_sha256":crate::submission_receipts::digest(text)});
                input["expected_host_pid"]=json!(status.pid);
            }
            if initial {input["expected_host_pid"]=json!(status.pid);}
            let operation={service.lock().unwrap().prepare_terminal(&input)?.context("missing terminal input operation")?};
            let completion=operation.completion();let response=operation.run();
            {let mut state=service.lock().unwrap();
                if response.is_ok(){if let Some(completion)=completion{completion.apply(&mut state)?;}}
                else if let Some(uncertain)=response.as_ref().err().and_then(|e|e.downcast_ref::<UncertainInput>()){Completion::Uncertain(uncertain.task.clone()).apply(&mut state)?;}
                state.terminal_epoch=state.terminal_epoch.wrapping_add(1);
            }
            let response=response?;
            if initial || !receipt["submission_host_pid"].is_null() {anyhow::ensure!(response["input_sequence"].as_u64()==sequence.checked_add(1),"host did not acknowledge the guarded input sequence");}
            receipt["input_sequence"]=json!(sequence.checked_add(1));
        }
        // Submitted means acknowledged PTY Enter, not an observed model turn.
        receipt["state"]=json!(if draft{"drafted"}else{"submitted"});Ok(())
    })();
    if let Err(error)=send{receipt["state"]=json!("uncertain");receipt["error"]=json!(format!("{error:#}"));}
    service.lock().unwrap().workbench.finish_feedback(workspace,&request_id,&receipt)?;
    Ok(json!({"ok":true,"receipt":workbench_commands::feedback_public(receipt)}))
}

enum Operation{
    Task{context:task_execution::TaskExecution,task:Arc<Mutex<Task>>,request:Value},
    Ready(Value),
    Start{executable:PathBuf,desc:Descriptor,response:Value,completion:Option<Completion>},
    Output(Descriptor,u64),
    Host{desc:Descriptor,request:Value,typed:bool,invalidate:Option<String>},
    Stop{descriptors:Vec<Descriptor>,completion:Completion},
    Sessions(Vec<crate::workbench::Session>,std::collections::HashSet<String>),
    LegacySessions(Vec<Descriptor>),
}
#[derive(Clone)]
enum Completion{Invalidate(String),Uncertain(String),Cancel(String),Stopped(String,String),Recovered(String,String),Task(Arc<Mutex<Task>>)}
#[derive(Debug)]
struct UncertainInput{task:String,cause:String}
impl std::fmt::Display for UncertainInput{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{write!(f,"terminal input delivery is uncertain; automatic execution paused: {}",self.cause)}}
impl std::error::Error for UncertainInput{}
impl Completion{
    fn apply(self,state:&mut Service)->Result<()>{let cancel=matches!(&self,Self::Cancel(_));let uncertain=matches!(&self,Self::Uncertain(_));match self{
        Self::Invalidate(id)|Self::Uncertain(id)|Self::Cancel(id)=>{
            let index=state.tasks.iter().position(|t|t.id==id).context("task not found")?;
            let mut task=state.tasks[index].clone();
            if cancel{task.phase=Phase::Cancelled;task.reason="已停止；工作目录与改动保留".into();}
            else if uncertain{task.verified=false;task.phase=Phase::NeedsAttention;task.reason="终端输入发送结果未确认，已暂停自动执行；请核对原会话，不会自动重发。".into();}
            else{if !task.verified{return Ok(())}task.verified=false;}
            state.tasks[index]=task.clone();state.persist(&task)?;
        },
        Self::Stopped(workspace,id)=>{state.workbench.record_event(&workspace,"terminal.stop_requested",json!({"session_id":id}))?;},
        Self::Recovered(workspace,id)=>{state.workbench.record_event(&workspace,"terminal.recovered",json!({"session_id":id}))?;},
        Self::Task(task)=>{let task=task.lock().unwrap().clone();let current=state.tasks.iter_mut().find(|t|t.id==task.id).context("task not found")?;*current=task.clone();state.persist(&task)?;},
    }Ok(())}
}
pub(super) fn short_status(desc:&Descriptor)->Option<Status>{
    if desc.is_legacy_chat(){return None}
    if host::confirmed_dead(&desc.directory){
        return std::fs::read(desc.directory.join("status.json")).ok().and_then(|v|serde_json::from_slice::<Status>(&v).ok()).map(|mut s|{
            if s.exit.is_none(){s.state="host_lost".into();s.input_protocol=0;}
            s
        })
    }
    ipc::request_with_timeout(&desc.endpoint,&json!({"cmd":"status"}),Duration::from_millis(250)).ok().filter(|v|v["ok"]==true).and_then(|v|serde_json::from_value(v["status"].clone()).ok())
}
pub(super) fn session_row(s:&crate::workbench::Session,starting:bool)->Value {
    let status=short_status(&s.descriptor);
    let recoverable=s.kind=="terminal"&&s.task_id.is_none()&&!starting&&s.descriptor.spawn.adapter.as_deref()!=Some("hybrid-orchestration-worker")&&host::confirmed_dead(&s.descriptor.directory)&&workbench_commands::ensure_terminal_recovery_supported(&s.descriptor).is_ok();
    let native_id=crate::terminal_restore::native_id(&s.descriptor,s.native_id.as_deref()).or_else(||s.native_id.clone());
    json!({"id":s.id,"title":s.title,"kind":s.kind,"hybrid":matches!(s.descriptor.spawn.adapter.as_deref(),Some("hybrid-codex"|"hybrid-orchestration-worker")),"program":s.descriptor.spawn.program,"native_chat":s.descriptor.is_legacy_chat(),"execution_mode":workbench_commands::session_execution_mode(&s.descriptor.spawn.sandbox),"task_id":s.task_id,"native_id":native_id,"status":status,"recoverable":recoverable,"restore_selection_required":!starting&&crate::terminal_restore::needs_selection(s),"auto_recoverable":recoverable&&(workbench_commands::automatic_shell_recovery_allowed(s)||crate::terminal_restore::automatic_allowed(s)),"open_recoverable":recoverable&&crate::terminal_restore::open_allowed(s)})
}
impl Operation{
    fn completion(&self)->Option<Completion>{match self{Self::Task{task,..}=>Some(Completion::Task(task.clone())),Self::Host{invalidate:Some(id),..}=>Some(Completion::Invalidate(id.clone())),Self::Stop{completion,..}=>Some(completion.clone()),Self::Start{completion,..}=>completion.clone(),_=>None}}
    fn run(self)->Result<Value>{match self{
        Self::Task{context,task,request}=>{
            let mut task=task.lock().unwrap();let before=serde_json::to_vec(&*task)?;let result=context.command(&mut task,&request);
            if let Err(error)=&result{if task.phase.active()&&serde_json::to_vec(&*task)?!=before{task.phase=Phase::NeedsRecovery;task.verified=false;task.reason=format!("执行未确认，请检查原会话：{error:#}");}}
            result
        },
        Self::Ready(value)=>Ok(value),
        Self::Start{executable,desc,response,..}=>{start_host(&executable,&desc)?;Ok(response)},
        Self::Output(desc,since)=>host::read_output(&desc,since),
        Self::Host{desc,request,typed,invalidate}=>{
            if typed{anyhow::ensure!(short_status(&desc).is_some_and(|s|s.input_protocol==1),"session host does not support typed input or is unresponsive; input was not sent");}
            let response=ordered_host_request(&desc,&request).map_err(|error|match invalidate{Some(task)=>anyhow::Error::new(UncertainInput{task,cause:error.to_string()}),None=>error})?;
            anyhow::ensure!(response["ok"]==true,"{}",response["error"].as_str().unwrap_or("session operation failed"));Ok(response)
        },
        Self::Stop{descriptors,..}=>{for desc in descriptors{ipc::atomic_json(&desc.directory.join("terminal-stop-requested.json"),&json!({"requested":true}))?;stop_host(&desc)?;}Ok(json!({"ok":true}))},
        Self::Sessions(sessions,inflight)=>{
            let rows:Vec<_>=sessions.iter().map(|s|session_row(s,inflight.contains(&format!("starting:{}",s.id)))).collect();
            Ok(json!({"ok":true,"sessions":rows}))
        },
        Self::LegacySessions(sessions)=>Ok(json!({"ok":true,"terminals":sessions.iter().map(|d|json!({"id":d.id,"status":short_status(d)})).collect::<Vec<_>>()})),
    }}
}
impl Service{
    fn terminal_gate(&mut self,req:&Value)->Result<Option<(String,Arc<Mutex<()>>)>>{
        let cmd=req["cmd"].as_str().unwrap_or("");
        if cmd.ends_with(".output")||cmd=="workbench.sessions"||cmd=="workspace.list"{return Ok(None)}
        let key=if cmd=="workbench.create" {
            req.get("request_id").map(|id|->Result<_>{Ok(format!("creation:{}",uuid::Uuid::parse_str(id.as_str().context("request_id must be a UUID string")?)?))}).transpose()?
        }else if cmd=="workbench.terminal.open" {req["workspace_id"].as_str().map(|id|format!("workspace:{id}"))}
        else if let Some(id)=req["task_id"].as_str(){Some(format!("task:{id}"))}
        else if let Some(id)=req["session_id"].as_str(){
            if let Some(workspace)=req["workspace_id"].as_str(){
                self.workbench.session(workspace,id)?.map(|s|s.task_id.as_ref().map(|t|format!("task:{t}")).unwrap_or_else(||format!("session:{id}")))
            }else{Some(format!("session:{id}"))}
        }else if cmd=="workbench.update"{req["workspace_id"].as_str().map(|id|format!("workspace:{id}"))}else{None};
        Ok(key.map(|key|{let gate=self.terminal_gates.entry(key.clone()).or_default().clone();(key,gate)}))
    }
    fn prepare_terminal(&mut self,req:&Value)->Result<Option<Operation>>{
        let cmd=req["cmd"].as_str().unwrap_or("");
        if matches!(cmd,"task.feedback"|"task.feedback.status") {
            let id=req["task_id"].as_str().context("task required")?;
            let task=self.tasks.iter().find(|t|t.id==id).context("task not found")?;
            if let Some(response)=task_execution::TaskExecution::feedback_response(&self.workbench,task,req)? {
                return Ok(Some(Operation::Ready(response)));
            }
        }
        if !cmd.ends_with(".output"){if let Some(id)=req["session_id"].as_str(){anyhow::ensure!(!self.terminal_inflight.contains(&format!("starting:{id}")),"session is still starting; wait for its original launch to complete");}}
        if matches!(cmd,"terminal.shell"|"task.resume"|"task.verify"|"task.feedback"|"task.finish"){
            let id=req["task_id"].as_str().context("task required")?;
            let task=self.tasks.iter().find(|t|t.id==id).context("task not found")?.clone();
            self.validate_task_execution(&task,req)?;
            return Ok(Some(Operation::Task{context:self.execution(),task:Arc::new(Mutex::new(task)),request:req.clone()}))
        }
        if matches!(cmd,"workbench.terminal.open"|"workbench.terminal.recover"){
            self.workbench.sync_legacy(&self.projects,&self.tasks)?;
            let (desc,response)=self.prepare_workbench_launch(req)?;
            let completion=if cmd.ends_with(".recover"){Some(Completion::Recovered(req["workspace_id"].as_str().unwrap().into(),req["session_id"].as_str().unwrap().into()))}else{None};
            return Ok(Some(match desc{Some(desc)=>Operation::Start{executable:self.executable.clone(),desc,response,completion},None=>Operation::Ready(response)}))
        }
        if cmd=="task.cancel"{
            let id=req["task_id"].as_str().context("task required")?;
            let task=self.tasks.iter().find(|t|t.id==id).context("task not found")?;
            return Ok(Some(Operation::Stop{descriptors:[task.command.clone(),task.agent.clone(),task.shell.clone()].into_iter().flatten().collect(),completion:Completion::Cancel(id.into())}))
        }
        if cmd=="workbench.terminal.stop"{
            anyhow::ensure!(req["confirmed"]==true,"explicit process stop confirmation required");
            let id=req["workspace_id"].as_str().context("workspace required")?;
            let w=self.workbench.workspace(id)?;anyhow::ensure!(!w.archived,"restore the archived workspace before using it");
            let s=self.workbench.sessions(Some(id))?.into_iter().find(|s|Some(s.id.as_str())==req["session_id"].as_str()).context("session does not belong to workspace")?;
            if let Some(tid)=s.task_id{return self.prepare_terminal(&json!({"cmd":"task.cancel","task_id":tid}))}
            return Ok(Some(Operation::Stop{descriptors:vec![s.descriptor],completion:Completion::Stopped(id.into(),s.id)}))
        }
        if cmd=="workbench.sessions"{
            let id=req["workspace_id"].as_str().context("workspace required")?;
            self.workbench.sync_legacy(&self.projects,&self.tasks)?;
            self.workbench.workspace(id)?;
            return Ok(Some(Operation::Sessions(self.workbench.sessions(Some(id))?,self.terminal_inflight.clone())))
        }
        if cmd=="workspace.list"{return Ok(Some(Operation::LegacySessions(self.project(req["project_id"].as_str().context("project required")?)?.terminals)))}
        let (prefix,op)=match cmd.rsplit_once('.'){
            Some((prefix,op)) if ["workbench.terminal","workspace","terminal"].contains(&prefix)&&["output","input","resize","control"].contains(&op)=>(prefix,op),
            _=>return Ok(None),
        };
        if prefix=="workbench.terminal"{
            let id=req["workspace_id"].as_str().context("workspace required")?;
            let w=self.workbench.workspace(id)?;
            self.project(&w.project_id)?;
            anyhow::ensure!(!w.archived,"restore the archived workspace before using it");
            anyhow::ensure!(w.path.is_dir(),"workspace folder is unavailable");
            let sid=req["session_id"].as_str().context("session required")?;
            let s=self.workbench.session(id,sid)?.context("session does not belong to workspace")?;
            anyhow::ensure!(!s.descriptor.is_legacy_chat(),"native chat has no terminal output");
            if op=="output"{return Ok(Some(Operation::Output(s.descriptor,req["since"].as_u64().unwrap_or(0))))}
            if let Some(tid)=s.task_id{
                let task=self.tasks.iter().find(|t|t.id==tid).context("task not found")?;
                let target=if task.agent.as_ref().is_some_and(|d|d.id==sid){"agent"}else if task.shell.as_ref().is_some_and(|d|d.id==sid){"shell"}else if task.command.as_ref().is_some_and(|d|d.id==sid){"command"}else{anyhow::bail!("historical task session is read-only")};
                let mut forward=req.clone();forward["cmd"]=json!(format!("terminal.{op}"));forward["task_id"]=json!(tid);forward["target"]=json!(target);return self.prepare_terminal(&forward)
            }
            if op=="input"{self.ensure_workspace_writable(&w)?;}
            let mut forward=req.clone();forward["cmd"]=json!(op);forward["automated"]=json!(false);
            if op=="input"&&req["input_kind"]=="pointer"{anyhow::ensure!(req["enter"]!=true&&crate::terminal_input::is_pointer_report(req["text"].as_str().unwrap_or("")),"invalid terminal pointer report");forward["input_kind"]=json!("legacy");}
            return Ok(Some(Operation::Host{desc:s.descriptor,request:forward,typed:false,invalidate:None}))
        }
        if prefix=="workspace"{
            let project=self.project(req["project_id"].as_str().context("project required")?)?;
            if op=="input"&&project.direct{anyhow::ensure!(!self.tasks.iter().any(|t|t.project_id==project.id&&!matches!(t.phase,Phase::Queued|Phase::Cancelled|Phase::Delivered)),"此文件夹正在处理任务，请先结束或停止任务再使用工作区终端");}
            let desc=project.terminals.iter().find(|d|Some(d.id.as_str())==req["session_id"].as_str()).context("workspace terminal not found")?.clone();
            if op=="output"{return Ok(Some(Operation::Output(desc,req["since"].as_u64().unwrap_or(0))))}
            let mut forward=req.clone();forward["cmd"]=json!(op);forward["automated"]=json!(false);
            return Ok(Some(Operation::Host{desc,request:forward,typed:false,invalidate:None}))
        }
        let index=self.tasks.iter().position(|t|Some(t.id.as_str())==req["task_id"].as_str()).context("task not found")?;
        let task=self.tasks[index].clone();
        let protocol=op=="input"&&(req["input_kind"]=="protocol"||req["input_kind"]=="pointer");
        if op=="input"&&req["input_kind"]=="user"&&req["target"]!="shell"{anyhow::ensure!(task.phase!=Phase::Verifying,"verification is in progress; wait for it before continuing the Agent");}
        if op=="input"&&req["input_kind"]!="protocol"{anyhow::ensure!(self.folder_available(&task),"该文件夹还有未结束的任务，请先结束或停止它");}
        let desc=if req["target"]=="command"{&task.command}else if req["target"]=="shell"{&task.shell}else{&task.agent}.as_ref().context("terminal not started")?.clone();
        if op=="output"{return Ok(Some(Operation::Output(desc,req["since"].as_u64().unwrap_or(0))))}
        let mut forward=req.clone();forward["cmd"]=json!(op);
        if op=="input"{
            forward["automated"]=json!(false);
            if protocol{let text=req["text"].as_str().unwrap_or("");anyhow::ensure!(req["enter"]!=true&&if req["input_kind"]=="pointer"{crate::terminal_input::is_pointer_report(text)}else{crate::terminal_input::is_protocol_reply(text)},"invalid terminal non-driving input");if req["input_kind"]=="pointer"{forward["input_kind"]=json!("legacy");}}
        }
        Ok(Some(Operation::Host{desc,request:forward,typed:op=="input"&&req.get("input_kind").is_some(),invalidate:if op=="input"&&!protocol{Some(task.id)}else{None}}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::{Session,Workspace};
    use std::sync::mpsc;

    fn fixture()->(Arc<Mutex<Service>>,Descriptor){
        let tmp=std::env::temp_dir().join(format!("velella-stability-{}",uuid::Uuid::now_v7()));
        let root=tmp.join("data");std::fs::create_dir_all(&root).unwrap();
        let project_root=tmp.join("project");std::fs::create_dir_all(&project_root).unwrap();
        let workbench=crate::workbench::Workbench::open(&root).unwrap();
        let project=Project{id:"p".into(),path:project_root.clone(),name:"fixture".into(),setup:String::new(),default_base:None,setup_default:SetupPolicy::Skip,direct:true,terminals:vec![]};
        workbench.put_workspace(&Workspace{id:"w".into(),project_id:"p".into(),name:"w".into(),path:project_root,kind:"folder".into(),archived:false,favorite:false,position:0,parent_workspace_id:None}).unwrap();
        let directory=tmp.join("session");std::fs::create_dir_all(&directory).unwrap();
        let desc=Descriptor{id:"s".into(),endpoint:ipc::endpoint(&root,"slow"),directory,spawn:SpawnRequest{ sandbox: Default::default(),program:"fixture".into(),args:vec![],cwd:None,env:HashMap::new(),adapter:Some("generic".into())}};
        workbench.put_session(&Session{id:"s".into(),workspace_id:"w".into(),title:"slow".into(),kind:"terminal".into(),task_id:None,native_id:None,descriptor:desc.clone()}).unwrap();
        let service=Arc::new(Mutex::new(Service{root:root.clone(),executable:std::env::current_exe().unwrap(),projects:vec![project],tasks:vec![],project_locks:HashMap::new(),workbench,capabilities:capabilities::Capabilities::open(&root).unwrap(),terminal_gates:HashMap::new(),terminal_inflight:std::collections::HashSet::new(),terminal_epoch:0,collaboration:Arc::new(Mutex::new(None))}));
        (service,desc)
    }

    fn native_help_fixture(directory:&Path,program:&str,supported:bool,wait:bool) {
        std::fs::create_dir_all(directory).unwrap();
        #[cfg(windows)]
        {
            let waiting=if wait {":wait\r\nif not exist \"%~dp0help-release\" goto wait\r\n"}else{""};
            let help=if supported {"echo   --model\r\necho   -c\r\necho   --effort\r\n"}else{"echo   --help\r\n"};
            std::fs::write(directory.join(format!("{program}.cmd")),format!("@echo off\r\nif \"%~1\"==\"resume\" shift\r\nif not \"%~1\"==\"--help\" exit /b 9\r\necho probe>>\"%~dp0help-probes\"\r\n{waiting}{help}exit /b 0\r\n")).unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let waiting=if wait {"while [ ! -f \"${0%/*}/help-release\" ]; do sleep 0.01; done\n"}else{""};
            let help=if supported {"  --model\\n  -c\\n  --effort\\n"}else{"  --help\\n"};
            let file=directory.join(program);
            std::fs::write(&file,format!("#!/bin/sh\n[ \"$1\" != resume ] || shift\n[ \"$1\" = --help ] || exit 9\nprintf 'probe\\n' >> \"${{0%/*}}/help-probes\"\n{waiting}printf '{help}'\n")).unwrap();
            std::fs::set_permissions(file,std::fs::Permissions::from_mode(0o700)).unwrap();
        }
    }

    #[test]
    fn permission_defaults_bind_new_descriptors_and_replay_identity() {
        let (service,base)=fixture();
        for (launch,mode) in [("codex","ask"),("claude-code","accept-edits"),("gemini","full-access"),("opencode","ask"),("omp","write"),("kimi","auto"),("zcode","guarded")] {
            let id=uuid::Uuid::now_v7().to_string();
            let req=json!({"cmd":"workbench.terminal.open","workspace_id":"w","request_id":id,"launch":launch,"permission_mode":mode});
            let mut state=service.lock().unwrap();
            let desc=state.prepare_workbench_launch(&req).unwrap().0.unwrap();
            assert_eq!(crate::terminal_permissions::saved(&desc.spawn.env),mode);
            let expected=crate::terminal_permissions::plan(launch,mode).unwrap();
            assert!(expected.args.is_empty()||desc.spawn.args.windows(expected.args.len()).any(|args|args==expected.args));
            for (key,value) in expected.env {assert_eq!(desc.spawn.env[&key],value);}
            let original=serde_json::to_value(&desc).unwrap();
            assert!(state.prepare_workbench_launch(&req).unwrap().0.is_none());
            let mut changed=req.clone();changed["permission_mode"]=json!("inherit");
            assert!(state.prepare_workbench_launch(&changed).err().unwrap().to_string().contains("另一权限模式"));
            let recover=json!({"cmd":"workbench.terminal.recover","workspace_id":"w","session_id":id,"permission_mode":"inherit"});
            assert!(state.native_launch_validation(&recover).is_err());
            assert!(state.prepare_workbench_launch(&recover).is_err());
            assert_eq!(serde_json::to_value(state.workbench.session("w",&id).unwrap().unwrap().descriptor).unwrap(),original);
        }
        drop(service);std::fs::remove_dir_all(base.directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn invalid_permission_modes_and_policy_conflicts_have_no_session_effects() {
        let (service,base)=fixture();
        for extra in [json!({"launch":"terminal","permission_mode":"ask"}),json!({"launch":"codex","permission_mode":null}),
            json!({"launch":"codex","permission_mode":"yolo;bad"}),json!({"launch":"zcode","permission_mode":"plan"}),
            json!({"launch":"codex","permission_mode":"full-access","sandbox":{"mode":"restricted"}}),
            json!({"launch":"omp","permission_mode":"yolo","collaboration":true}),
            json!({"launch":"codex","permission_mode":"auto","role_config":{}})] {
            let mut req=json!({"cmd":"workbench.terminal.open","workspace_id":"w","request_id":uuid::Uuid::now_v7().to_string()});
            req.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            let mut state=service.lock().unwrap();
            assert!(state.native_launch_validation(&req).is_err());
            assert!(state.prepare_workbench_launch(&req).is_err());
            assert_eq!(state.workbench.sessions(Some("w")).unwrap().len(),1);
        }
        drop(service);std::fs::remove_dir_all(base.directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn permission_capability_failure_precedes_registration() {
        let (service,base)=fixture();
        let bin=base.directory.parent().unwrap().join("native-bin");
        native_help_fixture(&bin,"omp",false,false);
        service.lock().unwrap().executable=bin.join("missing-runtime");
        let id=uuid::Uuid::now_v7().to_string();
        let req=json!({"cmd":"workbench.terminal.open","workspace_id":"w","request_id":id,"launch":"omp","permission_mode":"write"});
        assert!(dispatch(&service,req).unwrap_err().to_string().contains("--approval-mode"));
        assert!(service.lock().unwrap().workbench.session("w",&id).unwrap().is_none());
        drop(service);std::fs::remove_dir_all(base.directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn native_capabilities_reject_new_terminal_and_creation_before_effects() {
        for (launch,program) in [("codex","codex"),("claude-code","claude")] {
            let (service,base)=fixture();
            let bin=base.directory.parent().unwrap().join("native-bin");
            native_help_fixture(&bin,program,false,false);
            {let mut state=service.lock().unwrap();state.executable=bin.join("missing-runtime");state.projects[0].direct=false;}
            for cmd in ["workbench.terminal.open","workbench.create"] {
                let id=uuid::Uuid::now_v7().to_string();
                let req=json!({"cmd":cmd,"workspace_id":"w","project_id":"p","name":"fixture","request_id":id,"launch":launch,"launch_options":{"model":"fixture/model","effort":"high"}});
                let error=dispatch(&service,req).unwrap_err();
                assert!(error.to_string().contains("does not advertise --model"),"{error:#}");
                let state=service.lock().unwrap();
                assert!(state.workbench.session("w",&id).unwrap().is_none());
                assert!(state.workbench.creation(&id,None).unwrap().is_none());
                assert!(!state.projects[0].path.join(".velella").exists());
                assert!(state.tasks.is_empty());
            }
            assert!(bin.join("help-probes").exists());
            drop(service);std::fs::remove_dir_all(base.directory.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn native_capabilities_empty_options_never_probe() {
        let (service,base)=fixture();
        let bin=base.directory.parent().unwrap().join("native-bin");
        for program in ["codex","claude"] {native_help_fixture(&bin,program,false,false);}
        let mut state=service.lock().unwrap();state.executable=bin.join("missing-runtime");
        for launch in ["codex","claude-code","terminal"] {
            let req=json!({"cmd":"workbench.terminal.open","workspace_id":"w","launch":launch,"launch_options":{}});
            assert!(state.native_launch_validation(&req).unwrap().is_none());
            let desc=state.prepare_workbench_launch(&req).unwrap().0.unwrap();
            workbench_commands::validate_saved_launch(&desc).unwrap();
            let mut empty=desc;empty.spawn.env.insert("VELELLA_NATIVE_LAUNCH_OPTIONS".into(),"{}".into());
            workbench_commands::validate_saved_launch(&empty).unwrap();
        }
        assert!(state.native_launch_validation(&json!({"cmd":"workbench.create","launch":"codex","launch_options":{}})).unwrap().is_none());
        assert!(!bin.join("help-probes").exists());
        drop(state);drop(service);std::fs::remove_dir_all(base.directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn native_capabilities_same_id_launch_replay_never_reprobes_or_rewrites() {
        for (launch,program) in [("codex","codex"),("claude-code","claude")] {
            let (service,base)=fixture();
            let bin=base.directory.parent().unwrap().join("native-bin");
            native_help_fixture(&bin,program,true,false);
            service.lock().unwrap().executable=bin.join("missing-runtime");
            let id=uuid::Uuid::now_v7().to_string();
            let req=json!({"cmd":"workbench.terminal.open","workspace_id":"w","request_id":id,"launch":launch,"launch_options":{"model":"fixture/model","effort":"high"}});
            let validation={service.lock().unwrap().native_launch_validation(&req).unwrap().unwrap()};
            validation.validate().unwrap();
            let desc=service.lock().unwrap().prepare_workbench_launch(&req).unwrap().0.unwrap();
            assert_eq!(std::env::split_paths(&desc.spawn.env["PATH"]).next().unwrap(),bin);
            let saved=serde_json::to_value(&desc).unwrap();
            let probes=std::fs::read(bin.join("help-probes")).unwrap();
            native_help_fixture(&bin,program,false,false);
            assert_eq!(dispatch(&service,req.clone()).unwrap()["session_id"],id);
            let mut changed=req;changed["launch_options"]["effort"]=json!("low");
            assert!(dispatch(&service,changed).unwrap_err().to_string().contains("创建请求已绑定"));
            assert_eq!(std::fs::read(bin.join("help-probes")).unwrap(),probes);
            assert_eq!(serde_json::to_value(service.lock().unwrap().workbench.session("w",&id).unwrap().unwrap().descriptor).unwrap(),saved);
            assert!(!desc.directory.join("launch.json").exists());
            drop(service);std::fs::remove_dir_all(base.directory.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn native_capabilities_creation_replay_keeps_frozen_intent_without_reprobe() {
        let (service,base)=fixture();
        let bin=base.directory.parent().unwrap().join("native-bin");
        native_help_fixture(&bin,"codex",true,false);
        {let mut state=service.lock().unwrap();state.executable=bin.join("missing-runtime");state.projects[0].direct=false;}
        let id=uuid::Uuid::now_v7().to_string();
        let req=json!({"cmd":"workbench.create","project_id":"p","name":"fixture","request_id":id,"launch":"codex","launch_options":{"model":"fixture/model","effort":"high"}});
        let validation={service.lock().unwrap().native_launch_validation(&req).unwrap().unwrap()};
        validation.validate().unwrap();
        let saved={let state=service.lock().unwrap();
            let mut workspace=state.workbench.workspace("w").unwrap();workspace.id=id.clone();workspace.kind="worktree".into();state.workbench.put_workspace(&workspace).unwrap();
            let setup=SetupSnapshot{decision:SetupPolicy::Skip,script:String::new(),digest:"fixture".into(),reviewed_at:None};
            let frozen=json!({"workspace":workspace,"setup":setup,"launch_request":creation_launch::freeze(&req,&id).unwrap()});
            let mut operation=state.workbench.accept_creation(&id,&req,&frozen).unwrap();
            for next in [crate::workbench::CreationStepState::Running,crate::workbench::CreationStepState::Succeeded] {
                operation.steps[0].state=next;operation=state.workbench.save_creation(&operation).unwrap();
            }
            serde_json::to_value(creation_setup::settle_skips(&state.workbench,operation).unwrap()).unwrap()
        };
        let probes=std::fs::read(bin.join("help-probes")).unwrap();
        native_help_fixture(&bin,"codex",false,false);
        assert_eq!(dispatch(&service,req.clone()).unwrap()["workspace"]["id"],id);
        let mut changed=req;changed["launch_options"]["model"]=json!("other/model");
        assert!(dispatch(&service,changed).unwrap_err().to_string().contains("different request"));
        assert_eq!(serde_json::to_value(service.lock().unwrap().workbench.creation(&id,None).unwrap().unwrap()).unwrap(),saved);
        assert_eq!(std::fs::read(bin.join("help-probes")).unwrap(),probes);
        assert!(!service.lock().unwrap().projects[0].path.join(".velella/worktrees").exists());
        drop(service);std::fs::remove_dir_all(base.directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn native_capabilities_recovery_and_spawn_boundary_use_saved_program_and_path() {
        let (service,mut desc)=fixture();
        let bin=desc.directory.parent().unwrap().join("saved-bin");
        let other=desc.directory.parent().unwrap().join("current-bin");
        native_help_fixture(&bin,"codex",false,false);
        native_help_fixture(&other,"codex",true,false);
        desc.spawn.program="codex".into();
        desc.spawn.env.insert("PATH".into(),bin.to_string_lossy().into_owned());
        desc.spawn.env.insert("VELELLA_NATIVE_LAUNCH_OPTIONS".into(),json!({"model":"saved/model","effort":"high"}).to_string());
        ipc::atomic_json(&desc.directory.join("status.json"),&Status{pid:1234,native_id:Some(uuid::Uuid::now_v7().to_string()),..Status::default()}).unwrap();
        {let mut state=service.lock().unwrap();state.executable=other.join("missing-runtime");let mut session=state.workbench.session("w","s").unwrap().unwrap();session.descriptor=desc.clone();state.workbench.put_session(&session).unwrap();}
        let req=json!({"cmd":"workbench.terminal.recover","workspace_id":"w","session_id":"s"});
        let mut changed=req.clone();changed["launch_options"]=json!({"model":"changed"});
        assert!(dispatch(&service,changed).is_err());
        assert!(!bin.join("help-probes").exists());
        std::fs::write(desc.directory.join("terminal-input-pending.json"),b"saved marker").unwrap();
        let saved=serde_json::to_value(&desc).unwrap();
        assert!(dispatch(&service,req).unwrap_err().to_string().contains("does not advertise --model"));
        assert_eq!(std::fs::read(desc.directory.join("terminal-input-pending.json")).unwrap(),b"saved marker");
        assert_eq!(serde_json::to_value(service.lock().unwrap().workbench.session("w","s").unwrap().unwrap().descriptor).unwrap(),saved);
        assert!(start_host(&other.join("missing-runtime"),&desc).unwrap_err().to_string().contains("does not advertise --model"));
        let probes=std::fs::read(bin.join("help-probes")).unwrap();
        desc.spawn.program="custom-wrapper".into();
        assert!(start_host(&other.join("missing-runtime"),&desc).is_err());
        assert_eq!(std::fs::read(bin.join("help-probes")).unwrap(),probes);
        assert!(!other.join("help-probes").exists());
        assert!(!desc.directory.join("launch.json").exists());
        drop(service);std::fs::remove_dir_all(desc.directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn native_capabilities_probe_releases_service_but_keeps_gate_and_revalidates_workspace() {
        let (service,base)=fixture();
        let bin=base.directory.parent().unwrap().join("native-bin");
        native_help_fixture(&bin,"codex",true,true);
        service.lock().unwrap().executable=bin.join("missing-runtime");
        let id=uuid::Uuid::now_v7().to_string();
        let req=json!({"cmd":"workbench.terminal.open","workspace_id":"w","request_id":id,"launch":"codex","launch_options":{"model":"fixture/model"}});
        let gate=service.lock().unwrap().terminal_gate(&req).unwrap().unwrap().1;
        let observed=service.clone();let worker=std::thread::spawn(move||dispatch(&observed,req));
        let until=Instant::now()+Duration::from_secs(3);
        while !bin.join("help-probes").exists()&&Instant::now()<until {std::thread::sleep(Duration::from_millis(5));}
        let entered=bin.join("help-probes").exists();
        let available=if let Ok(state)=service.try_lock() {
            let mut workspace=state.workbench.workspace("w").unwrap();workspace.archived=true;state.workbench.put_workspace(&workspace).unwrap();true
        }else{false};
        let queued=gate.try_lock().is_err();
        std::fs::write(bin.join("help-release"),b"release").unwrap();
        let result=worker.join().unwrap();
        assert!(entered,"fake CLI never entered help");
        assert!(available,"help probe held the global Service mutex");
        assert!(queued,"help probe released the workspace gate");
        assert!(result.unwrap_err().to_string().contains("archived"));
        assert!(service.lock().unwrap().workbench.session("w",&id).unwrap().is_none());
        drop(service);std::fs::remove_dir_all(base.directory.parent().unwrap()).unwrap();
    }

    fn with_task(service:&Arc<Mutex<Service>>,desc:&Descriptor){
        let mut state=service.lock().unwrap();
        let task=Task{id:"t".into(),project_id:"p".into(),spec:TaskSpec::from_yaml_str("name: fixture\nagent: codex").unwrap(),phase:Phase::Review,worktree:state.projects[0].path.clone(),branch:String::new(),base_oid:String::new(),agent:Some(desc.clone()),command:None,shell:None,native_id:None,deadline:None,created_at:String::new(),reason:String::new(),verified:true,checks:vec![],retries:0,observed_turns:0,status:None,pr_url:None,allowed_commands:vec![],direct:false,delivery:None,parent_workspace_id:None,setup_snapshot:None,creation_id:None};
        state.persist(&task).unwrap();state.tasks.push(task);
        let mut session=state.workbench.sessions(Some("w")).unwrap().remove(0);session.task_id=Some("t".into());state.workbench.put_session(&session).unwrap();
    }

    #[test]
    fn lost_shell_host_is_reported_and_can_reconnect_once() {
        let (service,mut desc)=fixture();
        desc.spawn.program=if cfg!(windows){"powershell"}else{"sh"}.into();
        desc.spawn.args=if cfg!(windows){vec!["-NoLogo".into(),"-NoProfile".into()]}else{vec![]};
        ipc::atomic_json(&desc.directory.join("status.json"),&Status{pid:1234,input_protocol:1,exit:None,..Status::default()}).unwrap();
        {let state=service.lock().unwrap();let mut s=state.workbench.session("w","s").unwrap().unwrap();s.descriptor=desc.clone();state.workbench.put_session(&s).unwrap();}
        let row=| |dispatch(&service,json!({"cmd":"workbench.sessions","workspace_id":"w"})).unwrap()["sessions"][0].clone();
        let lost=row();
        assert_eq!(lost["status"]["state"],"host_lost");
        assert_eq!(lost["status"]["input_protocol"],0);
        assert_eq!(lost["auto_recoverable"],true);
        ipc::atomic_json(&desc.directory.join("terminal-auto-recovery.json"),&json!({"attempted":true})).unwrap();
        assert_eq!(row()["auto_recoverable"],false,"a failed restart must not loop after UI reload");
        drop(service);std::fs::remove_dir_all(desc.directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn shell_recovery_rearms_after_a_successful_host_and_migrates_legacy_claims() {
        let (service,mut desc)=fixture();
        desc.spawn.program=if cfg!(windows){"powershell"}else{"sh"}.into();
        desc.spawn.args=if cfg!(windows){vec!["-NoLogo".into(),"-NoProfile".into()]}else{vec![]};
        let save=|pid,sequence|ipc::atomic_json(&desc.directory.join("status.json"),&Status{pid,input_protocol:1,input_sequence:Some(sequence),exit:None,..Status::default()}).unwrap();
        {let state=service.lock().unwrap();let mut s=state.workbench.session("w","s").unwrap().unwrap();s.descriptor=desc.clone();state.workbench.put_session(&s).unwrap();}
        let row=||dispatch(&service,json!({"cmd":"workbench.sessions","workspace_id":"w"})).unwrap()["sessions"][0].clone();
        let old=(chrono::Utc::now()-chrono::Duration::seconds(60)).to_rfc3339();
        let claim=desc.directory.join("terminal-auto-recovery.json");
        save(1234,0);
        ipc::atomic_json(&claim,&json!({"attempted":true,"at":old})).unwrap();
        assert_eq!(row()["auto_recoverable"],true,"legacy claim must not permanently block a shell whose later heartbeat proves successful startup");
        let request=json!({"cmd":"workbench.terminal.recover","workspace_id":"w","session_id":"s","automatic":true});
        let restored=service.lock().unwrap().prepare_workbench_launch(&request).unwrap().0.unwrap();
        assert_eq!(restored.spawn.args,desc.spawn.args,"recovery only starts the original interactive shell, never old commands");
        assert_eq!(row()["auto_recoverable"],false,"failed launch cannot reuse the previous host status");
        save(2345,0);
        assert_eq!(row()["auto_recoverable"],false,"a new PID alone does not prove successful startup");
        ipc::atomic_json(&claim,&json!({"attempted":true,"previous_host_pid":1234,"at":old})).unwrap();
        assert_eq!(row()["auto_recoverable"],true,"a subsequent loss after sustained host heartbeats is a new recovery event");
        ipc::atomic_json(&claim,&json!({"attempted":true,"previous_host_pid":2345,"at":old})).unwrap();
        assert_eq!(row()["auto_recoverable"],false,"same host cannot rearm even when the claim is old");
        ipc::atomic_json(&claim,&json!({"attempted":true,"previous_host_pid":1234,"at":chrono::Utc::now().to_rfc3339()})).unwrap();
        save(2345,1);
        assert_eq!(row()["auto_recoverable"],true,"acknowledged user activity also proves the new host was usable");
        ipc::atomic_json(&desc.directory.join("terminal-stop-requested.json"),&json!({"requested":true})).unwrap();
        assert_eq!(row()["auto_recoverable"],false,"explicit stop must remain stopped");
        drop(service);std::fs::remove_dir_all(desc.directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn native_agent_restore_on_open_crosses_stop_only_with_bound_identity() {
        use fs2::FileExt;
        for program in ["codex","claude","zcode-free"] {
            let (service,mut desc)=fixture();
            let native=if program=="zcode-free"{"sess_original".into()}else{uuid::Uuid::now_v7().to_string()};
            desc.spawn.program=program.into();desc.spawn.args=vec!["--model".into(),"preserved".into()];desc.spawn.adapter=Some("generic".into());
            ipc::atomic_json(&desc.directory.join("status.json"),&Status{pid:1234,input_protocol:1,input_sequence:Some(4),exit:Some(1),state:"stopped".into(),..Status::default()}).unwrap();
            ipc::atomic_json(&desc.directory.join("terminal-stop-requested.json"),&json!({"requested":true})).unwrap();
            let save=|id:Option<String>|{let state=service.lock().unwrap();let mut s=state.workbench.session("w","s").unwrap().unwrap();s.descriptor=desc.clone();s.native_id=id;state.workbench.put_session(&s).unwrap();};
            let row=||dispatch(&service,json!({"cmd":"workbench.sessions","workspace_id":"w"})).unwrap()["sessions"][0].clone();
            save(None);assert_eq!(row()["open_recoverable"],false,"opening must not guess a native conversation");
            save(Some(native.clone()));assert_eq!(row()["auto_recoverable"],false,"passive polling must leave a stopped terminal stopped");assert_eq!(row()["open_recoverable"],true);
            let request=json!({"cmd":"workbench.terminal.recover","workspace_id":"w","session_id":"s","automatic":true,"on_open":true});
            service.lock().unwrap().native_launch_validation(&request).unwrap();
            assert!(service.lock().unwrap().prepare_workbench_launch(&json!({"cmd":"workbench.terminal.recover","workspace_id":"w","session_id":"s","automatic":true})).is_err());
            let owner=std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(desc.directory.join("owner.lock")).unwrap();owner.lock_exclusive().unwrap();
            assert_eq!(row()["open_recoverable"],false);assert!(service.lock().unwrap().prepare_workbench_launch(&request).is_err(),"live host ownership must prohibit a second launch");drop(owner);
            let restored=service.lock().unwrap().prepare_workbench_launch(&request).unwrap().0.unwrap();
            assert_eq!(&restored.spawn.args[..2],&[if program=="codex"{"resume"}else{"--resume"},native.as_str()]);
            assert_eq!(&restored.spawn.args[2..],&desc.spawn.args);assert!(!desc.directory.join("terminal-stop-requested.json").exists());
            assert!(service.lock().unwrap().prepare_workbench_launch(&request).is_err(),"failed launch must not retry on every reopen or reload");
            drop(service);std::fs::remove_dir_all(desc.directory.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn native_agent_restore_eligibility_keeps_the_original_identity() {
        let (service,mut desc)=fixture();
        desc.spawn.program="codex".into();desc.spawn.args.clear();desc.spawn.adapter=Some("generic".into());
        let native=uuid::Uuid::now_v7().to_string();
        ipc::atomic_json(&desc.directory.join("status.json"),&Status{pid:1234,input_protocol:1,native_id:Some(native.clone()),exit:None,..Status::default()}).unwrap();
        {let state=service.lock().unwrap();let mut s=state.workbench.session("w","s").unwrap().unwrap();s.descriptor=desc.clone();state.workbench.put_session(&s).unwrap();}
        let row=dispatch(&service,json!({"cmd":"workbench.sessions","workspace_id":"w"})).unwrap()["sessions"][0].clone();
        assert_eq!(row["auto_recoverable"],true,"opening the saved Agent must automatically resume the original session");
        assert_eq!(row["native_id"],native,"resume must use the observed provider identity, never latest");
        drop(service);std::fs::remove_dir_all(desc.directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn native_agent_restore_prepares_resume_once_without_replaying_initial_input() {
        let (service,mut desc)=fixture();let native=uuid::Uuid::now_v7().to_string();
        desc.spawn.program="codex".into();desc.spawn.args=vec!["--model".into(),"unchanged".into()];
        desc.spawn.env.insert(host::INITIAL_PROMPT_ENV.into(),r#"{"text":"DO NOT RESEND","mode":"submit"}"#.into());
        ipc::atomic_json(&desc.directory.join("status.json"),&Status{pid:1234,native_id:Some(native.clone()),input_sequence:Some(4),..Status::default()}).unwrap();
        {let state=service.lock().unwrap();let mut s=state.workbench.session("w","s").unwrap().unwrap();s.descriptor=desc.clone();state.workbench.put_session(&s).unwrap();}
        let request=json!({"cmd":"workbench.terminal.recover","workspace_id":"w","session_id":"s","automatic":true});
        let restored={service.lock().unwrap().prepare_workbench_launch(&request).unwrap().0.unwrap()};
        assert_eq!(&restored.spawn.args[..4],["resume",native.as_str(),"--model","unchanged"]);
        assert!(desc.directory.join(host::INITIAL_PROMPT_CLOSED).exists());
        assert!(service.lock().unwrap().prepare_workbench_launch(&request).is_err(),"failed launch must not loop on reload");
        ipc::atomic_json(&desc.directory.join("terminal-stop-requested.json"),&json!({"requested":true})).unwrap();
        assert!(!crate::terminal_restore::automatic_allowed(&service.lock().unwrap().workbench.session("w","s").unwrap().unwrap()));
        drop(service);std::fs::remove_dir_all(desc.directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn automatic_recovery_never_relaunches_agent_or_clean_exit() {
        let (service,mut desc)=fixture();
        let request=json!({"cmd":"workbench.terminal.recover","workspace_id":"w","session_id":"s","automatic":true});
        for (program,args,exit) in [("codex",vec![],None),(if cfg!(windows){"powershell"}else{"sh"},if cfg!(windows){vec!["-NoLogo","-NoProfile"]}else{vec![]},Some(0))] {
            desc.spawn.program=program.into();desc.spawn.args=args.into_iter().map(String::from).collect();
            ipc::atomic_json(&desc.directory.join("status.json"),&Status{pid:1234,input_protocol:1,exit,..Status::default()}).unwrap();
            let mut state=service.lock().unwrap();let mut s=state.workbench.session("w","s").unwrap().unwrap();s.descriptor=desc.clone();state.workbench.put_session(&s).unwrap();
            assert!(state.prepare_workbench_launch(&request).is_err(),"automatic recovery accepted {program} with exit {exit:?}");
        }
        drop(service);std::fs::remove_dir_all(desc.directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn retired_chat_cannot_launch_recover_or_receive_input_and_keeps_saved_data(){
        let (service,mut desc)=fixture();
        desc.spawn.program="codex".into();desc.spawn.adapter=Some("codex-chat".into());
        let history=desc.directory.join("chat.json");std::fs::write(&history,b"saved history").unwrap();
        let launch=desc.directory.join("launch.json");std::fs::write(&launch,serde_json::to_vec(&desc).unwrap()).unwrap();
        {let state=service.lock().unwrap();let mut session=state.workbench.session("w","s").unwrap().unwrap();session.kind="agent".into();session.descriptor=desc.clone();state.workbench.put_session(&session).unwrap();}
        assert!(host::run(&launch).unwrap_err().to_string().contains("removed"));
        for command in ["workbench.terminal.input","workbench.terminal.output","workbench.terminal.resize","workbench.terminal.control","workbench.terminal.recover","workbench.chat.snapshot","workbench.chat.interrupt","workbench.chat.respond"] {
            let req=json!({"cmd":command,"workspace_id":"w","session_id":"s","text":"must not send"});
            assert!(dispatch(&service,req.clone()).is_err(),"{command}");
            assert!(service.lock().unwrap().workbench_command(req).is_err(),"direct {command}");
        }
        let feedback=json!({"cmd":"workbench.feedback.send","workspace_id":"w","session_id":"s","request_id":uuid::Uuid::now_v7().to_string(),"text":"must not send"});
        assert!(dispatch(&service,feedback.clone()).is_err());
        assert!(service.lock().unwrap().workbench_command(feedback).is_err());
        for sandbox in [json!({"mode":"native"}),json!({"mode":"restricted"})] {
            assert!(dispatch(&service,json!({"cmd":"workbench.terminal.open","workspace_id":"w","launch":"codex-chat","sandbox":sandbox})).is_err());
        }
        let listed=dispatch(&service,json!({"cmd":"workbench.sessions","workspace_id":"w"})).unwrap();
        assert_eq!(listed["sessions"].as_array().unwrap().len(),1);
        assert_eq!(listed["sessions"][0]["native_chat"],true);
        assert_eq!(listed["sessions"][0]["recoverable"],false);
        assert!(listed["sessions"][0]["status"].is_null());
        assert_eq!(std::fs::read(history).unwrap(),b"saved history");
        assert!(!desc.directory.join("owner.lock").exists());
        assert!(!desc.directory.join("terminal.bin").exists());
    }

    #[test]
    fn collaboration_launch_scopes_data_discovery_to_native_members(){
        let (service,_)=fixture();
        let mut state=service.lock().unwrap();
        for launch in ["codex","claude-code"] {
            let (ordinary,_)=state.prepare_workbench_launch(&json!({"cmd":"workbench.terminal.open","workspace_id":"w","launch":launch})).unwrap();
            assert!(!ordinary.unwrap().spawn.env.contains_key("VELELLA_DATA_DIR"));
            let (member,_)=state.prepare_workbench_launch(&json!({"cmd":"workbench.terminal.open","workspace_id":"w","launch":launch,"collaboration":true})).unwrap();
            let member=member.unwrap();
            assert_eq!(member.spawn.env.get("VELELLA_DATA_DIR"),Some(&state.root.to_string_lossy().into_owned()));
            assert_eq!(member.spawn.env.get("VELELLA_SESSION_ID"),Some(&member.id));
            assert!(!member.spawn.args.iter().any(|arg| matches!(arg.as_str(), "--ask-for-approval" | "--model" | "--dangerously-skip-permissions")));
            let request=json!({"cmd":"workbench.terminal.open","workspace_id":"w","launch":launch,"request_id":member.id});
            assert!(state.prepare_workbench_launch(&request).is_err());
        }
    }

    #[test]
    fn collaboration_rejects_external_receipts_and_dead_launch_replay(){
        let (service,_)=fixture();
        assert!(dispatch(&service,json!({"cmd":"collab.input.accepted"})).unwrap_err().to_string().contains("internal"));
        let id=uuid::Uuid::now_v7().to_string();
        service.lock().unwrap().prepare_workbench_launch(&json!({"cmd":"workbench.terminal.open","workspace_id":"w","launch":"codex","collaboration":true,"request_id":id})).unwrap();
        let request=json!({"cmd":"collab.start","workspace_id":"w","launch":"codex","request_id":id});
        assert!(dispatch(&service,request).unwrap_err().to_string().contains("host is unavailable"));
    }

    #[test]
    fn collaboration_flow_binds_real_files_and_does_not_accept_rejected_input() {
        struct Peer { endpoint:String, thread:Option<std::thread::JoinHandle<()>>, _owner:File }
        impl Drop for Peer {
            fn drop(&mut self){
                let _=ipc::request(&self.endpoint,&json!({"cmd":"fixture.stop"}));
                if let Some(thread)=self.thread.take(){thread.join().unwrap();}
            }
        }
        let (service,base)=fixture();
        let mut peers=Vec::new();let mut sessions=Vec::new();
        for name in ["lead","writer","reviewer"] {
            let id=uuid::Uuid::now_v7().to_string();
            let mut desc=base.clone();desc.id=id.clone();
            desc.directory=base.directory.parent().unwrap().join(name);
            std::fs::create_dir_all(&desc.directory).unwrap();
            let state=service.lock().unwrap();
            desc.endpoint=ipc::endpoint(&state.root,&id);
            desc.spawn.program="codex".into();desc.spawn.cwd=Some(state.projects[0].path.to_string_lossy().into_owned());
            state.workbench.put_session(&Session{id:id.clone(),workspace_id:"w".into(),title:name.into(),kind:"terminal".into(),task_id:None,native_id:None,descriptor:desc.clone()}).unwrap();
            drop(state);
            let owner=OpenOptions::new().create(true).truncate(false).read(true).write(true).open(desc.directory.join("owner.lock")).unwrap();
            owner.try_lock_exclusive().unwrap();
            let listener=ipc::listen(&desc.endpoint).unwrap();
            let thread=std::thread::spawn(move||loop {
                let mut stop=false;
                ipc::handle(listener.accept().unwrap(),|req|{
                    if req["cmd"]=="fixture.stop"{stop=true;return Ok(json!({"ok":true}));}
                    if req["cmd"]=="input"{return Ok(json!({"ok":false,"error":"permission changed before input"}));}
                    Ok(json!({"ok":true,"status":Status{state:"waiting_input".into(),hooks_ready:true,input_sequence:Some(0),..Status::default()}}))
                });
                if stop{break;}
            });
            peers.push(Peer{endpoint:desc.endpoint.clone(),thread:Some(thread),_owner:owner});sessions.push(id);
        }
        let created=dispatch(&service,json!({"cmd":"collab.create","request_id":uuid::Uuid::now_v7().to_string(),"session_id":sessions[0],"goal":"verify a real source candidate"})).unwrap();
        let lead=created["credentials"].clone();
        let call=|credentials:&Value,cmd:&str,args:Value|->Result<Value>{
            let mut req=credentials.clone();req["cmd"]=json!(format!("collab.{cmd}"));req["request_id"]=json!(uuid::Uuid::now_v7().to_string());
            for (key,value) in args.as_object().unwrap(){req[key]=value.clone();}
            dispatch(&service,req)
        };
        let mut members=Vec::new();
        for sid in &sessions[1..]{
            let invited=call(&lead,"invite",json!({"session_id":sid,"name":sid})).unwrap();
            let joined=dispatch(&service,json!({"cmd":"collab.join","request_id":uuid::Uuid::now_v7().to_string(),"session_id":sid,"run_id":lead["run_id"],"member_id":invited["member"]["id"],"invitation":invited["invitation"],"confirmed":true})).unwrap();
            members.push(joined["credentials"].clone());
        }
        let writer=&members[0];let reviewer=&members[1];
        let notify=call(&lead,"notify",json!({"to":writer["member_id"],"text":"check mailbox"})).unwrap();
        assert_eq!(notify["input"]["state"],"rejected");
        assert_eq!(call(&lead,"status",json!({})).unwrap()["notifications"][0]["state"],"intent");
        call(&lead,"claim",json!({"writer":writer["member_id"]})).unwrap();
        let workspace=created["member"]["identity"]["workspace_path"].clone();
        let assigned=call(&lead,"assign",json!({"assignee":writer["member_id"],"workspace_path":workspace,"write":true,"work_key":"change","title":"change","instruction":"verify source"})).unwrap();
        let work=assigned["work"]["id"].clone();
        call(writer,"started",json!({"attempt_id":assigned["attempt"]["id"]})).unwrap();
        let path=PathBuf::from(workspace.as_str().unwrap()).join("result.txt");
        std::fs::write(&path,"verified result").unwrap();
        let candidate=call(writer,"candidate",json!({"work_id":work})).unwrap()["candidate"].clone();
        call(writer,"report",json!({"attempt_id":assigned["attempt"]["id"],"outcome":"completed","summary":"fixture assertion passed","candidate":candidate})).unwrap();
        let review=call(&lead,"review.request",json!({"work_id":work,"reviewer":reviewer["member_id"]})).unwrap();
        std::fs::write(&path,"changed after report").unwrap();
        let report=json!({"review_id":review["review"]["id"],"verdict":"passed","summary":"checked","candidate":candidate});
        assert!(call(reviewer,"review.report",report.clone()).unwrap_err().to_string().contains("source changed"));
        std::fs::write(&path,"verified result").unwrap();
        call(reviewer,"review.report",report).unwrap();
        std::fs::write(&path,"changed after review").unwrap();
        assert!(call(&lead,"accept",json!({"work_id":work,"summary":"accept"})).is_err());
        std::fs::write(&path,"verified result").unwrap();
        call(&lead,"accept",json!({"work_id":work,"summary":"actual fixture checks passed"})).unwrap();
        call(writer,"release",json!({})).unwrap();
        let end=call(&lead,"end",json!({"outcome":"accepted","summary":"fixed candidate reviewed"})).unwrap();
        assert_eq!(end["terminals_preserved"],true);
        assert!(ipc::request(&peers[1].endpoint,&json!({"cmd":"status"})).unwrap()["ok"]==true);
    }

    #[test]
    fn collaboration_resolves_identity_and_replays_success_after_host_exit(){
        let (service,mut desc)=fixture();
        {let state=service.lock().unwrap();
            assert!(state.collaboration.lock().unwrap().is_none());
            assert!(!state.root.join("collaboration.db").exists());
            desc.spawn.program="codex".into();desc.spawn.cwd=Some(state.projects[0].path.to_string_lossy().into_owned());
            let mut session=state.workbench.sessions(Some("w")).unwrap().remove(0);session.descriptor=desc.clone();state.workbench.put_session(&session).unwrap();
        }
        let owner=OpenOptions::new().create(true).truncate(false).read(true).write(true).open(desc.directory.join("owner.lock")).unwrap();
        owner.try_lock_exclusive().unwrap();
        let listener=ipc::listen(&desc.endpoint).unwrap();
        let peer=std::thread::spawn(move||ipc::handle(listener.accept().unwrap(),|_|Ok(json!({"ok":true,"status":Status{state:"waiting_input".into(),..Status::default()}}))));
        let request=json!({"cmd":"collab.create","request_id":uuid::Uuid::now_v7().to_string(),"session_id":"s","goal":"test","identity":{"endpoint":"untrusted-pipe","workspace_path":"elsewhere"}});
        let created=dispatch(&service,request.clone()).unwrap();peer.join().unwrap();drop(owner);
        assert_ne!(created["member"]["identity"]["endpoint"],"untrusted-pipe");
        assert_eq!(dispatch(&service,request).unwrap(),created);
        let mut status=created["credentials"].clone();status["cmd"]=json!("collab.status");
        assert_eq!(dispatch(&service,status).unwrap()["run"]["goal"],"test");
        let mut wait=created["credentials"].clone();wait["cmd"]=json!("collab.receive");wait["wait_ms"]=json!(25000);
        let waiter_service=service.clone();let waiter=std::thread::spawn(move||dispatch(&waiter_service,wait).unwrap());
        // Polling the DB and waiting for a mailbox must leave the desktop free.
        let snapshot=dispatch(&service,json!({"cmd":"workbench.sessions","workspace_id":"w"}));
        assert!(snapshot.is_ok());
        let mut finish=created["credentials"].clone();finish["cmd"]=json!("collab.end");finish["outcome"]=json!("cancelled");finish["request_id"]=json!(uuid::Uuid::now_v7().to_string());
        dispatch(&service,finish).unwrap();
        // Deterministically cover a receiver arriving after end, as well as
        // the racing long poll above. Candidate checks must not reject reads.
        let mut after_end=created["credentials"].clone();after_end["cmd"]=json!("collab.receive");
        assert_eq!(dispatch(&service,after_end.clone()).unwrap()["state"],"cancelled");
        after_end["cmd"]=json!("collab.status");assert_eq!(dispatch(&service,after_end).unwrap()["run"]["state"],"cancelled");
        assert_eq!(waiter.join().unwrap()["state"],"cancelled");
    }

    #[test]
    fn targeted_output_preserves_cursor_and_workspace_guards(){
        let (service,desc)=fixture();
        std::fs::write(desc.directory.join("terminal.bin"),b"abcdef").unwrap();
        let request=json!({"cmd":"workbench.terminal.output","workspace_id":"w","session_id":"s","since":3});
        let value=dispatch(&service,request.clone()).unwrap();
        assert_eq!(value["from"],3);assert_eq!(value["end"],6);assert_eq!(value["total"],6);
        assert_eq!(value["data"],"ZGVm");
        {
            let state=service.lock().unwrap();let mut other=state.workbench.workspace("w").unwrap();other.id="other".into();
            state.workbench.put_workspace(&other).unwrap();
        }
        let mut wrong=request.clone();wrong["workspace_id"]=json!("other");
        assert!(dispatch(&service,wrong).is_err());
        let mut missing=request.clone();missing["session_id"]=json!("missing");
        assert!(dispatch(&service,missing).is_err());
        {
            let state=service.lock().unwrap();let mut workspace=state.workbench.workspace("w").unwrap();workspace.archived=true;
            state.workbench.put_workspace(&workspace).unwrap();
        }
        assert!(dispatch(&service,request).is_err());
    }

    #[test]
    fn source_workspace_retry_recovers_registration_after_link_write_failure(){
        let (service,_)=fixture();let mut state=service.lock().unwrap();state.projects[0].direct=false;let project=&state.projects[0];
        git(&project.path,&["init"]).unwrap();git(&project.path,&["-c","user.name=Fixture","-c","user.email=fixture@example.test","commit","--allow-empty","-m","fixture"]).unwrap();
        crate::task_sources::Store::open(&state.root).unwrap();
        let database=rusqlite::Connection::open(state.root.join("task-sources.db")).unwrap();
        let item=json!({"key":"source:7","source_id":"source","project_id":"","provider":"github","remote_id":"7","identifier":"#7","title":"Task","url":"https://example.test/issues/7","description":"","state":"open","closed":false,"updated_at":""});
        database.execute("INSERT INTO items(key,source_id,document) VALUES('source:7','source',?1)",[item.to_string()]).unwrap();
        database.execute_batch("CREATE TRIGGER fail_link BEFORE INSERT ON links BEGIN SELECT RAISE(ABORT,'fixture link failure'); END;").unwrap();
        let id=uuid::Uuid::now_v7().to_string();let request=json!({"cmd":"workbench.create","project_id":"p","source_key":"source:7","name":"Task","base":"HEAD","request_id":id});
        assert!(state.workbench_command(request.clone()).is_err());let kept=state.workbench.workspace(&id).unwrap();assert!(kept.path.exists());
        database.execute_batch("DROP TRIGGER fail_link;").unwrap();let result=state.workbench_command(request.clone()).unwrap();assert_eq!(result["workspace"]["id"],id);assert_eq!(result["recovered"],true);
        assert_eq!(state.workbench_command(request).unwrap()["workspace"]["id"],id);
        assert_eq!(git(&state.projects[0].path,&["worktree","list","--porcelain"]).unwrap().lines().filter(|l|l.starts_with("worktree ")).count(),2);
    }

    #[test]
    fn initial_prompt_launch_replay_and_native_recovery_never_send_a_prompt() {
        let (service,base)=fixture();let mut state=service.lock().unwrap();
        for launch in ["codex","claude-code"] {
            let request=json!({"cmd":"workbench.terminal.open","workspace_id":"w","launch":launch,"request_id":uuid::Uuid::now_v7().to_string(),"initial_prompt":{"text":"unique fixture prompt","mode":"submit"}});
            let (desc,_)=state.prepare_workbench_launch(&request).unwrap();let desc=desc.unwrap();
            assert_eq!(serde_json::from_str::<Value>(&desc.spawn.env[host::INITIAL_PROMPT_ENV]).unwrap(),request["initial_prompt"]);
            assert_eq!(desc.spawn.env["VELELLA_SESSION_ENDPOINT"],desc.endpoint);
            assert!(!desc.spawn.env.contains_key("VELELLA_DATA_DIR"));
            assert!(!desc.spawn.env.contains_key("VELELLA_COLLABORATION_SKILL"));
            assert!(!desc.spawn.args.iter().any(|arg|arg.contains("unique fixture prompt")||arg.contains("skip-permissions")||arg.contains("ask-for-approval")));
            assert!(state.prepare_workbench_launch(&request).unwrap().0.is_none());
            for prompt in [Value::Null,json!({"text":"different","mode":"submit"}),json!({"text":"unique fixture prompt","mode":"draft"})] {
                let mut changed=request.clone();if prompt.is_null(){changed.as_object_mut().unwrap().remove("initial_prompt");}else{changed["initial_prompt"]=prompt;}
                assert!(state.prepare_workbench_launch(&changed).is_err());
            }
            let recovery=json!({"cmd":"workbench.terminal.recover","workspace_id":"w","session_id":desc.id});
            let mut changed=recovery.clone();changed["initial_prompt"]=request["initial_prompt"].clone();
            assert!(state.prepare_workbench_launch(&changed).is_err());
            let native=uuid::Uuid::now_v7().to_string();
            ipc::atomic_json(&desc.directory.join("status.json"),&Status{pid:1234,native_id:Some(native.clone()),..Status::default()}).unwrap();
            let recovered=state.prepare_workbench_launch(&recovery).unwrap().0.unwrap();
            assert_eq!(recovered.spawn.env[host::INITIAL_PROMPT_ENV],desc.spawn.env[host::INITIAL_PROMPT_ENV]);
            assert_eq!(recovered.spawn.args[1],native);assert_eq!(&recovered.spawn.args[2..],desc.spawn.args);
            assert!(desc.directory.join(host::INITIAL_PROMPT_CLOSED).exists());
            assert!(workbench_commands::native_prompt::validate_feedback(&recovered,"unique fixture prompt",false).is_err());
            assert!(state.prepare_workbench_launch(&request).unwrap().0.is_none());
            assert!(!desc.directory.join("terminal-initial-prompt-feedback.json").exists());
        }
        for request in [json!({"launch":"terminal"}),json!({"launch":"codex","sandbox":{"mode":"restricted"}})] {
            let mut request=request;request["workspace_id"]=json!("w");request["initial_prompt"]=json!({"text":"fixture","mode":"draft"});
            assert!(state.prepare_workbench_launch(&request).is_err());
        }
        let ordinary=state.prepare_workbench_launch(&json!({"workspace_id":"w","launch":"codex"})).unwrap().0.unwrap();
        assert!(!ordinary.spawn.env.contains_key(host::INITIAL_PROMPT_ENV));
        assert!(!ordinary.spawn.env.contains_key("VELELLA_SESSION_ENDPOINT"));
        assert!(ordinary.spawn.args.iter().any(|arg|arg.starts_with("hooks.SessionStart=")&&arg.contains("native-restore-hook")));
        assert!(!ordinary.spawn.args.iter().any(|arg|arg.starts_with("hooks.UserPromptSubmit=")));
        drop(state);drop(service);std::fs::remove_dir_all(base.directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn skill_argv_session_recovery_cannot_repeat_an_initial_task() {
        let (service,_) = fixture();let mut state=service.lock().unwrap();
        let mut session=state.workbench.session("w","s").unwrap().unwrap();
        session.descriptor.spawn.env.insert("VELELLA_SKILL_ARGV_PROMPT".into(),"immutable task".into());
        state.workbench.put_session(&session).unwrap();
        let error=state.prepare_workbench_launch(&json!({"cmd":"workbench.terminal.recover","workspace_id":"w","session_id":"s"})).err().unwrap();
        assert!(error.to_string().contains("恢复不能重放任务"));
    }

    // Synthetic host: exercises real IPC and durable desktop receipts without
    // spawning a CLI or consulting auth. Every IPC callback verifies Service is
    // unlocked, including both preflight and the post-paste readiness read.
    fn initial_feedback_case(case:&'static str,draft:bool)->(bool,Value,Vec<Value>,usize) {
        let (service,mut desc)=fixture();
        desc.spawn.program="codex".into();
        desc.spawn.env.insert(host::INITIAL_PROMPT_ENV.into(),json!({"text":"fixture prompt","mode":if draft{"draft"}else{"submit"}}).to_string());
        {let state=service.lock().unwrap();let mut s=state.workbench.session("w","s").unwrap().unwrap();s.descriptor=desc.clone();state.workbench.put_session(&s).unwrap();}
        std::fs::write(desc.directory.join("terminal.bin"),b"").unwrap();
        let mut status=Status{pid:77,state:"waiting_input".into(),hooks_ready:true,native_id:Some("native".into()),input_protocol:1,input_ready_protocol:1,input_sequence:Some(0),..Status::default()};
        match case {"startup"=>status.state="starting".into(),"permission_before"=>status.state="waiting_permission".into(),"sequence_before"=>status.input_sequence=Some(1),"human_before"=>status.human_control=true,"old_host"=>status.input_ready_protocol=0,_=>{}}
        let listener=ipc::listen(&desc.endpoint).unwrap();let observed=service.clone();
        let peer=std::thread::spawn(move||{let mut inputs=Vec::new();let mut writes=0;loop {let mut stop=false;ipc::handle(listener.accept().unwrap(),|req|{
            assert!(observed.try_lock().is_ok(),"status/input IPC held Service");
            if req["cmd"]=="fixture.done" {stop=true;return Ok(json!({"ok":true}));}
            if req["cmd"]=="status" {
                if writes==1 {match case {"permission_after"=>status.state="waiting_permission".into(),"sequence_after"=>status.input_sequence=Some(2),"unknown_after"=>status.state="unknown".into(),_=>{}}}
                return Ok(json!({"ok":true,"status":status}));
            }
            assert_eq!(req["cmd"],"input");inputs.push(req.clone());
            assert_eq!(req["require_input_ready"],true);assert_eq!(req["initial"],true);
            let expected=if req["enter"]==true{1}else{0};
            assert_eq!(req["expected_input_sequence"],expected);
            let id=req["request_id"].as_str().unwrap();
            assert_eq!(observed.lock().unwrap().workbench.feedback("w",id).unwrap().unwrap()["state"],"sending","receipt must precede effects");
            match (case,expected) {("race_sequence_paste",0)|("race_sequence_enter",1)=>status.input_sequence=Some(expected+1),("race_permission_paste",0)|("race_permission_enter",1)=>status.state="waiting_permission".into(),_=>{}}
            host::require_input_ready(&status,expected,Some(id))?;
            writes+=1;status.input_sequence=Some(expected+1);status.human_control=true;status.typed_control=true;status.initial_input_request=Some(id.into());
            if req["enter"]==true{status.state="input_pending".into();}
            if (case=="unknown_paste"&&expected==0)||(case=="unknown_enter"&&expected==1) {return Ok(json!({"delivery":"unknown"}));}
            if case=="missing_sequence_ack" {return Ok(json!({"ok":true}));}
            Ok(json!({"ok":true,"input_sequence":status.input_sequence}))
        });if stop{return (inputs,writes);}}});
        let request=json!({"cmd":"workbench.feedback.send","workspace_id":"w","session_id":"s","request_id":uuid::Uuid::now_v7().to_string(),"text":"fixture prompt","source":"issue:7","initial":true,"draft":draft,"expected_input_sequence":0});
        let result=dispatch(&service,request.clone());let ok=result.is_ok();let value=result.unwrap_or_else(|e|json!({"error":e.to_string()}));
        if ok {
            assert_eq!(dispatch(&service,request.clone()).unwrap(),value,"same ID must be a read, not a retry");
            for (key,value) in [("draft",json!(!draft)),("initial",json!(false)),("expected_input_sequence",json!(1)),("text",json!("different")),("source",json!("issue:8"))] {
                let mut changed=request.clone();changed[key]=value;assert!(dispatch(&service,changed).is_err(),"{key} must bind to the receipt");
            }
            let mut changed=request;changed["request_id"]=json!(uuid::Uuid::now_v7().to_string());
            assert!(dispatch(&service,changed).is_err(),"another ID cannot resend the same initial prompt");
        }
        ipc::request(&desc.endpoint,&json!({"cmd":"fixture.done"})).unwrap();let (inputs,writes)=peer.join().unwrap();
        if ok {let receipt=service.lock().unwrap().workbench.feedback("w",value["receipt"]["request_id"].as_str().unwrap()).unwrap().unwrap();assert_eq!(receipt["text"],"fixture prompt");}
        drop(service);std::fs::remove_dir_all(desc.directory.parent().unwrap()).unwrap();
        (ok,value,inputs,writes)
    }

    #[test]
    fn initial_prompt_draft_submit_receipts_and_policy_replay() {
        for draft in [true,false] {
            let (ok,value,inputs,writes)=initial_feedback_case("ready",draft);assert!(ok);
            assert_eq!(value["receipt"]["state"],if draft{"drafted"}else{"submitted"});
            assert_eq!(writes,if draft{1}else{2});assert_eq!(inputs.len(),writes);
            assert_eq!(inputs[0]["text"],"\u{1b}[200~fixture prompt\u{1b}[201~");
            assert!(value["receipt"]["accepted"].is_null());assert!(value["receipt"]["turn_started"].is_null());
        }
    }

    #[test]
    fn initial_prompt_unready_preflight_never_pastes() {
        for case in ["startup","permission_before","sequence_before","human_before","old_host"] {
            let (ok,_,inputs,writes)=initial_feedback_case(case,false);assert!(!ok,"{case}");assert!(inputs.is_empty());assert_eq!(writes,0);
        }
    }

    #[test]
    fn initial_prompt_races_and_unknown_delivery_never_replay() {
        for (case,expected_writes,expected_inputs) in [
            ("race_sequence_paste",0,1),("race_permission_paste",0,1),
            ("sequence_after",1,1),("permission_after",1,1),("unknown_after",1,1),
            ("race_sequence_enter",1,2),("race_permission_enter",1,2),
            ("unknown_paste",1,1),("unknown_enter",2,2),("missing_sequence_ack",1,1),
        ] {
            let (ok,value,inputs,writes)=initial_feedback_case(case,false);assert!(ok,"{case}");
            assert_eq!(value["receipt"]["state"],"uncertain","{case}");assert_eq!(inputs.len(),expected_inputs,"{case}");assert_eq!(writes,expected_writes,"{case}");
        }
    }

    #[test]
    fn initial_prompt_flags_metadata_and_legacy_source_receipt_are_checked_without_ipc() {
        let (service,mut desc)=fixture();desc.spawn.program="codex".into();
        desc.spawn.env.insert(host::INITIAL_PROMPT_ENV.into(),json!({"text":"fixture","mode":"draft"}).to_string());
        {let state=service.lock().unwrap();let mut s=state.workbench.session("w","s").unwrap().unwrap();s.descriptor=desc.clone();state.workbench.put_session(&s).unwrap();}
        let request=json!({"cmd":"workbench.feedback.send","workspace_id":"w","session_id":"s","request_id":uuid::Uuid::now_v7().to_string(),"text":"fixture","initial":true,"draft":true,"expected_input_sequence":0});
        for (key,value) in [("initial",json!("true")),("draft",Value::Null),("draft",json!(false)),("text",json!("wrong")),("expected_input_sequence",json!("0")),("expected_input_sequence",json!(-1))] {
            let mut bad=request.clone();bad[key]=value;assert!(dispatch(&service,bad).is_err());
        }
        let mut legacy=request.clone();legacy.as_object_mut().unwrap().remove("initial");
        let receipt=json!({"request_id":legacy["request_id"],"session_id":"s","text":"fixture","draft":true,"state":"drafted"});
        service.lock().unwrap().workbench.begin_feedback("w",legacy["request_id"].as_str().unwrap(),&receipt).unwrap();
        assert_eq!(dispatch(&service,legacy).unwrap()["receipt"]["state"],"drafted");
        assert!(dispatch(&service,request).is_err(),"legacy draft cannot become an initial submission");
        drop(service);std::fs::remove_dir_all(desc.directory.parent().unwrap()).unwrap();
    }

    #[test]
    fn source_task_draft_is_pasted_once_without_enter_and_waits_for_ready_agent(){
        for ready in [true,false]{
            let (service,desc)=fixture();
            {let state=service.lock().unwrap();let mut session=state.workbench.sessions(Some("w")).unwrap().remove(0);session.descriptor.spawn.program="codex".into();state.workbench.put_session(&session).unwrap();}
            let listener=ipc::listen(&desc.endpoint).unwrap();
            let peer=std::thread::spawn(move||{let mut inputs=vec![];loop{let mut stop=false;ipc::handle(listener.accept().unwrap(),|req|{
                stop=req["cmd"]=="fixture.done";if req["cmd"]=="input"{inputs.push(req.clone());}
                Ok(json!({"ok":true,"status":Status{state:if ready{"waiting_input"}else{"waiting_permission"}.into(),input_protocol:1,input_sequence:Some(0),..Status::default()}}))
            });if stop{return inputs;}}});
            let request=json!({"cmd":"workbench.feedback.send","workspace_id":"w","session_id":"s","request_id":uuid::Uuid::now_v7().to_string(),"text":"Complete https://example.test/issue/7","source":"issue:7","draft":true,"expected_input_sequence":0});
            let first=dispatch(&service,request.clone());
            if ready{assert_eq!(first.unwrap()["receipt"]["state"],"drafted");assert_eq!(dispatch(&service,request).unwrap()["receipt"]["state"],"drafted");}else{assert!(first.is_err());}
            ipc::request(&desc.endpoint,&json!({"cmd":"fixture.done"})).unwrap();let inputs=peer.join().unwrap();
            assert_eq!(inputs.len(),usize::from(ready));if ready{assert_eq!(inputs[0]["enter"],false);assert!(inputs[0]["text"].as_str().unwrap().starts_with("\u{1b}[200~Complete "));}
        }
    }

    #[test]
    fn stalled_terminal_does_not_block_other_requests(){
        let (service,desc)=fixture();
        let listener=ipc::listen(&desc.endpoint).unwrap();
        let (entered_tx,entered_rx)=mpsc::channel();let (release_tx,release_rx)=mpsc::channel();
        let peer=std::thread::spawn(move||{let stream=listener.accept().unwrap();ipc::handle(stream,|_|{entered_tx.send(()).unwrap();release_rx.recv().unwrap();Ok(json!({"ok":true}))})});
        let slow_service=service.clone();
        let slow=std::thread::spawn(move||dispatch(&slow_service,json!({"cmd":"workbench.terminal.input","workspace_id":"w","session_id":"s","text":"a","input_kind":"user"})));
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let (done_tx,done_rx)=mpsc::channel();let fast_service=service.clone();
        let fast=std::thread::spawn(move||{done_tx.send(dispatch(&fast_service,json!({"cmd":"snapshot"}))).unwrap()});
        let responsive=done_rx.recv_timeout(Duration::from_millis(200)).is_ok();
        release_tx.send(()).unwrap();slow.join().unwrap().unwrap();peer.join().unwrap();fast.join().unwrap();
        assert!(responsive,"a stalled host held the entire daemon state lock for more than 200ms");
    }

    #[test]
    fn recovering_a_host_does_not_hold_global_state_while_waiting_for_startup(){
        let (service,desc)=fixture();
        // The controlled pipe below owns startup. Do not spawn the test binary
        // with the "host" test filter, which would recursively run this test.
        service.lock().unwrap().executable=if cfg!(windows){PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/whoami.exe")}else{PathBuf::from("/bin/true")};
        let listener=ipc::listen(&desc.endpoint).unwrap();
        let (entered_tx,entered_rx)=mpsc::channel();let (release_tx,release_rx)=mpsc::channel();
        let peer=std::thread::spawn(move||ipc::handle(listener.accept().unwrap(),|_|{
            entered_tx.send(()).unwrap();release_rx.recv().unwrap();
            Ok(json!({"ok":true,"status":Status::default()}))
        }));
        let observed=service.clone();let recovery=std::thread::spawn(move||dispatch(&observed,json!({"cmd":"workbench.terminal.recover","workspace_id":"w","session_id":"s"})));
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let available=service.try_lock().is_ok();
        release_tx.send(()).unwrap();peer.join().unwrap();recovery.join().unwrap().unwrap();
        assert!(available,"recovery held global state while its host was starting");
    }

    #[test]
    fn task_input_cancellation_and_aliases_share_one_queue(){
        let (service,desc)=fixture();with_task(&service,&desc);
        let mut state=service.lock().unwrap();
        let a=state.terminal_gate(&json!({"cmd":"terminal.input","task_id":"t"})).unwrap().unwrap().1;
        for req in [json!({"cmd":"task.cancel","task_id":"t"}),json!({"cmd":"task.resume","task_id":"t"}),json!({"cmd":"workbench.terminal.input","workspace_id":"w","session_id":"s"}),json!({"cmd":"workbench.terminal.recover","workspace_id":"w","session_id":"s"})]{
            let b=state.terminal_gate(&req).unwrap().unwrap().1;assert!(Arc::ptr_eq(&a,&b));
        }
        assert!(state.prepare_terminal(&json!({"cmd":"workbench.terminal.input","workspace_id":"wrong","session_id":"s","text":"x"})).is_err());
    }

    #[test]
    fn rejected_input_preserves_verification_and_success_invalidates_it(){
        for accepted in [false,true]{
            let (service,desc)=fixture();with_task(&service,&desc);
            let listener=ipc::listen(&desc.endpoint).unwrap();
            let peer=std::thread::spawn(move||ipc::handle(listener.accept().unwrap(),|_|Ok(json!({"ok":accepted,"error":"fixture rejection"}))));
            let response=dispatch(&service,json!({"cmd":"terminal.input","task_id":"t","text":"x","enter":false}));
            assert_eq!(response.is_ok(),accepted);peer.join().unwrap();
            let state=service.lock().unwrap();assert_eq!(state.tasks[0].verified,!accepted);
            let document:String=db(&state.projects[0]).unwrap().query_row("SELECT document FROM desktop_tasks WHERE id='t'",[],|row|row.get(0)).unwrap();
            assert_eq!(serde_json::from_str::<Task>(&document).unwrap().verified,!accepted);
        }
    }

    #[test]
    fn supervisor_status_io_does_not_hold_state_and_discards_stale_observations(){
        let (service,desc)=fixture();with_task(&service,&desc);
        let listener=ipc::listen(&desc.endpoint).unwrap();let (entered_tx,entered_rx)=mpsc::channel();let (release_tx,release_rx)=mpsc::channel();
        let peer=std::thread::spawn(move||ipc::handle(listener.accept().unwrap(),|_|{entered_tx.send(()).unwrap();release_rx.recv().unwrap();Ok(json!({"ok":true,"status":Status{state:"running".into(),..Status::default()}}))}));
        let observed=service.clone();let poll=std::thread::spawn(move||supervisor_tick(&observed));
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let available=service.try_lock().is_ok();
        if available{service.lock().unwrap().terminal_epoch+=1;}
        release_tx.send(()).unwrap();peer.join().unwrap();poll.join().unwrap().unwrap();
        assert!(available,"supervisor status held global state");
        assert_eq!(service.lock().unwrap().tasks[0].phase,Phase::Review,"a superseded running observation was applied");
    }

    #[test]
    fn task_feedback_cannot_release_human_control_or_answer_permission_prompts(){
        for (human,state_name) in [(true,"waiting_input"),(false,"waiting_permission")] {
            let (service,desc)=fixture();with_task(&service,&desc);
            service.lock().unwrap().tasks[0].phase=Phase::NeedsAttention;
            let listener=ipc::listen(&desc.endpoint).unwrap();
            let peer=std::thread::spawn(move||{let mut commands=vec![];loop{let mut stop=false;ipc::handle(listener.accept().unwrap(),|req|{
                let cmd=req["cmd"].as_str().unwrap().to_owned();stop=cmd=="fixture.done";if !stop{commands.push(cmd);}
                Ok(json!({"ok":true,"status":Status{human_control:human,state:state_name.into(),..Status::default()}}))
            });if stop{return commands}}});
            let result=dispatch(&service,json!({"cmd":"task.feedback","task_id":"t","request_id":uuid::Uuid::now_v7().to_string(),"text":"fixture"}));
            ipc::request(&desc.endpoint,&json!({"cmd":"fixture.done"})).unwrap();
            let commands=peer.join().unwrap();assert_eq!(commands,vec!["status"],"feedback must not send control/input before the operator clears the existing guard");
            assert_eq!(result.unwrap()["receipt"]["state"],"rejected");assert!(service.lock().unwrap().tasks[0].verified);
        }
    }

    #[test]
    fn ambiguous_input_blocks_following_input_until_the_original_host_acknowledges(){
        let (service,desc)=fixture();with_task(&service,&desc);
        let listener=ipc::listen(&desc.endpoint).unwrap();let (release_tx,release_rx)=mpsc::channel();
        let peer=std::thread::spawn(move||ipc::handle(listener.accept().unwrap(),|_|{release_rx.recv().unwrap();Ok(json!({"ok":true}))}));
        let first=dispatch(&service,json!({"cmd":"terminal.input","task_id":"t","text":"first"}));assert!(first.is_err());
        let started=Instant::now();let second=dispatch(&service,json!({"cmd":"terminal.input","task_id":"t","text":"second"}));
        let rejected_without_sending=started.elapsed()<Duration::from_millis(200)&&second.is_err();
        release_tx.send(()).unwrap();peer.join().unwrap();
        assert!(rejected_without_sending,"later input entered IPC while the previous delivery was unresolved");
        let until=Instant::now()+Duration::from_secs(1);while desc.directory.join("terminal-input-pending.json").exists()&&Instant::now()<until{std::thread::sleep(Duration::from_millis(10));}
        assert!(!desc.directory.join("terminal-input-pending.json").exists(),"a known acknowledgement releases the input barrier");
        let listener=ipc::listen(&desc.endpoint).unwrap();
        let peer=std::thread::spawn(move||ipc::handle(listener.accept().unwrap(),|req|{assert_eq!(req["text"],"fresh");Ok(json!({"ok":true}))}));
        dispatch(&service,json!({"cmd":"terminal.input","task_id":"t","text":"fresh"})).unwrap();peer.join().unwrap();
    }

    #[test]
    fn feedback_status_check_does_not_hold_global_state(){
        let (service,desc)=fixture();
        {let state=service.lock().unwrap();let mut session=state.workbench.sessions(Some("w")).unwrap().remove(0);session.descriptor.spawn.program="codex".into();state.workbench.put_session(&session).unwrap();}
        let listener=ipc::listen(&desc.endpoint).unwrap();let (entered_tx,entered_rx)=mpsc::channel();let (release_tx,release_rx)=mpsc::channel();
        let peer=std::thread::spawn(move||ipc::handle(listener.accept().unwrap(),|_|{
            entered_tx.send(()).unwrap();release_rx.recv().unwrap();
            Ok(json!({"ok":true,"status":Status{input_protocol:1,state:"waiting_permission".into(),..Status::default()}}))
        }));
        let observed=service.clone();let feedback=std::thread::spawn(move||dispatch(&observed,json!({"cmd":"workbench.feedback.send","workspace_id":"w","session_id":"s","request_id":uuid::Uuid::now_v7().to_string(),"text":"fixture"})));
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();let available=service.try_lock().is_ok();
        release_tx.send(()).unwrap();peer.join().unwrap();assert!(feedback.join().unwrap().is_err(),"permission prompts must still block feedback");
        assert!(available,"feedback status check waited for a host under global state");
    }

    #[test]
    fn explicit_task_resume_does_not_wait_for_host_under_global_state(){
        let (service,desc)=fixture();with_task(&service,&desc);
        service.lock().unwrap().tasks[0].phase=Phase::NeedsAttention;
        let listener=ipc::listen(&desc.endpoint).unwrap();let (entered_tx,entered_rx)=mpsc::channel();let (release_tx,release_rx)=mpsc::channel();
        let peer=std::thread::spawn(move||{let mut commands=vec![];let mut index=0;loop{let mut stop=false;ipc::handle(listener.accept().unwrap(),|req|{
            let cmd=req["cmd"].as_str().unwrap_or("").to_owned();stop=cmd=="fixture.done";
            if !stop{if index==0{entered_tx.send(()).unwrap();release_rx.recv().unwrap();}commands.push(cmd);index+=1;}
            Ok(json!({"ok":true,"status":Status{state:"running".into(),..Status::default()}}))
        });if stop{return commands}}});
        let observed=service.clone();let resume=std::thread::spawn(move||dispatch(&observed,json!({"cmd":"task.resume","task_id":"t"})));
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();let available=service.try_lock().is_ok();
        release_tx.send(()).unwrap();let result=resume.join().unwrap();
        ipc::request(&desc.endpoint,&json!({"cmd":"fixture.done"})).unwrap();let commands=peer.join().unwrap();
        assert!(result.as_ref().err().is_some_and(|error|format!("{error:#}").contains("native session ID missing")),"{result:?}");
        assert_eq!(commands,vec!["status"],"an unresolved agent was stopped or relaunched without a native session ID");
        assert!(available,"explicit resume waited for a host under global state");
    }

    #[test]
    fn supervisor_timeout_cleanup_does_not_hold_global_state(){
        let (service,desc)=fixture();with_task(&service,&desc);
        {let mut state=service.lock().unwrap();state.tasks[0].phase=Phase::Running;state.tasks[0].deadline=Some(0);}
        let listener=ipc::listen(&desc.endpoint).unwrap();let (entered_tx,entered_rx)=mpsc::channel();let (release_tx,release_rx)=mpsc::channel();
        let peer=std::thread::spawn(move||{
            for index in 0..4 {ipc::handle(listener.accept().unwrap(),|_|{
                if index==1{entered_tx.send(()).unwrap();release_rx.recv().unwrap();}
                Ok(json!({"ok":true,"status":Status{exit:if index==3{Some(0)}else{None},..Status::default()}}))
            });}
        });
        let observed=service.clone();let tick=std::thread::spawn(move||supervisor_tick(&observed));
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();let available=service.try_lock().is_ok();
        release_tx.send(()).unwrap();peer.join().unwrap();tick.join().unwrap().unwrap();
        assert!(available,"deadline cleanup waited for a host under global state");
        assert_eq!(service.lock().unwrap().tasks[0].phase,Phase::NeedsAttention);
    }

    #[test]
    fn an_input_that_completes_after_timeout_cannot_leave_a_verified_or_automatic_task(){
        let (service,desc)=fixture();with_task(&service,&desc);
        let listener=ipc::listen(&desc.endpoint).unwrap();
        let peer=std::thread::spawn(move||ipc::handle(listener.accept().unwrap(),|_|{
            std::thread::sleep(Duration::from_millis(2200));Ok(json!({"ok":true}))
        }));
        let response=dispatch(&service,json!({"cmd":"terminal.input","task_id":"t","text":"x","enter":false}));
        assert!(response.unwrap_err().downcast_ref::<UncertainInput>().is_some());
        peer.join().unwrap();let state=service.lock().unwrap();
        assert!(!state.tasks[0].verified);assert_eq!(state.tasks[0].phase,Phase::NeedsAttention);
        let document:String=db(&state.projects[0]).unwrap().query_row("SELECT document FROM desktop_tasks WHERE id='t'",[],|row|row.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<Task>(&document).unwrap().phase,Phase::NeedsAttention);
    }
}
