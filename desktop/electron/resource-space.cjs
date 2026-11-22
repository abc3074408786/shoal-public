const fs=require('node:fs/promises'),path=require('node:path'),crypto=require('node:crypto');
const CACHE_PATHS=['node_modules/.cache','.vite','.turbo','.parcel-cache'];
const inside=(root,file)=>file===root||file.startsWith(root+path.sep);
async function scanDirectory(root,{maxEntries=100000,deadline=Date.now()+12000}={}){
 const hash=crypto.createHash('sha256'),stack=[root];let bytes=0,files=0,entries=0,skipped=0,partial=false;
 while(stack.length){
  if(entries>=maxEntries||Date.now()>deadline){partial=true;break}
  const file=stack.pop();let stat;try{stat=await fs.lstat(file)}catch{skipped++;partial=true;continue}
  entries++;if(stat.isSymbolicLink()){skipped++;partial=true;continue}
  // Reparse/junction paths are not followed; verify every resolved entry remains in scope.
  let real;try{real=await fs.realpath(file)}catch{partial=true;skipped++;continue}if(!inside(root,real)){skipped++;partial=true;continue}
  hash.update(JSON.stringify([path.relative(root,file),stat.size,stat.mtimeMs,stat.ctimeMs,stat.ino]));
  if(stat.isDirectory()){try{const names=(await fs.readdir(file)).sort();for(let i=names.length-1;i>=0;i--)stack.push(path.join(file,names[i]))}catch{partial=true;skipped++}}
  else if(stat.isFile()){bytes+=stat.size;files++}
 }
 return {bytes,files,entries,skipped,partial,fingerprint:hash.digest('hex')};
}
async function safeCacheRoot(workspace,relative){
 const base=path.resolve(workspace),root=await fs.realpath(base);if(root===path.parse(root).root)throw Error('不支持扫描磁盘根目录');
 let current=base;
 for(const name of relative.split('/')){current=path.join(current,name);const stat=await fs.lstat(current);if(stat.isSymbolicLink()||!stat.isDirectory())throw Error('缓存路径必须为普通目录');const real=await fs.realpath(current);if(!inside(root,real))throw Error('缓存路径越界')}
 return {root,target:await fs.realpath(current)};
}
function createResourceSpace(runtime,trash,{now=Date.now,scan=scanDirectory}={}){
 const previews=new Map();let pending=false;
 async function workspaces(){const r=await runtime.request({cmd:'workbench.snapshot'});if(!r.ok)throw Error(r.error||'工作区不可用');return r.workspaces}
 async function ensureIdle(id){const r=await runtime.request({cmd:'workbench.sessions',workspace_id:id});if(!r.ok)throw Error('无法验证工作区会话');if(r.sessions.some(s=>!s.status||(s.status.exit===null&&!['host_lost','failed'].includes(s.status.state))))throw Error('工作区仍有运行或启动中的会话，请先明确结束它们')}
 return {
  async scan(args={}){
   if(pending)throw Error('已有空间操作正在进行');pending=true;
   try{
    const ws=(await workspaces()).filter(w=>!args.workspace_id||w.id===args.workspace_id);if(!ws.length)throw Error('工作区不存在');
    const deadline=now()+15000,rows=[],targets=[];
    for(const w of ws){
     const row={id:w.id,name:w.name,path:w.path,bytes:null,partial:false,caches:[],error:null};rows.push(row);
     if(now()>deadline){row.partial=true;row.error='达到扫描时间上限，请单独扫描此工作区';continue}
     try{
      const root=await fs.realpath(w.path);if(root===path.parse(root).root)throw Error('不支持扫描磁盘根目录');
      // Scan safe cache candidates first so a huge workspace cannot hide cleanup candidates.
      for(const relative of CACHE_PATHS){try{const {target}=await safeCacheRoot(w.path,relative),result=await scan(target,{deadline,maxEntries:50000});const key=crypto.randomUUID(),item={key,relative,path:target,...result};row.caches.push(item);if(!result.partial&&!result.skipped)targets.push({key,workspaceId:w.id,workspacePath:w.path,relative,target,root,fingerprint:result.fingerprint,bytes:result.bytes})}catch(e){if(e.code!=='ENOENT')row.caches.push({relative,error:String(e.message)})}}
      const result=await scan(root,{deadline});row.bytes=result.bytes;row.partial=result.partial;row.skipped=result.skipped;
     }catch(e){row.error=String(e.message)}
    }
    for(const [key,p] of previews)if(now()-p.created>300000)previews.delete(key);
    const token=crypto.randomUUID();previews.set(token,{created:now(),targets});return {token,scannedAt:now(),rows};
   }finally{pending=false}
  },
  async clean(args={}){
   if(pending)throw Error('已有空间操作正在进行');
   const preview=previews.get(args.token);if(!preview||now()-preview.created>300000)throw Error('预览已过期，请重新扫描');
   if(args.confirmed!==true||!Array.isArray(args.keys)||!args.keys.length||new Set(args.keys).size!==args.keys.length)throw Error('请选择并确认具体缓存目录');
   const targets=args.keys.map(k=>preview.targets.find(t=>t.key===k));if(targets.some(t=>!t))throw Error('清理对象不属于当前预览');
   if(new Set(targets.map(t=>t.target)).size!==targets.length)throw Error('同一个缓存目录被重复选择，请只保留一项');
   pending=true;const completed=[];
   try{
    const ws=await workspaces();
    for(const target of targets){
     const w=ws.find(w=>w.id===target.workspaceId&&w.path===target.workspacePath);if(!w)throw Error('工作区已变更，请重新扫描');
     await ensureIdle(w.id);const checked=await safeCacheRoot(w.path,target.relative);if(checked.root!==target.root||checked.target!==target.target)throw Error('缓存路径已变更');
     const current=await scan(target.target);if(current.partial||current.skipped||current.fingerprint!==target.fingerprint)throw Error('缓存内容已变更，请重新扫描');
    }
    previews.delete(args.token);
    for(const target of targets){await ensureIdle(target.workspaceId);const checked=await safeCacheRoot(target.workspacePath,target.relative);if(checked.root!==target.root||checked.target!==target.target)throw Error('缓存路径已变更');const current=await scan(target.target);if(current.partial||current.skipped||current.fingerprint!==target.fingerprint)throw Error('缓存内容已变更，请重新扫描');await trash(target.target);completed.push({path:target.target,bytes:target.bytes})}
    return {ok:true,completed};
   }catch(e){return {ok:false,error:String(e.message),completed}}finally{pending=false}
  },
 };
}
module.exports={createResourceSpace,scanDirectory,safeCacheRoot,CACHE_PATHS};
