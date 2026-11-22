use super::*;
use std::{io::{BufRead,BufReader,Read,Write},net::TcpListener,sync::mpsc,time::Duration};

fn fixture(responses:Vec<(u16,Value,&'static str)>)->(String,mpsc::Receiver<String>){
    let server=TcpListener::bind("127.0.0.1:0").unwrap();let address=format!("http://{}",server.local_addr().unwrap());
    let (tx,rx)=mpsc::channel();
    std::thread::spawn(move||for (status,body,headers) in responses{
        let (mut socket,_)=server.accept().unwrap();socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut reader=BufReader::new(socket.try_clone().unwrap());let mut request=String::new();let mut length=0;
        loop{let mut line=String::new();reader.read_line(&mut line).unwrap();if line=="\r\n"{break}if line.to_lowercase().starts_with("content-length:"){length=line.split(':').nth(1).unwrap().trim().parse::<usize>().unwrap()}request.push_str(&line);}
        let mut bytes=vec![0;length];reader.read_exact(&mut bytes).unwrap();request.push_str("\r\n");request.push_str(&String::from_utf8(bytes).unwrap());let _=tx.send(request);
        let body=body.to_string();write!(socket,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",body.len()).unwrap();
    });(address,rx)
}
fn source(provider:&str,url:String)->Source{Source{id:uuid::Uuid::now_v7().to_string(),project_id:"p".into(),provider:provider.into(),name:provider.into(),base_url:url,scope:match provider{"github"=>"owner/repo","gitlab"=>"group/sub/repo","linear"=>"team-uuid",_=>"DEV"}.into(),login:if provider=="jira"{"dev@example.test"}else{""}.into(),jira_kind:"cloud".into(),auth_mode:String::new(),disabled:false}}
fn github()->Value{json!({"number":7,"title":"中文任务","body":"验收条件","html_url":"https://github.com/owner/repo/issues/7","state":"open","updated_at":"2026-10-18"})}
fn gitlab()->Value{json!({"iid":7,"title":"GitLab task","description":"body","web_url":"https://gitlab.test/group/sub/repo/-/issues/7","state":"opened"})}
fn linear()->Value{json!({"id":"issue-uuid","identifier":"DEV-7","title":"Linear task","description":"body","url":"https://linear.app/team/issue/DEV-7","state":{"name":"Done","type":"completed"}})}
fn jira()->Value{json!({"key":"DEV-7","fields":{"summary":"Jira task","description":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"验收"}]}]},"status":{"name":"Done","statusCategory":{"key":"done"}}}})}
fn read(rx:&mpsc::Receiver<String>)->String{rx.recv_timeout(Duration::from_secs(3)).unwrap()}

#[test]fn task_metadata_survives_provider_mapping_and_cache_roundtrip(){
    let mut gh=github();gh["assignees"]=json!([{"login":"owner"}]);gh["user"]=json!({"login":"author"});gh["labels"]=json!([{"name":"bug"},"urgent"]);gh["milestone"]=json!({"title":"v1"});gh["created_at"]=json!("2026-10-01");
    let mut gl=gitlab();gl["assignees"]=json!([{"name":"owner"}]);gl["author"]=json!({"name":"author"});gl["labels"]=json!(["bug","urgent"]);gl["milestone"]=json!({"title":"v1"});gl["created_at"]=json!("2026-10-01");gl["due_date"]=json!("2026-11-01");
    let mut li=linear();li["assignee"]=json!({"name":"owner"});li["creator"]=json!({"name":"author"});li["labels"]=json!({"nodes":[{"name":"bug"},{"name":"urgent"}]});li["project"]=json!({"name":"Product"});li["priorityLabel"]=json!("High");li["cycle"]=json!({"number":8});li["createdAt"]=json!("2026-10-01");li["dueDate"]=json!("2026-11-01");
    let mut ji=jira();let f=&mut ji["fields"];f["assignee"]=json!({"displayName":"owner"});f["reporter"]=json!({"displayName":"author"});f["labels"]=json!(["bug","urgent"]);f["project"]=json!({"key":"DEV","name":"Product"});f["priority"]=json!({"name":"High"});f["fixVersions"]=json!([{"name":"v1"},{"name":"v2"}]);f["created"]=json!("2026-10-01");f["duedate"]=json!("2026-11-01");
    let root=tempfile_for_sources();let store=Store::open(&root).unwrap();
    for (kind,raw) in [("github",gh),("gitlab",gl),("linear",json!({"data":{"issue":li}})),("jira",ji)]{
        let (url,rx)=fixture(vec![(200,raw,"")]);let s=source(kind,url);
        let item=provider::Api::new(&s,"test-secret").unwrap().detail("7").unwrap();let request=read(&rx);
        let m=&item.metadata;assert_eq!(m.assignees,vec!["owner"]);assert_eq!(m.creator,"author");assert_eq!(m.labels,vec!["bug","urgent"]);assert_eq!(m.created_at,"2026-10-01");
        if kind=="linear"||kind=="jira"{assert_eq!(m.priority,"High");assert_eq!(m.project,"Product");assert!(request.contains("assignee"));assert!(request.contains("labels"));}
        else{assert!(m.priority.is_empty());assert_eq!(m.project,s.scope);}
        assert_eq!(m.milestone,match kind{"linear"=>"Cycle 8","jira"=>"v1、v2",_=>"v1"});
        assert_eq!(m.due_at,if kind=="github"{""}else{"2026-11-01"});
        store.cache(&[item.clone()]).unwrap();let document:String=store.conn.query_row("SELECT document FROM items WHERE key=?1",[&item.key],|r|r.get(0)).unwrap();
        let restored:Item=serde_json::from_str(&document).unwrap();assert_eq!(serde_json::to_value(restored.metadata).unwrap(),serde_json::to_value(m).unwrap());
        let mut legacy=serde_json::to_value(item).unwrap();legacy.as_object_mut().unwrap().remove("metadata");let legacy:Item=serde_json::from_value(legacy).unwrap();assert!(legacy.metadata.assignees.is_empty());
    }
    drop(store);std::fs::remove_dir_all(root).unwrap();
}

