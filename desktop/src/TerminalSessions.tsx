import {createContext,useContext,useLayoutEffect,useRef,useState,type ComponentProps,type ReactNode} from 'react';
import {createPortal} from 'react-dom';
import TerminalView from './TerminalView';

type Props=ComponentProps<typeof TerminalView>;
type Entry={key:string;workspace:string;session:string;element:HTMLDivElement;slot:HTMLElement|null;props:Props};
type Registry={attach:(props:Props,slot:HTMLElement)=>()=>void;update:(props:Props,slot:HTMLElement)=>void;retain:(workspace:string,sessions:Set<string>)=>void};
const Sessions=createContext<Registry|null>(null);

// The portal target and React key belong to the session, not to a pane. Moving
// the target DOM node retains xterm's parser, buffer, IME and consumed offset.
export function TerminalSessionProvider({children}:{children:ReactNode}){
  const entries=useRef(new Map<string,Entry>()),[,render]=useState(0);
  const [registry]=useState<Registry>(()=>({
    attach(props,slot){
      const workspace=props.workspace?.workspaceId||props.workspace?.projectId||props.task?.id||'';
      const session=props.workspace?.id||`${props.task?.id}:${props.target||'agent'}`;
      const key=JSON.stringify([workspace,session]);
      let entry=entries.current.get(key);
      if(!entry){const element=document.createElement('div');element.className='wb-terminal';entry={key,workspace,session,element,slot:null,props};entries.current.set(key,entry)}
      entry.props=props;entry.slot=slot;if(entry.element.parentElement!==slot)slot.appendChild(entry.element);render(v=>v+1);
      return()=>{if(entry.slot!==slot)return;const focused=entry.element.ownerDocument.activeElement;if(focused instanceof HTMLElement&&entry.element.contains(focused))focused.blur();entry.slot=null;entry.element.remove();render(v=>v+1)};
    },
    update(props,slot){for(const entry of entries.current.values())if(entry.slot===slot){entry.props=props;render(v=>v+1);break}},
    retain(workspace,sessions){
      let changed=false;
      for(const [key,entry] of entries.current)if(entry.workspace===workspace&&!sessions.has(entry.session)){entry.element.remove();entries.current.delete(key);changed=true}
      if(changed)render(v=>v+1);
    },
  }));
  return <Sessions.Provider value={registry}>{children}{[...entries.current.values()].map(entry=>createPortal(<TerminalView {...entry.props} visible={entry.slot!==null}/>,entry.element,entry.key))}</Sessions.Provider>;
}

export function useRetainedTerminals(workspace:string,sessionIds:string[]|null){
  const registry=useContext(Sessions),ids=sessionIds===null?null:JSON.stringify([...new Set(sessionIds)].sort());
  useLayoutEffect(()=>{if(ids!==null)registry?.retain(workspace,new Set(JSON.parse(ids)))},[registry,workspace,ids]);
}

export default function PersistentTerminalView(props:Props){
  const registry=useContext(Sessions),slot=useRef<HTMLDivElement>(null);
  const key=JSON.stringify([props.workspace?.workspaceId,props.workspace?.projectId,props.workspace?.id,props.task?.id,props.target]);
  useLayoutEffect(()=>{if(registry&&slot.current)return registry.attach(props,slot.current)},[registry,key]);
  useLayoutEffect(()=>{if(slot.current)registry?.update(props,slot.current)});
  return registry?<div className="wb-terminal" ref={slot}/>:<TerminalView {...props}/>;
}
