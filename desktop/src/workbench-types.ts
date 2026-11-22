import {currentTarget,namespacedKey} from './execution-target';
export interface Session {hybrid?:boolean}

export interface Workspace {id:string;project_id:string;name:string;path:string;kind:string;archived:boolean;favorite:boolean;position:number;parent_workspace_id?:string|null}
export interface Session {id:string;title:string;kind:string;native_chat?:boolean;execution_mode?:'native'|'isolated'|'restricted';program?:string;task_id:string|null;native_id:string|null;status:{pid?:number;exit:number|null;state:string;human_control?:boolean;input_protocol?:number}|null;recoverable:boolean;auto_recoverable?:boolean;open_recoverable?:boolean;restore_selection_required?:boolean}
export type SplitAxis='horizontal'|'vertical';
export type SplitNode={kind:'leaf';id:string}|{kind:'split';id:string;axis:SplitAxis;ratio:number;first:SplitNode;second:SplitNode};
export interface Tab {id:string;kind:'terminal'|'agent'|'files'|'diff'|'browser';target:string;title:string;terminalTree?:SplitNode}
export interface Pane {id:string;active:string|null;tabs:Tab[]}
export interface Layout {revision:number;axis:SplitAxis;ratio:number;panes:Pane[];closed:Tab[];tree?:SplitNode}
export const emptyLayout=():Layout=>({revision:0,axis:'horizontal',ratio:0.5,panes:[{id:'main',active:null,tabs:[]}],closed:[]});
export const draftPrefix=(workspace:string)=>namespacedKey(currentTarget(),`velella:draft:${workspace}:`);
export const draftKey=(workspace:string,path:string)=>`${draftPrefix(workspace)}${path}`;
