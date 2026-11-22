use crate::browser_cdp::call;
use serde_json::{json, Value};
use std::{collections::{HashMap,HashSet}, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}};
use tauri::Manager;

#[derive(Clone)]
struct NodeReference { backend: u64, fingerprint: Value }

#[derive(Clone)]
struct Snapshot {
    id: String,
    document: u64,
    context: u64,
    nodes: HashMap<String, NodeReference>,
}

#[derive(Default)]
pub struct BrowserAutomation {
    busy: Arc<AtomicBool>,
    sequence: AtomicU64,
    snapshots: Mutex<HashMap<String, Snapshot>>,
    loaded: Mutex<HashSet<String>>,
}

pub struct OperationGuard(Arc<AtomicBool>);
impl Drop for OperationGuard {
    fn drop(&mut self) { self.0.store(false, Ordering::Release); }
}

impl BrowserAutomation {
    pub fn acquire(&self) -> Result<OperationGuard, String> {
        self.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "browser_busy: another command is still running".to_owned())?;
        Ok(OperationGuard(self.busy.clone()))
    }
    pub fn invalidate(&self, page: &str) {
        if let Ok(mut snapshots) = self.snapshots.lock() { snapshots.remove(page); }
    }
    pub fn page_loading(&self,page:&str){self.invalidate(page);if let Ok(mut loaded)=self.loaded.lock(){loaded.remove(page);}}
    pub fn page_loaded(&self,page:&str){if let Ok(mut loaded)=self.loaded.lock(){loaded.insert(page.into());}}
    pub fn is_ready(&self,page:&str)->bool{self.loaded.lock().is_ok_and(|loaded|loaded.contains(page))}
}

fn required<'a>(request: &'a Value, name: &str) -> Result<&'a str, String> {
    request[name].as_str().filter(|s| !s.is_empty()).ok_or_else(|| format!("missing {name}"))
}

fn short(value: &Value) -> String {
    value.as_str().unwrap_or("").chars().take(500).collect()
}

fn fingerprint(node: &Value) -> Value {
    let mut properties=serde_json::Map::new();
    for property in node["properties"].as_array().into_iter().flatten() {
        if let Some(name)=property["name"].as_str() {
            if matches!(name,"disabled"|"readonly"|"checked"|"pressed"|"expanded"|"required"|"hasPopup") {
                properties.insert(name.into(),property["value"]["value"].clone());
            }
        }
    }
    json!({"role":node["role"]["value"],"name":node["name"]["value"],"description":node["description"]["value"],"value":node["value"]["value"],"properties":properties})
}

fn snapshot_nodes(nodes: &[Value]) -> (Vec<Value>, HashMap<String, NodeReference>) {
    let mut result = Vec::new();
    let mut references = HashMap::new();
    for node in nodes.iter().filter(|n| n["ignored"] != true).take(1000) {
        let role = short(&node["role"]["value"]);
        if matches!(role.as_str(), "none" | "InlineTextBox") { continue; }
        let mut item = json!({"role":role,"name":short(&node["name"]["value"])});
        if let Some(id) = node["backendDOMNodeId"].as_u64() {
            let reference = format!("@e{}", references.len() + 1);
            references.insert(reference.clone(), NodeReference{backend:id,fingerprint:fingerprint(node)});
            item["ref"] = json!(reference);
        }
        let protected = node["properties"].as_array().is_some_and(|p| p.iter().any(|p| {
            p["name"] == "protected" && p["value"]["value"] == true
        }));
        if !node["value"].is_null() && !protected { item["value"] = json!(short(&node["value"]["value"])); }
        result.push(item);
    }
    (result, references)
}

async fn document(web: &tauri::Webview) -> Result<u64, String> {
    call(web, "DOM.getDocument", json!({"depth":0})).await?["root"]["backendNodeId"]
        .as_u64().ok_or_else(|| "browser_not_ready: document is unavailable".into())
}

fn js_value(value: Value) -> Result<Value, String> {
    if !value["exceptionDetails"].is_null() {
        return Err("browser_element_unavailable: element changed, is hidden, disabled, or cannot accept this action; take a new snapshot".into());
    }
    Ok(value["result"]["value"].clone())
}

