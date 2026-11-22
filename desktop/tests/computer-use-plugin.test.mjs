import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdtemp,mkdir,writeFile,readFile,readdir,stat,rm,copyFile,cp} from 'node:fs/promises';
import {join,resolve,sep} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
const require=createRequire(import.meta.url),run=promisify(execFile);
const {createComputerUseMcp}=require('../electron/computer-use-mcp.cjs');
const {createExtensions}=require('../electron/extensions.cjs');
const desktop=fileURLToPath(new URL('..',import.meta.url));
const realService=process.env.VELELLA_COMPUTER_USE_TEST_SOURCE||resolve(desktop,'../tools/computer-use');
const runtime=process.env.VELELLA_COMPUTER_USE_TEST_RUNTIME||resolve(desktop,'../artifacts/computer-use-20261117/runtime/velella-runtime.exe');
const python=join(realService,'.venv/Scripts/python.exe');

async function fixture(t){
  const directory=await mkdtemp(join(tmpdir(),'velella-computer-use-plugin-'));
  assert.ok(resolve(directory).startsWith(resolve(tmpdir())+sep));
  t.after(()=>rm(directory,{recursive:true,force:true}));
  return directory;
}

test('real reviewed generator produces stable observation/control catalog without launching service',async t=>{
  const profileRoot=await fixture(t);
  const service=createComputerUseMcp({profileRoot,serviceRoot:realService});
  const [a,b]=await Promise.all([service.prepare(),service.prepare()]);assert.equal(a.path,b.path);
  const bytes=await readFile(a.path,'utf8'),catalog=JSON.parse(bytes),modified=(await stat(a.path)).mtimeMs;
  assert.equal(catalog.entries[0].id,'velella-computer-use-observe');
  assert.deepEqual(catalog.entries[0].catalog,['computer_screenshot']);
  assert.deepEqual(catalog.entries[0].args,['-m','velella_computer_use.server']);
  assert.equal(catalog.entries[1].id,'velella-computer-use-control');
  assert.ok(catalog.entries[1].args.includes('--allow-input'));
  assert.ok(catalog.entries[1].catalog.includes('computer_type_text'));
  assert.ok(catalog.entries.every(e=>e.command===python));
  await service.prepare();assert.equal(await readFile(a.path,'utf8'),bytes);assert.equal((await stat(a.path)).mtimeMs,modified);
  assert.deepEqual(await readdir(join(profileRoot,'extensions/computer-use')),['catalog.json']);
  assert.deepEqual(await readdir(profileRoot),['extensions']);
});

test('bridge accepts only local known MCP plugin; never registers or enables native plugins',async t=>{
  const profileRoot=await fixture(t);let calls=0;
  const service=createExtensions({profileRoot,plugins:[],computerUse:{prepare:async()=>{calls++;return {path:join(profileRoot,'catalog.json')}}}});
  assert.equal((await service.request({target:{kind:'ssh'},action:'prepareBundledMcp',pluginId:'computer-use'})).ok,false);
  assert.equal((await service.request({target:{kind:'local'},action:'prepareBundledMcp',pluginId:'arbitrary',path:'C:/evil',command:'bad'})).ok,false);
  assert.equal(calls,0);
  const result=await service.request({target:{kind:'local'},action:'prepareBundledMcp',pluginId:'computer-use',path:'C:/ignored',command:'ignored'});
  assert.equal(result.ok,true);assert.equal(calls,1);assert.deepEqual(await readdir(profileRoot),[]);
});

test('unsupported platform or absent environment fails without creating profile state',async t=>{
  const profileRoot=await fixture(t),source=join(profileRoot,'service');
  await mkdir(join(source,'src/velella_computer_use'),{recursive:true});await writeFile(join(source,'src/velella_computer_use/catalog.py'),'not executed');
  await assert.rejects(createComputerUseMcp({profileRoot,serviceRoot:source,platform:'linux'}).prepare(),/仅支持本机 Windows/);
  await assert.rejects(createComputerUseMcp({profileRoot,serviceRoot:source}).prepare(),/Python 环境尚未准备/);
  await assert.rejects(createComputerUseMcp({profileRoot,serviceRoot:join(profileRoot,'missing')}).prepare(),/服务文件缺失/);
  assert.deepEqual(await readdir(profileRoot),['service']);
});

test('generator failure cleans temporary output and retains previous catalog; explicit retry can recover',async t=>{
  const profileRoot=await fixture(t),valid=await createComputerUseMcp({profileRoot,serviceRoot:realService}).prepare();
  const before=await readFile(valid.path);let failed=true;
  const service=createComputerUseMcp({profileRoot,serviceRoot:realService,run:async(command,args,options)=>{
    if(!args.includes('--output'))return run(command,args,options);
    if(failed){await writeFile(args[args.indexOf('--output')+1],'partial');throw Error('synthetic generator failure');}
    return run(command,args,options);
  }});
  await assert.rejects(service.prepare(),/synthetic generator failure/);assert.deepEqual(await readFile(valid.path),before);
  assert.deepEqual(await readdir(join(profileRoot,'extensions/computer-use')),['catalog.json']);
  failed=false;assert.equal((await service.prepare()).path,valid.path);
});

