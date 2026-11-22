import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {tmpdir} from 'node:os';
const {createPrChecks,createReadOnlyRunner,parseRemote,normalizeCheck,normalizePull,safeLink}=createRequire(import.meta.url)('../electron/pr-checks.cjs');
const repository={host:'github.com',name:'owner/repo',url:'https://github.com/owner/repo'};
const pull={number:7,title:'修复检查',url:repository.url+'/pull/7',state:'OPEN',isDraft:false,headRefName:'feature',baseRefName:'main',headRefOid:'a'.repeat(40),updatedAt:'2026-10-25T01:00:00Z',reviewDecision:'REVIEW_REQUIRED',headRepository:{nameWithOwner:'owner/repo'},statusCheckRollup:[]};
const snapshot={ok:true,workspaces:[{id:'w',path:tmpdir()}]};
function fixture(overrides={}){
 const calls=[];
 const run=async(binary,args,cwd)=>{calls.push({binary,args,cwd});if(overrides.run)return overrides.run(binary,args,cwd);
  if(binary==='git'&&args.includes('rev-parse'))return {code:0,stdout:'true\n',stderr:''};
  if(binary==='git'&&args.includes('symbolic-ref'))return {code:0,stdout:overrides.branch||'feature\n',stderr:''};
  if(binary==='git')return {code:0,stdout:overrides.remotes||'remote.origin.url https://github.com/owner/repo.git\n',stderr:''};
  return overrides.gh||{code:0,stdout:JSON.stringify(overrides.pulls||[pull]),stderr:''};
 };
 return {calls,service:createPrChecks({request:async()=>overrides.snapshot||snapshot},{run})};
}
test('GitHub remotes and links reject credentials, command transports and unsafe URLs',()=>{
 for(const value of ['https://github.com/owner/repo.git','git@github.com:owner/repo.git','ssh://git@github.com/owner/repo.git'])assert.deepEqual(parseRemote(value),repository);
 for(const value of ['ext::cmd /c calc','https://u:p@github.com/owner/repo','https://github.com/owner/repo?token=secret','file:///repo','https://gitlab.com/owner/repo','https://github.com/../repo'])assert.equal(parseRemote(value),null);
 assert.equal(safeLink('javascript:alert(1)'),null);assert.equal(safeLink('https://u:p@example.com/run'),null);
});
test('check states never turn pending, cancelled or unknown results into success',()=>{
 const states={SUCCESS:'success',FAILURE:'failure',TIMED_OUT:'failure',ACTION_REQUIRED:'failure',IN_PROGRESS:'pending',QUEUED:'pending',CANCELLED:'cancelled',NEUTRAL:'skipped',SKIPPED:'skipped',COMPLETED:'unknown'};
 for(const [status,expected] of Object.entries(states))assert.equal(normalizeCheck({name:'CI',status},0).status,expected);
 assert.equal(normalizeCheck({__typename:'StatusContext',context:'external CI',state:'ERROR'},0).status,'failure');
 assert.equal(normalizeCheck({__typename:'StatusContext',context:'external CI',state:'PENDING'},0).status,'pending');
 const value=normalizePull({...pull,statusCheckRollup:[{name:'Build',conclusion:'SUCCESS'},{name:'Test',status:'IN_PROGRESS'},{name:'Deploy',conclusion:'CANCELLED'}]},repository);
 assert.deepEqual(value.summary,{success:1,pending:1,failure:0,cancelled:1,skipped:0,unknown:0});assert.equal(value.head_sha,pull.headRefOid);
 assert.throws(()=>normalizePull({...pull,url:'https://elsewhere.test/pull/7'},repository));
});
test('workspace path comes from runtime, requests use fixed readonly commands and explicit repo/branch',async()=>{
 const {service,calls}=fixture({branch:'feature;echo suspicious'});const result=await service.read({workspace_id:'w',path:'C:/not-authorized'});
 assert.equal(result.state,'ready');assert.equal(result.pull_requests[0].number,7);
 assert(calls.every(c=>c.cwd===tmpdir()));const gh=calls.find(c=>c.binary==='gh');assert.deepEqual(gh.args.slice(0,2),['pr','list']);assert(gh.args.includes('--head=feature;echo suspicious'));assert(gh.args.includes('--repo=owner/repo'));
 const unknown=fixture({snapshot:{ok:true,workspaces:[]}});assert.equal((await unknown.service.read({workspace_id:'unknown'})).state,'error');assert.equal(unknown.calls.length,0);
});
test('forks with identical branch names are excluded and upstream PRs remain discoverable',async()=>{
 const {service}=fixture({remotes:'remote.origin.url git@github.com:fork/repo.git\nremote.upstream.url https://github.com/owner/repo.git',pulls:[{...pull,headRepository:{nameWithOwner:'other/repo'}},{...pull,number:8,url:repository.url+'/pull/8',headRepository:{nameWithOwner:'fork/repo'}}]});
 const result=await service.read({workspace_id:'w'});assert.deepEqual(result.pull_requests.map(p=>p.number),[8]);
});
test('no PR, unavailable auth, provider errors and non-Git states are distinct and do not leak stderr',async()=>{
 assert.equal((await fixture({pulls:[]}).service.read({workspace_id:'w'})).state,'no_pr');
 assert.equal((await fixture({gh:{code:4,stdout:'',stderr:'secret token'}}).service.read({workspace_id:'w'})).state,'auth_required');
 const failed=await fixture({gh:{code:1,stdout:'',stderr:'secret token'}}).service.read({workspace_id:'w'});assert.equal(failed.state,'error');assert(!JSON.stringify(failed).includes('secret'));
 assert.equal((await fixture({run:async()=>({code:128,stdout:'',stderr:'not a git repository'})}).service.read({workspace_id:'w'})).state,'not_git');
 assert.equal((await fixture({run:async()=>{throw Object.assign(Error('secret'),{code:'missing'})}}).service.read({workspace_id:'w'})).state,'tool_unavailable');
});
test('concurrent refreshes share a single query',async()=>{
 let release;const wait=new Promise(resolve=>release=resolve);let count=0;
 const service=createPrChecks({request:async()=>{count++;await wait;return {ok:true,workspaces:[]}}},{run:async()=>assert.fail()});
 const first=service.read({workspace_id:'w'}),second=service.read({workspace_id:'w'});assert.equal(first,second);release();await first;assert.equal(count,1);
});
test('owned command runner preserves split UTF-8 and enforces timeout and output limit',async()=>{
 const runner=createReadOnlyRunner();
 try{
  const value=await runner.run(process.execPath,['-e',"const b=Buffer.from('中文');process.stdout.write(b.subarray(0,1));setTimeout(()=>process.stdout.write(b.subarray(1)),10)"],tmpdir());assert.equal(value.stdout,'中文');
  await assert.rejects(runner.run(process.execPath,['-e','setTimeout(()=>{},10000)'],tmpdir(),50),{code:'timeout'});
  await assert.rejects(runner.run(process.execPath,['-e','process.stdout.write(Buffer.alloc(5*1024*1024))'],tmpdir()),{code:'too_large'});
 }finally{runner.dispose()}
});
