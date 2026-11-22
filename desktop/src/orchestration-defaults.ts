import type {AgentPreferences} from './agent-preferences';

export function defaultOrchestrationSelection(preferences:AgentPreferences,providers:readonly {id:string}[]){
  return {provider:preferences.orchestration?.provider||(providers.some(provider=>provider.id==='codex')?'codex':providers[0]?.id||'codex'),templateId:preferences.orchestration?.templateId||''};
}
export function orchestrationSelectionError(selection:{provider:string;templateId:string},providers:readonly {id:string}[],templates:readonly {id:string}[]){
  if(!providers.some(provider=>provider.id===selection.provider))return '默认主控在当前主机不可用，请重新选择主控 CLI。';
  if(selection.templateId&&!templates.some(template=>template.id===selection.templateId))return '所选模板已不可用，请重新选择模板或明确选择“按目标生成”。';
  return '';
}
