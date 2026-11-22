import {test} from 'node:test';import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';import {runInNewContext} from 'node:vm';import ts from 'typescript';
import resources from '../electron/desktop-resources.cjs';
const source=ts.transpileModule(readFileSync(new URL('../src/resource-status.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS}}).outputText;
const exports={};runInNewContext(source,{exports});
test('native totals use private KiB, exclude unrequested fields and cache for five seconds',()=>{
 let calls=0,time=10000;const sample=resources.createDesktopResources({getAppMetrics(){calls++;return [{pid:1,type:'Browser',name:'not exported',cpu:{percentCPUUsage:99},memory:{privateBytes:1024,workingSetSize:99999}},{pid:2,type:'GPU',memory:{privateBytes:2048}}]}},()=>time);
 const first=sample();assert.equal(first.privateBytes,3*1024*1024);assert.equal(first.processes[0].privateBytes,1024*1024);assert.equal('name' in first.processes[0],false);assert.equal('cpu' in first.processes[0],false);
 time+=4999;assert.equal(sample(),first);assert.equal(calls,1);time++;sample();assert.equal(calls,2);
});
test('missing native data is unavailable instead of a partial total or zero',()=>{
 for(const items of [[],[{pid:1,type:'Browser',memory:{workingSetSize:1000}}],[{pid:1,type:'Tab',memory:{privateBytes:NaN}}]])assert.equal(resources.summarizeResources(items,1).privateBytes,null);
 assert.equal(exports.formatResourceBytes(null),'—');assert.equal(exports.formatResourceBytes(1.73e9),'1.73 GB');assert.equal(exports.formatResourceBytes(320e6),'320 MB');
});
test('count includes live terminal hosts and excludes history, missing and lost hosts',()=>{
 const live={id:'live',kind:'terminal',native_chat:false,status:{pid:42,exit:null,state:'running'}};
 const sessions=[live,{...live,id:'ended',status:{...live.status,exit:0}},{...live,id:'lost',status:{...live.status,state:'host_lost'}},{...live,id:'missing',status:null},{...live,id:'chat',native_chat:true},{...live,id:'invalid',status:{...live.status,pid:0}},{...live,id:'agent',kind:'agent'}];
 assert.equal(exports.runningTerminals(sessions).length,1);assert.equal(exports.runningTerminals(sessions)[0].id,'live');
});
test('connection distinguishes connecting, retrying, failed and recovered',()=>{
 assert.equal(exports.connectionState(false,0),'connecting');assert.equal(exports.connectionState(false,1),'reconnecting');assert.equal(exports.connectionState(false,3),'failed');assert.equal(exports.connectionState(true,3),'connected');
});
