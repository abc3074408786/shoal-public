// Real Electron clipboard roundtrip; original contents stay in memory and are restored.
const {app,clipboard,ClipboardItem,nativeImage}=require('electron');
const {readTerminalClipboard,copyTerminalText}=require('../electron/terminal-clipboard.cjs');
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict'),crypto=require('node:crypto');
const out=process.env.TERMINAL_INPUT_OUTPUT||path.resolve(__dirname,'../../artifacts/terminal-input-performance');
fs.mkdirSync(out,{recursive:true});app.setPath('userData',path.join(out,'clipboard-profile'));
async function snapshot(){
 const values=[];
 for(const item of await clipboard.read()){
  const value={};for(const type of item.types)value[type]=await item.getType(type);values.push(value);
 }
 return values;
}
async function fingerprint(values){
 const hash=crypto.createHash('sha256');
 for(const value of values)for(const type of Object.keys(value).sort()){
  hash.update(type);const data=value[type];hash.update(data instanceof Blob?Buffer.from(await data.arrayBuffer()):JSON.stringify(data));
 }
 return hash.digest('hex');
}
app.whenReady().then(async()=>{
 const result={status:'running',checks:[],originalRestored:false};let original,owned;
 try{
  original=await snapshot();
  await copyTerminalText(clipboard,'中文 clipboard\r\n第二行');owned=await fingerprint(await snapshot());
  assert.equal(await clipboard.readText(),'中文 clipboard\r\n第二行');
  assert.deepEqual(await readTerminalClipboard(clipboard,nativeImage,out),{kind:'text',value:'中文 clipboard\r\n第二行'});
  result.checks.push('real async text copy and paste with Chinese/newlines');
  const png=nativeImage.createFromBitmap(Buffer.from([0,64,255,255]),{width:1,height:1}).toPNG();
  await clipboard.write([new ClipboardItem({'image/png':new Blob([png],{type:'image/png'})})]);owned=await fingerprint(await snapshot());
  const image=await readTerminalClipboard(clipboard,nativeImage,out);
  assert.equal(image.kind,'image');assert.ok(fs.existsSync(image.value));
  assert.deepEqual(nativeImage.createFromPath(image.value).getSize(),{width:1,height:1});
  result.checks.push('real image clipboard becomes a valid PNG file');result.status='passed';
 }catch(error){result.status='failed';result.error=String(error);process.exitCode=1}
 finally{
  if(original&&owned){
   try{
    if(await fingerprint(await snapshot())===owned){
     if(original.length)await clipboard.write(original.map(value=>new ClipboardItem(value)));else clipboard.clear();
     result.originalRestored=await fingerprint(await snapshot())===await fingerprint(original);
     assert.ok(result.originalRestored,'original clipboard was not restored');
    }else result.userClipboardChanged=true;
   }catch(error){result.status='failed';result.restoreError=String(error);process.exitCode=1}
  }
  fs.writeFileSync(path.join(out,'clipboard-native.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));app.exit(process.exitCode||0);
 }
});