test('unexpected generator output is rejected before publication',async t=>{
  const profileRoot=await fixture(t);
  const service=createComputerUseMcp({profileRoot,serviceRoot:realService,run:async(command,args,options)=>{
    if(!args.includes('--output'))return run(command,args,options);
    await writeFile(args[args.indexOf('--output')+1],JSON.stringify({version:1,entries:[]}));
  }});
  await assert.rejects(service.prepare(),/不符合预期/);assert.deepEqual(await readdir(join(profileRoot,'extensions/computer-use')),[]);
});

test('packaged resource layout generates with isolated profile interpreter, without developer path',async t=>{
  const profileRoot=await fixture(t),serviceRoot=join(profileRoot,'resources/plugins/computer-use'),venv=join(profileRoot,'extensions/computer-use/venv');
  await cp(join(realService,'src'),join(serviceRoot,'src'),{recursive:true,filter:p=>!p.includes('__pycache__')&&!p.endsWith('.pyc')});
  await mkdir(join(venv,'Scripts'),{recursive:true});
  await copyFile(python,join(venv,'Scripts/python.exe'));await copyFile(join(realService,'.venv/pyvenv.cfg'),join(venv,'pyvenv.cfg'));
  // An interpreter-only venv is incomplete and must not publish a catalog.
  await assert.rejects(createComputerUseMcp({profileRoot,serviceRoot}).prepare(),/运行依赖未就绪/);
  assert.equal((await readdir(join(profileRoot,'extensions/computer-use'))).includes('catalog.json'),false);
  await cp(join(realService,'.venv/Lib'),join(venv,'Lib'),{recursive:true,filter:p=>!p.includes('__pycache__')&&!p.endsWith('.pyc')});
  await writeFile(join(venv,'Lib/site-packages/_editable_impl_velella_computer_use.pth'),join(serviceRoot,'src'));
  const result=await createComputerUseMcp({profileRoot,serviceRoot}).prepare();
  const catalog=JSON.parse(await readFile(result.path,'utf8'));
  assert.ok(catalog.entries.every(e=>e.command===join(venv,'Scripts/python.exe')));
  assert.ok(!JSON.stringify(catalog).includes(realService));
});

test('plugin-generated directory passes actual Rust preview and subset confirmation on disposable daemon',async t=>{
  const profileRoot=await fixture(t),catalog=await createComputerUseMcp({profileRoot,serviceRoot:realService}).prepare();
  const data=join(profileRoot,'daemon-data'),home=join(profileRoot,'home');await mkdir(home);
  const env={...process.env,HOME:home,USERPROFILE:home,APPDATA:join(home,'roaming'),LOCALAPPDATA:join(home,'local'),CODEX_HOME:join(home,'codex'),VELELLA_DATA_DIR:data};
  const daemon=spawn(runtime,['daemon',data],{env,windowsHide:true,stdio:'ignore'});
  const exited=new Promise(resolveExit=>daemon.once('exit',resolveExit));
  t.after(async()=>{if(daemon.exitCode===null){daemon.kill();await exited;}});
  async function cli(...args){const result=await run(join(runtime,'../velella-cli.exe'),['--data-dir',data,'--json',...args],{env,windowsHide:true,timeout:15000,maxBuffer:1048576});return JSON.parse(result.stdout);}
  let ready=false;
  for(let i=0;i<40;i++){try{ready=(await cli('status')).ok}catch{}if(ready)break;await new Promise(r=>setTimeout(r,100));}
  assert.equal(ready,true,'isolated daemon is available');
  const preview=await cli('mcp','preview',catalog.path);assert.equal(preview.ok,true);
  const manifest=preview.result.catalog;assert.equal(manifest.entries.length,2);
  const confirmed=await cli('mcp','confirm',catalog.path,'--digest',manifest.digest,'--entry','velella-computer-use-observe','--tool','computer_screenshot','--confirm');
  assert.equal(confirmed.ok,true);assert.deepEqual(confirmed.result.server.tools,['computer_screenshot']);
  assert.equal(confirmed.result.server.source.path,manifest.path);assert.equal(confirmed.result.server.source.digest,manifest.digest);
  if(process.env.VELELLA_COMPUTER_USE_PLUGIN_EVIDENCE){
    const out=resolve(process.env.VELELLA_COMPUTER_USE_PLUGIN_EVIDENCE);await mkdir(out,{recursive:true});
    await writeFile(join(out,'real-catalog.json'),JSON.stringify({completed:true,native_input:false,model_requests:false,preview,confirmed},null,2)+'\n');
  }
  daemon.kill();await exited;assert.ok(daemon.exitCode!==null||daemon.signalCode!==null,'daemon emitted exit after termination');
});
