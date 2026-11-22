//! GitHub CLI owns OAuth and credential storage. Only device UI data crosses RPC.
use super::cli;
use anyhow::{Context, Result};
use qrcode::{render::svg, QrCode};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::HashMap, io::{BufRead, BufReader, Read}, path::{Path, PathBuf}, process::Stdio,
    sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc, Mutex, OnceLock}, time::{Duration, Instant}};

const VERIFY_URL: &str = "https://github.com/login/device";
const WAIT_LIMIT: Duration = Duration::from_secs(16 * 60);
const OUTPUT_LIMIT: u64 = 64 * 1024;

#[derive(Clone, Serialize)]
struct View {
    request_id: String,
    state: String,
    user_code: Option<String>,
    verification_uri: Option<String>,
    qr_svg: Option<String>,
    login: Option<String>,
    message: Option<String>,
}
struct Attempt { view: Mutex<View>, cancel: AtomicBool, created: Instant }
impl Attempt {
    fn new(id: &str) -> Self {
        Self { view: Mutex::new(View { request_id:id.into(), state:"starting".into(), user_code:None,
            verification_uri:None, qr_svg:None, login:None, message:None }), cancel:AtomicBool::new(false), created:Instant::now() }
    }
    fn snapshot(&self) -> Value { json!({"ok":true,"attempt":self.view.lock().unwrap().clone()}) }
    fn active(&self) -> bool { matches!(self.view.lock().unwrap().state.as_str(),"starting"|"waiting") }
    fn finish(&self, state: &str, message: Option<&str>, login: Option<String>) {
        let mut view=self.view.lock().unwrap();
        if view.state=="cancelled" {return;}
        view.state=state.into();view.message=message.map(str::to_owned);view.login=login;
        view.user_code=None;view.verification_uri=None;view.qr_svg=None;
    }
    fn cancel(&self) {
        self.cancel.store(true,Ordering::SeqCst);
        let mut view=self.view.lock().unwrap();
        if matches!(view.state.as_str(),"starting"|"waiting") {
            view.state="cancelled".into();view.user_code=None;view.verification_uri=None;view.qr_svg=None;
            view.message=Some("已停止等待授权。若已在 GitHub 批准，可重新检查登录。".into());
        }
    }
}

#[derive(Default)]
struct Registry { attempts: HashMap<String, Arc<Attempt>> }
impl Registry {
    fn reserve(&mut self,id:&str,cancel:bool)->Result<(Arc<Attempt>,bool)> {
        if let Some(attempt)=self.attempts.get(id) {
            if cancel {attempt.cancel();}
            return Ok((attempt.clone(),false));
        }
        self.attempts.retain(|_,a|a.active()||a.created.elapsed()<Duration::from_secs(3600));
        anyhow::ensure!(self.attempts.len()<32,"连接尝试过多，请稍后再试");
        if !cancel {anyhow::ensure!(!self.attempts.values().any(|a|a.active()),"另一个窗口正在等待 GitHub 授权，请先完成或取消该连接");}
        let attempt=Arc::new(Attempt::new(id));
        if cancel {attempt.cancel();}
        self.attempts.insert(id.into(),attempt.clone());
        Ok((attempt,!cancel))
    }
}
fn registries()->&'static Mutex<HashMap<PathBuf,Registry>> {
    static REGISTRIES:OnceLock<Mutex<HashMap<PathBuf,Registry>>>=OnceLock::new();
    REGISTRIES.get_or_init(||Mutex::new(HashMap::new()))
}

pub(super) fn dispatch(root:&Path,req:&Value)->Result<Value> {
    match req["cmd"].as_str().unwrap_or("") {
        "sources.github.account"=>account(),
        "sources.github.repositories"=>repositories(req),
        "sources.github.auth.start"|"sources.github.auth.cancel"=>{
            let id=uuid::Uuid::parse_str(req["request_id"].as_str().context("连接请求标识缺失")?)?.to_string();
            let cancel=req["cmd"]=="sources.github.auth.cancel";
            let (attempt,start)=registries().lock().unwrap().entry(root.to_path_buf()).or_default().reserve(&id,cancel)?;
            if start {
                let running=attempt.clone();
                if std::thread::Builder::new().name("github-device-login".into()).spawn(move||run_login(running)).is_err() {
                    attempt.finish("failed",Some("无法开始授权，请重试。"),None);
                }
            }
            Ok(attempt.snapshot())
        }
        "sources.github.auth.status"=>{
            let id=uuid::Uuid::parse_str(req["request_id"].as_str().context("连接请求标识缺失")?)?.to_string();
            let registry=registries().lock().unwrap();
            let attempt=registry.get(root).and_then(|r|r.attempts.get(&id)).context("授权等待已结束，请重新检查登录或生成二维码")?;
            Ok(attempt.snapshot())
        }
        _=>anyhow::bail!("不支持的 GitHub 连接操作"),
    }
}

