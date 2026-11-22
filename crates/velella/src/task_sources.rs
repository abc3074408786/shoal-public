//! External development work items. HTTP never runs under the supervisor lock.
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

mod credentials;
mod provider;
mod cli;
mod github_login;
mod linear_login;
#[cfg(test)] mod tests;

#[derive(Clone, Serialize, Deserialize)]
pub struct Source {
    #[serde(default)] pub id: String,
    #[serde(default)] pub project_id: String,
    pub provider: String,
    pub name: String,
    pub base_url: String,
    pub scope: String,
    #[serde(default)] pub login: String,
    #[serde(default)] pub jira_kind: String,
    #[serde(default)] pub auth_mode: String,
    #[serde(default)] pub disabled: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Item {
    pub key: String,
    pub source_id: String,
    pub project_id: String,
    pub provider: String,
    pub remote_id: String,
    pub identifier: String,
    pub title: String,
    pub url: String,
    pub description: String,
    pub state: String,
    pub closed: bool,
    pub updated_at: String,
    #[serde(default)] pub scope: String,
    #[serde(default)] pub metadata: ItemMetadata,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct ItemMetadata {
    pub assignees: Vec<String>,
    pub creator: String,
    pub priority: String,
    pub labels: Vec<String>,
    pub project: String,
    pub milestone: String,
    pub created_at: String,
    pub due_at: String,
}

