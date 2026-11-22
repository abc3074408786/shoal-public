import {SourceItem} from './task-sources';

export function sourceDate(value?:string){
  if(!value)return '';
  if(/^\d{4}-\d{2}-\d{2}$/.test(value))return value;
  const date=new Date(value);
  return Number.isNaN(date.getTime())?'':date.toLocaleString('zh-CN',{year:'numeric',month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',hour12:false});
}

export default function SourceTaskInfo({item}:{item:SourceItem}){
  const m=item.metadata;
  const rows:[string,string|undefined][]=[
    ['负责人',m?.assignees?.join('、')],
    [item.provider==='jira'?'报告人':'创建人',m?.creator],
    ['来源项目',m?.project],
    ...(item.provider==='linear'||item.provider==='jira'?[['优先级',m?.priority] as [string,string|undefined]]:[]),
    [item.provider==='linear'?'迭代':item.provider==='jira'?'修复版本':'里程碑',m?.milestone],
    ['创建时间',sourceDate(m?.created_at)],
    ['更新时间',sourceDate(item.updated_at)],
    ...(item.provider!=='github'?[['截止日期',sourceDate(m?.due_at)] as [string,string]]:[]),
  ];
  return <dl className="source-task-info" aria-label="任务信息">
    {rows.map(([label,value])=><div key={label}><dt>{label}</dt><dd>{value||'未提供'}</dd></div>)}
    <div className="source-task-labels"><dt>标签</dt><dd>{m?.labels?.length?m.labels.map((label,index)=><span className="source-task-label" key={label+index}>{label}</span>):'未提供'}</dd></div>
  </dl>;
}
