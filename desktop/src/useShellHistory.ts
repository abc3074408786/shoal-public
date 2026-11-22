import {useEffect,useRef,useState} from 'react';
import {moveLocation,recordLocation,type ShellHistory,type ShellLocation} from './shell-navigation';

export function useShellHistory(location:ShellLocation,scope:string,restore:(location:ShellLocation)=>void){
  const [history,setHistory]=useState<ShellHistory>(()=>({entries:[location],index:0}));
  const previousScope=useRef(scope);
  useEffect(()=>{
    if(previousScope.current!==scope){previousScope.current=scope;setHistory({entries:[location],index:0});return}
    setHistory(current=>recordLocation(current,location));
  },[location.page,scope]);
  return {canBack:history.index>0,canForward:history.index<history.entries.length-1,go(delta:number){
    const next=moveLocation(history,delta);
    if(next===history)return;
    setHistory(next);restore(next.entries[next.index]);
  }};
}
