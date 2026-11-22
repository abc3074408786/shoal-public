import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
const {createExecutionHosts,checkedTarget,requireLocalTarget,validateRemoteRequest}=require('../electron/execution-hosts.cjs');
const hostId='018fa61b-4db2-7000-8000-0000000000aa';
const serverId='018fa61b-4db2-7000-8000-0000000000bb';
const otherServer='018fa61b-4db2-7000-8000-0000000000cc';
const identity={protocol:1,server_id:serverId,data_root:'/srv/中文 remote',os:'linux',runtime_version:'0.2.0',persistence:'prestarted_unverified',capabilities:['identity-v1','multiplex-v1']};
const target={kind:'ssh',host_id:hostId,server_id:serverId};
const profile={id:'profile-1',name:'测试主机',host:'fixture.test',user:'tester',port:2222};
const row=state=>({ok:true,confirmation_valid:state.confirmation_valid,profile_fingerprint:state.profile_fingerprint,
  host:{id:hostId,schema:1,revision:state.revision,name:'测试主机',ssh_profile_id:'profile-1',data_root:identity.data_root,confirmation:state.confirmation},
  profile:{...profile,...state.profile}});
function registry(initial={}) {
  const state={revision:3,profile_fingerprint:'fp-1',confirmation_valid:false,confirmation:null,profile:{},...initial};
  const calls=[];
  return {state,calls,request:async request=>{calls.push(request);
    if(request.cmd==='execution_hosts.get')return row(state);
    if(request.cmd==='execution_hosts.list')return {ok:true,hosts:[row(state).host]};
    if(request.cmd==='execution_hosts.save')return {ok:true,host:row(state).host};
    if(request.cmd==='execution_hosts.delete')return {ok:true,deleted:request.id};
    if(request.cmd==='execution_hosts.confirm'){
      state.revision+=1;state.confirmation={server_id:request.identity.server_id,data_root:request.identity.data_root,os:request.identity.os,profile_fingerprint:state.profile_fingerprint};state.confirmation_valid=true;
      return {ok:true,host:row(state).host};}
    return {ok:true,echo:request};}};
}
function fixture({local=registry(),remote=identity,confirmConnection=()=>true,remoteFactory}={}) {
  const remoteCalls=[];const remotes=[];
  function fakeRemote() {
    const entry={state:'new',identity:null,inspections:[],connects:[],disconnects:0};
    entry.inspect=async options=>{entry.inspections.push(options);
      if(options?.expected_server_id&&remote.server_id!==options.expected_server_id)throw Error('remote server identity changed; explicit review required');
      entry.state='awaiting_confirmation';entry.identity=remote;remoteCalls.push('inspect');return remote;};
    entry.connect=async confirmation=>{entry.connects.push(confirmation);
      if(confirmation?.confirm!==true)throw Error('explicit remote confirmation required');
      if(confirmation.server_id!==remote.server_id)throw Error('remote confirmation identity mismatch');
      entry.state='connected';remoteCalls.push('connect');return remote;};
    entry.request=async request=>{remoteCalls.push('rpc');if(entry.state!=='connected')throw Error('remote disconnected; connect explicitly before requesting');return {ok:true,routed:request.cmd};};
    entry.disconnect=()=>{entry.disconnects++;if(entry.state!=='disconnected')entry.state='disconnected';remoteCalls.push('disconnect');};
    remotes.push(entry);return entry;
  }
  const hosts=createExecutionHosts(local,{remoteFactory:remoteFactory||(()=>fakeRemote()),confirmConnection});
  return {hosts,local,remoteCalls,remotes};
}
async function connectedHost(f) {
  const inspected=await f.hosts.manage({action:'inspect',id:hostId});
  return {inspection_id:inspected.inspection_id,...inspected,...await f.hosts.manage({action:'connect',id:hostId,inspection_id:inspected.inspection_id})};
}

