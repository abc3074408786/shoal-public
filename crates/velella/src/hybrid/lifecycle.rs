//! Validation for session-scoped remote native lifecycle receipts.
//!
//! Validation alone does not authenticate the producer or establish readiness.
//! The transport must bind its caller, expected directory and native identity
//! to the owning hybrid session before using any receipt.
use anyhow::{Context,Result};
use serde_json::{json,Value};

pub(super) fn receipt(event:&Value,remote_directory:&str,expected_native:Option<&str>)->Result<Value>{
    anyhow::ensure!(event.is_object(),"invalid remote lifecycle receipt");
    let name=event["hook_event_name"].as_str().context("remote lifecycle event required")?;
    anyhow::ensure!(matches!(name,"SessionStart"|"UserPromptSubmit"|"PreToolUse"|"PostToolUse"|"PermissionRequest"|"Notification"|"Stop"|"Interrupt"),"unsupported remote lifecycle event");
    anyhow::ensure!(event.get("agent_id").is_none_or(Value::is_null),"subagent lifecycle receipt rejected");
    let native=event["session_id"].as_str().context("remote native session required")?;
    uuid::Uuid::parse_str(native).context("invalid remote native session")?;
    anyhow::ensure!(event["cwd"].as_str()==Some(remote_directory),"remote lifecycle directory mismatch");
    match expected_native {
        Some(id)=>anyhow::ensure!(native==id,"remote lifecycle session mismatch"),
        None=>anyhow::ensure!(name=="SessionStart","remote native session has not started"),
    }
    let mut clean=json!({"hook_event_name":name,"session_id":native});
    if let Some(notification)=event.get("notification_type") {
        let text=notification.as_str().context("invalid remote notification")?;
        anyhow::ensure!(text.len()<=256&&!text.chars().any(char::is_control),"invalid remote notification");
        clean["notification_type"]=json!(text);
    }
    Ok(clean)
}

#[cfg(test)]mod tests {
    use super::*;
    #[test]fn lifecycle_receipts_are_scoped_and_drop_unneeded_content(){
        let native=uuid::Uuid::new_v4().to_string();let root="/home/user/.cache/velella-hybrid/session";
        let start=json!({"hook_event_name":"SessionStart","session_id":native,"cwd":root,"prompt":"private","tool_input":{"cmd":"not-a-hook"}});
        let clean=receipt(&start,root,None).unwrap();assert_eq!(clean,json!({"hook_event_name":"SessionStart","session_id":native}));
        let mut next=start.clone();next["hook_event_name"]=json!("Stop");assert!(receipt(&next,root,None).is_err());assert!(receipt(&next,root,Some(&native)).is_ok());
        next["cwd"]=json!("/tmp");assert!(receipt(&next,root,Some(&native)).is_err());next["cwd"]=json!(root);
        next["session_id"]=json!(uuid::Uuid::new_v4().to_string());assert!(receipt(&next,root,Some(&native)).is_err());
        next["session_id"]=json!(native);next["agent_id"]=json!("subagent");assert!(receipt(&next,root,Some(&native)).is_err());
        next["agent_id"]=Value::Null;next["hook_event_name"]=json!("unknown");assert!(receipt(&next,root,Some(&native)).is_err());
    }
}
