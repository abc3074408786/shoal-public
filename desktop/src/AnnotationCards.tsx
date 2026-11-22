import {Button,Textarea} from './ui';
import {Annotation} from './annotation-drafts';

export default function AnnotationCards({items,workspaceId,disabled=false,onEdit,onRemove}:{items:Annotation[];workspaceId:string;disabled?:boolean;onEdit?:(id:string,text:string)=>void;onRemove?:(id:string)=>void}){
  return <ol className="annotation-cards" aria-label="网页批注列表">{items.map((item,index)=><li key={item.id} data-annotation-id={item.id}>
    <div className="annotation-card-heading"><strong>{index+1}. {item.selection.kind==='region'?'区域批注':item.selection.tag}</strong><Button type="button" onClick={()=>window.dispatchEvent(new CustomEvent('velella:open-browser',{detail:{workspaceId,page:{id:item.pageId,url:item.selection.url}}}))}>查看网页</Button>{onRemove&&<Button type="button" disabled={disabled} aria-label={`移除批注 ${index+1}`} onClick={()=>onRemove(item.id)}>移除</Button>}</div>
    <small title={item.selection.url}>{item.selection.title||item.selection.url}</small><p>{item.selection.text.slice(0,160)}</p>
    {Object.keys(item.selection.patch).length>0&&<p className="annotation-patch">{Object.entries(item.selection.patch).map(([k,v])=>`${k}: ${v}`).join(' · ')}</p>}
    {onEdit?<Textarea aria-label={`批注 ${index+1} 的要求`} rows={2} value={item.comment} maxLength={4000} disabled={disabled} onChange={e=>onEdit(item.id,e.target.value)}/>:<p>{item.comment}</p>}
  </li>)}</ol>;
}
