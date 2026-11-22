//! PKCE stays on the desktop. Browser OAuth uses a temporary loopback callback.
use super::{credentials, normalize, provider, Source, Store};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use qrcode::{render::svg, QrCode};
use reqwest::{blocking::Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, io::{Read,Write}, net::{TcpListener,TcpStream}, path::{Path, PathBuf}, sync::{Arc, Mutex, OnceLock}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

const TOKEN_URL: &str = "https://api.linear.app/oauth/token";
const API_URL: &str = "https://api.linear.app";
const LIMIT: Duration = Duration::from_secs(600);
const LOCAL_CALLBACK: &str = "http://127.0.0.1:17887/callback";
#[derive(Deserialize)]
struct Config { #[serde(default)] relay_url: String, client_id: String }
impl Config {
    fn read(root: &Path) -> Result<Self> {
        let bytes=std::fs::read(root.join("linear-oauth.json")).context("Linear 授权应用尚未配置")?;
        anyhow::ensure!(bytes.len()<4096,"Linear 授权配置无效");
        let mut config:Self=serde_json::from_slice(&bytes).context("Linear 授权配置无效")?;
        anyhow::ensure!(!config.client_id.is_empty()&&config.client_id.len()<=200&&!config.client_id.chars().any(char::is_control),"Linear 应用标识无效");
        if !config.relay_url.is_empty(){
            let url=Url::parse(&config.relay_url).context("Linear 授权服务地址无效")?;
            anyhow::ensure!(url.scheme()=="https"&&url.host_str().is_some()&&url.username().is_empty()&&url.password().is_none()&&url.query().is_none()&&url.fragment().is_none()&&url.path()=="/","Linear 授权服务必须使用 HTTPS 站点地址");
            config.relay_url=url.as_str().trim_end_matches('/').into();
        }
        Ok(config)
    }
}
#[derive(Clone, Serialize, Default)]
struct View {
    request_id:String, state:String, authorization_url:Option<String>, qr_svg:Option<String>,
    message:Option<String>, source:Option<Source>,
}
struct Attempt { view:Mutex<View>, listener:Mutex<Option<TcpListener>>, created:Instant }
impl Attempt {
    fn new(id:&str)->Self {Self{view:Mutex::new(View{request_id:id.into(),state:"starting".into(),..View::default()}),listener:Mutex::new(None),created:Instant::now()}}
    fn snapshot(&self)->Value {json!({"ok":true,"attempt":self.view.lock().unwrap().clone()})}
    fn active(&self)->bool {matches!(self.view.lock().unwrap().state.as_str(),"starting"|"waiting")}
    fn finish(&self,state:&str,message:&str) {
        let mut view=self.view.lock().unwrap();if !matches!(view.state.as_str(),"starting"|"waiting"){return;}
        view.state=state.into();view.message=Some(message.into());view.authorization_url=None;view.qr_svg=None;
        self.listener.lock().unwrap().take();
    }
}
#[derive(Default)]
struct Registry { attempts:HashMap<String,Arc<Attempt>> }
impl Registry {
    fn reserve(&mut self,id:&str,cancel:bool)->Result<(Arc<Attempt>,bool)> {
        if let Some(a)=self.attempts.get(id){if cancel{a.finish("cancelled","已停止等待授权。");}return Ok((a.clone(),false));}
        self.attempts.retain(|_,a|a.active()||a.created.elapsed()<Duration::from_secs(3600));
        anyhow::ensure!(self.attempts.len()<32,"连接尝试过多，请稍后再试");
        if !cancel{anyhow::ensure!(!self.attempts.values().any(|a|a.active()),"另一个窗口正在等待 Linear 授权，请先完成或取消");}
        let a=Arc::new(Attempt::new(id));if cancel{a.finish("cancelled","已停止等待授权。");}
        self.attempts.insert(id.into(),a.clone());Ok((a,!cancel))
    }
}
fn registries()->&'static Mutex<HashMap<PathBuf,Registry>> {
    static REGISTRY:OnceLock<Mutex<HashMap<PathBuf,Registry>>>=OnceLock::new();REGISTRY.get_or_init(Default::default)
}
fn client()->Result<Client> {Ok(Client::builder().connect_timeout(Duration::from_secs(8)).timeout(Duration::from_secs(20)).redirect(reqwest::redirect::Policy::none()).build()?)}
fn response(response:reqwest::blocking::Response)->Result<Value> {
    anyhow::ensure!(response.status().is_success(),"授权服务暂不可用或授权已过期，请重新连接");
    let mut bytes=Vec::new();response.take(32769).read_to_end(&mut bytes).context("无法读取授权结果")?;
    anyhow::ensure!(bytes.len()<=32768,"授权结果过大");serde_json::from_slice(&bytes).context("授权结果无效")
}
fn secret()->String {format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple())}
fn challenge(verifier:&str)->String {URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))}
fn qr_svg(url:&str)->Result<String>{Ok(QrCode::new(url)?.render::<svg::Color>().min_dimensions(256,256).quiet_zone(true).dark_color(svg::Color("#000000")).light_color(svg::Color("#ffffff")).build())}
fn epoch()->u64 {SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()}
fn field<'a>(v:&'a Value,key:&str)->Result<&'a str>{v[key].as_str().filter(|s|!s.is_empty()&&s.len()<=16384&&!s.chars().any(char::is_control)).context("授权结果缺少必要字段")}

