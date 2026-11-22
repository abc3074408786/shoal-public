import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
import xterm from '@xterm/xterm';
const js=ts.transpileModule(readFileSync(new URL('../src/terminal-preferences.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {terminalAppearance,defaultTerminalPreferences}=await import('data:text/javascript;base64,'+Buffer.from(js).toString('base64'));
const write=(term,text)=>new Promise(resolve=>term.write(text,resolve));
const lines=term=>Array.from({length:term.buffer.active.length},(_,i)=>term.buffer.active.getLine(i).translateToString(true));
function frame(cols){
 const box=Array.from({length:9},(_,i)=>i===2?'| Welcome back! '+'.'.repeat(cols-17)+'|':'|'+'.'.repeat(cols-2)+'|').join('\r\n');
 return '\x1b[H'+box+'\x1b[10;1H';
}
function clear(rows){return '\x1b[H'+('\x1b[2K\x1b[1B').repeat(rows-1)+'\x1b[2K\x1b[H'}

test('ConPTY viewport repaint does not retain a second reflowed welcome panel after narrowing',async()=>{
 const term=new xterm.Terminal({...terminalAppearance(defaultTerminalPreferences,{platform:'Win32',local:true}),cols:120,rows:10,scrollback:1000});
 try{
  await write(term,'SHELL_HISTORY\r\n'.repeat(20));
  await write(term,clear(10)+frame(120));
  term.resize(60,10);
  await write(term,clear(10)+frame(60));
  const text=lines(term);
  assert.equal(text.filter(s=>s.includes('Welcome back!')).length,1,'the old wide panel must not be wrapped into scrollback beside the new repaint');
  assert.ok(text.some(s=>s.includes('SHELL_HISTORY')),'normal pre-existing shell history remains');
 }finally{term.dispose()}
});

test('Linux and SSH viewers keep ordinary shell reflow',()=>{
 for(const environment of [{platform:'Linux x86_64',local:true},{platform:'MacIntel',local:true},{platform:'Win32',local:false}]){
  assert.equal(terminalAppearance(defaultTerminalPreferences,environment).windowsPty,undefined);
 }
});
