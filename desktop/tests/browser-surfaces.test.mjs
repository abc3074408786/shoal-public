import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import ts from 'typescript';

test('native surfaces recover the same layout after a rejected placement and stale cleanup',async()=>{
  const queued=[],calls=[];let fail=true,blocked=false;
  globalThis.ResizeObserver=class{observe(){}unobserve(){}};
  globalThis.MutationObserver=class{observe(){}};
  globalThis.document={body:{},querySelector:()=>blocked?{}:null};
  globalThis.window={addEventListener(){},dispatchEvent(){}};
  globalThis.requestAnimationFrame=fn=>queued.push(fn);
  globalThis.innerWidth=1440;globalThis.innerHeight=900;
  globalThis.browserSurfaceInvoke=async(_,{request})=>{calls.push(request.frames);if(fail&&request.frames.length){fail=false;throw Error('transient placement failure')}};
  let source=readFileSync(new URL('../src/browser-surfaces.ts',import.meta.url),'utf8');
  source=source.replace("import {invoke} from './desktop-platform';","const invoke=globalThis.browserSurfaceInvoke;").replace("import {isTauri} from './api';","const isTauri=()=>true;")
    .replace("import {currentTarget} from './execution-target';","const currentTarget=()=>globalThis.browserSurfaceTarget||{kind:'local'};");
  const compiled=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
  const mod=await import('data:text/javascript;base64,'+Buffer.from(compiled).toString('base64'));
  const flush=async()=>{queued.splice(0).forEach(fn=>fn());for(let i=0;i<3;i++)await new Promise(resolve=>setImmediate(resolve))};
  const element=()=>({isConnected:true,checkVisibility:()=>true,getBoundingClientRect:()=>({x:240,y:90,right:1200,bottom:800})});
  const disposeOld=mod.registerBrowserSurface('browser-one','workspace',element());
  await flush();assert.deepEqual(calls.at(-1),[],'failed placement hides native pages');
  mod.scheduleBrowserSurfaces();await flush();assert.equal(calls.at(-1)[0].id,'browser-one','same geometry can recover');
  const disposeCurrent=mod.registerBrowserSurface('browser-one','workspace',element());disposeOld();
  blocked=true;mod.scheduleBrowserSurfaces();await flush();assert.deepEqual(calls.at(-1),[],'dialog hides native content');
  blocked=false;mod.scheduleBrowserSurfaces();await flush();assert.equal(calls.at(-1)[0].id,'browser-one','old cleanup does not remove a remounted surface');
  disposeCurrent();await flush();assert.deepEqual(calls.at(-1),[]);
  const before=calls.length;
  globalThis.browserSurfaceTarget={kind:'ssh',host_id:'018fa61b-4db2-7000-8000-0000000000aa',server_id:'018fa61b-4db2-7000-8000-0000000000bb'};
  mod.registerBrowserSurface('browser-remote','workspace',element());await flush();
  assert.equal(calls.length,before,'a remote execution target must not move the local browser surface');
  delete globalThis.browserSurfaceTarget;
  mod.scheduleBrowserSurfaces();await flush();
  assert.equal(calls.at(-1)[0]?.id,'browser-remote','local placement resumes after switching back');
});
