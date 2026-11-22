import {Children,Fragment,isValidElement,ReactElement,ReactNode,useCallback,useState} from 'react';
import * as Menu from '@radix-ui/react-dropdown-menu';
import {Button,Tip} from './ui';

function items(children:ReactNode):ReactNode {
  return Children.map(children,child=>{
    if(!isValidElement(child))return child;
    const element=child as ReactElement<{children?:ReactNode;disabled?:boolean;className?:string}>;
    if(element.type===Fragment)return <>{items(element.props.children)}</>;
    if(element.type==='button'||element.type===Button)return <Menu.Item asChild disabled={element.props.disabled}>{element}</Menu.Item>;
    if(element.props.className?.split(' ').includes('wb-menu-label'))return <Menu.Label className={element.props.className}>{element.props.children}</Menu.Label>;
    if(element.props.className==='wb-menu-divider')return <Menu.Separator className="wb-menu-divider"/>;
    return element;
  });
}
export default function ActionMenu({label,children,icon,disabled=false,align='end'}:{label:string;children:ReactNode;icon?:ReactNode;disabled?:boolean;align?:'start'|'end'}){
  const [container,setContainer]=useState<HTMLDialogElement|null>(null);
  // Native modal dialogs make body-level portals inert; keep their menus in the same top layer.
  const bindTrigger=useCallback((button:HTMLButtonElement|null)=>setContainer(button?.closest<HTMLDialogElement>('dialog')??null),[]);
  return <Menu.Root modal={false}><Tip label={label}><Menu.Trigger asChild><Button ref={bindTrigger} aria-label={label} disabled={disabled}>{icon||label}</Button></Menu.Trigger></Tip><Menu.Portal container={container??undefined}><Menu.Content className="wb-menu-panel" align={align} sideOffset={5} collisionPadding={8} onCloseAutoFocus={e=>{const focused=document.activeElement;if(focused instanceof HTMLInputElement||focused instanceof HTMLTextAreaElement||focused?.closest('dialog[open], [role="dialog"]'))e.preventDefault()}}>{items(children)}</Menu.Content></Menu.Portal></Menu.Root>;
}
