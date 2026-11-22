use super::{Config,tools};
use anyhow::{Context,Result};
use serde_json::{json,Value};
use std::{collections::{HashMap,HashSet},io::{BufRead,BufReader,Read,Write},net::{Shutdown,TcpListener,TcpStream},sync::{Arc,Mutex,atomic::{AtomicBool,AtomicUsize,Ordering}},time::Duration};

fn line(reader:&mut BufReader<TcpStream>)->Result<Option<Value>>{
    let mut bytes=Vec::new();let read=reader.take((2*tools::LIMIT+1)as u64).read_until(b'\n',&mut bytes)?;
    if read==0{return Ok(None)}
    anyhow::ensure!(bytes.len()<=2*tools::LIMIT&&bytes.last()==Some(&b'\n'),"invalid or oversized MCP frame");
    Ok(Some(serde_json::from_slice(&bytes)?))
}
fn serve(mut stream:TcpStream,config:&Config,token:&str)->Result<()>{
    // Windows accepts inherit the listener's nonblocking mode. A timeout alone
    // does not restore blocking reads, so an idle MCP peer used to be dropped.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut input=BufReader::new(stream.try_clone()?);
    let auth=line(&mut input)?.context("missing bridge authentication")?;
    anyhow::ensure!(auth["session_id"]==config.session_id&&auth["token"]==token,"invalid bridge authentication");
    stream.set_read_timeout(Some(Duration::from_secs(3600)))?;
    // Windows duplicated socket handles retain their own receive timeout.
    // The reader was cloned during the short authentication phase, so update
    // that handle too before waiting for native model/tool requests.
    input.get_ref().set_read_timeout(Some(Duration::from_secs(3600)))?;
    let connected=Arc::new(AtomicBool::new(true));let live=connected.clone();let watch=stream.try_clone()?;
    std::thread::spawn(move||{let mut byte=[0];while live.load(Ordering::SeqCst){match watch.peek(&mut byte){Ok(0)=>{live.store(false,Ordering::SeqCst);break},Ok(_)=>std::thread::sleep(Duration::from_millis(25)),Err(e)if matches!(e.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut)=>{},Err(_)=>{live.store(false,Ordering::SeqCst);break}}}});
    struct Connection{socket:TcpStream,live:Arc<AtomicBool>}
    impl Drop for Connection{fn drop(&mut self){self.live.store(false,Ordering::SeqCst);let _=self.socket.shutdown(std::net::Shutdown::Both);}}
    let _connection=Connection{socket:stream.try_clone()?,live:connected.clone()};
    let mut ids=HashSet::new();
    while let Some(req)=line(&mut input)?{
        anyhow::ensure!(req["jsonrpc"]=="2.0","invalid JSON-RPC version");
        let method=req["method"].as_str().context("MCP method missing")?;
        if req.get("id").is_none(){anyhow::ensure!(matches!(method,"notifications/initialized"|"notifications/cancelled"),"unknown notification");continue}
        let id=req["id"].clone();anyhow::ensure!(id.is_number()||id.as_str().is_some_and(|s|s.len()<=256),"invalid request identity");
        anyhow::ensure!(ids.len()<4096&&ids.insert(id.to_string()),"duplicate request identity or exhausted connection");
        let value=match method{
            "initialize"=>Ok(json!({"protocolVersion":req["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),"capabilities":{"tools":{}},"serverInfo":{"name":"velella-local-project","version":"1"},"instructions":"These tools operate on the user's LOCAL authoritative project. Native shell commands run REMOTELY. Local writes and commands require approval in Velella; never replay an uncertain request."})),
            "ping"=>Ok(json!({})),
            "tools/list"=>{
                let mut catalog=tools::catalog().as_array().cloned().unwrap_or_default();
                if config.collaboration_data.is_none(){catalog.retain(|tool|tool["name"]!="local_write_evidence");}
                if config.collaboration_data.is_some()&&!config.collaboration_writes {catalog.retain(|tool|!matches!(tool["name"].as_str(),Some("local_write_file"|"local_run_command")));}
                if config.collaboration_data.is_some(){catalog.push(crate::collaboration::mcp::catalog());}
                Ok(json!({"tools":catalog}))
            },
            "tools/call"=>{
                let name=req["params"]["name"].as_str().context("tool name missing")?;
                let result=if name=="collaboration"{
                    match config.collaboration_data.as_deref(){Some(data)=>crate::collaboration::mcp::request(data,&config.session_id,req["params"]["arguments"].clone()),None=>Err(anyhow::anyhow!("collaboration is not enabled for this session"))}
                }else{tools::call_connected(config,name,&req["params"]["arguments"],Some(&connected))};
                Ok(match result{Ok(v)=>json!({"content":[{"type":"text","text":serde_json::to_string(&v)?}]}),Err(e)=>json!({"isError":true,"content":[{"type":"text","text":e.to_string()}]})})
            },_=>Err(anyhow::anyhow!("unknown MCP method")),
        };
        let response=match value{Ok(value)=>json!({"jsonrpc":"2.0","id":id,"result":value}),Err(e)=>json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":e.to_string()}})};
        let mut bytes=serde_json::to_vec(&response)?;anyhow::ensure!(bytes.len()<=2*tools::LIMIT,"MCP response too large");bytes.push(b'\n');stream.write_all(&bytes)?;
    }Ok(())
}
pub struct Server{pub port:u16,stop:Arc<AtomicBool>,thread:Option<std::thread::JoinHandle<()>>,connections:Arc<Mutex<HashMap<usize,TcpStream>>>}
impl Server{
    pub fn start(config:Config,token:String)->Result<Self>{
        let listener=TcpListener::bind("127.0.0.1:0")?;let port=listener.local_addr()?.port();listener.set_nonblocking(true)?;
        let stop=Arc::new(AtomicBool::new(false));let flag=stop.clone();let active=Arc::new(AtomicUsize::new(0));let next=Arc::new(AtomicUsize::new(0));
        let connections:Arc<Mutex<HashMap<usize,TcpStream>>>=Arc::new(Mutex::new(HashMap::new()));let registry=connections.clone();
        let thread=std::thread::spawn(move||{while !flag.load(Ordering::SeqCst){match listener.accept(){
            Ok((stream,_))=>{
                if active.load(Ordering::SeqCst)>=4{drop(stream);continue}
                let Ok(copy)=stream.try_clone() else{continue};let id=next.fetch_add(1,Ordering::SeqCst);
                {let mut guard=registry.lock().unwrap();if flag.load(Ordering::SeqCst){let _=stream.shutdown(Shutdown::Both);continue}guard.insert(id,copy);}
                active.fetch_add(1,Ordering::SeqCst);let active=active.clone();let registry=registry.clone();let c=config.clone();let t=token.clone();std::thread::spawn(move||{if let Err(error)=serve(stream,&c,&t){let _=crate::ipc::atomic_json(&c.directory.join("hybrid-transport-error.json"),&json!({"error":error.to_string()}));}registry.lock().unwrap().remove(&id);active.fetch_sub(1,Ordering::SeqCst);});
            },
            Err(e)if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(20)),Err(_)=>break,
        }}});
        Ok(Self{port,stop,thread:Some(thread),connections})
    }
}
impl Drop for Server{fn drop(&mut self){self.stop.store(true,Ordering::SeqCst);for socket in self.connections.lock().unwrap().values(){let _=socket.shutdown(Shutdown::Both);}if let Some(t)=self.thread.take(){let _=t.join();}}}

#[cfg(test)]mod tests{
 use super::*;
 #[test]fn unauthenticated_peer_cannot_discover_tools(){
  let root=std::env::temp_dir().join(format!("velella-mcp-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&root).unwrap();
  let config=Config{session_id:uuid::Uuid::new_v4().to_string(),workspace_id:"w".into(),project_root:root.clone(),directory:root.clone(),profile:json!({"host":"agent"}),profile_fingerprint:"x".into(),collaboration_data:None,collaboration_writes:false};
  let session=config.session_id.clone();let server=Server::start(config,"secret".into()).unwrap();
  for auth in [json!({"session_id":"wrong","token":"secret"}),json!({"session_id":session,"token":"wrong"})]{let mut socket=TcpStream::connect(("127.0.0.1",server.port)).unwrap();socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();writeln!(socket,"{auth}").unwrap();let mut b=[0];assert_eq!(socket.read(&mut b).unwrap(),0);}
  drop(server);std::fs::remove_dir_all(root).unwrap();
 }
 #[test]fn authenticated_mcp_reads_local_file_and_disconnect_rejects_pending_write(){
  let root=std::env::temp_dir().join(format!("velella-mcp-ok-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&root).unwrap();let root=dunce::canonicalize(root).unwrap();std::fs::write(root.join("seed.txt"),"LOCAL_SEED").unwrap();
  let config=Config{session_id:uuid::Uuid::new_v4().to_string(),workspace_id:"w".into(),project_root:root.clone(),directory:root.clone(),profile:json!({"host":"agent"}),profile_fingerprint:"x".into(),collaboration_data:None,collaboration_writes:false};let server=Server::start(config.clone(),"secret".into()).unwrap();let mut socket=TcpStream::connect(("127.0.0.1",server.port)).unwrap();socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
  writeln!(socket,"{}",json!({"session_id":config.session_id,"token":"secret"})).unwrap();
  writeln!(socket,"{}",json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}})).unwrap();let mut input=BufReader::new(socket.try_clone().unwrap());assert_eq!(line(&mut input).unwrap().unwrap()["id"],1);
  writeln!(socket,"{}",json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"local_read_file","arguments":{"path":"seed.txt"}}})).unwrap();let result=line(&mut input).unwrap().unwrap();assert!(result.to_string().contains("LOCAL_SEED"));
  writeln!(socket,"{}",json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"local_write_file","arguments":{"request_id":"disconnect","path":"new.txt","content":"x","expected_digest":"absent"}}})).unwrap();
  let deadline=std::time::Instant::now()+Duration::from_secs(3);loop{if !tools::status(&config).unwrap()["operations"].as_array().unwrap().is_empty(){break}assert!(std::time::Instant::now()<deadline);std::thread::sleep(Duration::from_millis(10));}
  drop(input);drop(socket);loop{let state=tools::status(&config).unwrap()["operations"][0]["state"].clone();if state=="rejected"{break}assert!(std::time::Instant::now()<deadline);std::thread::sleep(Duration::from_millis(20));}
  assert!(!root.join("new.txt").exists());drop(server);std::fs::remove_dir_all(root).unwrap();
 }
 #[test]fn oversized_frame_is_rejected_before_any_operation(){
  let root=std::env::temp_dir().join(format!("velella-mcp-limit-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&root).unwrap();
  let config=Config{session_id:uuid::Uuid::new_v4().to_string(),workspace_id:"w".into(),project_root:root.clone(),directory:root.clone(),profile:json!({"host":"agent"}),profile_fingerprint:"x".into(),collaboration_data:None,collaboration_writes:false};let server=Server::start(config,"secret".into()).unwrap();
  let mut socket=TcpStream::connect(("127.0.0.1",server.port)).unwrap();socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();socket.set_write_timeout(Some(Duration::from_secs(3))).unwrap();let payload=vec![b'x';2*tools::LIMIT+1];let _=socket.write_all(&payload);let mut byte=[0];let result=socket.read(&mut byte);
  assert!(matches!(result,Ok(0))||result.is_err_and(|e|matches!(e.kind(),std::io::ErrorKind::ConnectionReset|std::io::ErrorKind::ConnectionAborted|std::io::ErrorKind::BrokenPipe)));assert!(!root.join("hybrid-audit.json").exists());drop(server);std::fs::remove_dir_all(root).unwrap();
 }
 #[test]fn collaboration_tool_requires_opt_in_and_exact_local_session_binding(){
  let root=std::env::temp_dir().join(format!("velella-mcp-collab-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&root).unwrap();let root=dunce::canonicalize(root).unwrap();
  let mut config=Config{session_id:uuid::Uuid::new_v4().to_string(),workspace_id:"w".into(),project_root:root.clone(),directory:root.clone(),profile:json!({"host":"agent"}),profile_fingerprint:"x".into(),collaboration_data:None,collaboration_writes:false};
  for enabled in [false,true]{
   if enabled {std::fs::create_dir(root.join("collaboration-sessions")).unwrap();std::fs::write(root.join("collaboration-sessions").join(format!("{}.json",config.session_id)),json!({"session_id":uuid::Uuid::new_v4().to_string(),"member_id":uuid::Uuid::new_v4().to_string()}).to_string()).unwrap();config.collaboration_data=Some(root.clone());}
   let server=Server::start(config.clone(),"secret".into()).unwrap();let mut socket=TcpStream::connect(("127.0.0.1",server.port)).unwrap();socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
   writeln!(socket,"{}",json!({"session_id":config.session_id,"token":"secret"})).unwrap();let mut input=BufReader::new(socket.try_clone().unwrap());
   writeln!(socket,"{}",json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap();let listing=line(&mut input).unwrap().unwrap();assert_eq!(listing.to_string().contains("\"collaboration\""),enabled);assert_eq!(listing.to_string().contains("local_write_file"),!enabled);
   writeln!(socket,"{}",json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"collaboration","arguments":{"cmd":"collab.status","member_id":"forged","token":"forged"}}})).unwrap();let result=line(&mut input).unwrap().unwrap();assert_eq!(result["result"]["isError"],true);let text=result.to_string();assert!(text.contains(if enabled{"session binding mismatch"}else{"collaboration is not enabled"}));assert!(!text.contains("forged"));
   if enabled {writeln!(socket,"{}",json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"local_write_file","arguments":{"request_id":"bad","path":"p","content":"x","expected_digest":"absent"}}})).unwrap();let denied=line(&mut input).unwrap().unwrap();assert_eq!(denied["result"]["isError"],true);assert!(!root.join("p").exists());}
   drop(input);drop(socket);drop(server);
  }
  std::fs::remove_dir_all(root).unwrap();
 }
 #[test]fn shutting_down_server_closes_existing_authenticated_connections(){
  let root=std::env::temp_dir().join(format!("velella-mcp-shutdown-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&root).unwrap();let root=dunce::canonicalize(root).unwrap();
  let config=Config{session_id:uuid::Uuid::new_v4().to_string(),workspace_id:"w".into(),project_root:root.clone(),directory:root.clone(),profile:json!({"host":"agent"}),profile_fingerprint:"x".into(),collaboration_data:None,collaboration_writes:false};
  let server=Server::start(config.clone(),"secret".into()).unwrap();let mut socket=TcpStream::connect(("127.0.0.1",server.port)).unwrap();socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
  writeln!(socket,"{}",json!({"session_id":config.session_id,"token":"secret"})).unwrap();writeln!(socket,"{}",json!({"jsonrpc":"2.0","id":1,"method":"ping"})).unwrap();
  let mut reader=BufReader::new(socket.try_clone().unwrap());assert_eq!(line(&mut reader).unwrap().unwrap()["id"],1);drop(server);
  let result=line(&mut reader);assert!(matches!(result,Ok(None))||result.is_err());drop(reader);drop(socket);std::fs::remove_dir_all(root).unwrap();
 }
 #[cfg(windows)]
 #[test]fn authenticated_connection_survives_the_short_authentication_deadline(){
  let root=std::env::temp_dir().join(format!("velella-mcp-idle-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&root).unwrap();
  let config=Config{session_id:uuid::Uuid::new_v4().to_string(),workspace_id:"w".into(),project_root:root.clone(),directory:root.clone(),profile:json!({"host":"fixture"}),profile_fingerprint:"fixture".into(),collaboration_data:None,collaboration_writes:false};
  let server=Server::start(config.clone(),"fixture-token".into()).unwrap();let mut socket=TcpStream::connect(("127.0.0.1",server.port)).unwrap();socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
  writeln!(socket,"{}",json!({"session_id":config.session_id,"token":"fixture-token"})).unwrap();let mut reader=BufReader::new(socket.try_clone().unwrap());
  writeln!(socket,"{}",json!({"jsonrpc":"2.0","id":1,"method":"ping"})).unwrap();assert_eq!(line(&mut reader).unwrap().unwrap()["id"],1);
  std::thread::sleep(Duration::from_secs(11));
  writeln!(socket,"{}",json!({"jsonrpc":"2.0","id":2,"method":"ping"})).unwrap();assert_eq!(line(&mut reader).unwrap().context("authenticated bridge closed at the auth deadline").unwrap()["id"],2);
  drop(reader);drop(socket);drop(server);std::fs::remove_dir_all(root).unwrap();
 }
}
