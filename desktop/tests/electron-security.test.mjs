import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const { safeURL, trustedSender } = require("../electron/security.cjs");
const { createRuntime } = require("../electron/runtime.cjs");
test("Electron local IPC rejects foreign contents, subframes and changed origin", () => {
  const frame = { url: "file:///app/index.html" },
    wc = { mainFrame: frame };
  assert.equal(
    trustedSender({ sender: wc, senderFrame: frame }, wc, frame.url),
    true,
  );
  assert.equal(
    trustedSender({ sender: {}, senderFrame: frame }, wc, frame.url),
    false,
  );
  assert.equal(
    trustedSender(
      { sender: wc, senderFrame: { url: frame.url } },
      wc,
      frame.url,
    ),
    false,
  );
  assert.equal(
    trustedSender({ sender: wc, senderFrame: frame }, wc, "https://evil.test"),
    false,
  );
});
test("Electron navigation disallows local files, executable schemes and URL credentials", () => {
  for (const u of [
    "file:///etc/passwd",
    "javascript:alert(1)",
    "data:text/html,x",
    "https://user:password@example.com",
  ])
    assert.throws(() => safeURL(u));
  assert.equal(safeURL("http://127.0.0.1:3000"), "http://127.0.0.1:3000/");
});
test("runtime transport never replays a mutation after ambiguous connection failure", async () => {
  const calls = [];
  const runtime = createRuntime("unused", "unused", {
    transport: async (r) => {
      calls.push(r.cmd);
      if (r.cmd === "mutation") throw Error("lost reply");
      return { ok: true };
    },
  });
  await assert.rejects(runtime.request({ cmd: "mutation" }), /lost reply/);
  assert.deepEqual(calls, ["snapshot", "mutation"]);
  await runtime.request({ cmd: "read" });
  assert.deepEqual(calls, ["snapshot", "mutation", "snapshot", "read"]);
});
test("concurrent first callers share readiness but execute mutations once each", async () => {
  const calls = [];
  const runtime = createRuntime("unused", "unused", {
    transport: async (r) => {
      calls.push(r.cmd);
      await new Promise((r) => setImmediate(r));
      return { ok: true };
    },
  });
  await Promise.all([
    runtime.request({ cmd: "one" }),
    runtime.request({ cmd: "two" }),
  ]);
  assert.deepEqual(calls, ["snapshot", "one", "two"]);
});

test('runtime UTF-8 survives replies split in the middle of a Chinese character',async()=>{
  const net=await import('node:net');const {socketRequest}=require('../electron/runtime.cjs');
  const server=net.createServer(socket=>socket.once('data',()=>{const reply=Buffer.from('{"ok":true,"text":"终端"}\n');const split=reply.indexOf(Buffer.from('终'))+1;socket.write(reply.subarray(0,split));setTimeout(()=>socket.end(reply.subarray(split)),10)}));
  await new Promise(r=>server.listen(0,'127.0.0.1',r));
  try{assert.deepEqual(await socketRequest({host:'127.0.0.1',port:server.address().port},{cmd:'read'}),{ok:true,text:'终端'})}finally{await new Promise(r=>server.close(r))}
});
