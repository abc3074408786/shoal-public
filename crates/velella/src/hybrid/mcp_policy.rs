//! Fail-closed checks for a thread-scoped native MCP status receipt.
//! This validator is not yet connected to remote execution.
use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::BTreeSet;

pub(crate) fn verify_thread_servers(receipt:&Value,writes:bool)->Result<()> {
    anyhow::ensure!(receipt.get("nextCursor").is_none_or(Value::is_null),"native MCP status is incomplete");
    let servers=receipt["data"].as_array().context("native MCP status is missing")?;
    anyhow::ensure!(servers.len()<=128,"native MCP status exceeds limit");
    let mut seen=BTreeSet::new();let mut bridge=false;
    for server in servers {
        let name=server["name"].as_str().context("native MCP server name missing")?;
        anyhow::ensure!(!name.is_empty()&&name.len()<=256&&seen.insert(name),"invalid or duplicate native MCP server");
        let tools=server["tools"].as_object().context("native MCP tool catalog missing")?;
        anyhow::ensure!(server["toolsError"].is_null(),"native MCP tool catalog failed");
        if name=="velella_local" {
            anyhow::ensure!(server["runtimeStatus"]=="connected","local bridge is not connected");
            let mut expected:BTreeSet<&str>=["collaboration","local_list_files","local_read_file","local_write_evidence"].into_iter().collect();
            if writes {expected.extend(["local_write_file","local_run_command"]);}
            let actual:BTreeSet<&str>=tools.keys().map(String::as_str).collect();
            anyhow::ensure!(actual==expected,"local bridge tool catalog does not match role authority");bridge=true;
        } else {
            anyhow::ensure!(server["runtimeStatus"]=="disabled"&&tools.is_empty(),"an unselected native MCP server is active or its status is unknown");
        }
    }
    anyhow::ensure!(bridge,"local collaboration bridge is missing");Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;use serde_json::json;
    fn receipt()->Value {json!({"nextCursor":null,"data":[{"name":"velella_local","runtimeStatus":"connected","tools":{"collaboration":{},"local_list_files":{},"local_read_file":{},"local_write_evidence":{}}},{"name":"inherited","runtimeStatus":"disabled","tools":{}}]})}
    #[test] fn disabled_inherited_server_is_accepted_but_active_or_unknown_is_rejected(){
        let mut v=receipt();assert!(verify_thread_servers(&v,false).is_ok());
        v["data"][1]["runtimeStatus"]=json!("connected");assert!(verify_thread_servers(&v,false).is_err());
        v["data"][1]["runtimeStatus"]=Value::Null;assert!(verify_thread_servers(&v,false).is_err());
        v["data"][1]["runtimeStatus"]=json!("disabled");v["data"][1]["tools"]=json!({"unexpected":{}});assert!(verify_thread_servers(&v,false).is_err());
    }
    #[test] fn bridge_permissions_and_complete_receipt_are_required(){
        let mut v=receipt();assert!(verify_thread_servers(&v,true).is_err());
        v["data"][0]["tools"]["local_write_file"]=json!({});v["data"][0]["tools"]["local_run_command"]=json!({});
        assert!(verify_thread_servers(&v,true).is_ok());assert!(verify_thread_servers(&v,false).is_err());
        v["nextCursor"]=json!("more");assert!(verify_thread_servers(&v,true).is_err());
        v["nextCursor"]=Value::Null;v["data"][0]["toolsError"]=json!("failed");assert!(verify_thread_servers(&v,true).is_err());
    }
}
