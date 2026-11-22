import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import ts from 'typescript';

// Virtual time exercises the production scheduler without timing-sensitive CPU assertions.
function fixture(page) {
  let now=0,next=0;const timers=new Map(),errors=[],exports={};
  const code=ts.transpileModule(readFileSync(new URL('../src/terminal-output.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
  runInNewContext(code,{exports,performance:{now:()=>now},setTimeout:(fn,delay)=>{const id=++next;timers.set(id,{at:now+delay,fn});return id},clearTimeout:id=>timers.delete(id)});
  const pump=exports.createTerminalOutputPump(page,error=>errors.push(error));
  const flush=async()=>{for(let i=0;i<12;i++)await Promise.resolve()};
  async function advance(ms){const end=now+ms;let guard=0;while(true){await flush();const entry=[...timers.entries()].sort((a,b)=>a[1].at-b[1].at)[0];if(!entry||entry[1].at>end)break;assert.ok(++guard<10000,'scheduler busy loop');now=entry[1].at;timers.delete(entry[0]);entry[1].fn()}now=end;await flush()}
  return {pump,advance,flush,errors,timers,time:()=>now};
}

test('hidden idle terminals back off, while reveal and user input wake immediately',async()=>{
  const reads=[];let f;f=fixture(async()=>{reads.push(f.time());return {backlog:false,changed:false}});
  f.pump.setVisible(false);await f.advance(10000);
  console.log(JSON.stringify({hiddenIdleReadsPer10Seconds:reads.length,readTimes:reads}));
  assert.ok(reads.length<=12,`hidden terminal issued ${reads.length} empty reads in 10 seconds`);
  let count=reads.length;f.pump.setVisible(true);await f.advance(0);assert.equal(reads.length,count+1);
  await f.advance(1000);count=reads.length;f.pump.wake();await f.advance(0);assert.equal(reads.length,count+1);
  f.pump.dispose();await f.advance(10000);assert.equal(f.timers.size,0);assert.deepEqual(f.errors,[]);
});

test('input during an in-flight page queues one immediate read without overlap',async()=>{
  let release,reads=0,active=0,maxActive=0;
  const f=fixture(async()=>{reads++;maxActive=Math.max(maxActive,++active);if(reads===1)await new Promise(resolve=>release=resolve);active--;return {backlog:false,changed:false}});
  await f.advance(0);f.pump.wake();f.pump.wake();await f.advance(1000);assert.equal(reads,1);
  release();await f.flush();await f.advance(0);assert.equal(reads,2);assert.equal(maxActive,1);f.pump.dispose();
});

test('hidden output backlogs drain without idle delays and dispose cancels pending work',async()=>{
  let reads=0;const f=fixture(async()=>({changed:true,backlog:++reads<20}));
  f.pump.setVisible(false);await f.advance(0);assert.equal(reads,20);f.pump.dispose();await f.advance(1000);assert.equal(reads,20);
});

test('caught-up hidden streaming is bounded and reveal bypasses its wait',async()=>{
 const reads=[];let f;f=fixture(async()=>{reads.push(f.time());return {changed:true,backlog:false}});
 f.pump.setVisible(false);await f.advance(10000);
 console.log(JSON.stringify({hiddenActiveReadsPer10Seconds:reads.length}));
 assert.ok(reads.length<=41,`hidden caught-up stream issued ${reads.length} reads in 10 seconds`);
 await f.advance(1);const count=reads.length;f.pump.setVisible(true);await f.advance(0);
 assert.equal(reads.length,count+1,'reveal must not wait for the hidden cadence');
 await f.advance(80);assert.ok(reads.length>=count+10,'visible stream must recover its interactive cadence');
 f.pump.dispose();assert.deepEqual(f.errors,[]);
});
test('a delayed key echo is not left waiting behind idle backoff',async()=>{
 let f,readyAt=Infinity,observedAt;const reads=[];
 f=fixture(async()=>{reads.push(f.time());const changed=f.time()>=readyAt&&observedAt===undefined;if(changed)observedAt=f.time();return {changed,backlog:false}});
 await f.advance(1000);readyAt=f.time()+50;f.pump.wake();await f.advance(150);
 assert.ok(observedAt-readyAt<=8,`echo waited ${observedAt-readyAt} ms after it was ready`);
 // The echo itself renews the interactive window. Measure quiet polling after
 // that bounded window expires, not while the last output is still active.
 await f.advance(150);
 const before=reads.length;await f.advance(2000);assert.ok(reads.length-before<=25,'interactive polling must expire back to the idle cadence');f.pump.dispose();
});

test('ongoing streamed output stays responsive between chunks without user input',async()=>{
  let f,nextChunk=1000;const latencies=[];
  f=fixture(async()=>{
    let changed=false;
    while(nextChunk<=f.time()&&nextChunk<3000){latencies.push(f.time()-nextChunk);nextChunk+=80;changed=true;}
    return {changed,backlog:false};
  });
  await f.advance(5000);f.pump.dispose();
  // The first chunk wakes an idle reader; subsequent chunks belong to the
  // same active stream even when several empty reads fall between them.
  const streaming=latencies.slice(1);
  console.log(JSON.stringify({streamChunkIntervalMs:80,streamingLatenciesMs:streaming}));
  assert.equal(latencies.length,25,'every streamed chunk must be consumed');
  assert.ok(Math.max(...streaming)<=8,`active stream waited up to ${Math.max(...streaming)} ms`);
});
