import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const load=async name=>{
  const source=readFileSync(new URL(`../src/${name}.ts`,import.meta.url),'utf8');
  const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
  return import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
};
const prefs=await load('terminal-preferences'),{surfaceThemes}=await load('surface-themes');
test('new and partial configurations use Mist spacing and the existing blinking block',()=>{
  for(const value of [undefined,null,{}, {fontSize:18}, {cursorBlink:'invalid',cursorStyle:'invalid'}]){
    const parsed=prefs.parseTerminalPreferences(value);
    assert.equal(parsed.cursorStyle,'block');
    assert.equal(parsed.cursorBlink,true);
    assert.equal(parsed.lineHeight,1.16);
  }
  assert.equal(prefs.defaultTerminalPreferences.cursorBlink,true);
});
test('an explicit steady cursor and other chosen shapes remain valid',()=>{
  for(const cursorStyle of ['block','bar','underline']){
    const parsed=prefs.parseTerminalPreferences({cursorStyle,cursorBlink:false});
    assert.equal(parsed.cursorBlink,false);
    assert.equal(parsed.cursorStyle,cursorStyle);
    assert.equal(prefs.terminalAppearance(parsed).cursorInactiveStyle,cursorStyle==='block'?'outline':cursorStyle);
  }
});
test('legacy and damaged preferences fall back safely without dropping a legacy font size',()=>{
  const previous=globalThis.localStorage;
  try{
    const data=new Map([['velella:terminal-font-size','18']]);
    globalThis.localStorage={getItem:key=>data.get(key)??null};
    assert.equal(prefs.readTerminalPreferences().fontSize,18);
    assert.equal(prefs.readTerminalPreferences().cursorBlink,true);
    data.set(prefs.TERMINAL_PREFERENCES_KEY,'broken json');
    assert.equal(prefs.readTerminalPreferences().cursorBlink,true);
  }finally{globalThis.localStorage=previous}
});
test('applying the requested cursor settings preserves all other saved preferences',()=>{
  const previousStorage=globalThis.localStorage,previousWindow=globalThis.window;
  try{
    const existing={...prefs.defaultTerminalPreferences,fontFamily:'Consolas',fontSize:18,lineHeight:1.25,renderer:'dom',scrollSensitivity:2,fastScrollSensitivity:7,rightClickPaste:false,trimCopyGutter:false,cursorStyle:'bar',cursorBlink:false};
    const data=new Map([[prefs.TERMINAL_PREFERENCES_KEY,JSON.stringify(existing)]]),events=[];
    globalThis.localStorage={getItem:key=>data.get(key)??null,setItem:(key,value)=>data.set(key,value)};
    globalThis.window={dispatchEvent:event=>events.push(event.type)};
    prefs.saveTerminalPreferences({...prefs.readTerminalPreferences(),cursorStyle:'block',cursorBlink:true});
    assert.deepEqual(JSON.parse(data.get(prefs.TERMINAL_PREFERENCES_KEY)),{...existing,cursorStyle:'block',cursorBlink:true});
    assert.deepEqual(events,[prefs.TERMINAL_PREFERENCES_EVENT]);
  }finally{globalThis.localStorage=previousStorage;globalThis.window=previousWindow}
});
test('Mist terminal has its own gray-green surface and mint cursor',()=>{
  assert.equal(surfaceThemes.dark.cursor,'#a7d7bd');
  assert.equal(surfaceThemes.dark.background,'#232d2a');
  assert.equal(surfaceThemes.dark.foreground,'#dfe8e3');
  assert.equal(surfaceThemes.dark.selectionBackground,'#466656');
  assert.equal(surfaceThemes.light.cursor,'#171717');
  for(const theme of Object.values(surfaceThemes))assert.equal(theme.cursorAccent,theme.background);
});
