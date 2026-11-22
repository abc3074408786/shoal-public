use super::*;

fn setup()->(PathBuf,Workbench){
    let root=std::env::temp_dir().join(format!("velella-split-test-{}",uuid::Uuid::now_v7()));
    std::fs::create_dir(&root).unwrap();
    let store=Workbench::open(&root).unwrap();
    for wid in ["w","other"] {
        store.put_workspace(&Workspace{id:wid.into(),project_id:"p".into(),name:wid.into(),path:root.clone(),kind:"folder".into(),archived:false,favorite:false,position:0,parent_workspace_id:None}).unwrap();
    }
    for (sid,wid) in [("s1","w"),("s2","w"),("s3","w"),("foreign","other")] {
        store.put_session(&Session{id:sid.into(),workspace_id:wid.into(),title:sid.into(),kind:"terminal".into(),task_id:None,native_id:None,descriptor:Descriptor{id:sid.into(),endpoint:"unused".into(),directory:root.join(sid),spawn:crate::sessions::SpawnRequest{program:"fixture".into(),args:vec![],cwd:None,env:Default::default(),adapter:None,sandbox:Default::default()}}}).unwrap();
    }
    (root,store)
}
fn leaf(id:&str)->Value{json!({"kind":"leaf","id":id})}
fn split(id:&str,first:Value,second:Value)->Value{json!({"kind":"split","id":id,"axis":"horizontal","ratio":0.6,"first":first,"second":second})}
fn fixture()->Value{
    json!({"revision":0,"axis":"horizontal","ratio":0.5,"closed":[],"panes":[
        {"id":"main","active":"t","tabs":[{"id":"t","kind":"terminal","target":"s1","title":"Shell","terminalTree":split("inner",leaf("s1"),split("nested",leaf("s2"),leaf("s3")))}]},
        {"id":"files","active":null,"tabs":[]},{"id":"browser","active":null,"tabs":[]}
    ],"tree":split("outer",leaf("main"),split("side",leaf("files"),leaf("browser")))})
}
#[test]
fn nested_split_roundtrip_and_legacy_layouts(){
    let (root,mut store)=setup();
    let old:Layout=serde_json::from_value(json!({"revision":0,"axis":"vertical","ratio":0.35,"closed":[],"panes":[{"id":"main","active":null,"tabs":[]}]})).unwrap();
    assert!(old.tree.is_none());
    let saved=store.save_layout("w",serde_json::from_value(fixture()).unwrap()).unwrap();
    assert_eq!(saved.revision,1);
    let restored=serde_json::to_value(store.layout("w").unwrap()).unwrap();
    assert_eq!(restored["tree"],fixture()["tree"]);
    assert_eq!(restored["panes"][0]["tabs"][0]["terminalTree"],fixture()["panes"][0]["tabs"][0]["terminalTree"]);
    assert!(store.save_layout("w",old).is_err(),"stale writers cannot overwrite nested layouts");
    drop(store);std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn invalid_split_trees_do_not_mutate_storage(){
    let (root,mut store)=setup();
    for change in 0..7 {
        let mut value=fixture();
        match change {
            0=>value["tree"]["second"]=leaf("main"),
            1=>value["tree"]["first"]=leaf("absent"),
            2=>value["tree"]["ratio"]=json!(0.99),
            3=>value["panes"][0]["tabs"][0]["terminalTree"]["second"]=leaf("foreign"),
            4=>value["panes"][0]["tabs"][0]["terminalTree"]["second"]=leaf("s1"),
            5=>value["panes"][0]["tabs"][0]["terminalTree"]=leaf("s2"),
            _=>value["panes"][0]["tabs"][0]["kind"]=json!("files"),
        }
        assert!(store.save_layout("w",serde_json::from_value(value).unwrap()).is_err(),"invalid case {change}");
        assert_eq!(store.layout("w").unwrap().revision,0);
    }
    drop(store);std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn merging_many_file_tabs_preserves_the_workspace_budget(){
    let (root,mut store)=setup();
    let tabs:Vec<Value>=(0..40).map(|i|json!({"id":format!("f{i}"),"kind":"files","target":format!("file-{i}.md"),"title":format!("file {i}")})).collect();
    let two=json!({"revision":0,"axis":"horizontal","ratio":0.5,"closed":[],"panes":[{"id":"a","active":null,"tabs":&tabs[..20]},{"id":"b","active":null,"tabs":&tabs[20..]}],"tree":split("ab",leaf("a"),leaf("b"))});
    store.save_layout("w",serde_json::from_value(two).unwrap()).unwrap();
    let merged=json!({"revision":1,"axis":"horizontal","ratio":0.5,"closed":[],"panes":[{"id":"a","active":"f39","tabs":tabs}],"tree":leaf("a")});
    store.save_layout("w",serde_json::from_value(merged.clone()).unwrap()).unwrap();
    assert_eq!(store.layout("w").unwrap().panes[0].tabs.len(),40);
    let mut excessive=merged;excessive["revision"]=json!(2);
    excessive["panes"][0]["tabs"]=json!((0..513).map(|i|json!({"id":format!("f{i}"),"kind":"files","target":format!("file-{i}.md"),"title":"file"})).collect::<Vec<_>>());
    assert!(store.save_layout("w",serde_json::from_value(excessive).unwrap()).is_err());
    assert_eq!(store.layout("w").unwrap().revision,2);
    drop(store);std::fs::remove_dir_all(root).unwrap();
}
