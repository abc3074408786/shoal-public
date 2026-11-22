import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn} from 'node:child_process';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url);
const modulePath='../electron/codex-updater/codex-npm.cjs';
const vendor=path.join('node_modules','@openai','codex-win32-x64','vendor','x86_64-pc-windows-msvc','bin','codex.exe');
function pkg(root,version){
 fs.mkdirSync(path.join(root,'bin'),{recursive:true});
 fs.writeFileSync(path.join(root,'package.json'),JSON.stringify({name:'@openai/codex',version}));
 fs.writeFileSync(path.join(root,'bin/codex.js'),`console.log('codex-cli '+require('../package.json').version)`);
 fs.mkdirSync(path.dirname(path.join(root,vendor)),{recursive:true});
 fs.copyFileSync(path.join(process.env.SystemRoot,'System32/cmd.exe'),path.join(root,vendor));
}
function fixture(){
 const root=fs.mkdtempSync(path.join(os.tmpdir(),'velella codex update '));
 const prefix=path.join(root,'global'),current=path.join(prefix,'node_modules/@openai/codex'),next=path.join(root,'next');
 pkg(current,'1.0.0');pkg(next,'1.1.0');
 for(const name of ['codex','codex.cmd','codex.ps1'])fs.writeFileSync(path.join(prefix,name),'node "%~dp0/node_modules/@openai/codex/bin/codex.js"');
 const npm=path.join(root,'real npm/node_modules/npm/bin/npm-cli.js');
 fs.mkdirSync(path.dirname(npm),{recursive:true});
 fs.writeFileSync(npm,`const fs=require('node:fs'),path=require('node:path');const args=process.argv.slice(2);if(args[0]==='prefix'){console.log(process.env.FIXTURE_PREFIX);process.exit(0)}if(process.env.FIXTURE_FAILURE){process.exit(7)}if(args[0]==='view'){console.log(JSON.stringify(require(path.join(process.env.FIXTURE_NEXT,'package.json')).version));process.exit(0)}if(args[0]==='install'){const stage=args[args.indexOf('--prefix')+1];fs.cpSync(process.env.FIXTURE_NEXT,path.join(stage,'node_modules/@openai/codex'),{recursive:true});for(const name of ['codex','codex.cmd','codex.ps1'])fs.writeFileSync(path.join(stage,name),'node "%~dp0/node_modules/@openai/codex/bin/codex.js"');}else{console.log(JSON.stringify(args));process.exit(13)}`);
 return {root,prefix,current,next,npm,env:{...process.env,FIXTURE_PREFIX:prefix,FIXTURE_NEXT:next},cleanup(){fs.rmSync(root,{recursive:true,force:true})}};
}
test('only standalone global Codex installs are intercepted',()=>{
 const {isCodexUpdate}=require(modulePath);
 for(const args of [['install','-g','@openai/codex'],['i','--global','@openai/codex@latest'],['install','@openai/codex@0.166.0','-g']])assert.ok(isCodexUpdate(args));
 for(const args of [['install','@openai/codex'],['install','-g','other'],['install','-g','@openai/codex','other'],['install','-g','@openai/codex','--prefix','elsewhere'],['run','build'],['--version']])assert.equal(isCodexUpdate(args),false);
});
test('a live Windows image and denied directory renames do not prevent pointer activation',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();let held;
 try{
  held=spawn(path.join(f.current,vendor),['/d','/q','/c','echo HOLD_READY & set /p hold='],{windowsHide:true,stdio:['pipe','pipe','pipe']});
  await new Promise((resolve,reject)=>{held.once('error',reject);held.stdout.once('data',resolve)});
  assert.equal(held.exitCode,null);
  assert.throws(()=>fs.copyFileSync(path.join(f.next,vendor),path.join(f.current,vendor)),error=>['EBUSY','EPERM','EACCES'].includes(error.code));
  const {updateCodex,resolveActivePackage}=require(modulePath),rename=fs.renameSync;
  let result;
  fs.renameSync=(from,to)=>{if(fs.lstatSync(from).isDirectory()){const e=new Error('directory rename denied');e.code='EPERM';throw e}return rename(from,to)};
  try{result=await updateCodex({npmCli:f.npm,argv:['install','-g','@openai/codex'],env:f.env,stdout:()=>{}})}finally{fs.renameSync=rename}
  assert.equal(result.version,'1.1.0');assert.equal(held.exitCode,null,'old process must remain alive');
  assert.equal(JSON.parse(fs.readFileSync(path.join(f.current,'package.json'))).version,'1.0.0');
  assert.equal(resolveActivePackage(f.prefix),result.directory);
  assert.equal(JSON.parse(fs.readFileSync(path.join(result.directory,'package.json'))).version,'1.1.0');
  assert.equal(result.previousDirectory,f.current);
  const actual=await promisify(execFile)(process.execPath,[path.join(f.prefix,'.velella-codex-tools/codex-launch.cjs'),'--version'],{env:f.env,windowsHide:true});
  assert.equal(actual.stdout.trim(),'codex-cli 1.1.0');
  held.stdin.end('\n');await new Promise(resolve=>held.once('exit',resolve));held=undefined;
 }finally{if(held){held.kill();await new Promise(resolve=>held.once('exit',resolve))}f.cleanup()}
});
test('failed installation leaves the current package and launchers untouched',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  fs.writeFileSync(path.join(f.prefix,'codex.cmd'),'ORIGINAL');
  const {updateCodex}=require(modulePath);
  await assert.rejects(updateCodex({npmCli:f.npm,argv:['install','-g','@openai/codex'],env:{...f.env,FIXTURE_FAILURE:'1'},stdout:()=>{}}),/7/);
  assert.equal(JSON.parse(fs.readFileSync(path.join(f.current,'package.json'))).version,'1.0.0');
  assert.equal(fs.readFileSync(path.join(f.prefix,'codex.cmd'),'utf8'),'ORIGINAL');
 }finally{f.cleanup()}
});
test('a bad staged CLI fails validation before changing the current package',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  fs.writeFileSync(path.join(f.next,'bin/codex.js'),'process.exit(8)');
  const {updateCodex}=require(modulePath);
  await assert.rejects(updateCodex({npmCli:f.npm,argv:['install','-g','@openai/codex'],env:f.env,stdout:()=>{}}),/8/);
  assert.equal(JSON.parse(fs.readFileSync(path.join(f.current,'package.json'))).version,'1.0.0');
 }finally{f.cleanup()}
});
test('ordinary npm commands retain their arguments, working directory and exit status',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  const {runNpm}=require(modulePath);let output='';
  const status=await runNpm({npmCli:f.npm,argv:['run','build','--','a b'],env:f.env,cwd:f.root,stdout:text=>{output+=text}});
  assert.equal(status,13);assert.deepEqual(JSON.parse(output),['run','build','--','a b']);
 }finally{f.cleanup()}
});
test('a failed second pointer commit preserves the prior active version and launchers',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  const {updateCodex,resolveActivePackage}=require(modulePath),run=()=>updateCodex({npmCli:f.npm,argv:['install','-g','@openai/codex'],env:f.env,stdout:()=>{}});
  await run();const pointer=path.join(f.prefix,'.velella-codex-updates/active.json'),prior=fs.readFileSync(pointer),active=resolveActivePackage(f.prefix),original=fs.readFileSync(path.join(f.prefix,'codex.cmd'));
  fs.writeFileSync(path.join(f.next,'package.json'),JSON.stringify({name:'@openai/codex',version:'1.2.0'}));
  const rename=fs.renameSync;fs.renameSync=(from,to)=>{if(to===pointer){const e=new Error('pointer commit denied');e.code='EPERM';throw e}return rename(from,to)};
  try{await assert.rejects(run(),/pointer commit denied/)}finally{fs.renameSync=rename}
  assert.deepEqual(fs.readFileSync(pointer),prior);assert.equal(resolveActivePackage(f.prefix),active);
  assert.equal(JSON.parse(fs.readFileSync(path.join(f.current,'package.json'))).version,'1.0.0');
  assert.deepEqual(fs.readFileSync(path.join(f.prefix,'codex.cmd')),original);
 }finally{f.cleanup()}
});

