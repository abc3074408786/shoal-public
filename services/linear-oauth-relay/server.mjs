// Public HTTPS is terminated by the deployment's reverse proxy. Never log URLs:
// the callback query contains an authorization code. Tokens never visit this service.
import {createServer} from 'node:http';
import {randomBytes,createHash,timingSafeEqual} from 'node:crypto';
import {pathToFileURL} from 'node:url';

const random=()=>randomBytes(32).toString('base64url');
const digest=value=>createHash('sha256').update(value).digest();
export function createRelay({clientId,publicUrl,now=Date.now,ttl=600_000,maxSessions=256}) {
  const base=new URL(publicUrl);
  if(!clientId||!/^https:$/.test(base.protocol)||base.username||base.password||base.search||base.hash||base.pathname!=='/')throw new Error('Configure clientId and an HTTPS origin');
  const redirectUri=new URL('/callback',base).href;
  const sessions=new Map(),states=new Map(),rates=new Map();
  function clean() {
    for(const [id,s] of sessions)if(s.expires<=now()){sessions.delete(id);states.delete(s.state);}
    for(const [ip,r] of rates)if(r.until<=now())rates.delete(ip);
  }
  function reply(res,status,value,html=false) {
    res.writeHead(status,{'Content-Type':html?'text/html; charset=utf-8':'application/json','Cache-Control':'no-store','Referrer-Policy':'no-referrer','X-Content-Type-Options':'nosniff','Content-Security-Policy':"default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"});
    res.end(html?`<!doctype html><html lang="zh-CN"><meta charset="utf-8"><title>Velella · Linear</title><h1>${value}</h1><p>请回到电脑上的 Velella 查看连接结果。可以关闭此页面。</p></html>`:JSON.stringify(value));
  }
  const server=createServer(async(req,res)=>{
    clean();
    try {
      const url=new URL(req.url,base);
      if(req.method==='GET'&&url.pathname==='/health')return reply(res,200,{ok:true});
      const entry=/^\/authorize\/([A-Za-z0-9_-]{43})$/.exec(url.pathname);
      if(req.method==='GET'&&entry){
        const s=sessions.get(entry[1]);
        if(!s||s.status!=='waiting')return reply(res,410,'二维码已失效，请在 Velella 重新生成。',true);
        res.writeHead(302,{'Location':s.authorizationUrl,'Cache-Control':'no-store','Referrer-Policy':'no-referrer'});res.end();return;
      }
      if(req.method==='GET'&&url.pathname==='/callback') {
        const state=url.searchParams.get('state'),id=states.get(state),s=sessions.get(id);
        if(!s||s.status!=='waiting')return reply(res,400,'授权已失效或已处理，请在 Velella 重新发起。',true);
        const codes=url.searchParams.getAll('code'),errors=url.searchParams.getAll('error');
        if(url.searchParams.getAll('state').length!==1||codes.length+errors.length!==1)return reply(res,400,'授权回调无效。',true);
        if(errors.length){s.status='denied';return reply(res,200,'已取消 Linear 授权',true);}
        if(!codes[0]||codes[0].length>4096||/[\x00-\x20\x7f]/.test(codes[0]))return reply(res,400,'授权回调无效。',true);
        s.code=codes[0];s.status='authorized';
        return reply(res,200,'Linear 授权已收到',true);
      }
      // Desktop-only control API. No cross-origin browser access or cookies.
      if(req.headers.origin)return reply(res,403,{error:'origin_not_allowed'});
      if(req.method==='POST'&&url.pathname==='/sessions') {
        const ip=req.socket.remoteAddress;
        const rate=rates.get(ip)||{count:0,until:now()+60_000};rates.set(ip,rate);
        if(++rate.count>20||sessions.size>=maxSessions)return reply(res,429,{error:'busy'});
        if(req.headers['content-type']?.split(';')[0]!=='application/json')return reply(res,415,{error:'json_required'});
        let body='';for await(const chunk of req){body+=chunk;if(body.length>2048){reply(res,413,{error:'too_large'});return;}}
        let data;try{data=JSON.parse(body);}catch{return reply(res,400,{error:'invalid_json'});}
        if(!/^[A-Za-z0-9_-]{43}$/.test(data.code_challenge||''))return reply(res,400,{error:'invalid_challenge'});
        const id=random(),state=random(),pollToken=random();
        const authorization=new URL('https://linear.app/oauth/authorize');
        authorization.search=new URLSearchParams({client_id:clientId,redirect_uri:redirectUri,response_type:'code',scope:'read,write',state,code_challenge:data.code_challenge,code_challenge_method:'S256',prompt:'consent'}).toString();
        sessions.set(id,{state,pollHash:digest(pollToken),status:'waiting',expires:now()+ttl,authorizationUrl:authorization.href});states.set(state,id);
        return reply(res,201,{id,poll_token:pollToken,authorization_url:authorization.href,client_id:clientId,redirect_uri:redirectUri,expires_in:Math.floor(ttl/1000)});
      }
      const match=/^\/sessions\/([A-Za-z0-9_-]{43})$/.exec(url.pathname);
      if(match) {
        const s=sessions.get(match[1]),token=req.headers.authorization?.replace(/^Bearer /,'')||'';
        if(!s||token.length>128||!timingSafeEqual(s.pollHash,digest(token)))return reply(res,404,{error:'not_found'});
        if(req.method==='DELETE'){sessions.delete(match[1]);states.delete(s.state);return reply(res,200,{status:'cancelled'});}
        if(req.method==='GET')return reply(res,200,{status:s.status,...(s.code?{code:s.code}:{})});
      }
      reply(res,404,{error:'not_found'});
    } catch {if(!res.headersSent)reply(res,400,{error:'invalid_request'});else res.end();}
  });
  server.requestTimeout=10_000;server.headersTimeout=10_000;
  const timer=setInterval(clean,30_000);timer.unref();server.on('close',()=>clearInterval(timer));
  return server;
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href) {
  const server=createRelay({clientId:process.env.LINEAR_CLIENT_ID,publicUrl:process.env.LINEAR_PUBLIC_URL});
  server.listen(Number(process.env.PORT||8787),process.env.HOST||'127.0.0.1',()=>console.log('Linear callback relay ready'));
}
