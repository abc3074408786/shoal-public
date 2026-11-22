import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
import xterm from '@xterm/xterm';
const compiled=ts.transpileModule(readFileSync(new URL('../src/terminal-ime-anchor.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const anchors=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
const resolveAnchor=anchors.resolveTerminalImeAnchor??anchors.resolveCursorAgentImeAnchor;
const rule='─'.repeat(149);
async function fixture({input='❯\u00a0\x1b[7m \x1b[27m',header=true,footer=true,bottom=22,park='\x1b[52;149H'}={}){
 const term=new xterm.Terminal({cols:149,rows:52,allowProposedApi:true});
 const output=(header?'\x1b[1;1H╭─── Claude Code v2.1.238 ───╮':'')+
  `\x1b[20;1H${rule}\x1b[21;1H${input}\x1b[${bottom};1H${rule}`+
  (footer?`\x1b[${bottom+1};1H  ⏸ manual mode on · ? for shortcuts`:'')+park;
 await new Promise(resolve=>term.write(output,resolve));
 const b=term.buffer.active;
 return {term,anchor:()=>resolveAnchor({buffer:b,rows:term.rows,cols:term.cols,cursorX:b.cursorX,cursorY:b.cursorY})};
}

test('real Claude inverse caret wins over ConPTY cursor parked at empty bottom-right',async()=>{
 const {term,anchor}=await fixture();try{assert.equal(term.buffer.active.cursorY,51);assert.deepEqual(anchor(),{row:20,column:2})}finally{term.dispose()}
});

test('after a Chinese commit Claude can park the output cursor at a populated footer end',async()=>{
 const {term,anchor}=await fixture({input:'❯\u00a0你\x1b[7m \x1b[27m',park:'\x1b[23;149H'});
 try{assert.ok(term.buffer.active.getLine(22).translateToString(true).includes('manual mode on'));assert.deepEqual(anchor(),{row:20,column:4})}finally{term.dispose()}
});

test('a public cursor over status text is not replaced by the painted caret',async()=>{
 const {term,anchor}=await fixture({park:'\x1b[23;6H'});try{assert.equal(anchor(),null)}finally{term.dispose()}
});
test('Claude midline CJK caret uses display cells rather than text length or input end',async()=>{
 const {term,anchor}=await fixture({input:'❯\u00a0你e\u0301\x1b[7m好\x1b[27m继续'});try{assert.deepEqual(anchor(),{row:20,column:5})}finally{term.dispose()}
});
test('wrapped Claude composer finds the caret on the continuation line after header scrolls away',async()=>{
 const {term,anchor}=await fixture({header:false,input:'❯\u00a0first line\x1b[22;1H第二行\x1b[7m \x1b[27m',bottom:23});try{assert.deepEqual(anchor(),{row:21,column:6})}finally{term.dispose()}
});
test('unidentified prompts and ambiguous inverse regions do not invent an IME caret',async()=>{
 for(const options of [{header:false,footer:false},{input:'❯\u00a0plain text'},{input:'❯\u00a0\x1b[7mab\x1b[27m'}]){
  const {term,anchor}=await fixture(options);try{assert.equal(anchor(),null)}finally{term.dispose()}
 }
});
test('a real cursor on a populated input row keeps its public terminal position',async()=>{
 const {term,anchor}=await fixture({park:'\x1b[21;8H'});try{assert.equal(anchor(),null)}finally{term.dispose()}
});
test('the existing Cursor Agent fallback is retained',async()=>{
 const term=new xterm.Terminal({cols:80,rows:24,allowProposedApi:true});
 try{
  await new Promise(resolve=>term.write('\x1b[1;1HCursor Agent\x1b[11;1H→ Plan, search, build anything\x1b[24;1H',resolve));
  const b=term.buffer.active;
  assert.deepEqual(resolveAnchor({buffer:b,rows:24,cols:80,cursorX:b.cursorX,cursorY:b.cursorY}),{row:10,column:2});
 }finally{term.dispose()}
});
