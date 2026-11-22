import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';

const compiled=ts.transpileModule(readFileSync(new URL('../src/image-attachment.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {imagePaste,pasteClipboard}=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));

test('image is pasted once with bracketed protocol and no Enter or instruction',async()=>{
 const sent=[];
 await pasteClipboard(async()=>({kind:'image',value:'C:/temp/中文 image.png'}),()=>true,()=>assert.fail(),async value=>sent.push(value));
 assert.deepEqual(sent,['\x1b[200~C:/temp/中文 image.png\x1b[201~']);
});

test('tab switch cancels an outstanding clipboard read',async()=>{
 let current=true;
 await pasteClipboard(async()=>{current=false;return {kind:'image',value:'C:/temp/a.png'}},()=>current,()=>assert.fail(),async()=>assert.fail());
});
test('text retains ordinary paste behavior and unsafe image paths are rejected',async()=>{
 const sent=[];
 for(const value of ['a\nb',''])await pasteClipboard(async()=>({kind:'text',value}),()=>true,text=>sent.push(text),async()=>assert.fail());
 assert.deepEqual(sent,['a\nb']);
 assert.throws(()=>imagePaste('C:/bad\npath.png'));
});
