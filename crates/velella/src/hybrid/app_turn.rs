//! Thread-scoped app-server turn observation. Not yet wired to remote execution.
use anyhow::{Context, Result};
use serde_json::Value;

pub(crate) struct AppTurn {
    thread: String,
    turn: Option<String>,
    acknowledged: bool,
    completed: bool,
    invalid: bool,
}
impl AppTurn {
    pub(crate) fn new(thread:&str)->Result<Self>{
        uuid::Uuid::parse_str(thread).context("invalid native thread identity")?;
        Ok(Self{thread:thread.into(),turn:None,acknowledged:false,completed:false,invalid:false})
    }
    fn bind_turn(&mut self,turn:&Value)->Result<()> {
        let id=turn["id"].as_str().context("native turn identity missing")?;
        uuid::Uuid::parse_str(id).context("invalid native turn identity")?;
        anyhow::ensure!(self.turn.as_deref().is_none_or(|old|old==id),"native turn identity changed");
        self.turn=Some(id.into());Ok(())
    }
    pub(crate) fn acknowledge(&mut self,response:&Value)->Result<()> {
        let result=(||{anyhow::ensure!(!self.invalid&&!self.acknowledged,"duplicate or invalid turn start response");
            let turn=&response["turn"];self.bind_turn(turn)?;
            anyhow::ensure!(turn["status"]=="inProgress"&&turn["error"].is_null(),"native turn did not start cleanly");
            self.acknowledged=true;Ok(())})();
        if result.is_err(){self.invalid=true;}result
    }
    pub(crate) fn notification(&mut self,method:&str,params:&Value)->Result<()> {
        if !matches!(method,"turn/started"|"turn/completed"){return Ok(())}
        let result=(||{anyhow::ensure!(!self.invalid,"native turn is already invalid");
            anyhow::ensure!(params["threadId"]==self.thread,"native thread identity changed");
            let turn=&params["turn"];self.bind_turn(turn)?;
            if method=="turn/started" {
                anyhow::ensure!(!self.completed&&turn["status"]=="inProgress"&&turn["error"].is_null(),"invalid native turn start");
            } else {
                anyhow::ensure!(!self.completed&&turn["status"]=="completed"&&turn["error"].is_null(),"native turn did not complete successfully");
                self.completed=true;
            } Ok(())})();
        if result.is_err(){self.invalid=true;}result
    }
    /// Caller must also verify process/transport and local audit completion.
    pub(crate) fn observed_complete(&self)->bool {!self.invalid&&self.acknowledged&&self.completed}
}

#[cfg(test)]
mod tests {
    use super::*;use serde_json::json;
    #[test]fn completion_requires_matching_acknowledgement_and_terminal_event(){
        let thread=uuid::Uuid::new_v4().to_string();let id=uuid::Uuid::new_v4().to_string();let mut state=AppTurn::new(&thread).unwrap();
        state.notification("turn/started",&json!({"threadId":thread,"turn":{"id":id,"status":"inProgress"}})).unwrap();
        state.notification("turn/completed",&json!({"threadId":thread,"turn":{"id":id,"status":"completed"}})).unwrap();
        assert!(!state.observed_complete());
        state.acknowledge(&json!({"turn":{"id":id,"status":"inProgress"}})).unwrap();assert!(state.observed_complete());
    }
    #[test]fn failures_wrong_id_and_duplicate_events_do_not_complete(){
        let thread=uuid::Uuid::new_v4().to_string();let id=uuid::Uuid::new_v4().to_string();let mut state=AppTurn::new(&thread).unwrap();
        state.acknowledge(&json!({"turn":{"id":id,"status":"inProgress"}})).unwrap();
        assert!(state.notification("turn/completed",&json!({"threadId":thread,"turn":{"id":uuid::Uuid::new_v4().to_string(),"status":"completed"}})).is_err());
        assert!(state.notification("turn/completed",&json!({"threadId":thread,"turn":{"id":id,"status":"completed"}})).is_err());assert!(!state.observed_complete());
        let mut failed=AppTurn::new(&thread).unwrap();failed.acknowledge(&json!({"turn":{"id":id,"status":"inProgress"}})).unwrap();
        assert!(failed.notification("turn/completed",&json!({"threadId":thread,"turn":{"id":id,"status":"failed","error":{"message":"failed"}}})).is_err());assert!(!failed.observed_complete());
        let mut duplicate=AppTurn::new(&thread).unwrap();duplicate.acknowledge(&json!({"turn":{"id":id,"status":"inProgress"}})).unwrap();
        duplicate.notification("turn/completed",&json!({"threadId":thread,"turn":{"id":id,"status":"completed"}})).unwrap();
        assert!(duplicate.notification("turn/completed",&json!({"threadId":thread,"turn":{"id":id,"status":"completed"}})).is_err());assert!(!duplicate.observed_complete());
    }
}
