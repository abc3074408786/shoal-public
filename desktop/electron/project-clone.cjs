const {spawn} = require('node:child_process');
const fs = require('node:fs/promises');
const path = require('node:path');

function validateRemote(value) {
  if(typeof value!=='string'||value.length>2048||/[\x00-\x20\x7f]/.test(value))throw Error('请输入有效的 HTTPS 或 SSH Git 地址');
  if(/^[\w.-]+@[\w.-]+:[\w./~@+-]+$/.test(value))return value;
  let url;try{url=new URL(value)}catch{throw Error('请输入有效的 HTTPS 或 SSH Git 地址')}
  if(!['https:','http:','ssh:'].includes(url.protocol)||!url.hostname||url.password||url.search||url.hash||url.protocol!=='ssh:'&&url.username||url.pathname==='/')throw Error('Git 地址不能包含密码、查询参数或本地命令');
  return value;
}
function validateName(name) {
  if(typeof name!=='string'||!name||name.length>80||name.trim()!==name||/[<>:"/\\|?*\x00-\x1f]/.test(name)||/[. ]$/.test(name)||['.','..'].includes(name)||/^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)/i.test(name))throw Error('请输入有效的项目文件夹名称');
  return name;
}
function within(parent,child){const relative=path.relative(parent,child);return !relative||relative!=='..'&&!relative.startsWith('..'+path.sep)&&!path.isAbsolute(relative)}

function createProjectCloner(runtime) {
  const jobs=new Map();
  function stop(job){
    if(job.phase==='preparing'){job.cancelled=true;return true}
    if(job.phase!=='cloning'||!job.child||job.child.exitCode!==null)return false;
    job.cancelled=true;
    if(process.platform==='win32'){
      // Only the Git process created by this job and its helpers are targeted.
      const killer=spawn('taskkill',['/PID',String(job.child.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});
      killer.on('error',()=>job.child.kill());
    }else{job.child.kill('SIGTERM')}
    return true;
  }
  async function register(job){
    job.phase='registering';
    try{const result=await runtime.request({cmd:'project.add',path:job.target});if(!result.ok)throw Error(result.error||'添加项目失败');job.phase='complete';job.result={...result,path:job.target};return job.result}
    catch(error){job.phase='registration_failed';return {ok:false,error:`仓库已克隆，添加到项目列表失败：${String(error.message||error)}`,retained_path:job.target,retry_registration:true}}
  }
  async function run(job){
    let created=false;
    try{
      const parent=await fs.realpath(job.request.parent);if(!(await fs.stat(parent)).isDirectory())throw Error('保存位置必须是文件夹');
      job.target=path.join(parent,job.request.name);
      const snapshot=await runtime.request({cmd:'workbench.snapshot'});if(!snapshot.ok)throw Error(snapshot.error||'无法读取项目列表');
      if(snapshot.projects.some(p=>p.direct&&(within(p.path,job.target)||within(job.target,p.path))))throw Error('保存位置与已打开的普通文件夹项目重叠，请选择其他位置');
      if(job.cancelled)throw Error('已取消克隆。');
      await fs.mkdir(job.target);created=true;if(job.cancelled)throw Error('已取消克隆。');job.phase='cloning';
      await new Promise((resolve,reject)=>{
        let output='',settled=false;
        const child=spawn('git',['-c','credential.interactive=false','clone','--progress','--',job.request.url,job.target],{windowsHide:true,shell:false,stdio:['ignore','ignore','pipe'],env:{...process.env,GIT_TERMINAL_PROMPT:'0',GCM_INTERACTIVE:'never',GIT_SSH_COMMAND:process.env.GIT_SSH_COMMAND||'ssh -o BatchMode=yes -o StrictHostKeyChecking=yes'}});
        job.child=child;
        const timer=setTimeout(()=>{job.timedOut=true;stop(job)},10*60*1000);
        const finish=(error)=>{if(settled)return;settled=true;clearTimeout(timer);job.child=null;error?reject(error):resolve()};
        child.stderr.on('data',data=>{output=(output+data.toString()).slice(-6000)});
        child.on('error',error=>finish(Error(error.code==='ENOENT'?'未找到 Git，请先安装 Git。':error.message)));
        child.on('close',code=>finish(job.timedOut?Error('克隆超过十分钟，已停止。'):job.cancelled?Error('已取消克隆。'):code!==0?Error(output.trim()||`Git 克隆失败（${code}）`):null));
      });
      return await register(job);
    }catch(error){
      job.phase='failed';job.result={ok:false,error:error.code==='EEXIST'?'目标文件夹已存在，请选择新的名称。':String(error.message||error),...(created?{retained_path:job.target}:{})};return job.result;
    }
  }
  function clone(request){
    if(!request||typeof request.parent!=='string'||!path.isAbsolute(request.parent)||!/^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(request.request_id||''))throw Error('请选择保存位置后重试');
    const clean={url:validateRemote(request.url),parent:request.parent,name:validateName(request.name)};
    const signature=JSON.stringify(clean);let job=jobs.get(request.request_id);
    if(job){
      if(job.signature!==signature)throw Error('创建请求已改变，请重新提交');
      if(job.phase==='registration_failed'){job.promise=register(job);return job.promise}
      return job.promise;
    }
    // Keep recent retry receipts without growing the desktop process indefinitely.
    if(jobs.size>=32)for(const [id,old] of jobs){if(['complete','failed'].includes(old.phase)){jobs.delete(id);break}}
    if([...jobs.values()].some(j=>['preparing','cloning','registering'].includes(j.phase)))throw Error('已有仓库正在克隆，请等待完成或取消');
    job={request:clean,signature,phase:'preparing',child:null,cancelled:false};jobs.set(request.request_id,job);job.promise=run(job);return job.promise;
  }
  return {clone,cancel:id=>({cancelled:stop(jobs.get(id)||{})}),dispose:()=>{for(const job of jobs.values())stop(job)}};
}
module.exports={createProjectCloner,validateRemote,validateName};
