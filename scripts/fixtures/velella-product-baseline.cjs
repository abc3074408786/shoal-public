// Opt-in extension of verify-terminal-reopen.cjs. Real product, ordinary shell.
// No DB/layout/localStorage injection, fake provider, credentials or model call.
const {spawn, execFileSync} = require('node:child_process');
const fs = require('node:fs'), path = require('node:path');
const assert = require('node:assert/strict'), crypto = require('node:crypto');
const {socketRequest} = require('../../desktop/electron/runtime.cjs');
const {waitForWritableViewer}=require('./velella-writable-viewer.cjs');
const {installEntryObserver,prepareProfileHome,assertProfileHome,assertProfileProject}=require('./velella-profile-entry.cjs');
function option(name) {
 const i = process.argv.indexOf(name);
 assert.ok(i >= 0 && process.argv[i+1], `missing ${name}`);
 return path.resolve(process.argv[i+1]);
}
const out = option('--out'), runtime = option('--runtime');
const cli = option('--cli'), electron = option('--electron');
const {_electron,chromium} = require(option('--playwright'));
const nativeShell=process.argv.includes('--native-shell')?option('--native-shell'):null;
const dormantCore=process.argv.includes('--dormant-core')?option('--dormant-core'):null;
const root = path.resolve(__dirname, '../..');
const desktopEntry = process.argv.includes('--desktop') ? option('--desktop') : path.join(root,'desktop');
const withEditor = process.argv.includes('--editor-flow');
const focusDiagnostic = process.argv.includes('--recovery-focus-diagnostic');
const verifyRecoveryFocus=process.argv.includes('--verify-recovery-focus');
const hudFlow=process.argv.includes('--hud-flow');
const hudBaseline=process.argv.includes('--hud-baseline-observation');
const desktopProfile=process.argv.includes('--desktop-memory-profile');
const viewportDiagnostic=process.argv.includes('--native-viewport-diagnostic');
const chromiumMemoryTrace=process.argv.includes('--chromium-memory-trace');
const editorWorkerProfile=process.argv.includes('--editor-worker-profile');
const expectWorkerRelease=process.argv.includes('--expect-worker-release');
const terminalGpuProfile=process.argv.includes('--terminal-gpu-profile');
const expectGpuRelease=process.argv.includes('--expect-gpu-release');
assert.ok(!(withEditor&&focusDiagnostic),'diagnostic cannot run editor acceptance');
assert.ok(!verifyRecoveryFocus||focusDiagnostic,'focus verification requires diagnostic mode');
assert.ok(!hudFlow||withEditor,'HUD verification includes core+editor');
assert.ok(!hudBaseline||!withEditor&&!hudFlow&&!focusDiagnostic,'HUD baseline observation is separate from full acceptance');
assert.ok(!desktopProfile||!withEditor&&!hudFlow&&!hudBaseline&&!focusDiagnostic,'desktop profile is separate from full acceptance');
assert.ok(!nativeShell||desktopProfile,'native shell is a feasibility profile only');
assert.ok(!viewportDiagnostic||nativeShell&&desktopProfile,'viewport diagnostic requires native profile setup');
assert.ok(!chromiumMemoryTrace||desktopProfile&&!nativeShell&&!viewportDiagnostic,'Chromium allocation trace uses the full Electron product only');
assert.ok(!editorWorkerProfile||desktopProfile&&!nativeShell&&!chromiumMemoryTrace&&!viewportDiagnostic,'editor worker profile is a separate Electron path');
assert.ok(!expectWorkerRelease||editorWorkerProfile,'worker release expectation requires its profile');
assert.ok(!terminalGpuProfile||desktopProfile&&!nativeShell&&!chromiumMemoryTrace&&!editorWorkerProfile&&!viewportDiagnostic,'terminal GPU profile is a separate Electron path');
assert.ok(!expectGpuRelease||terminalGpuProfile,'GPU release expectation requires its profile');
assert.ok(!dormantCore||desktopProfile&&!nativeShell&&!chromiumMemoryTrace&&!editorWorkerProfile&&!terminalGpuProfile&&!viewportDiagnostic,'dormancy is a separate full-desktop profile');
const data = path.join(out, 'data'), project = path.join(out, 'project');
const home = path.join(out, 'home'), temp = path.join(out, 'temp');
for (const p of [data, project, home, temp]) fs.mkdirSync(p, {recursive:true});
// Allowlist so a clean profile cannot inherit model credentials or real CLI homes.
const env = Object.fromEntries(Object.entries(process.env).filter(([key]) =>
 /^(PATH|PATHEXT|SYSTEMROOT|WINDIR|COMSPEC|SYSTEMDRIVE|PROCESSOR_ARCHITECTURE|NUMBER_OF_PROCESSORS)$/i.test(key)));
Object.assign(env, {HOME:home, USERPROFILE:home, APPDATA:path.join(home, 'roaming'),
 LOCALAPPDATA:path.join(home, 'local'), TEMP:temp, TMP:temp,
 NPM_CONFIG_PREFIX:path.join(home,'npm'),
 VELELLA_DATA_DIR:data, VELELLA_RUNTIME_PATH:runtime, VELELLA_DESKTOP_BACKGROUND:'1'});
for (const p of [env.APPDATA, env.LOCALAPPDATA]) fs.mkdirSync(p, {recursive:true});
execFileSync('git', ['init', '--quiet', project], {env, windowsHide:true});
const editorFile=path.join(project,'kb-editor.ts');
if(withEditor||nativeShell||dormantCore||chromiumMemoryTrace||editorWorkerProfile)fs.writeFileSync(editorFile,editorWorkerProfile?'export const kbEditorValue: number = "type-check";\n':'export const kbEditorValue = 1;\n');
const endpoint = JSON.parse(execFileSync(runtime, ['ipc-endpoint', data, 'daemon'], {env, encoding:'utf8', windowsHide:true}));
const result = {checks:[], realProduct:true, realShell:true, controlledProvider:false,
 realUpstreamModel:false, nativeAgentWorkloadVerified:false, layoutOrIdentityInjected:false,
 nativeCliProbes:'Product may invoke installed CLI help/version/auth-status probes in the isolated environment.'};
