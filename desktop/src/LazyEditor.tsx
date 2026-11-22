import {Component,lazy,Suspense,useEffect,useRef,type ReactNode} from 'react';
import type {EditorProps,DiffEditorProps} from '@monaco-editor/react';
import LoadingState from './LoadingState';
import {Button} from './ui';

const SourceEditor=lazy(()=>import('./MonacoEditors').then(module=>({default:module.Editor})));
const SourceDiffEditor=lazy(()=>import('./MonacoEditors').then(module=>({default:module.DiffEditor})));

class EditorLoadBoundary extends Component<{children:ReactNode},{failed:boolean}>{
  state={failed:false};
  static getDerivedStateFromError(){return {failed:true}}
  render(){
    if(this.state.failed)return <div className="empty small"><p role="alert">编辑器未能打开。请重新加载工作台后重试。</p><Button onClick={()=>window.location.reload()}>重新加载工作台</Button></div>;
    return this.props.children;
  }
}

export default function Editor(props:EditorProps){
  return <EditorLoadBoundary><Suspense fallback={props.loading??<LoadingState label="正在打开文件"/>}><SourceEditor {...props}/></Suspense></EditorLoadBoundary>;
}
export function DiffEditor(props:DiffEditorProps){
  const instance=useRef<Parameters<NonNullable<DiffEditorProps['onMount']>>[0]|null>(null);
  const current=useRef(props);current.current=props;
  useEffect(()=>()=>{
    const editor=instance.current,models=editor?.getModel();
    // The React adapter disposes models before its diff widget. Detach first
    // so Monaco does not observe a disposed model in a still-mounted widget.
    editor?.setModel(null);
    if(!current.current.keepCurrentOriginalModel)models?.original.dispose();
    if(!current.current.keepCurrentModifiedModel)models?.modified.dispose();
    instance.current=null;
  },[]);
  return <EditorLoadBoundary><Suspense fallback={props.loading??<LoadingState label="正在打开差异"/>}><SourceDiffEditor {...props} onMount={(editor,monaco)=>{instance.current=editor;current.current.onMount?.(editor,monaco)}}/></Suspense></EditorLoadBoundary>;
}
