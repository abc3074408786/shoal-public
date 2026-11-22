// Run with Electron. Uses the real output pump; never touches user data or PTYs.
const {app,BrowserWindow}=require('electron');
const assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),ts=require('typescript');
const out=process.env.TERMINAL_SCHEDULING_OUTPUT||path.resolve(__dirname,'../../artifacts/terminal-window-scheduling');
fs.mkdirSync(out,{recursive:true});app.setPath('userData',path.join(out,'isolated-profile'));app.enableSandbox();
const source=ts.transpileModule(fs.readFileSync(path.resolve(__dirname,'../src/terminal-output.ts'),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText.replace('export function createTerminalOutputPump','function createTerminalOutputPump');
const results=[];
app.whenReady().then(async()=>{
 await Promise.all([true,false].map(backgroundThrottling=>new Promise(async(resolve,reject)=>{
  const win=new BrowserWindow({show:false,width:240,height:160,webPreferences:{sandbox:true,nodeIntegration:false,contextIsolation:true,backgroundThrottling}});
  const deadline=setTimeout(()=>{win.destroy();reject(Error('Scheduling probe timed out'))},20000);
  win.webContents.on('console-message',event=>{
   if(!event.message.startsWith('SCHEDULING:'))return;
   results.push({backgroundThrottling,...JSON.parse(event.message.slice(11))});clearTimeout(deadline);win.destroy();resolve();
  });
  const run=`window.start=()=>{let pages=0;const start=performance.now();const pump=createTerminalOutputPump(async()=>{if(++pages===12){pump.dispose();console.log('SCHEDULING:'+JSON.stringify({ms:performance.now()-start,pages,visibility:document.visibilityState}))}return {changed:true,backlog:false}},error=>{throw error})};`;
  await win.loadURL('data:text/html,'+encodeURIComponent('<!doctype html><script>'+source+run+'</script>'));
  win.hide();await win.webContents.executeJavaScript('window.start()');
 })));
 const fixed=results.find(r=>!r.backgroundThrottling),baseline=results.find(r=>r.backgroundThrottling);
 assert.equal(baseline.visibility,'hidden');assert.equal(fixed.pages,12);
 assert.ok(baseline.ms>5000,`expected hidden-window regression, got ${baseline.ms} ms`);
 assert.ok(fixed.ms<1000,`output pump remained throttled: ${fixed.ms} ms`);
 fs.writeFileSync(path.join(out,'result.json'),JSON.stringify({status:'passed',results},null,2));console.log(JSON.stringify(results));app.exit(0);
}).catch(error=>{fs.writeFileSync(path.join(out,'result.json'),JSON.stringify({status:'failed',error:String(error),results},null,2));console.error(error);app.exit(1)});
