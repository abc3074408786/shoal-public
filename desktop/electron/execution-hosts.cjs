const {randomUUID}=require('node:crypto');
const {createSshRuntime}=require('./ssh-runtime.cjs');
const UUID=/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const object=value=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const REMOTE_COMMANDS=new Set([
  'workbench.snapshot','project.add','project.create','project.update','agent.check','sandbox.capabilities',
  'worktree.list','workbench.create','workbench.creation.status','workbench.creation.retry',
  'workbench.update','workbench.layout','workbench.layout.save','workbench.events','workbench.sessions',
  'workbench.files','workbench.search','workbench.file.read','workbench.file.asset','workbench.file.create',
  'workbench.file.write','workbench.file.rename','workbench.file.delete','workbench.git','workbench.git.stage',
  'workbench.git.unstage','workbench.git.commit','workbench.terminal.open','workbench.terminal.recover',
  'workbench.terminal.stop','workbench.terminal.output','workbench.terminal.input','workbench.terminal.resize',
  'workbench.terminal.control','workbench.terminal.activity','workbench.feedback.send','workbench.feedback.status',
  'task.create','task.cancel','task.resume','task.verify','task.feedback','task.feedback.status','task.files',
  'task.diff','task.events','task.commit','task.pr','task.delivery.reset','task.finish','terminal.shell',
  'terminal.input','terminal.output','terminal.resize','terminal.control','file.read','file.write',
]);
function checkedTarget(target) {
  if(target===undefined)return Object.freeze({kind:'local'});
  if(!object(target))throw Error('execution target required');
  if(target.kind==='local'&&Object.keys(target).length===1)return Object.freeze({kind:'local'});
  if(target.kind!=='ssh'||Object.keys(target).length!==3||!UUID.test(target.host_id)||!UUID.test(target.server_id))throw Error('invalid execution target');
  return Object.freeze({kind:'ssh',host_id:target.host_id,server_id:target.server_id});
}
function requireLocalTarget(target) {
  if(checkedTarget(target).kind!=='local')throw Error('此功能仅支持本机；不会将远端路径或进程交给本机接口');
}
function validateRemoteRequest(request) {
  if(request?.launch==='hybrid-codex')throw Error('混合执行必须从本机项目启动');
  if(!object(request)||!REMOTE_COMMANDS.has(request.cmd))throw Error('此功能未开放远端执行');
  if(request.collaboration===true||request.role_config!=null||request.source_key!=null||request.browser!=null)throw Error('远端首版不支持浏览器、编排或任务来源绑定');
}
function createExecutionHosts(local,{remoteFactory=createSshRuntime,confirmConnection}={}) {
  const entries=new Map();let disposed=false;
  async function localRequest(request) {
    const result=await local.request(request);
    if(!result||result.ok!==true)throw Error(result?.error||'执行主机记录读取失败');
    return result;
  }
  function requireActive(entry) {
    if(disposed||entries.get(entry.id)!==entry)throw Error('执行主机连接已取消，请重新连接');
  }
  async function binding(entry) {
    const current=await localRequest({cmd:'execution_hosts.get',id:entry.id});
    requireActive(entry);
    if(current.host.revision!==entry.record.host.revision||current.profile_fingerprint!==entry.record.profile_fingerprint) {
      entry.remote.disconnect();entries.delete(entry.id);throw Error('执行主机配置已改变，请重新检查并连接');
    }
    return current;
  }
  function view(entry) {
    return {host_id:entry.id,inspection_id:entry.inspection_id,state:entry.remote.state,
      identity:entry.identity||null,target:entry.remote.state==='connected'?{kind:'ssh',host_id:entry.id,server_id:entry.identity.server_id}:null};
  }
  async function manage(args) {
    if(disposed)throw Error('执行主机管理已关闭');
    if(!object(args))throw Error('execution host operation required');
    if(args.action==='list') {
      const result=await localRequest({cmd:'execution_hosts.list'});
      return {...result,connections:[...entries.values()].map(view)};
    }
    if(args.action==='save')return localRequest({cmd:'execution_hosts.save',host:args.host});
    if(!UUID.test(args.id))throw Error('invalid execution host ID');
    if(args.action==='delete') {
      const result=await localRequest({cmd:'execution_hosts.delete',id:args.id,confirmed:args.confirmed,expected_revision:args.expected_revision});
      const entry=entries.get(args.id);entry?.remote.disconnect();entries.delete(args.id);return result;
    }
    if(args.action==='disconnect') {
      const entry=entries.get(args.id);entry?.remote.disconnect();entries.delete(args.id);return {ok:true};
    }
    if(args.action==='inspect') {
      if(entries.has(args.id))throw Error('请先断开旧连接，再检查该执行主机');
      const record=await localRequest({cmd:'execution_hosts.get',id:args.id});
      if(disposed||entries.has(args.id))throw Error('执行主机连接已改变，请重试');
      const remote=remoteFactory(record.profile);
      const entry={id:args.id,inspection_id:randomUUID(),record,remote,identity:null};entries.set(args.id,entry);
      try {
        const known=record.confirmation_valid?record.host.confirmation:null;
        entry.identity=await remote.inspect({data_root:known?.data_root??record.host.data_root??undefined,expected_server_id:known?.server_id});
        await binding(entry);return {ok:true,...view(entry),profile:{name:record.profile.name,host:record.profile.host,user:record.profile.user,port:record.profile.port},confirmation_valid:record.confirmation_valid};
      }catch(error){remote.disconnect();if(entries.get(args.id)===entry)entries.delete(args.id);throw error;}
    }
    if(args.action==='connect') {
      const entry=entries.get(args.id);
      if(!entry||entry.inspection_id!==args.inspection_id||entry.remote.state!=='awaiting_confirmation')throw Error('请先检查并确认当前执行主机身份');
      if(entry.confirming)throw Error('执行主机确认正在进行');
      entry.confirming=true;
      try {
        await binding(entry);
        if(typeof confirmConnection!=='function')throw Error('缺少执行主机确认入口');
        if(!await confirmConnection({host:entry.record.host,profile:entry.record.profile,identity:entry.identity}))return {ok:true,cancelled:true};
        await binding(entry);
        await entry.remote.connect({...entry.identity,confirm:true});requireActive(entry);
        const result=await localRequest({cmd:'execution_hosts.confirm',id:entry.id,confirmed:true,
          reviewed_host:entry.record.host,reviewed_profile:entry.record.profile,identity:entry.identity});
        requireActive(entry);entry.record={...entry.record,host:result.host,confirmation_valid:true};
        return {ok:true,...view(entry)};
      }catch(error){entry.remote.disconnect();if(entries.get(args.id)===entry)entries.delete(args.id);throw error;}
      finally{entry.confirming=false;}
    }
    throw Error('unknown execution host operation');
  }
  async function request(request,target) {
    const bound=checkedTarget(target);
    if(bound.kind==='local') {
      if(typeof request?.cmd==='string'&&request.cmd.startsWith('execution_hosts.'))throw Error('请使用执行主机管理入口');
      return local.request(request);
    }
    validateRemoteRequest(request);
    const entry=entries.get(bound.host_id);
    if(!entry||entry.remote.state!=='connected'||entry.confirming||entry.identity.server_id!==bound.server_id)throw Error('远端执行主机未连接或身份不匹配；不会转到本机');
    await binding(entry);
    if(entry.remote.state!=='connected'||entry.confirming)throw Error('远端连接已断开；请求未发送');
    return entry.remote.request(request);
  }
  return {manage,request,dispose(){disposed=true;for(const entry of entries.values())entry.remote.disconnect();entries.clear();}};
}
module.exports={createExecutionHosts,checkedTarget,requireLocalTarget,validateRemoteRequest};
