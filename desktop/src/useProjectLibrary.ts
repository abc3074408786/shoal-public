import {useEffect,useState} from 'react';
import {commitProjectLibrary,deleteProjectGroup,editProject,parseProjectLibrary,defaultWorkspaceView,type WorkspaceView,type ProjectLibrary,type ProjectPresentation,type WorkspacePresentation} from './project-library';

/** Presentation preferences only; a removed list entry never deletes runtime data. */
export function useProjectLibrary(key:string,onError:(message:string)=>void,ownsSnapshot:()=>boolean){
  function read(){try{return parseProjectLibrary(localStorage.getItem(key))}catch{return parseProjectLibrary(null)}}
  const [cache,setCache]=useState(()=>({key,state:read()}));
  const state=cache.key===key?cache.state:read();
  useEffect(()=>{setCache({key,state:read()});const update=(e:StorageEvent)=>{if(e.key===key||e.key===null)setCache({key,state:read()})};window.addEventListener('storage',update);return()=>window.removeEventListener('storage',update)},[key]);
  function update(change:(current:ProjectLibrary)=>ProjectLibrary){
    try{const next=commitProjectLibrary(localStorage,key,change,ownsSnapshot);setCache({key,state:next});return true}
    catch(e){onError('无法保存项目列表设置：'+String(e));return false}
  }
  function edit(id:string,patch:Partial<ProjectPresentation>){return update(current=>editProject(current,id,patch))}
  return {state,edit,restore:(id:string)=>edit(id,{removed:false}),
    editView:(patch:Partial<WorkspaceView>)=>update(current=>({...current,view:{...(current.view||defaultWorkspaceView()),...patch}})),
    editWorkspace:(id:string,patch:Partial<WorkspacePresentation>)=>update(current=>({...current,workspaces:{...current.workspaces,[id]:{...current.workspaces?.[id],...patch}}})),
    createGroup:(name:string,projectId:string)=>update(current=>{const id=crypto.randomUUID();return editProject({...current,groups:[...current.groups,{id,name}]},projectId,{group:id})}),
    renameGroup:(id:string,name:string)=>update(current=>({...current,groups:current.groups.map(g=>g.id===id?{...g,name}:g)})),
    deleteGroup:(id:string)=>update(current=>deleteProjectGroup(current,id))};
}
export type ProjectLibraryController=ReturnType<typeof useProjectLibrary>;