#[test]fn task_metadata_allows_missing_and_null_optional_provider_fields(){
    for (kind,raw) in [("github",github()),("gitlab",gitlab()),("linear",json!({"data":{"issue":linear()}})),("jira",jira())]{
        let (url,rx)=fixture(vec![(200,raw,"")]);let s=source(kind,url);let item=provider::Api::new(&s,"test-secret").unwrap().detail("7").unwrap();read(&rx);
        assert!(item.metadata.assignees.is_empty());assert!(item.metadata.creator.is_empty());assert!(item.metadata.labels.is_empty());assert!(item.metadata.due_at.is_empty());
    }
}

#[test]fn github_contract_uses_scoped_search_paging_and_bearer(){
    let (url,rx)=fixture(vec![(200,json!({"items":[github()],"total_count":75,"incomplete_results":false}),"")]);let s=source("github",url);
    let page=provider::Api::new(&s,"test-secret").unwrap().list("","open","quote \"x\"").unwrap();assert_eq!(page.items[0].identifier,"#7");assert_eq!(page.next.as_deref(),Some("2"));
    let request=read(&rx);assert!(request.starts_with("GET /api/v3/search/issues?"));assert!(request.contains("repo%3Aowner%2Frepo"));assert!(request.to_lowercase().contains("authorization: bearer test-secret"));assert!(!request.lines().next().unwrap().contains("secret"));
}
#[test]fn gitlab_nested_project_is_one_encoded_segment_and_has_next_page(){
    let (url,rx)=fixture(vec![(200,json!([gitlab()]),"X-Next-Page: 3\r\n")]);let s=source("gitlab",url);
    let page=provider::Api::new(&s,"test-secret").unwrap().list("2","all","title").unwrap();assert_eq!(page.next.as_deref(),Some("3"));
    let request=read(&rx);assert!(request.starts_with("GET /api/v4/projects/group%2Fsub%2Frepo/issues?"));assert!(request.to_lowercase().contains("private-token: test-secret"));assert!(!request.contains("state="));
}
#[test]fn linear_cursor_and_team_filter_are_graphql_variables(){
    let (url,rx)=fixture(vec![(200,json!({"data":{"issues":{"nodes":[linear()],"pageInfo":{"hasNextPage":true,"endCursor":"next-2"}}}}),"")]);let s=source("linear",url);
    let page=provider::Api::new(&s,"test-secret").unwrap().list("cursor-1","closed","输入").unwrap();assert!(page.items[0].closed);assert_eq!(page.next.as_deref(),Some("next-2"));
    let request=read(&rx);let body:Value=serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();assert_eq!(body["variables"]["after"],"cursor-1");assert_eq!(body["variables"]["filter"]["team"]["id"]["eq"],"team-uuid");assert!(request.to_lowercase().contains("authorization: test-secret"));
}
#[test]fn jira_cloud_and_dc_use_their_own_pagination_and_auth(){
    let (url,rx)=fixture(vec![(200,json!({"issues":[jira()],"nextPageToken":"next-token"}),"")]);let s=source("jira",url);
    let page=provider::Api::new(&s,"test-secret").unwrap().list("previous","all","").unwrap();assert!(page.items[0].closed);assert_eq!(page.items[0].description,"验收\n");assert_eq!(page.next.as_deref(),Some("next-token"));let request=read(&rx);assert!(request.starts_with("POST /rest/api/3/search/jql "));assert!(request.to_lowercase().contains("authorization: basic "));assert!(request.contains("\"nextPageToken\":\"previous\""));
    let (url,rx)=fixture(vec![(200,json!({"issues":[jira()],"startAt":50,"total":52}),"")]);let mut s=source("jira",url);s.jira_kind="dc".into();s.login.clear();let page=provider::Api::new(&s,"test-secret").unwrap().list("50","open","").unwrap();assert_eq!(page.next.as_deref(),Some("51"));let request=read(&rx);assert!(request.starts_with("POST /rest/api/2/search "));assert!(request.contains("\"startAt\":50"));assert!(request.to_lowercase().contains("authorization: bearer test-secret"));
}
#[test]fn four_providers_create_once_with_expected_fields(){
    for kind in ["github","gitlab","linear","jira"]{
        let response=match kind{"github"=>github(),"gitlab"=>gitlab(),"linear"=>json!({"data":{"issueCreate":{"success":true,"issue":linear()}}}),_=>json!({"key":"DEV-7"})};
        let (url,rx)=fixture(vec![(201,response,"")]);let s=source(kind,url);
        let item=provider::Api::new(&s,"test-secret").unwrap().create(&json!({"title":"New task","description":"line 1\nline 2","type_id":"10001","extra":{"customfield_1":"required"}})).unwrap();assert!(!item.url.is_empty());
        let request=read(&rx);assert!(request.starts_with("POST "));let body:Value=serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        match kind{"github"=>assert_eq!(body["body"],"line 1\nline 2"),"gitlab"=>assert_eq!(body["description"],"line 1\nline 2"),"linear"=>assert_eq!(body["variables"]["input"]["teamId"],"team-uuid"),_=>{assert_eq!(body["fields"]["project"]["key"],"DEV");assert_eq!(body["fields"]["description"]["content"].as_array().unwrap().len(),2);assert_eq!(body["fields"]["customfield_1"],"required");}}
    }
}
#[test]fn source_store_protects_tokens_and_preserves_links_after_restart_and_disconnect(){
    let root=std::env::temp_dir().join(format!("velella-source-{}",uuid::Uuid::now_v7()));std::fs::create_dir(&root).unwrap();
    let s=source("github","https://api.github.com".into());let secret="private-sample-token";
    {let store=Store::open(&root).unwrap();store.save(&s,secret).unwrap();let stored:String=store.conn.query_row("SELECT secret FROM sources",[],|r|r.get(0)).unwrap();assert!(!stored.contains(secret));assert_eq!(store.source(&s.id).unwrap().1,secret);
    let item=Item{metadata:ItemMetadata::default(),key:format!("{}:7",s.id),source_id:s.id.clone(),project_id:"p".into(),provider:"github".into(),remote_id:"7".into(),identifier:"#7".into(),title:"Task".into(),url:"https://github.com/owner/repo/issues/7".into(),description:"Body".into(),state:"open".into(),closed:false,updated_at:"".into(),scope:"owner/repo".into()};store.cache(&[item.clone()]).unwrap();assert!(store.attach("missing","wrong","other").is_err());store.attach(&item.key,"workspace","p").unwrap();store.attach(&item.key,"workspace","p").unwrap();assert!(store.attach(&item.key,"second","p").is_err());}
    {let store=Store::open(&root).unwrap();assert_eq!(store.links().unwrap().len(),1);store.conn.execute("DELETE FROM sources",[]).unwrap();assert_eq!(store.links().unwrap()[0]["workspace_id"],"workspace");assert!(store.source(&s.id).is_err());}
    std::fs::remove_dir_all(&root).unwrap();
}
#[test]fn repeated_create_request_reuses_receipt_without_posting_again(){
    let root=std::env::temp_dir().join(format!("velella-create-{}",uuid::Uuid::now_v7()));std::fs::create_dir(&root).unwrap();
    let (url,rx)=fixture(vec![(201,github(),"")]);let s=source("github",url);Store::open(&root).unwrap().save(&s,"test-secret").unwrap();
    let req=json!({"cmd":"sources.create","source_id":s.id,"request_id":uuid::Uuid::now_v7().to_string(),"fields":{"title":"Task"}});
    let first=dispatch(&root,&req).unwrap();let second=dispatch(&root,&req).unwrap();assert_eq!(first,second);assert!(first["ok"]==true);read(&rx);
    let mut changed=req.clone();changed["fields"]["title"]=json!("Changed");assert!(dispatch(&root,&changed).is_err());std::fs::remove_dir_all(&root).unwrap();
}
#[test]fn rate_limit_and_graphql_errors_are_not_empty_successes(){
    let (url,_)=fixture(vec![(403,json!({"message":"test-secret"}),"X-RateLimit-Remaining: 0\r\n")]);let s=source("github",url);let error=provider::Api::new(&s,"test-secret").unwrap().list("","open","").err().unwrap().to_string();assert!(error.contains("限流"));assert!(!error.contains("test-secret"));
    let (url,_)=fixture(vec![(200,json!({"data":null,"errors":[{"message":"not permitted"}]}),"")]);let s=source("linear",url);assert!(provider::Api::new(&s,"test-secret").unwrap().list("","open","").is_err());
}
#[test]fn sources_reject_credential_urls_and_wrong_scope(){
    let mut s=source("github","https://user:secret@github.com".into());assert!(normalize(s.clone()).is_err());s.base_url="https://github.com".into();s.scope="owner/../repo".into();assert!(normalize(s).is_err());
    let mut s=source("linear","https://other.example".into());assert!(normalize(s.clone()).is_err());s.base_url="https://api.linear.app".into();assert!(normalize(s).is_ok());
}

