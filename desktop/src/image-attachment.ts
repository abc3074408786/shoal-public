export type ClipboardPayload={kind:'text'|'image';value:string};
export function imagePaste(path:string){
  if(!path || /[\x00-\x1f\x7f]/.test(path))throw new Error('图片路径无效');
  return `\x1b[200~${path}\x1b[201~`;
}
// Bind async clipboard reads to their original viewer, never the next tab.
export async function pasteClipboard(read:()=>Promise<ClipboardPayload>,current:()=>boolean,text:(value:string)=>void|Promise<void>,image:(value:string)=>Promise<void>){
  const value=await read();
  if(!current())return;
  if(value.kind==='image')await image(imagePaste(value.value));
  else if(value.value)await text(value.value);
}
