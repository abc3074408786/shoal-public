// Test-only observer around the actual production Electron entry point.
const { app } = require('electron');
const fs = require('node:fs');
app.on('web-contents-created', (_event, contents) => {
  contents.on('did-finish-load', async () => {
    if (!contents.getURL().endsWith('/dist/index.html')) return;
    try {
      const until = Date.now() + 15000;
      while (Date.now() < until) {
        const ready = await contents.executeJavaScript(`(async () => {
          if (!window.velellaDesktop || document.body.innerText.length < 20) return false;
          const value = await window.velellaDesktop.invoke('rpc', {request:{cmd:'workbench.snapshot'}});
          return value.ok === true;
        })()`);
        if (ready) {
          fs.writeFileSync(process.env.VELELLA_PERF_READY, JSON.stringify({pid:process.pid, ready:true}));
          return;
        }
        await new Promise(resolve => setTimeout(resolve, 25));
      }
      throw Error('workbench readiness timeout');
    } catch (error) { fs.writeFileSync(process.env.VELELLA_PERF_READY, JSON.stringify({error:String(error)})); }
  });
});
require('../desktop/electron/main.cjs');
