import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
import xterm from '@xterm/xterm';
import {createRequire} from 'node:module';
import {pathToFileURL} from 'node:url';
const graphemes=pathToFileURL(createRequire(import.meta.url).resolve('@xterm/addon-unicode-graphemes')).href;
const load=async(name)=>{
  const compiled=ts.transpileModule(readFileSync(new URL(`../src/${name}.ts`,import.meta.url),'utf8').replace("'@xterm/addon-unicode-graphemes'",JSON.stringify(graphemes)),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
  return import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
};
const preferences=await load('terminal-preferences'),{configureTerminalUnicode}=await load('terminal-unicode');
const {subscribeTerminalInput,isProtocolReply}=await load('terminal-input');
const {stripTerminalSelectionGutter}=await load('terminal-selection');
test('official graphemes match the CLI grid for CJK, combining marks, emoji and PUA',async()=>{
 const term=new xterm.Terminal({allowProposedApi:true});configureTerminalUnicode(term);
 try{
  for(const [text,cells] of [['中文',4],['e\u0301',1],['👩\u200d💻',2],['👨‍👩‍👧‍👦',2],['👍🏽',2],['🇨🇳',2],['\ue0b0',1]]){
   term.reset();await new Promise(resolve=>term.write(text,resolve));assert.equal(term.buffer.active.cursorX,cells,text);
  }
 }finally{term.dispose()}
});
test('Kitty negotiation is a bounded protocol reply and never escapes replay',async()=>{
 const term=new xterm.Terminal({allowProposedApi:true,vtExtensions:{kittyKeyboard:true}}),sent=[];
 const input=subscribeTerminalInput(term,(...args)=>sent.push(args));
 try{
  await input.write('\x1b[?u',true);assert.deepEqual(sent,[]);
  await input.write('\x1b[?u',false);assert.deepEqual(sent,[['\x1b[?0u','protocol']]);
  for(const text of ['\x1b[?32u','\x1b[?1;2u','\x1b[?1uwhoami\r'])assert.equal(isProtocolReply(text),false);
 }finally{input.dispose();term.dispose()}
});
test('malformed terminal preferences are bounded and use an actual symbol font fallback',()=>{
 const value=preferences.parseTerminalPreferences({fontSize:999,lineHeight:0,scrollSensitivity:-1,renderer:'invalid',fontFamily:'"evil\nfont"'});
 assert.equal(value.fontSize,40);assert.equal(value.lineHeight,1);assert.equal(value.scrollSensitivity,.1);assert.equal(value.renderer,'auto');
 assert.equal(value.fontFamily,'evilfont');assert.ok(preferences.terminalFontFamily(value.fontFamily).includes('Velella Nerd Font Symbols'));
});
test('copy gutter removal preserves code indentation and Windows line endings',()=>{
 assert.equal(stripTerminalSelectionGutter('  message\r\n    code\r\n\r\n  done'),'message\r\n  code\r\n\r\ndone');
 assert.equal(stripTerminalSelectionGutter('message\n  code'),'message\n  code');
});
