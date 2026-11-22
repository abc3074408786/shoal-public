use anyhow::{Context, Result};
use reqwest::{header::{HeaderMap, HeaderName, HeaderValue}, Method, Url};
use serde_json::Value;
use std::{io::{Read, Write}, path::{Path, PathBuf}, process::Stdio, time::{Duration, Instant}};

const LIMIT: u64 = 8 * 1024 * 1024;

pub(super) fn executable(name: &str) -> Option<PathBuf> {
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.into() };
    std::env::split_paths(&std::env::var_os("PATH")?).filter(|p| p.is_absolute()).map(|p|p.join(&file)).find(|p|p.is_file())
}

pub fn run(program: &str, args: &[String], input: Option<Vec<u8>>, directory: Option<&Path>) -> Result<Vec<u8>> {
    let binary=executable(program).with_context(||format!("未找到 {program}，请安装后使用已有登录，或在连接设置中选择个人 Token"))?;
    let mut command=crate::process::command(binary.to_str().context("CLI path is not Unicode")?);
    command.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .env("GH_PROMPT_DISABLED","1").env("GLAB_PROMPT_DISABLED","1").env("GIT_TERMINAL_PROMPT","0");
    if let Some(directory)=directory { command.current_dir(directory); }
    let mut child=command.spawn().with_context(||format!("无法启动 {program}"))?;
    let mut stdin=child.stdin.take().context("CLI stdin unavailable")?;
    let writer=std::thread::spawn(move|| { if let Some(input)=input { let _=stdin.write_all(&input); } });
    let stdout=child.stdout.take().context("CLI stdout unavailable")?;
    let reader=std::thread::spawn(move|| { let mut bytes=Vec::new();stdout.take(LIMIT+1).read_to_end(&mut bytes).map(|_|bytes) });
    let deadline=Instant::now()+Duration::from_secs(30);
    let status=loop {
        if let Some(status)=child.try_wait()? { break status; }
        if Instant::now()>=deadline { let _=child.kill();let _=child.wait();anyhow::bail!("{program} 请求超时；不会自动重试写入，请检查来源平台"); }
        std::thread::sleep(Duration::from_millis(25));
    };
    let _=writer.join();
    let bytes=reader.join().map_err(|_|anyhow::anyhow!("CLI response reader failed"))??;
    anyhow::ensure!(bytes.len() as u64<=LIMIT,"来源响应过大，请缩小范围");
    // CLI stderr can contain credentials or server diagnostics; never return it to the UI.
    anyhow::ensure!(status.success(),"{program} 请求失败，请确认该站点的 CLI 登录及项目权限后刷新；写入结果请先到来源平台核对");
    Ok(bytes)
}

pub fn request(provider: &str, method: Method, url: &Url, body: Option<&Value>) -> Result<(Value, HeaderMap)> {
    let program=match provider {"github"=>"gh","gitlab"=>"glab",_=>anyhow::bail!("此来源不支持 CLI 登录")};
    let host=if url.host_str()==Some("api.github.com") {"github.com"} else {url.host_str().context("source host required")?};
    anyhow::ensure!(url.port().is_none(),"CLI 来源暂不支持自定义端口，请使用个人 Token");
    let prefix=if provider=="github" {"/api/v3"} else {"/api/v4"};
    let endpoint=format!("{}{}",url.path().strip_prefix(prefix).unwrap_or(url.path()),url.query().map(|q|format!("?{q}")).unwrap_or_default());
    let mut args=vec!["api".into(),"--hostname".into(),host.into(),"--method".into(),method.to_string(),"--include".into(),endpoint];
    if body.is_some() {args.extend(["--input".into(),"-".into()]);}
    let bytes=run(program,&args,body.map(serde_json::to_vec).transpose()?,None)?;
    parse_response(&bytes)
}

