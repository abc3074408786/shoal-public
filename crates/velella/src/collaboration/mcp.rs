//! Session-scoped stdio MCP transport for the existing authenticated collaboration protocol.
use anyhow::{Context,Result};
use serde_json::{json,Value};
use std::{io::{BufRead,Read,Write},path::Path};
mod external;
pub(crate) use external::{descriptor as external_descriptor, serve as external_serve};
const COMMANDS:&[&str]=&["collab.status","collab.receive","collab.ack","collab.started","collab.report","collab.review.report","collab.candidate","collab.reconcile","collab.plan.submit","collab.revision.propose","collab.retry.propose"];
pub(crate) fn catalog()->Value {json!({"name":"collaboration","description":format!("Read durable assignments; report started and fixed candidate. A report never accepts work. Use stable request_id for mutations. {}",super::orchestration::evidence::instructions()),"inputSchema":{"type":"object","properties":{"cmd":{"type":"string","enum":COMMANDS},"request_id":{"type":"string"},"attempt_id":{"type":"string"},"work_id":{"type":"string"},"review_id":{"type":"string"},"revision":{"type":"integer"},"plan":{"type":"object"},"candidate":{"type":"object"},"evidence":{"type":"object"},"outcome":{"enum":["completed","failed"]},"verdict":{"enum":["passed","failed"]},"summary":{"type":"string"},"reason":{"type":"string"},"new_evidence":{"type":"string"},"cause_key":{"description":"stable, human-assigned root-cause key for a bounded retry","type":"string"},"seq":{"type":"integer"}},"required":["cmd"]}})}
fn bind_request(data:&Path,session:&str,mut req:Value)->Result<Value>{
    let session=uuid::Uuid::parse_str(session)?;
    let binding:Value=serde_json::from_slice(&std::fs::read(data.join("collaboration-sessions").join(format!("{session}.json")))?)?;
    anyhow::ensure!(binding["session_id"]==session.to_string(),"session binding mismatch");
    let member=uuid::Uuid::parse_str(binding["member_id"].as_str().context("member missing")?)?;
    let cred:Value=serde_json::from_slice(&std::fs::read(data.join("collaboration-clients").join(format!("{member}.json")))?)?;
    anyhow::ensure!(cred["member_id"]==member.to_string(),"credential binding mismatch");
    anyhow::ensure!(req.is_object(),"tool arguments must be an object");
    let cmd=req["cmd"].as_str().context("cmd required")?;
    anyhow::ensure!(COMMANDS.contains(&cmd),"control command is not available to an Agent");
    if !matches!(cmd,"collab.status"|"collab.receive"|"collab.candidate"|"collab.reconcile") {uuid::Uuid::parse_str(req["request_id"].as_str().context("mutations require a stable request_id")?)?;}
    for key in ["run_id","member_id","token"]{req[key]=cred[key].clone();}
    // A handed-off member retains its credential, so bind every request to the
    // actual calling session; otherwise a surviving old bridge can impersonate
    // the replacement session after the member identity has moved.
    req["native_session_id"]=json!(session.to_string());
    req["wait_ms"]=json!(0);
    Ok(req)
}
pub fn request(data:&Path,session:&str,req:Value)->Result<Value>{
    let req=bind_request(data,session,req)?;
    crate::ipc::request(&crate::ipc::endpoint(data,"daemon"),&req)
}
pub fn serve(data:&Path,session:&str)->Result<()> {
    uuid::Uuid::parse_str(session)?;
    transport(std::io::stdin().lock(),std::io::stdout().lock(),|req|request(data,session,req))
}
fn transport(mut input:impl BufRead,mut output:impl Write,mut call:impl FnMut(Value)->Result<Value>)->Result<()> {
    loop {
        let mut line=Vec::new();let size=(&mut input).take(1024*1024+1).read_until(b'\n',&mut line)?;
        if size==0{break}anyhow::ensure!(size<=1024*1024,"MCP request too large");
        let req:Value=match serde_json::from_slice(&line){Ok(req)=>req,Err(_)=>{writeln!(output,"{}",json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}))?;output.flush()?;continue}};
        if req["jsonrpc"]!="2.0" || !req["method"].is_string() || req.get("id").is_some_and(|id|!id.is_string() && !id.is_number() && !id.is_null()) {
            writeln!(output,"{}",json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid request"}}))?;output.flush()?;continue
        }
        let Some(id)=req.get("id") else{continue};
        let result=match req["method"].as_str().unwrap_or("") {
            "initialize"=>Ok(json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"velella-collaboration","version":"2"}})),
            "ping"=>Ok(json!({})),
            "tools/list"=>Ok(json!({"tools":[catalog()]})),
            "tools/call"=>{
                let answer=if req["params"]["name"]!="collaboration"{Err(anyhow::anyhow!("unknown tool"))}else{call(req["params"]["arguments"].clone())};
                Ok(match answer {Ok(v)=>json!({"content":[{"type":"text","text":v.to_string()}],"isError":v["ok"]==false}),Err(e)=>json!({"content":[{"type":"text","text":e.to_string()}],"isError":true})})
            }
            _=>Err(anyhow::anyhow!("unknown method")),
        };
        let response=match result {Ok(v)=>json!({"jsonrpc":"2.0","id":id,"result":v}),Err(e)=>json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":e.to_string()}})};
        writeln!(output,"{response}")?;output.flush()?;
    }Ok(())
}

