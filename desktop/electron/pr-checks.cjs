const {spawn} = require('node:child_process');

const fields='number,title,url,state,isDraft,headRefName,baseRefName,headRefOid,updatedAt,reviewDecision,statusCheckRollup,headRepository';
function failure(code){return Object.assign(new Error(code),{code})}
function createReadOnlyRunner(){
  const children=new Set();let disposed=false;
  function run(binary,args,cwd,timeoutMs=20000){
    if(disposed)return Promise.reject(failure('disposed'));
    return new Promise((resolve,reject)=>{
      const env={...process.env,GH_PROMPT_DISABLED:'1',GIT_TERMINAL_PROMPT:'0',GCM_INTERACTIVE:'never',GIT_OPTIONAL_LOCKS:'0',NO_COLOR:'1',CLICOLOR:'0',LC_ALL:'C'};
      for(const key of Object.keys(env))if(['GH_REPO','GH_HOST','GH_FORCE_TTY','GIT_DIR','GIT_WORK_TREE','GIT_COMMON_DIR','GIT_INDEX_FILE','GIT_CONFIG_COUNT'].includes(key)||/^GIT_CONFIG_(KEY|VALUE)_/.test(key))delete env[key];
      const child=spawn(binary,args,{cwd,env,shell:false,windowsHide:true,stdio:['ignore','pipe','pipe']});
      children.add(child);let stdout='',stderr='',size=0,settled=false;
      const finish=(error,code)=>{if(settled)return;settled=true;clearTimeout(timer);children.delete(child);error?reject(error):resolve({code,stdout,stderr});};
      const timer=setTimeout(()=>{child.kill();finish(failure('timeout'))},timeoutMs);
      child.stdout.setEncoding('utf8');child.stderr.setEncoding('utf8');
      const receive=kind=>bytes=>{size+=Buffer.byteLength(bytes);if(size>4*1024*1024){child.kill();finish(failure('too_large'));return}if(kind==='out')stdout+=bytes;else stderr+=bytes;};
      child.stdout.on('data',receive('out'));child.stderr.on('data',receive('err'));
      child.on('error',error=>finish(failure(error.code==='ENOENT'?'missing':'command_failed')));
      child.on('close',code=>finish(null,code));
    });
  }
  return {run,dispose(){disposed=true;for(const child of children)child.kill();children.clear()}};
}

