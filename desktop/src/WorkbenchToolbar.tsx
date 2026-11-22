import {Children,isValidElement,useEffect,useRef,type ReactNode} from 'react';
import {MoreHorizontal} from 'lucide-react';

function primary(item:ReactNode){
  if(!isValidElement(item))return false;
  const props=item.props as {label?:string;children?:ReactNode};
  if(props.label==='新建标签'||props.label==='会话记录')return true;
  return isValidElement(props.children)&&String((props.children.props as {className?:string}).className).includes('workspace-tools-toggle');
}

export default function WorkbenchToolbar({compact,hasTabs,children}:{compact:boolean;hasTabs:boolean;children:ReactNode}){
  const details=useRef<HTMLDetailsElement>(null);
  useEffect(()=>{
    if(!compact)return;
    function outside(event:PointerEvent){const target=event.target;if(target instanceof Element&&!details.current?.contains(target)&&!target.closest('.wb-menu-panel,dialog,[role="dialog"]'))details.current?.removeAttribute('open')}
    function escape(event:KeyboardEvent){if(event.key==='Escape'&&details.current?.open&&!document.querySelector('[role="menu"],dialog[open]')){details.current.removeAttribute('open');details.current.querySelector('summary')?.focus()}}
    document.addEventListener('pointerdown',outside);document.addEventListener('keydown',escape);
    return()=>{document.removeEventListener('pointerdown',outside);document.removeEventListener('keydown',escape)};
  },[compact]);
  const items=Children.toArray(children);
  return <div className={'chrome-actions '+(hasTabs?'has-open-tabs':'empty-tools')+(compact?' compact-workbench-toolbar':'')}>
    {compact?<>{items.filter(primary)}<details className="workbench-toolbar-overflow" ref={details}><summary aria-label="更多项目工具" title="更多项目工具"><MoreHorizontal size={16}/></summary><div className="workbench-toolbar-panel" onClick={event=>{const button=(event.target as Element).closest('button');if(button&&!button.disabled&&!button.hasAttribute('aria-haspopup'))details.current?.removeAttribute('open')}}>{items.filter(item=>!primary(item))}</div></details></>:children}
  </div>;
}