#[cfg(test)]mod tests {
    use super::*;
    #[test]fn mcp_transport_recovers_from_bad_json_and_keeps_notifications_silent(){
        let input=b"bad\n[]\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"ping\"}\n";
        let mut out=Vec::new();transport(&input[..],&mut out,|_|panic!("no tool calls")).unwrap();
        let lines:Vec<Value>=String::from_utf8(out).unwrap().lines().map(|s|serde_json::from_str(s).unwrap()).collect();
        assert_eq!(lines.len(),3);assert_eq!(lines[0]["error"]["code"],-32700);assert_eq!(lines[1]["error"]["code"],-32600);assert_eq!(lines[2]["id"],4);
    }
    #[test]fn mcp_transport_bounds_input_and_does_not_call_unknown_tools(){
        let mut out=Vec::new();let large=vec![b'x';1024*1024+1];assert!(transport(&large[..],&mut out,|_|panic!()).is_err());
        let input=json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"launch","arguments":{}}}).to_string()+"\n";
        transport(input.as_bytes(),&mut out,|_|panic!("unknown tool cannot dispatch")).unwrap();
        let response:Value=serde_json::from_slice(&out).unwrap();assert_eq!(response["result"]["isError"],true);
    }
    #[test]fn mcp_binding_replaces_forged_credentials_and_restricts_commands(){
        let root=std::env::temp_dir().join(format!("velella-collab-binding-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("collaboration-sessions")).unwrap();std::fs::create_dir_all(root.join("collaboration-clients")).unwrap();
        let session=uuid::Uuid::new_v4().to_string();let member=uuid::Uuid::new_v4().to_string();let run=uuid::Uuid::new_v4().to_string();
        std::fs::write(root.join("collaboration-sessions").join(format!("{session}.json")),json!({"session_id":session,"member_id":member}).to_string()).unwrap();
        std::fs::write(root.join("collaboration-clients").join(format!("{member}.json")),json!({"run_id":run,"member_id":member,"token":"private-token"}).to_string()).unwrap();
        let bound=bind_request(&root,&session,json!({"cmd":"collab.status","run_id":"forged","member_id":"forged","token":"forged","native_session_id":"replacement","wait_ms":9999})).unwrap();
        assert_eq!(bound["run_id"],run);assert_eq!(bound["member_id"],member);assert_eq!(bound["token"],"private-token");assert_eq!(bound["native_session_id"],session);assert_eq!(bound["wait_ms"],0);
        assert!(bind_request(&root,&session,json!({"cmd":"collab.launch"})).is_err());assert!(bind_request(&root,&session,json!({"cmd":"collab.report","request_id":"invalid"})).is_err());
        assert!(bind_request(&root,&uuid::Uuid::new_v4().to_string(),json!({"cmd":"collab.status"})).is_err());std::fs::remove_dir_all(root).unwrap();
    }
}