fn account()->Result<Value> {
    if cli::executable("gh").is_none() {return Ok(json!({"ok":true,"available":false,"authenticated":false}));}
    let args=["auth","status","--active","--hostname","github.com","--json","hosts","--jq","[.hosts.\"github.com\"[]? | {login,state}]"];
    let rows:Value=serde_json::from_slice(&cli::run("gh",&args.map(str::to_owned),None,None)?)?;
    let login=rows.as_array().and_then(|rows|rows.iter().find(|v|v["state"]=="success")).and_then(|v|v["login"].as_str()).filter(|s|valid_login(s));
    Ok(json!({"ok":true,"available":true,"authenticated":login.is_some(),"login":login}))
}
fn valid_login(value:&str)->bool {
    !value.is_empty()&&value.len()<=100&&value.chars().all(|c|c.is_ascii_alphanumeric()||matches!(c,'-'|'_'|'['|']'))
}
fn repositories(req:&Value)->Result<Value> {
    let page=req["page"].as_u64().unwrap_or(1);anyhow::ensure!((1..=1000).contains(&page),"无效的仓库页码");
    let args=vec!["api".into(),"--hostname".into(),"github.com".into(),format!("/user/repos?per_page=50&sort=pushed&direction=desc&page={page}"),"--jq".into(),"[.[] | {full_name,private,description,archived}]".into()];
    let rows:Vec<Value>=serde_json::from_slice(&cli::run("gh",&args,None,None)?)?;
    let mut choices=Vec::new();
    for row in &rows {
        let name=row["full_name"].as_str().context("仓库名称缺失")?;
        anyhow::ensure!(name.len()<=256&&name.split('/').count()==2&&name.split('/').all(|s|!s.is_empty()&&s!="."&&s!=".."&&s.chars().all(|c|c.is_ascii_alphanumeric()||matches!(c,'-'|'_'|'.'))),"仓库名称无效");
        choices.push(json!({"full_name":name,"private":row["private"]==true,"archived":row["archived"]==true,"description":row["description"].as_str().unwrap_or("").chars().take(400).collect::<String>()}));
    }
    Ok(json!({"ok":true,"repositories":choices,"next_page":if rows.len()==50&&page<1000{Some(page+1)}else{None}}))
}

enum Display { Code(String), Url }
fn display_line(line:&str)->Result<Option<Display>> {
    if let Some((_,value))=line.split_once("First copy your one-time code: ") {
        let code=value.trim();
        anyhow::ensure!(code.len()==9&&code.as_bytes()[4]==b'-'&&code.bytes().enumerate().all(|(i,c)|i==4||c.is_ascii_uppercase()||c.is_ascii_digit()),"不支持的设备码格式");
        return Ok(Some(Display::Code(code.into())));
    }
    if let Some((_,value))=line.split_once("Open this URL to continue in your web browser: ") {
        // WebAppFlow/enterprise URLs cannot be used as a cross-device QR login.
        anyhow::ensure!(value.trim()==VERIFY_URL,"此授权需要本机浏览器，请使用手动连接");
        return Ok(Some(Display::Url));
    }
    Ok(None)
}
fn qr_svg()->Result<String> {
    Ok(QrCode::new(VERIFY_URL)?.render::<svg::Color>().min_dimensions(224,224).quiet_zone(true)
        .dark_color(svg::Color("#000000")).light_color(svg::Color("#ffffff")).build())
}

