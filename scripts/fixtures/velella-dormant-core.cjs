const {spawn}=require('node:child_process'),fs=require('node:fs'),path=require('node:path'),{createInterface}=require('node:readline');
const assert=require('node:assert/strict');
async function startDormantCore({binary,electron,desktop,data,out,env,event}){
 fs.writeFileSync(path.join(data,'dormancy-experiment.json'),JSON.stringify({protocol:'velella-dormancy-v1',scope:'local-shell-edit',root:data}));
 const child=spawn(binary,[],{windowsHide:true,env:{...env,VELELLA_DORMANCY_EXPERIMENT:'1',VELELLA_CORE_EXECUTABLE:electron,VELELLA_CORE_ENTRY:desktop,VELELLA_CORE_LOG:path.join(out,'core-electron.log')},stdio:['pipe','pipe',fs.openSync(path.join(out,'core-error.log'),'a')]});
 event({kind:'root',role:'dormant-core',pid:child.pid});
 const messages=[],waiters=new Set();let failure;
 child.on('error',error=>{failure=error;for(const waiter of waiters)waiter()});
 child.stdin.on('error',error=>{failure=error;for(const waiter of waiters)waiter()});
 const lines=createInterface({input:child.stdout});
 lines.on('line',line=>{try{const value=JSON.parse(line);messages.push(value);event({kind:'core-state',...value});for(const waiter of waiters)waiter()}catch(error){failure=error;for(const waiter of waiters)waiter()}});
 function wait(predicate,after=0){return new Promise((resolve,reject)=>{
  let timer;const check=()=>{const found=messages.slice(after).find(predicate);if(found||failure){clearTimeout(timer);waiters.delete(check);failure?reject(failure):resolve(found)}};
  timer=setTimeout(()=>{waiters.delete(check);reject(Error('Native core response timeout'))},10000);waiters.add(check);check();
 })}
 await wait(value=>value.event==='ready');
 const command=async(text,expected)=>{const after=messages.length;child.stdin.write(text+'\n');const response=await wait(value=>value.event===expected||value.event==='refused',after);assert.equal(response.error,'',JSON.stringify(response));return response};
 return {child,messages,wake:()=>command('1','active'),status:()=>command('?','status'),async stop(){const state=await command('?','status');assert.equal(state.state,0,'will not stop core with a live GUI');await command('q','stopped');if(child.exitCode===null)await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('Core shutdown timeout')),5000);child.once('exit',()=>{clearTimeout(timer);resolve()})});lines.close()}};
}
module.exports={startDormantCore};
