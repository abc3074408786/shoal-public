import assert from 'node:assert/strict';
import {readFileSync,readdirSync,writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {createHash} from 'node:crypto';

const [control,candidate,out]=process.argv.slice(2).map(value=>resolve(value));
assert.ok(control&&candidate&&out,'control candidate receipt paths required');
function inspect(root){
 const manifest=JSON.parse(readFileSync(resolve(root,'dist/.vite/manifest.json'),'utf8'));
 const main=Object.keys(manifest).find(key=>manifest[key].isEntry&&manifest[key].name==='main');assert.ok(main);
 const closure=new Set();function visit(key){if(closure.has(key))return;closure.add(key);for(const dep of manifest[key].imports||[])visit(dep)}visit(main);
 const source=[...closure].map(key=>readFileSync(resolve(root,'dist',manifest[key].file),'utf8')).join('\n');
 const editor=Object.keys(manifest).find(key=>manifest[key].name==='MonacoEditors'&&manifest[key].isDynamicEntry===true);
 const workers={};
 for(const prefix of ['editor.worker-','json.worker-','css.worker-','html.worker-','ts.worker-']){
  const files=readdirSync(resolve(root,'dist/assets')).filter(name=>name.startsWith(prefix)&&name.endsWith('.js'));assert.equal(files.length,1,prefix);
  workers[prefix]=createHash('sha256').update(readFileSync(resolve(root,'dist/assets',files[0]))).digest('hex');
 }
 return {main,staticChunks:[...closure],startupJsBytes:Buffer.byteLength(source),startupHasMonaco:source.includes('MonacoEnvironment'),editorDynamicEntry:editor?manifest[editor]:null,editorInStaticClosure:editor?closure.has(editor):false,workers};
}
const before=inspect(control),after=inspect(candidate);
assert.equal(before.startupHasMonaco,true);assert.equal(after.startupHasMonaco,false);
assert.ok(after.editorDynamicEntry?.isDynamicEntry);assert.equal(after.editorInStaticClosure,false);
assert.deepEqual(after.workers,before.workers,'all original worker bytes must remain unchanged');
const result={ok:true,control:before,candidate:after,deferredStartupJsBytes:before.startupJsBytes-after.startupJsBytes,totalEngineRemoved:false};
writeFileSync(out,JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));
