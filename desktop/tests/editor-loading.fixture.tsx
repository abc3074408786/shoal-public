import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import Editor,{DiffEditor} from '../src/LazyEditor';
import WorkbenchFile from '../src/WorkbenchFile';
import FilesView from '../src/FilesView';
import {AppearanceProvider,useAppearance} from '../src/Appearance';
import {ConfirmationProvider} from '../src/ConfirmAction';
import {Button,TooltipProvider} from '../src/ui';
import {draftKey} from '../src/workbench-types';
import type {Task} from '../src/api';
import '../src/style.css';import '../src/theme.css';import '../src/workspace.css';import '../src/editor-a.css';
import './editor-loading.fixture.css';

// Synthetic file service only, never a real runtime, account or model.
const fixture=(window as any).__editorFixture={files:{'note.ts':'const answer = 42;\n','preview.md':'# Fixture document\n\nRead before editing.\n','task.ts':'const changed = 2;\n'},writes:0,requests:[],failSave:false,editor:null,monaco:null,diff:null,diffModels:null,keepDiff:false,draftKey:draftKey('editor-fixture','note.ts')};
window.velellaDesktop={invoke:async(command,args:any)=>{
  if(command==='window')return false as any;
  if(command!=='rpc')throw Error('Unexpected synthetic command '+command);
  const request=args.request;fixture.requests.push(request);
  if(request.cmd==='task.files')return {ok:true,files:['task.ts']} as any;
  if(request.cmd==='workbench.file.read'||request.cmd==='file.read')return {ok:true,content:fixture.files[request.path],original:'const changed = 1;\n'} as any;
  if(request.cmd==='workbench.file.write'||request.cmd==='file.write'){
    fixture.writes++;
    if(fixture.failSave||request.previous!==fixture.files[request.path])return {ok:false,error:'合成文件冲突：磁盘版本已变化'} as any;
    fixture.files[request.path]=request.content;return {ok:true} as any;
  }
  throw Error('Unexpected synthetic RPC '+request.cmd);
},listen:()=>()=>{}};
const task={id:'editor-task',direct:false,phase:'running'} as Task;
function App(){
  const [mode,setMode]=useState('home'),[text,setText]=useState('const value: number = "wrong";\n');
  const {theme,setTheme}=useAppearance();
  return <div className="ide-shell editor-fixture">
    <nav aria-label="验证模式"><Button onClick={()=>setMode('home')}>返回首页</Button><Button onClick={()=>setMode('code')}>打开代码</Button><Button onClick={()=>setMode('markdown')}>打开文档</Button><Button onClick={()=>setMode('direct')}>语言与主题</Button><Button onClick={()=>setMode('diff')}>打开差异</Button><Button onClick={()=>setMode('task')}>任务文件</Button><Button onClick={()=>setTheme(theme==='dark'?'light':'dark')}>切换主题</Button></nav>
    {mode==='home'?<p role="status">尚未打开编辑器</p>:mode==='code'?<WorkbenchFile key="code" workspaceId="editor-fixture" path="note.ts"/>:mode==='markdown'?<WorkbenchFile key="markdown" workspaceId="editor-fixture" path="preview.md"/>:mode==='task'?<FilesView task={task}/>:mode==='diff'?<DiffEditor theme={`velella-${theme}`} original="before" modified="after" keepCurrentOriginalModel={fixture.keepDiff} keepCurrentModifiedModel={fixture.keepDiff} options={{readOnly:true}} onMount={(editor,monaco)=>{fixture.diff=editor;fixture.diffModels=editor.getModel();fixture.monaco=monaco}}/>:<Editor path="file:///fixture/types.ts" language="typescript" value={text} theme={`velella-${theme}`} onMount={(editor,monaco)=>{fixture.editor=editor;fixture.monaco=monaco}} onChange={value=>setText(value??'')}/>}
  </div>;
}
createRoot(document.getElementById('root')!).render(<AppearanceProvider><TooltipProvider><ConfirmationProvider><App/></ConfirmationProvider></TooltipProvider></AppearanceProvider>);
