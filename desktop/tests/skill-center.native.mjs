// Real CLI smoke runs. Isolated project directories; no login, global installs or permission bypass.
import {mkdir,writeFile,readFile} from 'node:fs/promises';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {join,resolve} from 'node:path';
import {randomUUID} from 'node:crypto';
const exec=promisify(execFile),out=resolve('../artifacts/skill-center-20261109/native'),cli=resolve('../target/debug/velella-cli.exe');
await mkdir(out,{recursive:true});
const results=await readFile(join(out,'results.json'),'utf8').then(s=>JSON.parse(s).results,()=>[]);
async function engine(library,r){const file=join(library,'request.json');await mkdir(library,{recursive:true});await writeFile(file,JSON.stringify(r));const {stdout}=await exec(cli,['skills','center','--library',library,'--file',file,'--json'],{windowsHide:true,maxBuffer:10*1024*1024});const value=JSON.parse(stdout);if(!value.ok)throw Error(JSON.stringify(value.error));return value.result;}
async function run(agent,cwd,args){
 const script=join(cwd,'run.ps1'),output=join(cwd,'output.txt');
 const literal=s=>"'"+s.replaceAll("'","''")+"'";
 await writeFile(script,'$ErrorActionPreference=\'Stop\'\n& '+({codex:'codex','claude-code':'claude',opencode:'opencode',omp:'omp',kimi:'kimi',gemini:'gemini',zcode:'zcode-free'}[agent])+' '+args.map(literal).join(' ')+'\nexit $LASTEXITCODE\n');
 return new Promise(resolveResult=>{const child=spawn('powershell.exe',['-NoProfile','-NonInteractive','-File',script],{cwd,windowsHide:true,stdio:['ignore','pipe','pipe']});let text='',timedOut=false;const consume=b=>{if(text.length<4*1024*1024)text+=b.toString()};child.stdout.on('data',consume);child.stderr.on('data',consume);const timer=setTimeout(()=>{timedOut=true;execFile('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true},()=>{})},60000);child.on('error',e=>consume(e.message));child.on('close',async code=>{clearTimeout(timer);await writeFile(output,text);resolveResult({code,timedOut,text,output})});});
}
for(const agent of process.argv.slice(2).length?process.argv.slice(2):['codex','claude-code','omp','opencode','kimi']){
 const attempt=process.env.SKILL_CENTER_SMOKE_ATTEMPT||'';const base=join(out,agent,attempt);const cwd=join(base,'workspace'),library=join(base,'library'),source=join(base,'source');await mkdir(cwd,{recursive:true});await mkdir(join(source,'references'),{recursive:true});
 const marker='SC_'+randomUUID();await writeFile(join(source,'references/marker.txt'),marker);await writeFile(join(source,'SKILL.md'),'---\nname: velella-skill-center-smoke\ndescription: Use for the Skill Center smoke check.\n---\nRead references/marker.txt relative to this skill directory. Return exactly the marker from that resource, without other text.\n');
 try{
  const p=await engine(library,{operation:'preview',source:{kind:'local',path:source}});await engine(library,{operation:'import',name:'velella-skill-center-smoke',candidate:p.candidate.id,digest:p.digest,confirmed:true});
  let prompt='Use the velella-skill-center-smoke skill for the Skill Center smoke check and return only its resource marker. Do not edit any files.';
  if(agent==='omp'){const context=await engine(library,{operation:'context',name:'velella-skill-center-smoke',digest:p.digest,agent,task:prompt});prompt=context.prompt;}
  else{
   const sourcePreview=await engine(library,{operation:'preview',source:{kind:'local',path:source}});await engine(library,{operation:'import',scope:'project',project:cwd,name:'velella-skill-center-smoke',candidate:sourcePreview.candidate.id,digest:p.digest,confirmed:true});
   const dest=await engine(library,{operation:'distributionPreview',scope:'project',project:cwd,name:'velella-skill-center-smoke',agent});
   await engine(library,{operation:'distribute',scope:'project',project:cwd,name:'velella-skill-center-smoke',agent,digest:p.digest,expected_digest:dest.current_digest??null,confirmed:true});
  }
  const argv={codex:['exec','--skip-git-repo-check','--sandbox','read-only','--json',prompt],'claude-code':['-p','--output-format','json','--tools=Read','--allowedTools=Read','--',prompt],omp:['--no-session','--mode','json','-p',prompt],opencode:['run','--format','json',prompt],kimi:['--output-format','stream-json','--prompt',prompt],gemini:['--prompt',prompt]}[agent];
  if(!argv)throw Error('Agent has no verified smoke invocation');const r=await run(agent,cwd,argv);const passed=r.code===0&&r.text.includes(marker);
  const result={agent,passed,exitCode:r.code,timedOut:r.timedOut,markerObserved:r.text.includes(marker),output:r.output,route:agent==='omp'?'snapshot_context':'native_project_directory'};results.push(result);console.log(JSON.stringify(result));
 }catch(error){const result={agent,passed:false,stage:'setup',error:error.message};results.push(result);console.log(JSON.stringify(result));}
 await writeFile(join(out,'results.json'),JSON.stringify({results,liveModelCalls:true,globalSkillsChanged:false},null,2));
}