pub(super) fn dispatch(root:&Path,req:&Value)->Result<Value> {
    let cmd=req["cmd"].as_str().unwrap_or("");
    if cmd=="sources.linear.account" {
        return match Config::read(root){Ok(c)=>Ok(json!({"ok":true,"configured":true,"mode":if c.relay_url.is_empty(){"browser"}else{"qr"}})),Err(_)=>Ok(json!({"ok":true,"configured":false,"mode":"browser","message":"Linear 授权应用尚未配置。配置完成后可在浏览器中一键连接，也可以使用 API Key 手动连接。"}))};
    }
    let id=uuid::Uuid::parse_str(req["request_id"].as_str().context("连接请求标识缺失")?)?.to_string();
    if cmd=="sources.linear.auth.status" {
        let registry=registries().lock().unwrap();
        return Ok(registry.get(root).and_then(|r|r.attempts.get(&id)).context("授权等待已结束，请重新连接")?.snapshot());
    }
    anyhow::ensure!(matches!(cmd,"sources.linear.auth.start"|"sources.linear.auth.cancel"),"不支持的 Linear 连接操作");
    let cancel=cmd=="sources.linear.auth.cancel";
    let (attempt,start)=registries().lock().unwrap().entry(root.to_path_buf()).or_default().reserve(&id,cancel)?;
    if start {
        let config=match Config::read(root){Ok(c)=>c,Err(_)=>{attempt.finish("failed","Linear 授权应用尚未配置。");return Ok(attempt.snapshot());}};
        let running=attempt.clone();let root=root.to_path_buf();
        if std::thread::Builder::new().name("linear-pkce-login".into()).spawn(move||{
            let result=if config.relay_url.is_empty(){run_browser_login(&root,&running,&config,TOKEN_URL,API_URL)}else{run_login(&root,&running,&config,TOKEN_URL,API_URL)};
            if result.is_err(){running.finish("failed","连接未完成、授权已过期或网络不可用，请重新连接。");}
        }).is_err(){attempt.finish("failed","无法启动授权，请重试。");}
    }
    Ok(attempt.snapshot())
}