pub async fn operate(app: &tauri::AppHandle, page: &str, request: &Value) -> Result<Value, String> {
    let web = app.get_webview(page).ok_or("browser_page_closed: reopen this page first")?;
    let state = app.state::<BrowserAutomation>();
    let op = required(request, "op")?;
    if matches!(op,"snapshot"|"screenshot")&&!state.is_ready(page){return Err("browser_not_ready: wait for page load completion before controlling this page".into());}
    if op == "screenshot" {
        let result = call(&web, "Page.captureScreenshot", json!({"format":"png","fromSurface":true,"captureBeyondViewport":false})).await?;
        let data = result["data"].as_str().ok_or("browser_capture_failed: no PNG returned")?;
        if data.len() > 6 * 1024 * 1024 { return Err("browser_capture_too_large: reduce the viewport".into()); }
        return Ok(json!({"ok":true,"page_id":page,"mime":"image/png","data":data}));
    }
    if op == "snapshot" {
        state.invalidate(page);
        call(&web, "Accessibility.enable", json!({})).await?;
        let doc = document(&web).await?;
        let frame = call(&web, "Page.getFrameTree", json!({})).await?;
        let frame_id = frame["frameTree"]["frame"]["id"].as_str().ok_or("browser_not_ready: no main frame")?;
        let world = call(&web, "Page.createIsolatedWorld", json!({"frameId":frame_id,"worldName":"velella-browser-control","grantUniveralAccess":false})).await?;
        let context = world["executionContextId"].as_u64().ok_or("browser_not_ready: no isolated context")?;
        let metadata = js_value(call(&web, "Runtime.evaluate", json!({
            "expression":"({url:location.href,title:document.title,readyState:document.readyState})",
            "contextId":context,"returnByValue":true
        })).await?)?;
        let tree = call(&web, "Accessibility.getFullAXTree", json!({"frameId":frame_id})).await?;
        let all_nodes = tree["nodes"].as_array().ok_or("browser_not_ready: accessibility tree unavailable")?;
        let (nodes, references) = snapshot_nodes(all_nodes);
        if document(&web).await? != doc { return Err("browser_navigation_changed: take a new snapshot".into()); }
        let id = format!("s{}-{}", std::process::id(), state.sequence.fetch_add(1, Ordering::Relaxed));
        state.snapshots.lock().map_err(|e|e.to_string())?.insert(page.into(), Snapshot {id:id.clone(), document:doc, context, nodes:references});
        return Ok(json!({"ok":true,"page_id":page,"snapshot_id":id,"url":metadata["url"],"title":metadata["title"],
            "ready_state":metadata["readyState"],"scope":"main-frame","content_is_untrusted":true,"truncated":all_nodes.len()>1000,"nodes":nodes}));
    }
    let snapshot_id = required(request, "snapshot_id")?;
    let reference = required(request, "element")?;
    let snapshot = state.snapshots.lock().map_err(|e|e.to_string())?.get(page).cloned()
        .ok_or("browser_stale_ref: take a new snapshot")?;
    if !state.is_ready(page){return Err("browser_not_ready: wait for page load completion before controlling this page".into());}
    if snapshot.id != snapshot_id || document(&web).await? != snapshot.document {
        state.invalidate(page);
        return Err("browser_stale_ref: take a new snapshot".into());
    }
    let presented=app.state::<crate::browser::BrowserStore>().presentation.lock().map_err(|e|e.to_string())?.as_array().is_some_and(|frames|frames.iter().any(|f|f["id"]==page));
    if !presented{return Err("browser_not_visible: select this browser tab in the workbench before clicking or filling".into());}
    let node = snapshot.nodes.get(reference).ok_or("browser_stale_ref: reference does not belong to this snapshot")?;
    let current=call(&web,"Accessibility.getPartialAXTree",json!({"backendNodeId":node.backend,"fetchRelatives":false})).await?;
    let same=current["nodes"].as_array().is_some_and(|nodes|nodes.iter().any(|n|n["backendDOMNodeId"]==node.backend&&fingerprint(n)==node.fingerprint));
    if !same {state.invalidate(page);return Err("browser_stale_ref: element meaning or state changed; take a new snapshot".into());}
    let remote = call(&web, "DOM.resolveNode", json!({"backendNodeId":node.backend,"executionContextId":snapshot.context,"objectGroup":"velella-browser-action"})).await?;
    let object = remote["object"]["objectId"].as_str().ok_or("browser_stale_ref: element is detached")?;
    let action = act(&web, object, request).await;
    state.invalidate(page);
    let _ = call(&web, "Runtime.releaseObjectGroup", json!({"objectGroup":"velella-browser-action"})).await;
    action?;
    Ok(json!({"ok":true,"page_id":page,"action":op,"requires_snapshot":true}))
}

