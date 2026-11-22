// Native IME test window; all RPCs are owned by the isolated browser fixture.
const {app,BrowserWindow}=require('electron');
const path=require('node:path');
app.setPath('userData',path.resolve(__dirname,'../../artifacts/terminal-input-parity-20261101/native-profile'));
app.whenReady().then(()=>{
 const window=new BrowserWindow({width:1280,height:800,title:'Velella 输入隔离验证',webPreferences:{contextIsolation:true,nodeIntegration:false,sandbox:true}});
 window.webContents.setWindowOpenHandler(()=>({action:'deny'}));
 window.loadURL('http://127.0.0.1:8808/tests/terminal-input-parity.html');
});
app.on('window-all-closed',()=>app.quit());
