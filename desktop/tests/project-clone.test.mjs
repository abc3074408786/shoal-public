import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdtemp,mkdir,writeFile,readFile,stat} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve,relative,sep} from 'node:path';
import {execFileSync} from 'node:child_process';
import {createServer} from 'node:http';
import {randomUUID} from 'node:crypto';
const {createProjectCloner,validateRemote,validateName}=createRequire(import.meta.url)('../electron/project-clone.cjs');
const git=(...args)=>execFileSync('git',args,{encoding:'utf8',windowsHide:true}).trim();

test('clone validation excludes command transports, credentials and invalid destination names',()=>{
  for(const value of ['ext::cmd /c calc','file:///C:/repo','--upload-pack=evil','https://u:p@example.com/repo','https://u@example.com/repo','https://example.com/repo?q=1','C:\\repo','https://example.com/'])assert.throws(()=>validateRemote(value));
  for(const value of ['https://example.com/repo.git','ssh://git@example.com/team/repo.git','git@example.com:team/repo.git'])assert.equal(validateRemote(value),value);
  for(const value of ['../escape','CON','aux.txt','name.','a:b',''])assert.throws(()=>validateName(value));
  assert.equal(validateName('我的项目'),'我的项目');
});

test('real HTTP clone, idempotency, registration recovery, no overwrite and cancellation',{timeout:45000},async()=>{
  const root=await mkdtemp(join(tmpdir(),'velella-clone-test-')),seed=join(root,'seed'),remote=join(root,'repo.git'),dest=join(root,'projects');
  await mkdir(seed);await mkdir(dest);git('-C',seed,'init','-b','main');await writeFile(join(seed,'README.md'),'local clone verification\n');git('-C',seed,'add','.');git('-C',seed,'-c','user.name=Fixture','-c','user.email=fixture@localhost','-c','commit.gpgsign=false','commit','-m','fixture');git('clone','--bare',seed,remote);git('--git-dir='+remote,'update-server-info');
  let slowRequested;const slowSeen=new Promise(resolve=>slowRequested=resolve);
  const server=createServer(async(req,res)=>{try{const url=new URL(req.url,'http://localhost');if(url.pathname.startsWith('/slow.git/')){slowRequested();return}const file=resolve(root,'.'+decodeURIComponent(url.pathname)),rel=relative(root,file);if(rel.startsWith('..'+sep)||rel==='..'){res.writeHead(403);res.end();return}const bytes=await readFile(file);res.setHeader('Content-Type','text/plain');res.end(bytes)}catch{res.writeHead(404);res.end()}});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const url=`http://127.0.0.1:${server.address().port}/repo.git`;let registrations=0,failRegistration=false;
  const runtime={request:async r=>r.cmd==='workbench.snapshot'?{ok:true,projects:[]}:(registrations++,failRegistration?(failRegistration=false,{ok:false,error:'registration unavailable'}):{ok:true,project:{id:'p-'+registrations,path:r.path}})};
  const cloner=createProjectCloner(runtime);
  try{
    const request={request_id:randomUUID(),url,parent:dest,name:'clone-one'};
    const [a,b]=await Promise.all([cloner.clone(request),cloner.clone(request)]);assert(a.ok&&b.ok);assert.equal(a.project.id,b.project.id);assert.equal(registrations,1);assert.equal(await readFile(join(a.path,'README.md'),'utf8'),'local clone verification\n');assert.equal(git('-C',a.path,'rev-parse','HEAD'),git('-C',seed,'rev-parse','HEAD'));
    const existing=await cloner.clone({...request,request_id:randomUUID()});assert(!existing.ok);assert.match(existing.error,/已存在/);assert.equal(await readFile(join(a.path,'README.md'),'utf8'),'local clone verification\n');
    failRegistration=true;const recovery={...request,request_id:randomUUID(),name:'recover'};const first=await cloner.clone(recovery);assert(first.retry_registration);await writeFile(join(first.retained_path,'user-note.txt'),'retain me');const second=await cloner.clone(recovery);assert(second.ok);assert.equal(await readFile(join(second.path,'user-note.txt'),'utf8'),'retain me');
    const early={...request,request_id:randomUUID(),name:'cancel-before'};const pending=cloner.clone(early);assert(cloner.cancel(early.request_id).cancelled);const cancelled=await pending;assert.match(cancelled.error,/取消/);await assert.rejects(stat(join(dest,'cancel-before')),{code:'ENOENT'});
    const slow={...request,request_id:randomUUID(),name:'cancel-during',url:url.replace('repo.git','slow.git')};const running=cloner.clone(slow);await slowSeen;assert(cloner.cancel(slow.request_id).cancelled);const result=await running;assert(!result.ok);assert.match(result.error,/取消/);
    console.log(JSON.stringify({verified:'real Git HTTP clone and cancellation',root,registrations}));
  }finally{cloner.dispose();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
});