async fn act(web: &tauri::Webview, object: &str, request: &Value) -> Result<(), String> {
    let op = required(request, "op")?;
    let check = "if(!this.isConnected || this.disabled || this.getAttribute('aria-disabled')==='true')throw Error('unavailable');this.scrollIntoView({block:'center',inline:'center'});const r=this.getBoundingClientRect();const s=getComputedStyle(this);if(!r.width||!r.height||s.visibility==='hidden'||s.display==='none')throw Error('hidden');";
    if op == "click" {
        let function = format!("function(){{{check}const x=Math.max(0,Math.min(innerWidth,r.right)+Math.max(0,r.left))/2;const y=Math.max(0,Math.min(innerHeight,r.bottom)+Math.max(0,r.top))/2;const target=document.elementFromPoint(x,y);if(!target||!(target===this||this.contains(target)))throw Error('covered');return {{x,y}};}}");
        let point = js_value(call(web,"Runtime.callFunctionOn",json!({"objectId":object,"functionDeclaration":function,"returnByValue":true})).await?)?;
        let x=point["x"].as_f64().ok_or("browser_element_unavailable")?;
        let y=point["y"].as_f64().ok_or("browser_element_unavailable")?;
        call(web,"Input.dispatchMouseEvent",json!({"type":"mouseMoved","x":x,"y":y})).await?;
        call(web,"Input.dispatchMouseEvent",json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":1})).await?;
        call(web,"Input.dispatchMouseEvent",json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1})).await?;
    } else if op == "fill" {
        let value=request["value"].as_str().ok_or("missing value")?;
        if value.len()>65536 {return Err("browser_value_too_large".into());}
        let function=format!("function(){{{check}if(this.readOnly||!(this instanceof HTMLTextAreaElement||this instanceof HTMLInputElement&&['text','email','url','search','tel','password'].includes(this.type)))throw Error('unsupported input');this.focus();if(document.activeElement!==this)throw Error('focus failed');this.select();return true;}}");
        js_value(call(web,"Runtime.callFunctionOn",json!({"objectId":object,"functionDeclaration":function,"returnByValue":true})).await?)?;
        if value.is_empty() {
            call(web,"Input.dispatchKeyEvent",json!({"type":"keyDown","key":"Backspace","code":"Backspace","windowsVirtualKeyCode":8})).await?;
            call(web,"Input.dispatchKeyEvent",json!({"type":"keyUp","key":"Backspace","code":"Backspace","windowsVirtualKeyCode":8})).await?;
        } else { call(web,"Input.insertText",json!({"text":value})).await?; }
    } else { return Err("unknown browser automation operation".into()); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masks_password_values_and_maps_backend_nodes() {
        let (nodes, refs)=snapshot_nodes(&[
            json!({"role":{"value":"textbox"},"name":{"value":"密码"},"backendDOMNodeId":9,"value":{"value":"secret"},"properties":[{"name":"protected","value":{"value":true}}]}),
            json!({"ignored":true,"name":{"value":"hidden"}})
        ]);
        assert_eq!(nodes.len(),1); assert!(nodes[0].get("value").is_none()); assert_eq!(refs["@e1"].backend,9);
    }
    #[test]
    fn same_dom_node_with_changed_meaning_is_not_the_same_reference() {
        let original=json!({"backendDOMNodeId":3,"role":{"value":"button"},"name":{"value":"View"}});
        let changed=json!({"backendDOMNodeId":3,"role":{"value":"button"},"name":{"value":"Confirm"}});
        assert_ne!(fingerprint(&original),fingerprint(&changed));
    }
    #[test]
    fn a_new_load_revokes_ready_state_and_old_references() {
        let state=BrowserAutomation::default();assert!(!state.is_ready("one"));
        state.page_loaded("one");assert!(state.is_ready("one"));
        state.snapshots.lock().unwrap().insert("one".into(),Snapshot{id:"s1".into(),document:1,context:1,nodes:HashMap::new()});
        state.page_loading("one");assert!(!state.is_ready("one"));assert!(!state.snapshots.lock().unwrap().contains_key("one"));
        state.page_loaded("one");assert!(state.is_ready("one"));assert!(!state.snapshots.lock().unwrap().contains_key("one"));
    }
    #[test]
    fn exclusive_commands_release_on_drop_and_invalidate_only_the_page() {
        let state=BrowserAutomation::default(); let guard=state.acquire().unwrap(); assert!(state.acquire().is_err()); drop(guard); assert!(state.acquire().is_ok());
        for p in ["one","two"] {state.snapshots.lock().unwrap().insert(p.into(),Snapshot{id:p.into(),document:1,context:1,nodes:HashMap::new()});}
        state.invalidate("one"); assert!(!state.snapshots.lock().unwrap().contains_key("one")); assert!(state.snapshots.lock().unwrap().contains_key("two"));
    }
}
