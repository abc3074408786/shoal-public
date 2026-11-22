import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdtemp,readFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve,dirname,basename} from 'node:path';
const {copyTerminalText,readTerminalClipboard}=createRequire(import.meta.url)('../electron/terminal-clipboard.cjs');
const item=(type,value)=>({types:[type],getType:async()=>new Blob([value],{type})});
test('Electron 44 text paste reads one asynchronous snapshot and preserves Chinese/newlines',async()=>{
 let reads=0;
 const value=await readTerminalClipboard({read:async()=>{reads++;return [item('text/plain','你好\r\n第二行')]}},null,'unused');
 assert.deepEqual(value,{kind:'text',value:'你好\r\n第二行'});assert.equal(reads,1);
 assert.deepEqual(await readTerminalClipboard({read:async()=>[]},null,'unused'),{kind:'text',value:''});
});
test('copy waits for write completion and propagates native rejection',async()=>{
 let finish;let completed=false;
 const copying=copyTerminalText({writeText:()=>new Promise(resolve=>finish=resolve)},'中文').then(()=>completed=true);
 await Promise.resolve();assert.equal(completed,false);finish();await copying;assert.equal(completed,true);
 await assert.rejects(copyTerminalText({writeText:async()=>{throw Error('busy')}},'x'),/busy/);
 await assert.rejects(copyTerminalText({},'x'.repeat(1024*1024+1)),/too large/);
});
test('image paste persists PNG bytes before returning its unique path',async()=>{
 const root=await mkdtemp(join(tmpdir(),'velella-clipboard-api-'));
 try{
  const png=Buffer.from('fixture PNG');
  const image={createFromBuffer:bytes=>({isEmpty:()=>bytes.length===0,toPNG:()=>png})};
  const clipboard={read:async()=>[item('text/plain','ignored'),item('image/png',png)]};
  const first=await readTerminalClipboard(clipboard,image,root),second=await readTerminalClipboard(clipboard,image,root);
  assert.equal(first.kind,'image');assert.notEqual(first.value,second.value);assert.deepEqual(await readFile(first.value),png);
  await assert.rejects(readTerminalClipboard({read:async()=>[{types:['image/png'],getType:async()=>({size:33*1024*1024})}]},image,root),/32 MB/);
 }finally{const target=resolve(root);assert.equal(dirname(target),resolve(tmpdir()));assert.ok(basename(target).startsWith('velella-clipboard-api-'));await rm(target,{recursive:true,force:true})}
});
test('older Electron image/text adapter remains supported',async()=>{
 const result=await readTerminalClipboard({readImage:()=>({isEmpty:()=>true}),readText:()=>Promise.resolve('text')},null,'unused');
 assert.deepEqual(result,{kind:'text',value:'text'});
});
