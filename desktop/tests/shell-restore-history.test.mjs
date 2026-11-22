import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
import xterm from '@xterm/xterm';
const compile=async name=>import('data:text/javascript;base64,'+Buffer.from(ts.transpileModule(readFileSync(new URL('../src/'+name,import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText).toString('base64'));
const {resetTerminalWithHistory}=await compile('terminal-frames.ts'),{writeTerminalPage}=await compile('terminal-replay.ts');
const encoder=new TextEncoder(),text=t=>Array.from({length:t.buffer.active.length},(_,i)=>t.buffer.active.getLine(i)?.translateToString(true)).join('\n');
test('real process reset preserves styled shell history while the new prompt can repaint at row one',async()=>{
 const t=new xterm.Terminal({cols:80,rows:10,scrollback:1000,allowProposedApi:true});
 const writes=[],write=(bytes,history)=>new Promise(resolve=>{writes.push(history);t.write(bytes,resolve)});
 const prior=encoder.encode('\x1b[31m原来的结果 OLD_RESULT\x1b[0m\r\nPS original> '),next=encoder.encode('\x1b[1;1HPS restored> '),all=new Uint8Array([...prior,...next]);
 const cursor={reset:async at=>{if(at>0)await resetTerminalWithHistory(t,write);else t.reset()}};
 try{
  await writeTerminalPage(all,0,all.length,[{offset:0,rows:10,cols:80,reset:true,sequence:0},{offset:prior.length,rows:10,cols:80,reset:true,sequence:1}],t,write,cursor);
  assert.match(text(t),/原来的结果 OLD_RESULT/);assert.match(text(t),/PS restored>/);
  const row=Array.from({length:t.buffer.normal.length},(_,i)=>t.buffer.normal.getLine(i)).find(l=>l.translateToString(true).includes('OLD_RESULT'));
  assert.equal(row.getCell(0).getFgColor(),1,'original red text must retain its palette colour');
  assert.ok(t.buffer.normal.baseY>0,'old viewport must be scrollback before the new cursor-home repaint');
  assert.ok(writes.every(Boolean),'restoring display must never emit live protocol input');
  const again=encoder.encode('\x1b[1;1HPS restored again> ');
  await writeTerminalPage(again,all.length,all.length+again.length,[{offset:all.length,rows:10,cols:80,reset:true,sequence:2}],t,write,cursor);
  assert.match(text(t),/OLD_RESULT/);assert.match(text(t),/PS restored>/);assert.match(text(t),/PS restored again>/);
 }finally{t.dispose()}
});
test('default native TUI reset still clears the prior screen',async()=>{
 const t=new xterm.Terminal({cols:80,rows:10});const write=bytes=>new Promise(resolve=>t.write(bytes,resolve));
 try{await write(encoder.encode('OLD_TUI'));await writeTerminalPage(encoder.encode('NEW_TUI'),7,14,[{offset:7,rows:10,cols:80,reset:true,sequence:0}],t,write,{});assert.doesNotMatch(text(t),/OLD_TUI/);assert.match(text(t),/NEW_TUI/)}finally{t.dispose()}
});
