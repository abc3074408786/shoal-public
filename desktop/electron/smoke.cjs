// Runs in a separate Electron process/profile. Never shipped in the application.
const { app, BrowserWindow, ipcMain } = require("electron");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const http = require("node:http");
const { createRuntime, socketRequest } = require("./runtime.cjs");
const { startControl, activateExisting } = require("./control.cjs");
const { BrowserHost } = require("./browser.cjs");
const { trustedSender } = require("./security.cjs");
const { pathToFileURL } = require("node:url");
const root = fs.mkdtempSync("/tmp/velella-electron-smoke-");
app.setPath("userData", path.join(root, "profile"));
app.enableSandbox();
let win, host, runtime, server, relay;
const results = [];
const check = (name, value) => {
  assert.ok(value, name);
  results.push(name);
};
app
  .whenReady()
  .then(async () => {
    runtime = createRuntime(
      path.resolve(__dirname, "../../target/release/velella-runtime"),
      root,
    );
    relay = await startControl(runtime.executable, root, async (request) =>
      request.op === "activate_main" || request.probe
        ? { ok: true, echo: request, protocol: "smoke" }
        : host.command(request),
    );
    check(
      "native owner-only browser relay",
      (fs.statSync(path.join(root, "browser.sock")).mode & 0o777) === 0o600,
    );
    check(
      "native relay roundtrip",
      (
        await socketRequest(path.join(root, "browser.sock"), {
          probe: "roundtrip",
        })
      ).echo.probe === "roundtrip",
    );
    check(
      "existing desktop ownership detected",
      await activateExisting(runtime.executable, root),
    );
    const snapshot = await runtime.request({ cmd: "workbench.snapshot" });
    check("real Rust daemon boots", snapshot.ok);
    const entry = pathToFileURL(
      path.resolve(__dirname, "../dist/index.html"),
    ).href;
    win = new BrowserWindow({
      show: true,
      width: 1200,
      height: 800,
      webPreferences: {
        preload: path.join(__dirname, "preload.cjs"),
        sandbox: true,
        contextIsolation: true,
        nodeIntegration: false,
      },
    });
    ipcMain.handle("velella:command", (event, command, args) => {
      if (!trustedSender(event, win.webContents, entry))
        throw Error("untrusted");
      if (command === "rpc") return runtime.request(args.request);
      if (command === "window") return false;
      if (command === "browser_layout") return { ok: true };
      throw Error("unsupported smoke command: " + command);
    });
    const consoleErrors = [];
    win.webContents.on("console-message", (event) => {
      if (event.level === "error") consoleErrors.push(event.message);
    });
    await win.loadURL(entry);
    const security = await win.webContents.executeJavaScript(
      "({node:typeof require,bridge:typeof window.velellaDesktop,body:document.body.innerText})",
    );
    check(
      "sandboxed preload bridge available",
      security.bridge === "object" && security.node === "undefined",
    );
    const viaBridge = await win.webContents.executeJavaScript(
      "window.velellaDesktop.invoke('rpc',{request:{cmd:'workbench.snapshot'}})",
    );
    check("frontend IPC reaches real runtime", viaBridge.ok);
    check("React workbench mounts", security.body.length > 20);
    server = http.createServer((_req, res) =>
      res.end(
        '<title>Electron fixture</title><label>Name<input aria-label="Name"></label><button onclick="this.textContent=\'Done\'">Click me</button>',
      ),
    );
    await new Promise((r) => server.listen(0, "127.0.0.1", r));
    host = new BrowserHost(win, runtime);
    fs.mkdirSync(path.join(root, "project"));
    const project = await runtime.request({
      cmd: "project.add",
      path: path.join(root, "project"),
    });
    assert.ok(project.ok, JSON.stringify(project));
    const wb = await runtime.request({ cmd: "workbench.snapshot" });
    const workspace = wb.workspaces[0].id;
    const bridgeCall = (request) =>
      win.webContents.executeJavaScript(
        'window.velellaDesktop.invoke("rpc",{request:' +
          JSON.stringify(request) +
          "})",
      );
    const opened = await bridgeCall({
      cmd: "workbench.terminal.open",
      workspace_id: workspace,
      launch: "terminal",
    });
    check("real PTY created through Electron IPC", opened.ok);
    const terminal = { workspace_id: workspace, session_id: opened.session_id };
    await new Promise((r) => setTimeout(r, 300));
    const sent = await bridgeCall({
      cmd: "workbench.terminal.input",
      ...terminal,
      text: "printf 'ELECTRON_%s\\n' 'PTY_OK'",
      enter: true,
      input_kind: "user",
    });
    check("PTY input accepted", sent.ok);
    let output = "";
    for (let i = 0; i < 40; i++) {
      const read = await bridgeCall({
        cmd: "workbench.terminal.output",
        ...terminal,
        since: 0,
      });
      output = Buffer.from(read.data || "", "base64").toString();
      if (output.includes("ELECTRON_PTY_OK")) break;
      await new Promise((r) => setTimeout(r, 100));
    }
    check(
      "real shell executes and returns PTY bytes",
      output.includes("ELECTRON_PTY_OK"),
    );
    win.hide();
    const hidden = await runtime.request({
      cmd: "workbench.sessions",
      workspace_id: workspace,
    });
    check(
      "terminal survives hidden desktop",
      hidden.sessions.some(
        (s) =>
          s.id === opened.session_id &&
          s.status?.exit === null &&
          s.status?.pid > 0,
      ),
    );
    win.show();
    const created = await host.command({
      op: "create",
      workspace_id: workspace,
      url: "http://127.0.0.1:" + server.address().port,
      reveal: false,
    });
    const id = created.page.id,
      base = { id, workspace_id: workspace };
    host.layout({
      frames: [{ ...base, x: 0, y: 0, width: 1000, height: 700 }],
    });
    const page = host.pages.get(id);
    const remote = await page.view.webContents.executeJavaScript(
      "({node:typeof require,bridge:typeof window.velellaDesktop})",
    );
    check(
      "remote page has no Node or desktop bridge",
      remote.node === "undefined" && remote.bridge === "undefined",
    );
    let snap = await host.command({ ...base, op: "snapshot" });
    check(
      "browser accessibility snapshot",
      snap.nodes.some((n) => n.name === "Click me"),
    );
    const input = snap.nodes.find((n) => n.role === "textbox");
    await host.command({
      ...base,
      op: "fill",
      snapshot_id: snap.snapshot_id,
      ref: input.ref,
      value: "Electron native",
    });
    check(
      "browser fill uses actual page",
      (await page.view.webContents.executeJavaScript(
        'document.querySelector("input").value',
      )) === "Electron native",
    );
    await assert.rejects(
      host.command({
        ...base,
        op: "fill",
        snapshot_id: snap.snapshot_id,
        ref: input.ref,
        value: "replay",
      }),
      /stale/,
    );
    results.push("stale browser reference rejected");
    snap = await host.command({ ...base, op: "snapshot" });
    const button = snap.nodes.find((n) => n.name === "Click me");
    await host.command({
      ...base,
      op: "click",
      snapshot_id: snap.snapshot_id,
      ref: button.ref,
    });
    await new Promise((r) => setTimeout(r, 100));
    check(
      "browser native click",
      (await page.view.webContents.executeJavaScript(
        'document.querySelector("button").textContent',
      )) === "Done",
    );
    const shot = await host.command({ ...base, op: "screenshot" });
    check(
      "browser screenshot is PNG",
      Buffer.from(shot.data, "base64").subarray(1, 4).toString() === "PNG",
    );
    await page.view.webContents.executeJavaScript(
      "console.log('ELECTRON_CONSOLE_FIXTURE');fetch('/network-fixture').then(r=>r.text())",
    );
    await new Promise((r) => setTimeout(r, 50));
    check(
      "console tools bound to the target page",
      (
        await host.command({ ...base, op: "list_console_messages" })
      ).messages.some((m) => m.text === "ELECTRON_CONSOLE_FIXTURE"),
    );
    check(
      "network tools capture target requests",
      (
        await host.command({ ...base, op: "list_network_requests" })
      ).requests.some((r) => r.url.startsWith("http://127.0.0.1:")),
    );
    const annotation = await host.command({ ...base, op: "annotation_status" });
    check(
      "isolated annotation engine",
      typeof annotation.picking === "boolean",
    );
    check(
      "annotation not exposed to page",
      (await page.view.webContents.executeJavaScript(
        "typeof globalThis.__velellaAnnotation",
      )) === "undefined",
    );
    await assert.rejects(
      host.command({ ...base, op: "navigate", url: "file:///etc/passwd" }),
    );
    results.push("file navigation rejected");
    const { execFile } = require("node:child_process");
    const mcp = await new Promise((resolve, reject) => {
      const child = execFile(
        "node",
        [
          path.join(__dirname, "browser-agent/server.mjs"),
          runtime.executable,
          root,
          workspace,
        ],
        { timeout: 15000, maxBuffer: 8 * 1024 * 1024 },
        (error, stdout) =>
          error
            ? reject(error)
            : resolve(
                stdout
                  .trim()
                  .split("\n")
                  .map((line) => JSON.parse(line)),
              ),
      );
      const messages = [
        { id: 1, method: "initialize", params: {} },
        { id: 2, method: "tools/list" },
        {
          id: 3,
          method: "tools/call",
          params: { name: "take_snapshot", arguments: { page_id: id } },
        },
      ];
      child.stdin.end(
        messages
          .map((m) => JSON.stringify({ jsonrpc: "2.0", ...m }))
          .join("\n") + "\n",
      );
    });
    check("MCP tool discovery", mcp[1].result.tools.length === 10);
    check(
      "MCP reaches the exact embedded browser through native IPC",
      JSON.parse(mcp[2].result.content[0].text).page_id === id,
    );
    await assert.rejects(
      host.command({
        ...base,
        workspace_id: "not-this-workspace",
        op: "snapshot",
      }),
    );
    results.push("cross-workspace page access rejected");
    host.layout({ frames: [] });
    await assert.rejects(
      host.command({ ...base, op: "click", snapshot_id: "x", ref: "@e1" }),
    );
    results.push("hidden page input rejected");
    console.log(
      JSON.stringify(
        { ok: true, root, checks: results, consoleErrors },
        null,
        2,
      ),
    );
  })
  .catch((e) => {
    console.error(e);
    process.exitCode = 1;
  })
  .finally(async () => {
    host?.dispose();
    relay?.stdin.end();
    win?.destroy();
    server?.close();
    if (runtime) await runtime.request({ cmd: "service.stop" }).catch(() => {});
    app.exit(process.exitCode || 0);
  });
setTimeout(() => {
  console.error("smoke timed out");
  app.exit(1);
}, 45000).unref();
