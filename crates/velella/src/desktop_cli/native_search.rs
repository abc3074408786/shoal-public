//! Read-only native history search. Never reads auth/config or model reasoning.
use super::*;
use std::{fs, io::{BufRead, BufReader, Read}, path::{Path, PathBuf}, time::Instant};

const MAX_LINE: usize = 2 * 1024 * 1024;
const MAX_ENTRIES: usize = 20000;

#[derive(ClapArgs)]
pub(super) struct Options {
    /// Case-sensitive literal text; 1..4096 UTF-8 bytes
    query: String,
    #[arg(long, required_unless_present = "path", conflicts_with = "path")]
    workspace: Option<String>,
    /// Exact existing working folder; works without a daemon
    #[arg(long)]
    path: Option<PathBuf>,
    #[arg(long, value_parser = ["codex", "claude"])]
    agent: Option<String>,
    /// Exact native session UUID, not a Velella terminal ID
    #[arg(long)]
    session: Option<uuid::Uuid>,
    #[arg(long, default_value = "conversation", value_parser = ["conversation", "all"])]
    scope: String,
    #[arg(long, default_value_t = 200, value_parser = clap::value_parser!(u32).range(1..=2000))]
    max_files: u32,
    /// Total JSONL bytes read across all files
    #[arg(long, default_value_t = 33554432, value_parser = clap::value_parser!(u64).range(1..=268435456))]
    max_bytes: u64,
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=500))]
    max_matches: u32,
    #[arg(long, default_value_t = 30000, value_parser = clap::value_parser!(u64).range(1..=60000))]
    deadline_ms: u64,
}

pub(super) fn execute(client: &Client, options: &Options) -> Result<Value> {
    anyhow::ensure!(client.remote.is_none(), "native history search is local-only");
    anyhow::ensure!(!options.query.is_empty() && options.query.len() <= 4096, "query must contain 1..4096 UTF-8 bytes");
    let cwd = if let Some(path) = &options.path { path.clone() } else {
        let id = options.workspace.as_ref().context("--workspace or --path required")?;
        let snapshot = client.snapshot()?;
        let workspace = find(array(&snapshot, "workspaces")?, id, "workspace")?;
        PathBuf::from(workspace["path"].as_str().context("workspace path unavailable")?)
    };
    let cwd = dunce::canonicalize(cwd).context("search working folder unavailable")?;
    anyhow::ensure!(cwd.is_dir(), "search path must be a folder");
    let home = std::env::var_os("USERPROFILE").or_else(||std::env::var_os("HOME")).map(PathBuf::from).context("user home unavailable")?;
    let codex = std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(||home.join(".codex"));
    let claude = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(||home.join(".claude"));
    search(&cwd, &[("codex",codex.join("sessions")), ("claude",claude.join("projects"))], options)
}

fn plain(path: &Path) -> Result<bool> {
    let meta = fs::symlink_metadata(path)?;
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 { return Ok(false); }
    }
    Ok(!meta.file_type().is_symlink())
}

struct Discovery { files: Vec<(String,PathBuf)>, entries: usize, incomplete: bool }
impl Discovery {
    fn walk(&mut self, agent: &str, root: &Path, path: &Path, depth: usize, deadline: Instant) -> Result<()> {
        if !path.exists() { return Ok(()); }
        if depth == 0 {
            if fs::read_dir(path)?.next().transpose()?.is_some() {self.incomplete=true;}
            return Ok(());
        }
        if Instant::now() >= deadline || self.entries >= MAX_ENTRIES { self.incomplete=true; return Ok(()); }
        if !plain(path)? { self.incomplete=true; return Ok(()); }
        anyhow::ensure!(dunce::canonicalize(path)?.starts_with(root), "history directory leaves its root");
        for entry in fs::read_dir(path)? {
            if Instant::now() >= deadline || self.entries >= MAX_ENTRIES { self.incomplete=true; break; }
            self.entries+=1;
            let entry=entry?; let path=entry.path();
            if !plain(&path)? { self.incomplete=true; continue; }
            if entry.file_type()?.is_dir() {
                // Claude subagents are deliberately outside this command's scope,
                // unlike an unexpected directory cut off by the traversal bound.
                if agent=="claude" && entry.file_name()=="subagents" {continue;}
                self.walk(agent,root,&path,depth-1,deadline)?;
            }
            else if path.extension().is_some_and(|e|e=="jsonl") && entry.file_type()?.is_file() {
                self.files.push((agent.into(),path));
            }
        }
        Ok(())
    }
}

