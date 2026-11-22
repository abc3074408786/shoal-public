const assert=require('node:assert/strict'),crypto=require('node:crypto');

async function runTerminalGpuProfile({app,page,sid,row,bytes,rpc,until,usable,probe,preserve,checkSibling,untouched,profileDesktop,record,result,expectRelease}){
 const owner=await app.browserWindow(page),initial=await row(sid),history=bytes();
 // Keep only a WeakRef to the original WebGL context, so diagnostics do not
 // themselves retain the context/ANGLE resources being measured.
 await page.evaluate(sid=>{
  const viewer=window.__kbBaselineViewers.find(v=>v.sessionId===sid&&v.terminal.element?.isConnected);
  if(!viewer)throw Error('Expected live terminal missing');
  const gl=viewer.terminal._core?._renderService?._renderer?.value?._gl;
  if(!gl||gl.isContextLost())throw Error('Native test requires actual WebGL');
  window.__kbGpuWitness={terminal:viewer.terminal,context:new WeakRef(gl)};
 },sid);
 const contexts=()=>page.evaluate(()=>{
  const {terminal,context}=window.__kbGpuWitness;
  const gl=terminal._core?._renderService?._renderer?.value?._gl,old=context.deref();
  return {drawing:!!gl&&!gl.isContextLost(),oldLost:!old||old.isContextLost(),sameContext:gl===old};
 });
 const checkHidden=async label=>{
  if(expectRelease)await until(async()=>{const s=await contexts();return !s.drawing&&s.oldLost},label+' releases WebGL');
  else{const s=await contexts();assert.ok(s.drawing&&s.sameContext,label+' control unexpectedly released WebGL')}
  result.terminalGpuLifecycle??=[];result.terminalGpuLifecycle.push({label,...await contexts()});
 };
 const checkRestored=async label=>{
  await until(async()=>(await contexts()).drawing,label+' restores WebGL');
  const state=await contexts();if(expectRelease)assert.equal(state.sameContext,false,label+' reused released context');
  assert.equal((await row(sid)).status.pid,initial.status.pid,'window operation changed session host');
  result.terminalGpuLifecycle.push({label,...state});
  await page.evaluate(()=>{const w=window.__kbGpuWitness;w.context=new WeakRef(w.terminal._core._renderService._renderer.value._gl)});
 };
 const writable=await usable();await writable.asElement().focus();await writable.dispose();
 const marker='VELELLA_HIDDEN_'+crypto.randomBytes(12).toString('hex'),backwards=[...marker].reverse().join('');
 const command=`Start-Sleep -Milliseconds 2000; Write-Output (-join ('${backwards}'.ToCharArray()[${marker.length-1}..0]))`;
 assert.ok(!command.includes(marker));const before=bytes().length;
 await page.keyboard.type(command);await page.keyboard.press('Enter');
 await owner.evaluate(win=>win.hide());
 await until(async()=>!(await owner.evaluate(win=>win.isVisible())),'test window hidden');
 await checkHidden('hidden');
 await until(()=>bytes().subarray(before).includes(Buffer.from(marker)),'shell keeps running while hidden');
 await until(()=>page.evaluate(marker=>{
  const term=window.__kbGpuWitness.terminal;
  return Array.from({length:term.buffer.active.length},(_,i)=>term.buffer.active.getLine(i)?.translateToString(true)).some(line=>line?.includes(marker));
 },marker),'hidden renderer keeps consuming output');
 const output=await rpc({cmd:'workbench.terminal.output',workspace_id:result.gpuWorkspace,session_id:sid,since:before});
 const chunk=Buffer.from(output.data,'base64');assert.ok(chunk.includes(Buffer.from(marker)));assert.ok(chunk.equals(bytes().subarray(before,before+chunk.length)));
 record('hidden original shell emits real output and retained xterm consumes exact raw bytes',{session_id:sid,marker,from:before,length:chunk.length,output_sha256:crypto.createHash('sha256').update(chunk).digest('hex')});
 await profileDesktop('terminal-hidden','hidden');
 await owner.evaluate(win=>{win.show();win.focus()});await checkRestored('after-hide');
 await probe('restore hidden window preserves original terminal keyboard input');preserve(history);await checkSibling(untouched);
 await profileDesktop('terminal-restored');
 await owner.evaluate(win=>win.minimize());
 await until(()=>owner.evaluate(win=>win.isMinimized()),'test window minimized');await checkHidden('minimized');
 await profileDesktop('terminal-minimized','minimized');
 await owner.evaluate(win=>{win.restore();win.show();win.focus()});await checkRestored('after-minimize');
 await probe('restore minimized window preserves original terminal keyboard input');preserve(history);await checkSibling(untouched);
 await profileDesktop('terminal-final');
 await page.evaluate(()=>{delete window.__kbGpuWitness});
 result.actualTerminalGraphicsVerified=true;
}
module.exports={runTerminalGpuProfile};
