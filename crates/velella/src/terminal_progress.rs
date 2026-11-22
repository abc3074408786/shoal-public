//! Session-bound, read-only native task plans. No log-volume progress estimates.
use crate::host::Descriptor;
use anyhow::{Context,Result};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::HashMap,fs,io::{BufRead,BufReader,Read,Seek,SeekFrom},path::{Path,PathBuf},sync::{Mutex,OnceLock},time::{Duration,Instant,SystemTime}};

const MAX_LINE:u64=2*1024*1024;
const PAGE_BYTES:u64=4*1024*1024;
const MAX_STEPS:usize=200;
#[path="terminal_progress_zcode.rs"]
mod zcode;
#[path="terminal_progress_codex.rs"]
mod codex;

#[derive(Clone,Serialize,Deserialize)]
struct Step{id:String,title:String,status:String,active_form:Option<String>,blocked_by:Vec<String>}
#[derive(Clone)]
struct Pending{name:String,input:Value,time:Option<String>}
#[derive(Default)]
struct Plan{
    bound:bool,steps:Vec<Step>,previous:Vec<Step>,pending:HashMap<String,Pending>,
    task_label:Option<String>,reported_at:Option<String>,source:Option<String>,explanation:Option<String>,
    turn_id:Option<String>,incomplete_tasks:bool,
}
impl Plan{
    fn new_turn(&mut self,label:Option<String>){
        if !self.steps.is_empty(){self.previous=std::mem::take(&mut self.steps)}
        self.pending.clear();self.task_label=label;self.reported_at=None;self.source=None;self.explanation=None;self.incomplete_tasks=false;
    }
    fn begin(&mut self,id:&str,name:&str,input:Value,time:Option<String>){
        if id.is_empty()||id.len()>256||self.pending.len()>=256{return}
        let name=name.rsplit('.').next().unwrap_or(name);
        if matches!(name,"update_plan"|"TodoWrite"|"TaskCreate"|"TaskUpdate"|"TaskList"|"TaskGet"){
            self.pending.insert(id.into(),Pending{name:name.into(),input,time});
        }
    }
    fn finish(&mut self,id:&str,failed:bool,result:&Value)->Result<()>{
        let Some(call)=self.pending.remove(id)else{return Ok(())};
        if failed{return Ok(())}
        let input=&call.input;
        match call.name.as_str(){
            "update_plan"|"TodoWrite"=>{
                let values=if call.name=="update_plan"{&input["plan"]}else{&input["todos"]};
                self.steps=steps(values)?;self.incomplete_tasks=false;
                self.explanation=text(&input["explanation"],2000);
            }
            "TaskCreate"=>{
                let Some(id)=task_id(&result["task"]).or_else(||task_id(result))else{self.incomplete_tasks=true;return Ok(())};
                let title=text(&input["subject"],1000).context("task subject missing")?;
                if self.steps.len()>=MAX_STEPS{anyhow::bail!("task plan exceeds 200 steps")}
                if !self.steps.iter().any(|s|s.id==id){self.steps.push(Step{id,title,status:"pending".into(),active_form:text(&input["activeForm"],1000).or_else(||text(&input["active_form"],1000)),blocked_by:strings(&input["blockedBy"])})}
            }
            "TaskUpdate"=>{
                let Some(id)=task_id(input)else{return Ok(())};
                if self.steps.is_empty()&&self.previous.iter().any(|s|s.id==id){self.steps=std::mem::take(&mut self.previous)}
                if input["status"]=="deleted"{self.steps.retain(|s|s.id!=id);}
                else if let Some(step)=self.steps.iter_mut().find(|s|s.id==id){
                    if let Some(status)=input["status"].as_str(){step.status=valid_status(status)?.into()}
                    if let Some(title)=text(&input["subject"],1000){step.title=title}
                    if let Some(active)=text(&input["activeForm"],1000).or_else(||text(&input["active_form"],1000)){step.active_form=Some(active)}
                    if input.get("blockedBy").is_some(){step.blocked_by=strings(&input["blockedBy"])}
                    for id in strings(&input["addBlockedBy"]){if !step.blocked_by.contains(&id){step.blocked_by.push(id)}}
                    let remove=strings(&input["removeBlockedBy"]);step.blocked_by.retain(|id|!remove.contains(id));
                }else{self.incomplete_tasks=true;}
            }
            "TaskList"=>{
                let list=result.get("tasks").unwrap_or(result);self.steps=steps(list)?;self.incomplete_tasks=false;
            }
            "TaskGet"=>{
                let row=result.get("task").unwrap_or(result);
                let mut found=steps(&json!([row]))?;let step=found.remove(0);
                if let Some(old)=self.steps.iter_mut().find(|old|old.id==step.id){*old=step}
                else if self.steps.len()<MAX_STEPS{self.steps.push(step)}
            }
            _=>return Ok(()),
        }
        self.reported_at=call.time;self.source=Some(if call.name=="update_plan"{"codex.update_plan".into()}else{format!("claude.{}",call.name)});
        Ok(())
    }
}

