import type {editor} from 'monaco-editor';
import {defaultWorkbenchPreferences,type WorkbenchPreferences} from './workbench-preferences';

export function fileEditorOptions(markdown=false,preferences:WorkbenchPreferences['editor']=defaultWorkbenchPreferences.editor):editor.IStandaloneEditorConstructionOptions {
  return {
    automaticLayout:true,minimap:{enabled:preferences.minimap},fontSize:preferences.fontSize,lineHeight:Math.round(preferences.fontSize*(markdown?27/14:24/14)),
    fontFamily:'"Cascadia Code", Consolas, "Microsoft YaHei UI", monospace',
    padding:{top:22,bottom:24},lineNumbersMinChars:3,lineDecorationsWidth:18,
    scrollBeyondLastLine:false,wordWrap:preferences.wordWrap==='auto'?(markdown?'on':'off'):preferences.wordWrap,wrappingIndent:'same',
    occurrencesHighlight:markdown?'off':'singleFile',
    renderLineHighlight:'gutter',overviewRulerBorder:false,hideCursorInOverviewRuler:true,
    scrollbar:{verticalScrollbarSize:10,horizontalScrollbarSize:10},
    // Documents contain natural-language punctuation and mathematical symbols.
    // Retain invisible-character warnings everywhere and confusable warnings in code.
    unicodeHighlight:{ambiguousCharacters:!markdown,invisibleCharacters:true,allowedLocales:{_os:true,_vscode:true,'zh-hans':true,'zh-hant':true}},
  };
}
