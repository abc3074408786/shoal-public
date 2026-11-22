import type {} from './desktop-platform';

type State={visible:boolean;minimized:boolean};
const listeners=new Set<(drawable:boolean)=>void>();
let drawable=true,generation=0,disconnect:(()=>void)|undefined;
const checked=(value:unknown):value is State=>!!value&&typeof value==='object'&&
  typeof (value as State).visible==='boolean'&&typeof (value as State).minimized==='boolean';
function publish(value:boolean){
  if(value===drawable)return;
  drawable=value;for(const listener of [...listeners])listener(value);
}
function connect(){
  const current=++generation,bridge=window.velellaDesktop;
  if(!bridge){
    const changed=()=>publish(document.visibilityState==='visible');
    drawable=document.visibilityState==='visible';
    document.addEventListener('visibilitychange',changed);
    disconnect=()=>document.removeEventListener('visibilitychange',changed);
    return;
  }
  // Older desktop bridges lack this optional graphics signal. Keep rendering
  // in that case, rather than guessing hidden from Chromium's throttling state.
  drawable=true;let events=0;
  const accept=(value:unknown)=>{if(current===generation&&checked(value))publish(value.visible&&!value.minimized)};
  try{
    disconnect=bridge.listen('window-visibility',event=>{if(current===generation&&checked(event.payload)){events++;accept(event.payload)}});
    const before=events;
    void bridge.invoke('window',{action:'visibility'}).then(value=>{if(events===before)accept(value)}).catch(()=>{});
  }catch{/* Older bridge: no signal, keep existing rendering behavior. */}
}
export function subscribeTerminalWindowVisibility(listener:(drawable:boolean)=>void){
  listeners.add(listener);if(listeners.size===1)connect();listener(drawable);
  let subscribed=true;
  return()=>{
    if(!subscribed)return;subscribed=false;listeners.delete(listener);
    if(!listeners.size){generation++;disconnect?.();disconnect=undefined;drawable=true}
  };
}
