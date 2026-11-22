import {invoke,isTauri} from './desktop-platform';

/** App UI copy actions use the same native transport as terminal selection. */
export async function copyText(text:string):Promise<void>{
  const native=isTauri();
  try{
    if(native){await invoke('terminal_copy',{text});return;}
    if(typeof navigator==='undefined'||!navigator.clipboard?.writeText)throw new Error('Clipboard API unavailable');
    await navigator.clipboard.writeText(text);
  }catch(cause){
    throw new Error(native
      ?'复制失败：系统剪贴板暂时不可用，请稍后重试。'
      :'当前页面无法写入剪贴板，请在桌面 Velella 中复制，或手动选择文字复制。',{cause});
  }
}
