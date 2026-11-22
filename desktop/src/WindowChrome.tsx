import {createContext,ReactNode,useContext,useEffect,useState} from 'react';
import {createPortal} from 'react-dom';
import {getCurrentWindow} from './desktop-platform';
import {isTauri} from './api';
import {Copy,Square,Minus,X} from 'lucide-react';
import {Button,Tip} from './ui';
export const ChromeTarget=createContext<HTMLElement|null>(null);
export const WorkspaceToolsTarget=createContext<HTMLElement|null>(null);
export const ProjectContentsTarget=createContext<HTMLElement|null>(null);
export function ProjectContentsPortal({children}:{children:ReactNode}){const target=useContext(ProjectContentsTarget);return target?createPortal(children,target):null}
export function TopbarPortal({children}:{children:ReactNode}){const target=useContext(ChromeTarget);return target?createPortal(children,target):null}
export function WorkspaceToolsPortal({children}:{children:ReactNode}){const target=useContext(WorkspaceToolsTarget),fallback=useContext(ChromeTarget);return target||fallback?createPortal(children,(target||fallback)!):null}
export function WindowControls(){
  const [maximized,setMaximized]=useState(false),[error,setError]=useState('');
  useEffect(()=>{if(!isTauri())return;const win=getCurrentWindow();let disposed=false;const update=()=>{void win.isMaximized().then(value=>{if(!disposed)setMaximized(value)}).catch(e=>{if(!disposed)setError(String(e))})};update();const off=win.onResized(update);return()=>{disposed=true;void off.then(fn=>fn())}},[]);
  if(!isTauri())return <div className="window-controls" title="浏览器预览：窗口控制仅在桌面端可用"><Button disabled aria-label="最小化窗口"><Minus size={14}/></Button><Button disabled aria-label="最大化窗口"><Square size={13}/></Button><Button disabled aria-label="关闭窗口"><X size={16}/></Button></div>;
  async function action(name:'minimize'|'toggleMaximize'|'close'){try{await getCurrentWindow()[name]();setError('')}catch(e){setError(String(e))}}
  return <><div className="window-controls"><Button aria-label="最小化窗口" onClick={()=>void action('minimize')}><Minus size={14}/></Button><Button aria-label={maximized?'还原窗口':'最大化窗口'} onClick={()=>void action('toggleMaximize')}>{maximized?<Copy size={13}/>:<Square size={13}/>}</Button><Tip label="关闭窗口，任务继续运行"><Button className="window-close" aria-label="关闭窗口" onClick={()=>void action('close')}><X size={16}/></Button></Tip></div>{error&&<div className="window-error" role="alert">{error}<Button onClick={()=>setError('')}>关闭</Button></div>}</>;
}
