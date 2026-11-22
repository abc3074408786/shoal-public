// Native state is authoritative when backgroundThrottling:false keeps the page
// visible to Chromium. This observer never changes the window or its timers.
function createWindowVisibility(window) {
  const contents=window.webContents;
  let disposed=false,last;
  const read=()=>({visible:!window.isDestroyed()&&window.isVisible(),minimized:!window.isDestroyed()&&window.isMinimized()});
  const publish=(force=false)=>{
    if(disposed||window.isDestroyed()||contents.isDestroyed())return;
    const state=read(),key=JSON.stringify(state);
    if(force||key!==last){last=key;contents.send('window-visibility',state)}
  };
  const changed=()=>publish(),loaded=()=>publish(true);
  const events=['show','hide','minimize','restore'];
  const dispose=()=>{
    if(disposed)return;disposed=true;
    for(const event of events)window.removeListener(event,changed);
    contents.removeListener('did-finish-load',loaded);
    window.removeListener('closed',dispose);
  };
  for(const event of events)window.on(event,changed);
  contents.on('did-finish-load',loaded);window.on('closed',dispose);
  return {read,dispose};
}
module.exports={createWindowVisibility};
