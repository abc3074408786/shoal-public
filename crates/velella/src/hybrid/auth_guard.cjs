// Linux-only authentication file identity guard. It never reads file contents.
// Not yet connected to remote execution or a mount lifecycle.
const fs=require('node:fs');
const path=require('node:path');

function same(a,b){return a.dev===b.dev&&a.ino===b.ino&&a.size===b.size&&a.mtimeNs===b.mtimeNs&&a.ctimeNs===b.ctimeNs&&a.mode===b.mode&&a.uid===b.uid&&a.nlink===b.nlink;}
function validate(stat){
  if(!stat.isFile()||typeof stat.mtimeNs!=='bigint'||typeof stat.ctimeNs!=='bigint'||stat.uid!==BigInt(process.getuid())||stat.nlink!==1n||(stat.mode&0o077n)!==0n||stat.size<=0n||stat.size>65536n)throw Error('unsafe authentication source');
}
function openAuthGuard(source){
  let fd;
  try{
    if(process.platform!=='linux'||typeof process.getuid!=='function'||typeof fs.constants.O_NOFOLLOW!=='number'||!path.isAbsolute(source)||path.resolve(source)!==source||fs.realpathSync.native(source)!==source)throw Error('invalid authentication source');
    fd=fs.openSync(source,fs.constants.O_RDONLY|fs.constants.O_NOFOLLOW|fs.constants.O_NONBLOCK);
    const before=fs.fstatSync(fd,{bigint:true}),named=fs.lstatSync(source,{bigint:true});validate(before);validate(named);
    if(!same(before,named))throw Error('authentication source changed');
    let closed=false;
    return {
      fd,
      verify(){
        try{
          if(closed||fs.realpathSync.native(source)!==source)throw Error('closed or redirected');
          const opened=fs.fstatSync(fd,{bigint:true}),current=fs.lstatSync(source,{bigint:true});validate(opened);validate(current);
          if(!same(before,opened)||!same(before,current))throw Error('changed');
        }catch{throw Error('authentication source changed or unavailable; stop and inspect before continuing');}
      },
      close(){if(!closed){closed=true;fs.closeSync(fd);}},
    };
  }catch{
    if(fd!==undefined)try{fs.closeSync(fd)}catch{}
    throw Error('authentication source cannot be safely bound');
  }
}
module.exports={openAuthGuard};
