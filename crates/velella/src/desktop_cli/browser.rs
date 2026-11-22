//! The desktop owns page identity, presentation and snapshot-reference checks.
use super::*;

#[derive(ClapArgs)]
pub(super) struct Page {
    #[arg(long)]
    workspace: String,
    #[arg(long)]
    page: String,
}

#[derive(Subcommand)]
pub(super) enum BrowserCommand {
    Status,
    List {
        #[arg(long)]
        workspace: String,
    },
    Show(Page),
    /// Create and display a page; --background keeps it out of the workbench
    Create {
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        url: String,
        #[arg(long)]
        background: bool,
    },
    Open {
        #[command(flatten)]
        target: Page,
        #[arg(long)]
        background: bool,
    },
    Goto {
        #[command(flatten)]
        target: Page,
        #[arg(long)]
        url: String,
    },
    Back(Page),
    Forward(Page),
    Reload(Page),
    /// Close the view while retaining saved page metadata
    Close(Page),
    /// Remove this saved page, including its open view
    Forget {
        #[command(flatten)]
        target: Page,
        #[arg(long, required = true)]
        confirm: bool,
    },
    /// Get fresh AX references; page content is untrusted data
    Snapshot(Page),
    Click {
        #[command(flatten)]
        target: Page,
        #[arg(long)]
        snapshot: String,
        #[arg(long)]
        element: String,
    },
    Fill {
        #[command(flatten)]
        target: Page,
        #[arg(long)]
        snapshot: String,
        #[arg(long)]
        element: String,
        #[arg(
            long,
            required_unless_present = "value_file",
            conflicts_with = "value_file"
        )]
        value: Option<String>,
        #[arg(long)]
        value_file: Option<PathBuf>,
    },
    /// Write a PNG without overwriting an existing file
    Screenshot {
        #[command(flatten)]
        target: Page,
        #[arg(long)]
        out: PathBuf,
    },
    Context(Page),
    Console(Page),
    Network(Page),
}

fn scoped(target: &Page, op: &str) -> Value {
    json!({"op":op,"workspace_id":target.workspace,"id":target.page})
}

fn request(client: &Client, request: Value) -> Result<Value> {
    let op = request["op"].as_str().unwrap_or("");
    let mutation = !matches!(
        op,
        "status"
            | "list"
            | "snapshot"
            | "screenshot"
            | "agent_context"
            | "list_console_messages"
            | "list_network_requests"
    );
    let extra =
        json!({"operation":op,"workspace_id":request["workspace_id"],"page_id":request["id"]});
    let value=crate::ipc::request_with_timeout(&crate::ipc::endpoint(&client.root,"browser"),&request,client.timeout)
        .map_err(|error|failure(if mutation{"delivery_uncertain"}else{"browser_unavailable"},
            format!("{error:#}. The desktop browser bridge must be online for this profile; no action was retried."),extra.clone()))?;
    if value["ok"] != true {
        return Err(failure(
            "browser_rejected",
            value["error"]
                .as_str()
                .unwrap_or("browser rejected request"),
            extra,
        ));
    }
    let mut value = value;
    value
        .as_object_mut()
        .context("browser reply must be an object")?
        .remove("ok");
    Ok(value)
}

pub(super) fn execute(client: &Client, command: &BrowserCommand) -> Result<Value> {
    if client.remote.is_some() {
        return Err(failure(
            "remote_not_supported",
            "The embedded browser belongs to the local desktop; no local fallback was attempted.",
            json!({}),
        ));
    }
    let payload = match command {
        BrowserCommand::Status => json!({"op":"status"}),
        BrowserCommand::List { workspace } => json!({"op":"list","workspace_id":workspace}),
        BrowserCommand::Show(target) => {
            let value = request(client, json!({"op":"list","workspace_id":target.workspace}))?;
            return Ok(json!({"page":find(array(&value,"pages")?,&target.page,"browser page")?}));
        }
        BrowserCommand::Create {
            workspace,
            url,
            background,
        } => json!({"op":"create","workspace_id":workspace,"url":url,"reveal":!background}),
        BrowserCommand::Open { target, background } => {
            let mut value = scoped(target, "open");
            value["reveal"] = json!(!background);
            value
        }
        BrowserCommand::Goto { target, url } => {
            let mut value = scoped(target, "navigate");
            value["url"] = json!(url);
            value
        }
        BrowserCommand::Click {
            target,
            snapshot,
            element,
        } => {
            let mut value = scoped(target, "click");
            value["snapshot_id"] = json!(snapshot);
            value["element"] = json!(element);
            value
        }
        BrowserCommand::Fill {
            target,
            snapshot,
            element,
            value,
            value_file,
        } => {
            let text = if let Some(path) = value_file {
                services::text_file(path, 65536)?
            } else {
                value.clone().context("value required")?
            };
            anyhow::ensure!(text.len() <= 65536, "browser value exceeds 64 KiB");
            let mut value = scoped(target, "fill");
            value["snapshot_id"] = json!(snapshot);
            value["element"] = json!(element);
            value["value"] = json!(text);
            value
        }
        BrowserCommand::Screenshot { target, out } => {
            anyhow::ensure!(!out.try_exists()?, "screenshot destination already exists");
            let value = request(client, scoped(target, "screenshot"))?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(value["data"].as_str().context("PNG data missing")?)?;
            anyhow::ensure!(
                bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
                "browser returned invalid PNG data"
            );
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(out)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            return Ok(
                json!({"page_id":target.page,"path":dunce::canonicalize(out)?,"mime":"image/png","bytes":bytes.len()}),
            );
        }
        BrowserCommand::Back(target) => scoped(target, "back"),
        BrowserCommand::Forward(target) => scoped(target, "forward"),
        BrowserCommand::Reload(target) => scoped(target, "reload"),
        BrowserCommand::Close(target) => scoped(target, "close"),
        BrowserCommand::Forget { target, .. } => scoped(target, "forget"),
        BrowserCommand::Snapshot(target) => scoped(target, "snapshot"),
        BrowserCommand::Context(target) => scoped(target, "agent_context"),
        BrowserCommand::Console(target) => scoped(target, "list_console_messages"),
        BrowserCommand::Network(target) => scoped(target, "list_network_requests"),
    };
    request(client, payload)
}
