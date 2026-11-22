import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';
const compile=name=>ts.transpileModule(readFileSync(new URL('../src/'+name,import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const url=code=>'data:text/javascript;base64,'+Buffer.from(code).toString('base64');
const split=url(compile('split-layout.ts'));
const {sessionSurfaceTab}=await import(url(compile('session-surface-tab.ts').replaceAll("'./split-layout'",JSON.stringify(split))));
const session={id:'s',kind:'terminal'},layout=tabs=>({panes:[{id:'main',tabs}]}),chat={id:'chat',kind:'agent',target:'s'},terminal={id:'terminal',kind:'terminal',target:'s'};

test('explicit native switch overrides annotation history and an existing chat tab',()=>{
 const result=sessionSurfaceTab(layout([chat]),session,'Codex',true,true);
 assert.equal(result.kind,'terminal');assert.equal(result.target,'s');assert.notEqual(result.id,'chat');
 assert.equal(sessionSurfaceTab(layout([]),session,'Codex',true,true).kind,'terminal');
 assert.equal(sessionSurfaceTab(layout([chat,terminal]),session,'Codex',true,true),terminal);
});
test('existing split leaf is selected without duplicating its group',()=>{
 const group={id:'group',kind:'terminal',target:'other',terminalTree:{kind:'split',id:'split',first:{kind:'leaf',id:'other'},second:{kind:'leaf',id:'s'}}};
 assert.equal(sessionSurfaceTab(layout([chat,group]),session,'Codex',true,true),group);
});
test('ordinary annotation navigation retains its existing chat behavior',()=>{
 assert.equal(sessionSurfaceTab(layout([chat]),session,'Codex',true),chat);
 assert.equal(sessionSurfaceTab(layout([]),session,'Codex',true).kind,'agent');
});
