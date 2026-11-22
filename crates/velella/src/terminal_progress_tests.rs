use super::*;
use std::{fs,path::PathBuf,io::Write};

struct Fixture{root:PathBuf,desc:Descriptor,native:String,history:PathBuf}
impl Fixture{
    fn new(program:&str)->Self{
        let root=std::env::temp_dir().join(format!("vl-progress-{}",uuid::Uuid::now_v7()));
        let cwd=root.join("workspace");fs::create_dir_all(&cwd).unwrap();
        let home=root.join("account");fs::create_dir_all(&home).unwrap();
        let native=uuid::Uuid::now_v7().to_string();
        let history=if program=="codex"{home.join(format!("sessions/2026/11/05/rollout-{native}.jsonl"))}else{home.join(format!("projects/project/{native}.jsonl"))};
        fs::create_dir_all(history.parent().unwrap()).unwrap();
        let mut env=std::collections::HashMap::new();env.insert(if program=="codex"{"CODEX_HOME"}else{"CLAUDE_CONFIG_DIR"}.into(),home.to_string_lossy().into_owned());
        let desc=Descriptor{id:native.clone(),endpoint:"unused".into(),directory:root.join("terminal"),spawn:crate::sessions::SpawnRequest{program:program.into(),args:vec![],cwd:Some(cwd.to_string_lossy().into_owned()),env,adapter:Some("generic".into()),sandbox:Default::default()}};
        fs::create_dir_all(&desc.directory).unwrap();
        Self{root,desc,native,history}
    }
    fn write(&self,rows:Vec<Value>){fs::write(&self.history,rows.iter().map(|r|r.to_string()+"\n").collect::<String>()).unwrap()}
    fn append(&self,rows:Vec<Value>){let mut file=fs::OpenOptions::new().append(true).open(&self.history).unwrap();for row in rows{writeln!(file,"{row}").unwrap()}}
    fn snapshot(&self)->Value{read(&self.desc,Some(&self.native)).unwrap()}
    fn meta(&self)->Value{json!({"type":"session_meta","payload":{"id":self.native,"cwd":self.desc.spawn.cwd}})}
    fn plan(&self,call:&str,steps:Value)->Value{json!({"type":"response_item","timestamp":"2026-11-05T06:00:00Z","payload":{"type":"function_call","name":"update_plan","call_id":call,"arguments":json!({"plan":steps}).to_string()}})}
    fn result(&self,call:&str)->Value{json!({"type":"response_item","payload":{"type":"function_call_output","call_id":call,"output":"Plan updated"}})}
    fn claude(&self,kind:&str,content:Value)->Value{json!({"sessionId":self.native,"cwd":self.desc.spawn.cwd,"type":kind,"message":{"content":content},"timestamp":"2026-11-05T06:00:00Z"})}
    fn tool(&self,id:&str,name:&str,input:Value)->Value{self.claude("assistant",json!([{"type":"tool_use","id":id,"name":name,"input":input}]))}
    fn tool_result(&self,id:&str,result:Value)->Value{let mut row=self.claude("user",json!([{"type":"tool_result","tool_use_id":id,"content":"Success"}]));row["tool_use_result"]=result;row}
}

fn three(completed:usize)->Value{Value::Array((0..3).map(|i|json!({"step":format!("步骤 {}",i+1),"status":if i<completed{"completed"}else if i==completed{"in_progress"}else{"pending"}})).collect())}

#[test]
fn progress_updates_only_after_success_and_clears_for_next_task(){
    let f=Fixture::new("codex");f.write(vec![f.meta(),f.plan("first",three(1)),f.result("first")]);
    assert_eq!(f.snapshot()["completed"],1);
    f.append(vec![f.plan("second",three(2))]);assert_eq!(f.snapshot()["completed"],1,"unacknowledged plan stays pending");
    f.append(vec![f.result("other")]);assert_eq!(f.snapshot()["completed"],1,"wrong tool result cannot confirm a plan");
    f.append(vec![f.result("second")]);assert_eq!(f.snapshot()["completed"],2);
    f.append(vec![f.plan("failed",three(3)),json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"failed","output":"{\"ok\":false}"}})]);
    assert_eq!(f.snapshot()["completed"],2);
    f.append(vec![f.plan("last",three(3)),f.result("last")]);assert_eq!(f.snapshot()["reported_complete"],true);
    f.append(vec![json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"另一个任务"}]}})]);
    assert_eq!(f.snapshot()["state"],"no_plan");
}

#[test]
fn codex_turn_start_clears_previous_plan_before_new_prompt_arrives(){
    let f=Fixture::new("codex");f.write(vec![f.meta(),json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"one"}}),f.plan("p",three(3)),f.result("p")]);
    assert_eq!(f.snapshot()["completed"],3);
    f.append(vec![json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"two"}})]);
    assert_eq!(f.snapshot()["state"],"no_plan");
}

