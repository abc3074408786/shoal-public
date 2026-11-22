// Electron reports these memory figures in KiB. Private bytes avoid counting
// shared working-set pages repeatedly; they are not whole-machine RSS.
function summarizeResources(metrics,sampledAt){
 const processes=metrics.map(item=>({pid:item.pid,type:item.type,
  privateBytes:typeof item.memory?.privateBytes==='number'&&Number.isFinite(item.memory.privateBytes)&&item.memory.privateBytes>=0?item.memory.privateBytes*1024:null,
 }));
 return {sampledAt,scope:'electron-private',privateBytes:processes.length&&processes.every(p=>p.privateBytes!==null)?processes.reduce((total,p)=>total+p.privateBytes,0):null,processes};
}
function createDesktopResources(app,now=Date.now){
 let cached;
 return ()=>{const time=now();if(cached&&time-cached.sampledAt<5000)return cached;cached=summarizeResources(app.getAppMetrics(),time);return cached};
}
module.exports={createDesktopResources,summarizeResources};