pub struct Store { conn: Connection }
impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        let conn = Connection::open(root.join("task-sources.db"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS sources(id TEXT PRIMARY KEY, identity TEXT UNIQUE NOT NULL, document TEXT NOT NULL, secret TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS items(key TEXT PRIMARY KEY, source_id TEXT NOT NULL, document TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS links(key TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, project_id TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS creations(id TEXT PRIMARY KEY, source_id TEXT NOT NULL, request TEXT NOT NULL, result TEXT);
            CREATE TABLE IF NOT EXISTS mutations(id TEXT PRIMARY KEY, source_id TEXT NOT NULL, request TEXT NOT NULL, result TEXT);
            CREATE TABLE IF NOT EXISTS ignored_repositories(identity TEXT PRIMARY KEY);")?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS workspace_creations(id TEXT PRIMARY KEY, request TEXT NOT NULL);")?;
        Ok(Self { conn })
    }
    fn sources(&self) -> Result<Vec<Source>> {
        Ok(self.all_sources()?.into_iter().filter(|s|!s.disabled).collect())
    }
    fn all_sources(&self) -> Result<Vec<Source>> {
        let mut query = self.conn.prepare("SELECT document FROM sources ORDER BY rowid")?;
        let docs = query.query_map([], |r| r.get::<_, String>(0))?.collect::<std::result::Result<Vec<_>, _>>()?;
        docs.iter().map(|s| Ok(serde_json::from_str(s)?)).collect()
    }
    fn source(&self, id: &str) -> Result<(Source, String)> {
        let row: (String, String) = self.conn.query_row("SELECT document,secret FROM sources WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?.context("任务来源已断开，请重新连接")?;
        let source:Source=serde_json::from_str(&row.0)?;anyhow::ensure!(!source.disabled,"任务来源已断开，请重新连接");
        let token=if source.provider=="linear"&&source.auth_mode=="oauth"{linear_login::access_token(self,id)?}else{credentials::unprotect(&row.1)?};
        Ok((source, token))
    }
    fn save(&self, source: &Source, token: &str) -> Result<()> {
        let identity = json!([source.project_id, source.provider, source.base_url, source.scope,source.login]).to_string();
        self.conn.execute("INSERT INTO sources(id,identity,document,secret) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET document=excluded.document,secret=excluded.secret", params![source.id, identity, serde_json::to_string(source)?, credentials::protect(token)?]).context("此项目已连接相同来源，请编辑现有连接")?;
        if !source.disabled{self.conn.execute("DELETE FROM ignored_repositories WHERE identity=?1",[json!([source.provider,source.base_url,source.scope]).to_string()])?;}
        Ok(())
    }
    fn cache(&self, items: &[Item]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for item in items {
            validate_item(item)?;
            tx.execute("INSERT INTO items(key,source_id,document) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET document=excluded.document", params![item.key, item.source_id, serde_json::to_string(item)?])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn item(&self, key: &str) -> Result<Item> {
        let text: String = self.conn.query_row("SELECT document FROM items WHERE key=?1", [key], |r| r.get(0)).optional()?.context("请先在任务页加载该任务")?;
        Ok(serde_json::from_str(&text)?)
    }
    pub fn linked_workspace(&self, key: &str, project: &str) -> Result<Option<String>> {
        let row:Option<(String,String)>=self.conn.query_row("SELECT workspace_id,project_id FROM links WHERE key=?1", [key], |r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((workspace,owner))=row {anyhow::ensure!(owner==project,"此任务已关联其他项目，请打开已有开发工作区");Ok(Some(workspace))}else{Ok(None)}
    }
    pub fn saved_workspace_request(&self,id:&str)->Result<Option<Value>> {
        let request:Option<String>=self.conn.query_row("SELECT request FROM workspace_creations WHERE id=?1",[id],|r|r.get(0)).optional()?;
        request.map(|request|Ok(serde_json::from_str(&request)?)).transpose()
    }
    pub fn workspace_request(&self,id:&str,request:&Value)->Result<()> {
        uuid::Uuid::parse_str(id)?;let serialized=request.to_string();
        self.conn.execute("INSERT OR IGNORE INTO workspace_creations(id,request) VALUES(?1,?2)",params![id,serialized])?;
        let existing:String=self.conn.query_row("SELECT request FROM workspace_creations WHERE id=?1",[id],|r|r.get(0))?;
        anyhow::ensure!(existing==serialized,"创建请求标识已属于其他工作区，请重新打开创建窗口");Ok(())
    }
    pub fn attach(&self, key: &str, workspace: &str, project: &str) -> Result<()> {
        self.item(key)?;
        self.conn.execute("INSERT OR IGNORE INTO links(key,workspace_id,project_id) VALUES(?1,?2,?3)", params![key,workspace,project])?;
        anyhow::ensure!(self.linked_workspace(key,project)?.as_deref()==Some(workspace), "此任务已关联其他工作区，请从任务页打开已有工作区");
        Ok(())
    }
    pub fn links(&self) -> Result<Vec<Value>> {
        let mut query = self.conn.prepare("SELECT l.workspace_id,i.document,l.project_id FROM links l JOIN items i ON i.key=l.key")?;
        let rows = query.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
        rows.into_iter().map(|(workspace,text,project)| {
            let item: Item=serde_json::from_str(&text)?;
            Ok(json!({"workspace_id":workspace,"key":item.key,"source_id":item.source_id,"project_id":project,"provider":item.provider,"identifier":item.identifier,"title":item.title,"url":item.url}))
        }).collect()
    }
}

fn validate_item(item: &Item) -> Result<()> {
    let url = reqwest::Url::parse(&item.url).context("来源返回了无效链接")?;
    anyhow::ensure!(matches!(url.scheme(),"https"|"http") && url.host_str().is_some() && url.username().is_empty() && url.password().is_none(), "来源链接必须是 HTTP(S) 地址");
    anyhow::ensure!(!item.remote_id.is_empty() && !item.title.is_empty() && item.description.len() <= 512 * 1024, "来源任务内容不完整或过大");
    Ok(())
}

fn normalize(mut source: Source) -> Result<Source> {
    if source.id.is_empty() { source.id=uuid::Uuid::now_v7().to_string(); }
    uuid::Uuid::parse_str(&source.id)?;
    source.name=source.name.trim().into();
    source.scope=source.scope.trim().into();
    source.base_url=source.base_url.trim().trim_end_matches('/').into();
    source.login=source.login.trim().into();
    anyhow::ensure!(["github","gitlab","linear","jira"].contains(&source.provider.as_str()), "不支持的任务来源");
    anyhow::ensure!(!source.name.is_empty() && source.name.len()<=200 && source.scope.len()<=300 && !source.scope.chars().any(char::is_control), "请填写连接名称，项目/团队格式不正确");
    anyhow::ensure!(matches!(source.auth_mode.as_str(),""|"token"|"cli"|"oauth") && (source.auth_mode!="cli"||matches!(source.provider.as_str(),"github"|"gitlab")) && (source.auth_mode!="oauth"||source.provider=="linear"),"不支持的登录方式");
    let mut url=reqwest::Url::parse(&source.base_url).context("请填写完整的 HTTPS 站点地址")?;
    let local_test=cfg!(test) && url.scheme()=="http" && url.host_str()==Some("127.0.0.1");
    anyhow::ensure!((url.scheme()=="https" || local_test) && url.host_str().is_some() && url.username().is_empty() && url.password().is_none() && url.query().is_none() && url.fragment().is_none(), "站点必须使用 HTTPS，且不能含账号、查询参数或片段");
    if source.provider=="linear" && !local_test { anyhow::ensure!(url.as_str().trim_end_matches('/')=="https://api.linear.app", "Linear 使用 https://api.linear.app"); }
    if source.provider=="github" && url.host_str()==Some("github.com") {url.set_host(Some("api.github.com"))?;}
    if source.auth_mode=="cli"{anyhow::ensure!(url.port().is_none()&&matches!(url.path(),""|"/"|"/api/v3"|"/api/v4"),"此站点包含自定义端口或路径，请改用个人 Token 登录");}
    source.base_url=url.as_str().trim_end_matches('/').into();
    if source.provider=="jira" {
        anyhow::ensure!(["cloud","dc"].contains(&source.jira_kind.as_str()), "请选择 Jira Cloud 或 Data Center");
        anyhow::ensure!(source.scope.chars().all(|c|c.is_ascii_alphanumeric()||c=='_'||c=='-'),"Jira 项目键格式不正确");
        anyhow::ensure!(source.jira_kind!="cloud" || !source.login.is_empty(), "Jira Cloud 需要账号邮箱");
    }
    if source.provider=="github" || source.provider=="gitlab" {
        anyhow::ensure!(source.scope.split('/').count()>=2 && source.scope.split('/').all(|s| !s.is_empty() && s!="." && s!=".." && s.chars().all(|c|c.is_ascii_alphanumeric()||matches!(c,'_'|'-'|'.'))), "项目路径应为 owner/repository 或 group/project");
        anyhow::ensure!(source.provider!="github" || source.scope.split('/').count()==2,"GitHub 项目路径应为 owner/repository");
    }
    Ok(source)
}

pub fn sync_repositories(root:&Path,projects:&[crate::desktop::Project])->Result<Value>{
    let store=Store::open(root)?;let mut sources=store.sources()?;let repositories=cli::repositories(projects);
    for repo in &repositories {
        let provider=repo["provider"].as_str().unwrap_or("");if provider.is_empty(){continue;}
        let source=normalize(serde_json::from_value(json!({"project_id":repo["project_id"],"provider":provider,"name":repo["project_name"],"base_url":format!("https://{}",repo["host"].as_str().unwrap()),"scope":repo["scope"],"auth_mode":"cli"}))?)?;
        let ignored:bool=store.conn.query_row("SELECT EXISTS(SELECT 1 FROM ignored_repositories WHERE identity=?1)",[json!([source.provider,source.base_url,source.scope]).to_string()],|r|r.get(0))?;if ignored{continue;}
        if !sources.iter().any(|s|s.provider==source.provider&&s.base_url==source.base_url&&s.scope==source.scope) {store.save(&source,"")?;sources.push(source);}
    }
    Ok(json!({"ok":true,"sources":sources,"repositories":repositories,"links":store.links()?}))
}

pub fn dispatch(root: &Path, req: &Value) -> Result<Value> {
    if req["cmd"].as_str().is_some_and(|c|c.starts_with("sources.github.")) {return github_login::dispatch(root,req);}
    if req["cmd"].as_str().is_some_and(|c|c.starts_with("sources.linear.")) {return linear_login::dispatch(root,req);}
    let store=Store::open(root)?;
    let cmd=req["cmd"].as_str().unwrap_or("");
    if cmd=="sources.list" {return Ok(json!({"ok":true,"sources":store.sources()?,"links":store.links()?}));}
    if cmd=="sources.cached" {return Ok(json!({"ok":true,"item":store.item(req["key"].as_str().context("task key required")?)?}));}
    if cmd=="sources.save" || cmd=="sources.discover" {
        let mut source=normalize(serde_json::from_value(req["source"].clone())?)?;source.disabled=false;
        anyhow::ensure!(source.auth_mode!="oauth","请通过 Linear 授权入口重新连接");
        let supplied=req["token"].as_str().unwrap_or("");anyhow::ensure!(supplied.len()<=16384&&!supplied.chars().any(char::is_control),"Token 格式不正确");
        if source.provider=="linear"&&source.scope.is_empty()&&req["source"]["id"].as_str().unwrap_or("").is_empty(){
            let account=provider::Api::new(&source,req["token"].as_str().unwrap_or(""))?.account()?;
            source.login=account["id"].as_str().context("Linear 工作区标识缺失")?.into();
            if source.name=="Linear"{source.name=account["name"].as_str().unwrap_or("Linear").into();}
        }
        source=normalize(source)?;
        if req["source"]["id"].as_str().unwrap_or("").is_empty(){
            if let Some(existing)=store.all_sources()?.into_iter().find(|s|s.provider==source.provider&&s.base_url==source.base_url&&s.scope==source.scope&&s.login==source.login){source.id=existing.id;source.project_id=existing.project_id;}
        }
        let old=store.all_sources()?.into_iter().find(|s|s.id==source.id);
        anyhow::ensure!(!old.as_ref().is_some_and(|s|s.auth_mode=="oauth")||!supplied.is_empty(),"从授权连接改为手动连接时，请填写新的 API Key");
        if let Some(old)=&old {anyhow::ensure!(old.project_id==source.project_id && old.provider==source.provider && old.scope==source.scope && old.base_url==source.base_url && old.login==source.login && old.jira_kind==source.jira_kind,"已有连接的目标或账号不可变；请新增连接，以免改变既有任务的归属");}
        let token=match req["token"].as_str().filter(|s|!s.is_empty()) {Some(token)=>token.to_owned(), None if old.as_ref().is_some_and(|s|!s.disabled)=>store.source(&source.id)?.1, _=>String::new()};
        anyhow::ensure!(token.len()<=16384 && !token.chars().any(char::is_control), "Token 格式不正确");
        anyhow::ensure!(source.auth_mode=="cli"||source.provider=="github" || !token.is_empty(), "请填写访问 Token");
        let api=provider::Api::new(&source,&token)?;
        if cmd=="sources.discover" {return Ok(json!({"ok":true,"choices":api.discover()?}));}
        api.verify_scope()?;
        let page=api.list("", "open", "")?;
        store.save(&source,&token)?;
        store.cache(&page.items)?;
        return Ok(json!({"ok":true,"source":source}));
    }
    let id=req["source_id"].as_str().context("请选择任务来源")?;
    if cmd=="sources.remove" {
        let mut source=store.all_sources()?.into_iter().find(|s|s.id==id&&!s.disabled).context("任务来源已断开")?;
        source.disabled=true;store.save(&source,"")?;
        if matches!(source.provider.as_str(),"github"|"gitlab"){store.conn.execute("INSERT OR IGNORE INTO ignored_repositories(identity) VALUES(?1)",[json!([source.provider,source.base_url,source.scope]).to_string()])?;}
        return Ok(json!({"ok":true}));
    }
    let (mut source,token)=store.source(id)?;
    if cmd=="sources.scopes" {return Ok(json!({"ok":true,"choices":provider::Api::new(&source,&token)?.discover()?}));}
    if matches!(source.provider.as_str(),"linear"|"jira") {
        if let Some(scope)=req["scope"].as_str().or_else(||req["fields"]["scope"].as_str()) {source.scope=scope.into();source=normalize(source)?;}
    }
    let api=provider::Api::new(&source,&token)?;
    if matches!(cmd,"sources.states"|"sources.comments"|"sources.mutate"|"sources.mutation_status") {
        let item=store.item(req["key"].as_str().context("请选择任务")?)?;anyhow::ensure!(item.source_id==id,"任务不属于当前来源");
        if cmd=="sources.states"{return Ok(json!({"ok":true,"states":api.states(&item.remote_id)?}));}
        if cmd=="sources.comments"{let data=api.comments(&item.remote_id,req["cursor"].as_str().unwrap_or(""))?;return Ok(json!({"ok":true,"comments":data["comments"],"next":data["next"]}));}
        let request=uuid::Uuid::parse_str(req["request_id"].as_str().context("request ID required")?)?.to_string();
        let body=json!({"key":item.key,"fields":req["fields"]}).to_string();
        let existing:Option<(String,String,Option<String>)>=store.conn.query_row("SELECT source_id,request,result FROM mutations WHERE id=?1",[&request],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if let Some((owner,stored,result))=existing {anyhow::ensure!(owner==id&&(cmd=="sources.mutation_status"||stored==body),"请求标识不匹配");return result.map(|v|Ok(serde_json::from_str(&v)?)).unwrap_or_else(||Ok(json!({"ok":false,"uncertain":true,"error":"修改结果尚未确认，请先在来源平台核对。"})));}
        anyhow::ensure!(cmd=="sources.mutate","未找到修改回执");
        anyhow::ensure!(body.len()<=128*1024,"修改内容过大");
        store.conn.execute("INSERT INTO mutations(id,source_id,request) VALUES(?1,?2,?3)",params![request,id,body])?;
        let result=match api.mutate(&item.remote_id,&req["fields"]){Ok(())=>json!({"ok":true}),Err(e)=>json!({"ok":false,"uncertain":true,"error":format!("{e}；不会自动重试，请核对来源平台。")})};
        store.conn.execute("UPDATE mutations SET result=?2 WHERE id=?1",params![request,result.to_string()])?;return Ok(result);
    }
    if cmd=="sources.items" {
        let scopes:Vec<String>=serde_json::from_value(req.get("scopes").cloned().unwrap_or_else(||json!([])))?;
        anyhow::ensure!(scopes.len()<=100,"最多选择 100 个项目或团队");
        for scope in &scopes {let mut check=source.clone();check.scope=scope.clone();normalize(check)?;}
        let page=api.list_scoped(req["cursor"].as_str().unwrap_or(""),req["state"].as_str().unwrap_or("open"),req["search"].as_str().unwrap_or(""),&scopes)?;
        store.cache(&page.items)?;
        return Ok(json!({"ok":true,"items":page.items,"next":page.next}));
    }
    if cmd=="sources.detail" {
        let cached=store.item(req["key"].as_str().context("task key required")?)?;
        anyhow::ensure!(cached.source_id==id,"任务不属于所选来源");
        if matches!(source.provider.as_str(),"linear"|"jira")&&!cached.scope.is_empty(){source.scope=cached.scope.clone();}
        let item=provider::Api::new(&source,&token)?.detail(&cached.remote_id)?;
        store.cache(std::slice::from_ref(&item))?;
        return Ok(json!({"ok":true,"item":item}));
    }
    if cmd=="sources.create_options" {return Ok(json!({"ok":true,"options":api.create_options(req["type_id"].as_str().unwrap_or(""))?}));}
    if cmd=="sources.create" || cmd=="sources.creation_status" {
        let request_id=uuid::Uuid::parse_str(req["request_id"].as_str().context("request ID required")?)?.to_string();
        let existing:Option<(String,String,Option<String>)>=store.conn.query_row("SELECT source_id,request,result FROM creations WHERE id=?1",[&request_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if let Some((owner,body,result))=existing {
            anyhow::ensure!(owner==id && (cmd=="sources.creation_status" || body==req["fields"].to_string()),"创建请求标识不匹配");
            return result.map(|r|Ok(serde_json::from_str(&r)?)).unwrap_or_else(||Ok(json!({"ok":false,"error":"创建结果尚未确认，请先在来源平台检查，勿重复提交。","uncertain":true})));
        }
        anyhow::ensure!(cmd=="sources.create","没有找到此创建请求");
        let title=req["fields"]["title"].as_str().unwrap_or("").trim();
        anyhow::ensure!(!title.is_empty() && title.len()<=1000,"请填写任务标题（最多 1000 字节）");
        anyhow::ensure!(req["fields"].to_string().len()<=512*1024,"任务内容过大");
        store.conn.execute("INSERT INTO creations(id,source_id,request) VALUES(?1,?2,?3)",params![request_id,id,req["fields"].to_string()])?;
        let result=match api.create(&req["fields"]) {
            Ok(item)=>{store.cache(std::slice::from_ref(&item))?;json!({"ok":true,"item":item})},
            Err(error)=>json!({"ok":false,"error":format!("{error}；未自动重试，请在来源平台核对后再决定是否新建。"),"uncertain":true}),
        };
        store.conn.execute("UPDATE creations SET result=?2 WHERE id=?1",params![request_id,result.to_string()])?;
        return Ok(result);
    }
    anyhow::bail!("unsupported task source operation")
}
