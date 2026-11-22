import type {Terminal} from '@xterm/xterm';

type ScrollbarState={
  _refreshComputedValues:()=>void;clone:()=>ScrollbarState;isNeeded:()=>boolean;
  _computedAvailableSize:number;_arrowSize:number;_scrollSize:number;_visibleSize:number;_scrollPosition:number;
  _computedSliderSize:number;_computedSliderRatio:number;_computedSliderPosition:number;
};
const noop=()=>{};

// xterm 6 has no public maximum thumb length. Keep this adapter confined to
// its native scrollbar state, including drag snapshots, so hit testing and
// pointer-to-history mapping use the same short thumb that is rendered.
export function capScrollbarThumb(state:ScrollbarState|undefined,maxLength=80):()=>void{
  if(!state||typeof state._refreshComputedValues!=='function'||typeof state.clone!=='function'||typeof state.isNeeded!=='function')return noop;
  if(!['_computedAvailableSize','_arrowSize','_scrollSize','_visibleSize','_scrollPosition','_computedSliderSize','_computedSliderRatio','_computedSliderPosition'].every(key=>Number.isFinite((state as unknown as Record<string,unknown>)[key])))return noop;
  const maximum=Math.max(20,Math.round(maxLength)),refresh=state._refreshComputedValues,clone=state.clone;
  const update=()=>{
    refresh.call(state);
    const range=state._scrollSize-state._visibleSize;
    if(!state.isNeeded()||range<=0||state._computedSliderSize<=maximum)return;
    const track=Math.max(0,state._computedAvailableSize-2*state._arrowSize);
    state._computedSliderSize=Math.min(maximum,track);
    state._computedSliderRatio=(track-state._computedSliderSize)/range;
    state._computedSliderPosition=Math.round(state._scrollPosition*state._computedSliderRatio);
  };
  const snapshot=()=>{const copy=clone.call(state);capScrollbarThumb(copy,maximum);return copy};
  state._refreshComputedValues=update;state.clone=snapshot;update();
  return()=>{if(state._refreshComputedValues===update){state._refreshComputedValues=refresh;refresh.call(state)}if(state.clone===snapshot)state.clone=clone};
}

export function manageTerminalScrollbar(term:Terminal):{dispose:()=>void}{
  const core=(term as unknown as {_core?:{_viewport?:{_scrollableElement?:{_verticalScrollbar?:{_scrollbarState?:ScrollbarState}}}}})._core;
  return {dispose:capScrollbarThumb(core?._viewport?._scrollableElement?._verticalScrollbar?._scrollbarState)};
}