#[test]fn jira_dc_creation_metadata_uses_documented_v2_paths_and_pagination(){
    let (url,rx)=fixture(vec![(200,json!({"values":[{"id":"10001","name":"Task","subtask":false}],"total":1,"isLast":true}),""),(200,json!({"values":[{"fieldId":"summary","required":true},{"fieldId":"customfield_1","name":"Labels","required":true,"schema":{"type":"array","items":"string"},"allowedValues":["red","blue"]}],"total":2,"isLast":true}),"")]);
    let mut s=source("jira",url);s.jira_kind="dc".into();let api=provider::Api::new(&s,"test-secret").unwrap();
    assert_eq!(api.create_options("").unwrap()["types"][0]["id"],"10001");
    let options=api.create_options("10001").unwrap();assert_eq!(options["fields"].as_array().unwrap().len(),1);assert_eq!(options["fields"][0]["allowedValues"][1],"blue");
    assert!(read(&rx).starts_with("GET /rest/api/2/issue/createmeta/DEV/issuetypes?"));assert!(read(&rx).starts_with("GET /rest/api/2/issue/createmeta/DEV/issuetypes/10001?"));
}

#[test]fn account_sources_filter_multiple_teams_and_projects_without_development_binding(){
    let (url,rx)=fixture(vec![(200,json!({"data":{"issues":{"nodes":[linear()],"pageInfo":{"hasNextPage":false}}}}),"")]);let mut s=source("linear",url);s.scope.clear();s.project_id.clear();
    let page=provider::Api::new(&s,"test-secret").unwrap().list_scoped("","open","",&["team-a".into(),"team-b".into()]).unwrap();assert!(page.items[0].project_id.is_empty());let request=read(&rx);assert!(request.contains("\"in\":[\"team-a\",\"team-b\"]"));
    let (url,rx)=fixture(vec![(200,json!({"issues":[jira()],"isLast":true}),"")]);let mut s=source("jira",url);s.scope.clear();s.project_id.clear();let page=provider::Api::new(&s,"test-secret").unwrap().list_scoped("","all","",&["DEV".into(),"OPS".into()]).unwrap();assert_eq!(page.items[0].scope,"DEV");let request=read(&rx);assert!(request.contains("project in"));assert!(request.contains("OPS"));
}

