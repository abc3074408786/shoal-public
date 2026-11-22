const {spawn}=require('node:child_process');
const path=require('node:path');
const MAX_REQUEST=2*1024*1024;
const MAX_RESPONSE=8*1024*1024;
const MAX_INFLIGHT=8;
const UUID=/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const plain=value=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const boundedText=(value,max)=>typeof value==='string'&&value.length>0&&Buffer.byteLength(value)<=max&&!/[\x00-\x1f\x7f]/.test(value);
function sshArguments(profile) {
  if(!plain(profile)||!boundedText(profile.host,255)||profile.host.startsWith('-')||!/^[a-zA-Z0-9._:\-]+$/.test(profile.host))throw Error('invalid SSH host');
  if(profile.user!==undefined&&profile.user!==''&&(!boundedText(profile.user,255)||profile.user.startsWith('-')||!/^[a-zA-Z0-9._@\\-]+$/.test(profile.user)))throw Error('invalid SSH user');
  const port=profile.port??22;
  if(!Number.isInteger(port)||port<1||port>65535)throw Error('invalid SSH port');
  const args=['-T'];
  for(const option of ['BatchMode=yes','StrictHostKeyChecking=yes','ForwardAgent=no','ForwardX11=no','ForwardX11Trusted=no','ClearAllForwardings=yes','PermitLocalCommand=no','RemoteCommand=none','RequestTTY=no','ControlMaster=no','ControlPath=none','ConnectTimeout=10','ConnectionAttempts=1','ServerAliveInterval=15','ServerAliveCountMax=2'])args.push('-o',option);
  args.push('-p',String(port));
  if(profile.user)args.push('-l',profile.user);
  if(profile.identity) {
    if(!boundedText(profile.identity,4096)||!path.isAbsolute(profile.identity))throw Error('SSH identity requires an absolute local path');
    args.push('-i',profile.identity);
  }
  args.push('--',profile.host,'velella-runtime remote-stdio');
  return args;
}
function encodeFrame(value,limit) {
  const bytes=Buffer.from(JSON.stringify(value),'utf8');
  if(!bytes.length||bytes.length>limit)throw Error('remote frame too large');
  const header=Buffer.alloc(4);header.writeUInt32BE(bytes.length);return Buffer.concat([header,bytes]);
}
class FrameDecoder {
  constructor(limit,onValue){this.limit=limit;this.onValue=onValue;this.header=Buffer.alloc(4);this.headerBytes=0;this.payload=null;this.payloadBytes=0;}
  push(chunk) {
    let offset=0;
    while(offset<chunk.length) {
      if(!this.payload) {
        const size=Math.min(4-this.headerBytes,chunk.length-offset);
        chunk.copy(this.header,this.headerBytes,offset,offset+size);this.headerBytes+=size;offset+=size;
        if(this.headerBytes!==4)continue;
        const length=this.header.readUInt32BE(0);
        if(length===0||length>this.limit)throw Error('invalid remote frame length');
        this.payload=Buffer.allocUnsafe(length);this.payloadBytes=0;
      }
      const size=Math.min(this.payload.length-this.payloadBytes,chunk.length-offset);
      chunk.copy(this.payload,this.payloadBytes,offset,offset+size);this.payloadBytes+=size;offset+=size;
      if(this.payloadBytes===this.payload.length) {
        const bytes=this.payload;this.payload=null;this.payloadBytes=0;this.headerBytes=0;
        this.onValue(JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(bytes)));
      }
    }
  }
  end(){if(this.headerBytes||this.payload)throw Error('truncated remote frame');}
  attach(stream){stream.on('data',chunk=>this.push(chunk));return this;}
}
function checkedIdentity(value) {
  if(!plain(value)||value.protocol!==1||!UUID.test(value.server_id)||!boundedText(value.data_root,4096)||!['linux','windows'].includes(value.os)||!boundedText(value.runtime_version,128)||value.persistence!=='prestarted_unverified'||!Array.isArray(value.capabilities)||value.capabilities.length!==2||!value.capabilities.includes('identity-v1')||!value.capabilities.includes('multiplex-v1'))throw Error('incompatible remote runtime identity');
  const absolute=value.os==='linux'?value.data_root.startsWith('/'):path.win32.isAbsolute(value.data_root);
  if(!absolute)throw Error('remote runtime identity requires an absolute data root');
  return Object.freeze({...value,capabilities:Object.freeze([...value.capabilities])});
}
function sameIdentity(a,b) {
  return ['protocol','server_id','data_root','os','runtime_version','persistence'].every(key=>a[key]===b[key]);
}
function createSshRuntime(profile,{spawnProcess=spawn,timeoutMs=50000}={}) {
  const args=sshArguments(profile);
  if(!Number.isInteger(timeoutMs)||timeoutMs<1||timeoutMs>60000)throw Error('invalid SSH request timeout');
  const child=spawnProcess('ssh',args,{stdio:['pipe','pipe','pipe'],shell:false,windowsHide:true});
  let state='new',identity=null,nextId=1,blocked=false,closed=false,stderr=Buffer.alloc(0);
  const pending=new Map();
  function fail(error,kill=true) {
    if(closed)return;
    closed=true;state='disconnected';
    for(const entry of pending.values()) {
      clearTimeout(entry.timer);
      const reason=Error(`${error.message}; original request was not replayed`);
      reason.code='REMOTE_DISCONNECTED';reason.uncertain=entry.method==='rpc';entry.reject(reason);
    }
    pending.clear();
    child.stdin.destroy();child.stdout.destroy();child.stderr.destroy();
    if(kill)child.kill();
  }
  const decoder=new FrameDecoder(MAX_RESPONSE,value=>{
    if(closed)return;
    if(!plain(value)||!Number.isSafeInteger(value.id)||value.id<1||typeof value.ok!=='boolean'||Object.keys(value).some(key=>!['id','ok','result','error'].includes(key))||value.ok&&(!Object.hasOwn(value,'result')||Object.hasOwn(value,'error'))||!value.ok&&(!boundedText(value.error,4096)||Object.hasOwn(value,'result')))throw Error('invalid remote response');
    const entry=pending.get(value.id);
    if(!entry)throw Error('unknown or duplicate remote response');
    pending.delete(value.id);clearTimeout(entry.timer);
    if(value.ok)entry.resolve(value.result);
    else {const error=Error(value.error);error.code='REMOTE_REJECTED';error.uncertain=entry.method==='rpc';entry.reject(error);fail(error);}
  });
  child.stdout.on('data',chunk=>{try{decoder.push(chunk);}catch(error){fail(error);}});
  child.stdout.on('end',()=>{try{decoder.end();fail(Error('remote connection closed'));}catch(error){fail(error);}});
  child.stdout.on('error',fail);child.stdin.on('error',fail);child.stderr.on('error',fail);
  child.stdin.on('drain',()=>{blocked=false;});
  child.stderr.on('data',chunk=>{stderr=Buffer.concat([stderr,chunk.subarray(-65536)]).subarray(-65536);});
  child.on('error',fail);child.on('exit',(code,signal)=>fail(Error(`SSH disconnected (${signal||code})`),false));
  function send(method,fields) {
    if(closed)return Promise.reject(Error('remote disconnected; connect explicitly before requesting'));
    if(blocked||pending.size>=MAX_INFLIGHT)return Promise.reject(Error('remote connection busy; request was not sent'));
    if(!Number.isSafeInteger(nextId)) {fail(Error('remote request sequence exhausted'));return Promise.reject(Error('connect again to reset request sequence'));}
    let frame;const id=nextId;
    try{frame=encodeFrame({id,method,protocol:1,...fields},MAX_REQUEST);}catch(error){return Promise.reject(error);}
    nextId++;
    return new Promise((resolve,reject)=>{
      const timer=setTimeout(()=>fail(Error('remote acknowledgement timed out')),timeoutMs);
      pending.set(id,{resolve,reject,timer,method});
      try{blocked=!child.stdin.write(frame);}catch(error){fail(error);}
    });
  }
  return {
    get state(){return state;},
    get identity(){return identity;},
    diagnostics(){return stderr.toString('utf8').slice(-16384);},
    async inspect({data_root,expected_server_id}={}) {
      if(state!=='new')throw Error('remote inspection requires a new connection');
      if(data_root!==undefined&&!boundedText(data_root,4096))throw Error('invalid remote data root');
      if(expected_server_id!==undefined&&!UUID.test(expected_server_id))throw Error('invalid expected server identity');
      state='inspecting';
      try {
        const result=checkedIdentity(await send('hello',data_root===undefined?{}:{data_root}));
        if(closed)throw Error('remote connection closed during inspection');
        if(expected_server_id&&result.server_id!==expected_server_id)throw Error('remote server identity changed; explicit review required');
        identity=result;state='awaiting_confirmation';return result;
      }catch(error){fail(error);throw error;}
    },
    async connect(confirmation) {
      if(state!=='awaiting_confirmation')throw Error('inspect remote identity before connecting');
      if(confirmation?.confirm!==true)throw Error('explicit remote confirmation required');
      if(confirmation.server_id!==identity.server_id||confirmation.data_root!==identity.data_root)throw Error('remote confirmation identity mismatch');
      state='connecting';
      try {
        const result=checkedIdentity(await send('connect',{server_id:identity.server_id,data_root:identity.data_root,confirm:true}));
        if(closed)throw Error('remote connection closed during confirmation');
        if(!sameIdentity(result,identity))throw Error('remote runtime identity changed during confirmation');
        state='connected';return identity;
      }catch(error){fail(error);throw error;}
    },
    async request(request) {
      if(state!=='connected')throw Error('remote disconnected; connect explicitly before requesting');
      if(!plain(request))throw Error('remote request must be an object');
      return send('rpc',{server_id:identity.server_id,data_root:identity.data_root,request});
    },
    disconnect(){fail(Error('remote connection closed by client'));},
  };
}
module.exports={createSshRuntime,sshArguments,encodeFrame,FrameDecoder,MAX_REQUEST,MAX_RESPONSE};
