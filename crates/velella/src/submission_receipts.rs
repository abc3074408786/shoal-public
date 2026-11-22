//! Request-scoped native execution evidence. PTY writes and TUI states are not proof.
use crate::{host::{Descriptor,Status},ipc};
use anyhow::{Context,Result};
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};

pub(crate) const TRACKING_ENV:&str="VELELLA_SUBMISSION_TRACKING";

#[derive(Clone,Serialize,Deserialize)]
pub(crate) struct Receipt {
    request_id:String,
    session_id:String,
    host_pid:u32,
    native_id:String,
    input_sequence:u64,
    prompt_sha256:String,
    state:String,
    prompt_observed:bool,
    turn_id:Option<String>,
    event:Option<String>,
    observed_at:Option<String>,
}

pub(crate) fn digest(text:&str)->String {format!("{:x}",Sha256::digest(text.as_bytes()))}

fn save(desc:&Descriptor,receipt:&Receipt)->Result<()> {
    let dir=desc.directory.join("submission-receipts");std::fs::create_dir_all(&dir)?;
    ipc::atomic_json(&dir.join(format!("{}.json",receipt.request_id)),receipt)
}

pub(crate) fn prepare(desc:&Descriptor,status:&Status,request:&Value)->Result<Option<Receipt>> {
    let Some(spec)=request.get("submission") else{return Ok(None)};
    anyhow::ensure!(status.submission_protocol==1 && request["enter"]==true && request["text"]==""
        && request["input_kind"]=="user" && request["require_input_ready"]==true,"submission tracking requires guarded native Enter");
    let id=uuid::Uuid::parse_str(spec["request_id"].as_str().context("submission request ID required")?)?.to_string();
    let hash=spec["prompt_sha256"].as_str().context("prompt digest required")?;
    anyhow::ensure!(hash.len()==64&&hash.bytes().all(|b|b.is_ascii_hexdigit()),"invalid prompt digest");
    let native=status.native_id.as_ref().filter(|id|!id.is_empty()).context("native identity is not ready")?;
    let sequence=status.input_sequence.context("input sequence unavailable")?.checked_add(1).context("input sequence exhausted")?;
    let path=desc.directory.join("submission-receipts").join(format!("{id}.json"));
    anyhow::ensure!(!path.exists(),"submission request already exists; inspect its saved receipt");
    let receipt=Receipt{request_id:id,session_id:desc.id.clone(),host_pid:status.pid,native_id:native.clone(),input_sequence:sequence,
        prompt_sha256:hash.into(),state:"sending".into(),prompt_observed:false,turn_id:None,event:None,observed_at:None};
    save(desc,&receipt)?;Ok(Some(receipt))
}

pub(crate) fn accepted(desc:&Descriptor,status:&mut Status,mut receipt:Receipt)->Result<()> {
    receipt.state="input_accepted".into();save(desc,&receipt)?;status.pending_submission=Some(receipt);Ok(())
}

pub(crate) fn observe(desc:&Descriptor,status:&mut Status,event:&Value)->Result<()> {
    let Some(receipt)=status.pending_submission.as_mut() else{return Ok(())};
    if receipt.state=="turn_started" || status.input_sequence!=Some(receipt.input_sequence)
        || event["session_id"]!=receipt.native_id || event.get("agent_id").is_some_and(|v|!v.is_null()) {return Ok(())}
    let name=event["hook_event_name"].as_str().unwrap_or_default();
    if name=="UserPromptSubmit" {
        // Native hooks that omit or transform the prompt stay unconfirmed.
        if event["prompt"].as_str().is_some_and(|text|digest(text)==receipt.prompt_sha256) {
            receipt.prompt_observed=true;
            receipt.turn_id=event["turn_id"].as_str().map(str::to_owned);
            save(desc,receipt)?;
        }
    } else if receipt.prompt_observed && matches!(name,"PreToolUse"|"Stop") {
        if receipt.turn_id.as_deref().is_some_and(|id|event["turn_id"].as_str()!=Some(id)) {return Ok(())}
        // Prompt-submit hooks can still block a turn. Require subsequent native
        // tool execution or response completion; neither implies business success.
        receipt.state="turn_started".into();receipt.event=Some(name.into());
        receipt.observed_at=Some(chrono::Utc::now().to_rfc3339());save(desc,receipt)?;
    }
    Ok(())
}

