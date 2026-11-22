if (process.argv.includes('--product-baseline')) {
 require('./fixtures/velella-product-baseline.cjs');
} else {
// Real Electron + Rust daemon + ConPTY, with a controlled native provider.
// No production profile, credentials or upstream model calls.
const {_electron}=require('playwright');
const {spawn,execFileSync}=require('node:child_process');
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const {socketRequest}=require('../desktop/electron/runtime.cjs');
const root=path.resolve(__dirname,'..'),program=process.env.VELELLA_REOPEN_PROVIDER||'codex';
assert.ok(['codex','claude','gemini','opencode','omp','kimi','zcode-free','mcode'].includes(program));
const out=path.join(root,'artifacts',process.env.VELELLA_REOPEN_EVIDENCE||'terminal-reopen-20261114','native-'+program+'-'+Date.now());
const data=path.join(out,'data'),project=path.join(out,'project'),bin=path.join(out,'bin');
for(const p of [data,project,bin])fs.mkdirSync(p,{recursive:true});
// An independent Git root prevents project.add from resolving to the enclosing
// live Velella repository and correctly refusing its existing ownership lock.
execFileSync('git',['init','--quiet',project],{windowsHide:true});
const candidate=process.env.VELELLA_REOPEN_CANDIDATE;
const runtime=candidate?path.join(candidate,'resources/runtime/velella-runtime.exe'):path.join(root,'target/release/velella-runtime.exe');
const endpoint=JSON.parse(execFileSync(runtime,['ipc-endpoint',data,'daemon'],{encoding:'utf8',windowsHide:true}));
const native=program==='zcode-free'?'sess_original':program==='opencode'?'ses_original':program==='kimi'?'session_d8fdd138-475c-43c0-9f4d-722ec8a48741':'d8fdd138-475c-43c0-9f4d-722ec8a48741';
const flag=program==='codex'?'resume':['opencode','kimi','mcode'].includes(program)?'--session':'--resume';
const witness=path.join(out,'provider.jsonl');
fs.writeFileSync(path.join(bin,program+'.cmd'),`@echo off\r\n"${process.execPath}" "${path.join(bin,'provider.cjs')}" %*\r\n`);
fs.writeFileSync(path.join(bin,'provider.cjs'),`
const fs=require('node:fs'),{spawnSync}=require('node:child_process');
const args=process.argv.slice(2);if(args[0]!==${JSON.stringify(flag)}||args[1]!==${JSON.stringify(native)})process.exit(9);
fs.appendFileSync(${JSON.stringify(witness)},JSON.stringify({args,cwd:process.cwd(),pid:process.pid})+'\\n');
const hook=event=>{if(${JSON.stringify(program)}==='kimi'){const path=require('node:path'),home=process.env.KIMI_CODE_HOME;fs.mkdirSync(path.join(home,'locks/sessions'),{recursive:true});fs.appendFileSync(path.join(home,'session_index.jsonl'),JSON.stringify({sessionId:args[1],workDir:process.cwd()})+'\\n');fs.writeFileSync(path.join(home,'locks/sessions',args[1]+'.lock'),JSON.stringify({version:1,pid:process.pid}));return;}spawnSync(process.env.VELELLA_RESTORE_RUNTIME,['native-restore-hook',args[1],event],{env:process.env,windowsHide:true});};
hook('SessionStart');console.log('SAME_NATIVE_READY_'+args[1]);
let input='';process.stdin.on('data',chunk=>{input+=chunk.toString();if(input.includes('probe')){hook('UserPromptSubmit');console.log('PROVIDER_'+'REPLIED_'+args[1]);input='';}});process.stdin.resume();
`);
const env={...process.env,VELELLA_DATA_DIR:data,VELELLA_RUNTIME_PATH:runtime,VELELLA_DESKTOP_BACKGROUND:'1'};delete env.ELECTRON_RUN_AS_NODE;
const result={out,checks:[],provider:program,controlledProvider:true,realUpstreamModel:false};
let daemon,app,page,wid,sid,dir;
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,label,timeout=15000){const end=Date.now()+timeout;let last;while(Date.now()<end){try{const value=await fn();if(value)return value;}catch(e){last=e;}await delay(80);}throw last||Error(label+' timed out');}
async function rpc(q){const r=await socketRequest(endpoint,q,12000);if(!r.ok)throw Error(r.error);return r;}
const row=async()=>(await rpc({cmd:'workbench.sessions',workspace_id:wid})).sessions.find(s=>s.id===sid);
const bytes=()=>fs.readFileSync(path.join(dir,'terminal.bin'));
const launches=()=>fs.existsSync(witness)?fs.readFileSync(witness,'utf8').trim().split('\n').filter(Boolean).map(JSON.parse):[];
async function desktop(){
 app=await _electron.launch({executablePath:candidate?path.join(candidate,'Velella.exe'):path.join(root,'desktop/node_modules/electron/dist/electron.exe'),args:candidate?[]:[path.join(root,'desktop')],env,timeout:30000});
 page=await app.firstWindow();await page.waitForLoadState('domcontentloaded');
 await page.addInitScript(()=>{window.__velellaRestoreTerms=[];window.addEventListener('velella:terminal-created',event=>window.__velellaRestoreTerms.push(event.detail.terminal));});
 await page.evaluate(id=>{localStorage.setItem('velella:workspace',id);localStorage.setItem('velella:terminal-preferences:v1',JSON.stringify({renderer:'dom'}));},wid);
 await page.reload();await page.locator('.xterm-helper-textarea').waitFor();
 await app.evaluate(({BrowserWindow})=>{const w=BrowserWindow.getAllWindows()[0];w.show();w.focus();});
}
async function usable(){return until(async()=>{const s=await row();return s?.status?.exit===null&&s.status.input_protocol===1&&launches().length?s:null;},'native host ready');}
async function probe(){await page.waitForFunction(()=>{const t=window.__velellaRestoreTerms?.at(-1);return t?.element?.isConnected&&!t.options.disableStdin&&!document.querySelector('.terminal-loading');});const before=bytes().length;await page.locator('.xterm-helper-textarea').focus();await page.keyboard.type('probe');await page.keyboard.press('Enter');await until(()=>bytes().subarray(before).includes(Buffer.from('PROVIDER_REPLIED_'+native)),'provider response to keyboard');}
(async()=>{try{
 daemon=spawn(runtime,['daemon',data],{env,windowsHide:true,stdio:'ignore'});await until(()=>rpc({cmd:'snapshot'}),'daemon');
 const p=(await rpc({cmd:'project.add',path:project,direct:true})).project;wid='main:'+p.id;
 sid=(await rpc({cmd:'workbench.terminal.open',workspace_id:wid})).session_id;
 await rpc({cmd:'workbench.terminal.stop',workspace_id:wid,session_id:sid,confirmed:true});
 dir=path.join(project,'.velella/sessions',sid);
 // Seed only our disposable session's saved launch and native identity.
 const nativeHome=path.join(out,'native-home');fs.mkdirSync(nativeHome,{recursive:true});
 const savedEnv={PATH:bin+path.delimiter+process.env.PATH,HOME:nativeHome,USERPROFILE:nativeHome,KIMI_CODE_HOME:path.join(nativeHome,'kimi'),VELELLA_RESTORE_RUNTIME:runtime,VELELLA_RESTORE_DIR:dir};
 if(program==='mcode'){savedEnv.VELELLA_MINIMAX_DATA_DIR=path.join(nativeHome,'minimax');savedEnv.MINIMAX_DATA_DIR=savedEnv.VELELLA_MINIMAX_DATA_DIR;const database=path.join(savedEnv.MINIMAX_DATA_DIR,'v2/sqlite/runtime-state.sqlite');fs.mkdirSync(path.dirname(database),{recursive:true});execFileSync('python',['-c',`import sqlite3,sys;c=sqlite3.connect(sys.argv[1]);c.execute('CREATE TABLE local_runtime_sessions(session_id TEXT,title TEXT,created_at_ms INTEGER,workspace_dir TEXT,parent_session_id TEXT,archived INTEGER)');c.execute('INSERT INTO local_runtime_sessions VALUES(?,?,1,?,NULL,0)',(sys.argv[2],'fixture',sys.argv[3]));c.commit();c.close()`,database,native,project],{windowsHide:true});}
 const seed=`import sqlite3,json,sys\nc=sqlite3.connect(sys.argv[1]);sid=sys.argv[2];s=json.loads(c.execute('select document from sessions where id=?',(sid,)).fetchone()[0]);s['native_id']=sys.argv[3];s['title']='Restore fixture';s['descriptor']['spawn'].update(program=sys.argv[5],args=[],cwd=sys.argv[4],adapter='generic',env=json.loads(sys.argv[6]));c.execute('update sessions set document=? where id=?',(json.dumps(s),sid));c.commit();c.close()`;
 execFileSync('python',['-c',seed,path.join(data,'workbench.db'),sid,native,project,path.join(bin,program+'.cmd'),JSON.stringify(savedEnv)],{windowsHide:true});
 const layout=(await rpc({cmd:'workbench.layout',workspace_id:wid})).layout;
 layout.panes=[{id:'main',active:'terminal',tabs:[{id:'terminal',kind:'terminal',target:sid,title:'Restore fixture'}]}];
 await rpc({cmd:'workbench.layout.save',workspace_id:wid,layout});
 assert.equal((await row()).auto_recoverable,false);await until(async()=>(await row()).open_recoverable,'stopped host ownership release');
 const prefix=bytes();await desktop();let live=await usable();await probe();
 assert.equal(launches().length,1);assert.ok(bytes().subarray(0,prefix.length).equals(prefix));result.checks.push('opening a stopped native terminal resumes its exact saved ID once; keyboard gets new provider output; original raw history prefix is unchanged');
 const pid=live.status.pid;await app.close();app=null;await desktop();live=await usable();assert.equal(live.status.pid,pid);assert.equal(launches().length,1);await probe();result.checks.push('closing and reopening Electron reattaches the same live host without launching another CLI');
 await rpc({cmd:'workbench.terminal.stop',workspace_id:wid,session_id:sid,confirmed:true});await page.getByRole('status').filter({hasText:'进程已结束'}).waitFor();await delay(2400);assert.equal(launches().length,1);assert.equal((await row()).auto_recoverable,false);result.checks.push('explicit stop remains stopped during visible-pane polling');
 await app.close();app=null;await desktop();live=await usable();assert.notEqual(live.status.pid,pid);await probe();assert.equal(launches().length,2);assert.ok(launches().every(x=>x.args[1]===native&&x.cwd===project));result.checks.push('a later application open resumes the stopped original conversation into a new host and accepts keyboard input');
 await page.screenshot({path:path.join(out,'restored.png')});result.ok=true;
}catch(e){result.ok=false;result.error=e.stack;process.exitCode=1;if(page){result.ui=await page.locator('body').innerText().catch(()=>null);await page.screenshot({path:path.join(out,'failure.png')}).catch(()=>{});}if(wid&&sid)result.session=await row().catch(()=>null);}
finally{if(app)await app.close().catch(()=>{});if(wid&&sid)await rpc({cmd:'workbench.terminal.stop',workspace_id:wid,session_id:sid,confirmed:true}).catch(()=>{});if(daemon&&!daemon.killed)daemon.kill();fs.writeFileSync(path.join(out,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));}})();
}