fn run_login(attempt:Arc<Attempt>) {
    if attempt.cancel.load(Ordering::SeqCst) {return;}
    // A login may have completed in another window since the dialog was opened.
    if let Ok(account)=account() {
        if account["authenticated"]==true {
            attempt.finish("complete",None,account["login"].as_str().map(str::to_owned));return;
        }
    }
    if attempt.cancel.load(Ordering::SeqCst) {return;}
    let Some(binary)=cli::executable("gh") else {
        attempt.finish("failed",Some("尚未安装 GitHub CLI，请先安装，或使用手动连接。"),None);return;
    };
    let result=(||->Result<()> {
        let mut command=crate::process::command(binary.to_str().context("CLI path unavailable")?);
        command.args(["auth","login","--web","--hostname","github.com","--git-protocol","https","--skip-ssh-key"])
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped())
            .env("GH_PROMPT_DISABLED","1").env("NO_COLOR","1").env("CLICOLOR","0").env_remove("GH_FORCE_TTY");
        let child=command.spawn().context("无法启动 GitHub CLI")?;
        let Some(status)=wait_for_login(&attempt,child,WAIT_LIMIT)? else {return Ok(());};
        anyhow::ensure!(status.success(),"GitHub 授权未完成");
        let verified=account()?;
        anyhow::ensure!(verified["authenticated"]==true,"授权结果尚未确认");
        attempt.finish("complete",None,verified["login"].as_str().map(str::to_owned));
        Ok(())
    })();
    if result.is_err() {
        // Never expose CLI stderr: it can contain credentials and arbitrary server text.
        attempt.finish("failed",Some("授权未完成、已过期或网络暂不可用。可重新检查登录，或重新生成二维码。"),None);
    }
}

