import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const code=ts.transpileModule(readFileSync(new URL('../src/terminal-performance.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {createTerminalPerformance}=await import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'));
test('timing history is bounded, ordered and independent of exported mutations',()=>{
 let clock=1000;const trace=createTerminalPerformance('session',()=>clock++);
 for(let i=0;i<600;i++)trace.record('output_parse',{ms:i/10,bytes:4});
 const report=trace.snapshot();assert.equal(report.retained,512);assert.equal(report.dropped,88);
 assert.equal(report.entries[0].sequence,89);assert.equal(report.entries.at(-1).sequence,600);
 report.entries[0].ms=999;assert.notEqual(trace.snapshot().entries[0].ms,999);
});
test('timing export excludes content, arbitrary fields and invalid stage/number values',()=>{
 const trace=createTerminalPerformance('fixture',()=>1000);
 trace.record('native_input',{kind:'enter',command:'secret command',text:'secret output',clipboard:'secret image',ms:Infinity,bytes:-1});
 trace.record('secret stage',{kind:'secret kind'});
 const report=trace.snapshot(),json=JSON.stringify(report);
 assert.equal(report.retained,1);assert.equal(report.entries[0].kind,'enter');assert.equal(report.entries[0].ms,undefined);
 assert.ok(!json.includes('secret'));assert.equal(report.entries[0].bytes,undefined);
});
