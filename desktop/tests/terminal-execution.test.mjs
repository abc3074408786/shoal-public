import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

function load(name,dependencies={}){
  const source=readFileSync(new URL(`../src/${name}.ts`,import.meta.url),'utf8'),module={exports:{}};
  vm.runInNewContext(ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,
    {module,exports:module.exports,require:name=>dependencies[name]});
  return module.exports;
}
const policy=load('sandbox-policy');
const {terminalExecutionBlockReason,terminalExecutionRequest,restrictedTerminalDescription,terminalSessionTitle,terminalSessionStatus}=load('terminal-execution',{'./sandbox-policy':policy});
const ready={status:'ready',capabilities:{platform:'linux',backend:'bubblewrap',available:false,reason:null,restricted:{platform:'linux',backend:'landlock-seccomp',available:true,reason:null,filesystem_allowlist:true,process_namespace:false,network_modes:['denied'],experimental:true}}};

test('native terminal preserves the existing RPC without sandbox and remains available for managed tasks',()=>{
  assert.equal(terminalExecutionRequest('native',{status:'error',error:'offline'},false,false),undefined);
  assert.equal(terminalExecutionRequest('native',{status:'loading'},false,true),undefined);
});
test('restricted terminal requires its own kernel capabilities and a worktree',()=>{
  assert.equal(terminalExecutionBlockReason('restricted',ready,true,false),'');
  assert.equal(JSON.stringify(terminalExecutionRequest('restricted',ready,true,false)),'{"mode":"restricted"}');
  assert.throws(()=>terminalExecutionRequest('restricted',ready,false,false),/Git/);
  for(const probe of [
    {status:'loading'},
    {status:'error',error:'offline'},
    {status:'ready',capabilities:{...ready.capabilities,restricted:undefined}},
    {status:'ready',capabilities:{...ready.capabilities,restricted:{...ready.capabilities.restricted,available:false}}},
  ])assert.throws(()=>terminalExecutionRequest('restricted',probe,true,false));
});
test('switching to a managed task cannot silently discard a selected restricted policy',()=>{
  assert.throws(()=>terminalExecutionRequest('restricted',ready,true,true),/交互式 Codex 终端/);
  assert.match(terminalExecutionBlockReason('restricted',ready,true,true),/切换为本机/);
  assert.equal(terminalExecutionRequest('native',ready,true,true),undefined);
});
test('terminal scope names the native TUI and separates existing login from offline tools',()=>{
  for(const expected of ['原有 Codex 终端界面','本机 CLI','命令和文件工具','关闭网络','未隔离进程命名空间','不会回退','本机已有的 ChatGPT 登录','沙箱工具无法读取','模型 API 连接','0.153.4','支持 !命令'])
    assert.ok(restrictedTerminalDescription.includes(expected),expected);
  assert.ok(!restrictedTerminalDescription.includes('聊天'));
});

test('reopened restricted terminal is identified without marking other sessions as restricted',()=>{
  const session={title:'Existing terminal',program:'codex',kind:'terminal',execution_mode:'restricted'};
  assert.equal(terminalSessionTitle(session),'Codex · 工具受限');
  for(const patch of [{execution_mode:'native'},{execution_mode:undefined},{kind:'agent'},{program:'bash'}])
    assert.equal(terminalSessionTitle({...session,...patch}),'Existing terminal');
});

test('recovery availability does not override the actual terminal state',()=>{
  assert.equal(terminalSessionStatus({native_chat:true,status:{exit:null},recoverable:true}),'已停用');
  assert.equal(terminalSessionStatus({status:{exit:0},recoverable:true}),'已结束');
  assert.equal(terminalSessionStatus({status:{exit:0},recoverable:false}),'已结束');
  assert.equal(terminalSessionStatus({status:{exit:null},recoverable:false}),'运行中');
  assert.equal(terminalSessionStatus({status:null,recoverable:false}),'尚未就绪');
  assert.equal(terminalSessionStatus({status:{exit:null,state:'host_lost'},recoverable:true}),'已失联');
  assert.equal(terminalSessionStatus({status:{exit:null,state:'waiting_permission'},recoverable:true}),'等待授权');
  assert.equal(terminalSessionStatus({status:{exit:null,state:'waiting_input'},recoverable:false}),'等待输入');
});