enum Callback { Code(String), Denied }
fn callback(request:&str,state:&str,port:u16)->Result<Callback>{
    let mut lines=request.split("\r\n");let first:Vec<_>=lines.next().unwrap_or("").split_whitespace().collect();
    anyhow::ensure!(first.len()==3&&first[0]=="GET"&&matches!(first[2],"HTTP/1.1"|"HTTP/1.0")&&first[1].starts_with("/callback?"),"invalid callback request");
    let hosts:Vec<_>=lines.filter_map(|line|line.split_once(':').filter(|(name,_)|name.eq_ignore_ascii_case("host")).map(|(_,v)|v.trim())).collect();
    anyhow::ensure!(hosts==[format!("127.0.0.1:{port}")],"invalid callback host");
    let url=Url::parse(&format!("http://127.0.0.1:{port}{}",first[1]))?;
    let pairs:Vec<_>=url.query_pairs().collect();let query:HashMap<_,_>=pairs.iter().cloned().collect();
    anyhow::ensure!(url.path()=="/callback"&&url.fragment().is_none()&&pairs.len()==query.len()&&query.get("state").is_some_and(|s|s==state),"invalid callback state");
    anyhow::ensure!(query.contains_key("code")!=query.contains_key("error"),"invalid callback result");
    if query.contains_key("error"){return Ok(Callback::Denied);}
    let code=query.get("code").filter(|v|!v.is_empty()&&v.len()<=4096&&!v.chars().any(|c|c.is_control()||c.is_whitespace())).context("invalid authorization code")?;
    Ok(Callback::Code(code.to_string()))
}
fn callback_response(stream:&mut TcpStream,valid:bool){
    let message=if valid{"授权结果已收到。请回到 Velella 查看连接结果，可以关闭此页面。"}else{"此授权请求无效。请回到 Velella 重新打开授权。"};
    let body=format!("<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\"><title>Velella · Linear</title><h1>Velella · Linear</h1><p>{message}</p></html>");
    let status=if valid{"200 OK"}else{"400 Bad Request"};
    let _=write!(stream,"HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n{body}",body.len());
}
fn bind_callback(attempt:&Attempt,address:&str)->Result<Option<u16>>{
    let view=attempt.view.lock().unwrap();if !matches!(view.state.as_str(),"starting"|"waiting"){return Ok(None);}
    let listener=TcpListener::bind(address)?;listener.set_nonblocking(true)?;let port=listener.local_addr()?.port();
    *attempt.listener.lock().unwrap()=Some(listener);Ok(Some(port))
}
fn wait_callback(attempt:&Attempt,state:&str,port:u16,timeout:Duration)->Result<Option<Callback>>{
    let started=Instant::now();
    while attempt.active()&&started.elapsed()<timeout {
        let accepted={let listener=attempt.listener.lock().unwrap();match listener.as_ref(){Some(listener)=>listener.accept(),None=>return Ok(None)}};
        match accepted{
            Ok((mut stream,peer))=>{
                if !peer.ip().is_loopback(){continue;}
                stream.set_read_timeout(Some(Duration::from_millis(200)))?;stream.set_write_timeout(Some(Duration::from_millis(200)))?;
                let mut bytes=Vec::new();let until=Instant::now()+Duration::from_secs(2);
                while attempt.active()&&Instant::now()<until&&bytes.len()<8192&&!bytes.ends_with(b"\r\n\r\n"){
                    let mut chunk=[0;512];match stream.read(&mut chunk){Ok(0)=>break,Ok(n)=>bytes.extend_from_slice(&chunk[..n]),Err(e) if matches!(e.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut)=>continue,Err(_)=>break}
                }
                if !attempt.active(){return Ok(None);}
                let result=if bytes.ends_with(b"\r\n\r\n")&&bytes.len()<=8192 {std::str::from_utf8(&bytes).ok().and_then(|text|callback(text,state,port).ok())}else{None};
                callback_response(&mut stream,result.is_some());
                if result.is_some(){attempt.listener.lock().unwrap().take();return Ok(result);}
            },
            Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(50)),
            Err(e)=>return Err(e.into()),
        }
    }
    attempt.finish("expired","等待授权已超时，请重新连接。");Ok(None)
}
fn run_browser_login(root:&Path,attempt:&Attempt,config:&Config,token_url:&str,api_url:&str)->Result<()>{
    if !attempt.active(){return Ok(());}
    let port=match bind_callback(attempt,"127.0.0.1:17887"){Ok(Some(v))=>v,Ok(None)=>return Ok(()),Err(_)=>{attempt.finish("failed","本机授权端口 17887 被占用，请关闭另一个授权窗口后重试。");return Ok(());}};
    browser_login(root,attempt,config,token_url,api_url,port,LOCAL_CALLBACK)
}
fn browser_login(root:&Path,attempt:&Attempt,config:&Config,token_url:&str,api_url:&str,port:u16,redirect_uri:&str)->Result<()>{
    let baseline=connections(&Store::open(root)?)?;
    let verifier=secret();let state=secret();let mut url=Url::parse("https://linear.app/oauth/authorize")?;
    url.query_pairs_mut().extend_pairs([("client_id",config.client_id.as_str()),("redirect_uri",redirect_uri),("response_type","code"),("scope","read,write"),("state",&state),("code_challenge",&challenge(&verifier)),("code_challenge_method","S256"),("prompt","consent")]);
    {let mut view=attempt.view.lock().unwrap();if view.state!="starting"{return Ok(());}view.state="waiting".into();view.authorization_url=Some(url.into());}
    match wait_callback(attempt,&state,port,LIMIT)? {
        Some(Callback::Code(code)) if attempt.active()=>{
            let tokens=exchange(&client()?,token_url,&[("grant_type","authorization_code"),("client_id",&config.client_id),("code",&code),("redirect_uri",redirect_uri),("code_verifier",&verifier)],&config.client_id)?;
            complete(root,attempt,tokens,api_url,&baseline)?;
        },
        Some(Callback::Denied)=>attempt.finish("denied","你已取消 Linear 授权，可重新连接。"),
        _=>{},
    }
    Ok(())
}