test('execution targets are closed shapes bound to a server identity',()=>{
  assert.deepEqual(checkedTarget(undefined),{kind:'local'});
  assert.deepEqual(checkedTarget({kind:'local'}),{kind:'local'});
  assert.deepEqual(checkedTarget(target),{kind:'ssh',host_id:hostId,server_id:serverId});
  for(const bad of [{kind:'local',extra:1},{kind:'ssh',host_id:hostId},{kind:'ssh',host_id:hostId,server_id:'not-a-uuid'},{kind:'ssh',host_id:hostId,server_id:serverId,data_root:'/extra'},{kind:'tcp',host_id:hostId,server_id:serverId},'ssh',null,[]])
    assert.throws(()=>checkedTarget(bad),/execution target/,String(bad));
  requireLocalTarget(undefined);requireLocalTarget({kind:'local'});
  assert.throws(()=>requireLocalTarget(target),/仅支持本机；不会将远端路径或进程交给本机接口/);
});
test('remote commands use an explicit allowlist and drop host-local capabilities',()=>{
  validateRemoteRequest({cmd:'workbench.create'});validateRemoteRequest({cmd:'task.verify'});validateRemoteRequest({cmd:'workbench.git.commit'});
  for(const cmd of ['automation.create','browser.open','sources.list','runtime.identity','remote.dial','host.launch','workbench.clone'])
    assert.throws(()=>validateRemoteRequest({cmd}),/未开放远端执行/,cmd);
  for(const request of [{cmd:'task.create',collaboration:true},{cmd:'task.create',role_config:{id:'review'}},{cmd:'task.create',source_key:'github:1'},{cmd:'workbench.terminal.open',browser:{url:'http://127.0.0.1'}}])
    assert.throws(()=>validateRemoteRequest(request),/远端首版不支持/,JSON.stringify(request));
});
test('registry management stays on the local daemon and never opens a remote channel',async()=>{
  const f=fixture();
  await f.hosts.manage({action:'list'});
  await f.hosts.manage({action:'save',host:{id:hostId,name:'测试主机'}});
  await f.hosts.manage({action:'delete',id:hostId,confirmed:true,expected_revision:3});
  assert.deepEqual(f.local.calls.map(call=>call.cmd),['execution_hosts.list','execution_hosts.save','execution_hosts.delete']);
  assert.deepEqual(f.remoteCalls,[]);
  await assert.rejects(f.hosts.request({cmd:'execution_hosts.list'},{kind:'local'}),/执行主机管理入口/);
  assert.equal(f.local.calls.length,3);
});
test('inspection requires an explicit connect and records the reviewed identity on the local host',async()=>{
  const f=fixture();
  const inspected=await f.hosts.manage({action:'inspect',id:hostId});
  assert.equal(inspected.state,'awaiting_confirmation');
  assert.equal(inspected.target,null,'an unconfirmed host must not accept business requests');
  assert.match(inspected.inspection_id,/^[0-9a-f-]{36}$/);
  assert.deepEqual(f.remotes[0].inspections.at(-1),{data_root:identity.data_root,expected_server_id:undefined});
  await assert.rejects(f.hosts.request({cmd:'workbench.snapshot'},target),/不会转到本机/);
  assert.deepEqual(f.remoteCalls,['inspect']);
  const connected=await f.hosts.manage({action:'connect',id:hostId,inspection_id:inspected.inspection_id});
  assert.deepEqual(connected.target,target);
  assert.equal(f.local.state.confirmation_valid,true);
  assert.deepEqual(f.local.state.confirmation,{server_id:serverId,data_root:identity.data_root,os:'linux',profile_fingerprint:'fp-1'});
  assert.deepEqual(f.local.state.revision,4,'connecting records the trust under a new revision');
  assert.equal(f.remotes[0].connects.at(-1).confirm,true);
});
test('connect refuses a stale inspection and a cancelled confirmation never dials',async()=>{
  const f=fixture();
  await assert.rejects(f.hosts.manage({action:'connect',id:hostId,inspection_id:'018fa61b-4db2-7000-8000-0000000000ff'}),/请先检查并确认当前执行主机身份/);
  assert.deepEqual(f.remoteCalls,[]);
  const answers=[false,true];
  const cancelled=fixture({confirmConnection:()=>answers.shift()!==false});
  const inspected=await cancelled.hosts.manage({action:'inspect',id:hostId});
  assert.deepEqual(await cancelled.hosts.manage({action:'connect',id:hostId,inspection_id:inspected.inspection_id}),{ok:true,cancelled:true});
  assert.deepEqual(cancelled.remoteCalls,['inspect']);
  assert.equal(cancelled.local.state.confirmation_valid,false,'a cancelled dialog must not persist trust');
  const retried=await cancelled.hosts.manage({action:'connect',id:hostId,inspection_id:inspected.inspection_id});
  assert.deepEqual(retried.target,target,'a cancelled confirmation may be retried without re-inspecting');
  assert.deepEqual(cancelled.remoteCalls,['inspect','connect']);
});
test('an edited profile or revision drops the channel instead of reusing the old trust',async()=>{
  for(const change of [host=>{host.profile_fingerprint='fp-2';},host=>{host.revision+=1;}]) {
    const f=fixture();const joined=await connectedHost(f);
    const business=f.local.calls.length;
    change(f.local.state);
    await assert.rejects(f.hosts.request({cmd:'workbench.sessions'},joined.target),/配置已改变，请重新检查并连接/);
    assert.equal(f.remotes[0].disconnects,1,'a stale review must close the machine channel');
    await assert.rejects(f.hosts.request({cmd:'workbench.sessions'},joined.target),/不会转到本机/);
    assert.deepEqual(f.local.calls.slice(business).map(call=>call.cmd),['execution_hosts.get'],'a remote command leaked to the local daemon');
  }
});
test('business requests route to the confirmed host and a lost channel is never replayed locally',async()=>{
  const f=fixture();const joined=await connectedHost(f);
  const business=f.local.calls.length;
  const routed=await f.hosts.request({cmd:'workbench.create',workspace_id:'w'},joined.target);
  assert.equal(routed.routed,'workbench.create');
  assert.deepEqual(f.local.calls.slice(business).map(call=>call.cmd),['execution_hosts.get'],'the remote call must not be answered by the local daemon');
  f.remotes[0].request=async()=>{const error=Error('remote connection closed; original request was not replayed');error.uncertain=true;throw error;};
  await assert.rejects(f.hosts.request({cmd:'workbench.create',workspace_id:'w'},joined.target),/not replayed/);
  assert.deepEqual(f.local.calls.slice(business).map(call=>call.cmd),['execution_hosts.get','execution_hosts.get']);
  assert.equal(f.remotes[0].disconnects,0,'an uncertain remote write must not silently close and re-dial');
});
test('a mismatched server identity is rejected before any request leaves the main process',async()=>{
  const f=fixture();const joined=await connectedHost(f);
  assert.equal(joined.target.kind,'ssh');
  const business=f.local.calls.length;
  await assert.rejects(f.hosts.request({cmd:'workbench.snapshot'},{...joined.target,server_id:otherServer}),/身份不匹配；不会转到本机/);
  await assert.rejects(f.hosts.request({cmd:'workbench.snapshot'},{kind:'ssh',host_id:'018fa61b-4db2-7000-8000-0000000000dd',server_id:serverId}),/身份不匹配；不会转到本机/);
  assert.deepEqual(f.local.calls.slice(business),[]);
  assert.deepEqual(f.remoteCalls,['inspect','connect']);
});
test('disconnect and dispose close every channel and block later use',async()=>{
  const f=fixture();const joined=await connectedHost(f);
  assert.deepEqual(await f.hosts.manage({action:'disconnect',id:hostId}),{ok:true});
  assert.equal(f.remotes[0].disconnects,1);
  await assert.rejects(f.hosts.request({cmd:'workbench.snapshot'},joined.target),/不会转到本机/);
  const second=fixture();await connectedHost(second);
  second.hosts.dispose();
  assert.equal(second.remotes[0].disconnects,1);
  await assert.rejects(second.hosts.manage({action:'list'}),/管理已关闭/);
  await assert.rejects(second.hosts.request({cmd:'workbench.snapshot'},joined.target),/不会转到本机/);
});
test('re-inspecting a live host requires an explicit disconnect first',async()=>{
  const f=fixture();await connectedHost(f);
  await assert.rejects(f.hosts.manage({action:'inspect',id:hostId}),/请先断开旧连接/);
  assert.deepEqual(f.remoteCalls,['inspect','connect']);
});
test('a trusted host is re-inspected against its recorded identity and a changed server blocks',async()=>{
  const f=fixture({local:registry({confirmation_valid:true,revision:4,confirmation:{server_id:serverId,data_root:identity.data_root,os:'linux',profile_fingerprint:'fp-1'}})});
  const inspected=await f.hosts.manage({action:'inspect',id:hostId});
  assert.deepEqual(f.remotes[0].inspections.at(-1),{data_root:identity.data_root,expected_server_id:serverId});
  assert.equal(inspected.confirmation_valid,true);
  const changed=fixture({local:registry({confirmation_valid:true,revision:4,confirmation:{server_id:serverId,data_root:identity.data_root,os:'linux',profile_fingerprint:'fp-1'}}),remote:{...identity,server_id:otherServer}});
  await assert.rejects(changed.hosts.manage({action:'inspect',id:hostId}),/server identity changed/);
  assert.equal(changed.remotes[0].state,'disconnected','a changed server identity must not stay open');
  assert.deepEqual((await changed.hosts.manage({action:'list'})).connections,[]);
});
test('local targets pass through while remote targets need the command allowlist',async()=>{
  const f=fixture();const joined=await connectedHost(f);
  const localResult=await f.hosts.request({cmd:'resources.scan'},{kind:'local'});
  assert.equal(localResult.echo.cmd,'resources.scan','local features keep their existing passthrough');
  await assert.rejects(f.hosts.request({cmd:'resources.scan'},joined.target),/未开放远端执行/);
  assert.equal(f.remotes[0].state,'connected');
});

