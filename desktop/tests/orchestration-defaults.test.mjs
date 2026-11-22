import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
async function moduleAt(path){const source=readFileSync(new URL(path,import.meta.url),'utf8');const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;return import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));}
const {parseAgentPreferences,withPermissionDefault}=await moduleAt('../src/agent-preferences.ts');
const {defaultOrchestrationSelection,orchestrationSelectionError}=await moduleAt('../src/orchestration-defaults.ts');
const providers=[{id:'codex'},{id:'claude-code'}],templates=[{id:'saved-plan'}];
test('old preferences keep the existing Codex and goal-generated creation defaults',()=>{
 const old={defaultAgent:'codex',disabled:[]};assert.deepEqual(parseAgentPreferences(old),old);
 assert.deepEqual(defaultOrchestrationSelection(parseAgentPreferences(old),providers),{provider:'codex',templateId:''});
});
test('saved orchestration defaults survive persistence and unrelated permission edits',()=>{
 const preferences=parseAgentPreferences(JSON.parse(JSON.stringify({defaultAgent:'terminal',disabled:['codex'],orchestration:{provider:'claude-code',templateId:'saved-plan'}})));
 assert.deepEqual(defaultOrchestrationSelection(preferences,providers),{provider:'claude-code',templateId:'saved-plan'});
 assert.deepEqual(withPermissionDefault(preferences,'ask').orchestration,preferences.orchestration);
 assert.deepEqual(parseAgentPreferences({...preferences,disabled:[]}).orchestration,preferences.orchestration);
});
test('malformed persisted defaults cannot inject arbitrary CLI names or template text',()=>{
 for(const orchestration of [null,[],{},'bad',{provider:'codex;whoami',templateId:'../plan'},{provider:'missing',templateId:'x\nrun'},{templateId:'x'.repeat(201)}]){
  assert.equal(parseAgentPreferences({orchestration}).orchestration,undefined);
 }
});
test('a deleted template requires an explicit decision instead of silently generating a different plan',()=>{
 const selection=defaultOrchestrationSelection({defaultAgent:'codex',disabled:[],orchestration:{provider:'claude-code',templateId:'deleted-plan'}},providers);
 assert.equal(selection.templateId,'deleted-plan');assert.match(orchestrationSelectionError(selection,providers,templates),/模板已不可用/);
 assert.equal(orchestrationSelectionError({...selection,templateId:''},providers,templates),'');
});
test('a removed preferred provider is retained for correction instead of switching CLI accounts',()=>{
 const selection=defaultOrchestrationSelection({defaultAgent:'codex',disabled:[],orchestration:{provider:'omp'}},providers);
 assert.equal(selection.provider,'omp');assert.match(orchestrationSelectionError(selection,providers,templates),/主控.*不可用/);
 assert.equal(orchestrationSelectionError({provider:'claude-code',templateId:'saved-plan'},providers,templates),'');
});
