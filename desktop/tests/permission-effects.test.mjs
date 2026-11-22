import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const compile=path=>ts.transpileModule(readFileSync(new URL(path,import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const url=source=>'data:text/javascript;base64,'+Buffer.from(source).toString('base64');
const prefs=url(compile('../src/agent-preferences.ts'));
const {permissionEffects,permissionRecipe}=await import(url(compile('../src/permission-effects.ts').replace("'./agent-preferences'",JSON.stringify(prefs))));
test('OMP write is explained as automatic read/write with explicit command confirmation',()=>{
 const effects=Object.fromEntries(permissionEffects('omp','write').map(row=>[row.label,row.value]));
 assert.equal(effects['文件写入'],'自动批准');assert.equal(effects['命令执行'],'需要确认');
 assert.equal(permissionRecipe('omp','write'),'omp --approval-mode write');
});
test('permission explanations keep native configuration and free wrapper boundaries visible',()=>{
 assert.match(permissionRecipe('opencode','ask'),/OPENCODE_PERMISSION=\{"\*":"ask"\}/);
 assert.match(permissionEffects('opencode','ask').map(row=>row.value).join(' '),/专属 Agent 规则可覆盖/);
 assert.equal(permissionRecipe('zcode','guarded'),'zcode-free --mode guarded');
 assert.match(permissionEffects('zcode','guarded').map(row=>row.value).join(' '),/不额外承诺读写隔离/);
 assert.match(permissionEffects('codex','auto').find(row=>row.label==='网络与目录').value,/保留 CLI/);
});
