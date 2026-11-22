import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const js=ts.transpileModule(readFileSync(new URL('../src/terminal-composition.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {installTerminalComposition}=await import('data:text/javascript;base64,'+Buffer.from(js).toString('base64'));
function fixture(){
 const listeners=new Map(),sent=[],textarea={value:'',isConnected:true,ownerDocument:{activeElement:null}};
 textarea.ownerDocument.activeElement=textarea;
 const term={textarea,element:{addEventListener(name,fn){listeners.set(name,fn)},removeEventListener(name){listeners.delete(name)}},options:{disableStdin:false},input(text){sent.push(text)}};
 const view={start(){},update(){},end(){}};
 const installed=installTerminalComposition(term,view,event=>event.shiftKey?'\x1b\r':event.ctrlKey?'\r':undefined);
 function fire(type,values={}){const event={type,target:textarea,key:'',code:'',keyCode:0,charCode:0,stopImmediatePropagation(){this.stopped=true},preventDefault(){this.prevented=true},...values};listeners.get(type)?.(event);return event}
 return {term,sent,fire,dispose:()=>installed.dispose()};
}
test('Shift+Enter commits the IME text before exactly one newline',()=>{
 const f=fixture();try{
  f.fire('compositionstart');f.fire('compositionupdate',{data:'ni'});
  f.fire('keydown',{key:'Enter',code:'Enter',keyCode:13,shiftKey:true});
  f.fire('compositionend',{data:'你'});f.fire('input',{data:'你',inputType:'insertFromComposition'});
  assert.deepEqual(f.sent,['你','\x1b\r']);
 }finally{f.dispose()}
});
test('Ctrl+Enter retains commit-before-submit ordering',()=>{
 const f=fixture();try{f.fire('compositionstart');f.fire('keydown',{key:'Enter',code:'Enter',keyCode:13,ctrlKey:true});f.fire('compositionend',{data:'你'});assert.deepEqual(f.sent,['你','\r'])}finally{f.dispose()}
});
test('a plain candidate-confirmation Enter does not submit the prompt',()=>{
 const f=fixture();try{f.fire('compositionstart');f.fire('keydown',{key:'Enter',code:'Enter',keyCode:13});f.fire('compositionend',{data:'你'});assert.deepEqual(f.sent,['你'])}finally{f.dispose()}
});
test('Escape and blur cancel a pending modified Enter',()=>{
 for(const cancel of ['keydown','blur']){const f=fixture();try{f.fire('compositionstart');f.fire('keydown',{key:'Enter',code:'Enter',keyCode:13,shiftKey:true});f.fire(cancel,{key:'Escape',code:'Escape'});f.fire('compositionend',{data:'你'});assert.deepEqual(f.sent,[])}finally{f.dispose()}}
});
test('one IME Enter owns trailing redispatched events until key release, then ordinary Enter works',()=>{
 const f=fixture();try{
  f.fire('compositionstart');f.fire('keydown',{key:'Process',code:'Enter',keyCode:229,shiftKey:true});f.fire('compositionend',{data:'你'});
  assert.equal(f.fire('keydown',{key:'Enter',code:'Enter',keyCode:13}).stopped,true);
  assert.equal(f.fire('keypress',{key:'Enter',code:'Enter',charCode:13}).stopped,true);
  f.fire('keyup',{key:'Enter',code:'Enter'});
  assert.equal(f.fire('keydown',{key:'Enter',code:'Enter',keyCode:13}).stopped,undefined);
  assert.deepEqual(f.sent,['你','\x1b\r']);
 }finally{f.dispose()}
});
test('a modifier released before composition commits does not block the next physical Enter',()=>{
 const f=fixture();try{
  f.fire('compositionstart');f.fire('keydown',{key:'Enter',code:'Enter',keyCode:13,shiftKey:true});f.fire('keyup',{key:'Enter',code:'Enter'});f.fire('compositionend',{data:'你'});
  assert.equal(f.fire('keydown',{key:'Enter',code:'Enter',keyCode:13}).stopped,undefined);assert.deepEqual(f.sent,['你','\x1b\r']);
 }finally{f.dispose()}
});
test('an abandoned implicit IME transaction cannot submit a later commit',()=>{
 const f=fixture();try{
  f.fire('keydown',{key:'Process',code:'Enter',keyCode:229,shiftKey:true});
  f.fire('keydown',{key:'Escape',code:'Escape',keyCode:27});
  f.fire('input',{data:'你',inputType:'insertFromComposition'});assert.deepEqual(f.sent,['你']);
 }finally{f.dispose()}
});
test('a new implicit IME key cannot inherit an earlier modified Enter',()=>{
 const f=fixture();try{
  f.fire('keydown',{key:'Process',code:'Enter',keyCode:229,shiftKey:true});
  f.fire('keydown',{key:'Process',code:'Space',keyCode:229});
  f.fire('input',{data:'你',inputType:'insertText'});assert.deepEqual(f.sent,['你']);
 }finally{f.dispose()}
});
test('a new candidate key cancels an earlier modified Enter in explicit composition',()=>{
 for(const next of [{key:' ',code:'Space',keyCode:32},{key:'1',code:'Digit1',keyCode:49},{key:'Process',code:'Space',keyCode:229}]){
  const f=fixture();try{
   f.fire('compositionstart');f.fire('keydown',{key:'Enter',code:'Enter',keyCode:13,shiftKey:true});f.fire('keydown',next);f.fire('compositionend',{data:'你'});assert.deepEqual(f.sent,['你']);
  }finally{f.dispose()}
 }
});
