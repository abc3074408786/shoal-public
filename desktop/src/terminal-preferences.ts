import type {ITerminalOptions} from '@xterm/xterm';

export const TERMINAL_PREFERENCES_KEY='velella:terminal-preferences:v1';
export const TERMINAL_PREFERENCES_EVENT='velella:terminal-preferences';
export type TerminalPreferences={
  fontFamily:string;fontSize:number;lineHeight:number;
  cursorStyle:'block'|'bar'|'underline';cursorBlink:boolean;
  renderer:'auto'|'dom'|'webgl';scrollSensitivity:number;fastScrollSensitivity:number;
  rightClickPaste:boolean;trimCopyGutter:boolean;
};
export const defaultTerminalPreferences:TerminalPreferences={
  fontFamily:'Cascadia Mono',fontSize:14,lineHeight:1.16,cursorStyle:'block',
  // Existing cursor behavior and explicit user preferences remain selectable.
  cursorBlink:true,renderer:'auto',scrollSensitivity:1.15,fastScrollSensitivity:5,
  rightClickPaste:true,trimCopyGutter:true,
};
export function parseTerminalPreferences(value:unknown):TerminalPreferences{
  const v=(value&&typeof value==='object'?value:{}) as Partial<TerminalPreferences>;
  const number=(n:unknown,fallback:number,min:number,max:number)=>typeof n==='number'&&Number.isFinite(n)?Math.min(max,Math.max(min,n)):fallback;
  return {...defaultTerminalPreferences,
    fontFamily:typeof v.fontFamily==='string'?v.fontFamily.replace(/["\\\x00-\x1f]/g,'').trim().slice(0,100)||'Cascadia Mono':'Cascadia Mono',
    fontSize:number(v.fontSize,14,8,40),lineHeight:number(v.lineHeight,defaultTerminalPreferences.lineHeight,1,2),
    cursorStyle:['block','bar','underline'].includes(v.cursorStyle??'')?v.cursorStyle!:'block',
    cursorBlink:typeof v.cursorBlink==='boolean'?v.cursorBlink:defaultTerminalPreferences.cursorBlink,
    renderer:['auto','dom','webgl'].includes(v.renderer??'')?v.renderer!:'auto',
    scrollSensitivity:number(v.scrollSensitivity,1.15,.1,10),fastScrollSensitivity:number(v.fastScrollSensitivity,5,1,20),
    rightClickPaste:v.rightClickPaste!==false,trimCopyGutter:v.trimCopyGutter!==false,
  };
}
export function readTerminalPreferences():TerminalPreferences{
  try{
    const raw=localStorage.getItem(TERMINAL_PREFERENCES_KEY);
    if(raw)return parseTerminalPreferences(JSON.parse(raw));
    return parseTerminalPreferences({fontSize:Number(localStorage.getItem('velella:terminal-font-size'))||14});
  }catch{return {...defaultTerminalPreferences}}
}
export function saveTerminalPreferences(value:TerminalPreferences){
  const preferences=parseTerminalPreferences(value);
  localStorage.setItem(TERMINAL_PREFERENCES_KEY,JSON.stringify(preferences));
  window.dispatchEvent(new Event(TERMINAL_PREFERENCES_EVENT));
  return preferences;
}
export function terminalFontFamily(font:string){
  const names=[font,'Cascadia Mono','Consolas','DejaVu Sans Mono','Liberation Mono','Velella Nerd Font Symbols','Symbols Nerd Font Mono','MesloLGS Nerd Font','JetBrainsMono Nerd Font','Hack Nerd Font'];
  return [...new Set(names)].map(name=>`"${name}"`).concat('monospace').join(', ');
}
export function terminalAppearance(preferences:TerminalPreferences,environment?:{platform:string;local:boolean}):ITerminalOptions{
  // ConPTY emits a positioned repaint when its grid changes. Use xterm's
  // fixed-grid compatibility profile to avoid reflowing that same old TUI
  // into scrollback. This selects behavior; it is not the detected OS build.
  const conpty=environment?.local&&/^Win/i.test(environment.platform);
  return {fontFamily:terminalFontFamily(preferences.fontFamily),fontSize:preferences.fontSize,
    fontWeight:'400',fontWeightBold:'600',lineHeight:preferences.lineHeight,
    cursorStyle:preferences.cursorStyle,cursorInactiveStyle:preferences.cursorStyle==='block'?'outline':preferences.cursorStyle,
    cursorBlink:preferences.cursorBlink,scrollSensitivity:preferences.scrollSensitivity,fastScrollSensitivity:preferences.fastScrollSensitivity,
    ...(conpty?{windowsPty:{backend:'conpty' as const,buildNumber:18309}}:{})};
}