fn parse_response(bytes: &[u8]) -> Result<(Value,HeaderMap)> {
    let separator=bytes.windows(4).position(|w|w==b"\r\n\r\n").map(|i|(i,4))
        .or_else(||bytes.windows(2).position(|w|w==b"\n\n").map(|i|(i,2))).context("CLI 未返回 HTTP 响应头，请更新 CLI")?;
    let headers=std::str::from_utf8(&bytes[..separator.0])?;
    let status=headers.lines().next().and_then(|l|l.split_whitespace().nth(1)).and_then(|s|s.parse::<u16>().ok()).context("CLI HTTP status missing")?;
    anyhow::ensure!((200..300).contains(&status),"来源请求失败（HTTP {status}），请检查 CLI 登录和权限");
    let mut map=HeaderMap::new();
    for line in headers.lines().skip(1) { if let Some((key,value))=line.split_once(':') { map.insert(HeaderName::from_bytes(key.trim().as_bytes())?,HeaderValue::from_str(value.trim())?); } }
    let body=&bytes[separator.0+separator.1..];
    Ok((if body.is_empty(){Value::Null}else{serde_json::from_slice(body).context("来源返回了无效 JSON")?},map))
}

pub fn remote(value: &str) -> Option<(String,String)> {
    let (host,path)=if let Ok(url)=Url::parse(value) {
        if !matches!(url.scheme(),"https"|"http"|"ssh") || url.port().is_some() {return None;}
        (url.host_str()?.to_owned(),url.path().to_owned())
    } else {
        let (prefix,path)=value.split_once(':')?;
        let host=prefix.rsplit('@').next()?;
        if host.contains(['/', '\\']) || !host.contains('.') {return None;}
        (host.into(),path.into())
    };
    let path=path.trim_matches('/').trim_end_matches(".git");
    if path.split('/').count()<2 || path.split('/').any(|p|p.is_empty()||p=="."||p==".."||!p.chars().all(|c|c.is_ascii_alphanumeric()||matches!(c,'-'|'_'|'.'))) {return None;}
    Some((host.to_lowercase(),path.into()))
}

pub fn repositories(projects: &[crate::desktop::Project]) -> Vec<Value> {
    let mut rows=Vec::new();let mut known_hosts=std::collections::HashMap::<String,String>::new();
    for project in projects.iter().filter(|p|!p.direct) {
        let Ok(bytes)=run("git",&["config".into(),"--get-regexp".into(),"^remote\\..*\\.url$".into()],None,Some(&project.path)) else {continue};
        for line in String::from_utf8_lossy(&bytes).lines() {
            let Some((_,value))=line.split_once(char::is_whitespace) else {continue};
            let Some((host,scope))=remote(value.trim()) else {continue};
            let provider=known_hosts.entry(host.clone()).or_insert_with(||{
                if host=="github.com" {return "github".into();}if host=="gitlab.com" {return "gitlab".into();}
                for (program,provider) in [("gh","github"),("glab","gitlab")] {
                    if executable(program).is_some()&&run(program,&["auth".into(),"status".into(),"--hostname".into(),host.clone()],None,None).is_ok(){return provider.into();}
                }String::new()
            });
            let row=serde_json::json!({"project_id":project.id,"project_name":project.name,"host":host,"scope":scope,"provider":provider});
            if !rows.contains(&row) {rows.push(row);}
        }
    }
    rows
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn parses_remotes_without_exposing_credentials() {
        assert_eq!(remote("git@github.com:owner/repo.git"),Some(("github.com".into(),"owner/repo".into())));
        assert_eq!(remote("https://user:secret@git.example/group/sub/repo.git"),Some(("git.example".into(),"group/sub/repo".into())));
        assert!(remote("C:\\repo").is_none());assert!(remote("ssh://git@example.com/../repo").is_none());
    }
    #[test] fn parses_cli_headers_for_pagination() {
        let (value,headers)=parse_response(b"HTTP/2.0 200 OK\r\nX-Next-Page: 2\r\n\r\n[{\"iid\":7}]").unwrap();
        assert_eq!(value[0]["iid"],7);assert_eq!(headers["x-next-page"],"2");
        assert!(parse_response(b"HTTP/2 403 Forbidden\n\n{}").is_err());
    }
}
