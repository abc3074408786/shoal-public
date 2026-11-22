import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const source=path=>readFileSync(new URL(path,import.meta.url),'utf8');
const js=code=>ts.transpileModule(code,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const dataUrl=code=>'data:text/javascript;base64,'+Buffer.from(code).toString('base64');
const hostId='018fa61b-4db2-7000-8000-0000000000aa',serverId='018fa61b-4db2-7000-8000-0000000000bb';
const remote={kind:'ssh',host_id:hostId,server_id:serverId};
const targetUrl=dataUrl(js(source('../src/execution-target.ts')));
const {selectTarget}=await import(targetUrl);
const platformUrl=dataUrl('export const calls=[];\nexport async function invoke(command,args){calls.push({command,args});return globalThis.clipPayload;}');
const apiUrl=dataUrl('export const isTauri=()=>globalThis.clipBridge===true;');
const attachmentUrl=dataUrl(js(source('../src/image-attachment.ts')));
const selectionUrl=dataUrl(js(source('../src/terminal-selection.ts')));
const keyboardUrl=dataUrl(js(source('../src/terminal-keyboard.ts')));
const clipboardUrl=dataUrl(js(source('../src/terminal-clipboard.ts'))
  .replaceAll("from './desktop-platform'",`from '${platformUrl}'`)
  .replaceAll("from './api'",`from '${apiUrl}'`)
  .replaceAll("from './image-attachment'",`from '${attachmentUrl}'`)
  .replaceAll("from './terminal-selection'",`from '${selectionUrl}'`)
  .replaceAll("from './execution-target'",`from '${targetUrl}'`)
  .replaceAll("from './terminal-keyboard'",`from '${keyboardUrl}'`));
const {installTerminalClipboard}=await import(clipboardUrl);
const platform=await import(platformUrl);
globalThis.document={addEventListener(){},removeEventListener(){},activeElement:null,visibilityState:'visible'};
function viewer(){
  const pasted=[],sent=[],errors=[];
  const term={element:{addEventListener(){},removeEventListener(){}},textarea:null,options:{ignoreBracketedPasteMode:false},
    parser:{registerCsiHandler:()=>({dispose(){}})},getSelection:()=>'',hasSelection:()=>false,clearSelection(){},focus(){},input(){}};
  const api=installTerminalClipboard(term,{current:()=>true,preferences:()=>({trimCopyGutter:false,rightClickPaste:true}),
    schedule:action=>Promise.resolve().then(action),pasteText:value=>Promise.resolve(pasted.push(value)),send:value=>Promise.resolve(sent.push(value)),report:error=>errors.push(error)});
  return {api,pasted,sent,errors};
}
async function withNavigator(readText,run){
  const saved=Object.getOwnPropertyDescriptor(globalThis,'navigator');
  Object.defineProperty(globalThis,'navigator',{configurable:true,value:{clipboard:{readText}}});
  try{return await run();}finally{if(saved)Object.defineProperty(globalThis,'navigator',saved);else delete globalThis.navigator}
}

test('a local terminal reads the native clipboard and pastes an image as its local path',async()=>{
  platform.calls.length=0;globalThis.clipBridge=true;globalThis.clipPayload={kind:'image',value:'C:/Users/developer/AppData/Local/Temp/图片.png'};
  selectTarget({kind:'local'});
  try{
    const {api,pasted,sent,errors}=viewer();
    await api.paste();
    assert.deepEqual(platform.calls,[{command:'terminal_clipboard',args:{target:{kind:'local'}}}],'the image bridge must name the host that wrote the file');
    assert.deepEqual(pasted,[]);assert.deepEqual(sent,['\x1b[200~C:/Users/developer/AppData/Local/Temp/图片.png\x1b[201~']);assert.deepEqual(errors,[]);
  }finally{delete globalThis.clipBridge;delete globalThis.clipPayload}
});

test('a remote terminal keeps text paste and never asks the local clipboard for an image path',async()=>{
  platform.calls.length=0;globalThis.clipBridge=true;globalThis.clipPayload={kind:'image',value:'C:/Users/developer/AppData/Local/Temp/图片.png'};
  selectTarget(remote);
  try{
    const {api,pasted,sent,errors}=viewer();
    await withNavigator(async()=>'远端可用的文本\r\n第二行',()=>api.paste());
    assert.deepEqual(platform.calls,[],'a local image path pasted into a remote session points at a file that host cannot open');
    assert.deepEqual(errors,[]);assert.deepEqual(sent,[]);assert.deepEqual(pasted,['远端可用的文本\r\n第二行']);
  }finally{delete globalThis.clipBridge;delete globalThis.clipPayload;selectTarget({kind:'local'})}
});

test('switching hosts between gestures re-reads the host for every paste',async()=>{
  platform.calls.length=0;globalThis.clipBridge=true;globalThis.clipPayload={kind:'text',value:'本机文本'};
  selectTarget({kind:'local'});
  try{
    const {api,pasted,sent}=viewer();
    await api.paste();
    assert.deepEqual(pasted,['本机文本']);assert.deepEqual(sent,[]);
    selectTarget(remote);
    await withNavigator(async()=>'远端文本',()=>api.paste());
    assert.equal(platform.calls.length,1,'the second paste must not reach the local clipboard');
    assert.deepEqual(pasted,['本机文本','远端文本']);
  }finally{delete globalThis.clipBridge;delete globalThis.clipPayload;selectTarget({kind:'local'})}
});
