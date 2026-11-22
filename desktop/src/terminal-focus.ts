import type {Terminal} from '@xterm/xterm';

export function manageTerminalFocus(term:Terminal){
  const element=term.element!;
  let blink=term.options.cursorBlink===true,disposed=false;
  type CursorModes={cursorBlink:boolean|undefined};
  const core=(term as unknown as {_core?:{coreService?:{decPrivateModes:CursorModes}}})._core?.coreService;
  let modes=core?.decPrivateModes,applicationBlink=modes?.cursorBlink,appliedBlink=applicationBlink;
  const cursorStyle=term.parser.registerCsiHandler({intermediates:' ',final:'q'},params=>{
    const value=params.length===0?1:Number(params[0]);
    if(value>=0&&value<=6)applicationBlink=value===0?undefined:value%2===1;
    return false;
  });
  const sync=()=>{
    if(disposed)return;
    element.querySelector('.xterm-rows')?.classList.toggle('xterm-focus',element.classList.contains('focus'));
    const visible=document.visibilityState==='visible'&&element.getClientRects().length>0;
    // DECSCUSR can override the public blink option. Park only that field,
    // retaining the application's request so reveal/re-enabling restores it.
    if(core){
      if(modes!==core.decPrivateModes||core.decPrivateModes.cursorBlink!==appliedBlink){modes=core.decPrivateModes;applicationBlink=modes.cursorBlink}
      appliedBlink=visible&&blink?applicationBlink:false;
      core.decPrivateModes.cursorBlink=appliedBlink;
    }
    // WebGL rebuilds its blink timer on public option changes, not on DEC
    // mode changes alone. Keep the option aligned with the resolved request.
    term.options.cursorBlink=visible&&blink?(applicationBlink??blink):false;
  };
  const parsed=term.onWriteParsed(sync);
  const mutation=new MutationObserver(sync);mutation.observe(element,{attributes:true,attributeFilter:['class']});
  const visibility=new IntersectionObserver(sync);visibility.observe(element);
  element.addEventListener('focusin',sync);element.addEventListener('focusout',sync);document.addEventListener('visibilitychange',sync);
  sync();
  return {setBlink(value:boolean){blink=value;sync();term.refresh(0,term.rows-1)},dispose(){disposed=true;cursorStyle.dispose();parsed.dispose();mutation.disconnect();visibility.disconnect();element.removeEventListener('focusin',sync);element.removeEventListener('focusout',sync);document.removeEventListener('visibilitychange',sync)}};
}