struct Session { id:String, poll_token:String, verification_url:String, redirect_uri:String }
fn session(config:&Config,data:&Value,code_challenge:&str)->Result<Session> {
    let id=field(data,"id")?.to_owned();let poll_token=field(data,"poll_token")?.to_owned();
    for value in [&id,&poll_token]{anyhow::ensure!(value.len()==43&&value.bytes().all(|c|c.is_ascii_alphanumeric()||matches!(c,b'-'|b'_')),"授权会话无效");}
    let redirect_uri=format!("{}/callback",config.relay_url);
    anyhow::ensure!(data["client_id"]==config.client_id&&data["redirect_uri"]==redirect_uri,"授权服务与应用配置不匹配");
    let authorization_url=field(data,"authorization_url")?.to_owned();let url=Url::parse(&authorization_url)?;
    let pairs:Vec<_>=url.query_pairs().collect();let query:HashMap<_,_>=pairs.iter().cloned().collect();
    anyhow::ensure!(pairs.len()==query.len()&&url.scheme()=="https"&&url.host_str()==Some("linear.app")&&url.port().is_none()&&url.path()=="/oauth/authorize"&&url.username().is_empty()&&url.password().is_none()&&url.fragment().is_none(),"授权地址无效");
    for (key,expected) in [("client_id",config.client_id.as_str()),("redirect_uri",redirect_uri.as_str()),("code_challenge",code_challenge),("code_challenge_method","S256"),("response_type","code"),("scope","read,write"),("prompt","consent")]{anyhow::ensure!(query.get(key).map(|v|v.as_ref())==Some(expected),"授权参数不匹配");}
    anyhow::ensure!(query.get("state").is_some_and(|v|v.len()==43)&&query.len()==8,"授权状态无效");
    let verification_url=format!("{}/authorize/{id}",config.relay_url);
    Ok(Session{id,poll_token,verification_url,redirect_uri})
}
fn run_login(root:&Path,attempt:&Attempt,config:&Config,token_url:&str,api_url:&str)->Result<()> {
    if !attempt.active(){return Ok(());}
    let baseline=connections(&Store::open(root)?)?;
    let client=client()?;let verifier=secret();let code_challenge=challenge(&verifier);
    let data=response(client.post(format!("{}/sessions",config.relay_url)).json(&json!({"code_challenge":code_challenge})).send().context("无法连接授权服务")?)?;
    let session=session(config,&data,&code_challenge)?;
    let endpoint=format!("{}/sessions/{}",config.relay_url,session.id);
    let result=(||->Result<()> {
        if !attempt.active(){return Ok(());}
        let qr=qr_svg(&session.verification_url)?;
        {let mut view=attempt.view.lock().unwrap();if view.state!="starting"{return Ok(());}view.state="waiting".into();view.authorization_url=Some(session.verification_url.clone());view.qr_svg=Some(qr);}
        while attempt.active()&&attempt.created.elapsed()<LIMIT {
            let data=response(client.get(&endpoint).bearer_auth(&session.poll_token).send().context("无法查询授权结果")?)?;
            match data["status"].as_str() {
                Some("waiting")=>std::thread::sleep(Duration::from_millis(1200)),
                Some("denied")=>{attempt.finish("denied","你已取消 Linear 授权，可重新连接。");return Ok(());},
                Some("authorized")=>{
                    if !attempt.active(){return Ok(());}
                    let token=exchange(&client,token_url,&[("grant_type","authorization_code"),("client_id",&config.client_id),("code",field(&data,"code")?),("redirect_uri",&session.redirect_uri),("code_verifier",&verifier)],&config.client_id)?;
                    complete(root,attempt,token,api_url,&baseline)?;return Ok(());
                },
                _=>anyhow::bail!("授权会话无效"),
            }
        }
        attempt.finish("expired","二维码已过期，请重新生成。");Ok(())
    })();
    // Best effort cleanup. Relay TTL also removes abandoned or disconnected sessions.
    let _=client.delete(endpoint).bearer_auth(&session.poll_token).send();
    result
}

