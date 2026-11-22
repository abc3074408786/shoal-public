// Session-private stdio MCP transport. Only this generated file is transferred.
const fs=require('node:fs');
const net=require('node:net');
const path=require('node:path');
// The model provider token belongs only to the native model process.
delete process.env.VELELLA_REMOTE_MODEL_TOKEN;
const config=JSON.parse(fs.readFileSync(path.join(__dirname,'bridge.json'),'utf8'));
if(!/^b-[0-9a-f]{16}\.sock$/.test(config.socket_name))throw Error('Invalid bridge socket');
const socket=net.createConnection(path.join(__dirname,config.socket_name));
socket.once('connect',()=>{socket.write(JSON.stringify({session_id:config.session_id,token:config.token})+'\n');process.stdin.pipe(socket);socket.pipe(process.stdout);});
socket.once('error',()=>{process.stderr.write('Velella local bridge unavailable; no request was replayed\n');process.exitCode=1;process.stdin.destroy();});
socket.once('close',()=>{process.stdin.destroy();});
process.stdin.once('end',()=>socket.end());
