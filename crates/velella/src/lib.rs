//! Shared desktop runtime. The GUI is a client; session hosts own the PTYs.
pub mod sandbox;
pub mod restricted;
pub mod ipc;
pub mod desktop;
pub mod desktop_cli;
mod desktop_bridge;
mod workbench;
mod host;
mod restricted_controller;
mod restricted_cleanup;
mod terminal_sandbox;
mod terminal_bridge;
mod terminal_config_audit;
mod terminal_input;
mod terminal_activity;
mod terminal_restore;
mod terminal_progress;
mod submission_receipts;
mod process;
mod browser_agent;
mod task_sources;
pub mod collaboration;
#[allow(dead_code)]
mod sessions;
mod power_continuity;
#[allow(dead_code)]
mod adapter;
mod terminal_recovery;
pub mod native_launch;
pub mod terminal_permissions;
mod remote_stdio;
mod hybrid;

pub fn runtime_main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("hybrid-status")=>{
            anyhow::ensure!(args.len()==3,"hybrid-status requires one session directory");
            let config=hybrid::load(std::path::Path::new(&args[2]))?;
            println!("{}",hybrid::tools::status(&config)?);Ok(())
        }
        Some("hybrid-decide")=>{
            anyhow::ensure!(args.len()==6&&matches!(args[5].as_str(),"approve"|"reject"),"hybrid-decide requires directory, operation ID, digest and approve/reject");
            let config=hybrid::load(std::path::Path::new(&args[2]))?;
            println!("{}",hybrid::tools::decide(&config,&args[3],&args[4],args[5]=="approve")?);Ok(())
        }
        Some("hybrid-session") => {
            anyhow::ensure!(args.len()==3||args.len()==5&&args[3]=="--exec","hybrid-session requires a session directory and optional --exec prompt file");
            hybrid::run(std::path::Path::new(&args[2]),args.get(4).map(std::path::Path::new),false)
        }
        Some("hybrid-probe")=>{anyhow::ensure!(args.len()==3,"hybrid-probe requires a session directory");hybrid::run(std::path::Path::new(&args[2]),None,true)}
        Some("hybrid-worker")=>{anyhow::ensure!(args.len()==3,"hybrid-worker requires a session directory");hybrid::worker(std::path::Path::new(&args[2]))}
        Some("remote-stdio") => {
            anyhow::ensure!(args.len() == 2, "remote-stdio accepts no CLI arguments; use protocol stdin");
            remote_stdio::run()
        }
        Some("restricted-probe") => restricted::run_probe(),
        Some("restricted-exec") => restricted::run_encoded(
            args.get(2).ok_or_else(|| anyhow::anyhow!("restricted-exec requires a launch specification"))?),
        Some("restricted-tool") => restricted::run_tool(
            args.get(2).ok_or_else(|| anyhow::anyhow!("restricted-tool requires a tool specification"))?),
        Some("restricted-exec-server") => restricted_controller::run_exec_server(std::path::Path::new(
            args.get(2).ok_or_else(|| anyhow::anyhow!("restricted-exec-server requires a configuration path"))?)),
        Some("sandbox-capabilities") => {
            println!("{}", serde_json::to_string_pretty(&sandbox::capabilities())?);
            Ok(())
        }
        Some("desktop-bridge") => desktop_bridge::run(&args.get(2).map(std::path::PathBuf::from).unwrap_or_else(ipc::data_dir)),
        Some("ipc-endpoint") => {
            let root=args.get(2).map(std::path::PathBuf::from).unwrap_or_else(ipc::data_dir);
            let name=args.get(3).map(String::as_str).unwrap_or("browser");
            anyhow::ensure!(matches!(name,"browser"|"daemon"),"unsupported IPC endpoint");
            let endpoint=ipc::endpoint(&root,name);
            println!("{}",serde_json::to_string(&if cfg!(windows){format!(r"\\.\pipe\{endpoint}")}else{endpoint})?);
            Ok(())
        }
        Some("daemon") => desktop::serve(args.get(2).map(std::path::PathBuf::from).unwrap_or_else(ipc::data_dir)),
        Some("host") => {
            let path=std::path::Path::new(args.get(2).ok_or_else(|| anyhow::anyhow!("host requires descriptor"))?);
            let result=host::run(path);
            if let Err(error)=&result {if let Some(parent)=path.parent(){let _=std::fs::write(parent.join("startup-error.txt"),format!("{error:#}"));}}
            result
        }
        Some("activity-hook") => terminal_activity::hook(),
        Some("native-restore-hook") => terminal_restore::hook(args.get(2).map(String::as_str),args.get(3).map(String::as_str)),
        Some("native-restore-hook-event") => terminal_restore::hook(None,args.get(2).map(String::as_str)),
        Some("context-usage") => {
            use std::io::Read;
            let mut input=Vec::new();std::io::stdin().take(65537).read_to_end(&mut input)?;
            anyhow::ensure!(input.len()<=65536,"context status input exceeds limit");
            let payload:serde_json::Value=serde_json::from_slice(&input)?;
            let response=ipc::request(&std::env::var("VELELLA_SESSION_ENDPOINT")?,&serde_json::json!({"cmd":"context_usage","payload":payload}))?;
            anyhow::ensure!(response["ok"]==true,"context observation was not recorded");
            if let Some(percent)=response["context_usage"]["percent"].as_f64(){
                println!("Context {percent:.1}%{}",if response["context_usage"]["handoff_required"]==true{" · 需交接"}else{""});
            }else{println!("Context 未知");}
            Ok(())
        }
        Some("collaboration-mcp") => collaboration::mcp::serve(
            std::path::Path::new(args.get(2).ok_or_else(||anyhow::anyhow!("data directory required"))?),
            args.get(3).ok_or_else(||anyhow::anyhow!("session required"))?),
        Some("external-mcp") => collaboration::mcp::external_serve(
            std::path::Path::new(args.get(2).ok_or_else(||anyhow::anyhow!("external MCP descriptor required"))?),
            args.get(3).ok_or_else(||anyhow::anyhow!("external MCP fingerprint required"))?),
        Some("orchestration-hook") => {
            use std::io::Read;
            let mut input=String::new();std::io::stdin().take(1024*1024).read_to_string(&mut input)?;
            let mut event:serde_json::Value=serde_json::from_str(&input).unwrap_or_else(|_|serde_json::json!({}));
            event["hook_event_name"]=serde_json::json!(args.get(2).ok_or_else(||anyhow::anyhow!("event required"))?);
            let event_name=event["hook_event_name"].as_str().unwrap_or_default();
            let result=(|| -> anyhow::Result<serde_json::Value> {
                let output=collaboration::providers::orchestration_hook_output(event_name)?;
                let response=ipc::request(&std::env::var("VELELLA_SESSION_ENDPOINT")?,&serde_json::json!({"cmd":"hook","event":event}))?;
                anyhow::ensure!(response["ok"]!=false,"session rejected the lifecycle receipt");
                Ok(output)
            })();
            match result {
                Ok(output)=>{println!("{output}");Ok(())},
                Err(_) if event_name=="UserPromptSubmit" && std::env::var("VELELLA_PROVIDER").ok().as_deref()==Some("gemini")=>{
                    // Gemini treats a generic non-zero hook exit as fail-open.
                    // A successful JSON blocking decision stops this task.
                    println!("{}",serde_json::json!({"decision":"block","reason":"Velella 角色技能校验或会话连接失败，任务已停止；请恢复受控会话后重试。"}));Ok(())
                },
                Err(error)=>Err(error),
            }
        }
        Some("hook") => {
            if let Some(inbox) = std::env::var_os("VELELLA_HOOK_INBOX") {
                host::hook_inbox(std::path::Path::new(&inbox))
            } else {
                host::hook(&args.get(2).cloned().or_else(|| std::env::var("VELELLA_SESSION_ENDPOINT").ok()).ok_or_else(|| anyhow::anyhow!("hook requires session endpoint"))?)
            }
        },
        Some("collab") => {
            use clap::Parser;
            #[derive(Parser)]
            struct Collab { #[command(flatten)] args: collaboration::cli::Args }
            collaboration::cli::run(Collab::parse_from(std::iter::once(args[0].clone()).chain(args.iter().skip(2).cloned())).args)
        }
        Some("rpc") => {
            let dir = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(ipc::data_dir);
            let input = if args.get(3).map(String::as_str) == Some("--stdin") {
                use std::io::Read;
                let mut input = String::new();
                std::io::stdin().take(2 * 1024 * 1024 + 1).read_to_string(&mut input)?;
                anyhow::ensure!(input.len() <= 2 * 1024 * 1024, "RPC request too large");
                input
            } else { args.get(3).cloned().unwrap_or_else(|| "{\"cmd\":\"snapshot\"}".into()) };
            let request: serde_json::Value = serde_json::from_str(&input)?;
            println!("{}", ipc::request(&ipc::endpoint(&dir, "daemon"), &request)?);
            Ok(())
        }
        Some("browser-mcp") => browser_agent::serve(
            std::path::Path::new(args.get(2).ok_or_else(|| anyhow::anyhow!("browser-mcp requires data directory"))?),
            args.get(3).ok_or_else(|| anyhow::anyhow!("browser-mcp requires workspace"))?),
        Some("browser") => {
            let dir = args.get(2).map(std::path::PathBuf::from).unwrap_or_else(ipc::data_dir);
            let input = if args.get(3).map(String::as_str) == Some("--request-file") {
                let path = args.get(4).ok_or_else(|| anyhow::anyhow!("--request-file requires a path"))?;
                anyhow::ensure!(std::fs::metadata(path)?.len() <= 1024 * 1024, "browser request too large");
                std::fs::read_to_string(path)?
            } else {
                args.get(3).cloned().unwrap_or_else(|| "{\"op\":\"status\"}".into())
            };
            let request: serde_json::Value = serde_json::from_str(input.trim_start_matches('\u{feff}'))?;
            let result = ipc::request(&ipc::endpoint(&dir, "browser"), &request)
                .map_err(|e| anyhow::anyhow!("browser_host_unavailable: open the Velella desktop for this data directory ({e})"))?;
            println!("{result}");
            if result["ok"] == false { std::process::exit(1); }
            Ok(())
        }
        _ => anyhow::bail!("usage: velella-runtime daemon [data-dir] | host <descriptor> | hook <endpoint> | collab --help | rpc <data-dir> <json> | browser <data-dir> [<json> | --request-file <path>]"),
    }
}
