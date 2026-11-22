//! Runtime-owned archive/restore and bounded, stable-membership history pages.
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryCursor {
    version: u8,
    automation: Option<String>,
    watermark: i64,
    started_at: i64,
    rowid: i64,
}

impl Capabilities {
    pub(super) fn migrate_automation_management(&self) -> Result<()> {
        // A separate table keeps removed schedules invisible to older runtimes.
        self.db.execute_batch("CREATE TABLE IF NOT EXISTS archived_automations(id TEXT PRIMARY KEY, document TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS runs_history ON runs(started_at DESC);
            CREATE INDEX IF NOT EXISTS runs_by_automation ON runs(automation_id,started_at DESC);")?;
        Ok(())
    }

    fn move_automation(&mut self, id: &str, restore: bool) -> Result<Value> {
        let (source, target) = if restore { ("archived_automations", "automations") } else { ("automations", "archived_automations") };
        let tx = self.db.transaction()?;
        let text: String = tx.query_row(&format!("SELECT document FROM {source} WHERE id=?1"), [id], |r| r.get(0))
            .optional()?.context("automation not found")?;
        let mut a: Value = serde_json::from_str(&text)?;
        a["enabled"] = json!(false);
        if restore {
            a.as_object_mut().context("automation must be an object")?.remove("archived_at");
        } else {
            a["archived_at"] = json!(chrono::Utc::now().timestamp());
        }
        tx.execute(&format!("INSERT INTO {target}(id,document) VALUES(?1,?2)"), params![id, serde_json::to_string(&a)?])?;
        tx.execute(&format!("DELETE FROM {source} WHERE id=?1"), [id])?;
        tx.commit()?;
        Ok(a)
    }

    fn history_page(&self, req: &Value) -> Result<Value> {
        let limit = req.get("limit").map(|v| v.as_u64().context("limit must be an integer")).transpose()?.unwrap_or(100);
        anyhow::ensure!((1..=500).contains(&limit), "limit must be between 1 and 500");
        let automation = req.get("automation_id").filter(|v| !v.is_null()).map(|v| v.as_str().context("automation_id must be a string")).transpose()?;
        if let Some(id) = automation { anyhow::ensure!(!id.is_empty() && id.len() <= 160, "invalid automation_id"); }
        let cursor = req.get("cursor").filter(|v| !v.is_null()).map(|v| -> Result<HistoryCursor> {
            let text = v.as_str().context("cursor must be a string")?;
            anyhow::ensure!(!text.is_empty() && text.len() <= 2048, "invalid history cursor");
            let c: HistoryCursor = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(text).context("invalid history cursor")?)?;
            anyhow::ensure!(c.version == 1 && c.automation.as_deref() == automation && c.watermark >= c.rowid && c.rowid > 0, "history cursor does not match this query");
            Ok(c)
        }).transpose()?;
        let watermark = if let Some(c) = &cursor { c.watermark } else {
            self.db.query_row("SELECT COALESCE(MAX(rowid),0) FROM runs", [], |r| r.get(0))?
        };
        let mut stmt = self.db.prepare("SELECT rowid,id,automation_id,task_id,started_at,state,error FROM runs
            WHERE rowid<=?1 AND (?2 IS NULL OR automation_id=?2)
            AND (?3 IS NULL OR started_at<?3 OR (started_at=?3 AND rowid<?4))
            ORDER BY started_at DESC,rowid DESC LIMIT ?5")?;
        let rows = stmt.query_map(params![watermark, automation, cursor.as_ref().map(|c|c.started_at), cursor.as_ref().map(|c|c.rowid), limit + 1], |r| {
            Ok((r.get::<_,i64>(0)?, json!({"id":r.get::<_,String>(1)?,"automation_id":r.get::<_,String>(2)?,"task_id":r.get::<_,String>(3)?,"started_at":r.get::<_,i64>(4)?,"state":r.get::<_,String>(5)?,"error":r.get::<_,String>(6)?})))
        })?.collect::<std::result::Result<Vec<_>,_>>()?;
        let has_more = rows.len() > limit as usize;
        let page = &rows[..rows.len().min(limit as usize)];
        let next = if has_more {
            let (rowid, last) = page.last().context("empty history page")?;
            Some(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&HistoryCursor { version:1, automation:automation.map(str::to_owned), watermark, started_at:last["started_at"].as_i64().unwrap(), rowid:*rowid })?))
        } else { None };
        Ok(json!({"ok":true,"runs":page.iter().map(|(_,r)|r).collect::<Vec<_>>(),"next_cursor":next,"has_more":has_more,"limit":limit,"snapshot_max_rowid":watermark,"state_consistency":"current_at_each_page"}))
    }
}

impl Service {
    pub(super) fn automation_management_command(&mut self, req: Value) -> Result<Value> {
        match req["cmd"].as_str().unwrap_or("") {
            "automation.archived" => Ok(json!({"ok":true,"automations":self.capabilities.documents("archived_automations")?})),
            "automation.runs" => {
                self.sync_automation_runs()?;
                self.capabilities.history_page(&req)
            },
            "automation.remove" | "automation.restore" => {
                anyhow::ensure!(req["confirmed"] == true, "请确认移除或恢复自动化；历史保留，恢复后保持禁用");
                let id = req["id"].as_str().context("id required")?;
                let restore = req["cmd"] == "automation.restore";
                if !restore {
                    let a = self.capabilities.get("automations", id)?;
                    anyhow::ensure!(!self.automation_busy(&a)?, "自动化仍有未结束任务，请先审查或停止任务");
                    let pending: bool = self.capabilities.db.query_row("SELECT EXISTS(SELECT 1 FROM runs WHERE automation_id=?1 AND state='dispatching')", [id], |r| r.get(0))?;
                    anyhow::ensure!(!pending, "自动化有未核对的派发记录，请等待后台核对后再移除");
                }
                let a = self.capabilities.move_automation(id, restore)?;
                Ok(json!({"ok":true,"automation":a,"archived":!restore,"history_retained":true}))
            },
            _ => anyhow::bail!("unknown automation management operation"),
        }
    }

    pub(super) fn sync_automation_runs(&mut self) -> Result<()> {
        // Synchronize by actual tasks, independent of the 200-row presentation cap.
        let tx = self.capabilities.db.transaction()?;
        for task in &self.tasks {
            let state = serde_json::to_value(&task.phase)?;
            tx.execute("UPDATE runs SET state=?1,error=?2 WHERE task_id=?3 AND (state<>?1 OR error<>?2)", params![state.as_str(), task.reason, task.id])?;
        }
        let interrupted = {
            let mut stmt = tx.prepare("SELECT id,automation_id FROM runs WHERE state='dispatching'")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
            rows
        };
        for (id, automation) in interrupted {
            tx.execute("UPDATE runs SET state='needs_attention',error='派发在持久化前中断；未自动重试，请核对现场后手动运行' WHERE id=?1", [&id])?;
            let text: Option<String> = tx.query_row("SELECT document FROM automations WHERE id=?1", [&automation], |r|r.get(0)).optional()?;
            if let Some(text) = text {
                let mut a: Value = serde_json::from_str(&text)?;
                a["enabled"] = json!(false);
                a["last_error"] = json!("上次派发中断，请核对任务现场后重新启用");
                tx.execute("UPDATE automations SET document=?1 WHERE id=?2", params![serde_json::to_string(&a)?,automation])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "automation_management_tests.rs"]
mod tests;
