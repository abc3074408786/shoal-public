// Content-free observation only. Never modifies product props, DOM or input.
// Serialized into the isolated renderer by the existing product harness.
function installRecoveryFocusTrace() {
 const records=[];
 const ids=new WeakMap();let next=0,last='';
 const identity=node=>{if(!node)return null;if(!ids.has(node))ids.set(node,++next);return ids.get(node)};
 function element(node){
  if(!(node instanceof Element))return null;
  const box=node.getBoundingClientRect(),style=getComputedStyle(node);
  return {node:identity(node),tag:node.tagName,className:node.className,
   connected:node.isConnected,pane:node.closest('[data-terminal-pane]')?.getAttribute('data-terminal-pane'),
   rect:{x:box.x,y:box.y,width:box.width,height:box.height},rects:node.getClientRects().length,
   display:style.display,visibility:style.visibility,contentVisibility:style.contentVisibility,
   disabled:node.disabled,readOnly:node.readOnly,tabIndex:node.tabIndex,
   hiddenAncestor:!!node.closest('[hidden],[inert]'),ariaHidden:node.getAttribute('aria-hidden')};
 }
 function props(canvas){
  // Read-only optional diagnostics; production React may omit this private key.
  const key=canvas&&Object.keys(canvas).find(value=>value.startsWith('__reactFiber$'));
  let fiber=key&&canvas[key];
  for(let depth=0;fiber&&depth<30;depth++,fiber=fiber.return){
   const value=fiber.memoizedProps;
   if(!value?.workspace?.id)continue;
   const {workspace,visible,historyOnly}=value;
   const select=status=>status?{pid:status.pid,exit:status.exit,state:status.state,input_protocol:status.input_protocol}:null;
   return {session:workspace.id,visible,historyOnly,status:select(workspace.status),
    alternateStatus:select(fiber.alternate?.memoizedProps?.workspace?.status)};
  }
  return null;
 }
 function state(){
  return {documentFocus:document.hasFocus(),visibilityState:document.visibilityState,
   active:element(document.activeElement),loadingCount:document.querySelectorAll('.terminal-loading').length,
   viewers:(window.__kbBaselineViewers||[]).slice(-8).map(({terminal,sessionId})=>{
    const root=terminal.element,canvas=root?.closest('.terminal-canvas');
    return {sessionId,root:element(root),input:element(root?.querySelector('.xterm-helper-textarea')),
     disableStdin:terminal.options.disableStdin,loading:canvas?.getAttribute('data-terminal-loading'),
     props:props(canvas),ancestors:canvas?[canvas,...ancestors(canvas)].slice(0,8).map(element):[]};
   })};
 }
 function ancestors(node){const result=[];for(let p=node.parentElement;p&&result.length<8;p=p.parentElement)result.push(p);return result}
 function capture(reason,force=false,extra={}){
  const value=state(),serialized=JSON.stringify(value);
  if(records.length<350&&(force||serialized!==last)){records.push({at:performance.now(),reason,...extra,...value});last=serialized}
 }
 const focus=event=>capture(event.type,true,{target:element(event.target),related:element(event.relatedTarget)});
 document.addEventListener('focusin',focus,true);document.addEventListener('focusout',focus,true);
 const created=()=>capture('terminal-created',true);
 window.addEventListener('velella:terminal-created',created);
 const observer=new MutationObserver(()=>capture('dom-change'));
 observer.observe(document.body,{subtree:true,childList:true,attributes:true,
  attributeFilter:['style','class','disabled','readonly','hidden','inert','data-terminal-loading']});
 const timer=setInterval(()=>capture('sample'),100);
 window.__kbFocusTrace={capture,finish(){
  capture('finished',true);clearInterval(timer);observer.disconnect();
  document.removeEventListener('focusin',focus,true);document.removeEventListener('focusout',focus,true);
  window.removeEventListener('velella:terminal-created',created);
  return {records,limit:350};
 }};
 capture('installed',true);
}
module.exports={installRecoveryFocusTrace};
