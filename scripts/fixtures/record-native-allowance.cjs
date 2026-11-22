// Operator utility: approve exact already-inspected native calls by immutable
// call ID. It edits only this disposable run's test allowlist; sends no key.
const fs=require('node:fs'),path=require('node:path');
const {pendingNativeApprovals,lastApproval}=require('./omp-native-approval.cjs');
const activeCaption=process.argv.includes('--active-caption');
const base=path.resolve(process.argv[2]),ids=new Set(process.argv.slice(3).filter(v=>v!=='--active-caption'));
if(!path.basename(base).startsWith('orchestrated-parallel-')||!ids.size||[...ids].some(id=>!/^call_[A-Za-z0-9_]+$/.test(id)))throw Error('Explicit fixture directory and native call IDs required');
const run=JSON.parse(fs.readFileSync(path.join(base,'snapshot.json'),'utf8')),found=[],sessions=new Map();
for(const member of run.status.members){
 const folder=path.join(base,'project/.velella/sessions',member.identity.session_id),file=path.join(folder,'launch.json');if(!fs.existsSync(file))continue;
 const launch=JSON.parse(fs.readFileSync(file,'utf8')),dir=launch.spawn.env.PI_CODING_AGENT_SESSION_DIR;
 const statusFile=path.join(folder,'status.json');if(!dir||!fs.existsSync(dir)||!fs.existsSync(statusFile))continue;
 const status=JSON.parse(fs.readFileSync(statusFile,'utf8')),name=fs.readdirSync(dir).find(n=>n.endsWith('_'+status.native_id+'.jsonl'));if(!name)continue;
 const pending=pendingNativeApprovals(fs.readFileSync(path.join(dir,name),'utf8'));sessions.set(member.identity.session_id,{folder,status,pending});
 for(const call of pending)if(ids.has(call.id))found.push({...call,sid:member.identity.session_id});
}
if(new Set(found.map(c=>c.id)).size!==ids.size)throw Error('A requested call is no longer pending; allowlist unchanged');
const file=path.join(base,'native-allowlist.json'),allow=JSON.parse(fs.readFileSync(file,'utf8'));
for(const call of found)if(!allow.includes(call.message))allow.push(call.message);
const captions=[];
if(activeCaption)for(const sid of new Set(found.map(c=>c.sid))){
 const s=sessions.get(sid);if(s.status.state!=='waiting_permission'||!s.pending.every(c=>allow.includes(c.message)||(()=>{const prefix='Allow tool: write Path: xd://mcp__velella_collaboration Arguments: ';try{return c.message.startsWith(prefix)&&['collab.status','collab.receive','collab.ack','collab.candidate','collab.started','collab.report','collab.review.report'].includes(JSON.parse(c.message.slice(prefix.length)).cmd);}catch{return false;}})()))throw Error('Unreviewed queued calls; active caption not allowed');
 const event=lastApproval(fs.readFileSync(path.join(s.folder,'terminal.bin'),'utf8'));if(!event||!event.rendered)throw Error('No current rendered native approval');
 if(!allow.includes(event.message))allow.push(event.message);captions.push({sid,message:event.message,pending_calls:s.pending});
}
fs.writeFileSync(file,JSON.stringify(allow,null,2));
const log=path.join(base,'operator-decisions.json'),rows=fs.existsSync(log)?JSON.parse(fs.readFileSync(log,'utf8')):[];
rows.push({decision:'allow exact inspected native calls',calls:found,normalized_active_captions:captions,at:new Date().toISOString()});fs.writeFileSync(log,JSON.stringify(rows,null,2));console.log(JSON.stringify({allowlisted:found.map(c=>c.id),active_captions:captions.length}));
