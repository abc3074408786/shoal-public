import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
import xterm from '@xterm/xterm';
const compiled=ts.transpileModule(readFileSync(new URL('../src/terminal-input.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {subscribeTerminalInput,isProtocolReply}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
test('SGR mouse motion and clicks are not editing keystrokes',()=>{
 const term=new xterm.Terminal(),sent=[],input=subscribeTerminalInput(term,(text,kind)=>sent.push({text,kind}));
 try{for(const text of ['\x1b[<35;37;26M','\x1b[<0;37;26M','\x1b[<0;37;26m'])term.input(text,true);assert.equal(sent.length,3);assert.ok(sent.every(x=>x.kind==='pointer'))}finally{input.dispose();term.dispose()}
});
test('real xterm distinguishes user input from live protocol replies',async()=>{
 const term=new xterm.Terminal(),sent=[],input=subscribeTerminalInput(term,(text,kind)=>sent.push({text,kind}));
 try{
  assert.equal(input.supported,true);
  term.input('继续执行',true);term.input('\r',true);
  await input.write('\x1b[c\x1b[5n\x1b[?1004h',false);
  assert.deepEqual(sent,[{text:'继续执行',kind:'user'},{text:'\r',kind:'user'},{text:'\x1b[?1;2c',kind:'protocol'},{text:'\x1b[0n',kind:'protocol'}]);
 }finally{input.dispose();term.dispose()}
});
test('replay and unclassified text never reach PTY, user input retains provenance during replay',async()=>{
 const term=new xterm.Terminal(),sent=[],input=subscribeTerminalInput(term,(text,kind)=>sent.push({text,kind}));
 try{
  const writing=input.write('\x1b[c\x1b[5n\x1b[?1004h',true);
  term.input('typed while replaying',true);term.input('，，有',false);await writing;
  assert.deepEqual(sent,[{text:'typed while replaying',kind:'user'}]);
  input.dispose();term.input('stale composition',true);assert.equal(sent.length,1);
 }finally{term.dispose()}
});
test('Chinese, paste, interrupt and modified keys are real input, not regex-classified replies',()=>{
 const term=new xterm.Terminal(),sent=[],input=subscribeTerminalInput(term,(text,kind)=>sent.push({text,kind}));
 try{const keys=['中文','\x03','\x1b[A','\x1bOR','\x1b[1;2R','\x1b[200~多行\n粘贴\x1b[201~'];for(const text of keys)term.input(text,true);assert.deepEqual(sent,keys.map(text=>({text,kind:'user'})))}finally{input.dispose();term.dispose()}
});
test('missing xterm provenance fails closed instead of reclassifying automatic data as input',()=>{
 let listener;const term={options:{},onData(fn){listener=fn;return {dispose(){}}}};
 const sent=[],input=subscribeTerminalInput(term,(...args)=>sent.push(args));assert.equal(input.supported,false);assert.equal(term.options.disableStdin,true);listener('unexpected');assert.deepEqual(sent,[]);input.dispose();
});
test('protocol reply validation rejects shell text and concatenated commands',()=>{
 for(const value of ['ls\r','，，有','\x1b[?1;2c\r','\x1b]11;rgb:0000/0000/0000\x1b\\whoami\r'])assert.equal(isProtocolReply(value),false);
 assert.equal(isProtocolReply('\x1b]11;rgb:0000/0000/0000\x1b\\'),true);
});