#[test]fn provider_state_changes_and_comments_use_provider_contracts(){
    for kind in ["github","gitlab","linear","jira"]{
        let result=if kind=="linear"{json!({"data":{"issueUpdate":{"success":true},"commentCreate":{"success":true}}})}else{json!({})};
        let (url,rx)=fixture(vec![(200,result.clone(),""),(200,result,"")]);let s=source(kind,url);let api=provider::Api::new(&s,"test-secret").unwrap();
        api.mutate("7",&json!({"action":"state","state":if kind=="linear"||kind=="jira"{"state-id"}else{"closed"}})).unwrap();
        api.mutate("7",&json!({"action":"comment","body":"完成验证"})).unwrap();
        let state=read(&rx);let comment=read(&rx);
        match kind{"github"=>{assert!(state.starts_with("PATCH "));assert!(comment.contains("/issues/7/comments"));},"gitlab"=>{assert!(state.contains("state_event"));assert!(comment.contains("/issues/7/notes"));},"linear"=>{assert!(state.contains("stateId"));assert!(comment.contains("issueId"));},_=>{assert!(state.contains("/issue/7/transitions"));assert!(comment.contains("\"version\":1"));}}
    }
}

#[test]fn source_mutation_receipt_survives_repeated_request_without_a_second_write(){
    let root=tempfile_for_sources();let (url,rx)=fixture(vec![(200,json!({}),"")]);let mut s=source("github",url);s.project_id.clear();let store=Store::open(&root).unwrap();store.save(&s,"test-secret").unwrap();
    let item=Item{metadata:ItemMetadata::default(),key:format!("{}:7",s.id),source_id:s.id.clone(),project_id:String::new(),provider:"github".into(),remote_id:"7".into(),identifier:"#7".into(),title:"Task".into(),url:"https://github.com/owner/repo/issues/7".into(),description:String::new(),state:"open".into(),closed:false,updated_at:String::new(),scope:s.scope.clone()};
    store.cache(&[item.clone()]).unwrap();store.attach(&item.key,"workspace","chosen-project").unwrap();assert_eq!(store.links().unwrap()[0]["project_id"],"chosen-project");assert!(store.linked_workspace(&item.key,"other-project").is_err());
    let req=json!({"cmd":"sources.mutate","source_id":s.id,"key":item.key,"request_id":uuid::Uuid::now_v7().to_string(),"fields":{"action":"comment","body":"Once"}});
    let result=dispatch(&root,&req).unwrap();assert_eq!(result["ok"],true);assert_eq!(dispatch(&root,&req).unwrap(),result);read(&rx);
    let mut different=req;different["fields"]["body"]=json!("Other");assert!(dispatch(&root,&different).is_err());drop(store);std::fs::remove_dir_all(root).unwrap();
}
fn tempfile_for_sources()->std::path::PathBuf{let root=std::env::temp_dir().join(format!("velella-flow-{}",uuid::Uuid::now_v7()));std::fs::create_dir(&root).unwrap();root}