fn text(value:&Value,max:usize)->Option<String>{value.as_str().filter(|s|!s.trim().is_empty()).map(|s|s.chars().take(max).collect())}
fn strings(value:&Value)->Vec<String>{value.as_array().map(|items|items.iter().filter_map(|v|text(v,128)).take(MAX_STEPS).collect()).unwrap_or_default()}
fn task_id(v:&Value)->Option<String>{for key in ["taskId","task_id","id"]{if let Some(s)=text(&v[key],128){return Some(s)}if let Some(id)=v[key].as_u64(){return Some(id.to_string())}}None}
fn valid_status(status:&str)->Result<&str>{anyhow::ensure!(matches!(status,"pending"|"in_progress"|"completed"),"unknown task step status");Ok(status)}
fn steps(value:&Value)->Result<Vec<Step>>{
    let list=value.as_array().context("task plan must contain a step array")?;
    anyhow::ensure!(list.len()<=MAX_STEPS,"task plan exceeds 200 steps");
    let mut result=Vec::new();
    for (index,row) in list.iter().enumerate(){
        let title=text(&row["step"],1000).or_else(||text(&row["content"],1000)).or_else(||text(&row["subject"],1000)).context("task step title missing")?;
        let status=valid_status(row["status"].as_str().context("task step status missing")?)?.into();
        let id=task_id(row).unwrap_or_else(||(index+1).to_string());
        anyhow::ensure!(!result.iter().any(|old:&Step|old.id==id),"duplicate task step identity");
        result.push(Step{id,title,status,active_form:text(&row["activeForm"],1000).or_else(||text(&row["active_form"],1000)),blocked_by:strings(&row["blockedBy"])});
    }
    Ok(result)
}

fn is_plain(path:&Path)->Result<bool>{
    let metadata=fs::symlink_metadata(path)?;
    #[cfg(windows)]{use std::os::windows::fs::MetadataExt;if metadata.file_attributes()&0x400!=0{return Ok(false)}}
    Ok(!metadata.file_type().is_symlink())
}
fn provider_root(desc:&Descriptor)->Result<PathBuf>{
    let key=if desc.spawn.program=="codex"{"CODEX_HOME"}else{"CLAUDE_CONFIG_DIR"};
    let explicit=desc.spawn.env.get(key).cloned().or_else(||std::env::var(key).ok());
    let home=if let Some(path)=explicit{PathBuf::from(path)}else{
        let home=desc.spawn.env.get("USERPROFILE").or_else(||desc.spawn.env.get("HOME")).cloned().or_else(||std::env::var("USERPROFILE").ok()).or_else(||std::env::var("HOME").ok()).context("native account directory unknown")?;
        PathBuf::from(home).join(if desc.spawn.program=="codex"{".codex"}else{".claude"})
    };
    anyhow::ensure!(home.is_absolute(),"native account directory must be absolute");
    let root=home.join(if desc.spawn.program=="codex"{"sessions"}else{"projects"});
    anyhow::ensure!(root.is_dir(),"native session history is unavailable for this account");
    for ancestor in root.ancestors(){anyhow::ensure!(is_plain(ancestor)?,"linked native history is not read")}
    Ok(dunce::canonicalize(root)?)
}
fn find_history(root:&Path,native:&str,depth:usize)->Result<Option<PathBuf>>{
    fn walk(path:&Path,native:&str,depth:usize,budget:&mut usize,until:Instant,found:&mut Option<PathBuf>)->Result<()>{
        anyhow::ensure!(*budget<20000&&Instant::now()<until,"native history lookup budget reached");
        if depth==0||!is_plain(path)?{return Ok(())}
        for entry in fs::read_dir(path)?{
            let entry=entry?;*budget+=1;
            anyhow::ensure!(*budget<20000&&Instant::now()<until,"native history lookup budget reached");
            let path=entry.path();if !is_plain(&path)?{continue}
            if entry.file_type()?.is_dir(){if entry.file_name()=="subagents"{continue}walk(&path,native,depth-1,budget,until,found)?}
            else if path.extension().is_some_and(|s|s=="jsonl")&&path.file_stem().is_some_and(|s|s.to_string_lossy().ends_with(native)){
                anyhow::ensure!(found.is_none(),"native session has ambiguous history files");*found=Some(path);
            }
        }
        Ok(())
    }
    let mut found=None;let mut budget=0;let until=Instant::now()+Duration::from_millis(300);
    // UUIDv7 encodes creation time. Native Codex rollouts live in date folders;
    // avoid rescanning years of unrelated history on every cold session lookup.
    if root.file_name().is_some_and(|name|name=="sessions"){
        if let Some(time)=uuid::Uuid::parse_str(native).ok().and_then(|id|id.get_timestamp()){
            let (seconds,nanos)=time.to_unix();
            if let Some(time)=chrono::DateTime::from_timestamp(seconds as i64,nanos){
                for day in [0,-1,1]{
                    let path=root.join((time+chrono::Duration::days(day)).format("%Y/%m/%d").to_string());
                    if !path.is_dir(){continue}
                    for ancestor in path.ancestors().take_while(|path|path.starts_with(root)){anyhow::ensure!(is_plain(ancestor)?,"linked native history is not read")}
                    walk(&path,native,depth,&mut budget,until,&mut found)?;
                }
                if found.is_some(){return Ok(found)}
            }
        }
    }
    walk(root,native,depth,&mut budget,until,&mut found)?;Ok(found)
}

