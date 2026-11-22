import {test} from 'node:test';import assert from 'node:assert/strict';import {readFileSync} from 'node:fs';import ts from 'typescript';
const compiled=ts.transpileModule(readFileSync(new URL('../src/terminal-input-queue.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {createTerminalInputQueue}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
test('an ambiguous send pauses input, cancels queued keys and never replays them on recovery',async()=>{
 let fail;const calls=[],errors=[];const queue=createTerminalInputQueue(error=>errors.push(String(error)));
 const first=queue.send(()=>{calls.push('first');return new Promise((_,reject)=>{fail=reject})},1);
 const second=queue.send(async()=>{calls.push('queued')},1);
 const outcomes=Promise.allSettled([first,second]);await Promise.resolve();fail(new Error('response lost'));await outcomes;await queue.drain();
 assert.deepEqual(calls,['first']);assert.equal(errors.length,1);
 queue.resume();await queue.send(async()=>{calls.push('new input')},1);assert.deepEqual(calls,['first','new input']);queue.dispose();
});
test('input is bounded and disposal cancels queued work without sending to a detached session',async()=>{
 const calls=[],queue=createTerminalInputQueue(()=>{});let release;
 const first=queue.send(()=>new Promise(resolve=>{release=resolve}),1024*1024);
 const extra=queue.send(async()=>{calls.push('overflow')},1);const results=Promise.allSettled([first,extra]);
 // Overflow invalidates the first item if it has not begun, too.
 await results;assert.deepEqual(calls,[]);queue.dispose();
});
