import {ChevronRight,FileText,Folder} from 'lucide-react';

type Node={name:string;path:string;children?:Map<string,Node>};
export default function FileTree({files,selected,disabled,onOpen,expanded=false}:{files:string[];selected?:string;disabled:boolean;onOpen:(path:string)=>void;expanded?:boolean}) {
  const root=new Map<string,Node>();
  for(const path of files){
    const parts=path.replaceAll('\\','/').split('/');
    let level=root;
    parts.forEach((name,index)=>{
      if(!level.has(name))level.set(name,{name,path:parts.slice(0,index+1).join('/'),...(index<parts.length-1?{children:new Map()}: {})});
      const node=level.get(name)!;
      if(node.children)level=node.children;
      else node.path=path;
    });
  }
  function nodes(level:Map<string,Node>){return [...level.values()].sort((a,b)=>Number(!!b.children)-Number(!!a.children)||a.name.localeCompare(b.name)).map(node=>node.children?
    <details className="wb-folder" key={node.path} open={expanded||undefined}>
      <summary title={node.path}><ChevronRight size={13} className="wb-chevron"/><Folder size={14}/><span>{node.name}</span></summary>
      <div className="wb-folder-children">{nodes(node.children)}</div>
    </details>:
    <button key={node.path} className={`wb-file-row ${selected===node.path?'selected':''}`} title={node.path} aria-current={selected===node.path?'page':undefined} disabled={disabled} onClick={()=>onOpen(node.path)}><FileText size={14}/><span>{node.name}</span></button>);
  }
  return <div className="wb-file-tree">{nodes(root)}</div>;
}
