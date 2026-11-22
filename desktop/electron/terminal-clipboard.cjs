const {mkdir,writeFile}=require('node:fs/promises');
const {join}=require('node:path');
const {randomUUID}=require('node:crypto');
const MAX_TEXT=1024*1024,MAX_IMAGE=32*1024*1024;

async function copyTerminalText(clipboard,text){
  if(typeof text!=='string'||Buffer.byteLength(text)>MAX_TEXT)throw Error('Clipboard text too large');
  await clipboard.writeText(text);
}

async function readTerminalClipboard(clipboard,nativeImage,root){
  let png,text='';
  if(typeof clipboard.readImage==='function'){
    // Electron <=43. Await also supports implementations returning promises.
    const image=await clipboard.readImage();
    if(!image.isEmpty())png=image.toPNG();
    else text=await clipboard.readText();
  }else{
    // Electron 44+: read once so image/text belong to the same clipboard snapshot.
    const items=await clipboard.read();
    if(!Array.isArray(items))throw Error('Clipboard API is unavailable');
    const imageTypes=['image/png','image/jpeg','image/webp'];
    const item=items.find(item=>imageTypes.some(type=>item.types.includes(type)));
    if(item){
      const type=imageTypes.find(type=>item.types.includes(type));
      const blob=await item.getType(type);
      if(blob.size>MAX_IMAGE)throw Error('剪贴板图片超过 32 MB。');
      const image=nativeImage.createFromBuffer(Buffer.from(await blob.arrayBuffer()));
      if(image.isEmpty())throw Error('无法读取剪贴板图片。');
      png=image.toPNG();
    }else{
      const item=items.find(item=>item.types.includes('text/plain'));
      if(item){
        const blob=await item.getType('text/plain');
        if(blob.size>MAX_TEXT)throw Error('Clipboard text too large');
        text=await blob.text();
      }
    }
  }
  if(png){
    if(png.length>MAX_IMAGE)throw Error('剪贴板图片超过 32 MB。');
    const directory=join(root,'clipboard');await mkdir(directory,{recursive:true,mode:0o700});
    const file=join(directory,randomUUID()+'.png');await writeFile(file,png,{flag:'wx',mode:0o600});
    return {kind:'image',value:file};
  }
  if(typeof text!=='string'||Buffer.byteLength(text)>MAX_TEXT)throw Error('Clipboard text too large');
  return {kind:'text',value:text};
}
module.exports={copyTerminalText,readTerminalClipboard};