test('successive updates and same-version checks select the pointer without replacing old directories',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  const {updateCodex,resolveActivePackage}=require(modulePath),run=()=>updateCodex({npmCli:f.npm,argv:['install','-g','@openai/codex'],env:f.env,stdout:()=>{}});
  const first=await run();fs.writeFileSync(path.join(f.next,'package.json'),JSON.stringify({name:'@openai/codex',version:'1.2.0'}));
  const second=await run();assert.equal(second.previousDirectory,first.directory);assert.notEqual(second.directory,first.directory);
  assert.equal(resolveActivePackage(f.prefix),second.directory);assert.equal((await run()).unchanged,true);
  assert.equal(JSON.parse(fs.readFileSync(path.join(first.directory,'package.json'))).version,'1.1.0');
  assert.equal(JSON.parse(fs.readFileSync(path.join(f.current,'package.json'))).version,'1.0.0');
 }finally{f.cleanup()}
});

test('invalid active pointers fail closed and release the update lock',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  const {updateCodex,resolveActivePackage}=require(modulePath),store=path.join(f.prefix,'.velella-codex-updates');fs.mkdirSync(store);
  fs.writeFileSync(path.join(store,'active.json'),JSON.stringify({schema:1,relative:'../../escape',version:'1.0.0'}));
  assert.throws(()=>resolveActivePackage(f.prefix),/inside this prefix/);
  await assert.rejects(updateCodex({npmCli:f.npm,argv:['install','-g','@openai/codex'],env:f.env,stdout:()=>{}}),/inside this prefix/);
  assert.equal(fs.existsSync(path.join(store,'update.lock')),false);
  fs.writeFileSync(path.join(store,'active.json'),'{invalid');
  await assert.rejects(updateCodex({npmCli:f.npm,argv:['install','-g','@openai/codex'],env:f.env,stdout:()=>{}}));
  assert.equal(fs.existsSync(path.join(store,'update.lock')),false);
 }finally{f.cleanup()}
});