#[test]
fn claude_native_tasks_pair_ids_and_track_dependencies(){
    let f=Fixture::new("claude");f.write(vec![f.claude("user",json!("修复登录")),f.tool("a","TaskCreate",json!({"subject":"复现","activeForm":"正在复现"})),f.tool_result("a",json!({"task":{"id":"1","subject":"复现","status":"pending"}})),f.tool("b","TaskCreate",json!({"subject":"修复"})),f.tool_result("b",json!({"task":{"id":"2"}})),f.tool("c","TaskUpdate",json!({"taskId":"2","status":"in_progress","addBlockedBy":["1"]})),f.tool_result("c",json!({"success":true}))]);
    let actual=f.snapshot();assert_eq!(actual["state"],"ready");assert_eq!(actual["total"],2);assert_eq!(actual["steps"][1]["blocked_by"],json!(["1"]));
    f.append(vec![f.tool("d","TaskUpdate",json!({"id":"1","status":"completed"})),f.tool_result("d",json!({"success":true}))]);
    assert_eq!(f.snapshot()["completed"],1);
    f.append(vec![f.claude("user",json!("继续修复"))]);assert_eq!(f.snapshot()["state"],"no_plan");
    f.append(vec![f.tool("e","TaskUpdate",json!({"task_id":"2","status":"completed","removeBlockedBy":["1"]})),f.tool_result("e",json!({"success":true}))]);
    let actual=f.snapshot();assert_eq!(actual["completed"],2);assert_eq!(actual["task_label"],"继续修复");assert_eq!(actual["steps"][1]["blocked_by"],json!([]));
}

#[test]
fn claude_todos_failed_results_and_sidechains_are_separate(){
    let f=Fixture::new("claude");let todos=json!({"todos":[{"content":"复现","status":"in_progress","activeForm":"正在复现"}]});
    f.write(vec![f.claude("user",json!("任务")),f.tool("a","TodoWrite",todos.clone()),f.tool_result("a",json!({"is_error":true}))]);
    assert_eq!(f.snapshot()["state"],"no_plan");
    f.append(vec![f.tool("b","TodoWrite",todos),f.tool_result("b",json!({"success":true}))]);assert_eq!(f.snapshot()["steps"][0]["active_form"],"正在复现");
    let mut side=f.tool("side","TodoWrite",json!({"todos":[{"content":"子代理","status":"completed"}]}));side["isSidechain"]=json!(true);
    let mut result=f.tool_result("side",json!({"success":true}));result["isSidechain"]=json!(true);
    f.append(vec![side,result]);assert_eq!(f.snapshot()["steps"][0]["title"],"复现");
    f.append(vec![f.claude("user",json!("新任务"))]);assert_eq!(f.snapshot()["state"],"no_plan");
    f.append(vec![f.tool("missing","TaskUpdate",json!({"taskId":"99","status":"completed"})),f.tool_result("missing",json!({"success":true}))]);assert_eq!(f.snapshot()["state"],"no_plan");
}

#[test]
fn reader_rejects_wrong_identity_directory_and_ambiguous_sources(){
    for key in ["id","cwd"]{
        let f=Fixture::new("codex");let mut meta=f.meta();meta["payload"][key]=json!(if key=="id"{uuid::Uuid::now_v7().to_string()}else{f.root.to_string_lossy().into_owned()});
        f.write(vec![meta,f.plan("p",three(3)),f.result("p")]);assert!(read(&f.desc,Some(&f.native)).is_err());
    }
    let f=Fixture::new("codex");f.write(vec![f.meta()]);fs::copy(&f.history,f.history.parent().unwrap().join(format!("duplicate-{}.jsonl",f.native))).unwrap();
    assert!(read(&f.desc,Some(&f.native)).unwrap_err().to_string().contains("ambiguous"));
}

#[test]
fn same_native_id_in_other_account_is_never_used(){
    let f=Fixture::new("codex");f.write(vec![f.meta(),f.plan("p",three(3)),f.result("p")]);assert_eq!(f.snapshot()["completed"],3);
    let mut other=f.desc.clone();let empty=f.root.join("other-account");fs::create_dir_all(empty.join("sessions")).unwrap();other.spawn.env.insert("CODEX_HOME".into(),empty.to_string_lossy().into_owned());
    assert_eq!(read(&other,Some(&f.native)).unwrap()["state"],"unavailable");
}

#[test]
fn incomplete_lines_do_not_publish_earlier_plan_and_rewrite_resets_cache(){
    let f=Fixture::new("codex");f.write(vec![f.meta(),f.plan("old",three(3)),f.result("old")]);assert_eq!(f.snapshot()["completed"],3);
    let next=json!({"type":"response_item","payload":{"type":"message","role":"user","content":"新任务"}}).to_string();
    fs::OpenOptions::new().append(true).open(&f.history).unwrap().write_all(next.as_bytes()).unwrap();
    assert_eq!(f.snapshot()["state"],"scanning");
    fs::OpenOptions::new().append(true).open(&f.history).unwrap().write_all(b"\n").unwrap();assert_eq!(f.snapshot()["state"],"no_plan");
    let mut meta=f.meta();meta["payload"]["padding"]=json!("x".repeat(5000));f.write(vec![meta,f.plan("new",three(1)),f.result("new")]);
    assert_eq!(f.snapshot()["completed"],1,"rewrite larger than the old cursor cannot keep a stale completed plan");
}

