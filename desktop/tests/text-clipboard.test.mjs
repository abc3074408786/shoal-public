import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const url=code=>'data:text/javascript;base64,'+Buffer.from(code).toString('base64');
const platform=url(`export const calls=[]; export const isTauri=()=>globalThis.clipNative===true;export const invoke=async(command,args)=>{calls.push({command,args});return globalThis.clipInvoke?.(command,args)};`);
const source=ts.transpileModule(readFileSync(new URL('../src/text-clipboard.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {copyText}=await import(url(source.replaceAll("from './desktop-platform'",`from '${platform}'`)));
const {calls}=await import(platform);
async function environment(native,clipboard,run){
 const saved=Object.getOwnPropertyDescriptor(globalThis,'navigator');
 Object.defineProperty(globalThis,'navigator',{configurable:true,value:{clipboard}});globalThis.clipNative=native;calls.length=0;
 try{await run()}finally{delete globalThis.clipNative;delete globalThis.clipInvoke;if(saved)Object.defineProperty(globalThis,'navigator',saved);else delete globalThis.navigator}
}
test('desktop copy bypasses a denied browser clipboard and preserves exact Unicode text',()=>environment(true,{writeText:()=>{throw new DOMException('Write permission denied','NotAllowedError')}},async()=>{
 const text='D:\\项目\\知识蒸馏\r\n第二行';await copyText(text);assert.deepEqual(calls,[{command:'terminal_copy',args:{text}}]);
}));
test('desktop copy waits for native completion before reporting success',()=>environment(true,{},async()=>{
 let finish,completed=false;globalThis.clipInvoke=()=>new Promise(resolve=>finish=resolve);
 const copying=copyText('工作区名称').then(()=>completed=true);await Promise.resolve();assert.equal(completed,false);finish();await copying;assert.equal(completed,true);
}));
test('native failure stays on native transport without browser fallback or false success',()=>environment(true,{writeText:()=>assert.fail('native failure must not fall back to browser')},async()=>{
 const cause=new Error('busy');globalThis.clipInvoke=()=>Promise.reject(cause);
 await assert.rejects(copyText('x'),error=>error.message.includes('系统剪贴板暂时不可用')&&error.cause===cause);assert.equal(calls.length,1);
}));
test('browser preview retains browser clipboard support',()=>environment(false,{writeText:async text=>{assert.equal(text,'中文\nbranch/main')}},async()=>{
 await copyText('中文\nbranch/main');assert.deepEqual(calls,[]);
}));
test('browser permission denial becomes a clear actionable message',()=>environment(false,{writeText:async()=>{throw new DOMException('Write permission denied','NotAllowedError')}},async()=>{
 await assert.rejects(copyText('path'),error=>error.message.includes('桌面 Velella')&&!error.message.includes('NotAllowedError')&&error.cause.name==='NotAllowedError');assert.deepEqual(calls,[]);
}));
test('missing browser Clipboard API has the same recoverable message',()=>environment(false,undefined,async()=>{
 await assert.rejects(copyText('path'),/手动选择文字复制/);assert.deepEqual(calls,[]);
}));
