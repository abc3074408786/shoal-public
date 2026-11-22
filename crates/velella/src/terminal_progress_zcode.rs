//! Exact session-bound native ZCode Todo reads. Never writes the provider DB.
use super::*;
use rusqlite::{Connection,OpenFlags,OptionalExtension};

pub(super) fn read(desc:&Descriptor,native:&str)->Result<Value>{
    let database=crate::terminal_restore::zcode_database(desc)?;
    for ancestor in database.ancestors(){anyhow::ensure!(is_plain(ancestor)?,"linked ZCode history is not read")}
    let db=Connection::open_with_flags(database,OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    read_database(desc,native,&db)
}
fn read_database(desc:&Descriptor,native:&str,db:&Connection)->Result<Value>{
    db.busy_timeout(Duration::from_millis(200))?;db.execute_batch("PRAGMA query_only=ON; BEGIN;")?;
    let session=db.query_row("SELECT directory,parent_id,title FROM session WHERE id=?1 AND time_deleted IS NULL",[native],|row|Ok((row.get::<_,String>(0)?,row.get::<_,Option<String>>(1)?,row.get::<_,String>(2)?))).optional()?;
    let Some((directory,parent,title))=session else{return Ok(response(desc,"unavailable",Some("此 ZCode 原生会话已不可用。")))};
    let cwd=dunce::canonicalize(desc.spawn.cwd.as_deref().context("saved working directory required")?)?;
    anyhow::ensure!(parent.is_none()&&same_cwd(&json!(directory),&cwd),"ZCode session identity, parent or working directory mismatch");
    let mut query=db.prepare("SELECT content,status,position,time_updated FROM todo WHERE session_id=?1 ORDER BY position LIMIT 201")?;
    let rows=query.query_map([native],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,row.get::<_,i64>(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    anyhow::ensure!(rows.len()<=MAX_STEPS,"task plan exceeds 200 steps");
    let updated=rows.iter().map(|row|row.3).max().unwrap_or(0);
    let latest_user:Option<i64>=db.query_row("SELECT max(time_created) FROM message WHERE session_id=?1 AND json_extract(data,'$.role')='user'",[native],|row|row.get(0))?;
    if rows.is_empty()||latest_user.is_some_and(|time|time>updated){return Ok(response(desc,"no_plan",Some("ZCode 尚未为最新指令更新任务清单。")))}
    let steps=rows.into_iter().map(|(content,status,position,_)|Ok(Step{id:position.to_string(),title:text(&json!(content),1000).context("task step title missing")?,status:valid_status(&status)?.into(),active_form:None,blocked_by:vec![]})).collect::<Result<Vec<_>>>()?;
    let completed=steps.iter().filter(|step|step.status=="completed").count();
    Ok(json!({"ok":true,"state":"ready","session_id":desc.id,"native_id":native,"program":desc.spawn.program,"task_label":title,"steps":steps,"completed":completed,"total":steps.len(),"reported_complete":completed==steps.len(),"source":"zcode.todo","plan_scope":"session","reported_at":chrono::DateTime::from_timestamp_millis(updated).map(|time|time.to_rfc3339()),"observed_at":chrono::Utc::now().to_rfc3339()}))
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn bound_zcode_todos_update_and_new_user_instruction_hides_stale_plan(){
        let root=std::env::temp_dir().join(format!("vl-zcode-progress-{}",uuid::Uuid::now_v7()));fs::create_dir_all(&root).unwrap();
        let desc=Descriptor{id:"local".into(),endpoint:"unused".into(),directory:root.clone(),spawn:crate::sessions::SpawnRequest{program:"zcode-free".into(),args:vec![],cwd:Some(root.to_string_lossy().into_owned()),env:Default::default(),adapter:None,sandbox:Default::default()}};
        let db=Connection::open_in_memory().unwrap();db.execute_batch("CREATE TABLE session(id TEXT,directory TEXT,parent_id TEXT,title TEXT,time_deleted INTEGER);CREATE TABLE todo(session_id TEXT,content TEXT,status TEXT,position INTEGER,time_updated INTEGER);CREATE TABLE message(session_id TEXT,time_created INTEGER,data TEXT);").unwrap();
        db.execute("INSERT INTO session VALUES ('sess_main',?1,NULL,'ZCode test',NULL)",[root.to_string_lossy().as_ref()]).unwrap();
        db.execute_batch("INSERT INTO todo VALUES ('sess_main','first','completed',0,100),('sess_main','second','in_progress',1,100),('sess_main','third','pending',2,100);INSERT INTO message VALUES ('sess_main',50,'{\"role\":\"user\"}');").unwrap();
        assert_eq!(read_database(&desc,"sess_main",&db).unwrap()["completed"],1);db.execute_batch("ROLLBACK;PRAGMA query_only=OFF;UPDATE todo SET status='completed',time_updated=200 WHERE position=1;").unwrap();
        assert_eq!(read_database(&desc,"sess_main",&db).unwrap()["completed"],2);db.execute_batch("ROLLBACK;PRAGMA query_only=OFF;INSERT INTO message VALUES ('sess_main',300,'{\"role\":\"user\"}');").unwrap();
        assert_eq!(read_database(&desc,"sess_main",&db).unwrap()["state"],"no_plan");db.execute_batch("ROLLBACK;PRAGMA query_only=OFF;UPDATE session SET parent_id='another';").unwrap();assert!(read_database(&desc,"sess_main",&db).is_err());
        drop(db);fs::remove_dir_all(root).unwrap();
    }
}
