import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFileSync,mkdtempSync,mkdirSync,writeFileSync,unlinkSync,rmdirSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import vm from 'node:vm';
const require=createRequire(import.meta.url),conpty=require('../scripts/conpty-runtime.cjs');
const configSource=readFileSync(new URL('../electron-builder.config.cjs',import.meta.url),'utf8');

// electron-builder evaluates the config as a CommonJS module, so the sandbox carries the same
// globals and the <config dir>/../target/release sidecar layout beforePack resolves against.
function loadConfig(platform,helper){
 const root=mkdtempSync(join(tmpdir(),'velella-pack-')),release=join(root,'target','release');
 mkdirSync(release,{recursive:true});
 const dirname=join(root,'desktop'),exe=platform==='win32'?'.exe':'';
 for(const bin of ['velella-cli','velella-runtime'])writeFileSync(join(release,bin+exe),'');
 const module={exports:{}};
 vm.runInNewContext(configSource,{module,exports:module.exports,__dirname:dirname,__filename:join(dirname,'electron-builder.config.cjs'),
  process:{platform,arch:'x64'},require:name=>name==='./scripts/conpty-runtime.cjs'?helper:name==='builder-util'?{Arch:{x64:1}}:require(name)});
 return {config:module.exports,root};
}

test('Windows packaging prepares and carries the complete native console pair',async()=>{
 let prepared=0;
 const helper={...conpty,prepare:async()=>{prepared++}};
 const {config,root}=loadConfig('win32',helper);
 try{
 const resources=config.extraResources.filter(r=>r.to.startsWith('runtime/conpty')||r.to==='runtime/OpenConsole.exe');
 assert.deepEqual(Array.from(resources,r=>r.to),['runtime/conpty.dll','runtime/OpenConsole.exe','runtime/conpty-LICENSE.txt','runtime/conpty-provenance.json']);
 await assert.rejects(config.beforePack({electronPlatformName:'darwin',arch:1}),/Build on the target OS/,'packaging must not copy a host-built runtime into another platform');
 await config.beforePack({electronPlatformName:'win32',arch:1});
 assert.equal(prepared,1,'packaging must prepare sidecars before copying resources');
 helper.prepare=async()=>{throw Error('console dependency unavailable')};
 await assert.rejects(config.beforePack({electronPlatformName:'win32',arch:1}),/console dependency unavailable/);
 }finally{rmSync(root,{recursive:true,force:true})}
});

test('Other platform packages do not acquire Windows runtime dependencies',async()=>{
 let prepared=0;
 const {config,root}=loadConfig('linux',{...conpty,prepare:async()=>{prepared++}});
 try{
 assert.equal(config.extraResources.some(r=>r.to==='runtime/conpty.dll'),false);
 await config.beforePack({electronPlatformName:'linux',arch:1});assert.equal(prepared,0);
 }finally{rmSync(root,{recursive:true,force:true})}
});

test('Packaging stops before copying when a sidecar was never built',async()=>{
 const {config,root}=loadConfig('win32',{...conpty,prepare:async()=>{}});
 try{rmSync(join(root,'target','release','velella-cli.exe'));
  await assert.rejects(config.beforePack({electronPlatformName:'win32',arch:1}),/CLI missing/);
 }finally{rmSync(root,{recursive:true,force:true})}
});

test('Corrupt console dependencies fail verification instead of silently falling back',()=>{
 const folder=mkdtempSync(join(tmpdir(),'velella-conpty-integrity-'));
 try{writeFileSync(join(folder,'conpty.dll'),'corrupt');assert.throws(()=>conpty.verify(folder,'x64'),/integrity check failed/)}
 finally{unlinkSync(join(folder,'conpty.dll'));rmdirSync(folder)}
});
