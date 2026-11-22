// Real native prerequisites and managed planning in a disposable, owned profile.
// Never approves CLI trust/login prompts or retries an uncertain input.
const fs=require('node:fs');const path=require('node:path');const {spawn,execFileSync}=require('node:child_process');const crypto=require('node:crypto');
const {createRuntime}=require('../electron/runtime.cjs');
const output=path.resolve(__dirname,'../../artifacts/orchestration-v2-20261025/native-'+(process.env.VELELLA_NATIVE_ATTEMPT||'1'));
if(fs.existsSync(path.join(output,'ownership.json')))throw Error('This attempt already exists; inspect it instead of resubmitting.');
const data=path.join(output,'data');fs.mkdirSync(data,{recursive:true});
const executable=path.resolve(__dirname,'../../target/debug/velella-runtime.exe');
const selected=(process.env.VELELLA_NATIVE_PROVIDERS||'codex,claude-code,gemini,opencode,kimi').split(',');
let daemon;const runs=[];const events=[];const delay=ms=>new Promise(r=>setTimeout(r,ms));
const runtime=createRuntime(executable,data,{launch(){daemon=spawn(executable,['daemon',data],{windowsHide:true,stdio:'ignore'});fs.writeFileSync(path.join(output,'ownership.json'),JSON.stringify({daemon:daemon.pid,data,executable,sha256:crypto.createHash('sha256').update(fs.readFileSync(executable)).digest('hex')},null,2));return daemon;}});
async function call(request){const result=await runtime.request(request);if(!result.ok)throw Error(result.error||JSON.stringify(result));return result;}
async function observe(item){const {run}=await call({cmd:'orchestration.snapshot',id:item.id});const message={provider:item.provider,state:run.state,blocker:run.blocker||null,revision:run.revision,members:run.status.members.length,work:run.status.work.map(w=>({key:w.work_key,state:w.state}))};const encoded=JSON.stringify(message);if(item.last!==encoded){console.log(encoded);item.last=encoded;events.push({...message,at:new Date().toISOString()});}fs.writeFileSync(path.join(output,item.provider+'.json'),JSON.stringify(run,null,2));return run;}
(async()=>{
  for(const provider of selected){
    const project=path.join(output,provider+'-project');fs.mkdirSync(project,{recursive:true});
    fs.writeFileSync(path.join(project,'.gitignore'),'.velella/\n');fs.writeFileSync(path.join(project,'value.txt'),'before\n');
    for(const args of [['init'],['config','user.name','Velella Native Test'],['config','user.email','fixture@localhost'],['add','.'],['commit','-m','native fixture']])execFileSync('git',args,{cwd:project,windowsHide:true,stdio:'ignore'});
    const added=await call({cmd:'project.add',path:project});const snap=await call({cmd:'workbench.snapshot'});const w=snap.workspaces.find(w=>w.project_id===added.project.id);
    const id=crypto.randomUUID();await call({cmd:'orchestration.create',request_id:id,schema_version:2,workspace_id:w.id,agent:provider,goal:'把隔离工作树的 value.txt 内容改为 after，提交改动，再经独立审查和整体验证。只安排一个实现步骤。不要修改其他源码，不派生 Agent。',acceptance:'集成工作树 value.txt 内容为 after，原目录仍为 before，提供实际读取检查证据。'});runs.push({id,provider,workspace:w.id,project});
  }
  fs.writeFileSync(path.join(output,'runs.json'),JSON.stringify(runs,null,2));
  const deadline=Date.now()+Number(process.env.VELELLA_NATIVE_OBSERVE_MS||45000);
  while(Date.now()<deadline){for(const item of runs)await observe(item);await delay(1500);}
})().catch(error=>{events.push({error:String(error)});console.error(String(error));process.exitCode=1;}).finally(async()=>{
  // Owned sessions only; retain all checkouts, records and output for diagnosis.
  for(const item of runs){try{await observe(item);await call({cmd:'orchestration.stop',id:item.id,request_id:crypto.randomUUID()});}catch(e){events.push({provider:item.provider,stop_error:String(e)});}}
  for(let i=0;i<20;i++){let stopped=true;for(const item of runs){try{const {run}=await call({cmd:'orchestration.snapshot',id:item.id});if(run.state!=='stopped')stopped=false;}catch{stopped=false;}}if(stopped)break;await delay(250);}
  fs.writeFileSync(path.join(output,'result.json'),JSON.stringify({scope:'real CLI startup and planning prerequisites; no trust/login bypass; not end-to-end acceptance',events},null,2));
  if(daemon&&!daemon.killed)daemon.kill();
});