#[derive(Serialize,Deserialize)]
struct Tokens { access_token:String, refresh_token:String, expires_at:u64, client_id:String }
fn exchange(client:&Client,url:&str,form:&[(&str,&str)],client_id:&str)->Result<Tokens> {
    let data=response(client.post(url).form(form).send().context("Linear 授权交换失败")?)?;
    anyhow::ensure!(data["token_type"].as_str().is_some_and(|s|s.eq_ignore_ascii_case("bearer")),"Linear 授权类型无效");
    let expires=data["expires_in"].as_u64().filter(|v|*v>60&&*v<=31_536_000).context("Linear 授权有效期无效")?;
    Ok(Tokens{access_token:field(&data,"access_token")?.into(),refresh_token:field(&data,"refresh_token")?.into(),expires_at:epoch()+expires,client_id:client_id.into()})
}
type Connections=HashMap<String,(String,String)>;
fn connections(store:&Store)->Result<Connections>{
    let mut query=store.conn.prepare("SELECT id,document,secret FROM sources")?;
    let result=query.query_map([],|r|Ok((r.get::<_,String>(0)?,(r.get::<_,String>(1)?,r.get::<_,String>(2)?))))?.collect::<std::result::Result<Connections,_>>()?;Ok(result)
}
fn complete(root:&Path,attempt:&Attempt,tokens:Tokens,api_url:&str,baseline:&Connections)->Result<()> {
    if !attempt.active(){return Ok(());}
    let mut source=normalize(serde_json::from_value(json!({"provider":"linear","name":"Linear","base_url":api_url,"scope":"","auth_mode":"oauth"}))?)?;
    let account=provider::Api::new(&source,&tokens.access_token)?.account()?;
    source.login=field(&account,"id")?.into();source.name=account["name"].as_str().unwrap_or("Linear").into();source=normalize(source)?;
    let store=Store::open(root)?;
    if let Some(old)=store.all_sources()?.into_iter().find(|s|s.provider=="linear"&&s.base_url==source.base_url&&s.scope.is_empty()&&s.login==source.login){source.id=old.id;source.project_id=old.project_id;}
    let page=provider::Api::new(&source,&tokens.access_token)?.list("","open","")?;
    // Serialize cancellation with the final save: after cancel returns no late save can occur.
    let mut view=attempt.view.lock().unwrap();if !matches!(view.state.as_str(),"starting"|"waiting"){return Ok(());}
    let transaction=store.conn.unchecked_transaction()?;
    anyhow::ensure!(connections(&store)?.get(&source.id)==baseline.get(&source.id),"授权期间连接已被修改或断开，请重新连接");
    store.save(&source,&serde_json::to_string(&tokens)?)?;
    for item in &page.items {super::validate_item(item)?;store.conn.execute("INSERT INTO items(key,source_id,document) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET document=excluded.document",rusqlite::params![item.key,item.source_id,serde_json::to_string(item)?])?;}
    transaction.commit()?;
    view.state="complete".into();view.source=Some(source);view.authorization_url=None;view.qr_svg=None;view.message=Some("Linear 已连接。".into());Ok(())
}

