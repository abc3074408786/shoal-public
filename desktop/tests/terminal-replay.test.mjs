import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
import xterm from '@xterm/xterm';
async function load(name){const compiled=ts.transpileModule(readFileSync(new URL(`../src/${name}.ts`,import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;return import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'))}
const {writeTerminalPage}=await load('terminal-replay');
const {subscribeTerminalInput}=await load('terminal-input');
test('absolute cursor TUI history uses recorded geometry before writing, regardless of viewer size',async()=>{
 for(const cols of [80,160]){
  const term=new xterm.Terminal({cols,rows:24}),sent=[],input=subscribeTerminalInput(term,(...args)=>sent.push(args));
  try{
   const bytes=Buffer.from('\x1b[2J\x1b[1;90H边界\x1b[2;1H第二行\x1b[c');
   await writeTerminalPage(bytes,0,bytes.length,[{offset:0,cols:100,rows:30,reset:true}],term,input.write);
   assert.equal(term.cols,100);
   assert.equal(term.buffer.active.getLine(0).translateToString(true).slice(89),'边界');
   assert.equal(term.buffer.active.getLine(1).translateToString(true),'第二行');
   assert.deepEqual(sent,[]);
  }finally{input.dispose();term.dispose()}
 }
});
test('resize boundaries and split UTF-8 chunks produce the same screen as original writes',async()=>{
 const term=new xterm.Terminal({cols:80,rows:24}),expected=new xterm.Terminal({cols:80,rows:24});
 const input=subscribeTerminalInput(term,()=>assert.fail('replay must not send input'));
 const write=(value)=>new Promise(resolve=>expected.write(value,resolve));
 try{
  const first=Buffer.from('\x1b[2J\x1b[1;90H旧画面\x1b[c');
  const second=Buffer.from('\x1b[2J\x1b[2;110H中文\x1b[3;1HDONE\x1b[5n');
  expected.resize(100,30);await write(first);expected.resize(120,35);await write(second);
  const all=Buffer.concat([first,second]);
  const geometry=[{offset:0,cols:100,rows:30},{offset:first.length,cols:120,rows:35}];
  // End the first page inside a UTF-8 codepoint, then cross a resize in the next.
  const cut=first.indexOf(Buffer.from('旧'))+1;
  await writeTerminalPage(all.subarray(0,cut),0,all.length,[geometry[0]],term,input.write);
  await writeTerminalPage(all.subarray(cut),cut,all.length,geometry,term,input.write);
  assert.equal(term.cols,expected.cols);assert.equal(term.rows,expected.rows);
  for(let row=0;row<35;row++)assert.equal(term.buffer.active.getLine(row).translateToString(true),expected.buffer.active.getLine(row).translateToString(true));
 }finally{input.dispose();term.dispose();expected.dispose()}
});
test('history cutoff suppresses replies but live output still answers; other viewers resize live geometry',async()=>{
 const term=new xterm.Terminal({cols:90,rows:24}),sent=[],input=subscribeTerminalInput(term,(...args)=>sent.push(args));
 try{
  const query=Buffer.from('\x1b[c'),bytes=Buffer.concat([query,query]);
  await writeTerminalPage(bytes,0,query.length,[],term,input.write);
  assert.deepEqual(sent,[['\x1b[?1;2c','protocol']]);
  await writeTerminalPage(query,bytes.length,query.length,[{offset:0,cols:80,rows:24,reset:true}],term,input.write);
  assert.equal(term.cols,80);
 }finally{input.dispose();term.dispose()}
});
test('empty polls apply live resize once without repeatedly resetting a new process',async()=>{
 const term=new xterm.Terminal(),input=subscribeTerminalInput(term,()=>{}),state={};
 try{
  const geometry=[{offset:0,cols:100,rows:30,reset:true,sequence:0}];
  await writeTerminalPage(new Uint8Array(),0,0,geometry,term,input.write,state);
  await writeTerminalPage(Buffer.from('retained'),0,0,geometry,term,input.write,state);
  assert.equal(term.buffer.active.getLine(0).translateToString(true),'retained');
  await writeTerminalPage(new Uint8Array(),8,0,[{offset:8,cols:110,rows:32,sequence:1}],term,input.write,state);
  assert.equal(term.cols,110);assert.equal(term.buffer.active.getLine(0).translateToString(true),'retained');
 }finally{input.dispose();term.dispose()}
});
test('legacy recording leaves selected dimensions alone; restarted recording clears old screen',async()=>{
 const term=new xterm.Terminal({cols:120,rows:30}),input=subscribeTerminalInput(term,()=>{});
 try{
  const old=Buffer.from('old process');await writeTerminalPage(old,0,100,[],term,input.write);assert.equal(term.cols,120);
  await writeTerminalPage(Buffer.from('new'),old.length,100,[{offset:old.length,cols:80,rows:24,reset:true}],term,input.write);
  assert.equal(term.buffer.active.getLine(0).translateToString(true),'new');
 }finally{input.dispose();term.dispose()}
});

test('historical collapsed-view geometry does not reflow the saved screen into two columns',async()=>{
 const term=new xterm.Terminal({cols:120,rows:30}),input=subscribeTerminalInput(term,()=>assert.fail('history must not send input')),state={};
 try{
  const first=Buffer.from('before collapse\r\n');
  const next=Buffer.from('retained output\r\n');
  const end=first.length+next.length;
  await writeTerminalPage(first,0,end,[{offset:0,cols:120,rows:30,reset:true,sequence:0}],term,input.write,state);
  const tiny={offset:first.length,cols:2,rows:1,sequence:1};
  await writeTerminalPage(next,first.length,end,[tiny],term,input.write,state);
  assert.equal(term.cols,120);assert.equal(term.rows,30);
  assert.equal(term.buffer.active.getLine(0).translateToString(true),'before collapse');
  assert.equal(term.buffer.active.getLine(1).translateToString(true),'retained output');
  assert.equal(state.tinyGeometryCount,1);
  await writeTerminalPage(new Uint8Array(),end,end,[tiny],term,input.write,state);
  assert.equal(state.tinyGeometryCount,1,'the same page boundary is counted once');
 }finally{input.dispose();term.dispose()}
});

test('raw replay and live or process-reset geometry remain exact',async()=>{
 for(const [state,replayEnd,reset] of [[{rawGeometry:true},100,false],[{},0,false],[{},100,true]]){
  const term=new xterm.Terminal({cols:120,rows:30}),input=subscribeTerminalInput(term,()=>{});
  try{
   await writeTerminalPage(Buffer.from('ok'),0,replayEnd,[{offset:0,cols:2,rows:1,reset,sequence:0}],term,input.write,state);
   assert.equal(term.cols,2);assert.equal(term.rows,1);
  }finally{input.dispose();term.dispose()}
 }
});

test('display history boundary removes only scrollback and survives full replay',async()=>{
 const old=Buffer.from(Array.from({length:30},(_,i)=>`OLD_${i}\r\n`).join(''));
 const screen=Buffer.from('\x1b[H\x1b[0JCURRENT_FRAME\r\nCURRENT_INPUT');
 const bytes=Buffer.concat([old,screen]);
 for(const boundary of [undefined,bytes.length]){
  const term=new xterm.Terminal({cols:80,rows:8,scrollback:10000});
  const input=subscribeTerminalInput(term,()=>assert.fail('display repair must not send terminal input'));
  try{
   const state={clearDisplayAt:boundary};
   await writeTerminalPage(bytes,0,bytes.length,[{offset:0,cols:80,rows:8,sequence:0}],term,input.write,state);
   const b=term.buffer.active;
   assert.equal(b.getLine(b.baseY).translateToString(true),'CURRENT_FRAME');
   assert.equal(b.getLine(b.baseY+1).translateToString(true),'CURRENT_INPUT');
   assert.equal(term.options.scrollback,10000,'new output still has normal history capacity');
   if(boundary!==undefined)assert.equal(b.baseY,0,'stale display history is removed on replay');
   else assert.ok(b.baseY>0,'removing the view boundary restores the original scrollback');
  }finally{input.dispose();term.dispose()}
 }
});

test('display repair at a page boundary preserves partial UTF-8 and ANSI parser state',async()=>{
 const prefix=Buffer.from('history\r\n'.repeat(15)),text=Buffer.from('中');
 const first=Buffer.concat([prefix,text.subarray(0,1)]),second=Buffer.concat([text.subarray(1),Buffer.from('\x1b[31')]),third=Buffer.from('mRED');
 const term=new xterm.Terminal({cols:80,rows:8,scrollback:10000});const input=subscribeTerminalInput(term,()=>assert.fail('no input'));
 try{
  const state={clearDisplayAt:first.length};
  await writeTerminalPage(first,0,first.length+second.length+third.length,[],term,input.write,state);
  await writeTerminalPage(second,first.length,first.length+second.length+third.length,[],term,input.write,state);
  state.clearDisplayAt=first.length+second.length;state.displayCleared=false;
  await writeTerminalPage(new Uint8Array(),state.clearDisplayAt,999,[],term,input.write,state);
  await writeTerminalPage(third,state.clearDisplayAt,999,[],term,input.write,state);
  const b=term.buffer.active,line=b.getLine(b.baseY+b.cursorY);
  assert.equal(line.translateToString(true),'中RED');
  assert.equal(line.getCell(2).getFgColor(),1);
  assert.equal(b.baseY,0);
 }finally{input.dispose();term.dispose()}
});
