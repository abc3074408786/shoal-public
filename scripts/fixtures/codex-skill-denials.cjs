// No model, login or trust operation. Exercise skills.config against the native
// app-server catalog; a listed skill with enabled=false is not an active skill.
const fs=require('node:fs'),path=require('node:path'),os=require('node:os');
const {spawn}=require('node:child_process');const assert=require('node:assert/strict');
const binary=process.argv[2];assert.ok(binary,'explicit installed Codex executable required');
const inspectCurrent=process.argv[3]==='--inspect-current';
const root=fs.mkdtempSync(path.join(os.tmpdir(),'velella-codex-denials-'));
const cwd=inspectCurrent?process.cwd():path.join(root,'workspace'),home=path.join(root,'home'),codexHome=path.join(root,'codex-home');
if(!inspectCurrent)for(const dir of [cwd,home,codexHome])fs.mkdirSync(dir,{recursive:true});
function skill(dir,name){fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(path.join(dir,'SKILL.md'),`---\nname: ${name}\ndescription: synthetic unselected skill\n---\nMust not be implicitly selected.\n`);}
if(!inspectCurrent){skill(path.join(cwd,'.agents/skills/project-unselected'),'project-unselected');skill(path.join(codexHome,'skills/user-unselected'),'user-unselected');}
const env=inspectCurrent?{...process.env}:{...process.env,CODEX_HOME:codexHome,HOME:home,USERPROFILE:home};
async function catalog(overrides=[]){
  const child=spawn(binary,[...overrides,'app-server'],{cwd,env,windowsHide:true,stdio:['pipe','pipe','ignore']});
  let text='',count=0;const pending=new Map();
  const request=(method,params)=>new Promise((resolve,reject)=>{const id=++count;pending.set(id,{resolve,reject});child.stdin.write(JSON.stringify({id,method,params})+'\n');});
  child.stdout.on('data',data=>{text+=data;if(text.length>1024*1024){for(const p of pending.values())p.reject(Error('catalog exceeds bound'));return;}
    let end;while((end=text.indexOf('\n'))>=0){const line=text.slice(0,end);text=text.slice(end+1);if(!line)continue;const value=JSON.parse(line);const p=pending.get(value.id);if(p){pending.delete(value.id);value.error?p.reject(Error('native RPC failed')):p.resolve(value.result);}}
  });
  child.on('error',error=>{for(const p of pending.values())p.reject(error);});
  const timeout=setTimeout(()=>{for(const p of pending.values())p.reject(Error('native catalog timed out'));child.kill();},15000);
  try{
    await request('initialize',{clientInfo:{name:'velella-skill-fixture',version:'1'},capabilities:{experimentalApi:true}});
    child.stdin.write('{"method":"initialized"}\n');
    const result=await request('skills/list',{cwds:[cwd],forceReload:true});
    assert.equal(result.data.length,1);assert.deepEqual(result.data[0].errors,[]);
    return result.data[0].skills.map(({path,name,enabled})=>({path,name,enabled}));
  }finally{clearTimeout(timeout);child.stdin.end();child.kill();await new Promise(resolve=>child.exitCode!==null?resolve():child.once('exit',resolve));}
}
(async()=>{
  const before=await catalog();
  if(inspectCurrent){console.log(JSON.stringify({scope:'current catalog metadata only; no model or account operation',skills:before.length,denialUtf16Length:before.map(s=>`{path=${JSON.stringify(s.path)},enabled=false}`).join(',').length,nameDenialUtf16Length:[...new Set(before.map(s=>s.name))].map(name=>`{name=${JSON.stringify(name)},enabled=false}`).join(',').length}));return;}
  for(const name of ['project-unselected','user-unselected'])assert.ok(before.some(s=>s.name===name&&s.enabled));
  const rules=[...new Set(before.map(s=>s.name))].map(name=>`{name=${JSON.stringify(name)},enabled=false}`).join(',');
  const args=['-c',`skills.config=[${rules}]`];
  const after=await catalog(args);assert.ok(after.length);assert.ok(after.every(s=>s.enabled===false));
  skill(path.join(cwd,'.agents/skills/new-unselected'),'new-unselected');
  const drift=await catalog(args);assert.ok(drift.some(s=>s.name==='new-unselected'&&s.enabled),'new path must be detected by recovery verification');
  const result={root,scope:'native app-server catalog, no model invocation; disabled entries remain listed',before,after,driftDetected:true};
  fs.writeFileSync(path.join(root,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result,null,2));
})().catch(error=>{console.error(error);process.exitCode=1;});
