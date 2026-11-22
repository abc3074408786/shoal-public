export function decodeGitPath(path:string):string {
  if(!path.startsWith('"')||!path.endsWith('"'))return path;
  const source=path.slice(1,-1),bytes:number[]=[],encoder=new TextEncoder();
  const escapes:Record<string,string>={n:'\n',r:'\r',t:'\t',b:'\b',f:'\f',v:'\v','\\':'\\','"':'"'};
  for(let i=0;i<source.length;){
    if(source[i]==='\\'){
      const octal=source.slice(i+1,i+4);
      if(/^[0-7]{3}$/.test(octal)){bytes.push(parseInt(octal,8));i+=4;continue}
      bytes.push(...encoder.encode(escapes[source[i+1]]??source[i+1]??'\\'));i+=2;
    }else{const point=String.fromCodePoint(source.codePointAt(i)!);bytes.push(...encoder.encode(point));i+=point.length}
  }
  return new TextDecoder().decode(new Uint8Array(bytes));
}
export function parseGitStatus(status:string){return status.split('\n').filter(line=>line.length>=4).map(line=>{
  const code=line.slice(0,2);let raw=line.slice(3),quoted=false,escaped=false,separator=-1;
  if(/[RC]/.test(code))for(let i=0;i<raw.length;i++){
    if(escaped){escaped=false;continue}if(raw[i]==='\\'){escaped=true;continue}if(raw[i]==='"')quoted=!quoted;
    if(!quoted&&raw.slice(i,i+4)===' -> ')separator=i;
  }
  if(separator>=0)raw=raw.slice(separator+4);
  return {code,path:decodeGitPath(raw),staged:code[0]!==' '&&code[0]!=='?',kind:code.includes('?')?'untracked':code.includes('D')?'deleted':code.includes('A')?'added':code.includes('R')?'renamed':'modified'};
})}
