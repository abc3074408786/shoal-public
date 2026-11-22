// Harness-only startup preparation. Product navigation and storage stay owned
// by the app; use its public UI once, never inject a route or saved workspace.
const assert = require('node:assert/strict');

function installEntryObserver() {
 if (window.__kbProfileEntry) return;
 const records=[];let dropped=0,stopped=false;
 const state=()=>({shellPage:document.querySelector('[data-shell-page]')?.getAttribute('data-shell-page')||null,
  workspace:localStorage.getItem('velella:workspace')});
 const target=node=>{
  const element=node instanceof Element?node.closest('button,[role="button"],input,textarea')||node:null;
  return element?{tag:element.tagName,className:String(element.className),label:element.getAttribute('aria-label')}:null;
 };
 const capture=(reason,event)=>{
  if(stopped)return;
  records.push({at:performance.now(),time:Date.now(),reason,...state(),
   isTrusted:event?.isTrusted,target:target(event?.target),button:event?.button,
   code:event?.code,documentFocus:document.hasFocus(),visibility:document.visibilityState});
  if(records.length>256){records.shift();dropped++}
 };
 let previous=JSON.stringify(state());
 const changed=()=>{const next=JSON.stringify(state());if(next!==previous){capture('state-change');previous=next}};
 const types=['pointerdown','pointerup','click','keydown','keyup','focusin','focusout','storage','velella:open-task'];
 const observe=event=>capture(event.type,event);
 for(const type of types)window.addEventListener(type,observe,true);
 const mutations=new MutationObserver(changed);
 mutations.observe(document,{subtree:true,childList:true,attributes:true,attributeFilter:['data-shell-page']});
 // Same-document setItem emits no storage event. Sample the single relevant key
 // without intercepting the application's Storage methods or changing them.
 const timer=setInterval(changed,200);
 capture('observer-attached');
 window.__kbProfileEntry={capture,finish(){
  if(!stopped){changed();capture('observer-finished');stopped=true;mutations.disconnect();clearInterval(timer);
   for(const type of types)window.removeEventListener(type,observe,true)}
  return {records,dropped,storageSampleMs:200,coverage:'from observer-attached only; isTrusted does not identify a human or automation source'};
 }};
}

async function entryState(page) {
 return page.evaluate(()=>({
  shellPage:document.querySelector('[data-shell-page]')?.getAttribute('data-shell-page')||null,
  workspace:localStorage.getItem('velella:workspace'),
  projectPath:document.querySelector('.sidebar-project-name[aria-current="page"]')?.getAttribute('title')||null,
 }));
}
const cardFor=(page,expected)=>page.getByRole('button',{name:'打开项目 '+expected.projectName,exact:true});
async function assertProfileHome(page,expected,label) {
 const state=await entryState(page);
 assert.equal(state.shellPage,'home',`PROFILE_ENTRY_CHANGED at ${label}: ${JSON.stringify(state)}`);
 assert.ok(!state.workspace||state.workspace===expected.workspaceId,`PROFILE_ENTRY_WRONG_WORKSPACE at ${label}`);
 const card=cardFor(page,expected);
 assert.ok(await card.isVisible()&&await card.isEnabled(),`PROFILE_ENTRY_NOT_READY at ${label}`);
 assert.equal(await card.locator('.project-home-info code').getAttribute('title'),expected.projectPath,'PROFILE_ENTRY_WRONG_PROJECT');
 return state;
}
async function prepareProfileHome(page,expected,record=()=>{}) {
 await page.locator('[data-shell-page]').waitFor({state:'attached'});
 const initial=await entryState(page);record({kind:'profile-entry-observed',...initial});
 let returnedHome=false;
 if(initial.shellPage==='project'){
  assert.equal(initial.workspace,expected.workspaceId,'PROFILE_ENTRY_WRONG_WORKSPACE');
  assert.equal(initial.projectPath,expected.projectPath,'PROFILE_ENTRY_WRONG_PROJECT');
  record({kind:'profile-entry-action',action:'return-home-once'});
  await page.evaluate(()=>window.__kbProfileEntry?.capture('driver-return-home'));
  await page.getByRole('button',{name:'返回项目首页',exact:true}).click();
  returnedHome=true;
 }else assert.equal(initial.shellPage,'home','PROFILE_ENTRY_UNEXPECTED_ROUTE');
 await cardFor(page,expected).waitFor();
 await page.waitForFunction(name=>{
  const card=[...document.querySelectorAll('button')].find(el=>el.getAttribute('aria-label')===name);
  return !!card&&!card.disabled;
 },'打开项目 '+expected.projectName);
 const ready=await assertProfileHome(page,expected,'prepared');
 record({kind:'profile-entry-ready',returnedHome,...ready});
 return {initial,ready,returnedHome,automaticRetry:false};
}
async function assertProfileProject(page,expected) {
 await page.locator('[data-shell-page="project"]').waitFor();
 await page.waitForFunction(id=>localStorage.getItem('velella:workspace')===id,expected.workspaceId);
 const state=await entryState(page);
 assert.equal(state.projectPath,expected.projectPath,'PROFILE_ENTRY_WRONG_PROJECT_AFTER_CLICK');
 return state;
}
module.exports={installEntryObserver,prepareProfileHome,assertProfileHome,assertProfileProject};
