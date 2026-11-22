import type {ITerminalOptions,Terminal} from '@xterm/xterm';

export type TerminalKeyboardEnvironment={platform:string;local:boolean};
type Key=Pick<KeyboardEvent,'key'|'code'|'keyCode'|'shiftKey'|'ctrlKey'|'altKey'|'metaKey'>;
export const isWindowsConpty=(environment:TerminalKeyboardEnvironment)=>environment.local&&/^Win/i.test(environment.platform);

// Local ConPTY translates legacy VT keys to Windows console key records. Do
// not advertise CSI-u there; SSH and Unix terminals keep native negotiation.
export function terminalKeyboardOptions(environment:TerminalKeyboardEnvironment):Pick<ITerminalOptions,'vtExtensions'>{
 return {vtExtensions:{kittyKeyboard:!isWindowsConpty(environment)}};
}
export function terminalKeyboardFlags(term:Terminal){
 const flags=(term as unknown as {_core?:{coreService?:{kittyKeyboard?:{flags:number}}}})._core?.coreService?.kittyKeyboard?.flags;
 return typeof flags==='number'&&Number.isInteger(flags)&&flags>=0&&flags<=31?flags:0;
}
export function terminalModifiedEnter(event:Key,flags:number,environment:TerminalKeyboardEnvironment):string|undefined{
 const enter=event.key==='Enter'||(event.keyCode===229&&['Enter','NumpadEnter'].includes(event.code));
 if(!enter||event.altKey||event.metaKey)return;
 if(event.shiftKey&&!event.ctrlKey)return flags>0?'\x1b[13;2u':'\x1b\r';
 if(event.ctrlKey&&!event.shiftKey)return !isWindowsConpty(environment)||flags>0?'\x1b[13;5u':'\r';
}
export function terminalEditingInput(event:Key,flags:number,environment:TerminalKeyboardEnvironment):string|undefined{
 if(event.ctrlKey&&!event.shiftKey&&!event.altKey&&!event.metaKey&&event.key==='Backspace')return '\x17';
 return terminalModifiedEnter(event,flags,environment);
}
