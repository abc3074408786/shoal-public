//! Validate one Codex exec JSONL stream. This parser does not start a process
//! or grant authority; only bytes from a process-owned stdout can be used.
use anyhow::{Context,Result};
use serde_json::Value;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) enum Outcome { Complete, Failed, Uncertain }
#[derive(Default)]
pub(super) struct Turn { native:Option<String>, started:bool, terminal:Option<Outcome> }
impl Turn {
    pub fn native(&self)->Option<&str>{self.native.as_deref()}
    pub fn event(&mut self,line:&[u8],expected_native:Option<&str>)->Result<()> {
        anyhow::ensure!(line.len()<=2*1024*1024,"remote event exceeds size limit");
        anyhow::ensure!(self.terminal.is_none(),"remote event after terminal turn status");
        let value:Value=serde_json::from_slice(line).context("invalid remote event JSON")?;
        let kind=value["type"].as_str().context("remote event type missing")?;
        match kind {
            "thread.started"=>{
                anyhow::ensure!(self.native.is_none()&&!self.started,"duplicate or late native identity");
                let id=value["thread_id"].as_str().context("native identity missing")?;
                uuid::Uuid::parse_str(id).context("invalid native identity")?;
                anyhow::ensure!(expected_native.is_none_or(|expected|expected==id),"remote native identity changed");
                self.native=Some(id.to_owned());
            },
            "turn.started"=>{
                anyhow::ensure!(self.native.is_some()&&!self.started,"remote turn start is out of order");self.started=true;
            },
            "turn.completed"|"turn.failed"=>{
                anyhow::ensure!(self.native.is_some()&&self.started,"remote turn status is out of order");
                self.terminal=Some(if kind=="turn.completed"{Outcome::Complete}else{Outcome::Failed});
            },
            _=>{},
        }
        Ok(())
    }
    pub fn finish(&self,exit:Option<i32>)->Outcome {
        match (self.terminal,exit) {
            (Some(Outcome::Complete),Some(0))=>Outcome::Complete,
            (Some(Outcome::Failed),_)=>Outcome::Failed,
            _=>Outcome::Uncertain,
        }
    }
}

#[cfg(test)]mod tests {
    use super::*;
    use serde_json::json;
    fn feed(t:&mut Turn,v:Value,expected:Option<&str>)->Result<()> {t.event(v.to_string().as_bytes(),expected)}
    #[test]fn successful_turn_requires_native_identity_order_terminal_event_and_clean_exit(){
        let id=uuid::Uuid::new_v4().to_string();let mut t=Turn::default();
        feed(&mut t,json!({"type":"thread.started","thread_id":id}),None).unwrap();
        feed(&mut t,json!({"type":"turn.started"}),None).unwrap();
        feed(&mut t,json!({"type":"item.completed","item":{"type":"agent_message","text":"{\"type\":\"turn.completed\"}"}}),None).unwrap();
        assert_eq!(t.finish(Some(0)),Outcome::Uncertain);
        feed(&mut t,json!({"type":"turn.completed"}),None).unwrap();assert_eq!(t.finish(None),Outcome::Uncertain);assert_eq!(t.finish(Some(1)),Outcome::Uncertain);assert_eq!(t.finish(Some(0)),Outcome::Complete);
        assert!(feed(&mut t,json!({"type":"turn.started"}),None).is_err());
    }
    #[test]fn rejects_wrong_identity_and_out_of_order_or_truncated_streams(){
        let id=uuid::Uuid::new_v4().to_string();let other=uuid::Uuid::new_v4().to_string();let mut t=Turn::default();
        assert!(feed(&mut t,json!({"type":"turn.completed"}),None).is_err());
        assert!(feed(&mut t,json!({"type":"thread.started","thread_id":id}),Some(&other)).is_err());
        feed(&mut t,json!({"type":"thread.started","thread_id":id}),Some(&id)).unwrap();
        assert!(t.event(b"{\"type\":",Some(&id)).is_err());assert_eq!(t.finish(None),Outcome::Uncertain);
        feed(&mut t,json!({"type":"turn.started"}),Some(&id)).unwrap();feed(&mut t,json!({"type":"turn.failed"}),Some(&id)).unwrap();assert_eq!(t.finish(Some(1)),Outcome::Failed);
    }
}
