//! Observe the native catalog, disable its entries, and verify the effective
//! state. Selected snapshots are supplied as developer context. No model turn.
use super::*;
use serde::{Serialize,Deserialize};
use std::collections::BTreeSet;

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
struct Entry { path:String, name:String, enabled:bool }

// Conservatively quote every argument using the Windows backslash rules.
fn quoted_len(arg:&str)->usize {
    let mut count=2;let mut slashes=0;
    for unit in arg.encode_utf16(){match unit {92=>slashes+=1,34=>{count+=2*slashes+2;slashes=0;},_=>{count+=slashes+1;slashes=0;}}}
    count+2*slashes
}
fn bounded_command(desc:&crate::host::Descriptor)->Result<()> {
    let count=quoted_len(&desc.spawn.program)+desc.spawn.args.iter().map(|arg|quoted_len(arg)+1).sum::<usize>();
    // Leave space for the terminal activity hooks added after role setup.
    anyhow::ensure!(count<28_000,"Codex 角色配置超出 Windows 命令长度，请减少本角色技能正文或 MCP 数量；未截断或启动角色");Ok(())
}

#[cfg(windows)]
fn catalog(desc:&crate::host::Descriptor)->Result<Vec<Entry>> {
    use std::io::{BufRead,BufReader,Write};
    bounded_command(desc)?;
    let cwd=desc.spawn.cwd.as_deref().context("Codex role workspace missing")?;
    let mut command=crate::process::command(&desc.spawn.program);
    command.current_dir(cwd).envs(desc.spawn.env.iter().filter(|(key,_)|!key.starts_with("VELELLA_")));
    let mut args=desc.spawn.args.iter();
    while let Some(arg)=args.next(){if matches!(arg.as_str(),"-c"|"--config"|"-p"|"--profile"){command.arg(arg).arg(args.next().context("incomplete Codex configuration argument")?);}}
    command.arg("app-server").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child=command.spawn().context("cannot inspect native Codex skills")?;
    let group=match crate::process::ChildGroup::attach(child.id()){Ok(group)=>group,Err(error)=>{let _=child.kill();let _=child.wait();return Err(error);}};
    let mut input=child.stdin.take().context("Codex probe input missing")?;
    let output=child.stdout.take().context("Codex probe output missing")?;
    let (tx,rx)=std::sync::mpsc::channel::<Result<Value>>();
    let reader=std::thread::spawn(move||{
        let result=(||->Result<()>{
            let mut reader=BufReader::new(output);let mut total=0;
            loop{let mut line=Vec::new();let count=reader.by_ref().take(1024*1024+1).read_until(b'\n',&mut line)?;
                anyhow::ensure!(count>0,"native skill probe closed");total+=count;
                anyhow::ensure!(count<=1024*1024&&total<=4*1024*1024,"native skill catalog exceeds bounds");
                let value=serde_json::from_slice(&line).map_err(|_|anyhow::anyhow!("invalid native skill protocol"))?;
                if tx.send(Ok(value)).is_err(){return Ok(())}
            }
        })();if let Err(error)=result{let _=tx.send(Err(error));}
    });
    let deadline=Instant::now()+Duration::from_secs(10);
    let response=|id:u64|->Result<Value>{loop{
        let value=rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).map_err(|_|anyhow::anyhow!("native skill probe failed or exceeded 10 seconds"))??;
        if value["id"]==id {anyhow::ensure!(value.get("error").is_none(),"Codex rejected the native skill query");return Ok(value["result"].clone());}
        anyhow::ensure!(value.get("id").is_none(),"unexpected interactive request during native skill probe");
    }};
    let result=(||->Result<Vec<Entry>>{
        writeln!(input,"{}",json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"velella-skill-policy","version":"1"},"capabilities":{"experimentalApi":true}}}))?;input.flush()?;response(1)?;
        writeln!(input,"{}",json!({"method":"initialized"}))?;
        writeln!(input,"{}",json!({"id":2,"method":"skills/list","params":{"cwds":[cwd],"forceReload":true}}))?;input.flush()?;
        parse_catalog(&response(2)?)
    })();
    drop(input);let _=group.kill();let _=child.wait();drop(group);drop(rx);let _=reader.join();result
}
#[cfg(not(windows))]
fn catalog(_desc:&crate::host::Descriptor)->Result<Vec<Entry>>{anyhow::bail!("Codex native skill verification currently requires Windows")}

