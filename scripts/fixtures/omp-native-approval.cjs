// Native OMP deliberately ignores approval keys during its 500 ms guard.
// This helper waits for the rendered dialog and adds margin, then uses the
// dedicated approve-once key instead of Enter (which may edit a draft).
const fs=require('node:fs');
const APPROVAL_ARM_MS=800;
const COLLAB_COMMANDS=new Set(['collab.status','collab.receive','collab.ack','collab.candidate','collab.started','collab.report','collab.review.report']);
function nativeApprovalAllowed(message,exactAllowlist){
 if(exactAllowlist.includes(message))return true;
 const prefix='Allow tool: write Path: xd://mcp__velella_collaboration Arguments: ';
 if(!message.startsWith(prefix))return false;
 try{return COLLAB_COMMANDS.has(JSON.parse(message.slice(prefix.length)).cmd);}catch{return false;}
}
function lastApproval(raw){
 const matches=[...raw.matchAll(/7501;([^\x07\x1b]+)(?:\x07|\x1b\\)/g)];
 if(!matches.length)return null;
 const match=matches.at(-1),body=match[1];
 if(raw.lastIndexOf('7501;')!==match.index||!body.startsWith('state=blocked:kind=permission:'))return null;
 const encoded=body.match(/:msg=([A-Za-z0-9+/=]+)$/);if(!encoded)return null;
 const message=Buffer.from(encoded[1],'base64').toString('utf8');
 // OMP refreshes the same permission OSC after painting an unchanged dialog.
 // Preserve rendering only across consecutive, byte-identical permission
 // markers. Any other state/message begins a new epoch and discards old UI.
 let first=matches.length-1;
 while(first>0&&matches[first-1][1]===body)first--;
 const epoch=matches[first];
 const tail=raw.slice(match.index).replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g,'');
 // OMP redraws only changed cells: a consecutive dialog may redraw its title
 // without repeating unchanged choices. Its fresh border/title is rendered
 // only after the approval component has been mounted and focused.
 // Full redraw can emit its unchanged permission OSC after drawing the UI.
 // Accept that order only after a fresh clear/home, between two identical
 // permission markers, with the full focused dialog; stale UI is insufficient.
 const prior=matches.at(-2),clear=raw.lastIndexOf('\x1b[2J',match.index);
 const before=clear>=0?raw.slice(clear,match.index).replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g,''):'';
 const repaint=prior&&prior[1]===body&&clear>=epoch.index+epoch[0].length&&raw.slice(clear,match.index).includes('\x1b[H')&&before.includes('╭─ Allow tool:')&&before.includes('❯ Approve')&&before.includes('Deny');
 return {offset:match.index,message,rendered:tail.includes('╭─ Allow tool:')||!!repaint};
}
function pendingNativeApprovals(text){
 const calls=new Map();
 for(const line of text.split('\n')){let entry;try{entry=JSON.parse(line);}catch{continue;}const m=entry.message;
  if(m?.role==='assistant')for(const c of m.content||[])if(c.type==='toolCall'&&typeof c.id==='string'){
   if(['read','grep','glob'].includes(c.name))continue;
   let message='Native pending call: '+JSON.stringify({name:c.name,arguments:c.arguments});
   if(c.name==='bash'&&typeof c.arguments?.command==='string')message='Allow tool: bash Command: '+c.arguments.command;
   if(c.name==='write'&&c.arguments?.path==='xd://mcp__velella_collaboration'){
    try{message='Allow tool: write Path: xd://mcp__velella_collaboration Arguments: '+JSON.stringify(JSON.parse(c.arguments.content));}catch{}
   }
   calls.set(c.id,{id:c.id,message});
  }
  if(m?.role==='toolResult'&&typeof m.toolCallId==='string')calls.delete(m.toolCallId);
 }
 return [...calls.values()];
}
function createApprovalGuard(){
 const pending=new Map(),consumed=new Set();
 return {
  observe({sid,status,raw,allowed,pendingCalls,now=Date.now()}){
   if(status.exit!==null||status.state!=='waiting_permission')return null;
   const event=lastApproval(raw);if(!event)return null;
   if(pendingCalls&&(!pendingCalls.length||!pendingCalls.every(c=>allowed(c.message))))return null;
   const operationSet=pendingCalls?.map(c=>c.id).sort().join(',');
   const key=sid+':'+event.offset+(operationSet?':'+operationSet:'');if(consumed.has(key))return null;
   const previous=pending.get(sid);
   if(!previous||previous.key!==key||previous.pid!==status.pid||previous.sequence!==status.input_sequence){pending.set(sid,{key,pid:status.pid,sequence:status.input_sequence,at:now});return null;}
   if(!event.rendered||now-previous.at<APPROVAL_ARM_MS||!allowed(event.message))return null;
   return {...event,key,sid,pid:status.pid,sequence:status.input_sequence,operationSet,pendingCalls};
  },
  consume(ticket){consumed.add(ticket.key);pending.delete(ticket.sid);},
 };
}
async function approveOnce({ticket,statusFile,terminalFile,request,workspaceId,record,pendingFile}){
 // Recheck the exact dialog immediately before guarded input. A PTY receipt
 // is recorded as input acknowledgement, never as tool completion.
 const status=JSON.parse(fs.readFileSync(statusFile,'utf8'));
 const fresh=lastApproval(fs.readFileSync(terminalFile,'utf8'));
 if(status.state!=='waiting_permission'||status.exit!==null||status.pid!==ticket.pid||status.input_sequence!==ticket.sequence||!fresh?.rendered||fresh.offset!==ticket.offset||fresh.message!==ticket.message)throw Error('Native approval changed; no key sent');
 if(ticket.operationSet){
  const current=pendingFile?pendingNativeApprovals(fs.readFileSync(pendingFile,'utf8')):[];
  if(current.map(c=>c.id).sort().join(',')!==ticket.operationSet||JSON.stringify(current)!==JSON.stringify(ticket.pendingCalls))throw Error('Native queued calls changed; no key sent');
 }
 const response=await request({cmd:'workbench.terminal.input',workspace_id:workspaceId,session_id:ticket.sid,text:'\x11',enter:false,input_kind:'user',expected_input_sequence:ticket.sequence,expected_host_pid:ticket.pid});
 if(!response.ok)throw Error(response.error||'native approval input failed');
 record({sid:ticket.sid,message:ticket.message,possible_calls:ticket.pendingCalls,decision:'approve once',state:'input_acknowledged',input_sequence:ticket.sequence,at:new Date().toISOString(),response});
 const control=await request({cmd:'workbench.terminal.control',workspace_id:workspaceId,session_id:ticket.sid,human:false});
 if(!control.ok)throw Error(control.error||'cannot return native control');
}
module.exports={APPROVAL_ARM_MS,lastApproval,pendingNativeApprovals,createApprovalGuard,approveOnce,nativeApprovalAllowed};
