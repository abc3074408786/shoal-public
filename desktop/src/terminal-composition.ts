import type {Terminal} from '@xterm/xterm';

/** Own IME transactions without changing xterm's source or relying on private events. */
export function installTerminalComposition(term:Terminal,view:{start:()=>void;update:(text:string)=>void;end:()=>void},modifiedEnter?:(event:KeyboardEvent)=>string|undefined) {
  const textarea=term.textarea,element=term.element;
  if(!textarea||!element)return {dispose(){}};
  let active=false,implicit=false,canceled=false,disposed=false,fallback='',tail='';
  let timer:ReturnType<typeof setTimeout>|undefined;
  let pendingEnter:{data:string;code:string;released:boolean}|undefined;
  let enterOwner:{code:string;until:number}|undefined;
  const stop=(event:Event)=>{event.stopImmediatePropagation()};
  const clearTimer=()=>{if(timer!==undefined){clearTimeout(timer);timer=undefined}};
  const writable=()=>!disposed&&!term.options.disableStdin&&textarea.isConnected&&textarea.ownerDocument.activeElement===textarea;
  const send=(value:string)=>{
    if(value&&!canceled&&writable()){
      term.input(value,true);
      if(pendingEnter){
        const pending=pendingEnter;pendingEnter=undefined;
        if(!pending.released)enterOwner={code:pending.code,until:Date.now()+250};
        term.input(pending.data,true);
      }
    }
    textarea.value='';
  };
  const ownsEnter=(event:KeyboardEvent)=>{
    if(enterOwner&&Date.now()>enterOwner.until)enterOwner=undefined;
    return !!enterOwner&&(event.code===enterOwner.code||(!event.code&&event.key==='Enter'));
  };
  const defer=()=>{
    clearTimer();timer=setTimeout(()=>{
      timer=undefined;
      if(!implicit||active||!writable())return;
      const value=textarea.value||fallback;
      if(!value)return;
      implicit=false;fallback='';tail=value;send(value);
    },32);
  };
  const start=(event:CompositionEvent)=>{
    stop(event);clearTimer();tail='';fallback='';implicit=false;pendingEnter=undefined;enterOwner=undefined;
    canceled=!writable();active=true;textarea.value='';view.start();
  };
  const update=(event:CompositionEvent)=>{stop(event);if(active&&!canceled)view.update(event.data)};
  const end=(event:CompositionEvent)=>{
    stop(event);clearTimer();
    const value=active&&!canceled?event.data:'';
    active=false;implicit=false;fallback='';tail=event.data;
    send(value);pendingEnter=undefined;view.end();
  };
  const keydown=(event:KeyboardEvent)=>{
    if(ownsEnter(event)){event.preventDefault();stop(event);return}
    if(active){
      if(event.key==='Escape'){canceled=true;pendingEnter=undefined;textarea.value='';view.end()}
      else if(!event.repeat&&!canceled){
        const data=modifiedEnter?.(event);
        if(data!==undefined)pendingEnter={data,code:event.code||'Enter',released:false};
        else if(pendingEnter&&event.code!==pendingEnter.code&&!['Shift','Control','Alt','Meta'].includes(event.key))pendingEnter=undefined;
      }
      stop(event);return;
    }
    tail='';
    if(event.keyCode===229||event.isComposing){
      stop(event);canceled=false;implicit=true;fallback='';textarea.value='';defer();
      const data=modifiedEnter?.(event);
      if(data!==undefined)pendingEnter={data,code:event.code||'Enter',released:false};
      else if(pendingEnter&&event.code!==pendingEnter.code)pendingEnter=undefined;
    }else{clearTimer();implicit=false;fallback='';pendingEnter=undefined}
  };
  const keypress=(event:KeyboardEvent)=>{
    if(ownsEnter(event)){event.preventDefault();stop(event);return}
    if(active){stop(event);return}
    if(tail&&(event.key===tail||String.fromCodePoint(event.charCode)===tail)){stop(event);return}
    if(implicit||(!event.ctrlKey&&!event.metaKey&&!event.altKey&&event.charCode>127)){
      stop(event);implicit=true;fallback=event.key.length===1?event.key:String.fromCodePoint(event.charCode);defer();
    }
  };
  const keyup=(event:KeyboardEvent)=>{
    if(pendingEnter&&(event.code===pendingEnter.code||(!event.code&&event.key==='Enter'))){pendingEnter.released=true;stop(event);return}
    if(ownsEnter(event)){enterOwner=undefined;event.preventDefault();stop(event);return}
    if(active)stop(event);
  };
  const beforeinput=(event:InputEvent)=>{
    if(active){stop(event);return}
    if(implicit){stop(event);defer()}
  };
  const input=(event:InputEvent)=>{
    if(active){stop(event);return}
    if(!writable()){stop(event);textarea.value='';return}
    const value=event.data??textarea.value;
    if(tail&&value===tail){stop(event);clearTimer();implicit=false;fallback='';tail='';textarea.value='';return}
    tail='';
    if(implicit||event.inputType==='insertFromComposition'||event.inputType==='insertCompositionText'||(event.inputType==='insertText'&&/[^\x00-\x7f]/.test(value))){
      stop(event);clearTimer();implicit=false;fallback='';send(value);
    }
  };
  const blur=()=>{canceled=true;active=false;implicit=false;tail='';fallback='';pendingEnter=undefined;enterOwner=undefined;clearTimer();textarea.value='';view.end()};
  const focus=()=>{canceled=false};
  const listeners:{[K in keyof HTMLElementEventMap]?: (event:any)=>void}={compositionstart:start,compositionupdate:update,compositionend:end,keydown,keypress,keyup,beforeinput,input,blur,focus};
  // xterm registers capture listeners on the textarea during open(). An ancestor
  // capture boundary must run first; registration order on the textarea cannot.
  const subscriptions=Object.entries(listeners).map(([name,listener])=>{
    const route=(event:Event)=>{if(event.target===textarea)listener(event)};
    element.addEventListener(name,route,true);return {name,route};
  });
  return {dispose(){
    disposed=true;blur();
    for(const {name,route} of subscriptions)element.removeEventListener(name,route,true);
  }};
}
