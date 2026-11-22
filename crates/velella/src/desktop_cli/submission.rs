use super::*;
use std::time::Instant;

pub(super) fn observe(client:&Client,target:&Target,id:uuid::Uuid)->Result<Value> {
    let mut value=client.request(json!({"cmd":"workbench.feedback.observe","workspace_id":target.workspace,"session_id":target.terminal,"request_id":id}))?;
    if let Some(object)=value.as_object_mut(){object.remove("ok");}
    Ok(value)
}

pub(super) fn send(client:&Client,target:&Target,session:&Value,text:&str,enter:bool,id:Option<uuid::Uuid>,sequence:Option<u64>,wait:Option<u64>)->Result<Value> {
    if wait.is_some() {
        anyhow::ensure!(enter,"--wait-submit requires --enter");
        if session["status"]["submission_protocol"]!=1 || session["status"]["native_id"].as_str().is_none_or(str::is_empty) {
            return Err(failure("submission_not_supported","This session cannot confirm native execution. Start a supported session with --track-submission; no input was sent.",json!({"session_id":target.terminal})));
        }
    }
    let id=id.unwrap_or_else(uuid::Uuid::now_v7);
    let mut request=json!({"cmd":"workbench.feedback.send","workspace_id":target.workspace,"session_id":target.terminal,"request_id":id,"text":text,"draft":!enter});
    if let Some(sequence)=sequence {request["expected_input_sequence"]=json!(sequence);}
    if wait.is_some(){request["require_submission_tracking"]=json!(true);}
    let value=client.request(request)?;
    if !matches!(value["receipt"]["state"].as_str(),Some("drafted"|"submitted")) {
        return Err(failure("delivery_uncertain","Inspect the saved feedback receipt and terminal; do not send another prompt.",json!({"request_id":id,"receipt":value["receipt"]})));
    }
    let mut result=json!({"request_id":id,"receipt":value["receipt"],"turn_started_confirmed":false});
    let Some(seconds)=wait else{return Ok(result)};
    let deadline=Instant::now()+Duration::from_secs(seconds);
    loop {
        let remaining=deadline.saturating_duration_since(Instant::now());
        let observer=Client{root:client.root.clone(),endpoint:client.endpoint.clone(),remote:None,timeout:client.timeout.min(remaining.max(Duration::from_millis(1)))};
        match observe(&observer,target,id) {
            Ok(observed)=>{
                result["submission"]=observed["submission"].clone();
                result["turn_started_confirmed"]=json!(observed["turn_started_confirmed"]==true);
                if result["turn_started_confirmed"]==true || observed["submission"]["state"]=="unsupported" {return Ok(result);}
            },
            Err(error)=>{
                // A late IPC timeout must not discard the last valid, durable
                // observation (including its native prompt correlation state).
                if result.get("submission").is_none() {
                    result["submission"]=json!({"state":"unconfirmed","reason":"observation_unavailable"});
                }
                result["observation_error"]=json!({"message":error.to_string()});
                result["observation_timed_out"]=json!(Instant::now()>=deadline);
                return Ok(result);
            },
        }
        if Instant::now()>=deadline {result["observation_timed_out"]=json!(true);return Ok(result);}
        std::thread::sleep(Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::mock;
    fn target()->Target {Target{workspace:"w".into(),terminal:"s".into()}}
    #[test] fn old_hosts_fail_before_input_and_drafts_cannot_wait() {
        let client=Client{root:PathBuf::new(),endpoint:"unused".into(),remote:None,timeout:Duration::from_millis(5)};
        let error=send(&client,&target(),&json!({"status":{}}),"never sent",true,None,None,Some(1)).unwrap_err();
        assert_eq!(error.downcast_ref::<Failure>().unwrap().detail["code"],"submission_not_supported");
        assert!(Cli::try_parse_from(["velella-cli","terminal","send","--workspace","w","--terminal","s","--text","x","--wait-submit","1"]).is_err());
    }
    #[test] fn observing_a_started_turn_sends_exactly_once() {
        let (client,worker,root)=mock(vec![json!({"ok":true,"receipt":{"state":"submitted"}}),
            json!({"ok":true,"submission":{"state":"turn_started"},"turn_started_confirmed":true})]);
        let result=send(&client,&target(),&json!({"status":{"submission_protocol":1,"native_id":"native"}}),"fixture",true,None,None,Some(1)).unwrap();
        assert_eq!(result["turn_started_confirmed"],true);
        let requests=worker.join().unwrap();assert_eq!(requests.len(),2);assert_eq!(requests[0]["cmd"],"workbench.feedback.send");assert_eq!(requests[1]["cmd"],"workbench.feedback.observe");
        assert_eq!(requests[0]["request_id"],requests[1]["request_id"]);std::fs::remove_dir_all(root).unwrap();
    }
    #[test] fn failed_observation_keeps_the_last_valid_receipt_without_resending() {
        let (client,worker,root)=mock(vec![json!({"ok":true,"receipt":{"state":"submitted"}}),
            json!({"ok":true,"submission":{"state":"input_accepted","prompt_observed":false},"turn_started_confirmed":false}),
            json!({"ok":false,"error":"observer unavailable"})]);
        let result=send(&client,&target(),&json!({"status":{"submission_protocol":1,"native_id":"native"}}),"fixture",true,None,None,Some(1)).unwrap();
        assert_eq!(result["submission"]["prompt_observed"],false);assert_eq!(result["turn_started_confirmed"],false);
        assert!(result.get("observation_error").is_some());
        let requests=worker.join().unwrap();assert_eq!(requests.iter().filter(|r|r["cmd"]=="workbench.feedback.send").count(),1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
