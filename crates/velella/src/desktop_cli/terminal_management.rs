use super::*;
use crate::workbench::{Layout, SplitNode, Tab};
use std::time::Instant;

fn with_deadline(client: &Client, deadline: Instant) -> Result<Client> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .context("terminal observation deadline exceeded")?;
    Ok(Client {
        root: client.root.clone(),
        remote: client.remote.clone(),
        endpoint: client.endpoint.clone(),
        timeout: client.timeout.min(remaining),
    })
}

pub(super) fn execute(client: &Client, command: &TerminalCommand) -> Result<Value> {
    match command {
        TerminalCommand::Switch { target } => switch(client, target),
        TerminalCommand::Wait {
            target,
            condition,
            contains,
            deadline_ms,
        } => wait(client, target, condition, contains.as_deref(), *deadline_ms),
        TerminalCommand::Screen { target } => screen(client, target),
        TerminalCommand::Rename { target, title } => {
            anyhow::ensure!(
                !title.trim().is_empty() && title.len() <= 160,
                "title must contain 1 to 160 bytes"
            );
            client.session(target)?;
            let mut layout = layout(client, &target.workspace)?;
            let index = terminal_tab(&mut layout, &target.terminal);
            layout.panes[index.0].tabs[index.1].title = title.clone();
            let value = save(client, &target.workspace, &layout)?;
            Ok(
                json!({"workspace_id":target.workspace,"session_id":target.terminal,"tab_title":title,"layout_revision":value["layout"]["revision"]}),
            )
        }
        TerminalCommand::Split {
            target,
            direction,
            launch,
            request_id,
        } => split(client, target, direction, launch, *request_id),
        TerminalCommand::Recover { target, .. } => services::call(
            client,
            json!({"cmd":"workbench.terminal.recover","workspace_id":target.workspace,"session_id":target.terminal,"confirmed":true}),
        ),
        _ => unreachable!(),
    }
}

fn switch(client: &Client, target: &Target) -> Result<Value> {
    anyhow::ensure!(client.remote.is_none(), "terminal switch is local-only");
    let snapshot = client.snapshot()?;
    let workspace = find(array(&snapshot, "workspaces")?, &target.workspace, "workspace")?;
    anyhow::ensure!(workspace["archived"] != true, "target workspace is archived");
    let session = client.session(target)?;
    anyhow::ensure!(session["native_chat"] != true && session["kind"] != "agent", "target is not a native terminal");
    let timeout = client.timeout.min(Duration::from_secs(9));
    let expires = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() + timeout.as_millis();
    let extra = json!({"workspace_id":target.workspace,"session_id":target.terminal});
    let result = crate::ipc::request_with_timeout(&crate::ipc::endpoint(&client.root, "browser"),
        &json!({"op":"terminal_switch","workspace_id":target.workspace,"session_id":target.terminal,"expires_at_ms":expires}), timeout)
        .map_err(|error|failure("desktop_switch_unconfirmed", format!("{error:#}. Open a compatible Velella desktop for this profile; no action was retried. Selection may have changed."), extra.clone()))?;
    if result["ok"] != true {
        return Err(failure(result["code"].as_str().unwrap_or("desktop_switch_rejected"), result["error"].as_str().unwrap_or("desktop did not confirm terminal switch"), extra));
    }
    anyhow::ensure!(result["protocol"] == "velella-terminal-switch-v1" && result["focused"] == true
        && result["workspace_id"] == target.workspace && result["session_id"] == target.terminal,
        "desktop did not confirm the exact terminal focus");
    let mut result = result;
    result.as_object_mut().context("desktop response must be an object")?.remove("ok");
    Ok(result)
}

