// Passive diagnostics only. No viewport override, forced click, layout rewrite,
// focus repair, native resize, reload, or change to product state.
const fs=require('node:fs'),path=require('node:path');
async function startNativeViewportTrace(page,out){
 const cdp=await page.context().newCDPSession(page);
 const file=path.join(out,'viewport-cdp.jsonl');
 let stopped=false,timer,pending=Promise.resolve(),count=0;
 await page.evaluate(()=>{
  const events=[];window.__kbNativeViewportEvents=events;
  const record=event=>{
   const target=event.target instanceof Element?event.target:null;
   const rect=target?.getBoundingClientRect();
   events.push({epoch:Date.now()/1000,at:performance.now(),type:event.type,
    inner:[innerWidth,innerHeight],outer:[outerWidth,outerHeight],
    target:target?{tag:target.tagName,className:String(target.className),
     text:target.textContent?.slice(0,80),rect:rect?{x:rect.x,y:rect.y,width:rect.width,height:rect.height}:null}:null,
    point:event instanceof MouseEvent?{x:event.clientX,y:event.clientY,button:event.button}:null});
   if(events.length>128)events.shift();
  };
  for(const type of ['resize','focus','blur'])window.addEventListener(type,record);
  for(const type of ['pointerdown','pointerup','mousedown','mouseup','click','focusin'])document.addEventListener(type,record,true);
  visualViewport?.addEventListener('resize',record);
 });
 async function capture(label){
  const started=Date.now()/1000;
  try{
   const [layout,dom]=await Promise.all([
    cdp.send('Page.getLayoutMetrics'),
    page.evaluate(()=>({epoch:Date.now()/1000,at:performance.now(),inner:[innerWidth,innerHeight],
     outer:[outerWidth,outerHeight],dpr:devicePixelRatio,document:[document.documentElement.clientWidth,document.documentElement.clientHeight],
     screen:[screen.width,screen.height,screen.availWidth,screen.availHeight],
     visibility:document.visibilityState,focus:document.hasFocus(),shellPage:document.querySelector('[data-shell-page]')?.getAttribute('data-shell-page'),
     historyButtons:[...document.querySelectorAll('button')].filter(e=>e.textContent.trim().startsWith('历史会话')).map(e=>{
      const r=e.getBoundingClientRect();return {disabled:e.disabled,rect:{x:r.x,y:r.y,width:r.width,height:r.height}};
     }),events:window.__kbNativeViewportEvents.splice(0)}))]);
   fs.appendFileSync(file,JSON.stringify({started,ended:Date.now()/1000,label,layout,dom})+'\n');count++;
  }catch(error){fs.appendFileSync(file,JSON.stringify({started,ended:Date.now()/1000,label,error:String(error)})+'\n')}
 }
 async function loop(){
  await capture('sample');
  if(!stopped&&count<800)timer=setTimeout(()=>{pending=loop()},75);
 }
 pending=loop();
 return async()=>{
  stopped=true;clearTimeout(timer);await pending;await capture('final');await cdp.detach();
  return {file,count,controllerBounds:'not directly queried; see Win32 child HWND bounds in samples.jsonl'};
 };
}
module.exports={startNativeViewportTrace};