fn same_cwd(value: &Value, cwd: &Path) -> bool {
    value.as_str().is_some_and(|p|dunce::canonicalize(p).is_ok_and(|p|p==cwd))
}

// Only visible message text and, when explicitly requested, tool inputs/results.
// Thinking/reasoning, system messages and arbitrary metadata are never searched.
fn blocks(value: &Value, tools: bool, output: &mut Vec<String>) {
    if let Some(text)=value.as_str() { output.push(text.into()); }
    else if let Some(parts)=value.as_array() { for part in parts {
        match part["type"].as_str().unwrap_or_default() {
            "text" | "input_text" | "output_text" => if let Some(text)=part["text"].as_str(){output.push(text.into());},
            "tool_result" if tools => blocks(&part["content"],true,output),
            "tool_use" if tools => output.push(part["input"].to_string()),
            _=>{},
        }
    }}
}

fn record_text(agent: &str, record: &Value, tools: bool) -> Option<(String,String)> {
    let mut texts=Vec::new();
    let role=if agent=="codex" {
        if record["type"]!="response_item" { return None; }
        let payload=&record["payload"];
        if payload["type"]=="message" {
            let role=payload["role"].as_str()?;
            if !matches!(role,"user"|"assistant") {return None;}
            blocks(&payload["content"],tools,&mut texts); role.to_owned()
        } else if tools {
            match payload["type"].as_str()? {
                "function_call" => blocks(&payload["arguments"],true,&mut texts),
                "custom_tool_call" => blocks(&payload["input"],true,&mut texts),
                "function_call_output" | "custom_tool_call_output" => blocks(&payload["output"],true,&mut texts),
                _=>return None,
            }; "tool".into()
        } else {return None;}
    } else {
        let role=record["type"].as_str()?;
        if !matches!(role,"user"|"assistant") {return None;}
        blocks(&record["message"]["content"],tools,&mut texts);role.to_owned()
    };
    (!texts.is_empty()).then(||(role,texts.join("\n")))
}

fn snippet(text: &str, query: &str) -> Option<String> {
    let at=text.find(query)?;
    let before=text[..at].chars().rev().take(80).collect::<Vec<_>>().into_iter().rev().collect::<String>();
    let after=text[at..].chars().take(240).collect::<String>();
    Some(format!("{before}{after}"))
}