test('format 2 crash recovery keeps the last atomic pointer authoritative',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  const {updateCodex,resolveActivePackage}=require(modulePath),run=()=>updateCodex({npmCli:f.npm,argv:['install','-g','@openai/codex'],env:f.env,stdout:()=>{}});
  const first=await run(),store=path.join(f.prefix,'.velella-codex-updates');
  fs.writeFileSync(path.join(store,'transaction.json'),JSON.stringify({format:2,phase:'switching',targetVersion:'1.2.0'}));
  assert.equal((await run()).unchanged,true);assert.equal(resolveActivePackage(f.prefix),first.directory);
 }finally{f.cleanup()}
});
test('concurrent updates are rejected without altering the live installation',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  const store=path.join(f.prefix,'.velella-codex-updates');fs.mkdirSync(store);
  fs.writeFileSync(path.join(store,'update.lock'),JSON.stringify({pid:process.pid,token:'fixture'}));
  const {updateCodex}=require(modulePath);
  await assert.rejects(updateCodex({npmCli:f.npm,argv:['install','-g','@openai/codex'],env:f.env,stdout:()=>{}}),/Another Codex update/);
  assert.equal(JSON.parse(fs.readFileSync(path.join(f.current,'package.json'))).version,'1.0.0');
 }finally{f.cleanup()}
});
test('a crash between directory renames is recovered before another update',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  const store=path.join(f.prefix,'.velella-codex-updates');fs.mkdirSync(store);
  const backup=path.join(store,'previous-fixture');fs.renameSync(f.current,backup);
  fs.writeFileSync(path.join(store,'transaction.json'),JSON.stringify({current:f.current,backup,phase:'switching'}));
  fs.writeFileSync(path.join(f.next,'package.json'),JSON.stringify({name:'@openai/codex',version:'1.0.0'}));
  const {updateCodex}=require(modulePath);
  const result=await updateCodex({npmCli:f.npm,argv:['install','-g','@openai/codex'],env:f.env,stdout:()=>{}});
  assert.equal(result.unchanged,true);assert.ok(fs.existsSync(f.current));
 }finally{f.cleanup()}
});
test('all npm Codex shims gain the guard without changing the official CLI or shell environment',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  const {installLauncherTools}=require(modulePath),cli=fs.readFileSync(path.join(f.current,'bin/codex.js'));
  installLauncherTools(f.prefix);installLauncherTools(f.prefix);
  for(const name of ['codex','codex.cmd','codex.ps1'])assert.match(fs.readFileSync(path.join(f.prefix,name),'utf8'),/\.velella-codex-tools/);
  assert.deepEqual(fs.readFileSync(path.join(f.current,'bin/codex.js')),cli);
  fs.writeFileSync(path.join(f.current,'bin/codex.js'),`console.log(JSON.stringify({argv:process.argv.slice(2),guard:process.env.PATH.split(';')[0]}))`);
  const child=spawn(process.execPath,[path.join(f.prefix,'.velella-codex-tools/codex-launch.cjs'),'resume','a b'],{env:f.env,windowsHide:true});let output='';child.stdout.on('data',data=>output+=data);
  const status=await new Promise(resolve=>child.once('exit',resolve));assert.equal(status,0);
  assert.deepEqual(JSON.parse(output),{argv:['resume','a b'],guard:path.join(f.prefix,'.velella-codex-tools')});
 }finally{f.cleanup()}
});
test('managed interactive config overrides explicitly select supported embedded mode without changing other launches',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  const {installLauncherTools}=require(modulePath);installLauncherTools(f.prefix);
  fs.writeFileSync(path.join(f.current,'bin/codex.js'),`if(process.argv.includes('--help'))console.log(process.env.FIXTURE_DAEMON_SUPPORTED==='1'?'  --no-daemon  Run without shared background server':'Usage: old Codex');else console.log(JSON.stringify(process.argv.slice(2)))`);
  const override=['-c','hooks.Stop=[{hooks=[{command="velella-runtime activity-hook"}]}]'];
  for(const [args,managed,supported,add] of [
   [override,true,true,true],
   [['resume','native session',...override],true,true,true],
   [['-m','agents',...override],true,true,true],
   [override,false,true,false],
   [override,true,false,false],
   [['--no-daemon',...override],true,true,false],
   [['exec',...override],true,true,false],
   [[...override,'app-server'],true,true,false],
   [['agents',...override],true,true,false],
   [['--remote=ws://127.0.0.1:1234',...override],true,true,false],
   [['--','-c','literal prompt'],true,true,false],
  ]){
   const env={...f.env,VELELLA_ACTIVITY_DIR:managed?path.join(f.root,'session'):'',VELELLA_SESSION_ENDPOINT:'',FIXTURE_DAEMON_SUPPORTED:supported?'1':'0'};
   const result=await promisify(execFile)(process.execPath,[path.join(f.prefix,'.velella-codex-tools/codex-launch.cjs'),...args],{env,windowsHide:true});
   assert.deepEqual(JSON.parse(result.stdout),add?['--no-daemon',...args]:args,JSON.stringify({args,managed,supported}));
  }
 }finally{f.cleanup()}
});

