export const WORKBENCH_PREFERENCES_KEY='velella:workbench-preferences:v1';
export const WORKBENCH_PREFERENCES_EVENT='velella:workbench-preferences';
export type WorkbenchPreferences={
  uiScale:number;
  editor:{fontSize:number;wordWrap:'auto'|'on'|'off';minimap:boolean;autoSave:boolean;autoSaveDelay:number};
  notifications:{enabled:boolean;review:boolean;attention:boolean;delivered:boolean;backgroundOnly:boolean;sound:boolean};
  shortcuts:Partial<Record<ShortcutId,string>>;
};
export const shortcutCatalog=[
  {id:'settings',label:'打开设置',keys:'Mod+,',scope:'global'},
  {id:'sidebar',label:'显示或隐藏项目栏',keys:'Mod+Shift+B',scope:'global'},
  {id:'workspace',label:'新建工作区',keys:'Mod+Alt+N',scope:'global'},
  {id:'projects',label:'打开项目与工作区列表',keys:'Mod+Alt+P',scope:'global'},
  {id:'resources',label:'打开状态与资源',keys:'Mod+Alt+R',scope:'global'},
  {id:'restoreTab',label:'恢复关闭的标签',keys:'Ctrl+Shift+T',scope:'workbench'},
  {id:'nextTab',label:'切到下一个标签',keys:'Ctrl+Tab',scope:'workbench'},
  {id:'previousTab',label:'切到上一个标签',keys:'Ctrl+Shift+Tab',scope:'workbench'},
  {id:'saveFile',label:'保存当前文件',keys:'Mod+S',scope:'editor'},
] as const;
export type ShortcutId=typeof shortcutCatalog[number]['id'];
export const defaultWorkbenchPreferences:WorkbenchPreferences={uiScale:100,editor:{fontSize:14,wordWrap:'auto',minimap:false,autoSave:false,autoSaveDelay:1500},notifications:{enabled:false,review:true,attention:true,delivered:false,backgroundOnly:true,sound:false},shortcuts:{}};
const bounded=(value:unknown,fallback:number,min:number,max:number)=>typeof value==='number'&&Number.isFinite(value)?Math.min(max,Math.max(min,value)):fallback;
export function parseWorkbenchPreferences(value:unknown):WorkbenchPreferences{
  const v=(value&&typeof value==='object'?value:{}) as Partial<WorkbenchPreferences>;
  const e:Partial<WorkbenchPreferences['editor']>=v.editor||{},n:Partial<WorkbenchPreferences['notifications']>=v.notifications||{};
  const shortcuts:WorkbenchPreferences['shortcuts']={};
  for(const action of shortcutCatalog){const key=v.shortcuts?.[action.id];if(typeof key==='string'&&validShortcut(key)&&!reservedShortcut(key))shortcuts[action.id]=key;}
  // Corrupt or imported collisions fall back together, never make an action unreachable.
  if(shortcutConflicts(shortcuts).length)for(const id of Object.keys(shortcuts))delete shortcuts[id as ShortcutId];
  const bool=(v:unknown,d:boolean)=>typeof v==='boolean'?v:d;
  return {uiScale:bounded(v.uiScale,100,80,130),editor:{fontSize:bounded(e.fontSize,14,10,24),wordWrap:['auto','on','off'].includes(e.wordWrap||'')?e.wordWrap!:'auto',minimap:bool(e.minimap,false),autoSave:bool(e.autoSave,false),autoSaveDelay:bounded(e.autoSaveDelay,1500,500,10000)},notifications:{enabled:bool(n.enabled,false),review:bool(n.review,true),attention:bool(n.attention,true),delivered:bool(n.delivered,false),backgroundOnly:bool(n.backgroundOnly,true),sound:bool(n.sound,false)},shortcuts};
}
export function readWorkbenchPreferences():WorkbenchPreferences{try{return parseWorkbenchPreferences(JSON.parse(localStorage.getItem(WORKBENCH_PREFERENCES_KEY)||'null'))}catch{return parseWorkbenchPreferences(null)}}
type KeyEvent=Pick<KeyboardEvent,'key'|'ctrlKey'|'metaKey'|'altKey'|'shiftKey'>;
export function shortcutFromEvent(event:KeyEvent):string|null{
  if(['Control','Meta','Alt','Shift','Dead','Process','Unidentified'].includes(event.key)||(!event.ctrlKey&&!event.metaKey&&!event.altKey))return null;
  return [...(event.metaKey?['Meta']:[]),...(event.ctrlKey?['Ctrl']:[]),...(event.altKey?['Alt']:[]),...(event.shiftKey?['Shift']:[]),event.key.length===1?event.key.toUpperCase():event.key].join('+');
}
export function validShortcut(keys:string){return /^(?:(?:Mod|Ctrl|Meta|Alt|Shift)\+)+(?:[A-Z0-9,./;\[\]\\\-=`]|Tab|Enter|Backspace|Arrow(?:Up|Down|Left|Right)|F(?:[1-9]|1[0-2]))$/.test(keys)&&/(?:Mod|Ctrl|Meta|Alt)\+/.test(keys)&&keys.length<65;}
export function shortcutMatches(id:ShortcutId,event:KeyEvent,preferences:WorkbenchPreferences,mac=typeof navigator!=='undefined'&&/Mac/i.test(navigator.platform)){
  const binding=preferences.shortcuts[id]||shortcutCatalog.find(action=>action.id===id)!.keys;
  const resolved=binding.replace('Mod+',mac?'Meta+':'Ctrl+');
  return shortcutFromEvent(event)===resolved;
}
export function shortcutConflicts(overrides:WorkbenchPreferences['shortcuts']):string[]{
  const conflicts=new Set<string>();
  for(const mac of [false,true]){
    const used=new Map<string,string>();
    for(const action of shortcutCatalog){const keys=(overrides[action.id]||action.keys).replace('Mod+',mac?'Meta+':'Ctrl+');const old=used.get(keys);if(old)conflicts.add(`${old} / ${action.label}`);used.set(keys,action.label);}
  }
  return [...conflicts];
}
export function reservedShortcut(keys:string){return /^(?:Mod|Ctrl|Meta)\+(?:C|V|X|Z|A|Q|W|R|L|N|T)$/.test(keys)||/^(?:Ctrl\+Alt\+Delete|Alt\+F4|Meta\+Tab)$/.test(keys);}
