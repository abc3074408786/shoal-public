import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
const source=readFileSync(new URL('../src/sandbox-policy.ts',import.meta.url),'utf8');
const module={exports:{}};
vm.runInNewContext(ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{module,exports:module.exports});
const {buildExecution,emptySandboxDraft,readCapabilities,isolationBlockReason,restrictedBlockReason,executionBlockReason,executionDescription}=module.exports;
const ready={status:'ready',capabilities:{available:true,platform:'linux',backend:'bubblewrap',reason:null}};
const draft=()=>({...emptySandboxDraft(),mode:'isolated'});
const restrictedCapability={platform:'linux',backend:'landlock-seccomp',available:true,reason:null,filesystem_allowlist:true,process_namespace:false,network_modes:['denied'],experimental:true};
const restrictedReady={status:'ready',capabilities:readCapabilities({...ready.capabilities,available:false,restricted:restrictedCapability})};
const restrictedDraft=()=>({...emptySandboxDraft(),mode:'restricted'});
test('native mode remains available without a usable sandbox',()=>{
  for(const probe of [{status:'loading'},{status:'error',error:'offline'},{status:'ready',capabilities:{...ready.capabilities,available:false}}]){
    assert.equal(buildExecution(emptySandboxDraft(),probe,true).mode,'native');
    assert.throws(()=>buildExecution(draft(),probe));
  }
});
test('isolated selection cannot silently degrade on a direct folder or failed probe',()=>{
  assert.throws(()=>buildExecution(draft(),ready,true),/Git/);
  const selected={...draft(),network:'host'};
  assert.throws(()=>buildExecution(selected,{status:'error',error:'changed'}));
  assert.equal(selected.mode,'isolated');assert.equal(selected.network,'host');
});
test('execution payload carries only explicit directory and network grants',()=>{
  const policy=buildExecution({...draft(),readOnly:' /opt/tools\n/opt/tools\n',writable:'/opt/cache'},ready);
  assert.equal(JSON.stringify(policy),JSON.stringify({mode:'isolated',network:'denied',read_only:['/opt/tools'],writable:['/opt/cache']}));
  assert.equal(buildExecution({...draft(),network:'host'},ready).network,'host');
  assert.throws(()=>buildExecution({...draft(),readOnly:'relative/path'},ready),/绝对路径/);
  assert.throws(()=>buildExecution({...draft(),readOnly:'/opt/tools',writable:'/opt/tools'},ready),/同时/);
});
test('invalid or missing capability data never means available',()=>{
  for(const value of [undefined,null,{}, {available:'true',platform:'linux',backend:'bubblewrap'}])assert.throws(()=>readCapabilities(value));
  assert.equal(readCapabilities(ready.capabilities).available,true);
  assert.match(isolationBlockReason({status:'ready',capabilities:{...ready.capabilities,available:false,platform:'windows'}}),/尚未实现/);
});
test('task details describe the saved policy and preserve legacy task behavior',()=>{
  assert.match(executionDescription(undefined),/本机执行/);
  assert.match(executionDescription(undefined,true),/没有自动回滚/);
  assert.match(executionDescription(buildExecution(draft(),ready)),/网络关闭/);
  assert.match(executionDescription(buildExecution({...draft(),network:'host'},ready)),/包含本机服务/);
});
test('restricted execution is independently available when bubblewrap cannot start',()=>{
  assert.notEqual(isolationBlockReason(restrictedReady),'');
  assert.equal(restrictedBlockReason(restrictedReady),'');
  const payload=buildExecution({...restrictedDraft(),readOnly:'/opt/tools',writable:'/opt/cache'},restrictedReady);
  assert.equal(JSON.stringify(payload),JSON.stringify({mode:'restricted',network:'denied',read_only:['/opt/tools'],writable:['/opt/cache']}));
  assert.throws(()=>buildExecution(draft(),restrictedReady));
});
test('missing and malformed restricted capability fail closed without disabling legacy isolation',()=>{
  for(const restricted of [undefined,null,{}, {...restrictedCapability,backend:'unknown'}, {...restrictedCapability,process_namespace:true}, {...restrictedCapability,experimental:false}, {...restrictedCapability,network_modes:['host']}, {...restrictedCapability,network_modes:['denied','host']}]){
    const probe={status:'ready',capabilities:readCapabilities({...ready.capabilities,restricted})};
    assert.notEqual(restrictedBlockReason(probe),'');
    assert.throws(()=>buildExecution(restrictedDraft(),probe));
    assert.equal(buildExecution(draft(),probe).mode,'isolated');
  }
});
test('restricted execution requires the filesystem and denied-network capabilities on Linux',()=>{
  for(const override of [{available:false},{filesystem_allowlist:false},{network_modes:[]},{platform:'windows'}]){
    const probe={status:'ready',capabilities:readCapabilities({...ready.capabilities,restricted:{...restrictedCapability,...override}})};
    assert.notEqual(restrictedBlockReason(probe),'');
    assert.throws(()=>buildExecution(restrictedDraft(),probe));
  }
  assert.throws(()=>buildExecution(restrictedDraft(),restrictedReady,true),/Git/);
  assert.throws(()=>buildExecution({...restrictedDraft(),network:'host'},restrictedReady),/仅支持关闭网络/);
});
test('rechecking cannot silently replace a restricted selection or permit submission',()=>{
  const selected={...restrictedDraft(),readOnly:'/opt/tools'};
  for(const probe of [{status:'loading'},{status:'error',error:'changed'},ready]){
    assert.notEqual(executionBlockReason(selected.mode,probe),'');
    assert.throws(()=>buildExecution(selected,probe));
    assert.equal(selected.mode,'restricted');assert.equal(selected.readOnly,'/opt/tools');
    assert.equal(executionBlockReason('native',probe,true),'');
  }
});
test('restricted task description states the boundary and offline model limitation',()=>{
  const description=executionDescription(buildExecution(restrictedDraft(),restrictedReady));
  for(const expected of [/实验性/,/网络关闭/,/独立 HOME/,/未隔离进程命名空间/,/准备脚本、Agent、验证和任务 Shell/,/无法连接远程模型或联网登录/,/失败不会转为本机执行/])assert.match(description,expected);
});
