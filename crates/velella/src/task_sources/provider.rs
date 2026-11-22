use super::{Item, ItemMetadata, Source};
use anyhow::{Context, Result};
use reqwest::{blocking::Client, header::HeaderMap, Method, Url};
use serde_json::{json, Value};
use std::{io::Read, time::Duration};

const SIZE: usize=50;
const LINEAR_FIELDS: &str="id identifier title description url updatedAt createdAt dueDate priorityLabel assignee { name } creator { name } labels(first:100) { nodes { name } } project { name } cycle { name number } state { name type } team { id name key }";
pub struct Page {pub items:Vec<Item>,pub next:Option<String>}
pub struct Api<'a> {source:&'a Source,token:&'a str,client:Client}
impl<'a> Api<'a> {
    pub fn new(source:&'a Source,token:&'a str)->Result<Self>{
        Ok(Self{source,token,client:Client::builder().connect_timeout(Duration::from_secs(8)).timeout(Duration::from_secs(25)).redirect(reqwest::redirect::Policy::none()).user_agent("Velella/0.2 task-sources").build()?})
    }
    fn url(&self,segments:&[&str])->Result<Url>{
        let s=self.source;
        let mut url=Url::parse(&s.base_url)?;
        let path=url.path().trim_end_matches('/').to_string();
        let suffix=match s.provider.as_str(){
            "github" if url.host_str()==Some("api.github.com")=>"",
            "github" if !path.ends_with("/api/v3")=>"/api/v3",
            "gitlab" if !path.ends_with("/api/v4")=>"/api/v4",
            "jira"=>if s.jira_kind=="cloud"{"/rest/api/3"}else{"/rest/api/2"},
            "linear"=>"/graphql",_=>"",
        };
        url.set_path(&format!("{path}{suffix}"));
        {let mut parts=url.path_segments_mut().map_err(|_|anyhow::anyhow!("invalid source URL"))?;parts.pop_if_empty();for segment in segments{parts.push(segment);}}
        Ok(url)
    }
    fn request(&self,method:Method,url:Url,body:Option<&Value>)->Result<(Value,HeaderMap)>{
        if self.source.auth_mode=="cli" {return super::cli::request(&self.source.provider,method,&url,body);}
        let mut request=self.client.request(method,url).header("Accept","application/json");
        if !self.token.is_empty(){request=match self.source.provider.as_str(){
            "gitlab"=>request.header("PRIVATE-TOKEN",self.token),
            "linear" if self.source.auth_mode=="oauth"=>request.bearer_auth(self.token),
            "linear"=>request.header("Authorization",self.token),
            "jira" if !self.source.login.is_empty()=>request.basic_auth(&self.source.login,Some(self.token)),
            _=>request.bearer_auth(self.token),
        };}
        if self.source.provider=="github"{request=request.header("Accept","application/vnd.github+json").header("X-GitHub-Api-Version","2022-11-28");}
        if let Some(body)=body{request=request.json(body);}
        let response=request.send().map_err(|e|anyhow::anyhow!(if e.is_timeout(){"来源请求超时，请稍后手动刷新"}else{"无法连接任务来源，请检查站点地址、网络或代理"}))?;
        let status=response.status();let headers=response.headers().clone();
        if !status.is_success(){
            let mut detail=String::new();let _=response.take(32768).read_to_string(&mut detail);
            let detail=detail.to_ascii_lowercase();
            let limited=status.as_u16()==429 || (status.as_u16()==403 && (headers.get("x-ratelimit-remaining").is_some_and(|v|v=="0")||headers.contains_key("retry-after")||detail.contains("rate limit")||detail.contains("rate_limit")));
            let reason=if limited{"来源 API 已限流，请等待后再刷新"}else{match status.as_u16(){401=>"认证失败，请检查 Token 或账号",403=>"没有此项目的 API 权限",404=>"项目/任务不存在，或当前账号无权访问",400|422=>"来源拒绝请求，请检查项目、字段和必填项",300..=399=>"站点发生重定向，请使用实际 API 站点地址",_=>"来源服务暂时不可用"}};
            anyhow::bail!("{reason}（HTTP {}）",status.as_u16());
        }
        anyhow::ensure!(response.content_length().unwrap_or(0)<=8*1024*1024,"来源响应过大，请缩小查询范围");
        let mut bytes=Vec::new();response.take(8*1024*1024+1).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len()<=8*1024*1024,"来源响应过大");
        let value:Value=if status.as_u16()==204{Value::Null}else{serde_json::from_slice(&bytes).context("来源返回了无法读取的 JSON")?};
        if self.source.provider=="linear" {anyhow::ensure!(value["errors"].as_array().is_none_or(Vec::is_empty),"Linear 拒绝了请求，请检查 API Key、团队权限或字段");}
        Ok((value,headers))
    }
    fn get(&self,segments:&[&str],params:&[(&str,String)])->Result<(Value,HeaderMap)>{
        let mut url=self.url(segments)?;url.query_pairs_mut().extend_pairs(params.iter().map(|(k,v)|(*k,v.as_str())));
        self.request(Method::GET,url,None)
    }
    fn gql(&self,query:&str,variables:Value)->Result<Value>{
        Ok(self.request(Method::POST,self.url(&[])?,Some(&json!({"query":query,"variables":variables})))?.0["data"].clone())
    }
    fn item(&self,v:&Value)->Result<Item>{
        let s=self.source;
        let (id,identifier,title,url,description,state,closed,updated)=match s.provider.as_str(){
            "github"=>{let id=v["number"].as_u64().context("GitHub issue number missing")?.to_string();(id.clone(),format!("#{id}"),text(v,"title"),text(v,"html_url"),text(v,"body"),text(v,"state"),v["state"]=="closed",text(v,"updated_at"))},
            "gitlab"=>{let id=v["iid"].as_u64().context("GitLab issue iid missing")?.to_string();(id.clone(),format!("#{id}"),text(v,"title"),text(v,"web_url"),text(v,"description"),text(v,"state"),v["state"]=="closed",text(v,"updated_at"))},
            "linear"=>(text(v,"id"),text(v,"identifier"),text(v,"title"),text(v,"url"),text(v,"description"),text(&v["state"],"name"),matches!(v["state"]["type"].as_str(),Some("completed"|"canceled")),text(v,"updatedAt")),
            "jira"=>{let key=text(v,"key");let mut url=Url::parse(&s.base_url)?;url.path_segments_mut().map_err(|_|anyhow::anyhow!("invalid Jira URL"))?.pop_if_empty().push("browse").push(&key);
                (key.clone(),key,text(&v["fields"],"summary"),url.to_string(),adf_text(&v["fields"]["description"]),text(&v["fields"]["status"],"name"),v["fields"]["status"]["statusCategory"]["key"]=="done",text(&v["fields"],"updated"))},
            _=>anyhow::bail!("unsupported source"),
        };
        let scope=match s.provider.as_str(){"linear"=>v["team"]["id"].as_str().unwrap_or(&s.scope).to_owned(),"jira"=>v["fields"]["project"]["key"].as_str().unwrap_or_else(||identifier.split('-').next().unwrap_or(&s.scope)).to_owned(),_=>s.scope.clone()};
        let metadata=self.metadata(v);
        let item=Item{key:format!("{}:{id}",s.id),source_id:s.id.clone(),project_id:s.project_id.clone(),provider:s.provider.clone(),remote_id:id,identifier,title,url,description,state,closed,updated_at:updated,scope,metadata};
        super::validate_item(&item)?;Ok(item)
    }
    fn metadata(&self,v:&Value)->ItemMetadata {
        let names=|values:&Value,field:&str|->Vec<String>{values.as_array().into_iter().flatten().filter_map(|value|value.as_str().or_else(||value[field].as_str())).filter(|s|!s.is_empty()).map(str::to_owned).collect()};
        match self.source.provider.as_str(){
            "github"=>ItemMetadata{assignees:names(&v["assignees"],"login"),creator:text(&v["user"],"login"),labels:names(&v["labels"],"name"),project:self.source.scope.clone(),milestone:text(&v["milestone"],"title"),created_at:text(v,"created_at"),..Default::default()},
            "gitlab"=>ItemMetadata{assignees:names(&v["assignees"],"name"),creator:text(&v["author"],"name"),labels:names(&v["labels"],"name"),project:self.source.scope.clone(),milestone:text(&v["milestone"],"title"),created_at:text(v,"created_at"),due_at:text(v,"due_date"),..Default::default()},
            "linear"=>{
                let assignee=text(&v["assignee"],"name");
                let cycle=text(&v["cycle"],"name");
                ItemMetadata{assignees:if assignee.is_empty(){vec![]}else{vec![assignee]},creator:text(&v["creator"],"name"),priority:text(v,"priorityLabel"),labels:names(&v["labels"]["nodes"],"name"),project:text(&v["project"],"name"),milestone:if cycle.is_empty(){v["cycle"]["number"].as_u64().map(|n|format!("Cycle {n}")).unwrap_or_default()}else{cycle},created_at:text(v,"createdAt"),due_at:text(v,"dueDate")}
            },
            "jira"=>{
                let f=&v["fields"];let assignee=text(&f["assignee"],"displayName");
                ItemMetadata{assignees:if assignee.is_empty(){vec![]}else{vec![assignee]},creator:text(&f["reporter"],"displayName"),priority:text(&f["priority"],"name"),labels:names(&f["labels"],"name"),project:text(&f["project"],"name"),milestone:names(&f["fixVersions"],"name").join("、"),created_at:text(f,"created"),due_at:text(f,"duedate")}
            },
            _=>ItemMetadata::default(),
        }
    }
    pub fn list(&self,cursor:&str,state:&str,search:&str)->Result<Page>{
        self.list_scoped(cursor,state,search,&[])
    }
    pub fn list_scoped(&self,cursor:&str,state:&str,search:&str,scopes:&[String])->Result<Page>{
        anyhow::ensure!(["open","closed","all"].contains(&state)&&search.len()<=500&&cursor.len()<=2048,"无效查询条件");
        let s=self.source;
        let page=if cursor.is_empty(){1}else if matches!(s.provider.as_str(),"github"|"gitlab"){cursor.parse::<usize>().context("无效页码")?}else{1};
        anyhow::ensure!(page>0&&page<=10000,"无效页码");
        let (nodes,next)=match s.provider.as_str(){
            "github"=>{
                let mut query=format!("repo:{} is:issue",s.scope);
                if state!="all"{query.push_str(&format!(" is:{state}"));}
                if !search.trim().is_empty(){query.push_str(&format!(" {} in:title,body",serde_json::to_string(search.trim())?));}
                let (v,_)=self.get(&["search","issues"],&[("q",query),("sort","updated".into()),("order","desc".into()),("per_page",SIZE.to_string()),("page",page.to_string())])?;
                anyhow::ensure!(v["incomplete_results"]!=true,"GitHub 搜索结果尚不完整，请缩小查询后重试");
                let nodes=v["items"].as_array().context("GitHub items missing")?.clone();
                let total=v["total_count"].as_u64().unwrap_or(0) as usize;
                // GitHub search only exposes 1000 results; never call that an all-history scan.
                (nodes,if page*SIZE<total.min(1000){Some((page+1).to_string())}else{None})
            },
            "gitlab"=>{
                let mut params=vec![("per_page",SIZE.to_string()),("page",page.to_string()),("order_by","updated_at".into()),("sort","desc".into())];
                if state!="all"{params.push(("state",if state=="open"{"opened"}else{"closed"}.into()));}
                if !search.is_empty(){params.push(("search",search.into()));}
                let (v,h)=self.get(&["projects",&s.scope,"issues"],&params)?;
                let nodes=v.as_array().context("GitLab issues missing")?.clone();
                let next=if let Some(header)=h.get("x-next-page"){header.to_str().ok().filter(|s|!s.is_empty()).map(str::to_owned)}else if nodes.len()==SIZE{Some((page+1).to_string())}else{None};
                (nodes,next)
            },
            "linear"=>{
                let mut filter=if !scopes.is_empty(){json!({"team":{"id":{"in":scopes}}})}else if !s.scope.is_empty(){json!({"team":{"id":{"eq":s.scope}}})}else{json!({})};
                if state!="all" {filter["state"]=if state=="open"{json!({"type":{"nin":["completed","canceled"]}})}else{json!({"type":{"in":["completed","canceled"]}})};}
                if !search.is_empty(){filter["title"]=json!({"containsIgnoreCase":search});}
                let query=format!("query($after:String,$filter:IssueFilter){{issues(first:{SIZE},after:$after,filter:$filter,orderBy:updatedAt){{nodes{{{LINEAR_FIELDS}}} pageInfo{{hasNextPage endCursor}}}}}}");
                let data=self.gql(&query,json!({"after":if cursor.is_empty(){Value::Null}else{json!(cursor)},"filter":filter}))?;
                let page=&data["issues"];let nodes=page["nodes"].as_array().context("Linear issues missing")?.clone();
                (nodes,if page["pageInfo"]["hasNextPage"]==true{Some(text(&page["pageInfo"],"endCursor"))}else{None})
            },
            "jira"=>{
                let mut clauses=Vec::new();
                if !scopes.is_empty(){clauses.push(format!("project in ({})",scopes.iter().map(serde_json::to_string).collect::<std::result::Result<Vec<_>,_>>()?.join(",")));}
                else if !s.scope.is_empty(){clauses.push(format!("project = {}",serde_json::to_string(&s.scope)?));}
                if state!="all"{clauses.push(if state=="open"{"statusCategory != Done"}else{"statusCategory = Done"}.into());}
                if !search.is_empty(){clauses.push(format!("text ~ {}",serde_json::to_string(search)?));}
                let mut jql=clauses.join(" AND ");
                jql.push_str(" ORDER BY updated DESC");
                let mut body=json!({"jql":jql,"maxResults":SIZE,"fields":["summary","description","status","updated","project","assignee","reporter","priority","labels","fixVersions","created","duedate"]});
                let cloud=s.jira_kind=="cloud";
                if !cursor.is_empty(){if cloud{body["nextPageToken"]=json!(cursor)}else{body["startAt"]=json!(cursor.parse::<usize>().context("无效 Jira 页码")?)}}
                let (v,_)=self.request(Method::POST,self.url(if cloud{&["search","jql"]}else{&["search"]})?,Some(&body))?;
                let nodes=v["issues"].as_array().context("Jira issues missing")?.clone();
                let next=if cloud{v["nextPageToken"].as_str().filter(|s|!s.is_empty()).map(str::to_owned)}else{let offset=v["startAt"].as_u64().unwrap_or(0) as usize+nodes.len();if offset<(v["total"].as_u64().unwrap_or(0) as usize)&&!nodes.is_empty(){Some(offset.to_string())}else{None}};
                (nodes,next)
            },_=>anyhow::bail!("unsupported source"),
        };
        anyhow::ensure!(next.as_deref()!=Some(cursor),"来源返回了重复分页标记，请刷新");
        Ok(Page{items:nodes.iter().map(|v|self.item(v)).collect::<Result<_>>()?,next})
    }
    pub fn detail(&self,id:&str)->Result<Item>{
        let s=self.source;
        let v=match s.provider.as_str(){
            "github"=>{let parts=s.scope.split('/').collect::<Vec<_>>();anyhow::ensure!(parts.len()==2,"GitHub 项目路径应为 owner/repository");self.get(&["repos",parts[0],parts[1],"issues",id],&[])?.0},
            "gitlab"=>self.get(&["projects",&s.scope,"issues",id],&[])?.0,
            "linear"=>self.gql(&format!("query($id:String!){{issue(id:$id){{{LINEAR_FIELDS}}}}}"),json!({"id":id}))?["issue"].clone(),
            "jira"=>self.get(&["issue",id],&[("fields","summary,description,status,updated,project,assignee,reporter,priority,labels,fixVersions,created,duedate".into())])?.0,
            _=>anyhow::bail!("unsupported source"),
        };self.item(&v)
    }
    pub fn discover(&self)->Result<Value>{
        match self.source.provider.as_str(){
            "linear"=>{let mut rows=Vec::new();let mut cursor=Value::Null;loop{let v=self.gql("query($after:String) { teams(first:100,after:$after) { nodes { id name key } pageInfo { hasNextPage endCursor } } }",json!({"after":cursor}))?;let page=&v["teams"];rows.extend(page["nodes"].as_array().context("teams missing")?.iter().map(|n|json!({"id":n["id"],"name":format!("{} · {}",text(n,"key"),text(n,"name"))})));if page["pageInfo"]["hasNextPage"]!=true{break;}let next=page["pageInfo"]["endCursor"].clone();anyhow::ensure!(next.is_string()&&next!=cursor&&rows.len()<5000,"团队列表分页异常或过大，请缩小权限范围");cursor=next;}Ok(json!(rows))},
            "jira"=>{let mut rows=Vec::new();let mut offset=0;loop{let v=if self.source.jira_kind=="cloud"{self.get(&["project","search"],&[("maxResults","100".into()),("startAt",offset.to_string())])?.0}else{self.get(&["project"],&[])?.0};let page=v.as_array().or_else(||v["values"].as_array()).context("projects missing")?;rows.extend(page.iter().map(|n|json!({"id":n["key"],"name":format!("{} · {}",text(n,"key"),text(n,"name"))})));offset+=page.len();if self.source.jira_kind!="cloud"||v["isLast"]==true||page.is_empty()||offset>=v["total"].as_u64().unwrap_or(offset as u64) as usize{break;}anyhow::ensure!(offset<5000,"项目列表过大，请缩小权限范围");}Ok(json!(rows))},
            _=>Ok(json!([])),
        }
    }
    pub fn account(&self)->Result<Value>{
        let data=self.gql("query { organization { id name } }",json!({}))?;
        anyhow::ensure!(data["organization"]["id"].as_str().is_some_and(|s|!s.is_empty()),"无法识别 Linear 工作区");Ok(data["organization"].clone())
    }
    pub fn verify_scope(&self)->Result<()> {
        if self.source.provider=="linear" && !self.source.scope.is_empty() {
            let data=self.gql("query($id:String!){team(id:$id){id}}",json!({"id":self.source.scope}))?;
            anyhow::ensure!(data["team"]["id"].as_str()==Some(self.source.scope.as_str()),"Linear 团队不存在或无权访问，请从团队列表选择");
        }
        Ok(())
    }
    pub fn create_options(&self,type_id:&str)->Result<Value>{
        if self.source.provider!="jira"{return Ok(json!({"types":[],"fields":[]}));}
        anyhow::ensure!(!self.source.scope.is_empty(),"请选择新任务所属的 Jira 项目");
        let mut segments=vec!["issue","createmeta",self.source.scope.as_str(),"issuetypes"];
        if !type_id.is_empty(){segments.push(type_id);}
        let mut entries=vec![];let mut offset=0;
        loop {
            let (v,_)=self.get(&segments,&[("maxResults","100".into()),("startAt",offset.to_string())])?;
            let page=v["values"].as_array().or_else(||v["issueTypes"].as_array()).or_else(||v["fields"].as_array()).context("Jira 创建字段缺失")?;
            entries.extend(page.iter().cloned());offset+=page.len();
            if v["isLast"]==true || page.is_empty() || offset>=v["total"].as_u64().unwrap_or(offset as u64) as usize {break;}
            anyhow::ensure!(offset<2000,"Jira 创建字段过多，请在来源平台创建");
        }
        if type_id.is_empty(){return Ok(json!({"types":entries,"fields":[]}));}
        Ok(json!({"types":[],"fields":entries.into_iter().filter(|f|!matches!(f["fieldId"].as_str().or(f["key"].as_str()),Some("summary"|"description"|"project"|"issuetype"))).collect::<Vec<_>>()}))
    }
    pub fn create(&self,fields:&Value)->Result<Item>{
        anyhow::ensure!(!self.source.scope.is_empty(),"请选择新任务所属的团队或项目");
        let s=self.source;let title=text(fields,"title");let description=text(fields,"description");
        match s.provider.as_str(){
            "github"=>{let p=s.scope.split('/').collect::<Vec<_>>();anyhow::ensure!(p.len()==2,"GitHub 项目路径错误");let v=self.request(Method::POST,self.url(&["repos",p[0],p[1],"issues"])?,Some(&json!({"title":title,"body":description})))?.0;self.item(&v)},
            "gitlab"=>{let v=self.request(Method::POST,self.url(&["projects",&s.scope,"issues"])?,Some(&json!({"title":title,"description":description})))?.0;self.item(&v)},
            "linear"=>{let data=self.gql(&format!("mutation($input:IssueCreateInput!){{issueCreate(input:$input){{success issue{{{LINEAR_FIELDS}}}}}}}"),json!({"input":{"teamId":s.scope,"title":title,"description":description}}))?;anyhow::ensure!(data["issueCreate"]["success"]==true,"Linear 未确认任务创建");self.item(&data["issueCreate"]["issue"])},
            "jira"=>{
                let type_id=fields["type_id"].as_str().filter(|s|!s.is_empty()).context("请选择 Jira 事项类型")?;
                let mut payload=fields["extra"].as_object().cloned().unwrap_or_default();
                payload.insert("project".into(),json!({"key":s.scope}));payload.insert("issuetype".into(),json!({"id":type_id}));payload.insert("summary".into(),json!(title));
                payload.insert("description".into(),if s.jira_kind=="cloud"{json!({"type":"doc","version":1,"content":description.lines().map(|line|json!({"type":"paragraph","content":if line.is_empty(){vec![]}else{vec![json!({"type":"text","text":line})]}})).collect::<Vec<_>>()})}else{json!(description)});
                let v=self.request(Method::POST,self.url(&["issue"])?,Some(&json!({"fields":payload})))?.0;
                // Creation succeeded. Do not turn a failed follow-up read into a second POST.
                self.item(&json!({"key":v["key"],"fields":{"summary":title,"description":description,"status":{"name":"已创建"},"updated":""}}))
            },_=>anyhow::bail!("unsupported source"),
        }
    }

    fn issue_path(&self,id:&str)->Result<Vec<String>> {
        let s=self.source;
        Ok(match s.provider.as_str(){
            "github"=>{let p=s.scope.split('/').collect::<Vec<_>>();anyhow::ensure!(p.len()==2,"GitHub 仓库路径错误");vec!["repos".into(),p[0].into(),p[1].into(),"issues".into(),id.into()]},
            "gitlab"=>vec!["projects".into(),s.scope.clone(),"issues".into(),id.into()],
            "jira"=>vec!["issue".into(),id.into()],_=>vec![]})
    }
    pub fn states(&self,id:&str)->Result<Value>{
        match self.source.provider.as_str(){
            "linear"=>{let d=self.gql("query($id:String!){issue(id:$id){team{states(first:100){nodes{id name}}}}}",json!({"id":id}))?;Ok(d["issue"]["team"]["states"]["nodes"].clone())},
            "jira"=>Ok(self.get(&["issue",id,"transitions"],&[])?.0["transitions"].clone()),
            _=>Ok(json!([{"id":"open","name":"重新打开"},{"id":"closed","name":"关闭任务"}]))
        }
    }
    pub fn comments(&self,id:&str,cursor:&str)->Result<Value>{
        if self.source.provider=="linear"{
            let d=self.gql("query($id:String!,$after:String){issue(id:$id){comments(first:50,after:$after){nodes{id body createdAt user{name}} pageInfo{hasNextPage endCursor}}}}",json!({"id":id,"after":if cursor.is_empty(){Value::Null}else{json!(cursor)}}))?;
            let p=&d["issue"]["comments"];let comments=p["nodes"].as_array().context("评论列表无效")?.iter().map(|v|json!({"id":v["id"],"body":v["body"],"author":v["user"]["name"],"created":v["createdAt"]})).collect::<Vec<_>>();
            return Ok(json!({"comments":comments,"next":if p["pageInfo"]["hasNextPage"]==true{p["pageInfo"]["endCursor"].clone()}else{Value::Null}}));
        }
        let page=if cursor.is_empty(){1}else{cursor.parse::<usize>().context("无效评论页码")?};anyhow::ensure!(page>0&&page<=10000,"无效评论页码");
        let mut path=self.issue_path(id)?;path.push(if self.source.provider=="gitlab"{"notes"}else if self.source.provider=="jira"{"comment"}else{"comments"}.into());
        let params=if self.source.provider=="jira"{vec![("startAt",((page-1)*50).to_string()),("maxResults","50".into())]}else{vec![("page",page.to_string()),("per_page","50".into())]};
        let (d,h)=self.get(&path.iter().map(String::as_str).collect::<Vec<_>>(),&params)?;
        let nodes=d.as_array().or_else(||d["comments"].as_array()).context("评论列表无效")?;
        let comments=nodes.iter().filter(|v|v["system"]!=true).map(|v|json!({"id":v["id"].to_string(),"body":adf_text(&v["body"]),"author":v["user"]["login"].as_str().or(v["author"]["name"].as_str()).or(v["author"]["displayName"].as_str()).unwrap_or(""),"created":v["created_at"].as_str().or(v["created"].as_str()).unwrap_or("")})).collect::<Vec<_>>();
        let more=if self.source.provider=="jira"{page*50<d["total"].as_u64().unwrap_or(0) as usize}else if let Some(next)=h.get("x-next-page"){!next.is_empty()}else{nodes.len()==50};
        Ok(json!({"comments":comments,"next":if more{Some((page+1).to_string())}else{None}}))
    }
    pub fn mutate(&self,id:&str,fields:&Value)->Result<()> {
        let action=fields["action"].as_str().unwrap_or("");let s=self.source;
        anyhow::ensure!(["comment","state","edit"].contains(&action),"不支持的任务操作");
        if action=="comment"{anyhow::ensure!(!text(fields,"body").trim().is_empty(),"请填写评论");}
        if action=="edit"{anyhow::ensure!(!text(fields,"title").trim().is_empty(),"请填写任务标题");}
        if s.provider=="linear"{
            let (query,input)=if action=="comment"{("mutation($input:CommentCreateInput!){commentCreate(input:$input){success}}",json!({"issueId":id,"body":fields["body"]}))}else{("mutation($id:String!,$input:IssueUpdateInput!){issueUpdate(id:$id,input:$input){success}}",if action=="state"{json!({"stateId":fields["state"]})}else{json!({"title":fields["title"],"description":fields["description"]})})};
            let result=self.gql(query,json!({"id":id,"input":input}))?;
            anyhow::ensure!(result[if action=="comment"{"commentCreate"}else{"issueUpdate"}]["success"]==true,"Linear 未确认修改");return Ok(());
        }
        let mut path=self.issue_path(id)?;
        let (method,body)=match (s.provider.as_str(),action){
            ("github","comment")=>{path.push("comments".into());(Method::POST,json!({"body":fields["body"]}))},
            ("gitlab","comment")=>{path.push("notes".into());(Method::POST,json!({"body":fields["body"]}))},
            ("jira","comment")=>{path.push("comment".into());(Method::POST,json!({"body":if s.jira_kind=="cloud"{adf_document(&text(fields,"body"))}else{fields["body"].clone()}}))},
            ("github","state")=>{anyhow::ensure!(matches!(fields["state"].as_str(),Some("open"|"closed")),"无效任务状态");(Method::PATCH,json!({"state":fields["state"]}))},
            ("gitlab","state")=>{anyhow::ensure!(matches!(fields["state"].as_str(),Some("open"|"closed")),"无效任务状态");(Method::PUT,json!({"state_event":if fields["state"]=="open"{"reopen"}else{"close"}}))},
            ("jira","state")=>{path.push("transitions".into());(Method::POST,json!({"transition":{"id":fields["state"]}}))},
            ("github","edit")=>(Method::PATCH,json!({"title":fields["title"],"body":fields["description"]})),
            ("gitlab","edit")=>(Method::PUT,json!({"title":fields["title"],"description":fields["description"]})),
            ("jira","edit")=>(Method::PUT,json!({"fields":{"summary":fields["title"],"description":if s.jira_kind=="cloud"{adf_document(&text(fields,"description"))}else{fields["description"].clone()}}})),
            _=>anyhow::bail!("不支持的任务操作")
        };
        self.request(method,self.url(&path.iter().map(String::as_str).collect::<Vec<_>>())?,Some(&body))?;Ok(())
    }
}
fn adf_document(text:&str)->Value{json!({"type":"doc","version":1,"content":text.lines().map(|line|json!({"type":"paragraph","content":if line.is_empty(){vec![]}else{vec![json!({"type":"text","text":line})]}})).collect::<Vec<_>>()})}
fn text(v:&Value,key:&str)->String{v[key].as_str().unwrap_or("").into()}
fn adf_text(v:&Value)->String{
    if let Some(text)=v.as_str(){return text.into()}
    if v["type"]=="text"{return text(v,"text")}
    if v["type"]=="hardBreak"{return "\n".into()}
    let mut result=v["content"].as_array().map(|items|items.iter().map(adf_text).collect::<String>()).unwrap_or_default();
    if matches!(v["type"].as_str(),Some("paragraph"|"heading"|"listItem")){result.push('\n');}result
}
