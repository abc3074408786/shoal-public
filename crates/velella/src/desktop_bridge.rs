//! Electron uses the native owner-only listener on every OS; only JSON travels
//! over the private parent/child stdio pipes. Closing the GUI never stops PTYs.
use anyhow::Result;
use interprocess::local_socket::prelude::*;
use serde_json::{json, Value};
use std::{collections::HashMap, io::{BufRead, Write}, sync::{Arc, Mutex, mpsc}, time::Duration};

pub fn run(root: &std::path::Path) -> Result<()> {
    let listener=crate::ipc::listen_recovering(&crate::ipc::endpoint(root,"browser"))?;
    let pending:Arc<Mutex<HashMap<String,mpsc::Sender<Value>>>>=Arc::new(Mutex::new(HashMap::new()));
    let replies=pending.clone();
    std::thread::spawn(move||{
        for line in std::io::stdin().lock().lines() {
            let Ok(line)=line else{break};
            if line.len()>16*1024*1024 {break}
            if let Ok(message)=serde_json::from_str::<Value>(&line) {
                if let Some(id)=message["id"].as_str() {
                    if let Some(tx)=replies.lock().unwrap().remove(id) {let _=tx.send(message["response"].clone());}
                }
            }
        }
        // Only this dedicated IPC relay exits, never the daemon or terminal hosts.
        std::process::exit(0);
    });
    println!("{}",json!({"ready":true}));std::io::stdout().flush()?;
    for stream in listener.incoming() {
        let stream=stream?;let pending=pending.clone();
        std::thread::spawn(move||crate::ipc::handle(stream,move|request|{
            let id=uuid::Uuid::now_v7().to_string();let (tx,rx)=mpsc::channel();
            pending.lock().unwrap().insert(id.clone(),tx);
            {let mut out=std::io::stdout().lock();writeln!(out,"{}",json!({"id":id,"request":request}))?;out.flush()?;}
            let reply=rx.recv_timeout(Duration::from_secs(50));pending.lock().unwrap().remove(&id);
            Ok(reply?)
        }));
    }
    Ok(())
}
