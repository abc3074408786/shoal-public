import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
import xterm from '@xterm/xterm';
const code=ts.transpileModule(readFileSync(new URL('../src/terminal-keyboard.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {terminalKeyboardOptions,terminalEditingInput}=await import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'));
const windows={platform:'Win32',local:true},unix={platform:'Linux x86_64',local:true},remote={platform:'Win32',local:false};
const key=values=>({key:'Enter',code:'Enter',keyCode:13,shiftKey:false,ctrlKey:false,altKey:false,metaKey:false,...values});
test('local Windows withholds CSI-u advertisement while Unix and SSH retain negotiation',async()=>{
 for(const [environment,expected] of [[windows,[]],[unix,['\x1b[?0u']],[remote,['\x1b[?0u']]]){
  const term=new xterm.Terminal({allowProposedApi:true,...terminalKeyboardOptions(environment)}),sent=[];
  const sub=term.onData(text=>sent.push(text));
  try{await new Promise(resolve=>term.write('\x1b[?u',resolve));assert.deepEqual(sent,expected)}finally{sub.dispose();term.dispose()}
 }
});
test('modified Enter follows negotiated protocols and ConPTY fallbacks',()=>{
 assert.equal(terminalEditingInput(key({shiftKey:true}),0,windows),'\x1b\r');
 assert.equal(terminalEditingInput(key({shiftKey:true}),1,unix),'\x1b[13;2u');
 assert.equal(terminalEditingInput(key({ctrlKey:true}),0,windows),'\r');
 assert.equal(terminalEditingInput(key({ctrlKey:true}),0,unix),'\x1b[13;5u');
 assert.equal(terminalEditingInput(key({ctrlKey:true}),1,windows),'\x1b[13;5u');
 assert.equal(terminalEditingInput(key({key:'Process',keyCode:229,shiftKey:true}),0,windows),'\x1b\r');
});
test('Ctrl+Backspace deletes a word; ordinary typing and native modifier chords stay with xterm',()=>{
 assert.equal(terminalEditingInput(key({key:'Backspace',ctrlKey:true}),0,windows),'\x17');
 for(const event of [key(),key({key:'a',code:'KeyA'}),key({altKey:true}),key({ctrlKey:true,altKey:true}),key({shiftKey:true,ctrlKey:true})])assert.equal(terminalEditingInput(event,0,windows),undefined);
});
