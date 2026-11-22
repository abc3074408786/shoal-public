import {Component,Suspense,lazy,useCallback,useState,type ReactNode} from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import {X} from 'lucide-react';
import {currentTarget} from '../execution-target';
import {nativePluginRequest} from '../native-plugins';
import {Button} from '../ui';
import type {ExtensionInfo,ExtensionRequest} from './types';
import './extensions.css';

const panels={ontology:lazy(()=>import('../../plugins/ontology/ui')),'skill-center':lazy(()=>import('../../plugins/skill-center/ui'))};
class ExtensionErrorBoundary extends Component<{children:ReactNode},{error:string}>{
  state={error:''};
  static getDerivedStateFromError(error:Error){return {error:error.message}}
  render(){return this.state.error?<p role="alert">插件界面暂不可用：{this.state.error}</p>:this.props.children}
}
export default function DesktopExtensions({plugin,onClose}:{plugin:ExtensionInfo;onClose:()=>void}){
  const [target]=useState(currentTarget);
  const [busy,setBusy]=useState(false),[dirty,setDirty]=useState(false),[error,setError]=useState('');
  const request=useCallback<ExtensionRequest>(<T,>(operation:string,values:Record<string,unknown>={})=>nativePluginRequest<T>({action:'call',pluginId:plugin.id,request:{operation,...values}},target),[plugin.id,target]);
  const Panel=plugin.id in panels?panels[plugin.id as keyof typeof panels]:null;
  function close(){if(busy||dirty){setError('请先保存或取消当前编辑，再关闭插件。');return}onClose()}
  return <Dialog.Root open onOpenChange={value=>{if(!value)close()}}>
    <Dialog.Portal><Dialog.Overlay className="extension-overlay"/><Dialog.Content className="extension-surface" aria-describedby="extension-scope" onKeyDownCapture={event=>{if(event.ctrlKey||event.metaKey||event.altKey)event.stopPropagation()}} onPointerDownOutside={event=>event.preventDefault()} onEscapeKeyDown={event=>{if(busy||dirty){event.preventDefault();setError('请先保存或取消当前编辑。')}}}>
      <header className="extension-heading"><div><div><Dialog.Title>{plugin.name}</Dialog.Title><p id="extension-scope">{plugin.description}</p></div></div><Button aria-label="关闭插件" disabled={busy||dirty} onClick={close}><X size={19}/></Button></header>
      {error&&<div role="alert" className="extension-error">{error}</div>}
      {Panel?<ExtensionErrorBoundary><Suspense fallback={<p role="status">正在加载插件…</p>}><Panel info={plugin} request={request} onBusyChange={setBusy} onDirtyChange={setDirty}/></Suspense></ExtensionErrorBoundary>:<p role="alert">此插件没有可打开的界面。</p>}
    </Dialog.Content></Dialog.Portal>
  </Dialog.Root>;
}