pub(super) fn access_token(store:&Store,id:&str)->Result<String> {refresh(store,id,TOKEN_URL)}
fn refresh(store:&Store,id:&str,token_url:&str)->Result<String> {
    static REFRESH:Mutex<()>=Mutex::new(());let _guard=REFRESH.lock().unwrap();
    // Reread after acquiring the lock so simultaneous requests cannot rotate twice.
    let (document,encrypted):(String,String)=store.conn.query_row("SELECT document,secret FROM sources WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let source:Source=serde_json::from_str(&document)?;
    anyhow::ensure!(!source.disabled&&source.provider=="linear"&&source.auth_mode=="oauth","Linear 连接已改变，请刷新后重试");
    let stored=credentials::unprotect(&encrypted)?;
    let mut token:Tokens=serde_json::from_str(&stored).map_err(|_|anyhow::anyhow!("Linear 授权凭据无效，请重新连接"))?;
    if token.expires_at<=epoch()+120 {
        token=exchange(&client()?,token_url,&[("grant_type","refresh_token"),("client_id",&token.client_id),("refresh_token",&token.refresh_token)],&token.client_id)?;
        let protected=credentials::protect(&serde_json::to_string(&token)?)?;
        let changed=store.conn.execute("UPDATE sources SET secret=?1 WHERE id=?2 AND secret=?3 AND document=?4",rusqlite::params![protected,id,encrypted,document])?;
        anyhow::ensure!(changed==1,"Linear 连接已改变，请刷新后重试");
    }
    Ok(token.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{BufRead,BufReader,Write},net::TcpListener,sync::mpsc};
    struct Temp(PathBuf);
    impl Temp {fn new()->Self {let p=std::env::temp_dir().join(format!("velella-linear-oauth-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&p).unwrap();Self(p)}}
    impl Drop for Temp {fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
    fn fixture(count:usize,handler:impl Fn(usize,&str,&str)->Value+Send+'static)->(String,mpsc::Receiver<String>) {
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();let base=format!("http://{}",listener.local_addr().unwrap());let address=base.clone();let (tx,rx)=mpsc::channel();
        std::thread::spawn(move||for i in 0..count {
            let (mut socket,_)=listener.accept().unwrap();socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut reader=BufReader::new(socket.try_clone().unwrap());let mut request=String::new();let mut length=0;
            loop {let mut line=String::new();reader.read_line(&mut line).unwrap();if line=="\r\n"{break;}if line.to_lowercase().starts_with("content-length:"){length=line.split(':').nth(1).unwrap().trim().parse::<usize>().unwrap();}request.push_str(&line);}
            let mut body=vec![0;length];reader.read_exact(&mut body).unwrap();request.push_str("\r\n");request.push_str(&String::from_utf8(body).unwrap());
            let response=handler(i,&request,&address).to_string();let _=tx.send(request);
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n{}",response.len(),response).unwrap();
        });(base,rx)
    }
    fn relay(config:&Config,challenge:&str)->Value {
        let redirect=format!("{}/callback",config.relay_url);let mut url=Url::parse("https://linear.app/oauth/authorize").unwrap();
        url.query_pairs_mut().extend_pairs([("client_id",config.client_id.as_str()),("redirect_uri",&redirect),("code_challenge",challenge),("code_challenge_method","S256"),("state",&"s".repeat(43)),("response_type","code"),("scope","read,write"),("prompt","consent")]);
        json!({"id":"i".repeat(43),"poll_token":"p".repeat(43),"authorization_url":url.to_string(),"client_id":config.client_id,"redirect_uri":redirect})
    }
    fn tokens()->Value {json!({"token_type":"Bearer","access_token":"access-fixture","refresh_token":"refresh-fixture","expires_in":86400})}
    fn organization()->Value {json!({"data":{"organization":{"id":"workspace-fixture","name":"Test workspace"}}})}
    fn issues()->Value {json!({"data":{"issues":{"nodes":[],"pageInfo":{"hasNextPage":false}}}})}
    #[test] fn local_callback_validates_state_host_method_and_result(){
        let request=|query:&str|format!("GET /callback?{query} HTTP/1.1\r\nHost: 127.0.0.1:17887\r\n\r\n");
        assert!(matches!(callback(&request("state=s&code=c"),"s",17887).unwrap(),Callback::Code(c) if c=="c"));
        assert!(matches!(callback(&request("state=s&error=access_denied"),"s",17887).unwrap(),Callback::Denied));
        for query in ["code=c","state=wrong&code=c","state=s&state=s&code=c","state=s&code=","state=s&code=c&error=e","state=s&code=c&code=d"]{assert!(callback(&request(query),"s",17887).is_err());}
        for bad in [request("state=s&code=c").replace("GET ","POST "),request("state=s&code=c").replace("/callback?","/other?"),request("state=s&code=c").replace("127.0.0.1","evil.test"),request("state=s&code=c").replace("\r\n\r\n","\r\nHost: 127.0.0.1:17887\r\n\r\n")]{assert!(callback(&bad,"s",17887).is_err());}
    }
    #[test] fn local_listener_cancellation_and_timeout_release_the_port(){
        for cancel in [false,true]{
            let a=Arc::new(Attempt::new("local"));let port=bind_callback(&a,"127.0.0.1:0").unwrap().unwrap();let address=format!("127.0.0.1:{port}");let worker=a.clone();
            let handle=std::thread::spawn(move||wait_callback(&worker,"state",port,Duration::from_millis(80)).unwrap());
            if cancel{a.finish("cancelled","cancelled");assert!(TcpListener::bind(&address).is_ok());}assert!(handle.join().unwrap().is_none());
            assert_eq!(a.snapshot()["attempt"]["state"],if cancel{"cancelled"}else{"expired"});assert!(TcpListener::bind(address).is_ok());
        }
    }
    #[test] fn browser_flow_uses_pkce_and_persists_only_after_matching_callback(){
        let root=Temp::new();let path=root.0.clone();let a=Arc::new(Attempt::new("local"));let port=bind_callback(&a,"127.0.0.1:0").unwrap().unwrap();let redirect=format!("http://127.0.0.1:{port}/callback");let callback_url=redirect.clone();
        let (base,requests)=fixture(3,|i,_,_|match i{0=>tokens(),1=>organization(),_=>issues()});
        let worker=a.clone();
        let handle=std::thread::spawn(move||browser_login(&path,&worker,&Config{relay_url:String::new(),client_id:"client-fixture".into()},&format!("{base}/oauth/token"),&base,port,&redirect).unwrap());
        let deadline=Instant::now()+Duration::from_secs(5);
        let url=loop{if let Some(url)=a.snapshot()["attempt"]["authorization_url"].as_str(){break Url::parse(url).unwrap();}assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(10));};
        assert_eq!(url.host_str(),Some("linear.app"));let query:HashMap<_,_>=url.query_pairs().map(|(k,v)|(k.into_owned(),v.into_owned())).collect();assert_eq!(query["redirect_uri"],callback_url);
        let bad=client().unwrap().get(format!("{callback_url}?state=wrong&code=bad")).send().unwrap();assert_eq!(bad.status(),400);assert!(a.active());
        let good=client().unwrap().get(&callback_url).query(&[("state",query["state"].as_str()),("code","code-fixture")]).send().unwrap();assert_eq!(good.status(),200);assert!(!good.text().unwrap().contains("code-fixture"));
        handle.join().unwrap();let view=a.snapshot();assert_eq!(view["attempt"]["state"],"complete");assert!(view["attempt"]["qr_svg"].is_null());
        let exchange=requests.recv_timeout(Duration::from_secs(2)).unwrap();let form=url_form(exchange.split("\r\n\r\n").nth(1).unwrap());assert_eq!(challenge(&form["code_verifier"]),query["code_challenge"]);assert_eq!(form["redirect_uri"],callback_url);
        let store=Store::open(&root.0).unwrap();let sources=store.sources().unwrap();assert_eq!(sources.len(),1);assert_eq!(store.source(&sources[0].id).unwrap().1,"access-fixture");
        for secret in ["access-fixture","refresh-fixture","code-fixture","code_verifier"]{assert!(!view.to_string().contains(secret));}
    }
    #[test] fn pkce_matches_rfc7636_and_random_verifiers_differ(){
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),"E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert_ne!(secret(),secret());assert_eq!(secret().len(),64);
    }
    #[test] fn generates_linear_qr_fixture(){
        let config=Config{relay_url:"https://auth.example.test".into(),client_id:"client-fixture".into()};let value=relay(&config,&challenge("browser-fixture"));
        let s=session(&config,&value,&challenge("browser-fixture")).unwrap();let url=&s.verification_url;let svg=qr_svg(url).unwrap();assert!(svg.contains("<svg"));
        if let Some(path)=std::env::var_os("VELELLA_LINEAR_QR_TEST_OUTPUT"){std::fs::write(path,serde_json::to_string(&json!({"authorization_url":url,"qr_svg":svg})).unwrap()).unwrap();}
    }
    #[test] fn binds_authorization_to_official_origin_application_callback_and_challenge(){
        let config=Config{relay_url:"https://auth.example.test".into(),client_id:"client-fixture".into()};let original=relay(&config,"c");
        assert!(session(&config,&original,"c").is_ok());assert!(session(&config,&original,"wrong").is_err());
        for (key,value) in [("client_id","foreign-client"),("redirect_uri","https://evil.test/callback"),("id","../elsewhere"),("poll_token","short"),("authorization_url","https://evil.test/oauth/authorize")]{let mut bad=original.clone();bad[key]=json!(value);assert!(session(&config,&bad,"c").is_err());}
        let mut bad=original.clone();bad["authorization_url"]=json!(format!("{}&client_id=extra",original["authorization_url"].as_str().unwrap()));assert!(session(&config,&bad,"c").is_err());
    }
    #[test] fn cancel_before_start_and_duplicate_ids_cannot_spawn_or_overwrite_terminal_state(){
        let mut registry=Registry::default();let (a,start)=registry.reserve("a",true).unwrap();assert!(!start);assert!(!registry.reserve("a",false).unwrap().1);
        a.finish("complete","late");assert_eq!(a.snapshot()["attempt"]["state"],"cancelled");
        let (b,start)=registry.reserve("b",false).unwrap();assert!(start);assert!(!registry.reserve("b",false).unwrap().1);assert!(registry.reserve("c",false).is_err());b.finish("expired","timeout");assert!(registry.reserve("c",false).unwrap().1);
    }
    #[test] fn missing_configuration_has_no_fake_qr_and_request_stays_idempotent(){
        let root=Temp::new();assert_eq!(dispatch(&root.0,&json!({"cmd":"sources.linear.account"})).unwrap()["configured"],false);
        let req=json!({"cmd":"sources.linear.auth.start","request_id":uuid::Uuid::new_v4().to_string()});let first=dispatch(&root.0,&req).unwrap();assert_eq!(first["attempt"]["state"],"failed");assert!(first["attempt"]["qr_svg"].is_null());assert_eq!(dispatch(&root.0,&req).unwrap(),first);
    }
    #[test] fn full_authorization_exchange_saves_encrypted_credentials_without_renderer_secrets(){
        let root=Temp::new();
        let (base,requests)=fixture(6,|i,request,base|match i {
            0=>{let body:Value=serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();relay(&Config{relay_url:base.into(),client_id:"client-fixture".into()},body["code_challenge"].as_str().unwrap())},
            1=>json!({"status":"authorized","code":"code-fixture"}),2=>tokens(),3=>organization(),4=>issues(),_=>json!({"status":"cancelled"}),
        });
        let attempt=Attempt::new("request-fixture");let config=Config{relay_url:base.clone(),client_id:"client-fixture".into()};
        run_login(&root.0,&attempt,&config,&format!("{base}/oauth/token"),&base).unwrap();
        let view=attempt.snapshot();assert_eq!(view["attempt"]["state"],"complete");
        for secret in ["access-fixture","refresh-fixture","code-fixture","poll_token","code_verifier"]{assert!(!view.to_string().contains(secret));}
        let rows:Vec<_>=(0..6).map(|_|requests.recv_timeout(Duration::from_secs(2)).unwrap()).collect();
        assert!(rows[1].to_lowercase().contains("authorization: bearer ppp"));
        let form:HashMap<_,_>=url_form(rows[2].split("\r\n\r\n").nth(1).unwrap());let init:Value=serde_json::from_str(rows[0].split("\r\n\r\n").nth(1).unwrap()).unwrap();assert_eq!(challenge(form.get("code_verifier").unwrap()),init["code_challenge"]);
        assert!(rows[3].to_lowercase().contains("authorization: bearer access-fixture"));assert!(rows[5].starts_with("DELETE /sessions/"));
        let store=Store::open(&root.0).unwrap();let saved=store.sources().unwrap();assert_eq!(saved.len(),1);assert_eq!(saved[0].auth_mode,"oauth");assert_eq!(store.source(&saved[0].id).unwrap().1,"access-fixture");
        let encrypted:String=store.conn.query_row("SELECT secret FROM sources",[],|r|r.get(0)).unwrap();assert!(!encrypted.contains("access-fixture"));assert!(!encrypted.contains("refresh-fixture"));
        assert!(view["attempt"]["authorization_url"].is_null());assert!(view["attempt"]["qr_svg"].is_null());
    }
    fn url_form(value:&str)->HashMap<String,String>{Url::parse(&format!("http://test/?{value}")).unwrap().query_pairs().map(|(k,v)|(k.into_owned(),v.into_owned())).collect()}
    #[test] fn refresh_rotates_once_and_disconnect_works_with_expired_credentials(){
        let root=Temp::new();let store=Store::open(&root.0).unwrap();
        let s=normalize(serde_json::from_value(json!({"provider":"linear","name":"workspace","base_url":API_URL,"scope":"","login":"org","auth_mode":"oauth"})).unwrap()).unwrap();
        let token=Tokens{access_token:"expired".into(),refresh_token:"old-refresh".into(),expires_at:0,client_id:"client-fixture".into()};store.save(&s,&serde_json::to_string(&token).unwrap()).unwrap();
        let (base,requests)=fixture(1,|_,_,_|tokens());assert_eq!(refresh(&store,&s.id,&base).unwrap(),"access-fixture");assert_eq!(refresh(&store,&s.id,&base).unwrap(),"access-fixture");
        let request=requests.recv_timeout(Duration::from_secs(2)).unwrap();assert!(request.contains("grant_type=refresh_token"));assert!(request.contains("refresh_token=old-refresh"));
        store.save(&s,&serde_json::to_string(&token).unwrap()).unwrap();dispatch(&root.0,&json!({"cmd":"sources.linear.account"})).unwrap();
        super::super::dispatch(&root.0,&json!({"cmd":"sources.remove","source_id":s.id})).unwrap();assert!(store.sources().unwrap().is_empty());assert!(store.source(&s.id).is_err());
    }
    #[test] fn cancelled_completion_never_saves_and_oauth_cannot_be_forged_via_manual_save(){
        let root=Temp::new();let a=Attempt::new("cancelled");a.finish("cancelled","cancelled");
        complete(&root.0,&a,Tokens{access_token:"x".into(),refresh_token:"y".into(),expires_at:epoch()+500,client_id:"c".into()},API_URL,&Connections::new()).unwrap();assert!(Store::open(&root.0).unwrap().sources().unwrap().is_empty());
        assert!(super::super::dispatch(&root.0,&json!({"cmd":"sources.save","source":{"provider":"linear","name":"fake","base_url":API_URL,"scope":"","auth_mode":"oauth"},"token":"fake"})).is_err());
    }
    #[test] fn disconnect_during_authorization_cannot_be_undone_by_late_completion(){
        let root=Temp::new();let store=Store::open(&root.0).unwrap();
        let (base,_)=fixture(2,|i,_,_|if i==0{organization()}else{issues()});
        let mut s=normalize(serde_json::from_value(json!({"provider":"linear","name":"Test workspace","base_url":base,"scope":"","login":"workspace-fixture","auth_mode":"token"})).unwrap()).unwrap();store.save(&s,"manual-key").unwrap();
        let baseline=connections(&store).unwrap();s.disabled=true;store.save(&s,"").unwrap();
        let a=Attempt::new("late");let token=Tokens{access_token:"a".into(),refresh_token:"r".into(),expires_at:epoch()+1000,client_id:"c".into()};
        assert!(complete(&root.0,&a,token,&base,&baseline).is_err());assert!(store.sources().unwrap().is_empty());
    }
}
