import {useEffect,useState} from 'react';
import {annotationEvent,AnnotationState,readAnnotations} from './annotation-drafts';

export function useAnnotations(workspaceId:string){
  const [state,setState]=useState<AnnotationState>({version:1,items:[],chats:{}}),[error,setError]=useState('');
  useEffect(()=>{
    const load=()=>{try{setState(readAnnotations(workspaceId));setError('')}catch(e){setError(String(e))}};
    const changed=(e:Event)=>{if((e as CustomEvent).detail?.workspaceId===workspaceId)load()};
    load();window.addEventListener(annotationEvent,changed);window.addEventListener('storage',load);
    return()=>{window.removeEventListener(annotationEvent,changed);window.removeEventListener('storage',load)};
  },[workspaceId]);
  return {state,error};
}