fn wait_for_login(attempt:&Attempt,mut child:std::process::Child,timeout:Duration)->Result<Option<std::process::ExitStatus>> {
        #[cfg(windows)]
        let _group=match crate::process::ChildGroup::attach(child.id()) {Ok(group)=>group,Err(error)=>{let _=child.kill();let _=child.wait();return Err(error);}};
        let stderr=child.stderr.take().context("GitHub CLI output unavailable")?;
        let (tx,rx)=mpsc::channel();
        let reader=std::thread::spawn(move||{
            let mut total=0;let mut reader=BufReader::new(stderr.take(OUTPUT_LIMIT+1));
            loop {
                let mut line=String::new();
                match reader.read_line(&mut line) {
                    Ok(0)=>break,
                    Ok(n)=>{total+=n;if total>OUTPUT_LIMIT as usize{let _=tx.send(Err(anyhow::anyhow!("CLI output limit")));break;}if tx.send(display_line(&line)).is_err(){break;}},
                    Err(_)=>{let _=tx.send(Err(anyhow::anyhow!("CLI output unreadable")));break;}
                }
            }
        });
        let started=Instant::now();let mut code=None;let mut url_ready=false;
        let outcome=(||->Result<Option<std::process::ExitStatus>> {
            loop {
                if attempt.cancel.load(Ordering::SeqCst) {return Ok(None);}
                anyhow::ensure!(started.elapsed()<timeout,"等待授权超时");
                for event in rx.try_iter() {
                    match event? {Some(Display::Code(value))=>code=Some(value),Some(Display::Url)=>url_ready=true,None=>{}}
                    if url_ready&&code.is_some() {
                        let mut view=attempt.view.lock().unwrap();
                        if view.state!="cancelled" {view.state="waiting".into();view.user_code=code.clone();view.verification_uri=Some(VERIFY_URL.into());view.qr_svg=Some(qr_svg()?);}
                    }
                }
                if let Some(status)=child.try_wait()? {return Ok(Some(status));}
                std::thread::sleep(Duration::from_millis(100));
            }
        })();
        if !matches!(&outcome,Ok(Some(_))) {let _=child.kill();let _=child.wait();}
        #[cfg(windows)]
        drop(_group); // Close every owned pipe before joining the output reader.
        let _=reader.join();
        outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn parses_only_expected_device_display() {
        assert!(matches!(display_line("! First copy your one-time code: ABCD-1234\n").unwrap(),Some(Display::Code(_))));
        assert!(matches!(display_line(&format!("Open this URL to continue in your web browser: {VERIFY_URL}\n")).unwrap(),Some(Display::Url)));
        for url in ["http://github.com/login/device","https://evil.test/login/device","https://github.com/login/oauth/authorize?redirect_uri=http://127.0.0.1/callback"] {
            assert!(display_line(&format!("Open this URL to continue in your web browser: {url}")).is_err());
        }
        assert!(display_line("First copy your one-time code: secret-token").is_err());
        assert!(display_line("token=secret ignored diagnostic").unwrap().is_none());
    }
    #[test] fn cancellation_before_start_is_a_tombstone_and_does_not_cancel_other_requests() {
        let mut registry=Registry::default();
        let (a,spawn)=registry.reserve("a",false).unwrap();assert!(spawn);
        assert!(registry.reserve("b",false).is_err());
        let (b,spawn)=registry.reserve("b",true).unwrap();assert!(!spawn);assert_eq!(b.snapshot()["attempt"]["state"],"cancelled");
        assert!(a.active());assert!(!registry.reserve("b",false).unwrap().1);
        registry.reserve("a",true).unwrap();assert!(!a.active());
        a.finish("complete",None,Some("late".into()));assert_eq!(a.snapshot()["attempt"]["state"],"cancelled");
        assert!(registry.reserve("c",false).unwrap().1);
    }
    #[test] fn terminal_states_clear_codes_and_duplicate_start_is_idempotent() {
        let mut registry=Registry::default();let (a,spawn)=registry.reserve("a",false).unwrap();assert!(spawn);
        assert!(!registry.reserve("a",false).unwrap().1);
        {let mut v=a.view.lock().unwrap();v.user_code=Some("ABCD-1234".into());v.qr_svg=Some(qr_svg().unwrap());}
        a.finish("failed",Some("expired"),None);let v=a.snapshot();assert!(v["attempt"]["user_code"].is_null());assert!(v["attempt"]["qr_svg"].is_null());
    }
    #[test] fn generates_local_qr_fixture_for_decode_verification() {
        let svg=qr_svg().unwrap();assert!(svg.contains("<svg"));assert!(!svg.contains("<script"));
        if let Some(path)=std::env::var_os("VELELLA_QR_TEST_OUTPUT") {std::fs::write(path,svg).unwrap();}
    }
    #[test] #[ignore] fn cli_child_fixture() {
        let mode=std::env::var("VELELLA_QR_CHILD_FIXTURE").unwrap();
        eprintln!("diagnostic token=SHOULD_NOT_REACH_UI");
        eprintln!("! First copy your one-time code: ABCD-1234");
        eprintln!("Open this URL to continue in your web browser: {}",if mode=="unsupported"{"https://github.com/login/oauth/authorize"}else{VERIFY_URL});
        if mode!="complete" {std::thread::sleep(Duration::from_secs(20));}
    }
    fn fixture(mode:&str)->std::process::Child {
        crate::process::command(std::env::current_exe().unwrap().to_str().unwrap())
            .args(["--ignored","--exact","task_sources::github_login::tests::cli_child_fixture","--nocapture"])
            .env("VELELLA_QR_CHILD_FIXTURE",mode).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap()
    }
    #[test] fn owned_login_process_cancels_without_publishing_diagnostics() {
        let attempt=Arc::new(Attempt::new("cancel-test"));let worker=attempt.clone();
        let thread=std::thread::spawn(move||wait_for_login(&worker,fixture("waiting"),Duration::from_secs(5)));
        let deadline=Instant::now()+Duration::from_secs(4);
        while attempt.snapshot()["attempt"]["state"]!="waiting" {assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(20));}
        assert!(!attempt.snapshot().to_string().contains("SHOULD_NOT_REACH_UI"));
        attempt.cancel();assert!(thread.join().unwrap().unwrap().is_none());
        assert_eq!(attempt.snapshot()["attempt"]["state"],"cancelled");
        assert!(attempt.snapshot()["attempt"]["user_code"].is_null());
    }
    #[test] fn unsupported_flow_and_timeout_stop_the_owned_process() {
        for (mode,timeout) in [("unsupported",Duration::from_secs(4)),("waiting",Duration::from_millis(150))] {
            let started=Instant::now();assert!(wait_for_login(&Attempt::new(mode),fixture(mode),timeout).is_err());
            assert!(started.elapsed()<Duration::from_secs(5));
        }
    }
    #[test] fn cli_exit_alone_does_not_claim_verified_login() {
        let attempt=Attempt::new("complete-test");
        assert!(wait_for_login(&attempt,fixture("complete"),Duration::from_secs(4)).unwrap().unwrap().success());
        assert_ne!(attempt.snapshot()["attempt"]["state"],"complete");
    }
}
