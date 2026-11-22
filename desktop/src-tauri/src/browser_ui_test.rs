//! Debug-only, explicit opt-in native-rendered UI test seam; never built for release.
use crate::browser_cdp::call;
use serde_json::{json,Value};
use tauri::Manager;

pub async fn run(app:&tauri::AppHandle,request:&Value)->Result<Value,String>{
    if request["step"]=="resize"{
        let width=request["width"].as_f64().filter(|n|*n>=960.0&&*n<=1920.0).ok_or("invalid test width")?;
        let height=request["height"].as_f64().filter(|n|*n>=640.0&&*n<=1080.0).ok_or("invalid test height")?;
        app.get_window("main").ok_or("workbench missing")?.set_size(tauri::LogicalSize::new(width,height)).map_err(|e|e.to_string())?;
        return Ok(json!({"ok":true}));
    }
    if request["step"]=="reload"{app.state::<crate::browser_automation::BrowserAutomation>().page_loading("main");app.get_webview("main").ok_or("workbench missing")?.reload().map_err(|e|e.to_string())?;return Ok(json!({"ok":true}));}
    if !app.state::<crate::browser_automation::BrowserAutomation>().is_ready("main"){return Err("browser_not_ready: workbench is loading".into());}
    let web=app.get_webview("main").ok_or("workbench missing")?;
    let step=request["step"].as_str().ok_or("step required")?;
    if step=="bounds"{
        let child=app.get_webview(request["id"].as_str().ok_or("id required")?).ok_or("child missing")?;
        let position=child.position().map_err(|e|e.to_string())?;let size=child.size().map_err(|e|e.to_string())?;
        return Ok(json!({"ok":true,"x":position.x,"y":position.y,"width":size.width,"height":size.height,"scale":child.window().scale_factor().map_err(|e|e.to_string())?}));
    }
    if step=="screenshot"{let r=call(&web,"Page.captureScreenshot",json!({"format":"png"})).await?;return Ok(json!({"ok":true,"data":r["data"]}));}
    if step=="inspect"{
        let value=call(&web,"Runtime.evaluate",json!({"returnByValue":true,"expression":r#"({chat:{status:document.querySelector('.codex-chat-status')?.textContent,transcript:document.querySelector('.codex-transcript')?.textContent.slice(-12000),messages:document.querySelectorAll('.codex-message').length,requests:document.querySelectorAll('.codex-request').length},title:document.title,workspace:localStorage.getItem('velella:workspace'),tabs:[...document.querySelectorAll('.ide-tab [role=tab]')].map(e=>({text:e.textContent.trim(),active:e.getAttribute('aria-selected')})),panes:document.querySelectorAll('.wb-pane').length,browsers:[...document.querySelectorAll('[data-browser-page]')].map(e=>{const r=e.querySelector('.browser-native-slot').getBoundingClientRect();return {id:e.dataset.browserPage,x:r.x,y:r.y,width:r.width,height:r.height}}),dialogs:document.querySelectorAll('dialog[open],[role=dialog][data-state=open]').length,menus:document.querySelectorAll('[role=menu]').length,errors:[...document.querySelectorAll('[role=alert]')].map(e=>e.textContent),buttons:[...document.querySelectorAll('button')].map(e=>({text:e.textContent.trim(),aria:e.getAttribute('aria-label'),disabled:e.disabled}))})"#})).await?;
        return Ok(json!({"ok":true,"ui":value["result"]["value"]}));
    }
    let selector=request["selector"].as_str().ok_or("selector required")?;
    let params=serde_json::to_string(&json!({"selector":selector,"text":request["text"]})).map_err(|e|e.to_string())?;
    let expression=format!("(()=>{{const p={params},e=[...document.querySelectorAll(p.selector)].find(e=>!p.text||e.textContent.trim()===p.text);if(!e||e.disabled)throw Error('control unavailable');e.scrollIntoView({{block:'nearest'}});if({}==='fill'){{e.focus();e.select()}}const r=e.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+r.height/2,target=document.elementFromPoint(x,y);if(!target||!(target===e||e.contains(target)))throw Error('control covered by '+target?.outerHTML);return {{x,y}}}})()",serde_json::to_string(step).unwrap());
    let point=call(&web,"Runtime.evaluate",json!({"expression":expression,"returnByValue":true})).await?;
    if !point["exceptionDetails"].is_null(){return Err(format!("UI control unavailable: {selector}: {}",point["exceptionDetails"]["exception"]["description"]));}
    if step=="fill"{
        call(&web,"Input.insertText",json!({"text":request["value"].as_str().ok_or("value required")?})).await?;
    }else if step=="click"{
        let p=&point["result"]["value"];
        for kind in ["mouseMoved","mousePressed","mouseReleased"]{call(&web,"Input.dispatchMouseEvent",json!({"type":kind,"x":p["x"],"y":p["y"],"button":"left","clickCount":1})).await?;}
    }else{return Err("unsupported UI test step".into());}
    Ok(json!({"ok":true}))
}
