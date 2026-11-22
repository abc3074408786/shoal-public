export const projectIconNames=['folder','code','book','layers','rocket','package','terminal','globe'] as const;
export type ProjectIconName=typeof projectIconNames[number];
export type ProjectPresentation={icon?:ProjectIconName;group?:string;removed?:boolean};
export const workspaceStatuses=[{id:'todo',label:'待处理'},{id:'in_progress',label:'进行中'},{id:'in_review',label:'待审核'},{id:'done',label:'已完成'}] as const;
export type WorkspacePresentation={status?:typeof workspaceStatuses[number]['id'];unread?:boolean;sleeping?:boolean};
export const workspaceProperties=[{id:'status',label:'状态'},{id:'unread',label:'未读标记'},{id:'favorite',label:'固定标记'},{id:'primary',label:'主要标记'},{id:'host',label:'执行主机'},{id:'branch',label:'Git 分支'},{id:'project',label:'项目名称'},{id:'path',label:'目录路径'},{id:'sleeping',label:'休眠状态'}] as const;
export type WorkspaceProperty=typeof workspaceProperties[number]['id'];
export type WorkspaceView={project:string;groupBy:'none'|'status'|'pr'|'project';workspaceSort:'manual'|'name'|'name_desc';projectSort:'manual'|'name'|'name_desc';layout:'compact'|'detailed'|'selected';properties:WorkspaceProperty[];hideSleeping:boolean};
export function defaultWorkspaceView():WorkspaceView{return {project:'',groupBy:'project',workspaceSort:'manual',projectSort:'manual',layout:'selected',properties:['status','unread','favorite','primary','host','branch','sleeping'],hideSleeping:false}}
export function parseWorkspaceView(raw:unknown):WorkspaceView{
  const defaults=defaultWorkspaceView();if(!raw||typeof raw!=='object'||Array.isArray(raw))return defaults;
  const value=raw as Record<string,unknown>,pick=<T extends string>(key:string,choices:readonly T[],fallback:T)=>choices.includes(value[key] as T)?value[key] as T:fallback;
  return {project:typeof value.project==='string'?value.project:'',groupBy:pick('groupBy',['none','status','pr','project'],defaults.groupBy),workspaceSort:pick('workspaceSort',['manual','name','name_desc'],defaults.workspaceSort),projectSort:pick('projectSort',['manual','name','name_desc'],defaults.projectSort),layout:pick('layout',['compact','detailed','selected'],defaults.layout),properties:Array.isArray(value.properties)?workspaceProperties.filter(p=>(value.properties as unknown[]).includes(p.id)).map(p=>p.id):defaults.properties,hideSleeping:value.hideSleeping===true};
}
export type ProjectLibrary={version:1;groups:{id:string;name:string}[];projects:Record<string,ProjectPresentation>;workspaces?:Record<string,WorkspacePresentation>;view?:WorkspaceView};
export function ownsProjectLibrary(snapshotScope:string,boundScope:string,currentScope:string){return snapshotScope===boundScope&&boundScope===currentScope}
export function parseProjectLibrary(raw:string|null):ProjectLibrary{
  const empty:ProjectLibrary={version:1,groups:[],projects:{}};
  try{
    const value=JSON.parse(raw||'null');if(!value||value.version!==1)return empty;
    const ids=new Set<string>();
    const groups:Array<{id:string;name:string}>=Array.isArray(value.groups)?value.groups.flatMap((g:unknown)=>{
      if(!g||typeof g!=='object')return [];const {id,name}=g as Record<string,unknown>;
      if(typeof id!=='string'||!id||ids.has(id)||typeof name!=='string'||!name.trim())return [];
      ids.add(id);return [{id,name:name.trim().slice(0,64)}];
    }):[];
    const projects=Object.fromEntries(value.projects&&typeof value.projects==='object'&&!Array.isArray(value.projects)?Object.entries(value.projects).flatMap(([id,p])=>{
      if(!p||typeof p!=='object')return [];const item=p as ProjectPresentation;
      return [[id,{...(projectIconNames.includes(item.icon!)?{icon:item.icon}:{}),...(typeof item.group==='string'&&ids.has(item.group)?{group:item.group}:{}),...(item.removed===true?{removed:true}:{})}]];
    }):[]);
    const workspaces=Object.fromEntries(value.workspaces&&typeof value.workspaces==='object'&&!Array.isArray(value.workspaces)?Object.entries(value.workspaces).flatMap(([id,w])=>{
      if(!w||typeof w!=='object')return [];const item=w as WorkspacePresentation;
      return [[id,{...(workspaceStatuses.some(s=>s.id===item.status)?{status:item.status}:{}),...(item.unread===true?{unread:true}:{}),...(item.sleeping===true?{sleeping:true}:{})}]];
    }):[]);
    return {version:1,groups,projects,...(Object.keys(workspaces).length?{workspaces}:{}),...(value.view?{view:parseWorkspaceView(value.view)}:{})};
  }catch{return empty}
}
export function editProject(state:ProjectLibrary,id:string,patch:Partial<ProjectPresentation>):ProjectLibrary{return {...state,projects:{...state.projects,[id]:{...state.projects[id],...patch}}}}
export function deleteProjectGroup(state:ProjectLibrary,id:string):ProjectLibrary{return {...state,groups:state.groups.filter(g=>g.id!==id),projects:Object.fromEntries(Object.entries(state.projects).map(([key,p])=>[key,p.group===id?{...p,group:undefined}:p]))}}
export function commitProjectLibrary(storage:Pick<Storage,'getItem'|'setItem'>,key:string,change:(current:ProjectLibrary)=>ProjectLibrary,ownsSnapshot:()=>boolean){
  if(!ownsSnapshot())throw Error('执行主机已切换，或项目列表尚未加载，请稍后重试。');
  const next=parseProjectLibrary(JSON.stringify(change(parseProjectLibrary(storage.getItem(key)))));
  if(!ownsSnapshot())throw Error('执行主机已切换，未保存项目列表设置。');
  storage.setItem(key,JSON.stringify(next));return next;
}
