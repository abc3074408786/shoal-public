import {test} from 'node:test';import assert from 'node:assert/strict';import {readFileSync} from 'node:fs';import ts from 'typescript';import xterm from '@xterm/xterm';
async function load(name){const compiled=ts.transpileModule(readFileSync(new URL(`../src/${name}.ts`,import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;return import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'))}
const {captureTerminalClears,captureTerminalFrame,createTerminalFrameStore}=await load('terminal-frames'),{writeTerminalPage}=await load('terminal-replay'),{subscribeTerminalInput}=await load('terminal-input');

test('clear and alternate-screen exit preserve conversation within one output page and across ANSI fragments',async()=>{
 for(const ending of ['\x1b[?1049l','\x1b[?1047l','\x1b[?47l','\x1b[2J','\x1bc']){
  const term=new xterm.Terminal({cols:80,rows:6,allowProposedApi:true}),store=createTerminalFrameStore();
  const observer=captureTerminalClears(term,()=>store.capture(term,0));
  try{
   const transcript='\x1b[?1049h\x1b[2J\x1b[HQUESTION\r\nANSWER'+ending+'\x1b[2J\x1b[H';
   // Fragments include a split CSI terminator; xterm owns parsing throughout.
   for(const chunk of [transcript.slice(0,-5),transcript.slice(-5)])await new Promise(r=>term.write(chunk,r));
   const value=store.snapshot();assert.ok(value.frames.some(frame=>frame.ansi.includes('QUESTION')&&frame.ansi.includes('ANSWER')),JSON.stringify(ending));
   assert.equal(term.buffer.active.getLine(0).translateToString(true),'','normal clear handling still executes');
  }finally{observer.dispose();term.dispose()}
 }
});
test('geometry replay retains immutable full-width screens before later reflow',async()=>{
 const term=new xterm.Terminal({cols:120,rows:12,allowProposedApi:true}),sent=[],input=subscribeTerminalInput(term,(...args)=>sent.push(args)),store=createTerminalFrameStore();
 try{
  const header='╭'+'─'.repeat(118)+'╮',footer='╰'+'─'.repeat(118)+'╯';
  const first=Buffer.from('\x1b[2J\x1b[H\x1b[38;2;210;120;80m'+header+'\x1b[0m\r\n中文 HISTORY\r\n'+footer),last=Buffer.from('\x1b[2J\x1b[HLIVE');
  const bytes=Buffer.concat([first,last]);
  await writeTerminalPage(bytes,0,bytes.length,[{offset:0,cols:120,rows:12,reset:true,sequence:0},{offset:first.length,cols:60,rows:8,sequence:1}],term,input.write,{beforeResize:offset=>store.capture(term,offset)});
  const saved=store.snapshot().frames[0],originalAnsi=saved.ansi;assert.equal(saved.cols,120);assert.equal(saved.contentRows,3);
  term.resize(40,6);assert.equal(saved.ansi,originalAnsi,'later resizes cannot mutate stored original cells');assert.deepEqual(sent,[]);
  const preview=new xterm.Terminal({cols:saved.cols,rows:saved.rows,allowProposedApi:true});try{await new Promise(r=>preview.write(saved.ansi,r));assert.equal(preview.buffer.active.getLine(0).translateToString(true),header);assert.equal(preview.buffer.active.getLine(1).translateToString(true).trimEnd(),'中文 HISTORY');assert.equal(preview.buffer.active.getLine(2).translateToString(true),footer);assert.equal(preview.buffer.active.getLine(0).getCell(0).getFgColor(),0xd27850)}finally{preview.dispose()}
 }finally{input.dispose();term.dispose()}
});
test('screen capture uses visible screen rather than reflowed scrollback and preserves active terminal',async()=>{
 const term=new xterm.Terminal({cols:80,rows:6,scrollback:100,allowProposedApi:true});
 try{await new Promise(r=>term.write('old\r\n'.repeat(25)+'\x1b[2J\x1b[HCURRENT',r));term.scrollToTop();const before={base:term.buffer.active.baseY,view:term.buffer.active.viewportY,x:term.buffer.active.cursorX,y:term.buffer.active.cursorY};const frame=captureTerminalFrame(term,99);assert.equal(frame.contentRows,1);assert.match(frame.ansi,/CURRENT/);assert.doesNotMatch(frame.ansi,/old/);assert.deepEqual({base:term.buffer.active.baseY,view:term.buffer.active.viewportY,x:term.buffer.active.cursorX,y:term.buffer.active.cursorY},before)}finally{term.dispose()}
});
test('frame store bounds retained display copies and counts omitted frames without touching output',async()=>{
 const term=new xterm.Terminal({cols:40,rows:4,allowProposedApi:true}),store=createTerminalFrameStore(50);
 try{for(let i=0;i<5;i++){await new Promise(r=>term.write('\x1b[2J\x1b[HFRAME_'+i+'x'.repeat(30),r));store.capture(term,i);store.capture(term,i)}const value=store.snapshot();assert.equal(value.frames.length,1);assert.equal(value.dropped,4);assert.match(value.frames[0].ansi,/FRAME_4/)}finally{term.dispose()}
});

test('compact frames preserve styled wide cells and distinguish style-only changes',async()=>{
 const term=new xterm.Terminal({cols:40,rows:6,allowProposedApi:true}),store=createTerminalFrameStore();
 const preview=new xterm.Terminal({cols:40,rows:6,allowProposedApi:true});
 try{
  const prefix='\x1b[2J\x1b[H\x1b[1;2;3;4;7;38;2;50;100;150;48;5;42m';
  await new Promise(r=>term.write(prefix+'中e\u0301文\x1b[0m tail\r\n\r\nEND',r));store.capture(term,1);store.capture(term,2);
  const frame=store.snapshot().frames[0];assert.equal(frame.contentRows,3);
  await new Promise(r=>preview.write(frame.ansi,r));
  for(let row=0;row<3;row++)for(let col=0;col<40;col++){
   const expected=term.buffer.active.getLine(row).getCell(col),actual=preview.buffer.active.getLine(row).getCell(col);
   assert.equal(actual.getChars()||' ',expected.getChars()||' ',`${row}:${col} visible characters`);
   for(const method of ['getWidth','getFgColor','getBgColor','isBold','isDim','isItalic','isUnderline','isInverse'])assert.equal(actual[method](),expected[method](),`${row}:${col} ${method}`);
  }
  assert.equal(store.count,1);await new Promise(r=>term.write('\x1b[H\x1b[31m中',r));store.capture(term,3);assert.equal(store.count,2,'same text with changed style is a distinct frame');
 }finally{preview.dispose();term.dispose()}
});