// The main process is the last component that can tell a remote workspace from a local one, so
// an IPC surface reading a local path, process or window has to refuse a remote target there.
// A control the renderer merely hides is not a boundary.
const LOCAL_ONLY_IPC=['desktop_resources','resource_manager','resource_space_scan','resource_space_clean','project_clone','pr_checks','browser_command','browser_layout','terminal_clipboard','terminal_performance_export'];
const REVIEWED_IPC=['desktop_resources','resource_manager','resource_space_scan','resource_space_clean','rpc','execution_hosts',
  'project_clone','project_clone_cancel','pr_checks','browser_command','browser_layout','terminal_copy','terminal_clipboard',
  'terminal_open_link','terminal_performance_export','window','dialog_open','dialog_confirm','notify'];
test('every host-local desktop command refuses a remote target in the main process',()=>{
  const main=readFileSync(new URL('../electron/main.cjs',import.meta.url),'utf8');
  const blocks=[...main.matchAll(/^ {10}case "([a-z_]+)":([\s\S]*?)(?=^ {10}case |^ {8}\})/gm)]
    .map(match=>({command:match[1],body:match[2]}));
  assert.deepEqual(blocks.map(block=>block.command),REVIEWED_IPC,'a new desktop IPC command must be classified as local-only or host-independent');
  for(const command of LOCAL_ONLY_IPC)
    assert.match(blocks.find(block=>block.command===command).body,/requireLocalTarget\(args\.target\)/,command);
  for(const command of ['rpc','execution_hosts'])
    assert.match(blocks.find(block=>block.command===command).body,/executionHosts\./,command);
});

// The main-process guard only fires when the renderer names the host it is asking about: a missing
// target normalises to local and silently reads this machine. So every call site of a host-local
// command has to carry one, whatever the surrounding UI decides to hide.
test('renderer call sites of host-local commands name the execution host',()=>{
  const sources=['AddProjectDialog.tsx','BrowserAnnotationPanel.tsx','BrowserAutomationPanel.tsx','BrowserPane.tsx','CapabilitiesPanel.tsx',
    'ResourceStatus.tsx','ResourceManager.tsx','TerminalView.tsx','Workbench.tsx','browser-surfaces.ts','pr-checks.ts','terminal-clipboard.ts'];
  const offenders=[];
  for(const name of sources)for(const [index,line] of readFileSync(new URL('../src/'+name,import.meta.url),'utf8').split('\n').entries())
    if(/invoke[<(][^)]*'[a-z_']+/.test(line)&&LOCAL_ONLY_IPC.some(command=>line.includes(`'${command}'`))&&!/target[,:}]/.test(line))offenders.push(`${name}:${index+1}`);
  assert.deepEqual(offenders,[],'a host-local IPC call must pass the host it is asking about');
});