function parseRemote(value){
  if(typeof value!=='string'||value.length>2048||/[\x00-\x20\x7f]/.test(value))return null;
  let host,path;
  const ssh=/^git@([^:]+):(.+)$/.exec(value);
  if(ssh){host=ssh[1];path=ssh[2]}
  else{try{const url=new URL(value);if(!['https:','ssh:'].includes(url.protocol)||url.password||url.search||url.hash||url.port||url.protocol==='https:'&&url.username)return null;host=url.hostname;path=url.pathname.replace(/^\//,'')}catch{return null}}
  path=path.replace(/\/$/,'').replace(/\.git$/,'');
  if(host.toLowerCase()!=='github.com'||!/^[-\w.]+\/[-\w.]+$/.test(path)||path.split('/').some(p=>p==='.'||p==='..'))return null;
  return {host:'github.com',name:path,url:`https://github.com/${path}`};
}
function safeLink(value){try{const url=new URL(value);return url.protocol==='https:'&&!url.username&&!url.password&&value.length<=4096?url.href:null}catch{return null}}
function text(value,limit=300){return typeof value==='string'?value.replace(/[\x00-\x1f\x7f]/g,' ').slice(0,limit):''}
function normalizeCheck(item,index){
  const context=item.__typename==='StatusContext'||typeof item.context==='string';
  const result=String(context?item.state:item.conclusion||item.status||'UNKNOWN').toUpperCase();
  let status='unknown',label='未知';
  if(result==='SUCCESS'){status='success';label='通过'}
  else if(['FAILURE','ERROR','TIMED_OUT','ACTION_REQUIRED','STALE','STARTUP_FAILURE'].includes(result)){status='failure';label=result==='ACTION_REQUIRED'?'需要处理':result==='TIMED_OUT'?'超时':'失败'}
  else if(['PENDING','EXPECTED','QUEUED','IN_PROGRESS','WAITING','REQUESTED'].includes(result)){status='pending';label=result==='IN_PROGRESS'?'进行中':'等待中'}
  else if(result==='CANCELLED'){status='cancelled';label='已取消'}
  else if(['SKIPPED','NEUTRAL'].includes(result)){status='skipped';label=result==='NEUTRAL'?'中立':'已跳过'}
  return {id:String(index),name:text(context?item.context:item.name)||'未命名检查',status,label,url:safeLink(context?item.targetUrl:item.detailsUrl),workflow:text(item.workflowName),started_at:text(item.startedAt,40),completed_at:text(item.completedAt,40)};
}
function normalizePull(pr,repository){
  if(!Number.isSafeInteger(pr.number)||pr.number<1||!/^[0-9a-f]{40}$/i.test(pr.headRefOid||''))throw failure('invalid_response');
  const url=safeLink(pr.url);
  if(url!==`${repository.url}/pull/${pr.number}`)throw failure('invalid_response');
  if(!Array.isArray(pr.statusCheckRollup))throw failure('invalid_response');
  const checks=pr.statusCheckRollup.map(normalizeCheck),summary={success:0,pending:0,failure:0,cancelled:0,skipped:0,unknown:0};
  for(const check of checks)summary[check.status]++;
  return {number:pr.number,title:text(pr.title,500),url,state:['OPEN','CLOSED','MERGED'].includes(pr.state)?pr.state:'UNKNOWN',draft:pr.isDraft===true,head_sha:pr.headRefOid,head_branch:text(pr.headRefName),base_branch:text(pr.baseRefName),updated_at:text(pr.updatedAt,40),review_decision:['APPROVED','CHANGES_REQUESTED','REVIEW_REQUIRED'].includes(pr.reviewDecision)?pr.reviewDecision:'',checks,summary};
}

function createPrChecks(runtime,{run:injectedRun}={}){
  const runner=injectedRun?null:createReadOnlyRunner(),run=injectedRun||runner.run,inflight=new Map();
  const result=(state,message,extra={})=>({state,message,checked_at:new Date().toISOString(),...extra});
  async function inspect(workspaceId){
    try{
      const snapshot=await runtime.request({cmd:'workbench.snapshot'});
      if(!snapshot.ok)throw failure('workspace_unavailable');
      const workspace=snapshot.workspaces?.find(w=>w.id===workspaceId);
      if(!workspace||typeof workspace.path!=='string')return result('error','工作区已不可用，请重新选择工作区。');
      const cwd=workspace.path;
      const git=async args=>run('git',['-c','core.fsmonitor=false',...args],cwd,5000);
      const root=await git(['rev-parse','--is-inside-work-tree']);
      if(root.code!==0||root.stdout.trim()!=='true')return result('not_git','当前文件夹不是 Git 仓库。打开 Git 项目或工作树后，这里会显示关联 PR 的检查。');
      const head=await git(['symbolic-ref','--quiet','--short','HEAD']);
      if(head.code!==0||!head.stdout.trim())return result('detached_head','当前工作树没有检出分支。切换到 PR 对应的分支后再刷新。');
      const branch=head.stdout.trim();
      const remotes=await git(['config','--get-regexp','^remote\\..*\\.url$']);
      const parsed=[];
      for(const line of remotes.stdout.split(/\r?\n/)){const match=/^remote\.(.+)\.url\s+(.+)$/.exec(line);if(match){const repository=parseRemote(match[2]);if(repository)parsed.push({remote:match[1],repository})}}
      if(!parsed.length)return result(remotes.stdout.trim()?'unsupported_host':'no_remote',remotes.stdout.trim()?'目前支持 GitHub 仓库的 PR 检查。':'仓库尚未配置 GitHub 远程地址。',{branch});
      const source=parsed.find(r=>r.remote==='origin')||parsed[0];
      const repository=(parsed.find(r=>r.remote==='upstream')||source).repository;
      const response=await run('gh',['pr','list',`--repo=${repository.name}`,`--head=${branch}`,'--state=all','--limit=20',`--json=${fields}`],cwd);
      if(response.code===4||/gh auth login|not logged into|authentication token/i.test(response.stderr))return result('auth_required','请先在“任务来源”中连接 GitHub，再回到这里刷新。',{repository,branch});
      if(response.code!==0)return result('error','无法读取 PR 检查。请确认 GitHub 仓库访问权限和网络连接后重试。',{repository,branch});
      let rows;try{rows=JSON.parse(response.stdout)}catch{throw failure('invalid_response')}
      if(!Array.isArray(rows)||rows.length>20)throw failure('invalid_response');
      // A base repository may have forks with the same branch name. Only show
      // PRs from this checkout's source repository, not an unrelated fork.
      rows=rows.filter(pr=>pr.headRepository?.nameWithOwner?.toLowerCase()===source.repository.name.toLowerCase());
      const pulls=rows.map(pr=>normalizePull(pr,repository)).sort((a,b)=>Number(b.state==='OPEN')-Number(a.state==='OPEN')||b.updated_at.localeCompare(a.updated_at));
      return pulls.length?result('ready','',{repository,branch,pull_requests:pulls}):result('no_pr','当前分支还没有关联的 PR。创建或推送对应 PR 后，点击刷新。',{repository,branch});
    }catch(error){
      if(error.code==='missing')return result('tool_unavailable','未找到 Git 或 GitHub 连接工具，请检查桌面安装与 GitHub 连接。');
      if(error.code==='timeout')return result('error','读取检查超时，请稍后刷新。');
      if(error.code==='too_large')return result('error','检查结果过大，暂时无法显示。');
      return result('error','暂时无法读取 PR 检查，请稍后刷新。');
    }
  }
  return {read(request){
    const id=request?.workspace_id;
    if(typeof id!=='string'||!id||id.length>512)return Promise.reject(Error('工作区标识无效'));
    if(inflight.has(id))return inflight.get(id);
    if(inflight.size>=4)return Promise.resolve(result('error','正在读取其他工作区，请稍后刷新。'));
    const pending=inspect(id).finally(()=>inflight.delete(id));inflight.set(id,pending);return pending;
  },dispose(){runner?.dispose()}};
}
module.exports={createPrChecks,createReadOnlyRunner,parseRemote,normalizeCheck,normalizePull,safeLink};
