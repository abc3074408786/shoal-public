//! Claim the profile before WebView2 opens it; a repeated launch only reveals its owner.
use interprocess::local_socket::Listener;
use serde_json::{json, Value};
use std::{path::Path, sync::mpsc, time::Duration};
use tauri::Manager;

const PROTOCOL: &str = "velella-desktop-activation-v1";
pub const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(15);

pub fn claim(root: &Path, timeout: Duration) -> Result<Option<Listener>, String> {
    // Normalize an existing directory even on the very first launch.
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let endpoint = velella::ipc::endpoint(root, "browser");
    match velella::ipc::listen_recovering(&endpoint) {
        Ok(listener) => Ok(Some(listener)),
        Err(bind_error) => {
            // Stream::connect itself may wait indefinitely on a busy Windows pipe.
            // Bound the complete handoff, not just the read/write portion. This is
            // only called before Tauri starts; the secondary exits after returning.
            bounded(timeout, move || {
                let response = velella::ipc::request_with_timeout(
                    &endpoint, &json!({"op":"activate_main"}), timeout,
                ).map_err(|e| e.to_string())?;
                validate_activation(&response)
            }).map_err(|error| format!("无法打开鱼群窗口：{error}。已有窗口可能仍在启动或无响应，请稍后重试。启动占用信息：{bind_error}"))?;
            Ok(None)
        }
    }
}

fn bounded<T: Send + 'static>(timeout: Duration, work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let (send, receive) = mpsc::sync_channel(1);
    std::thread::spawn(move || { let _ = send.send(work()); });
    receive.recv_timeout(timeout).map_err(|e| format!("窗口唤回未完成：{e}"))?
}

fn validate_activation(response: &Value) -> Result<(), String> {
    if response["ok"] == true && response["protocol"] == PROTOCOL
        && response["pid"].as_u64().is_some_and(|pid| pid > 0)
        && response["window"] == "main" && response["visible"] == true
        && response["minimized"] == false {
        Ok(())
    } else {
        Err(response["error"].as_str().unwrap_or("已有进程未确认主窗口已显示").into())
    }
}

pub fn reveal(app: &tauri::AppHandle) -> Result<Value, String> {
    let window = app.get_window("main").ok_or("主窗口尚未就绪")?;
    window.show().and_then(|_| window.unminimize()).map_err(|e| e.to_string())?;
    // Windows can restrict focus stealing. Visibility/restoration is required;
    // a focus restriction must not create a second GUI or terminate the owner.
    if let Err(error) = window.set_focus() { eprintln!("restore desktop focus: {error}"); }
    let response = json!({"ok":true,"protocol":PROTOCOL,"pid":std::process::id(),"window":"main",
        "visible":window.is_visible().map_err(|e| e.to_string())?,
        "minimized":window.is_minimized().map_err(|e| e.to_string())?});
    validate_activation(&response)?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_requires_a_confirmed_restored_owner() {
        let valid = json!({"ok":true,"protocol":PROTOCOL,"pid":123,"window":"main","visible":true,"minimized":false});
        assert!(validate_activation(&valid).is_ok());
        for (key, value) in [("ok",json!(false)),("protocol",json!("other")),("pid",json!(0)),
            ("window",json!("other")),("visible",json!(false)),("minimized",json!(true))] {
            let mut invalid = valid.clone(); invalid[key] = value;
            assert!(validate_activation(&invalid).is_err(), "{key}");
        }
        assert!(validate_activation(&json!({"ok":true})).is_err());
    }

    #[test]
    fn deadline_includes_a_blocked_connection() {
        let (release, wait) = mpsc::channel::<()>();
        let started = std::time::Instant::now();
        let result = bounded(Duration::from_millis(40), move || {
            wait.recv().unwrap(); Ok(())
        });
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        release.send(()).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn repeated_launch_forwards_but_other_profiles_claim_independently() {
        use interprocess::local_socket::prelude::*;
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let listener = claim(root.path(), Duration::from_secs(2)).unwrap().unwrap();
        assert!(claim(other.path(), Duration::from_secs(2)).unwrap().is_some());
        let server = std::thread::spawn(move || {
            let stream = listener.accept().unwrap();
            velella::ipc::handle(stream, |request| {
                assert_eq!(request, json!({"op":"activate_main"}));
                Ok(json!({"ok":true,"protocol":PROTOCOL,"pid":123,"window":"main","visible":true,"minimized":false}))
            });
        });
        assert!(claim(root.path(), Duration::from_secs(2)).unwrap().is_none());
        server.join().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn occupied_endpoint_without_activation_does_not_count_as_success() {
        use interprocess::local_socket::prelude::*;
        let root = tempfile::tempdir().unwrap();
        let listener = claim(root.path(), Duration::from_secs(2)).unwrap().unwrap();
        let server = std::thread::spawn(move || {
            velella::ipc::handle(listener.accept().unwrap(), |_| Ok(json!({"ok":true})));
        });
        assert!(claim(root.path(), Duration::from_secs(2)).is_err());
        server.join().unwrap();
    }
}
