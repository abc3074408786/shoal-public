// Real Chromium/CDP + owner IPC + CLI in an owned hidden Electron window.
// Uses only a loopback fixture page; no production profile, accounts or models.
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');
const crypto = require('node:crypto');
const os = require('node:os');
const { spawn, execFile } = require('node:child_process');
const { promisify } = require('node:util');
const { app, BrowserWindow } = require('electron');
const { BrowserHost } = require('../desktop/electron/browser.cjs');
const { createRuntime } = require('../desktop/electron/runtime.cjs');
const { startControl } = require('../desktop/electron/control.cjs');
const argv = process.argv.slice(2);
function option(name) { const at = argv.indexOf(name); if (at < 0 || !argv[at + 1]) throw Error(name + ' required'); return path.resolve(argv[at + 1]); }
const cli = option('--cli'), executable = option('--runtime'), receipt = option('--receipt');
// Keep fixture projects outside this Git checkout: project.add intentionally
// resolves nested folders to their owning repository root.
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'velella-cli-browser-'));
const data = path.join(root, 'data');
const chrome = path.join(root, 'chromium');
fs.mkdirSync(data, { recursive: true });
fs.mkdirSync(chrome, { recursive: true });
app.setPath('userData', chrome);
app.disableHardwareAcceleration();
app.on('window-all-closed', () => {});
const evidence = { cli, cli_sha256: crypto.createHash('sha256').update(fs.readFileSync(cli)).digest('hex'),
  executable, root, checks: [], production_profile_mutated: false, real_model_requests: false,
  scope: 'Real CLI/owner IPC/BrowserHost/hidden Chromium; workbench frame layout supplied by fixture; loopback content only' };
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const run = promisify(execFile);
let daemon, bridge, window, browser, server, runtime;
async function rpc(request) { const reply = await runtime.request(request); if (!reply.ok) throw Error(reply.error); return reply; }
async function invoke(args, ok = true) {
  let stdout, code = 0;
  try { ({ stdout } = await run(cli, ['--data-dir', data, '--timeout-ms', '8000', '--json', ...args], { windowsHide: true, timeout: 12000, encoding: 'utf8' })); }
  catch (error) { stdout = error.stdout; code = error.code; }
  const reply = JSON.parse(String(stdout).replace(/^\uFEFF/, ''));
  if (reply.ok !== ok || (code === 0) !== ok) throw Error(JSON.stringify({ args, reply, code }));
  return ok ? reply.result : reply;
}
async function untilPage(workspace, page, suffix = '') {
  const deadline = Date.now() + 10000;
  for (;;) {
    const info = (await invoke(['browser', 'show', '--workspace', workspace, '--page', page])).page;
    if (info.ready && info.url.endsWith(suffix)) return info;
    if (Date.now() > deadline) throw Error('Page did not become ready: ' + suffix);
    await delay(50);
  }
}
function check(name) { evidence.checks.push(name); }
async function waitExit(child) {
  if (!child || child.exitCode !== null) return;
  await Promise.race([new Promise(resolve => child.once('exit', resolve)), delay(2000)]);
  if (child.exitCode === null) { child.kill(); await Promise.race([new Promise(resolve => child.once('exit', resolve)), delay(2000)]); }
}

