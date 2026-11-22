import {invoke} from './desktop-platform';
import {isTauri} from './api';
import {currentTarget} from './execution-target';

type Surface={workspace:string;element:HTMLElement};
const surfaces=new Map<string,Surface>();
const opening=new Map<string,Promise<unknown>>();
let installed=false,scheduled=false,dragging=false,lastPayload='',queue=Promise.resolve();
const resize=new ResizeObserver(()=>scheduleBrowserSurfaces());

export function ensureBrowserPage(key:string,action:()=>Promise<unknown>){
  const existing=opening.get(key);if(existing)return existing;
  const pending=action().finally(()=>{if(opening.get(key)===pending)opening.delete(key)});
  opening.set(key,pending);return pending;
}

export function installBrowserSurfaces(){
  if(installed||!isTauri())return;
  installed=true;
  new MutationObserver(()=>scheduleBrowserSurfaces()).observe(document.body,{subtree:true,childList:true,attributes:true,attributeFilter:['open','data-state','style','class','hidden']});
  window.addEventListener('resize',scheduleBrowserSurfaces);
  window.addEventListener('scroll',scheduleBrowserSurfaces,true);
  window.addEventListener('dragstart',()=>{dragging=true;scheduleBrowserSurfaces()},true);
  const end=()=>{dragging=false;scheduleBrowserSurfaces()};
  window.addEventListener('dragend',end,true);window.addEventListener('drop',end,true);
  scheduleBrowserSurfaces();
}

export function registerBrowserSurface(id:string,workspace:string,element:HTMLElement){
  installBrowserSurfaces();
  const previous=surfaces.get(id);if(previous)resize.unobserve(previous.element);
  surfaces.set(id,{workspace,element});resize.observe(element);scheduleBrowserSurfaces();
  return ()=>{if(surfaces.get(id)?.element===element){surfaces.delete(id);resize.unobserve(element);scheduleBrowserSurfaces()}};
}

export function scheduleBrowserSurfaces(){
  if(scheduled||!isTauri())return;
  scheduled=true;
  requestAnimationFrame(()=>{
    scheduled=false;
    queue=queue.catch(()=>{}).then(async()=>{
      const blocked=dragging||!!document.querySelector('dialog[open],[role="dialog"][data-state="open"],[role="alertdialog"],[role="menu"][data-state="open"]');
      const frames=blocked?[]:[...surfaces].flatMap(([id,{workspace,element}])=>{
        if(!element.isConnected)return [];
        const r=element.getBoundingClientRect(),x=Math.max(0,r.x),y=Math.max(0,r.y);
        const width=Math.min(innerWidth,r.right)-x,height=Math.min(innerHeight,r.bottom)-y;
        if(width<1||height<1||!element.checkVisibility())return [];
        return [{id,workspace_id:workspace,x,y,width,height}];
      });
      const payload=JSON.stringify(frames);
      // The embedded browser is a machine-local surface: a remote workbench must not move it or
      // hand it local paths. The main process repeats this check on the target it is given.
      const target=currentTarget();
      if(target.kind!=='local'){lastPayload='';return;}
      if(payload===lastPayload)return;
      lastPayload=payload;
      try{await invoke('browser_layout',{request:{frames},target})}catch(error){
        // A failed placement must never leave a webpage above a local dialog.
        await invoke('browser_layout',{request:{frames:[]},target}).catch(()=>{});
        lastPayload='';
        window.dispatchEvent(new CustomEvent('velella:browser-layout-error',{detail:String(error)}));
      }
    });
  });
}
