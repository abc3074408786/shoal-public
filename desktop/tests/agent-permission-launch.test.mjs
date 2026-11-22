import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const source=readFileSync(new URL('../src/agent-permission-launch.ts',import.meta.url),'utf8').replace("import {rpc} from './api';",'let handler;export function useRpc(value){handler=value}const rpc=(...args)=>handler(...args);');
const js=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {permissionLaunchFields,useRpc}=await import('data:text/javascript;base64,'+Buffer.from(js).toString('base64'));
test('inherit adds no fields and never probes a host',async()=>{
 useRpc(()=>{throw Error('must not call')});
 assert.deepEqual(await permissionLaunchFields('inherit',{kind:'local'}),{});
});
test('a requested override must be acknowledged by the exact execution host',async()=>{
 const target={kind:'ssh',host_id:'host',server_id:'server'};
 useRpc(async(request,bound)=>{assert.deepEqual(request,{cmd:'protocol'});assert.equal(bound,target);return {capabilities:[]}});
 await assert.rejects(permissionLaunchFields('ask',target),/尚不支持/);
 useRpc(async(_request,bound)=>{assert.equal(bound,target);return {capabilities:['terminal-permissions-v1']}});
 assert.deepEqual(await permissionLaunchFields('ask',target),{permission_mode:'ask'});
 useRpc(async()=>{throw Error('host offline')});
 await assert.rejects(permissionLaunchFields('ask',target),/host offline/);
});
