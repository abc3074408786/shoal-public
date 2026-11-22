#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{path::PathBuf, sync::{Arc,Mutex}};
use tauri::{Manager, menu::{Menu,MenuItem}, tray::TrayIconBuilder};
mod browser;
mod browser_cdp;
mod browser_automation;
mod browser_annotation;
mod clipboard;
mod terminal_links;
mod runtime_path;
mod instance;
mod window_bounds;
#[cfg(debug_assertions)] mod browser_ui_test;

struct Runtime { executable: PathBuf, ready: Arc<Mutex<bool>> }

#[tauri::command]
async fn rpc(webview: tauri::Webview, request: serde_json::Value, state: tauri::State<'_,Runtime>) -> Result<serde_json::Value,String> {
    if webview.label() != "main" {return Err("网页不能调用本地工作台服务".into());}
    let executable=state.executable.clone();
    let readiness=state.ready.clone();
    // All pipe I/O, including startup, runs off the async/UI executor. Only
    // startup/reconnection probes are serialized, never every keystroke/page.
    tauri::async_runtime::spawn_blocking(move|| {
        let endpoint=velella::ipc::endpoint(&velella::ipc::data_dir(),"daemon");
        let mut ready=readiness.lock().map_err(|e|e.to_string())?;
        if !*ready && velella::ipc::request(&endpoint,&serde_json::json!({"cmd":"snapshot"})).is_err() {
            let mut command=std::process::Command::new(&executable);
            command.arg("daemon").arg(velella::ipc::data_dir());
            velella::ipc::spawn_hidden(&mut command).map_err(|e|e.to_string())?;
            let until=std::time::Instant::now()+std::time::Duration::from_secs(10);
            while velella::ipc::request(&endpoint,&serde_json::json!({"cmd":"snapshot"})).is_err() {
                if std::time::Instant::now()>until {return Err("后台服务未能启动，请检查运行时安装和日志".into());}
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        *ready=true;drop(ready);
        let result=velella::ipc::request(&endpoint,&request).map_err(|e|e.to_string());
        if result.is_err(){*readiness.lock().map_err(|e|e.to_string())?=false;}
        // A request may have reached the host before a transport error. Never
        // replay it here. A subsequent request can reconnect independently.
        result
    }).await.map_err(|e|e.to_string())?
}

fn main() {
    let listener = match instance::claim(&velella::ipc::data_dir(), instance::ACTIVATION_TIMEOUT) {
        Ok(Some(listener)) => listener,
        Ok(None) => return,
        Err(error) => {
            eprintln!("{error}");
            #[cfg(windows)]
            unsafe {
                use windows::core::PCWSTR;
                use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_OK, MB_ICONERROR};
                let message: Vec<u16> = error.encode_utf16().chain(Some(0)).collect();
                let title: Vec<u16> = "鱼群启动失败".encode_utf16().chain(Some(0)).collect();
                MessageBoxW(None, PCWSTR(message.as_ptr()), PCWSTR(title.as_ptr()), MB_OK | MB_ICONERROR);
            }
            std::process::exit(1);
        }
    };
    let mut context=tauri::generate_context!();
    if std::env::var_os("VELELLA_DESKTOP_BACKGROUND").is_some_and(|v|v=="1"){
        for window in &mut context.config_mut().app.windows {window.visible=false;}
    }
    let isolated_windows=std::env::var_os("VELELLA_DATA_DIR").map(|root| {
        let windows=context.config_mut().app.windows.iter_mut().filter_map(|window| {
            if !window.create {return None;}
            let configured=window.clone();window.create=false;Some(configured)
        }).collect::<Vec<_>>();
        (PathBuf::from(root),windows)
    });
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(move |app| {
            let current=std::env::current_exe()?;
            let development=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug").join(runtime_path::RUNTIME_NAME);
            let executable=runtime_path::resolve(current.parent().unwrap(),std::env::var_os("VELELLA_RUNTIME_PATH").map(PathBuf::from),&development).map_err(std::io::Error::other)?;
            if !executable.exists(){return Err(format!("runtime missing: {}",executable.display()).into());}
            app.manage(Runtime {executable,ready:Arc::new(Mutex::new(false))});
            app.manage(browser::BrowserStore::open().map_err(std::io::Error::other)?);
            app.manage(browser_automation::BrowserAutomation::default());
            if let Some((root,windows))=&isolated_windows {
                std::fs::create_dir_all(root)?;
                let profile=std::fs::canonicalize(root)?.join("desktop-webview");
                for window in windows {
                    // Set the absolute profile through the builder, not relative-path config.
                    tauri::WebviewWindowBuilder::from_config(app,window)?.data_directory(profile.clone()).build()?;
                }
            }
            if let Some(window)=app.get_window("main") {if let Err(error)=window_bounds::fit_on_start(&window) {eprintln!("fit desktop window: {error}");}}
            browser::start_control_server(app.handle().clone(),listener);
            let title=app.get_window("main").and_then(|w|w.title().ok()).unwrap_or_else(||"Velella".into());
            let show=MenuItem::with_id(app,"show",format!("打开 {title}"),true,None::<&str>)?;
            let quit=MenuItem::with_id(app,"quit","退出桌面（任务继续运行）",true,None::<&str>)?;
            let menu=Menu::with_items(app,&[&show,&quit])?;
            let mut tray=TrayIconBuilder::new().menu(&menu).tooltip(format!("{title} · 任务在后台继续运行")).on_menu_event(|app,event|match event.id.as_ref(){
                "show"=>{
                    if let Err(error)=instance::reveal(app) {eprintln!("restore desktop window: {error}");}
                },
                "quit"=>app.exit(0),_=>{}
            });
            if let Some(icon)=app.default_window_icon(){tray=tray.icon(icon.clone());}
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|window,event|if window.label()=="main" {if let tauri::WindowEvent::CloseRequested {api,..}=event {api.prevent_close();let _=window.hide();}})
        .on_page_load(|webview,event|{if webview.label()=="main"{if let Some(state)=webview.try_state::<browser_automation::BrowserAutomation>(){match event.event(){tauri::webview::PageLoadEvent::Started=>state.page_loading("main"),tauri::webview::PageLoadEvent::Finished=>state.page_loaded("main")}}}})
        .invoke_handler(tauri::generate_handler![rpc,browser::browser_command,browser::browser_layout,clipboard::terminal_clipboard,clipboard::terminal_copy,terminal_links::terminal_open_link])
        .run(context).expect("Velella desktop runtime");
}