test('CMD and PowerShell npm entry points delegate ordinary commands with exact arguments',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  const guard=path.dirname(require.resolve(modulePath)),env={...f.env};
  const original=Object.entries(env).find(([key])=>key.toUpperCase()==='PATH')?.[1]||'';
  for(const key of Object.keys(env))if(key.toUpperCase()==='PATH')delete env[key];
  env.PATH=[guard,path.resolve(path.dirname(f.npm),'../../..'),path.dirname(process.execPath),original].join(';');
  for(const [program,args] of [
   ['cmd.exe',['/d','/c','npm run build -- "a b"']],
   ['powershell.exe',['-NoProfile','-NonInteractive','-File',path.join(guard,'npm.ps1'),'run','build','--','a b']],
  ]){
   const child=spawn(program,args,{env,cwd:f.root,windowsHide:true,windowsVerbatimArguments:program==='cmd.exe'});child.stdin.end();let output='';child.stdout.on('data',data=>output+=data);
   const status=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('exit',resolve)});
   assert.equal(status,13,output);assert.deepEqual(JSON.parse(output),['run','build','--','a b']);
  }
 }finally{f.cleanup()}
});
test('desktop runtime tools install only their own resources and preserve unrelated npm files',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  const {installTools}=require('../electron/codex-update-tools.cjs'),runtime=path.join(f.root,'runtime/velella-runtime.exe');
  fs.mkdirSync(path.dirname(runtime));fs.writeFileSync(runtime,'fixture runtime');
  await installTools(runtime);await installTools(runtime);
  for(const name of ['npm.cmd','npm.ps1','codex-npm.cjs','codex-launch.cjs'])assert.ok(fs.readFileSync(path.join(path.dirname(runtime),name),'utf8').includes('Velella Codex update guard v1'));
  fs.writeFileSync(path.join(path.dirname(runtime),'npm.cmd'),'UNRELATED');
  await assert.rejects(installTools(runtime),/unrelated/);
  assert.equal(fs.readFileSync(path.join(path.dirname(runtime),'npm.cmd'),'utf8'),'UNRELATED');
 }finally{f.cleanup()}
});
test('desktop bootstrap executes the unpacked runtime script and enables the npm launchers',{skip:process.platform!=='win32'},async()=>{
 const f=fixture();try{
  const {prepareCodexUpdateTools}=require('../electron/codex-update-tools.cjs'),runtime=path.join(f.root,'resources/runtime/velella-runtime.exe');
  fs.mkdirSync(path.dirname(runtime),{recursive:true});fs.writeFileSync(runtime,'fixture runtime');
  const env={...f.env};for(const key of Object.keys(env))if(key.toUpperCase()==='PATH')delete env[key];
  env.PATH=[path.resolve(path.dirname(f.npm),'../../..'),path.dirname(process.execPath),process.env.PATH||process.env.Path||''].join(';');
  let observed;
  const result=await prepareCodexUpdateTools({executable:runtime},{executeCommand:async(program,args,options)=>{
   observed=args[0];assert.ok(fs.existsSync(observed));assert.ok(!observed.includes('app.asar'));
   return promisify(execFile)(program,args,{...options,env,cwd:f.root});
  }});
  assert.equal(observed,path.join(path.dirname(runtime),'codex-npm.cjs'));
  assert.deepEqual(JSON.parse(result.stdout),{installed:true});
  assert.match(fs.readFileSync(path.join(f.prefix,'codex.cmd'),'utf8'),/\.velella-codex-tools/);
 }finally{f.cleanup()}
});
