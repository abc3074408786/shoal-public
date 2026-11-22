use super::*;
use super::tests::mock;

fn search(client: &Client, args: &[&str]) -> Result<Value> {
    let cli=Cli::try_parse_from(["velella-cli","terminal","search"].into_iter().chain(args.iter().copied()))?;
    execute(client,&cli.args.command)
}
fn page(from: u64, total: u64, bytes: &[u8]) -> Value {
    json!({"ok":true,"from":from,"total":total,"data":base64::engine::general_purpose::STANDARD.encode(bytes)})
}
fn sessions(ids: &[&str]) -> Value {
    json!({"ok":true,"sessions":ids.iter().map(|id|json!({"id":id,"native_chat":false})).collect::<Vec<_>>()})
}

#[test]
fn search_matches_utf8_across_pages_and_ignores_display_geometry() {
    let data="ab鱼群!".as_bytes();
    let mut first=page(0,9,&data[..3]); first["geometry_error"]=json!("display geometry is not used for raw recording search");
    let (client,worker,root)=mock(vec![sessions(&["s1"]),first,page(3,9,&data[3..7]),page(7,9,&data[7..])]);
    let result=search(&client,&["鱼群","--workspace","w"]).unwrap();
    assert_eq!(result["matches"][0]["byte_start"],2);
    assert_eq!(result["matches"][0]["byte_end"],8);
    assert_eq!(result["complete"],true);
    assert_eq!(result["native_transcripts_indexed"],false);
    let requests=worker.join().unwrap();
    assert_eq!(requests.iter().skip(1).map(|row|row["since"].as_u64().unwrap()).collect::<Vec<_>>(),[0,3,7]);
    assert!(requests.iter().all(|row|row["cmd"]=="workbench.sessions"||row["cmd"]=="workbench.terminal.output"));
    assert!(requests.iter().all(|row|row["workspace_id"]=="w"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn search_continuation_preserves_boundary_and_overlapping_matches() {
    for (cursor,bytes,expected,next,complete) in [(0,&b"xxababa!"[..],None,2,false),(2,&b"ababa!"[..],Some(2),4,false),(4,&b"aba!"[..],Some(4),8,true)] {
        let (client,worker,root)=mock(vec![sessions(&["s1"]),page(cursor,8,bytes)]);
        let cursor_text=cursor.to_string();
        let result=search(&client,&["aba","--workspace","w","--terminal","s1","--cursor",&cursor_text,"--snapshot-end","8","--max-bytes","4"]).unwrap();
        assert_eq!(result["sessions"][0]["next_cursor"],next);
        assert_eq!(result["complete"],complete);
        match expected {Some(at)=>assert_eq!(result["matches"][0]["byte_start"],at),None=>assert!(result["matches"].as_array().unwrap().is_empty())}
        worker.join().unwrap(); std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn match_limit_continuation_neither_duplicates_nor_loses_overlaps() {
    for cursor in 0..3 {
        let cursor_text=cursor.to_string();
        let (client,worker,root)=mock(vec![sessions(&["s1"]),page(cursor,4,&b"aaaa"[cursor as usize..])]);
        let result=search(&client,&["aa","--workspace","w","--terminal","s1","--cursor",&cursor_text,"--max-matches","1"]).unwrap();
        assert_eq!(result["matches"].as_array().unwrap().len(),1);
        assert_eq!(result["matches"][0]["byte_start"],cursor);
        assert_eq!(result["sessions"][0]["next_cursor"],if cursor==2 {4}else{cursor+1});
        assert_eq!(result["complete"],cursor==2);
        worker.join().unwrap(); std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn search_uses_fixed_end_despite_append_and_prefers_observed_native_identity() {
    let row=json!({"ok":true,"sessions":[{"id":"s1","native_id":"persisted","status":{"native_id":"observed"}}]});
    let (client,worker,root)=mock(vec![row,page(0,5,b"ma"),page(2,20,b"tch and extra")]);
    let result=search(&client,&["match","--workspace","w"]).unwrap();
    assert_eq!(result["sessions"][0]["snapshot_end"],5);
    assert_eq!(result["sessions"][0]["bytes_examined"],5);
    assert_eq!(result["sessions"][0]["native_id"],"observed");
    assert_eq!(result["sessions"][0]["native_id_source"],"host_status");
    assert_eq!(result["matches"].as_array().unwrap().len(),1);
    assert_eq!(result["complete"],true);
    worker.join().unwrap(); std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn search_rejects_foreign_session_before_reading_any_output() {
    let (client,worker,root)=mock(vec![sessions(&["owned"])]);
    assert!(search(&client,&["query","--workspace","w","--terminal","foreign"]).is_err());
    let requests=worker.join().unwrap();
    assert_eq!(requests.len(),1); assert_eq!(requests[0]["cmd"],"workbench.sessions");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn search_discards_failed_snapshot_hits_and_continues_other_sessions() {
    let (client,worker,root)=mock(vec![sessions(&["s1","s2"]),page(0,10,b"find"),page(2,2,b""),page(0,4,b"find")]);
    let result=search(&client,&["find","--workspace","w"]).unwrap();
    assert_eq!(result["complete"],false);
    assert_eq!(result["matches"].as_array().unwrap().len(),1);
    assert_eq!(result["matches"][0]["session_id"],"s2");
    assert_eq!(result["sessions"][0]["range_complete"],false);
    assert!(result["sessions"][0]["error"].as_str().unwrap().contains("changed"));
    assert_eq!(result["sessions"][0]["next_cursor"],0);
    assert_eq!(result["sessions"][1]["range_complete"],true);
    worker.join().unwrap(); std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn search_reports_omitted_sessions_and_unavailable_recordings() {
    let (client,worker,root)=mock(vec![sessions(&["s2","s1"]),page(0,0,b"")]);
    let result=search(&client,&["query","--workspace","w","--max-sessions","1"]).unwrap();
    assert_eq!(result["sessions"][0]["session_id"],"s1");
    assert_eq!(result["omitted_sessions"],1); assert_eq!(result["stop_reason"],"session_limit");
    assert_eq!(result["complete"],false);
    worker.join().unwrap(); std::fs::remove_dir_all(root).unwrap();
    let (client,worker,root)=mock(vec![sessions(&["s1"]),json!({"ok":false,"error":"recording unavailable"})]);
    let result=search(&client,&["query","--workspace","w"]).unwrap();
    assert_eq!(result["complete"],false); assert_eq!(result["sessions"][0]["snapshot_end"],Value::Null);
    assert_eq!(result["sessions"][0]["has_more"],Value::Null);
    assert!(result["sessions"][0]["error"].as_str().unwrap().contains("unavailable"));
    worker.join().unwrap(); std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn search_validates_ranges_and_bounds_before_contacting_runtime() {
    let (client,worker,root)=mock(vec![]);
    for args in [vec!["","--workspace","w"],vec!["long","--workspace","w","--max-bytes","1"],
        vec!["x","--workspace","w","--cursor","1"],vec!["x","--workspace","w","--terminal","s","--cursor","2","--snapshot-end","1"],
        vec!["x","--workspace","w","--max-sessions","0"]] { assert!(search(&client,&args).is_err()); }
    assert!(worker.join().unwrap().is_empty()); std::fs::remove_dir_all(root).unwrap();
}
