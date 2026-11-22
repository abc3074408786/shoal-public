import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';

const source=readFileSync(new URL('../src/agent-check-queue.ts',import.meta.url),'utf8');
const js=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {scheduleAgentCheck}=await import('data:text/javascript;base64,'+Buffer.from(js).toString('base64'));
const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no});return {promise,resolve,reject}};
const turn=()=>new Promise(resolve=>setImmediate(resolve));

test('same host runs only one real probe at a time and preserves all results',async()=>{
 const scan=new AbortController(),gates=[deferred(),deferred(),deferred()],started=[];
 let active=0,maximum=0;
 const checks=gates.map((gate,index)=>scheduleAgentCheck('serial-host',async()=>{
  active++;maximum=Math.max(maximum,active);started.push(index);
  try{return await gate.promise}finally{active--}
 },scan.signal));
 await turn();assert.deepEqual(started,[0]);
 gates[0].resolve({version:'one'});await turn();assert.deepEqual(started,[0,1]);
 gates[1].resolve({available:false});await turn();assert.deepEqual(started,[0,1,2]);
 gates[2].resolve({authenticated:false});
 assert.deepEqual(await Promise.all(checks),[{version:'one'},{available:false},{authenticated:false}]);
 assert.equal(maximum,1);
});

test('closed scan skips queued probes and a new scan waits for its active RPC',async()=>{
 const closed=new AbortController(),fresh=new AbortController(),gate=deferred(),calls=[];
 const active=scheduleAgentCheck('closed-host',()=>{calls.push('active');return gate.promise},closed.signal);
 const skipped=scheduleAgentCheck('closed-host',async()=>{calls.push('must not start')},closed.signal);
 const rejected=assert.rejects(skipped,{name:'AbortError'});
 await turn();closed.abort();
 const next=scheduleAgentCheck('closed-host',async()=>{calls.push('fresh');return 'fresh result'},fresh.signal);
 await turn();assert.deepEqual(calls,['active']);
 gate.resolve('actual old RPC result');assert.equal(await active,'actual old RPC result');
 await rejected;assert.equal(await next,'fresh result');assert.deepEqual(calls,['active','fresh']);
});

test('already aborted scans launch nothing and do not poison a later fresh scan',async()=>{
 const scan=new AbortController();scan.abort();let starts=0;
 await assert.rejects(scheduleAgentCheck('aborted-host',async()=>{starts++},scan.signal),{name:'AbortError'});
 assert.equal(await scheduleAgentCheck('aborted-host',async()=>{starts++;return 'new'},new AbortController().signal),'new');
 assert.equal(starts,1);
});

test('actual errors and synchronous throws release the slot without automatic retry',async()=>{
 const signal=new AbortController().signal,expected=new Error('native CLI unavailable');let calls=0;
 const failure=scheduleAgentCheck('failure-host',()=>{calls++;throw expected},signal);
 const rejected=assert.rejects(failure,error=>error===expected);
 const next=scheduleAgentCheck('failure-host',async()=>{calls++;return 'continued'},signal);
 await rejected;assert.equal(await next,'continued');assert.equal(calls,2);
 const remoteError=new Error('remote probe failed');
 await assert.rejects(scheduleAgentCheck('failure-host',()=>Promise.reject(remoteError),signal),error=>error===remoteError);
 assert.equal(await scheduleAgentCheck('failure-host',async()=>42,signal),42);
});

test('different execution targets proceed independently with captured account parameters',async()=>{
 const signal=new AbortController().signal,local=deferred();let remoteStarted=false;
 const localCheck=scheduleAgentCheck('local',()=>local.promise,signal);
 const target={kind:'ssh',host_id:'host-a',server_id:'server-a'},request={agent:'claude-code',account_home:'isolated-account'};
 const remote=await scheduleAgentCheck('ssh:host-a:server-a',async()=>{remoteStarted=true;return {target,request}},signal);
 assert.equal(remoteStarted,true);assert.equal(remote.target,target);assert.equal(remote.request,request);
 local.resolve('local result');assert.equal(await localCheck,'local result');
});

test('refresh always performs a new probe rather than returning cached status',async()=>{
 const signal=new AbortController().signal;let revision=0;
 assert.equal(await scheduleAgentCheck('refresh-host',async()=>++revision,signal),1);
 assert.equal(await scheduleAgentCheck('refresh-host',async()=>++revision,signal),2);
 assert.equal(revision,2);
});