pub(crate) fn read(desc:&Descriptor,id:&str,host_pid:u64,sequence:u64)->Result<Value> {
    let id=uuid::Uuid::parse_str(id)?.to_string();
    let path=desc.directory.join("submission-receipts").join(format!("{id}.json"));
    let receipt:Receipt=serde_json::from_slice(&std::fs::read(path).context("submission evidence is not available")?)?;
    anyhow::ensure!(receipt.request_id==id&&receipt.session_id==desc.id&&u64::from(receipt.host_pid)==host_pid
        &&receipt.input_sequence==sequence,"submission evidence belongs to another host or input");
    Ok(json!({"request_id":id,"session_id":receipt.session_id,"host_pid":receipt.host_pid,"native_id":receipt.native_id,
        "input_sequence":sequence,"state":receipt.state,"prompt_observed":receipt.prompt_observed,
        "turn_started_confirmed":receipt.state=="turn_started","event":receipt.event,"observed_at":receipt.observed_at,
        "completion_verified":false}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture()->(Descriptor,Status,String) {
        let root=std::env::temp_dir().join(format!("vl-submission-{}",uuid::Uuid::now_v7()));std::fs::create_dir_all(&root).unwrap();
        let desc=Descriptor{id:"s".into(),endpoint:"unused".into(),directory:root,spawn:crate::sessions::SpawnRequest{program:"codex".into(),args:vec![],cwd:None,env:Default::default(),sandbox:Default::default(),adapter:None}};
        (desc,Status{pid:42,input_sequence:Some(8),native_id:Some("native".into()),submission_protocol:1,..Status::default()},uuid::Uuid::now_v7().to_string())
    }
    fn accept(desc:&Descriptor,status:&mut Status,id:&str) {
        let receipt=prepare(desc,status,&json!({"submission":{"request_id":id,"prompt_sha256":digest("private fixture prompt")},"enter":true,"text":"","input_kind":"user","require_input_ready":true})).unwrap().unwrap();
        status.input_sequence=Some(9);accepted(desc,status,receipt).unwrap();
    }
    #[test] fn exact_prompt_then_execution_is_required_and_proof_survives_later_turns() {
        let (desc,mut status,id)=fixture();accept(&desc,&mut status,&id);
        for event in [json!({"hook_event_name":"SessionStart","session_id":"native"}),json!({"hook_event_name":"PreToolUse","session_id":"native"}),json!({"hook_event_name":"Stop","session_id":"native"}),json!({"hook_event_name":"UserPromptSubmit","session_id":"native","prompt":"other"})] {observe(&desc,&mut status,&event).unwrap();}
        assert_eq!(read(&desc,&id,42,9).unwrap()["turn_started_confirmed"],false);
        observe(&desc,&mut status,&json!({"hook_event_name":"UserPromptSubmit","session_id":"native","prompt":"private fixture prompt","turn_id":"t"})).unwrap();
        assert_eq!(read(&desc,&id,42,9).unwrap()["turn_started_confirmed"],false);
        observe(&desc,&mut status,&json!({"hook_event_name":"PreToolUse","session_id":"native","turn_id":"other"})).unwrap();
        assert_eq!(read(&desc,&id,42,9).unwrap()["turn_started_confirmed"],false);
        observe(&desc,&mut status,&json!({"hook_event_name":"PreToolUse","session_id":"native","turn_id":"t"})).unwrap();
        status.input_sequence=Some(10);let proof=read(&desc,&id,42,9).unwrap();assert_eq!(proof["turn_started_confirmed"],true);
        assert!(!proof.to_string().contains("private fixture prompt"));assert!(read(&desc,&id,43,9).is_err());
        assert!(read(&desc,&id,42,10).is_err());std::fs::remove_dir_all(desc.directory).unwrap();
    }
    #[test] fn newer_input_wrong_session_and_subagents_cannot_confirm_a_request() {
        let (desc,mut status,id)=fixture();accept(&desc,&mut status,&id);
        for event in [json!({"hook_event_name":"UserPromptSubmit","session_id":"foreign","prompt":"private fixture prompt"}),
            json!({"hook_event_name":"UserPromptSubmit","session_id":"native","agent_id":"child","prompt":"private fixture prompt"})] {observe(&desc,&mut status,&event).unwrap();}
        assert_eq!(read(&desc,&id,42,9).unwrap()["prompt_observed"],false);
        status.input_sequence=Some(10);observe(&desc,&mut status,&json!({"hook_event_name":"UserPromptSubmit","session_id":"native","prompt":"private fixture prompt"})).unwrap();
        assert_eq!(read(&desc,&id,42,9).unwrap()["prompt_observed"],false);std::fs::remove_dir_all(desc.directory).unwrap();
    }
}
