#[tauri::command]
pub fn terminal_open_link(webview: tauri::Webview, url: String) -> Result<(), String> {
    if webview.label() != "main" { return Err("只有本地终端可以打开链接".into()); }
    let address = tauri::Url::parse(&url).map_err(|e| e.to_string())?;
    if !matches!(address.scheme(), "http" | "https") || address.host_str().is_none()
        || !address.username().is_empty() || address.password().is_some() {
        return Err("只允许不含账号密码的 HTTP(S) 链接".into());
    }
    #[cfg(windows)] {
        let mut command = std::process::Command::new("explorer.exe");
        command.arg(address.as_str());
        velella::ipc::spawn_hidden(&mut command).map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(windows))] { Err("当前平台没有链接打开适配".into()) }
}
