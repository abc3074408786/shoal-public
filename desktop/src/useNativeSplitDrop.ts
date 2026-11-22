import {useEffect,useRef,type RefObject} from 'react';

// Retained terminal content is a React portal owned outside the workbench.
// React's synthetic bubbling follows that owner, not the visible pane's DOM.
// Native listeners therefore own drops on the entire visible pane, including
// xterm, while accepting only our explicitly typed internal drag payload.
export function useNativeSplitDrop(root:RefObject<HTMLElement|null>,type:string,handlers:{over:(event:DragEvent)=>void;drop:(event:DragEvent)=>void;leave:()=>void}){
  const latest=useRef(handlers);latest.current=handlers;
  useEffect(()=>{
    const element=root.current;if(!element)return;
    const over=(e:DragEvent)=>{if(!e.dataTransfer?.types.includes(type))return;e.preventDefault();e.stopPropagation();latest.current.over(e)};
    const drop=(e:DragEvent)=>{if(!e.dataTransfer?.types.includes(type))return;e.preventDefault();e.stopPropagation();latest.current.drop(e);latest.current.leave()};
    const leave=(e:DragEvent)=>{if(!element.contains(e.relatedTarget as Node))latest.current.leave()};
    const end=()=>latest.current.leave();
    element.addEventListener('dragover',over);element.addEventListener('drop',drop);element.addEventListener('dragleave',leave);window.addEventListener('dragend',end);
    return()=>{element.removeEventListener('dragover',over);element.removeEventListener('drop',drop);element.removeEventListener('dragleave',leave);window.removeEventListener('dragend',end)};
  },[root,type]);
}

export function useNativeSplitFocus(root:RefObject<HTMLElement|null>,onActive:(event:Event)=>void,onKeyDown?:(event:KeyboardEvent)=>void){
  const latest=useRef({onActive,onKeyDown});latest.current={onActive,onKeyDown};
  useEffect(()=>{const element=root.current;if(!element)return;
    const active=(e:Event)=>latest.current.onActive(e),key=(e:KeyboardEvent)=>latest.current.onKeyDown?.(e);
    element.addEventListener('pointerdown',active,true);element.addEventListener('focusin',active,true);element.addEventListener('keydown',key,true);
    return()=>{element.removeEventListener('pointerdown',active,true);element.removeEventListener('focusin',active,true);element.removeEventListener('keydown',key,true)};
  },[root]);
}