fn same_cwd(value:&Value,cwd:&Path)->bool{value.as_str().is_some_and(|p|dunce::canonicalize(p).is_ok_and(|p|p==cwd))}
fn public_label(value:&Value)->Option<String>{
    let body=if let Some(s)=value.as_str(){s.to_owned()}else{value.as_array()?.iter().filter(|p|matches!(p["type"].as_str(),Some("text"|"input_text"))).filter_map(|p|p["text"].as_str()).collect::<Vec<_>>().join("\n")};
    body.lines().map(str::trim).find(|s|!s.is_empty()&&!s.starts_with('<')).map(|s|s.chars().take(180).collect())
}
fn failed(value:&Value)->bool{
    if value["ok"]==false||value["is_error"]==true||value["isError"]==true||value.get("error").is_some_and(|v|!v.is_null()){return true}
    if let Some(s)=value.as_str(){if s.starts_with("Error:")||s.starts_with("error:"){return true}if let Ok(v)=serde_json::from_str::<Value>(s){return failed(&v)}}
    false
}
fn structured(value:&Value)->Value{if let Some(s)=value.as_str(){serde_json::from_str(s).unwrap_or_else(|_|value.clone())}else{value.clone()}}

fn ingest(plan:&mut Plan,record:&Value,program:&str,native:&str,cwd:&Path)->Result<()>{
    if record["isSidechain"]==true||record["agent_id"].as_str().is_some_and(|s|!s.is_empty())||record["agentId"].as_str().is_some_and(|s|!s.is_empty()){return Ok(())}
    let payload=&record["payload"];let time=text(&record["timestamp"],64);
    if program=="codex"{
        match record["type"].as_str().unwrap_or_default(){
            "session_meta"=>{
                let id=payload["id"].as_str().or_else(||payload["session_id"].as_str());
                anyhow::ensure!(id==Some(native)&&same_cwd(&payload["cwd"],cwd),"native plan identity or working directory mismatch");
                anyhow::ensure!(payload["source"].get("subagent").is_none(),"subagent plan is not the main task plan");plan.bound=true;
            }
            "turn_context"=>{
                if !plan.bound{return Ok(())}
                if payload.get("cwd").is_some(){anyhow::ensure!(same_cwd(&payload["cwd"],cwd),"native plan working directory changed")}
                if let Some(id)=text(&payload["turn_id"],256){if plan.turn_id.as_ref().is_some_and(|old|old!=&id){plan.new_turn(None)}plan.turn_id=Some(id)}
            }
            "event_msg" if plan.bound=>{
                if payload["type"]=="task_started"{
                    if let Some(id)=text(&payload["turn_id"],256){if plan.turn_id.as_ref().is_some_and(|old|old!=&id){plan.new_turn(None)}plan.turn_id=Some(id)}
                }
                else if payload["type"]=="user_message"{plan.new_turn(public_label(&payload["message"]))}
                else if payload["type"]=="plan_update"{plan.steps=steps(&payload["plan"])?;plan.reported_at=time;plan.source=Some("codex.update_plan".into());plan.explanation=text(&payload["explanation"],2000)}
            }
            "response_item" if plan.bound=>{
                if payload["type"]=="message"&&payload["role"]=="user"{plan.new_turn(public_label(&payload["content"]))}
                else if payload["type"]=="function_call"{
                    if let (Some(id),Some(name),Some(arguments))=(payload["call_id"].as_str(),payload["name"].as_str(),payload["arguments"].as_str()){
                        if name.rsplit('.').next()==Some("update_plan"){plan.begin(id,name,serde_json::from_str(arguments).context("native plan arguments are invalid")?,time)}
                    }
                }else if payload["type"]=="custom_tool_call"&&payload["name"]=="exec"{
                    if let (Some(id),Some(input))=(payload["call_id"].as_str(),payload["input"].as_str()){
                        if let Some(args)=codex::literal_plan(input){plan.begin(id,"update_plan",args,time)}
                        else if input.contains("update_plan"){
                            // An opaque update may supersede the previous plan.
                            // Hide it rather than keep showing a stale completion.
                            plan.steps.clear();plan.pending.retain(|_,call|call.name!="update_plan");plan.incomplete_tasks=true;
                        }
                    }
                }else if payload["type"]=="function_call_output"{if let Some(id)=payload["call_id"].as_str(){plan.finish(id,failed(&payload["output"]),&structured(&payload["output"]))?}}
                else if payload["type"]=="custom_tool_call_output"{
                    if let Some(id)=payload["call_id"].as_str(){
                        let output=payload["output"].as_str().unwrap_or_default();
                        let success=output.starts_with("Script completed\n")&&output.lines().any(|line|line=="Output:")&&output.split_once("Output:").is_some_and(|(_,body)|body.trim().is_empty());
                        plan.finish(id,!success,&Value::Null)?;
                    }
                }
            }
            _=>{},
        }
    }else{
        if let Some(id)=record["sessionId"].as_str(){anyhow::ensure!(id==native,"native Claude plan belongs to another session");if record.get("cwd").is_some(){anyhow::ensure!(same_cwd(&record["cwd"],cwd),"native Claude plan working directory mismatch");plan.bound=true}}
        if !plan.bound{return Ok(())}
        let content=&record["message"]["content"];
        if record["type"]=="user"&&record["isMeta"]!=true{
            let is_prompt=content.is_string()||content.as_array().is_some_and(|parts|parts.iter().any(|p|p["type"]=="text"));
            if is_prompt{plan.new_turn(public_label(content))}
        }
        if let Some(parts)=content.as_array(){for part in parts{
            if record["type"]=="assistant"&&part["type"]=="tool_use"{
                if let (Some(id),Some(name))=(part["id"].as_str(),part["name"].as_str()){plan.begin(id,name,part["input"].clone(),time.clone())}
            }else if record["type"]=="user"&&part["type"]=="tool_result"{
                if let Some(id)=part["tool_use_id"].as_str(){let result=record.get("toolUseResult").or_else(||record.get("tool_use_result")).unwrap_or(&part["content"]);plan.finish(id,part["is_error"]==true||failed(result),&structured(result))?}
            }
        }}
    }
    Ok(())
}

