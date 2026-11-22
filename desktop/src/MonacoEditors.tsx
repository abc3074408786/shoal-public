// The complete existing offline editor runtime. Import only on actual editor use.
import Editor,{DiffEditor,loader} from '@monaco-editor/react';
import * as monaco from 'monaco-editor';
import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';
import JsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker';
import CssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker';
import HtmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker';
import TsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker';
import {registerEditorThemes} from './surface-themes';
import {manageTypeScriptWorkerLifetime} from './editor-worker-lifetime';

const languageWorkers=manageTypeScriptWorkerLifetime(monaco);
import.meta.hot?.dispose(()=>languageWorkers.dispose());

(self as any).MonacoEnvironment={getWorker:(_id:string,label:string)=>{
  if(label==='json')return new JsonWorker();
  if(['css','scss','less'].includes(label))return new CssWorker();
  if(['html','handlebars','razor'].includes(label))return new HtmlWorker();
  if(label==='typescript'||label==='javascript'){
    const worker=new TsWorker();languageWorkers.created(label);return worker;
  }
  return new EditorWorker();
}};
registerEditorThemes(monaco.editor);
loader.config({monaco});

export {Editor,DiffEditor};
