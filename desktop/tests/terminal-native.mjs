// Drives the packaged Release WebView2 over its isolated, loopback-only CDP
// endpoint. No browser preview, no production data and no existing processes.
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {mkdirSync,writeFileSync,readFileSync,existsSync,copyFileSync,statSync,appendFileSync} from 'node:fs';
import {join,resolve} from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const {chromium}=createRequire(import.meta.url)('playwright');
const exec=promisify(execFile),sleep=ms=>new Promise(r=>setTimeout(r,ms));
const root=resolve(fileURLToPath(new URL('../..',import.meta.url))),label=process.argv[2]||'candidate';
const transport=process.argv.includes('--transport');
const faultsOnly=process.argv.includes('--faults-only');
const agentsOnly=process.argv.includes('--agents-only');
const renderer=process.env.VELELLA_TEST_RENDERER||'auto';
const evidence=join(root,'artifacts/terminal-stability'),appDir=join(evidence,label+'-app');
const dir=join(evidence,`${label}-native-${Date.now()}`),data=join(dir,'data'),project=join(dir,'fixture');
mkdirSync(data,{recursive:true});mkdirSync(project);
await exec('git',['init','--quiet',project],{windowsHide:true});
copyFileSync(join(root,'scripts/terminal-load-fixture.py'),join(project,'load.py'));
const runtime=join(appDir,'velella-runtime.exe'),guiPath=join(appDir,'velella-desktop.exe');
assert.ok(existsSync(runtime)&&existsSync(guiPath));
const port=label==='baseline'?19548:19549;
const env={...process.env,VELELLA_DATA_DIR:data,VELELLA_RUNTIME_PATH:runtime,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port} --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows`};
env.VELELLA_DESKTOP_BACKGROUND='1';
env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS+=' --disable-features=CalculateNativeWinOcclusion';
const children=[],result={label,profile:'release',workload:transport?'exact byte streams after ConPTY (two fixture hosts, six real PTYs)':'eight real PTYs; ConPTY coalesces producer bytes',dir,checks:[],metrics:{},started:new Date().toISOString()};
result.binaryHashes=Object.fromEntries([runtime,guiPath].map(path=>[path,createHash('sha256').update(readFileSync(path)).digest('hex')]));
result.rendererPreference=renderer;
let browser,page,workspace,sessionIds=[];const timers=[];
const logSize=i=>statSync(join(project,'.velella/sessions',sessionIds[i],'terminal.bin')).size;
const save=()=>writeFileSync(join(dir,'result.json'),JSON.stringify(result,null,2));
const rpc=async request=>{const {stdout}=await exec(runtime,['rpc',data,JSON.stringify(request)],{env,windowsHide:true,timeout:55000,maxBuffer:4*1024*1024});const response=JSON.parse(stdout);if(!response.ok)throw Error(response.error);return response};
const probe=async(mode,pids)=>JSON.parse((await exec('python',[join(root,'scripts/terminal-process-probe.py'),mode,dir,JSON.stringify(pids)],{windowsHide:true,timeout:10000})).stdout);
const start=(exe,args=[])=>{const child=spawn(exe,args,{env,windowsHide:true,stdio:'ignore'});children.push(child);return child};
async function waitFor(fn,timeout=15000){const until=Date.now()+timeout;let error;while(Date.now()<until){try{const value=await fn();if(value)return value}catch(e){error=e}await sleep(50)}throw error||Error('wait timed out')}
// xterm's public buffer works for both DOM and WebGL. Inspect visible rows
// only. This measures buffer observation, not a physical pixel timestamp.
async function screenContains(text){return page.evaluate(text=>window.nativeProbe.terms.some(({term})=>{
 const box=term.element.getBoundingClientRect();if(!box.width||!box.height)return false;
 const buffer=term.buffer.active;let value='';for(let row=buffer.viewportY;row<buffer.viewportY+term.rows;row++)value+=(buffer.getLine(row)?.translateToString(true)||'')+'\n';
 return value.includes(text);
}),text)}
async function typeSamples(identity,count){const times=[];await page.waitForFunction(identity=>[...document.querySelectorAll('[role="tab"][aria-selected="true"]')].some(e=>e.textContent==='terminal '+identity),identity);const area=page.locator('.wb-pane').nth(identity<4?0:1).locator('.xterm-helper-textarea');await area.focus();for(let n=0;n<count;n++){const seq=(result.metrics['sequence'+identity]||0)+1;const start=performance.now();await area.press('a');await waitFor(()=>screenContains(`ECHO_${identity}_${seq}`),5000);times.push(performance.now()-start);result.metrics['sequence'+identity]=seq}return times}
const percentile=(values,q)=>[...values].sort((a,b)=>a-b)[Math.min(values.length-1,Math.floor(values.length*q))];
try{
 let daemon=start(runtime,['daemon',data]);await waitFor(()=>rpc({cmd:'snapshot'}));
 const p=(await rpc({cmd:'project.add',path:project})).project;assert.equal(resolve(p.path).toLowerCase(),resolve(project).toLowerCase(),'fixture must not import an ancestor checkout');workspace='main:'+p.id;
 for(let i=0;i<8;i++){
  const s=await rpc({cmd:'workbench.terminal.open',workspace_id:workspace});sessionIds.push(s.session_id);
  writeFileSync(join(project,`control-${i}.json`),'{}');
  await rpc({cmd:'workbench.terminal.input',workspace_id:workspace,session_id:s.session_id,input_kind:'user',text:`python -u '${join(project,'load.py')}' '${join(project,`control-${i}.json`)}' ${i}`,enter:true});
 }
 if(transport)for(const i of [0,4]){
  await rpc({cmd:'workbench.terminal.stop',workspace_id:workspace,session_id:sessionIds[i],confirmed:true});
  await waitFor(async()=>(await rpc({cmd:'workbench.sessions',workspace_id:workspace})).sessions.find(s=>s.id===sessionIds[i]).recoverable);
  start('python',[join(root,'scripts/terminal-stream-fixture.py'),join(project,'.velella/sessions',sessionIds[i],'launch.json'),join(project,`control-${i}.json`),String(i)]);
  await waitFor(async()=>(await rpc({cmd:'workbench.sessions',workspace_id:workspace})).sessions.find(s=>s.id===sessionIds[i]).status?.exit===null);
 }
 let layout=(await rpc({cmd:'workbench.layout',workspace_id:workspace})).layout;
 layout.panes=[0,1].map(p=>({id:'pane'+p,active:'t'+p*4,tabs:sessionIds.slice(p*4,p*4+4).map((s,i)=>({id:'t'+(p*4+i),target:s,kind:'terminal',title:'terminal '+(p*4+i)}))}));layout.axis='horizontal';layout.ratio=.5;
 await rpc({cmd:'workbench.layout.save',workspace_id:workspace,layout});
 let gui=start(guiPath);await waitFor(async()=>{const r=await fetch(`http://127.0.0.1:${port}/json/version`);return r.ok},20000);
 browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
 // WebView's CDP endpoint appears before Tauri setup registers its control pipe.
 const browserStatus=await waitFor(async()=>JSON.parse((await exec(runtime,['browser',data,JSON.stringify({op:'status'})],{env,windowsHide:true,timeout:10000})).stdout),20000);
 assert.equal(browserStatus.ok,true);assert.equal(browserStatus.backend,'webview2');assert.equal(resolve(browserStatus.runtime_path),resolve(runtime));result.checks.push('integrated browser still resolves the registered runtime state');
 result.targets=browser.contexts().flatMap(c=>c.pages()).map(p=>p.url());save();
 page=await waitFor(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>/tauri|localhost/.test(p.url())));
 await page.addInitScript(({workspace,renderer})=>{
  localStorage.setItem('velella:workspace',workspace);localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer,cursorBlink:false}));
  const probe=window.nativeProbe={created:0,disposed:0,terms:[],longTasks:[],rpc:{},ranges:{},rangeErrors:0};
  window.addEventListener('velella:terminal-transport',event=>{const data=event.detail,values=probe.rpc[data.command]??=[];values.push(data.elapsedMs);if(values.length>2048)values.shift();
   if(data.command==='terminal.output'&&!data.failed){const previous=probe.ranges[data.sessionId]??0;if(data.since!==previous||data.from!==data.since)probe.rangeErrors++;probe.ranges[data.sessionId]=data.end;}
  });
  window.addEventListener('velella:terminal-created',event=>{
   const term=event.detail.terminal;probe.created++;const entry={id:event.detail.sessionId,term,written:0};probe.terms.push(entry);
   const write=term.write.bind(term);term.write=(data,callback)=>{entry.written+=typeof data==='string'?new TextEncoder().encode(data).length:data.byteLength;return write(data,callback)};
   const dispose=term.dispose.bind(term);term.dispose=()=>{probe.disposed++;probe.terms=probe.terms.filter(item=>item.term!==term);dispose()};
  });
  new PerformanceObserver(list=>{probe.longTasks.push(...list.getEntries().map(e=>e.duration));if(probe.longTasks.length>1024)probe.longTasks.splice(0,probe.longTasks.length-1024)}).observe({entryTypes:['longtask']});
 },{workspace,renderer});
 const cdp=await page.context().newCDPSession(page);
 await cdp.send('Emulation.setDeviceMetricsOverride',{width:1440,height:900,deviceScaleFactor:1.5,mobile:false});
 await cdp.send('Emulation.setFocusEmulationEnabled',{enabled:true});
 await page.reload();await waitFor(()=>screenContains('READY_0'),20000);await waitFor(()=>screenContains('READY_4'));
 result.measurementCondition='background native WebView2; 1440x900 CSS viewport, DPI 1.5; background throttling disabled; public xterm buffer observation, includes controller polling overhead';
 result.renderers=await page.evaluate(()=>window.nativeProbe.terms.map(({id,term})=>({id,cols:term.cols,rows:term.rows,renderer:term.element.parentElement.dataset.terminalRenderer})));
 assert.ok(result.renderers.filter(t=>t.renderer==='webgl').length>=2,'this hardware-specific run requires default auto to select WebGL in both visible panes');
 await page.screenshot({path:join(dir,'native-ready.png')});
 result.checks.push('eight sessions; two rendered panes; dedicated WebView2 profile');save();
 if(!faultsOnly&&!agentsOnly){
 const idle=await typeSamples(0,40);result.metrics.idleP95=percentile(idle,.95);result.metrics.idleSamples=idle;save();
 // Materialize all eight views, park six, and count actual native output RPCs.
 for(let i=0;i<8;i++)await page.getByRole('tab',{name:'terminal '+i,exact:true}).click();
 await page.getByRole('tab',{name:'terminal 0',exact:true}).click();
 await page.getByRole('tab',{name:'terminal 4',exact:true}).click();
 await sleep(3000);
 result.metrics.idlePolling=[];
 for(let round=0;round<5;round++){
  await page.evaluate(()=>{window.nativeIdleReads={};window.nativeIdleListener=e=>{if(e.detail.command==='terminal.output')window.nativeIdleReads[e.detail.sessionId]=(window.nativeIdleReads[e.detail.sessionId]||0)+1};window.addEventListener('velella:terminal-transport',window.nativeIdleListener)});
  await sleep(2000);
  const counts=await page.evaluate(()=>{window.removeEventListener('velella:terminal-transport',window.nativeIdleListener);return window.nativeIdleReads});
  result.metrics.idlePolling.push({round,windowMs:2000,counts,hiddenReads:sessionIds.filter((_,i)=>i!==0&&i!==4).reduce((n,id)=>n+(counts[id]||0),0)});
 }
 save();
 const loadPids=(await rpc({cmd:'workbench.sessions',workspace_id:workspace})).sessions.map(s=>s.status?.pid).filter(Boolean);
 const resourcesBefore=await probe('sample',[gui.pid,daemon.pid,...loadPids]);const backlogSamples=[];
 let sampleBusy=false;const backlogTimer=setInterval(async()=>{if(sampleBusy)return;sampleBusy=true;try{backlogSamples.push({at:Date.now(),rows:await page.evaluate(()=>[...document.querySelectorAll('.terminal-canvas')].map(e=>({offset:Number(e.dataset.terminalOffset||0),total:Number(e.dataset.terminalTotal||0)})))});}catch{}finally{sampleBusy=false}},1000);
 timers.push(backlogTimer);
 const beforeLoad=[0,4].map(logSize),until=Date.now()/1000+60;for(const i of [0,4])writeFileSync(join(project,`control-${i}.json`),JSON.stringify({until}));
 const loaded=await typeSamples(0,40);result.metrics.loadedP95=percentile(loaded,.95);result.metrics.loadedSamples=loaded;save();
 await sleep(Math.max(0,until*1000-Date.now()));
 await waitFor(()=>screenContains('LOAD_DONE_0'),60000);
 clearInterval(backlogTimer);result.metrics.backlogSamples=backlogSamples;
 const resourcesAfter=await probe('sample',[gui.pid,daemon.pid,...loadPids]);result.metrics.resources={before:resourcesBefore,after:resourcesAfter,loadCpuSeconds:resourcesAfter.cpuSeconds-resourcesBefore.cpuSeconds};
 result.metrics.recordedLoadBytes=[0,4].map((i,n)=>logSize(i)-beforeLoad[n]);save();
 const started=performance.now();writeFileSync(join(project,'control-0.json'),JSON.stringify({burst:1}));
 await waitFor(()=>screenContains('BURST_DONE_0'),60000);result.metrics.burstMs=performance.now()-started;save();
 const switches=[];
 for(let n=0;n<100;n++){const i=n%8;const start=performance.now();await page.getByRole('tab',{name:'terminal '+i,exact:true}).click();await waitFor(()=>screenContains(i===0?'BURST_DONE_0':i===4?'READY_4':'READY_'+i));switches.push(performance.now()-start)}
 result.metrics.switchP95=percentile(switches,.95);result.metrics.switchSamples=switches;
 result.metrics.lifecycle=await page.evaluate(()=>({created:window.nativeProbe.created,disposed:window.nativeProbe.disposed,longTasks:window.nativeProbe.longTasks}));
 if(label==='candidate'){
  await waitFor(()=>page.evaluate(()=>window.nativeProbe.terms.every(({term,written})=>written===Number(term.element.parentElement.dataset.terminalOffset||0))));
  const integrity=await page.evaluate(()=>({rangeErrors:window.nativeProbe.rangeErrors,terms:window.nativeProbe.terms.map(({id,term,written})=>({id,written,offset:Number(term.element.parentElement.dataset.terminalOffset||0)})),rpc:window.nativeProbe.rpc}));
  result.metrics.transportIntegrity=integrity;assert.equal(integrity.rangeErrors,0);assert.equal(result.metrics.lifecycle.created,8);assert.equal(result.metrics.lifecycle.disposed,0);
  assert.ok(integrity.rpc['terminal.output']?.length,'native RPC waits were measured');
  for(const item of integrity.terms){assert.equal(item.written,item.offset,'every consumed byte was written to xterm');assert.equal(item.offset,logSize(sessionIds.indexOf(item.id)),'the entire log was consumed');}
  writeFileSync(join(project,'control-2.json'),JSON.stringify({history:1}));
  await page.getByRole('tab',{name:'terminal 2',exact:true}).click();await waitFor(()=>screenContains('HISTORY_DONE_2'));
  await page.evaluate(id=>{const term=window.nativeProbe.terms.find(t=>t.id===id).term;if(term.buffer.active.baseY<100)throw Error('history fixture did not create scrollback');term.scrollToLine(20)},sessionIds[2]);
  await page.getByRole('tab',{name:'terminal 3',exact:true}).click();await waitFor(()=>page.getByRole('tab',{name:'terminal 3',exact:true}).getAttribute('aria-selected').then(v=>v==='true'));
  await page.getByRole('tab',{name:'terminal 2',exact:true}).click();await waitFor(()=>page.getByRole('tab',{name:'terminal 2',exact:true}).getAttribute('aria-selected').then(v=>v==='true'));
  assert.equal(await page.evaluate(id=>window.nativeProbe.terms.find(t=>t.id===id).term.buffer.active.viewportY,sessionIds[2]),20,'native scroll position survives cached view switching');
  result.checks.push('native long history keeps its scroll position without recreating xterm');
 }
 result.hosts=(await rpc({cmd:'workbench.sessions',workspace_id:workspace})).sessions.map(s=>({id:s.id,pid:s.status?.pid}));
 result.metrics.thresholds={idle:result.metrics.idleP95<=100,loaded:result.metrics.loadedP95<=150,burst:result.metrics.burstMs<=5000,switch:result.metrics.switchP95<=200};
 if(label==='candidate')result.metrics.thresholds.hiddenIdleReads=result.metrics.idlePolling.every(sample=>sample.hiddenReads<=12);
 if(transport)result.metrics.thresholds.byteRate=result.metrics.recordedLoadBytes.every(bytes=>bytes>=500*1024*60);
 result.status=Object.values(result.metrics.thresholds).every(Boolean)?'performance_pass':'performance_fail';save();
 console.log(JSON.stringify({dir,status:result.status,metrics:{idleP95:result.metrics.idleP95,loadedP95:result.metrics.loadedP95,burstMs:result.metrics.burstMs,switchP95:result.metrics.switchP95}}));
 }else{result.status=agentsOnly?'cli_running':'faults_running';result.hosts=(await rpc({cmd:'workbench.sessions',workspace_id:workspace})).sessions.map(s=>({id:s.id,pid:s.status?.pid}));save();}
 const hosts=async()=>(await rpc({cmd:'workbench.sessions',workspace_id:workspace})).sessions;
 const identities=()=>result.hosts.map(s=>s.pid).filter(Boolean);
 if(process.argv.includes('--faults')||faultsOnly){
  const victim=result.hosts.find(s=>s.id===sessionIds[7]).pid;
  await probe('suspend',[victim]);
  try{
   const stalled=rpc({cmd:'workbench.terminal.input',workspace_id:workspace,session_id:sessionIds[7],input_kind:'user',text:'a',enter:false}).then(()=>false,()=>true);
   await page.getByRole('tab',{name:'terminal 0',exact:true}).click();
   const samples=await typeSamples(0,5);result.metrics.stalledHostOtherInputP95=percentile(samples,.95);
   assert.ok(await stalled,'stalled input must report an ambiguous transport failure');assert.ok(result.metrics.stalledHostOtherInputP95<=150);
  }finally{await probe('resume',[victim])}
  result.checks.push('a suspended host times out without blocking another terminal');save();
  await page.evaluate(()=>{localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'webgl',cursorBlink:false}));window.dispatchEvent(new Event('velella:terminal-preferences'))});
  await page.waitForFunction(()=>document.querySelector('[data-terminal-renderer="webgl"]'));
  await page.evaluate(()=>{for(const {term} of window.nativeProbe.terms){const gl=term._core._renderService._renderer.value._gl;if(gl)gl.getExtension('WEBGL_lose_context').loseContext()}});
  await page.waitForFunction(()=>!document.querySelector('[data-terminal-renderer="webgl"]'));
  result.checks.push('native WebGL context loss falls back to DOM without a new PTY');
  await page.evaluate(()=>{localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom',cursorBlink:false}));window.dispatchEvent(new Event('velella:terminal-preferences'))});
  await browser.close();gui.kill();await sleep(1200);gui=start(guiPath);
  await waitFor(async()=>{const response=await fetch(`http://127.0.0.1:${port}/json/version`);return response.ok});
  browser=await chromium.connectOverCDP(`http://127.0.0.1:${port}`);page=await waitFor(()=>browser.contexts().flatMap(c=>c.pages()).find(p=>/tauri|localhost/.test(p.url())));
  await page.getByRole('tab',{name:'terminal 0',exact:true}).waitFor();
  assert.deepEqual((await hosts()).map(s=>s.status?.pid),result.hosts.map(s=>s.pid));result.checks.push('GUI process restart reconnects the same eight host PIDs');
  daemon.kill();await waitFor(async()=>{try{const response=await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('rpc',{request:{cmd:'snapshot'}}));return response.ok}catch{return false}},15000);
  assert.deepEqual((await hosts()).map(s=>s.status?.pid),result.hosts.map(s=>s.pid));result.checks.push('daemon process restart reconnects the same eight host PIDs');
  const before=logSize(7);await probe('terminate',[victim]);
  await waitFor(async()=>(await hosts()).find(s=>s.id===sessionIds[7]).recoverable===true);
  assert.ok(logSize(7)>=before);result.checks.push('confirmed host death is explicitly recoverable and retains history');
  await rpc({cmd:'workbench.terminal.recover',workspace_id:workspace,session_id:sessionIds[7]});
  await rpc({cmd:'workbench.terminal.input',workspace_id:workspace,session_id:sessionIds[7],input_kind:'user',text:`python -u '${join(project,'load.py')}' '${join(project,'control-7.json')}' 7`,enter:true});
  const restored=(await hosts()).find(s=>s.id===sessionIds[7]);assert.notEqual(restored.status.pid,victim);assert.ok(logSize(7)>=before);
  result.hosts=(await hosts()).map(s=>({id:s.id,pid:s.status?.pid}));result.checks.push('explicit recovery restarts only the dead descriptor');save();
  if(faultsOnly){result.status='faults_pass';save();console.log(JSON.stringify({dir,status:result.status,checks:result.checks}));}
 }
 if(process.argv.includes('--replay')){
  await page.getByRole('tab',{name:'terminal 0',exact:true}).click();
  const until=Date.now()/1000+60;
  writeFileSync(join(project,'control-0.json'),JSON.stringify({replay_until:until}));
  const samples=await typeSamples(0,40);
  await sleep(Math.max(0,until*1000-Date.now()));
  await waitFor(()=>screenContains('REPLAY_DONE_0'),10000);
  result.metrics.aiReplay={durationSeconds:60,echoSamples:samples,echoP95:percentile(samples,.95),completed:true};
  await page.screenshot({path:join(dir,'ai-replay.png')});save();
 }
 if(process.argv.includes('--agents')||agentsOnly){
  result.agents=[];
  for(const [i,name,suffix] of [[3,'codex','CODEX_OK'],[7,'claude','CLAUDE_OK']]){
   // A dedicated fresh Shell prevents a workload fixture from consuming the
   // CLI command when console Ctrl+C has not actually returned to PowerShell.
   const cliSession=(await rpc({cmd:'workbench.terminal.open',workspace_id:workspace})).session_id;
   const cliLayout=(await rpc({cmd:'workbench.layout',workspace_id:workspace})).layout;
   const tabId='cli-'+name;
   cliLayout.panes[0].tabs.push({id:tabId,target:cliSession,kind:'terminal',title:'CLI '+name});
   cliLayout.panes[0].active=tabId;
   await rpc({cmd:'workbench.layout.save',workspace_id:workspace,layout:cliLayout});
   await page.reload(); // External RPC layout writes are not live UI events.
   await page.getByRole('tab',{name:'CLI '+name,exact:true}).click();
   const send=(text,enter=false)=>rpc({cmd:'workbench.terminal.input',workspace_id:workspace,session_id:cliSession,input_kind:'user',text,enter});
   const check=await rpc({cmd:'agent.check',agent:name==='claude'?'claude-code':'codex'});
   const receipt={agent:name,sessionId:cliSession,version:check.version,available:check.available,authenticated:check.authenticated,reply:false,fixtureTrust:false};result.agents.push(receipt);save();
   if(!check.available||!check.authenticated){receipt.blocked='CLI installation or existing login unavailable';continue}
   // The expected token never appears literally in the submitted instruction,
   // so an echoed command/prompt cannot masquerade as a model reply.
   await send(`${name} 'Reply only with the concatenation of VELELLA_ and ${suffix}. Do not run tools or change files.'`,true);
   const until=Date.now()+90000;
   while(Date.now()<until){
    const text=await page.evaluate(id=>{const term=window.nativeProbe.terms.find(t=>t.id===id)?.term;if(!term)return '';const buffer=term.buffer.active;let value='';for(let row=buffer.viewportY;row<buffer.viewportY+term.rows;row++)value+=(buffer.getLine(row)?.translateToString(true)||'')+'\n';return value},cliSession);
    if(text.includes('VELELLA_'+suffix)){receipt.reply=true;break}
    if(!receipt.fixtureTrust&&/trust.{0,120}(folder|directory)|Yes, I trust/i.test(text)){receipt.fixtureTrust=true;await send('',true)}
    if(/connection refused|unable to connect|connection error|error connecting|authentication failed|invalid api key|HTTP 50[234]/i.test(text)){receipt.blocked='Configured provider returned a connection or authentication error';break}
    await sleep(500);
   }
   if(!receipt.reply&&!receipt.blocked)receipt.blocked='No verified model reply within 90 seconds; inspect the isolated terminal';
   await page.screenshot({path:join(dir,name+'-cli.png')});save();
  }
  result.checks.push('real Codex/Claude terminal probes recorded separately from fixture load tests');save();
  result.realCliStatus=result.agents.every(agent=>agent.reply)?'reply_pass':'incomplete';
  if(agentsOnly)result.status=result.realCliStatus==='reply_pass'?'cli_pass':'cli_incomplete';
  result.realCliMeasurement='Real model replies only; provider wait is excluded from terminal performance ranking';save();
 }
 const soakHours=Number(process.env.VELELLA_SOAK_HOURS||0);
 if(soakHours>0){
  assert.equal(transport,false,'the soak requires eight real PTYs');
  assert.equal(result.status,'performance_pass','do not soak a performance-failing candidate');
  const start=Date.now(),end=start+soakHours*3600000,samples=[];
  const cdp=await page.context().newCDPSession(page);
  for(let i=0;i<8;i++)await page.getByRole('tab',{name:'terminal '+i,exact:true}).click();
  result.status='soak_running';result.soak={hours:soakHours,started:new Date(start).toISOString(),expectedEnd:new Date(end).toISOString(),samples:0};save();
  console.log(JSON.stringify({soak:result.soak,receipt:join(dir,'result.json')}));
  while(Date.now()<end){
   for(const i of [samples.length%4,4+samples.length%4]){await page.getByRole('tab',{name:'terminal '+i,exact:true}).click();await waitFor(()=>page.getByRole('tab',{name:'terminal '+i,exact:true}).getAttribute('aria-selected').then(v=>v==='true'));}
   await page.getByRole('tab',{name:'terminal 0',exact:true}).click();const latency=await typeSamples(0,1);
   const current=await hosts();assert.deepEqual(current.map(s=>s.status?.pid),result.hosts.map(s=>s.pid),'host identity changed during soak');
   const metrics=await probe('sample',[gui.pid,daemon.pid,...identities()]);
   const terminalResources=await page.evaluate(()=>({created:window.nativeProbe.created,disposed:window.nativeProbe.disposed,renderers:window.nativeProbe.terms.map(({term})=>({visible:term.element.isConnected,renderer:term.element.parentElement.dataset.terminalRenderer,canvases:term.element.querySelectorAll('canvas').length}))}));
   assert.equal(terminalResources.created,8);assert.equal(terminalResources.disposed,0);
   const sample={elapsedMs:Date.now()-start,at:new Date().toISOString(),echoMs:latency[0],...metrics,terminalResources,dom:await cdp.send('Memory.getDOMCounters')};samples.push(sample);appendFileSync(join(dir,'soak-samples.jsonl'),JSON.stringify(sample)+'\n');
   if(samples.length%15===0){for(const i of [0,4])writeFileSync(join(project,`control-${i}.json`),JSON.stringify({until:Date.now()/1000+2}));}
   result.soak.samples=samples.length;result.soak.lastSample=sample.at;result.soak.latestPrivateBytes=sample.privateBytes;save();
   await sleep(Math.min(60000,Math.max(0,end-Date.now())));
  }
  const first=samples.filter(s=>s.elapsedMs>=3600000&&s.elapsedMs<7200000).map(s=>s.privateBytes),last=samples.filter(s=>s.elapsedMs>=(soakHours-1)*3600000).map(s=>s.privateBytes);
  assert.ok(first.length>=50&&last.length>=50,'both acceptance hours need complete sample coverage');
  result.soak.firstHourMedian=percentile(first,.5);result.soak.lastHourMedian=percentile(last,.5);result.soak.growth=result.soak.lastHourMedian/result.soak.firstHourMedian-1;
  result.status=result.soak.growth<=.2?'soak_pass':'soak_fail';save();console.log(JSON.stringify({status:result.status,soak:result.soak}));
 }
}catch(error){result.status='failed';result.error=String(error.stack||error);try{result.uiFailure=await page?.evaluate(()=>({errors:[...document.querySelectorAll('[role="alert"],.terminal-error')].map(e=>e.textContent),terminals:window.nativeProbe?.terms?.map(({id,term})=>({id,disabled:term.options.disableStdin,focused:term.textarea===document.activeElement,connected:term.element.isConnected})),visibility:document.visibilityState}));await page?.screenshot({path:join(dir,'failure.png')})}catch{}save();throw error}
finally{
 for(const timer of timers)clearInterval(timer);
 await browser?.close().catch(()=>{});
 // Only this generated data directory and our child process handles are used.
 try{await rpc({cmd:'service.stop'})}catch(error){result.cleanupError=String(error)}
 for(const child of children.reverse())if(child.exitCode===null)child.kill();
 result.finished=new Date().toISOString();save();
}
