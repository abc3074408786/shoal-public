import {useEffect,useRef,useState,type CSSProperties,type ReactNode} from 'react';
import type {SplitNode} from './workbench-types';
import {resizeSplit} from './split-layout';
import './split-surface.css';

type Rect={x:number;y:number;width:number;height:number};
type Divider={node:Extract<SplitNode,{kind:'split'}>;rect:Rect};
function project(tree:SplitNode,rect:Rect,leaves:{id:string;rect:Rect}[],dividers:Divider[]){
  if(tree.kind==='leaf'){leaves.push({id:tree.id,rect});return;}
  dividers.push({node:tree,rect});
  if(tree.axis==='horizontal'){
    project(tree.first,{...rect,width:rect.width*tree.ratio},leaves,dividers);
    project(tree.second,{...rect,x:rect.x+rect.width*tree.ratio,width:rect.width*(1-tree.ratio)},leaves,dividers);
  }else{
    project(tree.first,{...rect,height:rect.height*tree.ratio},leaves,dividers);
    project(tree.second,{...rect,y:rect.y+rect.height*tree.ratio,height:rect.height*(1-tree.ratio)},leaves,dividers);
  }
}
const position=(r:Rect):CSSProperties=>({insetInlineStart:`${r.x*100}%`,insetBlockStart:`${r.y*100}%`,inlineSize:`${r.width*100}%`,blockSize:`${r.height*100}%`});

// Leaves stay siblings with stable keys while the tree changes: editors and
// terminal slots keep their DOM identity through splits, moves and resizing.
export default function SplitSurface({tree,renderLeaf,onResize,disabled=false,maximized,label='分屏',className=''}:{tree:SplitNode;renderLeaf:(id:string)=>ReactNode;onResize:(id:string,ratio:number)=>void;disabled?:boolean;maximized?:string;label?:string;className?:string}){
  const root=useRef<HTMLDivElement>(null),drag=useRef<{id:string;ratio:number;pointer:number}|null>(null);
  const [preview,setPreview]=useState<{id:string;ratio:number}|null>(null);
  useEffect(()=>{drag.current=null;setPreview(null)},[tree]);
  const shown=preview?resizeSplit(tree,preview.id,preview.ratio):tree;
  const leaves:{id:string;rect:Rect}[]=[],dividers:Divider[]=[];
  project(shown,{x:0,y:0,width:1,height:1},leaves,dividers);
  const zoom=maximized&&leaves.some(l=>l.id===maximized)?maximized:undefined;
  return <div ref={root} className={`split-surface ${leaves.length>1?'has-splits':''} ${className}`}>
    {leaves.map(l=><div key={l.id} className="split-cell" data-split-leaf={l.id} style={{...position(zoom?{x:0,y:0,width:1,height:1}:l.rect),display:zoom&&l.id!==zoom?'none':undefined}}>{renderLeaf(l.id)}</div>)}
    {!zoom&&dividers.map(({node,rect})=>{
      const horizontal=node.axis==='horizontal';
      const style:CSSProperties=horizontal?{insetInlineStart:`calc(${(rect.x+rect.width*node.ratio)*100}% - 4px)`,insetBlockStart:`${rect.y*100}%`,inlineSize:8,blockSize:`${rect.height*100}%`}:{insetInlineStart:`${rect.x*100}%`,insetBlockStart:`calc(${(rect.y+rect.height*node.ratio)*100}% - 4px)`,inlineSize:`${rect.width*100}%`,blockSize:8};
      return <div key={node.id} className={`split-divider ${horizontal?'horizontal':'vertical'}`} style={style} role="separator" tabIndex={disabled?-1:0} aria-label={`${label}分界线`} aria-orientation={horizontal?'vertical':'horizontal'} aria-valuemin={15} aria-valuemax={85} aria-valuenow={Math.round(node.ratio*100)} aria-disabled={disabled} title="拖动调整大小 · 双击均分 · 方向键微调"
        onPointerDown={e=>{if(disabled||e.button!==0)return;e.preventDefault();e.stopPropagation();e.currentTarget.setPointerCapture(e.pointerId);drag.current={id:node.id,ratio:node.ratio,pointer:e.pointerId};}}
        onPointerMove={e=>{if(drag.current?.id!==node.id||drag.current.pointer!==e.pointerId||!root.current)return;const box=root.current.getBoundingClientRect();const extent=horizontal?box.width*rect.width:box.height*rect.height;const offset=horizontal?e.clientX-box.left-box.width*rect.x:e.clientY-box.top-box.height*rect.y;const minimum=Math.min(.45,Math.max(.15,(horizontal?140:100)/extent));const ratio=Math.max(minimum,Math.min(1-minimum,offset/extent));drag.current.ratio=ratio;setPreview({id:node.id,ratio});}}
        onPointerUp={e=>{if(drag.current?.id!==node.id)return;const {ratio}=drag.current;drag.current=null;setPreview(null);e.currentTarget.releasePointerCapture(e.pointerId);onResize(node.id,ratio);}}
        onPointerCancel={()=>{drag.current=null;setPreview(null)}} onLostPointerCapture={()=>{drag.current=null;setPreview(null)}}
        onDoubleClick={()=>{if(!disabled)onResize(node.id,.5)}}
        onKeyDown={e=>{if(e.key==='Escape'){drag.current=null;setPreview(null);return;}if(disabled)return;const minus=horizontal?'ArrowLeft':'ArrowUp',plus=horizontal?'ArrowRight':'ArrowDown';const ratio=e.key===minus?node.ratio-.05:e.key===plus?node.ratio+.05:e.key==='Home'?.15:e.key==='End'?.85:undefined;if(ratio===undefined)return;e.preventDefault();e.stopPropagation();onResize(node.id,Math.max(.15,Math.min(.85,ratio)));}}/>;
    })}
  </div>;
}
