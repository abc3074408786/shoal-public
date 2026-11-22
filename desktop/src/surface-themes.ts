import type {editor} from 'monaco-editor';
import type {ITheme} from '@xterm/xterm';
import type {Theme} from './ui-preferences';

// Orca's Builtin Tango Light reference: a complete readable ANSI palette.
// Dark uses the neutral One Dark canvas shown in the supplied Orca reference.
// TerminalView applies the same color to its padding and viewport; editor colors
// remain owned by the workbench editor palette below.
export const surfaceThemes = {
  light: {
    background:'#f1f1f1',foreground:'#2e3434',cursor:'#2e3434',cursorAccent:'#f1f1f1',selectionBackground:'#accef7',selectionForeground:'#2e3434',
    black:'#2e3436',red:'#cc0000',green:'#4e9a06',yellow:'#8e7700',blue:'#3465a4',magenta:'#75507b',cyan:'#05727e',white:'#6a6a6a',
    brightBlack:'#555753',brightRed:'#ef2929',brightGreen:'#1b7a1b',brightYellow:'#6d5a00',brightBlue:'#204a87',brightMagenta:'#ad7fa8',brightCyan:'#034b50',brightWhite:'#3d3d3d',
    scrollbarSliderBackground:'#c8c8c8',scrollbarSliderHoverBackground:'#b2b2b2',scrollbarSliderActiveBackground:'#999999',overviewRulerBorder:'#00000000',
  },
  dark: {background:'#282c34',foreground:'#abb2bf',cursor:'#abb2bf',cursorAccent:'#282c34',selectionBackground:'#3e4451',scrollbarSliderBackground:'#666666',scrollbarSliderHoverBackground:'#818181',scrollbarSliderActiveBackground:'#a1a1a1',overviewRulerBorder:'#00000000'},
} satisfies Record<Theme,ITheme>;

export function registerEditorThemes(api: {defineTheme: typeof editor.defineTheme}) {
  // Editor palettes are separate from terminal preferences. Values are the sRGB
  // equivalents of the selected A surface, text, border and mint accent tokens.
  const editors={
    dark:{background:'#0d1012',foreground:'#d6dbdf',accent:'#8cd8aa',muted:'#727b80',line:'#16191b',border:'#313336',selection:'#254a3b',widget:'#16191b'},
    light:{background:'#f1f1f1',foreground:'#262626',accent:'#3465a4',muted:'#666666',line:'#eaeaea',border:'#d3d3d3',selection:'#accef7',widget:'#f8f8f8'},
  };
  for (const mode of ['light','dark'] as const) {
    const palette=editors[mode];
    api.defineTheme(`velella-${mode}`, {
      base:mode==='dark'?'vs-dark':'vs', inherit:true, rules:[
        {token:'markup.heading',foreground:palette.accent.slice(1),fontStyle:'bold'},
        {token:'keyword.md',foreground:palette.accent.slice(1)},
      ],
      colors:{
        'editor.background':palette.background,
        'editor.foreground':palette.foreground,
        'editorCursor.foreground':palette.accent,
        'editor.selectionBackground':palette.selection,
        'editor.inactiveSelectionBackground':palette.selection+'80',
        'editor.lineHighlightBackground':palette.line,
        'editorLineNumber.foreground':palette.muted,
        'editorLineNumber.activeForeground':palette.accent,
        'editorGutter.background':palette.background,
        'editorIndentGuide.background1':palette.border,
        'editorWidget.background':palette.widget,
        'editorWidget.border':palette.border,
        'editor.findMatchBackground':palette.selection,
        'editor.findMatchBorder':palette.accent,
      },
    });
  }
}
