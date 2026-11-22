// Allocation diagnostics for an isolated Electron instance. Trace values are
// overlapping allocator accounts, not additive process-tree memory totals.
const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
async function captureChromiumMemory(app,page,out,label){
 const file=path.join(out,'chromium-memory-'+label+'.json');
 const session=await page.context().newCDPSession(page);
 let layers=[],layerError=null,started=false;
 session.on('LayerTree.layerTreeDidChange',event=>{layers=event.layers||[]});
 try{
  try{await session.send('LayerTree.enable')}catch(error){layerError=String(error)}
  const begin=Date.now()/1000;
  await app.evaluate(async({contentTracing})=>{
   await contentTracing.startRecording({
    included_categories:['disabled-by-default-memory-infra'],
    recording_mode:'record-until-full',trace_buffer_size_in_kb:8192,
    memory_dump_config:{allowed_dump_modes:['detailed'],triggers:[{mode:'detailed',periodic_interval_ms:1000}]},
   });
  });started=true;
  await new Promise(resolve=>setTimeout(resolve,2300));
  const processes=await app.evaluate(({app})=>app.getAppMetrics().map(({pid,type,creationTime,memory})=>({pid,type,creationTime,memory})));
  const usage=await app.evaluate(({contentTracing})=>contentTracing.getTraceBufferUsage());
  const recorded=await app.evaluate(({contentTracing},file)=>contentTracing.stopRecording(file),file);started=false;
  assert.equal(recorded,file);assert.ok(fs.statSync(file).size>0,'empty Chromium memory trace');
  const reasons=[];
  for(const layer of layers.filter(layer=>layer.drawsContent).sort((a,b)=>b.width*b.height-a.width*a.height).slice(0,12)){
   try{reasons.push({layerId:layer.layerId,...await session.send('LayerTree.compositingReasons',{layerId:layer.layerId})})}
   catch(error){reasons.push({layerId:layer.layerId,error:String(error)})}
  }
  return {label,file,begin,end:Date.now()/1000,processes,usage,layers,reasons,layerError,
   overhead:'Diagnostic trace buffer/dumps enabled; not comparable to an untraced memory profile.'};
 }finally{
  if(started)await app.evaluate(({contentTracing},file)=>contentTracing.stopRecording(file),file).catch(()=>{});
  await session.send('LayerTree.disable').catch(()=>{});await session.detach();
 }
}
module.exports={captureChromiumMemory};
