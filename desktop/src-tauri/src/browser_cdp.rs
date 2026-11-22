//! Call the existing WebView2 directly; never expose a browser debugging port.
use serde_json::Value;

#[cfg(windows)]
pub async fn call(web: &tauri::Webview, method: &str, params: Value) -> Result<Value, String> {
    use std::{sync::{mpsc, Arc, atomic::{AtomicBool, Ordering}}, time::Duration};
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    use windows::core::HSTRING;

    let method = method.to_owned();
    let diagnostic_method=method.clone();
    let trace=std::env::var_os("VELELLA_BROWSER_TRACE").is_some_and(|v|v=="1");
    let params = params.to_string();
    let (tx, rx) = mpsc::channel();
    let expired = Arc::new(AtomicBool::new(false));
    let queued_expired = expired.clone();
    web.with_webview(move |view| {
        if queued_expired.load(Ordering::Acquire) { return; }
        if trace {eprintln!("browser CDP dispatch: {method}");}
        let completed_method=method.clone();
        let completed_tx = tx.clone();
        let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |error, result| {
            if trace {eprintln!("browser CDP completion: {completed_method}, success={}",error.is_ok());}
            let value = error.map_err(|e| e.to_string())
                .and_then(|_| serde_json::from_str::<Value>(&result).map_err(|e| e.to_string()));
            let _ = completed_tx.send(value);
            Ok(())
        }));
        let result = unsafe {
            view.controller().CoreWebView2().and_then(|core| {
                core.CallDevToolsProtocolMethod(&HSTRING::from(&method), &HSTRING::from(&params), &handler)
            })
        };
        if let Err(error) = result { let _ = tx.send(Err(error.to_string())); }
    }).map_err(|e| e.to_string())?;
    let result = tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(4)))
        .await.map_err(|e| e.to_string())?;
    expired.store(true, Ordering::Release);
    result.map_err(|error| format!("browser_timeout: {diagnostic_method}: {error}; completion is unknown; inspect the page before retrying"))?
}

#[cfg(not(windows))]
pub async fn call(_: &tauri::Webview, _: &str, _: Value) -> Result<Value, String> {
    Err("browser_unsupported: same-page automation currently requires Windows WebView2".into())
}