result.editorFlowRequested=withEditor;
result.recoveryFocusDiagnostic=focusDiagnostic;
result.hudFlowRequested=hudFlow;result.hudBaselineObservation=hudBaseline;
result.desktopMemoryProfile=desktopProfile;
result.nativeShellFeasibility=!!nativeShell;
result.nativeViewportDiagnostic=viewportDiagnostic;
result.chromiumAllocationDiagnostic=chromiumMemoryTrace;
result.editorWorkerProfile=editorWorkerProfile;result.expectedWorkerRelease=expectWorkerRelease;
result.terminalGpuProfile=terminalGpuProfile;result.expectedGpuRelease=expectGpuRelease;
result.dormancyProfile=!!dormantCore;
let daemon, app, page, wid, sid, sibling, sessionDir, nativeChild, nativeBrowser, stopViewportTrace;
let expectedProfileEntry;
let coreController;
const ended = child => child.exitCode!==null || child.signalCode!==null;
const events = path.join(out, 'events.jsonl');
function event(value) {fs.appendFileSync(events, JSON.stringify({time:Date.now()/1000, ...value})+'\n');}
function phase(name) {event({kind:'phase', name});}
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(fn, label, timeout=18000) {
 const end = Date.now()+timeout; let last;
 while (Date.now()<end) {
  try {const value = await fn(); if (value) return value;} catch (e) {last=e;}
  await delay(100);
 }
 throw Error(`${label}: ${last?.message || 'timed out'}`);
}
async function rpc(request) {
 const value = await socketRequest(endpoint, request, 10000);
 assert.equal(value.ok, true, JSON.stringify(value)); return value;
}
function invoke(...args) {
 const text = execFileSync(cli, ['--data-dir', data, '--json', ...args], {env, encoding:'utf8', windowsHide:true, timeout:20000});
 const value = JSON.parse(text.replace(/^\uFEFF/, '')); assert.equal(value.ok,true, text); return value.result;
}
const rows = async () => (await rpc({cmd:'workbench.sessions', workspace_id:wid})).sessions;
const row = async id => (await rows()).find(s => s.id === id);
const bytes = () => fs.readFileSync(path.join(sessionDir, 'terminal.bin'));
function preserve(prefix) {assert.ok(bytes().subarray(0,prefix.length).equals(prefix), 'raw history prefix changed');}
function record(name, details={}) {result.checks.push({name, ...details});}
async function startDaemon() {
 daemon = spawn(runtime, ['daemon', data], {env, windowsHide:true, stdio:['ignore','ignore',fs.openSync(path.join(out,'daemon.log'),'a')]});
 event({kind:'root', role:'daemon', pid:daemon.pid});
 let spawnError; daemon.on('error', error => {spawnError=error;});
 await until(async () => {if(spawnError) throw spawnError; return !ended(daemon) && await rpc({cmd:'snapshot'});}, 'isolated daemon ready');
}
async function stopDaemon() {
 await rpc({cmd:'service.stop'});
 await until(() => ended(daemon), 'owned daemon stopped');
}
async function desktop() {
 if(dormantCore){
  if(!coreController)coreController=await require('./velella-dormant-core.cjs').startDormantCore({binary:dormantCore,electron,desktop:desktopEntry,data,out,env,event});
  assert.equal((await coreController.status()).state,0,'wake requires actual dormant state');
  // Match Playwright's own Electron startup protocol. Read only bytes emitted
  // by this activation, rather than assume a Chromium profile-file location.
  const logFile=path.join(out,'core-electron.log');
  const logOffset=fs.existsSync(logFile)?fs.statSync(logFile).size:0;
  const active=await coreController.wake();event({kind:'root',role:'electron',pid:active.gui_pid});
  const endpoint=await until(()=>{
   if(!fs.existsSync(logFile))return false;
   const fresh=fs.readFileSync(logFile).subarray(logOffset).toString('utf8');
   const match=fresh.match(/^DevTools listening on (ws:\/\/127\.0\.0\.1:\d+\/devtools\/browser\/[0-9a-f-]+)\s*$/m);
   if(!match)return false;
   const url=new URL(match[1]),port=Number(url.port);assert.ok(port>0&&port<=65535);return url.href;
  },'fresh dormant-module CDP endpoint',30000);
  nativeBrowser=await chromium.connectOverCDP(endpoint,{timeout:20000});
  const expected=require('node:url').pathToFileURL(path.join(desktopEntry,'dist/index.html')).href;
  page=await until(()=>nativeBrowser.contexts().flatMap(c=>c.pages()).find(p=>p.url()===expected),'full desktop module page');
  await page.waitForLoadState('domcontentloaded');
  const identity=await readDormantWindow();assert.equal(identity.pid,active.gui_pid,'CDP attached to a different module process');
 }else if(nativeShell){
  const nativeEnv={...env,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=0 --remote-debugging-address=127.0.0.1'};
  nativeChild=spawn(nativeShell,[],{env:nativeEnv,windowsHide:true,stdio:['ignore','ignore',fs.openSync(path.join(out,'native-shell.log'),'a')]});
  event({kind:'root',role:'native-shell',pid:nativeChild.pid});
  nativeChild.on('error',error=>event({kind:'native-spawn-error',message:error.message}));
  const portFile=path.join(data,'desktop-webview','EBWebView','DevToolsActivePort');
  const port=await until(()=>{
   assert.ok(!ended(nativeChild),'native shell exited before CDP');
   if(!fs.existsSync(portFile))return false;
   const port=Number(fs.readFileSync(portFile,'utf8').split(/\r?\n/)[0]);
   return Number.isInteger(port)&&port>0&&port<=65535&&port;
  },'task WebView2 CDP port',30000);
  nativeBrowser=await chromium.connectOverCDP('http://127.0.0.1:'+port,{timeout:20000});
  page=await until(()=>{
   result.nativeObservedPageUrls=nativeBrowser.contexts().flatMap(c=>c.pages()).map(p=>p.url());
   return nativeBrowser.contexts().flatMap(c=>c.pages()).find(p=>/^https?:\/\/tauri\.localhost\//.test(p.url()));
  },'native Tauri page');
  result.nativeEngineVersion=nativeBrowser.version();
 }else{
 app = await _electron.launch({executablePath:electron, args:[desktopEntry], env, timeout:30000});
 event({kind:'root', role:'electron', pid:app.process().pid});
 page = await app.firstWindow();
 }
 page.setDefaultTimeout(15000);
 page.on('pageerror', e => event({kind:'pageerror', message:e.message}));
 await page.waitForLoadState('domcontentloaded');
 if(editorWorkerProfile||terminalGpuProfile||dormantCore)await page.evaluate(installEntryObserver);
 // Observe future viewer creation without changing product/session state.
 // Recovery can make the backend ready before React replaces the readonly view.
 await page.evaluate(() => {
  window.__kbBaselineViewers=[];
  window.__kbBaselineTransport=[];
  window.__kbBaselineFocus=[];
  const nodeIds=new WeakMap();let nodeSequence=0;
  const describe=node=>{
   if(!(node instanceof Element))return null;
   if(!nodeIds.has(node))nodeIds.set(node,++nodeSequence);
   return {node:nodeIds.get(node),tag:node.tagName,className:node.className,
    pane:node.closest('[data-terminal-pane]')?.getAttribute('data-terminal-pane'),connected:node.isConnected,
    visibility:getComputedStyle(node).visibility,loading:node.closest('.terminal-canvas')?.getAttribute('data-terminal-loading')};
  };
  window.__kbBaselineCaptureFocus=(reason,event)=>{
   const records=window.__kbBaselineFocus;
   records.push({at:performance.now(),reason,active:describe(document.activeElement),
    documentFocus:document.hasFocus(),visibility:document.visibilityState,
    target:describe(event?.target),code:event?.code,ctrl:event?.ctrlKey,alt:event?.altKey,meta:event?.metaKey,
    shellPage:document.querySelector('[data-shell-page]')?.getAttribute('data-shell-page'),
    viewport:{width:innerWidth,height:innerHeight}});
   if(records.length>128)records.shift();
  };
  for(const type of ['focusin','focusout','keydown','keyup'])document.addEventListener(type,event=>window.__kbBaselineCaptureFocus(type,event),true);
  window.addEventListener('velella:terminal-created', event => {
   window.__kbBaselineViewers.push(event.detail);
  });
  window.addEventListener('velella:terminal-transport',event=>{
   if(window.__kbBaselineTransport.length<1000)window.__kbBaselineTransport.push(event.detail);
  });
 });
 if(dormantCore)await page.evaluate(async()=>{await window.velellaDesktop.invoke('window',{action:'show'});await window.velellaDesktop.invoke('window',{action:'setFocus'})});
 const identity=dormantCore?await readDormantWindow():nativeShell?await activateNative():await (await app.browserWindow(page)).evaluate(window=>{window.show();window.focus();return {id:window.id,url:window.webContents.getURL(),visible:window.isVisible()}});
 assert.equal(identity.url,page.url());assert.ok(!identity.url.endsWith('/hud.html'));
 event({kind:'desktop-window',...identity});
}
async function readDormantWindow(){return page.evaluate(()=>window.velellaDesktop.invoke('window',{action:'dormancy_window'}))}
async function parkDesktop(){
 const receipt=await page.evaluate(()=>window.velellaDesktop.invoke('window',{action:'dormancy_park'}));
 assert.equal(receipt.ok,true);await until(async()=>(await coreController.status()).state===0,'checkpointed Electron module exits');
 await nativeBrowser.close().catch(()=>{});nativeBrowser=null;page=null;return receipt;
}
async function activateNative(){
 const endpoint=JSON.parse(execFileSync(runtime,['ipc-endpoint',data,'browser'],{env,encoding:'utf8',windowsHide:true}));
 const state=await socketRequest(endpoint,{op:'activate_main'},10000);
 assert.equal(state.ok,true);assert.equal(state.pid,nativeChild.pid);assert.equal(state.visible,true);assert.equal(state.minimized,false);
 return {...state,url:page.url()};
}
async function readNativeWindow(){
 const state=await page.evaluate(async()=>{
  const invoke=window.__TAURI_INTERNALS__.invoke;
  const [minimized,visible]=await Promise.all([
   invoke('plugin:window|is_minimized',{label:'main'}),
   invoke('plugin:window|is_visible',{label:'main'})]);
  return {minimized,visible,width:innerWidth,height:innerHeight};
 });
 return {...state,pid:nativeChild.pid,url:page.url()};
}
async function requireNativeVisible(label){
 if(!nativeShell&&!dormantCore&&!chromiumMemoryTrace&&!editorWorkerProfile&&!terminalGpuProfile)return;
 const state=dormantCore?await readDormantWindow():nativeShell?await readNativeWindow():await (await app.browserWindow(page)).evaluate(window=>({id:window.id,url:window.webContents.getURL(),visible:window.isVisible(),minimized:window.isMinimized()}));
 event({kind:'native-window-check',label,...state});
 if(state.minimized||!state.visible){
  result.environmentInterruption={kind:'native-window-not-visible',label,...state};
  throw Error('NATIVE_WINDOW_INTERRUPTED: test window was minimized or hidden; no automatic restore/retry; foreground memory comparison is invalid');
 }
}
async function closeDesktop() {
 if(dormantCore){
  if(page&&!page.isClosed())await page.evaluate(()=>window.velellaDesktop.invoke('window',{action:'dormancy_quit'}));
  if(coreController){await until(async()=>(await coreController.status()).state===0,'owned desktop module quit');result.dormancyCoreMessages=coreController.messages;await coreController.stop();}
  await nativeBrowser?.close().catch(()=>{});nativeBrowser=null;page=null;result.desktopCleanup='graceful task-owned module quit and dormant native core stop';return;
 }
 if((editorWorkerProfile||terminalGpuProfile)&&page&&!result.entryObservation)result.entryObservation=await page.evaluate(()=>window.__kbProfileEntry?.finish()).catch(()=>null);
 if(page){result.windowFocusTraces??=[];result.windowFocusTraces.push(await page.evaluate(()=>({url:location.href,records:window.__kbBaselineFocus})).catch(()=>null))}
 if(nativeShell){
  if(nativeBrowser)await nativeBrowser.close();nativeBrowser=null;
  if(nativeChild&&!ended(nativeChild)){
   event({kind:'diagnostic-shell-termination',pid:nativeChild.pid});
   nativeChild.kill();await until(()=>ended(nativeChild),'task shell terminated');
  }
  result.desktopCleanup='explicit task-owned native-shell termination; not graceful lifecycle acceptance';
  page=null;return;
 }
 await app.close(); app=null; page=null;
}
async function usable(requireNewViewer=false) {
 await page.locator('.xterm-helper-textarea').first().waitFor({timeout:20000});
 await until(async () => {
  const s=await row(sid);
  return s?.status?.exit===null && s.status.input_protocol===1 && !await page.locator('.terminal-loading').count() && s;
 }, 'live terminal input protocol');
 return waitForWritableViewer(page,sid,{requireViewer:requireNewViewer});
}
async function selected() {
 const layout=(await rpc({cmd:'workbench.layout',workspace_id:wid})).layout;
 assert.ok(layout.panes.some(p => p.tabs.some(t => t.id===p.active && t.target===sid)), 'wrong active session');
 return layout;
}
async function probe(label, requireNewViewer=false) {
 const handle=await usable(requireNewViewer); await selected();
 await requireNativeVisible('before-terminal-input');
 if(!nativeShell&&!dormantCore&&!chromiumMemoryTrace&&!editorWorkerProfile&&!terminalGpuProfile)await page.bringToFront();
 {
  const input=handle.asElement();assert.ok(input,'writable session input missing');
  await input.focus();
  assert.ok(await input.evaluate(element=>element===document.activeElement&&document.hasFocus()),'writable input is not focused');
  await handle.dispose();
 }
 event({kind:'keyboard-target',label,requireNewViewer,...await page.evaluate(()=>({hasDocumentFocus:document.hasFocus(),textareaCount:document.querySelectorAll('.xterm-helper-textarea').length,activeClass:document.activeElement?.className}))});
 await executeKeyboardMarker(label);
}
async function executeKeyboardMarker(label){
 const marker='VELELLA_PRODUCT_'+crypto.randomBytes(12).toString('hex');
 const backwards=[...marker].reverse().join('');
 const command=`Write-Output (-join ('${backwards}'.ToCharArray()[${marker.length-1}..0]))`;
 assert.ok(!command.includes(marker));
 const before=bytes().length, old=bytes();
 await page.keyboard.type(command);
 await page.evaluate(()=>window.__kbBaselineCaptureFocus?.('after-keyboard-type'));
 await page.keyboard.press('Enter');
 await page.evaluate(()=>window.__kbBaselineCaptureFocus?.('after-keyboard-enter'));
 await until(() => bytes().subarray(before).includes(Buffer.from(marker)), 'executed keyboard marker');
 preserve(old);
 const output=await rpc({cmd:'workbench.terminal.output',workspace_id:wid,session_id:sid,since:before});
 const raw=bytes();
 const chunk=Buffer.from(output.data,'base64');
 assert.ok(chunk.includes(Buffer.from(marker)), 'RPC lacks executed marker');
 assert.ok(chunk.equals(raw.subarray(before,before+chunk.length)), 'RPC bytes differ from terminal.bin');
 record(label, {session_id:sid, marker, raw_length:raw.length, output_sha256:crypto.createHash('sha256').update(chunk).digest('hex')});
}
async function checkSibling(expected) {
 const s=await row(sibling);
 assert.equal(s.status.pid, expected.status.pid); assert.equal(s.status.exit,null);
 assert.equal(s.status.input_sequence, expected.status.input_sequence);
}
async function diagnoseRecoveryFocus(){
 const {installRecoveryFocusTrace}=require('./velella-recovery-focus.cjs');
 await page.evaluate(installRecoveryFocusTrace);
 result.focusBackend=[{phase:'initial',session:await row(sid)}];
 const oldHost=(await row(sid)).status.pid,prefix=bytes();
 phase('diagnostic-stop');
 await rpc({cmd:'workbench.terminal.stop',workspace_id:wid,session_id:sid,confirmed:true});
 await until(async()=>(await row(sid))?.recoverable,'diagnostic shell recoverable');
 result.focusBackend.push({phase:'stopped',session:await row(sid)});
 await page.getByRole('button',{name:'重新启动终端',exact:true}).waitFor();
 await page.evaluate(()=>window.__kbFocusTrace.capture('before-recovery-click',true));
 await page.getByRole('button',{name:'重新启动终端',exact:true}).click();
 await until(async()=>{const s=await row(sid);return s?.status?.exit===null&&s.status.pid!==oldHost},'diagnostic new host');
 const handle=await usable(true);await selected();await page.bringToFront();
 result.focusBackend.push({phase:'recovered-before-focus',session:await row(sid)});
 phase('diagnostic-single-focus');
 const input=handle.asElement();assert.ok(input,'diagnostic new textarea absent');
 await page.evaluate(()=>window.__kbFocusTrace.capture('before-single-focus',true));
 await input.focus();
 await page.evaluate(()=>window.__kbFocusTrace.capture('after-single-focus',true));
 for(const ms of [100,400,1500]){await delay(ms);await page.evaluate(()=>window.__kbFocusTrace.capture('settled-focus',true))}
 if(verifyRecoveryFocus){
  assert.ok(await input.evaluate(element=>element===document.activeElement&&document.hasFocus()),'recovered input lost focus');
  await executeKeyboardMarker('focused recovery sends one new marker to the new host and preserves exact raw history');
  result.recoveryFocusVerified=true;
 }
 result.focusBackend.push({phase:'after-focus-observation',session:await row(sid)});
 result.focusTrace=await page.evaluate(()=>window.__kbFocusTrace.finish());
 await handle.dispose();preserve(prefix);
 result.focusDiagnosticCollected=true;
 fs.writeFileSync(path.join(out,'focus-diagnostic.json'),JSON.stringify({backend:result.focusBackend,trace:result.focusTrace},null,2));
 await page.screenshot({path:path.join(out,'focus-diagnostic.png')});
}
async function verifyEditor(untouched){
 phase('editor-first-use');const history=bytes();
 const expand=page.getByRole('button',{name:'展开工具栏',exact:true});
 if(await expand.isVisible())await expand.click();
 await page.getByRole('tab',{name:'文件',exact:true}).click();
 await page.locator('button.wb-file-row[title="kb-editor.ts"]').click();
 const editor=page.locator('.wb-editor .monaco-editor textarea.inputarea');
 await editor.waitFor();
 await until(async()=> (await page.locator('.wb-editor .view-lines').innerText()).includes('kbEditorValue'),'real source editor loaded');
 const marker='VELELLA_FILE_'+crypto.randomBytes(12).toString('hex');
 await editor.focus();await page.keyboard.press('Control+End');await page.keyboard.type('\n// '+marker+'\n');
 await page.locator('.wb-editor .file-save').click();
 await until(()=>fs.readFileSync(editorFile,'utf8').includes(marker),'real edited file saved');
 preserve(history);await checkSibling(untouched);await delay(1500);
 const owner=await app.browserWindow(page);let captureTimer,editorImage;
 try{editorImage=await Promise.race([
  owner.evaluate(async window=>{
   const image=await window.webContents.capturePage();
   if(image.isEmpty())throw Error('native editor screenshot is empty');
   return image.toPNG().toString('base64');
  }),
  new Promise((_,reject)=>{captureTimer=setTimeout(()=>reject(Error('native editor screenshot timed out')),20000)})
 ])}finally{clearTimeout(captureTimer)}
 fs.writeFileSync(path.join(out,'editor.png'),Buffer.from(editorImage,'base64'));
 await closeDesktop();await desktop();
 await page.locator('.wb-editor .monaco-editor textarea.inputarea').waitFor();
 await until(async()=> (await page.locator('.wb-editor .view-lines').innerText()).includes(marker),'saved file reopened');
 record('real file-tree editor opens, accepts keyboard, saves through RPC and restores saved content after Electron reopen',{
  file:editorFile,file_marker:marker,sha256:crypto.createHash('sha256').update(fs.readFileSync(editorFile)).digest('hex')});
 phase('after-editor-use');
 const layout=(await rpc({cmd:'workbench.layout',workspace_id:wid})).layout;
 const terminalTab=layout.panes.flatMap(pane=>pane.tabs).find(tab=>tab.kind==='terminal'&&tab.target===sid);
 assert.ok(terminalTab,'original terminal tab missing after editing');
 await page.getByRole('tablist',{name:'工作区标签',exact:true}).getByRole('tab',{name:terminalTab.title,exact:true}).click();
 await probe('return from saved editor continues the original terminal with intact raw history');
 preserve(history);await checkSibling(untouched);await delay(1500);
 result.actualSourceEditingVerified=true;
}
async function windowInventory(){
 if(dormantCore)return [await readDormantWindow()];
 if(nativeShell)return [await readNativeWindow()];
 return app.evaluate(({BrowserWindow})=>BrowserWindow.getAllWindows().map(win=>({id:win.id,url:win.webContents.getURL(),visible:win.isVisible(),focused:win.isFocused(),pid:win.webContents.getOSProcessId()})));
}
async function profileDesktop(label,windowState='visible'){
 const checkPlannedState=async()=>{
  const state=await (await app.browserWindow(page)).evaluate(win=>({visible:win.isVisible(),minimized:win.isMinimized()}));
  event({kind:'planned-window-check',label,expected:windowState,...state});
  assert.ok(windowState==='hidden'?!state.visible&&!state.minimized:windowState==='minimized'?state.minimized:state.visible&&!state.minimized,'Unexpected window state at '+label);
 };
 assert.ok(windowState==='visible'||terminalGpuProfile,'only GPU profile permits planned hidden checkpoints');
 if(terminalGpuProfile)await checkPlannedState();
 else if(nativeShell||dormantCore||chromiumMemoryTrace||editorWorkerProfile)await requireNativeVisible('profile-'+label+'-start');
 else{
  await (await app.browserWindow(page)).evaluate(window=>{window.show();window.focus()});
  await page.bringToFront();
 }
 phase('profile-'+label);await delay(1500);
 if((editorWorkerProfile||terminalGpuProfile||dormantCore)&&label==='home')result.profileEntry=await prepareProfileHome(page,expectedProfileEntry,event);
 const gpu=dormantCore?await page.evaluate(()=>window.velellaDesktop.invoke('window',{action:'dormancy_metrics'})):nativeShell?{engine:'WebView2',version:nativeBrowser.version(),processMetrics:'see whole-tree observer'}:await app.evaluate(async({app})=>{
  const info=await app.getGPUInfo('complete');
  const {glRenderer,glVendor,displayType,glImplementationParts,sandboxed,skiaBackendType}=info.auxAttributes||{};
  return {features:app.getGPUFeatureStatus(),aux:{glRenderer,glVendor,displayType,glImplementationParts,sandboxed,skiaBackendType},devices:info.gpuDevice,
   processes:app.getAppMetrics().map(({pid,type,memory})=>({pid,type,memory}))};
 });
 const renderer=await page.evaluate(()=>{
  const terminals=(window.__kbBaselineViewers||[]).filter(x=>x.terminal.element?.isConnected).map(({terminal,sessionId})=>{
   const renderer=terminal._core?._renderService?._renderer?.value;
   const gl=renderer?._gl,ext=gl?.getExtension('WEBGL_debug_renderer_info');
   return {sessionId,cols:terminal.cols,rows:terminal.rows,
    implementation:terminal.element.closest('[data-terminal-renderer]')?.getAttribute('data-terminal-renderer'),
    gpu:ext?gl.getParameter(ext.UNMASKED_RENDERER_WEBGL):null,
    canvas:renderer?._canvas?{width:renderer._canvas.width,height:renderer._canvas.height}:null,
    atlasPages:renderer?._charAtlas?.pages?.map(p=>({width:p.canvas.width,height:p.canvas.height})),
    bufferLength:terminal.buffer.active.length};
  });
  return {width:innerWidth,height:innerHeight,dpr:devicePixelRatio,terminals,
   canvases:[...document.querySelectorAll('canvas')].map(c=>({width:c.width,height:c.height,connected:c.isConnected})),
   domNodes:document.querySelectorAll('*').length};
 });
 const cdp=await page.context().newCDPSession(page);let heap;
 try{heap=await cdp.send('Runtime.getHeapUsage')}finally{await cdp.detach()}
 if(terminalGpuProfile)await checkPlannedState();else await requireNativeVisible('profile-'+label+'-capture');
 const windows=await windowInventory();if(windowState==='visible')assert.ok(windows.find(win=>win.url===page.url())?.visible,'profile main window not visible');
 result.desktopProfiles??=[];result.desktopProfiles.push({label,gpu,renderer,heap,windows});
 if((editorWorkerProfile||terminalGpuProfile||dormantCore)&&label==='home')await assertProfileHome(page,expectedProfileEntry,'before-home-capture');
 // Capturing a hidden window could itself allocate/repaint GPU surfaces.
 if(windowState==='visible')await page.screenshot({path:path.join(out,'profile-'+label+'.png')});
 if(chromiumMemoryTrace&&['home','terminal-settled','editor'].includes(label)){
  const trace=await require('./velella-chromium-memory.cjs').captureChromiumMemory(app,page,out,label);
  result.chromiumMemoryTraces??=[];result.chromiumMemoryTraces.push(trace);
  await requireNativeVisible('trace-'+label+'-end');
 }
 await delay(1500);
}
async function nativeEditorProfile(untouched){
 phase('native-editor-first-use');const history=bytes();
 const expand=page.getByRole('button',{name:'展开工具栏',exact:true});if(await expand.isVisible())await expand.click();
 await page.getByRole('tab',{name:'文件',exact:true}).click();
 await page.locator('button.wb-file-row[title="kb-editor.ts"]').click();
 const editor=page.locator('.wb-editor .monaco-editor textarea.inputarea');await editor.waitFor();
 await until(async()=>(await page.locator('.wb-editor .view-lines').innerText()).includes('kbEditorValue'),'native source editor loaded');
 const marker='VELELLA_NATIVE_FILE_'+crypto.randomBytes(12).toString('hex');
 await editor.focus();await page.keyboard.press('Control+End');await page.keyboard.type('\n// '+marker+'\n');
 await page.locator('.wb-editor .file-save').click();
 await until(()=>fs.readFileSync(editorFile,'utf8').includes(marker),'native edited file saved');
 const workerUrls=()=>page.workers().filter(worker=>/ts[._]worker/.test(worker.url())).map(worker=>worker.url());
 if(editorWorkerProfile){
  await page.locator('.wb-editor .squiggly-error').first().waitFor();
  assert.ok(workerUrls().length>0,'real TS worker missing before idle');
  result.editorWorkerLifecycle={before:workerUrls()};
 }
 await profileDesktop('editor');
 const layout=(await rpc({cmd:'workbench.layout',workspace_id:wid})).layout;
 const tab=layout.panes.flatMap(p=>p.tabs).find(t=>t.kind==='terminal'&&t.target===sid);assert.ok(tab);
 await page.getByRole('tablist',{name:'工作区标签',exact:true}).getByRole('tab',{name:tab.title,exact:true}).click();
 if(editorWorkerProfile){
  await usable();phase('editor-worker-idle-wait');await delay(3500);
  await requireNativeVisible('after-editor-idle');
  result.editorWorkerLifecycle.afterIdle=workerUrls();
  if(expectWorkerRelease)assert.equal(workerUrls().length,0,'candidate retained unused TS worker');
  else assert.ok(workerUrls().length>0,'control worker unexpectedly disappeared');
  await profileDesktop('editor-idle');
 }
 await probe('native editor save returns to original terminal with exact raw output');
 preserve(history);await checkSibling(untouched);
 await page.locator('button.wb-file-row[title="kb-editor.ts"]').click();
 await until(async()=>(await page.locator('.wb-editor .view-lines').innerText()).includes(marker),'native editor saved content visible on reopen');
 if(editorWorkerProfile){
  await page.locator('.wb-editor .squiggly-error').first().waitFor();
  assert.ok(workerUrls().length>0,'reopened editor did not recreate the language service');
  result.editorWorkerLifecycle.afterReopen=workerUrls();
  await profileDesktop('editor-reopened');
 }
 record('native real file editing/save/tab reopen and original terminal return',{file:editorFile,file_marker:marker,sha256:crypto.createHash('sha256').update(fs.readFileSync(editorFile)).digest('hex')});
 result.actualSourceEditingVerified=true;
}
async function observeOldHud(){
 const inventory=await windowInventory();
 const toggle=await page.evaluate(async()=>{try{await window.velellaDesktop.invoke('hud',{action:'toggle'});return {accepted:true}}catch(error){return {accepted:false,error:String(error)}}});
 assert.equal(inventory.length,2);assert.ok(inventory.some(win=>!win.visible&&!win.url));assert.equal(toggle.accepted,false);
 result.oldHud={inventory,toggle,notEquivalentFunctionalBaseline:true};result.hudBaselineObservationCollected=true;
 record('old HUD eagerly allocates a blank window and rejects the main-window toggle; observation only',result.oldHud);
}
async function verifyHud(untouched){
 const history=bytes();phase('hud-before-first-use');
 const initial=await windowInventory();assert.equal(initial.length,1,'unused HUD allocated a window');
 const deny=await page.evaluate(async()=>{try{await window.velellaDesktop.invoke('hud',{action:'ready'});return false}catch{return true}});assert.equal(deny,true,'main gained HUD-only ready authority');
 // A deterministic local resource failure must surface, then wait for human retry.
 await app.evaluate(({session})=>session.defaultSession.webRequest.onBeforeRequest({urls:['file:///*']},(details,callback)=>callback({cancel:details.url.endsWith('/hud.html')})));
 await page.getByRole('button',{name:'任务监视窗',exact:true}).click();
 await page.locator('div.ide-error[role="alert"]').filter({hasText:'任务监视窗打开失败：'}).waitFor();
 await until(async()=>(await windowInventory()).length===1,'failed HUD disposed without auto retry');
 await app.evaluate(({session})=>session.defaultSession.webRequest.onBeforeRequest(null));
 phase('hud-first-use');
 const windowPromise=app.waitForEvent('window');
 await page.getByRole('button',{name:'任务监视窗',exact:true}).click();
 const hudPage=await windowPromise;await hudPage.waitForURL('**/hud.html');
 await hudPage.locator('.hud-dot.online').waitFor();await hudPage.getByText('暂无进行中的任务。',{exact:true}).waitFor();
 await until(async()=>(await windowInventory()).some(win=>win.url.endsWith('/hud.html')&&win.visible),'loaded HUD visible');
 assert.ok(await page.evaluate(()=>document.hasFocus()),'HUD stole main document focus');
 assert.equal(await app.evaluate(({BrowserWindow})=>BrowserWindow.getFocusedWindow()?.webContents.getURL()),page.url(),'HUD stole native focus');
 const forbidden=await hudPage.evaluate(async()=>{const results=[];for(const action of ['update','toggle']){try{await window.velellaDesktop.invoke('hud',{action,digest:{}});results.push(false)}catch{results.push(true)}}return results});assert.deepEqual(forbidden,[true,true]);
 const stamp=await hudPage.locator('.hud-footer small').innerText();
 await until(async()=>(await hudPage.locator('.hud-footer small').innerText())!==stamp,'real main digest keeps updating');
 const shownId=(await windowInventory()).find(win=>win.url.endsWith('/hud.html')).id;
 await hudPage.screenshot({path:path.join(out,'hud.png')});
 await hudPage.getByRole('button',{name:'隐藏监视窗',exact:true}).click();
 phase('hud-hidden');await until(async()=>(await windowInventory()).some(win=>win.url.endsWith('/hud.html')&&!win.visible),'HUD hidden');await delay(1200);
 await page.getByRole('button',{name:'任务监视窗',exact:true}).click();
 await until(async()=>(await windowInventory()).some(win=>win.url.endsWith('/hud.html')&&win.visible),'same HUD reopened');
 const reopened=await windowInventory();assert.equal(reopened.length,2);assert.equal(reopened.find(win=>win.url.endsWith('/hud.html')).id,shownId,'HUD recreated unexpectedly after hide/reopen');
 await hudPage.getByRole('button',{name:'打开主窗口',exact:true}).click();
 await page.getByRole('button',{name:'任务监视窗',exact:true}).click();
 phase('after-hud-use');await probe('HUD load failure, explicit retry, live digest, hide/reopen preserve original terminal input');
 preserve(history);await checkSibling(untouched);await delay(1200);
 result.actualHudVerified=true;record('real HUD first use/failure recovery/live updates/hide-reopen/main focus and IPC authority',{initial_windows:initial,final_windows:await windowInventory(),hud_forbidden_actions_rejected:forbidden});
}
(async () => {
 try {
  phase('daemon-only'); await startDaemon(); await delay(1000);
  const p=invoke('project','add',project).project;
  wid=invoke('workspace','list','--project',p.id).workspaces[0].id;
  expectedProfileEntry={projectName:p.name,projectPath:p.path,workspaceId:wid};
  phase('two-shells-before-desktop');
  sid=invoke('terminal','create','--workspace',wid).session_id;
  sibling=invoke('terminal','create','--workspace',wid).session_id;
  sessionDir=path.join(project,'.velella','sessions',sid);
  await until(async () => (await row(sibling))?.status?.input_protocol===1, 'second terminal ready');
  const untouched=await row(sibling);
  phase('desktop-two-shells'); await desktop();
  if(viewportDiagnostic)stopViewportTrace=await require('./velella-native-viewport.cjs').startNativeViewportTrace(page,out);
  if(desktopProfile)await profileDesktop('home');
  await requireNativeVisible('before-project-click');
  if(viewportDiagnostic)phase('viewport-project-click');
  if(editorWorkerProfile||terminalGpuProfile||dormantCore){
   await assertProfileHome(page,expectedProfileEntry,'before-project-click');
   await page.evaluate(()=>window.__kbProfileEntry?.capture('driver-open-project'));
  }
  await page.getByRole('button',{name:'打开项目 '+p.name,exact:true}).click();
  if(editorWorkerProfile||terminalGpuProfile||dormantCore){
   await assertProfileProject(page,expectedProfileEntry);
   result.entryObservation=await page.evaluate(()=>window.__kbProfileEntry.finish());
  }
  await requireNativeVisible('before-history-click');
  // A live session with no saved tab belongs to history, not the current list.
  if(viewportDiagnostic){
   phase('viewport-history-click');
   try{await page.getByRole('button',{name:/^历史会话/}).first().click();result.historyClickCompleted=true}
   catch(error){result.historyClickCompleted=false;result.observedFlowError=error.stack}
   await delay(500);
   result.viewportTrace=await stopViewportTrace();stopViewportTrace=null;
   assert.ok(result.viewportTrace.count>=4,'insufficient viewport observations');
   await page.screenshot({path:path.join(out,'viewport-final.png')});
   result.nativeViewportObservationCollected=true;result.ok=true;return;
  }
  await page.getByRole('button',{name:/^历史会话/}).first().click();
  await page.getByRole('button',{name:/^终端记录/}).click();
  await page.locator(`[data-session-id="${sid}"]`).click();
  await page.getByRole('button',{name:'打开终端',exact:true}).click();
  await probe('project card and existing session UI open; keyboard execution and exact raw output');
  if(desktopProfile){
   await profileDesktop('terminal');await delay(3000);await profileDesktop('terminal-settled');
   if(dormantCore){
    await require('./velella-dormancy-flow.cjs').runDormancyFlow({getPage:()=>page,park:parkDesktop,wake:desktop,core:coreController,sid,wid,row,rows,bytes,rpc,until,usable,probe,preserve,checkSibling,untouched,profileDesktop,phase,record,result,out,editorFile});
   }else if(terminalGpuProfile){
    result.gpuWorkspace=wid;
    await require('./velella-terminal-gpu.cjs').runTerminalGpuProfile({app,page,sid,row,bytes,rpc,until,usable,probe,preserve,checkSibling,untouched,profileDesktop,record,result,expectRelease:expectGpuRelease});
   }else if(nativeShell||chromiumMemoryTrace||editorWorkerProfile)await nativeEditorProfile(untouched);
   result.desktopMemoryProfileCollected=true;result.ok=true;return;
  }
  if(hudBaseline){await observeOldHud();result.ok=true;return;}
  if(focusDiagnostic){await diagnoseRecoveryFocus();result.ok=true;return;}
  const host=(await row(sid)).status.pid;
  record('initial process identities',{daemon_pid:daemon.pid,host_pid:host,sibling_host_pid:untouched.status.pid});
  await page.screenshot({path:path.join(out,'project.png')}); await delay(1200);

  phase('settings-return');
  await page.keyboard.press('Control+,');
  await page.getByRole('button',{name:'返回工作界面',exact:true}).click();
  assert.equal((await row(sid)).status.pid,host);
  await probe('settings return keeps same host and accepts keyboard'); await checkSibling(untouched);

  phase('electron-reopen'); const prefix=bytes(), layout=await selected();
  await closeDesktop(); await desktop();
  await probe('Electron reopen reattaches same session and accepts keyboard');
  assert.equal((await row(sid)).status.pid,host); preserve(prefix);
  assert.deepEqual(await selected(),layout); await checkSibling(untouched); await delay(1000);

  phase('daemon-restart'); const durable=bytes();
  await closeDesktop();
  // service.stop intentionally stops sessions. Simulate an interrupted daemon
  // only: its independent hosts must survive, then reconnect to the new daemon.
  event({kind:'owned-daemon-interruption',pid:daemon.pid});
  daemon.kill(); await until(() => ended(daemon),'owned daemon interrupted');
  await startDaemon();
  assert.equal((await row(sid)).status.pid,host); preserve(durable);
  assert.deepEqual((await rpc({cmd:'workbench.layout',workspace_id:wid})).layout,layout);
  await desktop(); await probe('daemon restart preserves host, durable bytes, layout and keyboard path');
  await checkSibling(untouched);

  phase('explicit-recovery'); const recoveryPrefix=bytes();
  await rpc({cmd:'workbench.terminal.stop',workspace_id:wid,session_id:sid,confirmed:true});
  await until(async () => (await row(sid))?.recoverable, 'owned stopped shell recoverable');
 await page.evaluate(() => {window.__kbBaselineViewers=[];});
  await page.getByRole('button',{name:'重新启动终端',exact:true}).click();
  await until(async () => {const s=await row(sid); return s?.status?.exit===null && s.status.pid!==host;}, 'explicit recovery new host');
  await probe('explicit UI recovery keeps original session ID/history and executes new input', true);
  preserve(recoveryPrefix); await checkSibling(untouched);
  assert.equal((await rows()).length,2,'recovery created extra session');
  await page.screenshot({path:path.join(out,'recovered.png')}); await delay(1200);
  if(withEditor)await verifyEditor(untouched);
  if(hudFlow)await verifyHud(untouched);
  result.ok=true;
 } catch (error) {
  result.ok=false; result.error=error.stack; process.exitCode=1;
  if(nativeShell&&page){
   const state=await readNativeWindow().catch(()=>null);
   if(state&&(state.minimized||!state.visible))result.environmentInterruption??={kind:'native-window-not-visible',label:'failure',...state};
  }
  if (page) {
   result.ui=await page.locator('body').innerText().catch(()=>null);
   result.terminalTransport=await page.evaluate(()=>window.__kbBaselineTransport).catch(()=>null);
   result.failureFocus=await page.evaluate(()=>{window.__kbBaselineCaptureFocus?.('failure');return window.__kbBaselineFocus}).catch(()=>null);
   if(focusDiagnostic)result.focusTrace=await page.evaluate(()=>window.__kbFocusTrace?.finish()).catch(()=>null);
   await page.screenshot({path:path.join(out,'failure.png')}).catch(()=>{});
  }
 } finally {
  phase('cleanup');
  result.cleanup_errors=[];
  if(stopViewportTrace)await stopViewportTrace().then(value=>{result.viewportTrace=value}).catch(error=>result.cleanup_errors.push(error.message));
  if (app||nativeChild||coreController) await closeDesktop().catch(e=>result.cleanup_errors.push(e.message));
  if (daemon && !ended(daemon)) {
   if (wid) for (const id of [sid,sibling].filter(Boolean)) {
    await rpc({cmd:'workbench.terminal.stop',workspace_id:wid,session_id:id,confirmed:true}).catch(e=>result.cleanup_errors.push(e.message));
   }
   await stopDaemon().catch(e=>result.cleanup_errors.push(e.message));
  }
  result.sessions={workspace:wid,primary:sid,secondary:sibling};
  if (result.cleanup_errors.length) {result.ok=false; process.exitCode=1;}
  fs.writeFileSync(path.join(out,'workflow.json'), JSON.stringify(result,null,2));
  event({kind:'finished',ok:result.ok});
  console.log(JSON.stringify({ok:result.ok,checks:result.checks.length,error:result.error,out}));
 }
})().catch(error => {console.error(error.stack);process.exitCode=1;});
