import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire,syncBuiltinESMExports} from 'node:module';
const child=createRequire(import.meta.url)('node:child_process');

test('OMP and OpenCode observers bind exact native sessions without changing native input results',async()=>{
 const original=child.spawnSync,calls=[];child.spawnSync=(...args)=>{calls.push(args);return {status:0}};syncBuiltinESMExports();
 const previous=process.env.VELELLA_RESTORE_RUNTIME;process.env.VELELLA_RESTORE_RUNTIME='fixture-runtime';
 try{
  const {default:omp}=await import('../../crates/velella/src/terminal_restore/omp.mjs');const handlers={};omp({on:(name,fn)=>{handlers[name]=fn}});
  const context={cwd:process.cwd(),sessionManager:{getSessionId:()=> 'd8fdd138-475c-43c0-9f4d-722ec8a48741'}};
  assert.equal(await handlers.session_start({},context),undefined);assert.equal(await handlers.input({},context),undefined);
  assert.deepEqual(calls.map(c=>c[1]),[['native-restore-hook',context.sessionManager.getSessionId(),'SessionStart'],['native-restore-hook',context.sessionManager.getSessionId(),'UserPromptSubmit']]);calls.length=0;
  const {VelellaRestore}=await import('../../crates/velella/src/terminal_restore/opencode.mjs');const plugin=await VelellaRestore({directory:process.cwd()});
  await plugin.event({event:{type:'session.created',properties:{info:{id:'ses_parent'}}}});
  await plugin.event({event:{type:'session.created',properties:{info:{id:'ses_child',parentID:'ses_parent'}}}});
  assert.equal(await plugin['chat.message']({sessionID:'ses_child'}),undefined);await plugin.event({event:{type:'session.idle',properties:{sessionID:'ses_child'}}});
  assert.equal(calls.length,1,'subagent messages must not overwrite the main conversation');
  assert.equal(await plugin['chat.message']({sessionID:'ses_parent'}),undefined);assert.equal(calls[1][1][1],'ses_parent');
  calls.length=0;const {Velella}=await import('../../crates/velella/src/collaboration/providers/opencode_lifecycle.mjs');const role=await Velella({directory:process.cwd()});
  await assert.rejects(role['chat.message']({sessionID:'ses_unknown'}),/root session is not bound/);
  await role.event({event:{type:'session.created',properties:{info:{id:'ses_main'}}}});await role.event({event:{type:'session.created',properties:{info:{id:'ses_child',parentID:'ses_main'}}}});
  await role['chat.message']({sessionID:'ses_child'});await role['chat.message']({sessionID:'ses_main'});assert.equal(calls.length,2);assert.ok(calls.every(c=>JSON.parse(c[2].input).session_id==='ses_main'&&JSON.parse(c[2].input).cwd===process.cwd()));
 }finally{child.spawnSync=original;syncBuiltinESMExports();if(previous===undefined)delete process.env.VELELLA_RESTORE_RUNTIME;else process.env.VELELLA_RESTORE_RUNTIME=previous;}
});
