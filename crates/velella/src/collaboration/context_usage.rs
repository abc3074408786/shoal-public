//! Native context observations, never billing totals or model-supplied reports.
//! Same-user native processes share the desktop authority boundary. These are
//! provenance receipts, not protection against a malicious local process.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{io::Read, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub session_id: String,
    pub native_id: String,
    pub provider: String,
    pub source: String,
    pub basis: String,
    pub tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub percent: Option<f64>,
    pub observed_at: String,
    pub handoff_required: bool,
}

fn count(v: &Value) -> Option<u64> {
    v.as_u64().filter(|n| *n <= 1_000_000_000)
}

pub(crate) fn observe(provider: &str, session: &str, native: &str, payload: &Value, previous: Option<&Receipt>) -> Result<Receipt> {
    ensure!(!native.is_empty() && native.len() <= 256, "native usage session ID is invalid");
    let (source, basis, counts) = match provider {
        // OMP v18.12.3 ContextUsage explicitly calls tokens an estimate.
        "omp" => ("omp.getContextUsage", "native_estimate", count(&payload["tokens"]).zip(count(&payload["contextWindow"]))),
        // Claude's used_percentage excludes output. Use the documented input
        // components, not lifetime costs or a missing current_usage as zero.
        "claude-code" => {
            let usage = &payload["context_window"]["current_usage"];
            let tokens = count(&usage["input_tokens"])
                .zip(count(&usage["cache_creation_input_tokens"]))
                .zip(count(&usage["cache_read_input_tokens"]))
                .and_then(|((a,b),c)| a.checked_add(b)?.checked_add(c));
            ("claude.statusLine", "last_api_input", tokens.zip(count(&payload["context_window"]["context_window_size"])))
        }
        _ => anyhow::bail!("native context usage source is not verified for this provider"),
    };
    let counts = counts.filter(|(_,window)| *window > 0);
    let percent = counts.map(|(tokens,window)| tokens as f64 * 100.0 / window as f64);
    // Unknown samples and compaction cannot silently remove the requirement.
    let latched = previous.is_some_and(|p| p.session_id == session && p.native_id == native && p.provider == provider && p.handoff_required);
    Ok(Receipt {session_id:session.into(), native_id:native.into(), provider:provider.into(), source:source.into(), basis:basis.into(),
        tokens:counts.map(|v|v.0), context_window:counts.map(|v|v.1), percent,
        observed_at:chrono::Utc::now().to_rfc3339(), handoff_required:latched || percent.is_some_and(|v|v >= 40.0)})
}

pub(crate) fn read(directory: &Path, session: &str) -> Result<Option<Receipt>> {
    let path=directory.join("context-usage.json");
    if !path.try_exists()? {return Ok(None)}
    let meta=std::fs::symlink_metadata(&path)?;
    ensure!(meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 8192, "invalid context usage receipt");
    let mut bytes=Vec::new(); std::fs::File::open(path)?.take(8193).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 8192, "context usage receipt exceeds limit");
    let receipt:Receipt=serde_json::from_slice(&bytes).context("invalid context usage receipt")?;
    ensure!(receipt.session_id == session, "context usage session mismatch");
    Ok(Some(receipt))
}

pub(crate) fn guard(receipt: Option<&Receipt>) -> Result<()> {
    ensure!(!receipt.is_some_and(|r|r.handoff_required), "上下文已达到 40%，停止自动输入；请保存固定基线、未完成缺口和证据索引并交接到新会话");
    Ok(())
}

#[cfg(test)] mod tests {
    use super::*;
    use serde_json::json;
    #[test] fn native_context_uses_current_input_and_actual_window() {
        let r=observe("claude-code","s","n",&json!({"cost":{"total_tokens":9999999},"context_window":{"total_input_tokens":9999999,"used_percentage":1,"context_window_size":200000,"current_usage":{"input_tokens":10000,"cache_creation_input_tokens":20000,"cache_read_input_tokens":50000,"output_tokens":90000}}}),None).unwrap();
        assert_eq!(r.tokens,Some(80000)); assert_eq!(r.percent,Some(40.0)); assert!(guard(Some(&r)).is_err());
        let r=observe("omp","s","n",&json!({"tokens":39999,"contextWindow":100000,"percent":99}),None).unwrap();
        assert_eq!(r.basis,"native_estimate"); assert!(guard(Some(&r)).is_ok());
    }
    #[test] fn native_context_unknown_is_not_zero_and_latch_survives_compaction() {
        for input in [json!(null),json!({"tokens":-1,"contextWindow":100}),json!({"tokens":80,"contextWindow":0}),json!({"tokens":"80","contextWindow":100})] {
            assert_eq!(observe("omp","s","n",&input,None).unwrap().percent,None);
        }
        let high=observe("omp","s","n",&json!({"tokens":80,"contextWindow":100}),None).unwrap();
        for input in [json!(null),json!({"tokens":1,"contextWindow":100})] {assert!(observe("omp","s","n",&input,Some(&high)).unwrap().handoff_required);}
        assert!(!observe("omp","s","new",&json!(null),Some(&high)).unwrap().handoff_required);
        assert!(observe("codex","s","n",&json!({}),None).is_err());
    }
    #[test] fn native_context_persists_and_binds_session() {
        let root=std::env::temp_dir().join(format!("velella-context-{}",uuid::Uuid::now_v7()));std::fs::create_dir(&root).unwrap();
        let r=observe("omp","s","n",&json!({"tokens":50,"contextWindow":100}),None).unwrap();
        crate::ipc::atomic_json(&root.join("context-usage.json"),&r).unwrap();
        let restored=read(&root,"s").unwrap().unwrap();assert_eq!(r,restored);assert!(guard(Some(&restored)).is_err());assert!(read(&root,"other").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