fn wait(
    client: &Client,
    target: &Target,
    condition: &str,
    contains: Option<&str>,
    deadline_ms: u64,
) -> Result<Value> {
    if condition == "output" {
        anyhow::ensure!(
            contains.is_some_and(|s| !s.is_empty() && s.len() <= 65536),
            "--for output requires nonempty --contains (up to 64 KiB)"
        );
    }
    let deadline = Instant::now() + Duration::from_millis(deadline_ms);
    let mut cursor = 0u64;
    let mut suffix = Vec::new();
    loop {
        if Instant::now() >= deadline {
            return Err(failure(
                "wait_timeout",
                "Terminal condition was not observed before the deadline; no input was sent.",
                json!({"workspace_id":target.workspace,"session_id":target.terminal,"condition":condition,"next_cursor":cursor}),
            ));
        }
        let timed = with_deadline(client, deadline)?;
        if condition == "output" {
            let value=timed.request(json!({"cmd":"workbench.terminal.output","workspace_id":target.workspace,"session_id":target.terminal,"since":cursor}))?;
            let data = base64::engine::general_purpose::STANDARD
                .decode(value["data"].as_str().context("output data missing")?)?;
            let from = value["from"].as_u64().context("output cursor missing")?;
            if from != cursor {
                suffix.clear();
            }
            cursor = from + data.len() as u64;
            suffix.extend_from_slice(&data);
            let needle = contains.unwrap().as_bytes();
            if suffix.windows(needle.len()).any(|part| part == needle) {
                return Ok(
                    json!({"condition":"output","matched":true,"next_cursor":cursor,"workspace_id":target.workspace,"session_id":target.terminal}),
                );
            }
            let keep = needle.len().saturating_sub(1);
            if suffix.len() > keep {
                suffix.drain(..suffix.len() - keep);
            }
            if value["total"].as_u64().is_some_and(|total| cursor < total) && !data.is_empty() {
                continue;
            }
        } else {
            let session = timed.session(target)?;
            let status = &session["status"];
            let exited = status["exit"].as_i64().is_some();
            let idle = status["exit"].is_null()
                && status["state"] == "waiting_input"
                && status["permission_input"] == false;
            if (condition == "exit" && exited) || (condition == "idle" && idle) {
                return Ok(
                    json!({"condition":condition,"matched":true,"status":status,"workspace_id":target.workspace,"session_id":target.terminal}),
                );
            }
        }
        std::thread::sleep(
            Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

/// Replay every geometry event at its byte offset, including native-host resets.
fn apply_page(
    parser: &mut Option<vt100::Parser>,
    sequence: &mut Option<u64>,
    complete: &mut bool,
    value: &Value,
    end: u64,
) -> Result<u64> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value["data"].as_str().context("output data missing")?)?;
    let from = value["from"].as_u64().context("output cursor missing")?;
    let length = bytes.len().min(end.saturating_sub(from) as usize);
    let bytes = &bytes[..length];
    let mut processed = 0;
    if let Some(events) = value["geometry"].as_array() {
        for event in events {
            let seq = event["sequence"]
                .as_u64()
                .context("geometry sequence missing")?;
            if sequence.is_some_and(|previous| seq <= previous) {
                continue;
            }
            let offset = event["offset"]
                .as_u64()
                .context("geometry offset missing")?;
            if offset > from + length as u64 {
                break;
            }
            let until = (offset.saturating_sub(from) as usize).min(length);
            anyhow::ensure!(until >= processed, "geometry offsets are unordered");
            if let Some(parser) = parser {
                parser.process(&bytes[processed..until]);
            }
            processed = until;
            let rows = event["rows"].as_u64().context("geometry rows missing")?;
            let cols = event["cols"].as_u64().context("geometry columns missing")?;
            anyhow::ensure!(
                (1..=1000).contains(&rows) && (1..=1000).contains(&cols),
                "invalid terminal geometry"
            );
            if event["reset"] == true || parser.is_none() {
                *parser = Some(vt100::Parser::new(rows as u16, cols as u16, 0));
                *complete = event["reset"] == true;
            } else if let Some(parser) = parser {
                parser.set_size(rows as u16, cols as u16);
            }
            *sequence = Some(seq);
        }
    }
    if let Some(parser) = parser {
        parser.process(&bytes[processed..]);
    }
    Ok(from + length as u64)
}

fn screen(client: &Client, target: &Target) -> Result<Value> {
    let deadline = Instant::now() + client.timeout;
    let mut cursor = 0u64;
    let mut end = None;
    let mut parser = None;
    let mut sequence = None;
    let mut complete = false;
    loop {
        let timed = with_deadline(client, deadline)?;
        let value=timed.request(json!({"cmd":"workbench.terminal.output","workspace_id":target.workspace,"session_id":target.terminal,"since":cursor}))?;
        anyhow::ensure!(
            value["geometry_error"].is_null(),
            "terminal geometry could not be read; a reliable screen cannot be reconstructed"
        );
        let end = *end.get_or_insert(value["total"].as_u64().context("output total missing")?);
        anyhow::ensure!(
            end <= 16 * 1024 * 1024,
            "screen replay exceeds 16 MiB; use cursor-based terminal read for this session"
        );
        let next = apply_page(&mut parser, &mut sequence, &mut complete, &value, end)?;
        anyhow::ensure!(
            next >= end || next > cursor,
            "terminal recording changed while replaying; inspect output before retrying"
        );
        cursor = next;
        if cursor >= end {
            break;
        }
    }
    let parser = parser
        .context("terminal geometry is unavailable; screen was not inferred from an output tail")?;
    let (rows, cols) = parser.screen().size();
    Ok(
        json!({"workspace_id":target.workspace,"session_id":target.terminal,"screen":parser.screen().contents(),"rows":rows,"cols":cols,"next_cursor":cursor,"complete_history":complete,"representation":"vt100-replay","geometry_sequence":sequence}),
    )
}

fn layout(client: &Client, workspace: &str) -> Result<Layout> {
    let layout: Layout = serde_json::from_value(
        client.request(json!({"cmd":"workbench.layout","workspace_id":workspace}))?["layout"]
            .clone(),
    )
    .context("invalid runtime layout")?;
    anyhow::ensure!(!layout.panes.is_empty(), "runtime layout has no panes");
    Ok(layout)
}
fn save(client: &Client, workspace: &str, layout: &Layout) -> Result<Value> {
    services::call(
        client,
        json!({"cmd":"workbench.layout.save","workspace_id":workspace,"layout":layout}),
    )
}
fn has_session(tree: &SplitNode, id: &str) -> bool {
    match tree {
        SplitNode::Leaf { id: leaf } => leaf == id,
        SplitNode::Split { first, second, .. } => has_session(first, id) || has_session(second, id),
    }
}
fn terminal_tab(layout: &mut Layout, session: &str) -> (usize, usize) {
    for (pane_index, pane) in layout.panes.iter().enumerate() {
        for (index, tab) in pane.tabs.iter().enumerate() {
            if tab.kind == "terminal"
                && (tab.target == session
                    || tab
                        .terminal_tree
                        .as_ref()
                        .is_some_and(|tree| has_session(tree, session)))
            {
                return (pane_index, index);
            }
        }
    }
    let pane = &mut layout.panes[0];
    let id = format!("terminal:{session}");
    pane.tabs.push(Tab {
        id: id.clone(),
        kind: "terminal".into(),
        target: session.into(),
        title: "Terminal".into(),
        terminal_tree: None,
    });
    pane.active = Some(id);
    (0, pane.tabs.len() - 1)
}
fn insert_split(tree: &mut SplitNode, target: &str, new: &str, axis: &str, node_id: &str) -> bool {
    match tree {
        SplitNode::Leaf { id } if id == target => {
            *tree = SplitNode::Split {
                id: node_id.into(),
                axis: axis.into(),
                ratio: 0.5,
                first: Box::new(SplitNode::Leaf { id: target.into() }),
                second: Box::new(SplitNode::Leaf { id: new.into() }),
            };
            true
        }
        SplitNode::Split { first, second, .. } => {
            insert_split(first, target, new, axis, node_id)
                || insert_split(second, target, new, axis, node_id)
        }
        _ => false,
    }
}

fn split(
    client: &Client,
    target: &Target,
    direction: &str,
    launch: &str,
    request_id: Option<uuid::Uuid>,
) -> Result<Value> {
    client.session(target)?;
    let mut layout = layout(client, &target.workspace)?;
    let index = terminal_tab(&mut layout, &target.terminal);
    let id = request_id.unwrap_or_else(uuid::Uuid::now_v7);
    let session = id.to_string();
    anyhow::ensure!(
        session != target.terminal,
        "split request UUID must differ from the target session"
    );
    let attached = layout.panes.iter().flat_map(|pane| &pane.tabs).any(|tab| {
        tab.target == session
            || tab
                .terminal_tree
                .as_ref()
                .is_some_and(|tree| has_session(tree, &session))
    });
    let tab = &layout.panes[index.0].tabs[index.1];
    let mut tree = tab
        .terminal_tree
        .clone()
        .unwrap_or_else(|| SplitNode::Leaf {
            id: tab.target.clone(),
        });
    anyhow::ensure!(
        has_session(&tree, &target.terminal),
        "target is missing from split tree; reload the layout before creating a session"
    );
    if !attached {
        anyhow::ensure!(
            insert_split(
                &mut tree,
                &target.terminal,
                &session,
                direction,
                &format!("split:{id}")
            ),
            "target is missing from split tree"
        );
    }
    let opened = terminal(
        client,
        &TerminalCommand::Create {
            workspace: target.workspace.clone(),
            launch: launch.into(),
            request_id: Some(id),
            track_submission: false,
            hybrid_host: None,
        },
    )?;
    if opened["session_id"] != session {
        return Err(failure("split_identity_unconfirmed","Created session did not match the requested UUID; inspect both identities before any further action.",json!({"request_id":id,"workspace_id":target.workspace,"returned_session_id":opened["session_id"]})));
    }
    if attached {
        return Ok(
            json!({"workspace_id":target.workspace,"session_id":session,"request_id":id,"already_attached":true}),
        );
    }
    let tab = &mut layout.panes[index.0].tabs[index.1];
    tab.terminal_tree = Some(tree);
    let saved=save(client,&target.workspace,&layout).map_err(|error|failure("split_layout_unconfirmed",format!("{error:#}. The new session was retained; inspect it and reload the layout. Reuse the original UUID to recover, never create a replacement blindly."),json!({"request_id":id,"workspace_id":target.workspace,"session_id":session,"session_created":true})))?;
    Ok(
        json!({"workspace_id":target.workspace,"session_id":session,"request_id":id,"layout_revision":saved["layout"]["revision"]}),
    )
}