fn search(cwd: &Path, roots: &[(&str,PathBuf)], options: &Options) -> Result<Value> {
    let deadline=Instant::now()+Duration::from_millis(options.deadline_ms);
    let mut discovery=Discovery{files:vec![],entries:0,incomplete:false};
    let mut errors=Vec::new();
    for (agent,root) in roots {
        if options.agent.as_deref().is_some_and(|wanted|wanted!=*agent) || !root.exists() {continue;}
        // Check each ancestor too: a symlinked native home must not escape silently.
        if root.ancestors().any(|p|plain(p).is_ok_and(|ok|!ok)) {discovery.incomplete=true;errors.push(json!({"agent":agent,"error":"linked_history_root"}));continue;}
        let root=dunce::canonicalize(root)?;
        if let Err(_) = discovery.walk(agent,&root,&root,if *agent=="codex"{5}else{2},deadline) {
            discovery.incomplete=true; errors.push(json!({"agent":agent,"error":"history_discovery_failed"}));
        }
    }
    if let Some(session)=&options.session {
        let id=session.to_string();
        discovery.files.retain(|(_,p)|p.file_stem().is_some_and(|s|s.to_string_lossy().ends_with(&id)));
    }
    discovery.files.sort_by_cached_key(|(_,p)|std::cmp::Reverse(fs::metadata(p).and_then(|m|m.modified()).ok()));
    let discovered=discovery.files.len();
    let (mut matches,mut reports)=(Vec::new(),Vec::new());
    let mut bytes=0u64;
    for (agent,path) in discovery.files.iter().take(options.max_files as usize) {
        if Instant::now()>=deadline || bytes>=options.max_bytes || matches.len()>=options.max_matches as usize {break;}
        let mut offset=0u64;let mut scoped=false;let mut native=String::new();let mut complete=true;
        let first_hit=matches.len();
        let result=(||->Result<()> {
            anyhow::ensure!(plain(path)? && fs::metadata(path)?.is_file(), "history file changed");
            let snapshot=fs::metadata(path)?.len();
            let mut reader=BufReader::new(fs::File::open(path)?.take(snapshot.min(options.max_bytes-bytes)));
            while offset<snapshot {
                if Instant::now()>=deadline || bytes>=options.max_bytes || matches.len()>=options.max_matches as usize {complete=false;break;}
                let allowance=(options.max_bytes-bytes).min(MAX_LINE as u64+1) as usize;
                let mut line=Vec::new();
                let n=reader.by_ref().take(allowance as u64).read_until(b'\n',&mut line)?;
                bytes+=n as u64;
                if n==0 {break;}
                let start=offset;offset+=n as u64;
                if line.last()!=Some(&b'\n') || line.len()>MAX_LINE {complete=false;break;}
                if line.iter().all(u8::is_ascii_whitespace) {continue;}
                let record:Value=serde_json::from_slice(&line).context("invalid JSONL record")?;
                if agent=="codex" {
                    if record["type"]=="session_meta" {
                        native=record["payload"]["id"].as_str().or_else(||record["payload"]["session_id"].as_str()).unwrap_or_default().into();
                        scoped=same_cwd(&record["payload"]["cwd"],cwd);
                    } else if record["type"]=="turn_context" {scoped=same_cwd(&record["payload"]["cwd"],cwd);}
                } else {
                    native=record["sessionId"].as_str().unwrap_or_default().into();
                    scoped=same_cwd(&record["cwd"],cwd);
                }
                if !scoped || uuid::Uuid::parse_str(&native).is_err() || options.session.is_some_and(|id|Some(id)!=uuid::Uuid::parse_str(&native).ok()) {continue;}
                if let Some((role,text))=record_text(agent,&record,options.scope=="all") {
                    if let Some(snippet)=snippet(&text,&options.query) {
                        matches.push(json!({"agent":agent,"native_session_id":native,"path":path,"byte_start":start,"byte_end":offset,"role":role,"timestamp":record["timestamp"],"snippet":snippet}));
                    }
                }
            }
            anyhow::ensure!(fs::metadata(path)?.len()>=snapshot,"history file was truncated");
            Ok(())
        })();
        if result.is_err() {matches.truncate(first_hit);complete=false;errors.push(json!({"path":path,"error":"history_read_failed"}));}
        reports.push(json!({"agent":agent,"path":path,"scanned_to":offset,"range_complete":complete}));
    }
    let omitted=discovered-reports.len();
    let complete=!discovery.incomplete && errors.is_empty() && omitted==0 && reports.iter().all(|r|r["range_complete"]==true);
    Ok(json!({"representation":"native-session-jsonl","persistent_index":false,"path":cwd,"workspace_id":options.workspace,
        "scope":options.scope,"case_sensitive":true,"matches":matches,"files":reports,"discovered_files":discovered,
        "omitted_files":omitted,"bytes_read":bytes,"complete":complete,"errors":errors,
        "snapshot_policy":"fixed byte end per file; append-only native history required"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options()->Options {Options{query:"搜索中文".into(),workspace:None,path:None,agent:None,session:None,scope:"conversation".into(),max_files:200,max_bytes:33554432,max_matches:100,deadline_ms:30000}}
    fn fixture()->(PathBuf,PathBuf,PathBuf) {
        let root=std::env::temp_dir().join(format!("vl-native-search-{}",uuid::Uuid::now_v7()));
        let cwd=root.join("work");let history=root.join("history");fs::create_dir_all(&cwd).unwrap();fs::create_dir_all(&history).unwrap();
        let cwd=dunce::canonicalize(cwd).unwrap();(root,cwd,history)
    }
    #[test] fn native_search_scopes_identity_and_excludes_reasoning_and_tools_by_default() {
        let (root,cwd,history)=fixture();let id=uuid::Uuid::now_v7();
        let records=[json!({"type":"session_meta","payload":{"id":id,"cwd":cwd}}),
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"搜索中文"}]}}),
            json!({"type":"response_item","payload":{"type":"reasoning","summary":[{"text":"搜索中文"}]}}),
            json!({"type":"response_item","payload":{"type":"function_call_output","output":"搜索中文 tool"}})];
        fs::write(history.join(format!("rollout-{id}.jsonl")),records.iter().map(|r|format!("{r}\n")).collect::<String>()).unwrap();
        let mut options=options();let result=search(&cwd,&[("codex",history.clone())],&options).unwrap();
        assert_eq!(result["matches"].as_array().unwrap().len(),1);assert_eq!(result["complete"],true);
        options.scope="all".into();assert_eq!(search(&cwd,&[("codex",history.clone())],&options).unwrap()["matches"].as_array().unwrap().len(),2);
        assert!(search(&root,&[("codex",history)],&options).unwrap()["matches"].as_array().unwrap().is_empty());fs::remove_dir_all(root).unwrap();
    }
    #[test] fn claude_text_tool_results_and_incomplete_tail_are_distinguished() {
        let (root,cwd,history)=fixture();let id=uuid::Uuid::now_v7();
        let row=json!({"type":"user","sessionId":id,"cwd":cwd,"message":{"content":[{"type":"text","text":"搜索中文"},{"type":"tool_result","content":"搜索中文 private tool"}]}});
        fs::write(history.join(format!("{id}.jsonl")),format!("{row}\n{{\"partial\":" )).unwrap();
        let result=search(&cwd,&[("claude",history.clone())],&options()).unwrap();
        assert_eq!(result["matches"].as_array().unwrap().len(),1);assert_eq!(result["complete"],false);
        assert!(!result["matches"][0]["snippet"].as_str().unwrap().contains("private tool"));
        let mut limited=options();limited.max_bytes=10;
        let result=search(&cwd,&[("claude",history)],&limited).unwrap();assert_eq!(result["bytes_read"],10);assert_eq!(result["complete"],false);fs::remove_dir_all(root).unwrap();
    }
    #[test] fn argument_scope_and_remote_rejection_are_explicit() {
        assert!(Cli::try_parse_from(["velella-cli","search","x"]).is_err());
        assert!(Cli::try_parse_from(["velella-cli","search","x","--path",".","--scope","all"]).is_ok());
        assert!(record_text("claude",&json!({"type":"assistant","message":{"content":[{"type":"thinking","thinking":"secret"}]}}),true).is_none());
    }
    #[test] fn depth_cutoffs_cannot_claim_complete_but_explicit_subagent_exclusion_can() {
        let (root,cwd,history)=fixture();
        let deeper=history.join("a/b/c/d/e");fs::create_dir_all(&deeper).unwrap();fs::write(deeper.join("hidden.jsonl"),b"{}\n").unwrap();
        let result=search(&cwd,&[("codex",history)],&options()).unwrap();assert_eq!(result["complete"],false);
        let claude=root.join("claude/project/subagents");fs::create_dir_all(&claude).unwrap();fs::write(claude.join("excluded.jsonl"),b"{}\n").unwrap();
        assert_eq!(search(&cwd,&[("claude",root.join("claude"))],&options()).unwrap()["complete"],true);
        fs::create_dir_all(root.join("claude/project/unexpected")).unwrap();fs::write(root.join("claude/project/unexpected/hidden.jsonl"),b"{}\n").unwrap();
        assert_eq!(search(&cwd,&[("claude",root.join("claude"))],&options()).unwrap()["complete"],false);
        fs::remove_dir_all(root).unwrap();
    }
}
