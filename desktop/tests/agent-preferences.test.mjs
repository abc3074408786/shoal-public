import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const source=readFileSync(new URL('../src/agent-preferences.ts',import.meta.url),'utf8');
const js=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {agentCatalog,parseAgentPreferences,defaultAgentLaunch,enabledAgents,agentPermission,permissionModesFor,permissionOverride,resolvedAgentPermission,withPermissionDefault}=await import('data:text/javascript;base64,'+Buffer.from(js).toString('base64'));

test('global defaults apply to supported agents while explicit and unsupported selections stay scoped',()=>{
 const old=parseAgentPreferences({defaultAgent:'codex',disabled:[],permissions:{'claude-code':'accept-edits'}});
 const next=withPermissionDefault(old,'ask');
 assert.equal(agentPermission(next,'codex'),'ask');assert.equal(resolvedAgentPermission(next,'codex').source,'global');
 assert.equal(agentPermission(next,'omp'),'always-ask-cached');
 assert.equal(agentPermission(next,'claude-code'),'accept-edits');
 assert.equal(agentPermission(next,'kimi'),'inherit');assert.equal(permissionOverride(next,'kimi'),'inherit');
 const own=parseAgentPreferences({...next,permissions:{...next.permissions,codex:'inherit'}});
 assert.equal(agentPermission(own,'codex'),'inherit');
 const follow=structuredClone(own);delete follow.permissions.codex;
 assert.equal(agentPermission(parseAgentPreferences(follow),'codex'),'ask');
 assert.equal(agentPermission(withPermissionDefault(old,'full-access'),'omp'),'yolo');
 assert.equal(agentPermission(withPermissionDefault(old,'full-access'),'zcode'),'yolo');
 assert.equal(agentPermission(withPermissionDefault(old,'full-access'),'opencode'),'inherit');
 assert.equal(agentPermission(withPermissionDefault(next,'inherit'),'claude-code'),'accept-edits');
});

test('malformed global defaults do not escalate and unsupported persisted defaults require a decision',()=>{
 for(const mode of [null,{},'YOLO','--auto','ask;bad'])assert.equal(agentPermission(parseAgentPreferences({permissionDefault:mode}),'codex'),'inherit');
 const prefs=parseAgentPreferences({permissionDefault:'auto'});
 assert.throws(()=>agentPermission(prefs,'omp'),/不支持当前全局权限/);
 assert.equal(agentPermission(withPermissionDefault(prefs,'ask'),'omp'),'always-ask-cached');
});

test('old preferences inherit CLI permissions and per-agent invalid modes never escalate',()=>{
 const old={defaultAgent:'codex',disabled:[]};assert.deepEqual(parseAgentPreferences(old),old);
 for(const agent of agentCatalog){
  assert.equal(agentPermission(parseAgentPreferences(old),agent.id),'inherit');
  for(const option of permissionModesFor(agent.id)){
   const prefs=parseAgentPreferences(JSON.parse(JSON.stringify({...old,permissions:{[agent.id]:option.id}})));
   assert.equal(agentPermission(prefs,agent.id),option.id);
  }
  for(const mode of [null,{},'unknown','--yolo','YOLO','plan','yolo;bad']){
   assert.equal(agentPermission(parseAgentPreferences({...old,permissions:{[agent.id]:mode}}),agent.id),'inherit');
  }
 }
 assert.equal(agentPermission(parseAgentPreferences({...old,permissions:{omp:'auto',codex:'yolo',zcode:'plan'}}),'omp'),'inherit');
 assert.deepEqual(parseAgentPreferences({...old,permissions:{unknown:'full-access'}}),old);
});
test('missing and invalid settings retain a usable default without accepting arbitrary programs',()=>{
 for(const value of [null,undefined,{},'codex',{defaultAgent:'run-custom-command',disabled:['unknown',null,'codex','codex']}]){
  const prefs=JSON.parse(JSON.stringify(parseAgentPreferences(value)));
  assert.ok(['auto','terminal',...agentCatalog.map(a=>a.id)].includes(prefs.defaultAgent));
  assert.ok(prefs.disabled.every(id=>agentCatalog.some(a=>a.id===id)));
  assert.equal(new Set(prefs.disabled).size,prefs.disabled.length);
 }
});
test('disabling a chosen default resets to automatic and leaves only enabled launch choices',()=>{
 const prefs=parseAgentPreferences({defaultAgent:'claude-code',disabled:['claude-code','codex']});
 assert.equal(prefs.defaultAgent,'auto');assert.equal(defaultAgentLaunch(prefs),'gemini');
 assert.ok(!enabledAgents(prefs).some(a=>a.id==='codex'||a.id==='claude-code'));
});
test('an explicit default is first after persistence round trip while other enabled agents remain available',()=>{
 const prefs=parseAgentPreferences(JSON.parse(JSON.stringify({defaultAgent:'opencode',disabled:['gemini']})));
 assert.equal(defaultAgentLaunch(prefs),'opencode');assert.equal(enabledAgents(prefs)[0].id,'opencode');
 assert.equal(enabledAgents(prefs).length,agentCatalog.length-1);
 assert.ok(enabledAgents(prefs).some(a=>a.id==='codex'));
});
test('blank terminal and all-agents-disabled choices keep a shell fallback',()=>{
 assert.equal(defaultAgentLaunch({defaultAgent:'terminal',disabled:[]}), 'terminal');
 const prefs=parseAgentPreferences({defaultAgent:'codex',disabled:agentCatalog.map(a=>a.id)});
 assert.equal(defaultAgentLaunch(prefs),'terminal');assert.deepEqual(enabledAgents(prefs),[]);
});

test('ZCode uses the free entry and supports default, persistence and hiding',()=>{
 assert.equal(agentCatalog.find(agent=>agent.id==='zcode')?.program,'zcode-free');
 const prefs=parseAgentPreferences(JSON.parse(JSON.stringify({defaultAgent:'zcode',disabled:[]})));
 assert.equal(defaultAgentLaunch(prefs),'zcode');
 assert.equal(enabledAgents(prefs)[0].id,'zcode');
 const hidden=parseAgentPreferences({...prefs,disabled:['zcode']});
 assert.equal(hidden.defaultAgent,'auto');
 assert.ok(!enabledAgents(hidden).some(agent=>agent.id==='zcode'));
});
test('MiniMax Code is a native selectable Agent and inherits permissions',()=>{
 assert.equal(agentCatalog.find(a=>a.id==='minimax').program,'mcode');
 const prefs=parseAgentPreferences({defaultAgent:'minimax',disabled:[]});
 assert.equal(defaultAgentLaunch(prefs),'minimax');assert.equal(agentPermission(prefs,'minimax'),'inherit');
 assert.equal(agentPermission(withPermissionDefault(prefs,'full-access'),'minimax'),'inherit');
 assert.equal(parseAgentPreferences({...prefs,disabled:['minimax']}).defaultAgent,'auto');
});
