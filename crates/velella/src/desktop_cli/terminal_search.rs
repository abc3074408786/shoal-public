//! Bounded, literal UTF-8 byte search through workspace-owned PTY recordings.
//! Reads only existing IPC APIs; never infers a native conversation transcript.
use super::*;
use std::time::Instant;

#[derive(ClapArgs)]
pub(super) struct SearchOptions {
    /// Case-sensitive literal text (up to 4096 UTF-8 bytes); ANSI bytes remain
    query: String,
    #[arg(long)]
    workspace: String,
    /// Restrict to an exact session ID owned by this workspace
    #[arg(long)]
    terminal: Option<String>,
    /// Byte cursor from a previous search; requires an exact terminal
    #[arg(long, requires = "terminal")]
    cursor: Option<u64>,
    /// Reuse the prior search's fixed end when continuing the same recording
    #[arg(long, requires = "terminal")]
    snapshot_end: Option<u64>,
    #[arg(long, default_value_t = 1048576, value_parser = clap::value_parser!(u32).range(1..=16777216))]
    max_bytes: u32,
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=200))]
    max_sessions: u32,
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=500))]
    max_matches: u32,
    #[arg(long, default_value_t = 30000, value_parser = clap::value_parser!(u64).range(1..=60000))]
    deadline_ms: u64,
}

fn timed(client: &Client, deadline: Instant) -> Result<Client> {
    let remaining = deadline.checked_duration_since(Instant::now()).context("search deadline exceeded")?;
    Ok(Client {root:client.root.clone(),remote:client.remote.clone(),endpoint:client.endpoint.clone(),timeout:client.timeout.min(remaining)})
}

fn within_deadline(deadline: Instant) -> Result<()> {
    anyhow::ensure!(Instant::now() < deadline,"search deadline exceeded during matching"); Ok(())
}

fn matching_offsets(buffer: &[u8], needle: &[u8], limit: usize, deadline: Instant) -> Result<Vec<usize>> {
    within_deadline(deadline)?;
    let mut offsets=Vec::new();
    if buffer.len() >= needle.len() {
        for offset in 0..=buffer.len()-needle.len() {
            if offset % 256 == 0 { within_deadline(deadline)?; }
            if &buffer[offset..offset+needle.len()] == needle {
                offsets.push(offset);
                if offsets.len() >= limit {break;}
            }
        }
    }
    within_deadline(deadline)?;
    Ok(offsets)
}

pub(super) fn execute(client: &Client, options: &SearchOptions) -> Result<Value> {
    let needle = options.query.as_bytes();
    anyhow::ensure!(!needle.is_empty() && needle.len() <= 4096,"query must contain 1 to 4096 UTF-8 bytes");
    anyhow::ensure!(options.max_bytes as usize >= needle.len(),"--max-bytes must be at least the query length");
    let start = options.cursor.unwrap_or(0);
    anyhow::ensure!(options.snapshot_end.is_none_or(|end|end >= start),"--snapshot-end precedes --cursor");
    let deadline = Instant::now()+Duration::from_millis(options.deadline_ms);
    let mut rows = timed(client,deadline)?.sessions(&options.workspace)?;
    // Resolve ownership before issuing any output read, even for an exact ID.
    if let Some(id) = &options.terminal { rows = vec![find(&rows,id,"terminal")?]; }
    rows.sort_by_key(|row|row["id"].as_str().unwrap_or_default().to_owned());
    within_deadline(deadline)?;
    let total = rows.len();
    let (mut reports, mut matches) = (Vec::new(),Vec::new());
    let mut stopped = "end";
    for row in rows.iter().take(options.max_sessions as usize) {
        if matches.len() >= options.max_matches as usize { stopped="match_limit"; break; }
        if Instant::now() >= deadline { stopped="deadline"; break; }
        let (report, hits) = scan(client,options,row,deadline,options.max_matches as usize-matches.len());
        reports.push(report); matches.extend(hits);
    }
    let omitted = total-reports.len();
    if omitted > 0 && stopped == "end" { stopped="session_limit"; }
    let complete = omitted == 0 && reports.iter().all(|row|row["range_complete"] == true);
    if !complete && stopped == "end" {stopped="session_results_incomplete";}
    Ok(json!({"workspace_id":options.workspace,"query":options.query,"representation":"raw-terminal-recording",
        "case_sensitive":true,"native_transcripts_indexed":false,"cursor_unit":"bytes",
        "matches":matches,"sessions":reports,"listed_sessions":total,"omitted_sessions":omitted,
        "stop_reason":stopped,"complete":complete,"searched_from_recording_start":start == 0,
        "snapshot_policy":"fixed byte end per session; requires append-stable recordings"}))
}

