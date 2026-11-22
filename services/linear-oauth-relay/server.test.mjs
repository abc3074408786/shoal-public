import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createRelay} from './server.mjs';

async function fixture(t,options={}) {
  const server=createRelay({clientId:'test-client',publicUrl:'https://auth.example.test',...options});
  await new Promise(r=>server.listen(0,'127.0.0.1',r));t.after(()=>new Promise(r=>{server.close(r);server.closeAllConnections();}));
  const base=`http://127.0.0.1:${server.address().port}`;
  const request=(path,init)=>fetch(base+path,init);
  const start=async()=>{const r=await request('/sessions',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({code_challenge:'x'.repeat(43)})});assert.equal(r.status,201);return r.json();};
  return {request,start};
}
test('cross-device callback requires state and private polling proof; replay cannot replace code',async t=>{
  const {request,start}=await fixture(t),s=await start(),url=new URL(s.authorization_url),path='/sessions/'+s.id,headers={authorization:'Bearer '+s.poll_token};
  assert.equal(url.origin,'https://linear.app');assert.equal(url.searchParams.get('code_challenge_method'),'S256');assert.equal(url.searchParams.get('redirect_uri'),'https://auth.example.test/callback');assert(!s.authorization_url.includes(s.poll_token));
  const entry=await request('/authorize/'+s.id,{redirect:'manual'});assert.equal(entry.status,302);assert.equal(entry.headers.get('location'),s.authorization_url);assert.equal(entry.headers.get('cache-control'),'no-store');
  assert.equal((await request(path)).status,404);
  assert.equal((await request('/callback?state=wrong&code=bad')).status,400);
  assert.deepEqual(await (await request(path,{headers})).json(),{status:'waiting'});
  const callback='/callback?state='+url.searchParams.get('state');
  const response=await request(callback+'&code=fixture-code');assert.equal(response.status,200);assert.equal(response.headers.get('cache-control'),'no-store');assert(!(await response.text()).includes('fixture-code'));
  assert.deepEqual(await (await request(path,{headers})).json(),{status:'authorized',code:'fixture-code'});
  assert.equal((await request(callback+'&code=replay')).status,400);
  assert.equal((await request('/authorize/'+s.id,{redirect:'manual'})).status,410);
  assert.equal((await request(path,{method:'DELETE',headers})).status,200);assert.equal((await request(path,{headers})).status,404);
});
test('denial, expiry and cancellation never disclose credentials',async t=>{
  let time=0;const {request,start}=await fixture(t,{now:()=>time}),a=await start(),b=await start();
  const state=new URL(a.authorization_url).searchParams.get('state');
  await request('/callback?state='+state+'&error=access_denied');
  assert.deepEqual(await (await request('/sessions/'+a.id,{headers:{authorization:'Bearer '+a.poll_token}})).json(),{status:'denied'});
  time=600001;assert.equal((await request('/sessions/'+b.id,{headers:{authorization:'Bearer '+b.poll_token}})).status,404);
});
test('reject malformed, cross-origin and excessive session creation',async t=>{
  const {request,start}=await fixture(t,{maxSessions:1});
  assert.equal((await request('/sessions',{method:'POST',headers:{origin:'https://evil.test'}})).status,403);
  assert.equal((await request('/sessions',{method:'POST',headers:{'content-type':'application/json'},body:'{}'})).status,400);
  await start();assert.equal((await request('/sessions',{method:'POST'})).status,429);
});
test('deployment requires HTTPS origin',()=>{
  for(const publicUrl of ['http://localhost','https://user:pass@host','https://host/path','https://host/?x=1'])assert.throws(()=>createRelay({clientId:'x',publicUrl}));
});
