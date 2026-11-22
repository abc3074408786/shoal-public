//! Native child surfaces follow the existing workbench's active content slots.
use serde_json::{json,Value};
use tauri::Manager;

pub fn present(app:&tauri::AppHandle,request:&Value)->Result<Value,String>{
    let frames=request["frames"].as_array().ok_or("frames required")?;
    if frames.len()>2{return Err("最多同时显示两个浏览器分屏".into());}
    let state=app.state::<super::BrowserStore>();
    let pages=state.pages.lock().map_err(|e|e.to_string())?.clone();
    let host=app.get_window("main").ok_or("工作台不可用")?;
    let scale=host.scale_factor().map_err(|e|e.to_string())?;
    let size=host.inner_size().map_err(|e|e.to_string())?;
    let mut validated=Vec::new();
    for frame in frames{
        let id=frame["id"].as_str().ok_or("page id required")?;
        if !pages.iter().any(|p|p["id"]==id&&p["workspace_id"]==frame["workspace_id"]){return Err("页面不属于当前工作区".into());}
        let number=|k:&str|frame[k].as_f64().filter(|n|n.is_finite()).ok_or_else(||format!("invalid {k}"));
        let (x,y,width,height)=(number("x")?,number("y")?,number("width")?,number("height")?);
        if x<0.||y<0.||width<1.||height<1.||x+width>size.width as f64/scale+1.||y+height>size.height as f64/scale+1.{return Err("浏览器内容区域超出工作台边界".into());}
        if validated.iter().any(|f:&Value|f["id"]==id){return Err("同一页面不能同时占用两个分屏".into());}
        validated.push(frame.clone());
    }
    let mut current=state.presentation.lock().map_err(|e|e.to_string())?;
    for page in &pages{
        let Some(id)=page["id"].as_str() else{continue};
        if let Some(view)=app.get_webview(id){
            if let Some(frame)=validated.iter().find(|f|f["id"]==id){
                view.set_bounds(tauri::Rect{position:tauri::LogicalPosition::new(frame["x"].as_f64().unwrap(),frame["y"].as_f64().unwrap()).into(),size:tauri::LogicalSize::new(frame["width"].as_f64().unwrap(),frame["height"].as_f64().unwrap()).into()}).map_err(|e|e.to_string())?;
                view.show().map_err(|e|e.to_string())?;
            }else{view.hide().map_err(|e|e.to_string())?;}
        }
    }
    *current=json!(validated);
    Ok(json!({"ok":true,"frames":*current}))
}