fn scan(client: &Client, options: &SearchOptions, row: &Value, deadline: Instant, match_limit: usize) -> (Value,Vec<Value>) {
    let start=options.cursor.unwrap_or(0);
    let (mut position,mut scanned_to,mut next)=(start,start,start);
    let mut end=options.snapshot_end;
    let (mut tail,mut hits)=(Vec::new(),Vec::new());
    let needle=options.query.as_bytes();
    let mut stop="end";
    let mut examined=0usize;
    let result=(|| -> Result<()> {
        anyhow::ensure!(row["native_chat"] != true,"legacy native chat has no terminal recording");
        let id=row["id"].as_str().context("session ID missing")?;
        loop {
            let value=timed(client,deadline)?.request(json!({"cmd":"workbench.terminal.output","workspace_id":options.workspace,"session_id":id,"since":position}))?;
            let from=value["from"].as_u64().context("output cursor missing")?;
            let total=value["total"].as_u64().context("output total missing")?;
            let encoded=value["data"].as_str().context("output data missing")?;
            anyhow::ensure!(encoded.len() <= 174764,"output page exceeds 128 KiB");
            let data=base64::engine::general_purpose::STANDARD.decode(encoded)?;
            anyhow::ensure!(data.len() <= 131072 && from.checked_add(data.len() as u64).is_some_and(|until|until <= total),"invalid output page bounds");
            let fixed=*end.get_or_insert(total);
            anyhow::ensure!(from == position && total >= fixed,"terminal recording changed or cursor is beyond its end");
            anyhow::ensure!(position <= fixed,"cursor is beyond snapshot end");
            within_deadline(deadline)?;
            if position == fixed { next=fixed; return Ok(()); }
            let count=data.len().min((fixed-position).min((options.max_bytes as usize-examined) as u64) as usize);
            anyhow::ensure!(count > 0,"terminal recording made no progress before its snapshot end");
            let mut buffer=std::mem::take(&mut tail);
            let base=position-buffer.len() as u64;
            buffer.extend_from_slice(&data[..count]);
            examined+=count; scanned_to=position+count as u64;
            {
                for offset in matching_offsets(&buffer,needle,match_limit-hits.len(),deadline)? {
                    let at=base+offset as u64;
                    let excerpt_start=offset.saturating_sub(64);
                    let excerpt_end=(offset+needle.len()+64).min(buffer.len());
                    hits.push(json!({"workspace_id":options.workspace,"session_id":id,
                        "byte_start":at,"byte_end":at+needle.len() as u64,
                        "excerpt":String::from_utf8_lossy(&buffer[excerpt_start..excerpt_end]),
                        "excerpt_from":base+excerpt_start as u64,"excerpt_is_lossy_utf8":true}));
                    if hits.len() >= match_limit {
                        within_deadline(deadline)?;
                        next=at+1;
                        if fixed.saturating_sub(next) < needle.len() as u64 {next=fixed;} else {stop="match_limit";}
                        return Ok(());
                    }
                }
            }
            within_deadline(deadline)?;
            position=scanned_to;
            if position == fixed { next=fixed; return Ok(()); }
            if examined == options.max_bytes as usize {
                // Rescan fewer than one needle's bytes on continuation. This
                // preserves boundary matches without duplicating earlier hits.
                next=position-(needle.len()-1) as u64; stop="byte_limit"; return Ok(());
            }
            let keep=buffer.len().min(needle.len()-1);
            tail=buffer[buffer.len()-keep..].to_vec();
        }
    })();
    let error=result.err().map(|error|format!("{error:#}"));
    if error.is_some() {stop="unavailable_or_changed";next=start;hits.clear();}
    let (native_id,native_source)=if row["status"]["native_id"].as_str().is_some_and(|id|!id.is_empty()) {
        (row["status"]["native_id"].clone(),"host_status")
    } else if row["native_id"].as_str().is_some_and(|id|!id.is_empty()) {(row["native_id"].clone(),"session_metadata")}
    else {(Value::Null,"unknown")};
    let complete=error.is_none() && end.is_some_and(|end|next == end);
    (json!({"session_id":row["id"],"title":row["title"],"program":row["program"],"task_id":row["task_id"],
        "native_id":native_id,"native_id_source":native_source,"searched_from":start,"scanned_to":scanned_to,
        "bytes_examined":examined,"snapshot_end":end,"next_cursor":next,"has_more":if error.is_some(){Value::Null}else{json!(!complete)},
        "range_complete":complete,"stop_reason":stop,"error":error,"match_count":hits.len()}),hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matching_an_expired_large_page_cannot_report_success() {
        let buffer=vec![b'a';128*1024];
        let mut needle=vec![b'a';4096]; needle[4095]=b'b';
        let error=matching_offsets(&buffer,&needle,100,Instant::now()-Duration::from_millis(1)).unwrap_err();
        assert!(error.to_string().contains("deadline"));
        assert_eq!(matching_offsets(b"aaaa",b"aa",2,Instant::now()+Duration::from_secs(1)).unwrap(),[0,1]);
    }
}