(async () => {
  await app.whenReady();
  daemon = spawn(executable, ['daemon', data], { windowsHide: true, stdio: 'ignore', env: { ...process.env, VELELLA_DATA_DIR: data } });
  runtime = createRuntime(executable, data, { launch: () => daemon });
  const project = path.join(root, 'project'), other = path.join(root, 'other');
  fs.mkdirSync(project); fs.mkdirSync(other);
  const added = await rpc({ cmd: 'project.add', path: project });
  const addedOther = await rpc({ cmd: 'project.add', path: other });
  if (!added.project.direct || !addedOther.project.direct) throw Error('Browser fixture must use independent ordinary folders');
  const snapshot = await rpc({ cmd: 'workbench.snapshot' });
  const workspace = snapshot.workspaces.find(w => w.project_id === added.project.id).id;
  const otherWorkspace = snapshot.workspaces.find(w => w.project_id === addedOther.project.id).id;
  window = new BrowserWindow({ show: false, width: 1000, height: 800, webPreferences: { sandbox: true, contextIsolation: true, nodeIntegration: false } });
  await window.loadURL('about:blank');
  browser = new BrowserHost(window, runtime);
  bridge = await startControl(executable, data, async request => {
    const result = await browser.command(request);
    if (result.page?.id && ['create', 'open'].includes(request.op) && request.reveal !== false) {
      browser.layout({ frames: [{ id: result.page.id, workspace_id: result.page.workspace_id, x: 0, y: 0, width: 900, height: 700 }] });
    }
    return result;
  });
  server = http.createServer((_req, response) => {
    response.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
    response.end('<!doctype html><html><head><title>Velella CLI fixture</title></head><body><label>Name<input id="name"></label><button onclick="document.getElementById(\'result\').textContent=document.getElementById(\'name\').value">Apply</button><p id="result">Before</p></body></html>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const url = `http://127.0.0.1:${server.address().port}`;
  if ((await invoke(['browser', 'status'])).backend !== 'electron-chromium') throw Error('Wrong browser backend');
  const page = (await invoke(['browser', 'create', '--workspace', workspace, '--url', url + '/one'])).page.id;
  if ((await invoke(['browser', 'list', '--workspace', workspace])).pages.length !== 1) throw Error('Page was not registered');
  await untilPage(workspace, page, '/one');
  const target = ['--workspace', workspace, '--page', page];
  await invoke(['browser', 'snapshot', '--workspace', otherWorkspace, '--page', page], false);
  check('real owner browser IPC, page create/list/show, workspace ownership rejection');

  let snap = await invoke(['browser', 'snapshot', ...target]);
  if (!snap.content_is_untrusted) throw Error('Snapshot did not label untrusted content');
  const input = snap.nodes.find(node => /textbox|textfield/i.test(node.role) && node.name === 'Name');
  const button = snap.nodes.find(node => node.role === 'button' && node.name === 'Apply');
  if (!input?.ref || !button?.ref) throw Error('Fixture AX controls missing: ' + JSON.stringify(snap.nodes));
  await invoke(['browser', 'fill', ...target, '--snapshot', snap.snapshot_id, '--element', input.ref, '--value', 'Hello fish swarm']);
  const stale = await invoke(['browser', 'click', ...target, '--snapshot', snap.snapshot_id, '--element', button.ref], false);
  if (!stale.error.message.includes('snapshot_stale')) throw Error('Stale snapshot was not rejected');
  snap = await invoke(['browser', 'snapshot', ...target]);
  const freshButton = snap.nodes.find(node => node.role === 'button' && node.name === 'Apply');
  await invoke(['browser', 'click', ...target, '--snapshot', snap.snapshot_id, '--element', freshButton.ref]);
  snap = await invoke(['browser', 'snapshot', ...target]);
  if (!snap.nodes.some(node => node.name === 'Hello fish swarm')) throw Error('Real DOM did not reflect fill/click');
  check('real AX snapshots, fill/click DOM effect, stale-reference rejection without replay');

  const screenshot = path.join(root, 'screenshot.png');
  const image = await invoke(['browser', 'screenshot', ...target, '--out', screenshot]);
  if (image.bytes < 100 || !fs.readFileSync(screenshot).subarray(0, 8).equals(Buffer.from('89504e470d0a1a0a', 'hex'))) throw Error('PNG capture failed');
  await invoke(['browser', 'screenshot', ...target, '--out', screenshot], false);
  evidence.screenshot = screenshot;
  await invoke(['browser', 'context', ...target]);
  await invoke(['browser', 'console', ...target]);
  await invoke(['browser', 'network', ...target]);
  check('real PNG capture with overwrite rejection, browser context/console/network metadata');

  await invoke(['browser', 'goto', ...target, '--url', url + '/two']);
  await untilPage(workspace, page, '/two');
  await invoke(['browser', 'back', ...target]); await untilPage(workspace, page, '/one');
  await invoke(['browser', 'forward', ...target]); await untilPage(workspace, page, '/two');
  await invoke(['browser', 'reload', ...target]); await untilPage(workspace, page, '/two');
  await invoke(['browser', 'close', ...target]);
  if ((await invoke(['browser', 'show', ...target])).page.open) throw Error('View did not close');
  await invoke(['browser', 'snapshot', ...target], false);
  await invoke(['browser', 'open', ...target]); await untilPage(workspace, page, '/two');
  await invoke(['browser', 'forget', ...target, '--confirm']);
  if ((await invoke(['browser', 'list', '--workspace', workspace])).pages.length) throw Error('Page metadata retained after forget');
  check('real navigation/back/forward/reload, view close/reopen and explicit saved-page removal');
  evidence.passed = true;
})().catch(error => { evidence.passed = false; evidence.error = error.stack || String(error); }).finally(async () => {
  try { browser?.dispose(); } catch {}
  try { if (window && !window.isDestroyed()) window.destroy(); } catch {}
  if (bridge) { bridge.stdin.end(); await waitExit(bridge); }
  if (daemon) {
    try { if (daemon.exitCode === null) await rpc({ cmd: 'service.stop' }); } catch {}
    await waitExit(daemon);
  }
  if (server) await new Promise(resolve => server.close(resolve));
  evidence.owned_daemon_stopped = !daemon || daemon.exitCode !== null;
  evidence.owned_bridge_stopped = !bridge || bridge.exitCode !== null;
  evidence.completed_at = new Date().toISOString();
  fs.mkdirSync(path.dirname(receipt), { recursive: true });
  fs.writeFileSync(receipt, JSON.stringify(evidence, null, 2));
  console.log(JSON.stringify(evidence, null, 2));
  app.exit(evidence.passed && evidence.owned_daemon_stopped && evidence.owned_bridge_stopped ? 0 : 1);
});
