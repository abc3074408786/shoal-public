// Harness readiness only; never changes the terminal, application state or focus.
function writableViewer({sessionId,stableMs,requireViewer}) {
 const registry=window.__kbBaselineInputReadiness??=new Map();
 const entry=window.__kbBaselineViewers?.filter(value=>value.sessionId===sessionId).at(-1);
 const observed=entry?.terminal?.element;
 const root=observed?.isConnected||requireViewer?observed:
  document.querySelector(`[data-terminal-pane="${CSS.escape(sessionId)}"] .xterm`);
 const canvas=root?.closest('.terminal-canvas');
 const input=root?.querySelector('.xterm-helper-textarea');
 const style=input&&getComputedStyle(input);
 const ready=root?.isConnected&&input?.isConnected&&canvas&&
  input.closest('[data-terminal-pane]')?.getAttribute('data-terminal-pane')===sessionId&&
  (root!==observed||!entry?.terminal.options.disableStdin)&&!input.disabled&&!input.readOnly&&
  canvas.getAttribute('data-terminal-loading')!=='true'&&canvas.getAttribute('aria-busy')!=='true'&&
  !canvas.closest('.terminal-wrap')?.querySelector('.terminal-loading')&&
  !input.closest('[inert],[hidden]')&&input.getClientRects().length>0&&
  style.display!=='none'&&style.visibility==='visible';
 if(!ready){registry.delete(sessionId);return false}
 const previous=registry.get(sessionId),now=performance.now();
 if(previous?.input!==input){registry.set(sessionId,{input,since:now});return false}
 if(now-previous.since<stableMs)return false;
 return input;
}
async function waitForWritableViewer(page,sessionId,{stableMs=100,timeout=18000,requireViewer=true}={}){
 try{
  return await page.waitForFunction(writableViewer,{sessionId,stableMs,requireViewer},{polling:'raf',timeout});
 }finally{
  await page.evaluate(id=>window.__kbBaselineInputReadiness?.delete(id),sessionId).catch(()=>{});
 }
}
module.exports={waitForWritableViewer};
