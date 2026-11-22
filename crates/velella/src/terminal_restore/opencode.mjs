import {spawnSync} from 'node:child_process';
export const VelellaRestore=async({directory})=>{
 const roots=new Set(process.env.VELELLA_RESTORE_NATIVE_ID?[process.env.VELELLA_RESTORE_NATIVE_ID]:[]),children=new Set();
 let active=process.env.VELELLA_RESTORE_NATIVE_ID;
 const emit=(id,name)=>{if(!id)return;try{spawnSync(process.env.VELELLA_RESTORE_RUNTIME,['native-restore-hook',id,name],{env:process.env,cwd:directory||process.cwd(),windowsHide:true,timeout:3000,stdio:'ignore'});}catch{/* Observation cannot deny native actions. */}};
 return {'chat.message':async input=>{if(roots.has(input.sessionID)&&!children.has(input.sessionID)){active=input.sessionID;emit(input.sessionID,'UserPromptSubmit');}},event:async({event})=>{const id=event.properties?.sessionID||event.properties?.info?.id;if(event.type==='session.created'){if(event.properties?.info?.parentID){children.add(id);return;}roots.add(id);active=id;}const name={'session.created':'SessionStart','session.idle':'Stop'}[event.type];if(name&&id===active&&roots.has(id)&&!children.has(id))emit(id,name);}};
};
