import {spawnSync} from 'node:child_process';
export default function observe(pi){
 const emit=(name,ctx)=>{try{const id=ctx.sessionManager.getSessionId();if(!id)return;spawnSync(process.env.VELELLA_RESTORE_RUNTIME,['native-restore-hook',id,name],{env:process.env,cwd:ctx.cwd||process.cwd(),windowsHide:true,timeout:3000,stdio:'ignore'});}catch{/* Observers never block native input. */}};
 pi.on('session_start',(_event,ctx)=>emit('SessionStart',ctx));
 pi.on('input',(_event,ctx)=>{emit('UserPromptSubmit',ctx);});
 pi.on('agent_end',(_event,ctx)=>emit('Stop',ctx));
}
