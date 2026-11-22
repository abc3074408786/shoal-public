import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
import xterm from '@xterm/xterm';
const compiled=ts.transpileModule(readFileSync(new URL('../src/terminal-resize.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {createTerminalResize}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
test('recorded live terminal measures and requests size without reflowing output ahead of the host',async()=>{
 const term=new xterm.Terminal({cols:100,rows:30}),sent=[];
 const resize=createTerminalResize(()=>({cols:80,rows:24}),s=>term.resize(s.cols,s.rows),()=>true,async s=>sent.push(s),assert.fail);
 try{resize.request();assert.equal(term.cols,100);assert.deepEqual(sent,[{cols:80,rows:24}])}finally{resize.dispose();term.dispose()}
});
const tick=()=>new Promise(resolve=>setImmediate(resolve));

test('returning to a narrow viewer reclaims geometry after another window resized the shared PTY',async()=>{
 const sent=[],resize=createTerminalResize(()=>({cols:72,rows:40}),()=>assert.fail('recorded output must keep its timeline'),()=>true,async s=>sent.push(s),assert.fail);
 try{
  resize.request();await tick();
  resize.observe({cols:72,rows:40});resize.request();await tick();
  assert.equal(sent.length,1,'matching acknowledged geometry stays deduplicated');
  resize.observe({cols:144,rows:48});
  assert.equal(sent.length,1,'observing a background window never starts a resize loop');
  resize.request();await tick();
  assert.deepEqual(sent,[{cols:72,rows:40},{cols:72,rows:40}],'the narrow viewer must request its size again despite last sending that same size');
  resize.request();await tick();assert.equal(sent.length,2);
 }finally{resize.dispose()}
});

test('temporarily collapsed views never shrink the persistent PTY to the fit addon minimum',async()=>{
 let size={cols:80,rows:24};const sent=[];
 const resize=createTerminalResize(()=>size,()=>{},()=>true,async s=>sent.push(s),assert.fail);
 resize.request();await tick();size={cols:2,rows:1};resize.request();await tick();
 assert.deepEqual(sent,[{cols:80,rows:24}]);resize.dispose();
});
test('rapid drag serializes RPC and coalesces intermediate sizes to the latest request',async()=>{
 let size={cols:80,rows:24},release;
 const sent=[],resize=createTerminalResize(()=>size,()=>assert.fail('no eager resize'),()=>true,s=>{sent.push(s);return new Promise(resolve=>{release=resolve})},assert.fail);
 resize.request();size={cols:90,rows:30};resize.request();size={cols:120,rows:35};resize.request();
 assert.deepEqual(sent,[{cols:80,rows:24}]);release();await tick();
 assert.deepEqual(sent,[{cols:80,rows:24},{cols:120,rows:35}]);release();await tick();resize.request();await tick();
 assert.equal(sent.length,2);resize.dispose();
});
test('switching sessions drops the queued size and never touches a disposed terminal',async()=>{
 let size={cols:80,rows:24},release;const sent=[];
 const resize=createTerminalResize(()=>size,()=>{},()=>true,s=>{sent.push(s);return new Promise(resolve=>{release=resolve})},assert.fail);
 resize.request();size={cols:100,rows:30};resize.request();resize.dispose();release();await tick();resize.request();
 assert.equal(sent.length,1);
});
test('legacy host still fits locally; failed request reports once and is retried only on a new request',async()=>{
 let calls=0;const applied=[],errors=[];
 const resize=createTerminalResize(()=>({cols:100,rows:30}),s=>applied.push(s),()=>false,async()=>{calls++;if(calls===1)throw new Error('offline')},e=>errors.push(String(e)));
 resize.request();await tick();assert.equal(calls,1);assert.equal(errors.length,1);assert.equal(applied.length,1);
 resize.request();await tick();assert.equal(calls,2);resize.dispose();
});