#[test]
fn paginated_scans_and_unsupported_modes_have_explicit_states(){
    let f=Fixture::new("codex");let filler=json!({"type":"irrelevant","payload":"x".repeat(900_000)});
    f.write(vec![f.meta(),f.plan("old",three(3)),f.result("old"),filler.clone(),filler.clone(),filler.clone(),filler.clone(),filler.clone(),json!({"type":"event_msg","payload":{"type":"user_message","message":"新任务"}})]);
    assert_eq!(f.snapshot()["state"],"scanning");let mut actual=f.snapshot();for _ in 0..10{if actual["state"]!="scanning"{break}actual=f.snapshot()}
    assert_eq!(actual["state"],"no_plan");
    let shell=Fixture::new("powershell");assert_eq!(shell.snapshot()["state"],"unsupported");
    let unbound=Fixture::new("codex");assert_eq!(read(&unbound.desc,None).unwrap()["state"],"unbound");
}

#[test]
fn real_claude_2_1_238_transcript_replays_zero_to_three_completed_steps(){
    let f=Fixture::new("claude");f.write(vec![]);let mut counts=Vec::new();let mut active=false;
    for line in include_str!("../tests/fixtures/claude-task-plan-2.1.238.jsonl").lines(){
        let mut row:Value=serde_json::from_str(line).unwrap();row["sessionId"]=json!(f.native);row["cwd"]=json!(f.desc.spawn.cwd);
        f.append(vec![row]);let actual=f.snapshot();
        if actual["state"]=="ready"&&actual["total"]==3{
            counts.push(actual["completed"].as_u64().unwrap());
            active|=actual["steps"].as_array().unwrap().iter().any(|s|s["status"]=="in_progress");
        }
    }
    counts.dedup();assert_eq!(counts,vec![0,1,2,3]);assert!(active);assert_eq!(f.snapshot()["reported_complete"],true);
}

#[test]
fn native_codex_code_mode_plans_require_literal_calls_and_success_ack(){
    let f=Fixture::new("codex");f.write(vec![]);let mut counts=Vec::new();
    for line in include_str!("../tests/fixtures/codex-plan-code-mode-0.166.0.jsonl").lines(){
        let mut row:Value=serde_json::from_str(line).unwrap();if row["type"]=="session_meta"{row["payload"]["id"]=json!(f.native);row["payload"]["cwd"]=json!(f.desc.spawn.cwd)}
        f.append(vec![row]);let actual=f.snapshot();if actual["state"]=="ready"{counts.push(actual["completed"].as_u64().unwrap())}
    }
    counts.dedup();assert_eq!(counts,vec![0,1,2]);
    let literal="await tools.update_plan({plan:[{step:\"test\",status:\"completed\"}]});";
    f.append(vec![json!({"type":"response_item","payload":{"type":"custom_tool_call","name":"exec","call_id":"opaque","input":"await tools.update_plan(dynamicPlan);"}})]);
    assert_eq!(f.snapshot()["state"],"no_plan","unreadable subsequent updates cannot leave an old completed plan visible");
    for code in [format!("if (false) {{ {literal} }}"),format!("throw Error();{literal}"),format!("// {literal}"),format!("{literal} await tools.exec_command({{cmd:\"x\"}});"),"await tools.update_plan(plan);".into()]{assert!(codex::literal_plan(&code).is_none())}
    f.append(vec![json!({"type":"event_msg","payload":{"type":"user_message","message":"next task"}}),json!({"type":"response_item","payload":{"type":"custom_tool_call","name":"exec","call_id":"failed","input":literal}}),json!({"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"failed","output":"Script failed\nError"}})]);
    assert_eq!(f.snapshot()["state"],"no_plan");
}
impl Drop for Fixture{fn drop(&mut self){let _=fs::remove_dir_all(&self.root);}}

#[test]
fn task_steps_are_read_from_the_bound_native_plan(){
    let f=Fixture::new("codex");
    f.write(vec![f.meta(),json!({"type":"event_msg","payload":{"type":"user_message","message":"修复登录问题"}}),f.plan("p1",json!([{"step":"复现失败","status":"completed"},{"step":"修改代码","status":"in_progress"},{"step":"运行测试","status":"pending"}])),f.result("p1")]);
    let actual=read(&f.desc,Some(&f.native)).unwrap();
    assert_eq!(actual["state"],"ready");assert_eq!(actual["completed"],1);assert_eq!(actual["total"],3);
    assert_eq!(actual["steps"][1]["title"],"修改代码");assert_eq!(actual["steps"][2]["status"],"pending");
    assert_eq!(actual["task_label"],"修复登录问题");
}
