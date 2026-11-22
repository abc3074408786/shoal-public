const fs=require('node:fs/promises');
const path=require('node:path');
const {execFile}=require('node:child_process');
const {randomUUID,createHash}=require('node:crypto');
const manifest=require('./manifest.json');
const programs={codex:'codex','claude-code':'claude',gemini:'gemini',opencode:'opencode',omp:'omp',kimi:'kimi',zcode:'zcode-free'};
const native=new Set(['codex','claude-code','gemini','opencode','kimi']);
function exec(file,args,options={}){return new Promise((resolve,reject)=>execFile(file,args,{windowsHide:true,shell:false,encoding:'utf8',timeout:120000,maxBuffer:10*1024*1024,...options},(error,stdout,stderr)=>error?reject(Object.assign(error,{stdout,stderr})):resolve(stdout)));}
async function located(program){
  if(process.platform==='win32')return exec('where.exe',[program],{timeout:4000}).then(s=>s.trim().split(/\r?\n/).filter(Boolean),()=>[]);
  const found=await Promise.all((process.env.PATH||'').split(path.delimiter).map(p=>{const file=path.join(p,program);return fs.access(file,1).then(()=>file,()=>null);}));return found.filter(Boolean);
}
function create({dataDir,stateDir,cliPath,runtime,onCommand,runEngine,locate=located}){
  const activeLaunches=new Set();
  async function publish(file,value){const temp=file+'.'+randomUUID()+'.tmp';await fs.writeFile(temp,JSON.stringify(value),{flag:'wx',mode:0o600});await fs.rename(temp,file);}
  async function run(request){
    if(runEngine)return runEngine(request);
    if(!cliPath)throw Error('技能中心引擎尚未配置，请使用完整候选安装包。');
    const dir=path.join(stateDir,'requests');await fs.mkdir(dir,{recursive:true});const file=path.join(dir,randomUUID()+'.json');
    await fs.writeFile(file,JSON.stringify(request),{flag:'wx',mode:0o600});
    const args=['skills','center','--library',dataDir,'--file',file,'--json'];
    try{
      let stdout;try{stdout=await exec(cliPath,args);}catch(error){stdout=error.stdout;if(!stdout)throw Error(error.killed?'操作超时，结果未知；请刷新检查历史与待恢复操作。':error.stderr||error.message);}
      let value;try{value=JSON.parse(stdout);}catch{throw Error('技能中心引擎返回无效响应，写入结果未知；请刷新核对。');}
      onCommand?.({executable:cliPath,args,stdout});
      if(!value.ok)throw Error(value.error?.message||JSON.stringify(value.error)||'技能操作失败');return value.result;
    }finally{await fs.unlink(file).catch(()=>{});}
  }
  async function snapshot(){
    if(!runtime)throw Error('本机运行时不可用');
    const value=await runtime.request({cmd:'workbench.snapshot'});if(value.ok===false)throw Error(value.error?.message||'无法读取工作区');return value;
  }
  async function scope(request){
    const clean={...request};delete clean.library;
    if(clean.project){const state=await snapshot();const workspace=state.workspaces.find(w=>!w.archived&&path.resolve(w.path)===path.resolve(clean.project));if(!workspace)throw Error('项目目录不属于当前可用工作区，请重新选择。');clean.project=workspace.path;}
    return clean;
  }
  const service={async call(request){
    if(!request||typeof request!=='object'||Array.isArray(request))throw Error('需要技能操作对象');
    if(request.operation==='workspaces'){const state=await snapshot();return {workspaces:state.workspaces.filter(w=>!w.archived)};}
    if(request.operation==='agents')return {agents:await Promise.all(Object.entries(programs).map(async([id,program])=>({id,program,paths:await locate(program),nativeDirectory:native.has(id),verification:'unverified'})))};
    if(request.operation==='launchStatus'){
      if(!/^[0-9a-f-]{36}$/i.test(request.request_id||''))throw Error('需要固定启动请求标识');
      if(activeLaunches.has(request.request_id))return {state:'pending',safe_to_clear:false};
      let saved;try{saved=JSON.parse(await fs.readFile(path.join(stateDir,'launches',request.request_id+'.json'),'utf8'));}catch(error){if(error.code==='ENOENT')return {state:'not_dispatched',safe_to_clear:true};throw error;}
      if(saved.result)return {state:'completed',safe_to_clear:false,result:saved.result};
      const value=await runtime.request({cmd:'workbench.sessions',workspace_id:saved.request.workspace_id});if(value.ok===false)throw Error('无法核对原会话，未清除启动请求');
      const session=value.sessions.find(s=>s.id===saved.request.request_id);
      return {state:session?'session_exists':saved.definite_failure?'failed_before_session':'unknown',safe_to_clear:!session&&saved.definite_failure===true};
    }
    const clean=await scope(request);
    if(request.operation!=='launch')return run(clean);
    if(!programs[request.agent])throw Error('不支持的 Agent');
    if(!/^[0-9a-f-]{36}$/i.test(request.request_id||''))throw Error('启动需要固定 request_id');
    if(!(await locate(programs[request.agent])).length)throw Error('未安装或未定位此 Agent，未启动会话。');
    const state=await snapshot();const workspace=state.workspaces.find(w=>w.id===request.workspace_id&&!w.archived);
    if(!workspace)throw Error('请选择可用工作区');
    if(request.scope==='project'&&path.resolve(workspace.path)!==path.resolve(clean.project))throw Error('项目技能只能在所属工作区启动');
    const dir=path.join(stateDir,'launches');await fs.mkdir(dir,{recursive:true});const file=path.join(dir,request.request_id+'.json');
    const fingerprint=createHash('sha256').update(JSON.stringify(clean)).digest('hex');
    let saved;try{saved=JSON.parse(await fs.readFile(file,'utf8'));}catch(error){if(error.code!=='ENOENT')throw error;}
    if(saved&&saved.fingerprint!==fingerprint)throw Error('原启动请求已固定，请检查原终端后新建任务。');
    if(!saved){
      const context=await run({...clean,operation:'context'});
      context.prompt=context.prompt.replace(/\r\n?/g,'\n');
      const prompt=['codex','claude-code'].includes(request.agent)?{initial_prompt:{text:context.prompt,mode:'submit'}}:{skill_prompt:context.prompt};
      saved={fingerprint,context,request:{cmd:'workbench.terminal.open',workspace_id:workspace.id,launch:request.agent,request_id:request.request_id,...prompt,...(request.account_home?{account_home:request.account_home}:{})}};
      await fs.writeFile(file,JSON.stringify(saved),{flag:'wx',mode:0o600});
    }
    if(saved.result)return saved.result;
    if((saved.dispatch_count||0)>=2)throw Error('原启动已尝试两次，请核对原会话或修复前置条件；未再次启动。');
    saved.dispatch_count=(saved.dispatch_count||0)+1;await publish(file,saved);
    const value=await runtime.request(saved.request);
    if(value.ok===false){saved.definite_failure=true;await publish(file,saved);throw Error(value.error?.message||'启动未完成；请核对原请求。');}
    if(!value.session_id)throw Error('未收到会话标识，结果未知；请检查终端，不会自动重试。');
    const result={session_id:value.session_id,workspace_id:workspace.id,snapshot:saved.context.snapshot,digest:saved.context.digest,verification:saved.request.initial_prompt?'waiting_input':'submitted_unverified',...(saved.request.initial_prompt?{initial_prompt:saved.context.prompt}:{})};
    await publish(file,{...saved,result});return result;
  }};
  return {async call(request){
    if(request?.operation!=='launch')return service.call(request);
    const id=request.request_id;if(activeLaunches.has(id))throw Error('原启动正在处理，请稍后核对状态；没有重复投递。');
    activeLaunches.add(id);try{return await service.call(request)}finally{activeLaunches.delete(id)}
  }};
}
module.exports={manifest,create,programs};
