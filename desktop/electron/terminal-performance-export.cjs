const {mkdir,writeFile}=require('node:fs/promises');
const {join}=require('node:path');
const {randomUUID}=require('node:crypto');

const stages=new Set(['native_input','composition_start','composition_end','input_queued','input_send','input_ack','output_read','output_parse','render_callback','paint_opportunity']);
const kinds=new Set(['key','enter','ime_key','paste','user','protocol','pointer']);
const numeric=['ms','queueMs','units','bytes','from','end'];
function nonnegative(value){return typeof value==='number'&&Number.isFinite(value)&&value>=0}

// Only diagnostic metadata crosses the file boundary. Neither a caller-supplied
// filename nor arbitrary snapshot properties are accepted.
function sanitizeSnapshot(value){
 if(!value||value.version!==1||typeof value.sessionId!=='string'||!/^[a-zA-Z0-9_-]{1,128}$/.test(value.sessionId)||!Array.isArray(value.entries)||value.entries.length>512)throw Error('Invalid terminal performance record');
 if(!Number.isSafeInteger(value.dropped)||value.dropped<0)throw Error('Invalid dropped count');
 const entries=value.entries.map(item=>{
  if(!item||!stages.has(item.stage)||!Number.isSafeInteger(item.sequence)||item.sequence<1||!nonnegative(item.atMs))throw Error('Invalid timing entry');
  const entry={sequence:item.sequence,atMs:item.atMs,stage:item.stage};
  if(item.kind!==undefined){if(!kinds.has(item.kind))throw Error('Invalid input category');entry.kind=item.kind}
  for(const key of numeric)if(item[key]!==undefined){if(!nonnegative(item[key]))throw Error('Invalid timing value');entry[key]=item[key]}
  for(const key of ['failed','documentHidden'])if(item[key]!==undefined){if(typeof item[key]!=='boolean')throw Error('Invalid timing flag');entry[key]=item[key]}
  return entry;
 });
 return {version:1,sessionId:value.sessionId,exportedAt:new Date().toISOString(),retained:entries.length,dropped:value.dropped,privacy:'Timing and counts only; no command, output or clipboard content.',paintMeaning:'paint_opportunity is a browser frame opportunity, not proof of physical screen presentation.',entries};
}

async function exportTerminalPerformance(snapshot,directory){
 const data=JSON.stringify(sanitizeSnapshot(snapshot),null,2);
 const path=join(directory,`velella-terminal-performance-${Date.now()}-${randomUUID()}.json`);
 await mkdir(directory,{recursive:true});
 await writeFile(path,data,{encoding:'utf8',flag:'wx',mode:0o600});
 return {path};
}
module.exports={sanitizeSnapshot,exportTerminalPerformance};
