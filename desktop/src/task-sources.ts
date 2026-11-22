import {invoke} from './desktop-platform';
import {isTauri} from './api';
export const providers={github:'GitHub',gitlab:'GitLab',linear:'Linear',jira:'Jira'};
export type Provider=keyof typeof providers;
export interface TaskSource {id:string;project_id:string;provider:Provider;name:string;base_url:string;scope:string;login:string;jira_kind:string;auth_mode?:string}
export interface SourceItemMetadata {assignees?:string[];creator?:string;priority?:string;labels?:string[];project?:string;milestone?:string;created_at?:string;due_at?:string}
export interface SourceItem {key:string;source_id:string;project_id:string;provider:Provider;remote_id:string;identifier:string;title:string;url:string;description:string;state:string;closed:boolean;updated_at:string;scope?:string;metadata?:SourceItemMetadata}
export interface RepositorySource {project_id:string;project_name:string;provider:Provider|'';host:string;scope:string}
export interface SourceLink extends Pick<SourceItem,'key'|'source_id'|'project_id'|'provider'|'identifier'|'title'|'url'> {workspace_id:string}
export const taskPrompt=(item:SourceItem)=>`${item.identifier} ${item.title}\n来源：${item.url}\n\n${item.description}`;
export const taskInstruction=(item:SourceItem)=>`Complete ${item.url}\n\n${taskPrompt(item)}`.replace(/[\u0000-\u0008\u000b-\u001f\u007f]/g,'').slice(0,12000);
export async function openSourceUrl(url:string){
  const parsed=new URL(url);
  if(!['https:','http:'].includes(parsed.protocol)||parsed.username||parsed.password)throw new Error('任务链接无效');
  if(isTauri())await invoke('terminal_open_link',{url});else window.open(url,'_blank','noopener,noreferrer');
}
export function sourceHome(source:TaskSource){
  if(source.provider==='linear')return 'https://linear.app';
  if(source.provider==='jira')return `${source.base_url}/issues/?jql=${encodeURIComponent(`project = "${source.scope}"`)}`;
  const site=source.base_url.replace('https://api.github.com','https://github.com').replace(/\/api\/v[34]$/,'');
  return `${site}/${source.scope}/${source.provider==='gitlab'?'-/issues':'issues'}`;
}