fn parse_catalog(result:&Value)->Result<Vec<Entry>> {
    let data=result["data"].as_array().context("Codex skill response omitted data")?;
    anyhow::ensure!(data.len()==1&&data[0]["errors"].as_array().is_some_and(|items|items.is_empty()),"Codex skill discovery reported an error; no role was started");
    let skills=data[0]["skills"].as_array().context("Codex skill catalog missing")?;
    anyhow::ensure!(skills.len()<=2048,"too many native Codex skills");
    let mut entries=Vec::new();let mut paths=BTreeSet::new();
    for skill in skills {
        let path=skill["path"].as_str().context("native skill path missing")?;let name=skill["name"].as_str().context("native skill name missing")?;
        anyhow::ensure!(Path::new(path).is_absolute()&&path.len()<=4096&&!name.is_empty()&&name.len()<=256&&paths.insert(path.to_owned()),"native skill identity is invalid or duplicated");
        entries.push(Entry{path:path.into(),name:name.into(),enabled:skill["enabled"].as_bool().context("native skill enabled state missing")?});
    }
    entries.sort_by(|a,b|a.path.cmp(&b.path));Ok(entries)
}
fn validate_disabled(actual:&[Entry],expected:&[Entry])->Result<()> {
    anyhow::ensure!(actual==expected&&actual.iter().all(|item|!item.enabled),"Codex 原生技能目录发生变化或仍有启用项，请新建角色会话");Ok(())
}
pub(super) fn configure(desc:&mut crate::host::Descriptor)->Result<()> {
    let mut expected=catalog(desc)?;
    // Name rules disable every same-name native entry, including duplicates
    // from distinct roots. Paths are retained in the effective recovery binding.
    let names:BTreeSet<_>=expected.iter().map(|entry|entry.name.clone()).collect();
    let rules=names.into_iter().map(|name|json!({"name":name,"enabled":false})).collect::<Vec<_>>();
    desc.spawn.args.extend(["-c".into(),format!("skills.config={}",to_toml(&json!(rules))?)]);
    for item in &mut expected{item.enabled=false;}
    bounded_command(desc)?;validate_disabled(&catalog(desc)?,&expected)?;
    desc.spawn.env.insert("VELELLA_CODEX_SKILL_CATALOG".into(),serde_json::to_string(&expected)?);Ok(())
}
pub(super) fn verify(desc:&crate::host::Descriptor)->Result<()> {
    let expected=serde_json::from_str::<Vec<Entry>>(desc.spawn.env.get("VELELLA_CODEX_SKILL_CATALOG").context("legacy Codex role lacks native skill policy; create a new session")?)?;
    bounded_command(desc)?;validate_disabled(&catalog(desc)?,&expected)
}

#[cfg(test)]mod tests {
    use super::*;
    #[test]fn disabled_catalog_rejects_enablement_new_paths_and_discovery_errors(){
        let path=std::env::temp_dir().join("SKILL.md").to_string_lossy().into_owned();
        let expected=vec![Entry{path,name:"test".into(),enabled:false}];validate_disabled(&expected,&expected).unwrap();
        let mut changed=expected.clone();changed[0].enabled=true;assert!(validate_disabled(&changed,&expected).is_err());changed[0].enabled=false;changed[0].path.push('x');assert!(validate_disabled(&changed,&expected).is_err());
        assert!(parse_catalog(&json!({"data":[{"errors":[{"message":"unreadable"}],"skills":[]}]})).is_err());
    }
    #[test]fn windows_budget_accounts_for_quotes_backslashes_and_utf16(){assert_eq!(quoted_len("abc"),5);assert_eq!(quoted_len("\\\""),6);assert_eq!(quoted_len("😀"),4);}
    #[cfg(windows)]
    #[test]#[ignore="requires an explicit installed Codex executable; no account or model call"]
    fn codex_native_denials_and_recovery(){
        let root=std::env::temp_dir().join(format!("velella-codex-policy-{}",uuid::Uuid::now_v7()));let cwd=root.join("workspace");let home=root.join("home");let codex=root.join("codex");
        for folder in [&cwd,&home,&codex]{std::fs::create_dir_all(folder).unwrap();}
        let project=cwd.join(".agents/skills/unselected");std::fs::create_dir_all(&project).unwrap();std::fs::write(project.join("SKILL.md"),"---\nname: unselected\ndescription: fixture\n---\nfixture").unwrap();
        let mut desc=crate::host::Descriptor{id:"fixture".into(),endpoint:"fixture".into(),directory:root.clone(),spawn:crate::sessions::SpawnRequest{sandbox:Default::default(),program:std::env::var("VELELLA_TEST_CODEX_EXE").unwrap(),args:vec![],cwd:Some(cwd.to_string_lossy().into_owned()),env:std::collections::HashMap::from([("CODEX_HOME".into(),codex.to_string_lossy().into_owned()),("HOME".into(),home.to_string_lossy().into_owned()),("USERPROFILE".into(),home.to_string_lossy().into_owned())]),adapter:None}};
        configure(&mut desc).unwrap();verify(&desc).unwrap();
        // Also consume the public provider setup path: full selected context,
        // native MCP binding and the persisted recovery descriptor together.
        desc.spawn.args.clear();desc.spawn.env.remove("VELELLA_CODEX_SKILL_CATALOG");
        let selected=root.join("selected");std::fs::create_dir_all(&selected).unwrap();std::fs::write(selected.join("SKILL.md"),"---\nname: chosen\ndescription: chosen fixture\n---\nChosen body must reach the developer context.").unwrap();
        let mut role:RoleDefinition=serde_json::from_value(super::super::super::orchestration::v2::seed("codex","goal")["roles"][0].clone()).unwrap();
        role.skills.push(super::super::super::orchestration::v2::Skill{path:selected.join("SKILL.md").to_string_lossy().into_owned(),digest:skill_digest(&selected).unwrap()});
        super::super::configure(&mut desc,&std::env::current_exe().unwrap(),&root,&role).unwrap();
        assert!(desc.spawn.args.iter().any(|arg|arg.starts_with("developer_instructions=")&&arg.contains("Chosen body must reach the developer context.")));
        super::super::verify_mcp_bindings(&desc).unwrap();
        let new=cwd.join(".agents/skills/new-skill");std::fs::create_dir_all(&new).unwrap();std::fs::write(new.join("SKILL.md"),"---\nname: new-skill\ndescription: fixture\n---\nfixture").unwrap();assert!(verify(&desc).is_err());
        crate::ipc::atomic_json(&root.join("result.json"),&json!({"native_disabled":true,"recovery_verified":true,"new_skill_rejected":true})).unwrap();println!("fixture evidence: {}",root.display());
    }
}