struct Cache{path:PathBuf,created:Option<SystemTime>,modified:Option<SystemTime>,prefix:Vec<u8>,cursor:u64,plan:Plan}
static CACHES:OnceLock<Mutex<HashMap<String,Cache>>>=OnceLock::new();
fn response(desc:&Descriptor,state:&str,reason:Option<&str>)->Value{
    json!({"ok":true,"state":state,"program":desc.spawn.program,"session_id":desc.id,"steps":[],"reason":reason,"observed_at":chrono::Utc::now().to_rfc3339()})
}
pub(crate) fn read(desc:&Descriptor,saved:Option<&str>)->Result<Value>{
    if !matches!(desc.spawn.program.as_str(),"codex"|"claude"|"zcode-free")||desc.is_legacy_chat()||desc.spawn.sandbox.is_sandboxed(){return Ok(response(desc,"unsupported",Some("此 CLI 或执行模式尚未接入可信的任务计划。")))}
    let Some(native)=crate::terminal_restore::native_id(desc,saved)else{return Ok(response(desc,"unbound",Some("等待 CLI 上报此终端的原生会话标识，不会读取其他会话的计划。")))};
    if desc.spawn.program=="zcode-free"{return zcode::read(desc,&native)}
    let root=match provider_root(desc){Ok(root)=>root,Err(_)=>return Ok(response(desc,"unavailable",Some("此终端绑定账号的原生会话目录不可用。")))};
    let cwd=dunce::canonicalize(desc.spawn.cwd.as_deref().context("saved working directory required")?)?;
    let key=format!("{}|{}|{}|{}",desc.id,native,root.display(),cwd.display());
    let mut caches=CACHES.get_or_init(Default::default).lock().map_err(|_|anyhow::anyhow!("task progress reader unavailable"))?;
    if caches.len()>=64&&!caches.contains_key(&key){caches.clear()}
    if !caches.contains_key(&key){
        let Some(path)=find_history(&root,&native,if desc.spawn.program=="codex"{6}else{3})?else{return Ok(response(desc,"unavailable",Some("没有找到这个原生会话的记录，不会使用最近的其他会话。")))};
        caches.insert(key.clone(),Cache{path,created:None,modified:None,prefix:Vec::new(),cursor:0,plan:Plan::default()});
    }
    let cache=caches.get_mut(&key).unwrap();
    anyhow::ensure!(is_plain(&cache.path)?&&dunce::canonicalize(&cache.path)?.starts_with(&root),"native progress source left its account directory");
    let mut file=fs::File::open(&cache.path)?;let metadata=file.metadata()?;
    let created=metadata.created().ok();let modified=metadata.modified().ok();let length=metadata.len();
    let mut prefix=vec![0;cache.prefix.len().min(length as usize)];file.read_exact(&mut prefix)?;
    if prefix!=cache.prefix||length<cache.cursor||cache.created.is_some()&&cache.created!=created||length==cache.cursor&&cache.modified.is_some()&&cache.modified!=modified{cache.cursor=0;cache.plan=Plan::default()}
    cache.created=created;cache.modified=modified;
    let from=cache.cursor;let until=Instant::now()+Duration::from_millis(150);
    let mut reader=BufReader::new(file);reader.seek(SeekFrom::Start(cache.cursor))?;
    while cache.cursor<length&&cache.cursor-from<PAGE_BYTES&&Instant::now()<until{
        let mut bytes=Vec::new();let n=reader.by_ref().take(MAX_LINE+1).read_until(b'\n',&mut bytes)?;
        if n==0||bytes.last()!=Some(&b'\n'){if bytes.len() as u64>MAX_LINE{anyhow::bail!("native plan record is too large")}break}
        anyhow::ensure!(bytes.len() as u64<=MAX_LINE,"native plan record is too large");
        let result=serde_json::from_slice::<Value>(&bytes).context("native plan record is damaged")
            .and_then(|value|ingest(&mut cache.plan,&value,&desc.spawn.program,&native,&cwd));
        if let Err(error)=result{cache.plan=Plan::default();cache.cursor=0;cache.prefix.clear();return Err(error)}
        cache.cursor+=n as u64;
    }
    cache.prefix.resize(cache.cursor.min(4096) as usize,0);reader.seek(SeekFrom::Start(0))?;reader.read_exact(&mut cache.prefix)?;
    if cache.cursor<length{return Ok(response(desc,"scanning",Some("正在同步这个会话的计划记录，尚未把历史计划当作本轮进度。")))}
    anyhow::ensure!(cache.plan.bound,"native plan session identity could not be verified");
    if cache.plan.steps.is_empty()||cache.plan.incomplete_tasks{return Ok(response(desc,"no_plan",Some(if cache.plan.incomplete_tasks{"计划更新缺少可核实的完整步骤记录；等待 CLI 提供原生任务清单。"}else{"请让 CLI 先列出本轮任务步骤，并在推进时更新状态；不会根据日志量猜测进度。"})))}
    let plan=&cache.plan;let completed=plan.steps.iter().filter(|s|s.status=="completed").count();
    Ok(json!({"ok":true,"state":"ready","session_id":desc.id,"native_id":native,"program":desc.spawn.program,"task_label":plan.task_label,"steps":plan.steps,"completed":completed,"total":plan.steps.len(),"reported_complete":completed==plan.steps.len(),"source":plan.source,"explanation":plan.explanation,"reported_at":plan.reported_at,"observed_at":chrono::Utc::now().to_rfc3339()}))
}

#[cfg(test)]
#[path="terminal_progress_tests.rs"]
mod tests;