#[test]fn reconnect_reuses_source_identity_and_existing_workspace_link(){
    let root=tempfile_for_sources();let (url,rx)=fixture(vec![(200,json!({"items":[github()],"total_count":1}),"")]);let s=source("github",url);let store=Store::open(&root).unwrap();store.save(&s,"first-secret").unwrap();
    let item=Item{metadata:ItemMetadata::default(),key:format!("{}:7",s.id),source_id:s.id.clone(),project_id:s.project_id.clone(),provider:"github".into(),remote_id:"7".into(),identifier:"#7".into(),title:"Task".into(),url:"https://github.com/owner/repo/issues/7".into(),description:String::new(),state:"open".into(),closed:false,updated_at:String::new(),scope:s.scope.clone()};
    store.cache(&[item.clone()]).unwrap();store.attach(&item.key,"workspace","p").unwrap();dispatch(&root,&json!({"cmd":"sources.remove","source_id":s.id})).unwrap();assert!(store.sources().unwrap().is_empty());assert!(store.source(&s.id).is_err());
    let protected:String=store.conn.query_row("SELECT secret FROM sources WHERE id=?1",[&s.id],|r|r.get(0)).unwrap();assert_eq!(credentials::unprotect(&protected).unwrap(),"");
    let mut reconnect=serde_json::to_value(&s).unwrap();reconnect["id"]=json!("");let result=dispatch(&root,&json!({"cmd":"sources.save","source":reconnect,"token":"new-secret"})).unwrap();assert_eq!(result["source"]["id"],s.id);assert_eq!(store.linked_workspace(&item.key,"p").unwrap(),Some("workspace".into()));read(&rx);drop(store);std::fs::remove_dir_all(root).unwrap();
}

#[test]#[ignore="read-only live public API check using the existing gh login"]
fn existing_github_cli_login_can_read_public_task(){let mut s=source("github","https://api.github.com".into());s.scope="microsoft/terminal".into();s.auth_mode="cli".into();let page=provider::Api::new(&s,"").unwrap().list("","open","").unwrap();assert!(!page.items.is_empty());assert!(page.items.iter().all(|i|i.url.starts_with("https://github.com/microsoft/terminal/issues/")));}
