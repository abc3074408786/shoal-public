import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { execFile } from 'node:child_process';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { homedir } from 'node:os';

export default defineConfig({
  base: './',
  plugins: [react(), {name:'desktop-production-csp',apply:'build',transformIndexHtml(html){return html.replace('<head>', '<head>'+'<meta http-equiv="Content-Security-Policy" content="default-src \'self\'; script-src \'self\'; style-src \'self\' \'unsafe-inline\'; img-src \'self\' data: blob: http: https:; font-src \'self\' data:; connect-src \'self\' http://127.0.0.1:* ws://127.0.0.1:*; worker-src \'self\' blob:; object-src \'none\'; base-uri \'none\'; frame-src \'none\'"/>')}}, {
    name: 'local-runtime-development-bridge',
    configureServer(server) {
      const executable=process.env.VELELLA_RUNTIME || resolve('../target/debug/velella-runtime'+(process.platform==='win32'?'.exe':''));
      const root=process.env.VELELLA_DATA_DIR || resolve(process.env.LOCALAPPDATA || homedir(),'Velella');
      const {socketRequest}=createRequire(resolve('package.json'))(resolve('electron/runtime.cjs')) as {socketRequest:(endpoint:string,request:unknown,timeout?:number)=>Promise<unknown>};
      let endpoint:Promise<string>|undefined;
      const connect=()=>endpoint??=(process.platform==='win32'?new Promise<string>((resolveEndpoint,reject)=>{
        execFile(executable,['ipc-endpoint',root,'daemon'],{windowsHide:true,timeout:5000},(error,stdout)=>{
          if(error){reject(error);return}try{resolveEndpoint(JSON.parse(stdout))}catch(error){reject(error)}
        });
      }):Promise.resolve(resolve(root,'daemon.sock'))).catch(error=>{endpoint=undefined;throw error});
      server.middlewares.use('/_velella', (req, res) => {
        const origin = req.headers.origin;
        if (req.method !== 'POST' || (origin && !/^http:\/\/127\.0\.0\.1:\d+$/.test(origin))) { res.statusCode=403;res.end();return; }
        let body='';
        req.on('data', c => { body+=c; if (body.length>2*1024*1024) req.destroy(); });
        req.on('end', () => {
          const reply=(value:string,status=200)=>{
            if(res.destroyed||res.writableEnded)return;
            res.statusCode=status;
            res.setHeader('Content-Type','application/json');
            res.end(value);
          };
          try {
            const request=JSON.parse(body);
            // Match the desktop transport: one endpoint lookup, then native pipe I/O.
            // Do not replay input or any other mutation after a connection failure.
            void connect().then(address=>socketRequest(address,request,50000)).then(result=>reply(JSON.stringify(result))).catch(error=>{
              endpoint=undefined;
              reply(JSON.stringify({ok:false,error:error instanceof Error?error.message:String(error)}),503);
            });
          } catch(error) {
            // Windows can throw synchronously when starting a child under resource pressure.
            // Return a failed RPC; never crash Vite or automatically replay a mutation.
            reply(JSON.stringify({ok:false,error:error instanceof Error?error.message:String(error)}),503);
          }
        });
      });
    },
  }],
  server: {port:1420,strictPort:true},
  clearScreen:false,
  build:{
    chunkSizeWarningLimit:1200,
    rollupOptions:{
      input:{
        main:fileURLToPath(new URL('./index.html',import.meta.url)),
        hud:fileURLToPath(new URL('./hud.html',import.meta.url)),
      },
    },
  },
});
