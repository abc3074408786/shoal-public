import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const hostId='018fa61b-4db2-7000-8000-0000000000aa';
const serverId='018fa61b-4db2-7000-8000-0000000000bb';
const otherServer='018fa61b-4db2-7000-8000-0000000000cc';
const otherHost='018fa61b-4db2-7000-8000-0000000000dd';
const target={kind:'ssh',host_id:hostId,server_id:serverId};
const source=path=>readFileSync(new URL(path,import.meta.url),'utf8');
const moduleUrl=code=>'data:text/javascript;base64,'+Buffer.from(code).toString('base64');
const stubUrl=moduleUrl(`export const calls=[];
export const isTauri=()=>!!globalThis.velellaDesktop;
export async function invoke(command,args){calls.push({command,args});return globalThis.velellaResult;}`);
const targetUrl=moduleUrl(ts.transpileModule(source('../src/execution-target.ts'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText);
const apiUrl=moduleUrl(ts.transpileModule(source('../src/api.ts'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText
  .replaceAll("from './desktop-platform'",`from '${stubUrl}'`)
  .replaceAll("from './execution-target'",`from '${targetUrl}'`));
const stub=await import(stubUrl);
const {checkedTarget,currentTarget,selectTarget,sameTarget,namespacedKey,LOCAL_TARGET}=await import(targetUrl);
const api=await import(apiUrl);
function reset(){stub.calls.length=0;delete globalThis.velellaDesktop;delete globalThis.velellaResult;}
async function withBridge(result,run){
  globalThis.velellaDesktop={invoke:()=>Promise.resolve(result)};globalThis.velellaResult=result;
  try{return await run();}finally{delete globalThis.velellaDesktop;delete globalThis.velellaResult;}
}
async function withFetch(run){
  const seen=[];globalThis.fetch=async(input,init)=>{seen.push([input,init?.body]);return {json:async()=>({ok:true,echo:JSON.parse(init.body)})};};
  try{return {seen,result:await run()};}finally{delete globalThis.fetch;}
}
test('execution targets are closed, frozen and identity-bound',()=>{
  assert.deepEqual(checkedTarget(),LOCAL_TARGET);
  assert.deepEqual(checkedTarget(null),LOCAL_TARGET);
  assert.deepEqual(checkedTarget({kind:'local'}),{kind:'local'});
  assert.deepEqual(checkedTarget(target),target);
  assert.equal(Object.isFrozen(checkedTarget(target)),true);
  for(const bad of [{kind:'ssh',host_id:hostId},{kind:'ssh',host_id:hostId,server_id:'x'},{kind:'ssh',host_id:hostId,server_id:serverId,data_root:'/srv'},{kind:'tcp',host_id:hostId,server_id:serverId},'ssh',42,[]])
    assert.throws(()=>checkedTarget(bad),/execution target/,String(bad));
  assert.equal(sameTarget(),true);
  assert.equal(sameTarget(target,{kind:'ssh',host_id:hostId,server_id:otherServer}),false);
  // Local keys keep the spelling that already exists in users' browsers; remote keys isolate by
  // host and server identity so a recreated host cannot inherit another server's drafts.
  assert.equal(namespacedKey(LOCAL_TARGET,'velella:draft:w:p'),'velella:draft:w:p');
  assert.equal(namespacedKey(target,'draft'),`ssh:${hostId}:${serverId}:draft`);
  assert.notEqual(namespacedKey(target,'draft'),namespacedKey({kind:'ssh',host_id:hostId,server_id:otherServer},'draft'));
  assert.notEqual(namespacedKey(target,'draft'),namespacedKey({kind:'ssh',host_id:otherHost,server_id:serverId},'draft'));
});
test('switching hosts never rewrites a request already bound to the previous one',async()=>{
  reset();
  assert.deepEqual(currentTarget(),LOCAL_TARGET);
  const bound=checkedTarget(target);
  selectTarget(target);assert.deepEqual(currentTarget(),target);
  selectTarget(LOCAL_TARGET);assert.deepEqual(currentTarget(),LOCAL_TARGET);
  await withBridge({ok:true,routed:'remote'},async()=>{
    await api.rpc({cmd:'workbench.sessions'},bound);
    assert.deepEqual(stub.calls.at(-1),{command:'rpc',args:{request:{cmd:'workbench.sessions'},target:bound}});
    selectTarget(target);
    await api.rpc({cmd:'workbench.sessions'});
    assert.deepEqual(stub.calls.at(-1).args.target,target,'a call issued while a host is active uses that host');
    selectTarget(LOCAL_TARGET);
  });
});
test('remote calls refuse the development bridge instead of using the local daemon',async()=>{
  reset();
  const local=await withFetch(()=>api.rpc({cmd:'workbench.snapshot'}));
  assert.equal(local.seen.length,1,'local calls keep the existing development transport');
  await assert.rejects(api.rpc({cmd:'workbench.snapshot'},target),/不会把请求转到本机守护进程/);
  assert.deepEqual(stub.calls,[]);
  assert.equal(local.seen.length,1,'a remote request leaked into the local fetch transport');
  await assert.rejects(api.executionHosts({action:'list'}),/执行主机管理仅在桌面正式入口可用/);
  await withBridge({ok:true,routed:'managed'},async()=>{
    assert.deepEqual(await api.executionHosts({action:'inspect',id:hostId}),{ok:true,routed:'managed'});
    assert.deepEqual(stub.calls.at(-1),{command:'execution_hosts',args:{action:'inspect',id:hostId}});
  });
});
test('remote failures surface the host error without a local retry',async()=>{
  reset();
  await withBridge({ok:false,error:'远端执行主机未连接或身份不匹配；不会转到本机'},async()=>{
    await assert.rejects(api.rpc({cmd:'workbench.create'},target),/不会转到本机/);
    assert.equal(stub.calls.length,1,'a failed remote write must not be retried');
  });
  reset();
  const local=await withFetch(()=>api.rpc({cmd:'workbench.snapshot'},{kind:'local'}));
  assert.equal(local.seen[0][0],'/_velella');
  assert.equal(local.seen[0][1],JSON.stringify({cmd:'workbench.snapshot'}));
});
test('a desktop bridge call stays on the local daemon without an injected target field',async()=>{
  reset();
  await withBridge({ok:true},async()=>{
    await api.rpc({cmd:'workbench.snapshot'});
    assert.deepEqual(stub.calls.at(-1),{command:'rpc',args:{request:{cmd:'workbench.snapshot'}}});
  });
});
test('file draft keys keep the local spelling and isolate remote workspaces',async()=>{
  reset();
  const typesUrl=moduleUrl(ts.transpileModule(source('../src/workbench-types.ts'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText
    .replaceAll("from './execution-target'",`from '${targetUrl}'`));
  const {draftKey,draftPrefix}=await import(typesUrl);
  assert.equal(draftKey('w','src/a.ts'),'velella:draft:w:src/a.ts');
  assert.equal(draftPrefix('w'),'velella:draft:w:');
  selectTarget(target);
  try{
    assert.equal(draftKey('w','src/a.ts'),`ssh:${hostId}:${serverId}:velella:draft:w:src/a.ts`);
    assert.equal(draftKey('w','src/a.ts').startsWith(draftPrefix('w')),true);
    assert.equal(draftKey('w','src/a.ts').startsWith('velella:draft:w:'),false,'local draft scans must never count remote drafts');
  }finally{selectTarget(LOCAL_TARGET)}
  assert.equal(draftKey('w','src/a.ts'),'velella:draft:w:src/a.ts');
});
test('PR checks carry the issuing host and never read a local repository for a remote workspace',async()=>{
  reset();
  const prUrl=moduleUrl(ts.transpileModule(source('../src/pr-checks.ts'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText
    .replaceAll("from './desktop-platform'",`from '${stubUrl}'`)
    .replaceAll("from './execution-target'",`from '${targetUrl}'`));
  const {readPrChecks}=await import(prUrl);
  globalThis.velellaDesktop={invoke:()=>Promise.resolve(globalThis.velellaResult),listen:()=>()=>{}};
  globalThis.window={velellaDesktop:globalThis.velellaDesktop};
  globalThis.velellaResult={state:'ready',message:'',checked_at:'2026-10-29T00:00:00Z',pull_requests:[]};
  try{
    assert.equal((await readPrChecks('w')).state,'ready');
    assert.deepEqual(stub.calls.at(-1),{command:'pr_checks',args:{workspace_id:'w',target:{kind:'local'}}});
    stub.calls.length=0;selectTarget(target);
    try{
      const remote=await readPrChecks('w');
      assert.equal(remote.state,'remote_unsupported');
      assert.match(remote.message,/不会用本机结果代替远端/);
      assert.deepEqual(stub.calls,[],'a remote workspace must not reach the local repository reader');
    }finally{selectTarget(LOCAL_TARGET)}
    await readPrChecks('w');
    assert.deepEqual(stub.calls.at(-1).args.target,{kind:'local'},'the host is read again for each request');
  }finally{delete globalThis.window;delete globalThis.velellaDesktop;delete globalThis.velellaResult;stub.calls.length=0}
});
