// Invocation-local native skill discovery. No model call and no global edits.
const {spawn}=require('node:child_process');
const path=require('node:path');
const {StringDecoder}=require('node:string_decoder');
function query(args){return new Promise((resolve,reject)=>{
 const child=spawn('codex',['-c','features.apps=false','-c','features.multi_agent=false','-c','features.fast_mode=false',...args,'app-server'],{cwd:__dirname,stdio:['pipe','pipe','ignore']});
 let buffer='',total=0,settled=false;const decoder=new StringDecoder('utf8');
 const done=(err,value)=>{if(settled)return;settled=true;clearTimeout(timer);child.kill();err?reject(err):resolve(value)};
 const timer=setTimeout(()=>done(Error('native skill preflight timed out')),10000);
 child.on('error',e=>done(e));child.on('exit',()=>done(Error('native skill preflight exited')));
 const send=v=>child.stdin.write(JSON.stringify(v)+'\n');
 child.stdout.on('data',chunk=>{try{
  total+=chunk.length;if(total>4*1024*1024)throw Error('native skill response exceeds limit');buffer+=decoder.write(chunk);
  while(buffer.includes('\n')){const end=buffer.indexOf('\n'),line=buffer.slice(0,end);buffer=buffer.slice(end+1);if(line.length>1024*1024)throw Error('native skill frame exceeds limit');const v=JSON.parse(line);
   if(v.id===1){if(v.error)throw Error('native skill initialize rejected');send({method:'initialized'});send({id:2,method:'skills/list',params:{cwds:[__dirname],forceReload:true}})}
   else if(v.id===2){if(v.error)throw Error('native skill query rejected');const data=v.result?.data;if(!Array.isArray(data)||data.length!==1||!Array.isArray(data[0].errors)||data[0].errors.length||!Array.isArray(data[0].skills)||data[0].skills.length>2048)throw Error('invalid native skill catalog');
    const seen=new Set(),items=data[0].skills.map(s=>{if(typeof s.name!=='string'||!s.name||s.name.length>256||typeof s.path!=='string'||!path.isAbsolute(s.path)||s.path.length>4096||typeof s.enabled!=='boolean'||seen.has(s.path))throw Error('invalid native skill identity');seen.add(s.path);return {name:s.name,path:s.path,enabled:s.enabled}}).sort((a,b)=>a.path.localeCompare(b.path));done(null,items)}
  }
 }catch(e){done(e)}});
 send({id:1,method:'initialize',params:{clientInfo:{name:'velella-remote-skill-policy',version:'1'},capabilities:{experimentalApi:true}}});
})}
(async()=>{
 const initial=await query([]),names=[...new Set(initial.map(s=>s.name))].sort();
 const rules='skills.config=['+names.map(name=>'{name='+JSON.stringify(name)+',enabled=false}').join(',')+']';
 if(Buffer.byteLength(rules)>24000)throw Error('remote skill policy exceeds limit');
 const disabled=await query(['-c',rules]);
 if(JSON.stringify(initial.map(s=>[s.path,s.name]))!==JSON.stringify(disabled.map(s=>[s.path,s.name]))||disabled.some(s=>s.enabled))throw Error('remote native skills could not be disabled');
 process.stdout.write(JSON.stringify({catalog:disabled,rules})+'\n');
})().catch(e=>{process.stderr.write(String(e.message||e)+'\n');process.exitCode=1});
