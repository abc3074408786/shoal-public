// Real browser/filesystem smoke. Only the explicitly supplied, isolated daemon is used.
import assert from 'node:assert/strict';
import {spawn, execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {createHash} from 'node:crypto';
import {mkdirSync, mkdtempSync, readFileSync, writeFileSync, realpathSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';
import {createServer} from 'vite';
import react from '@vitejs/plugin-react';

const require = createRequire(import.meta.url);
const {chromium} = require('playwright');
// Import only the socket transport: never use createRuntime's discovery/start fallback.
const {socketRequest} = require('../electron/runtime.cjs');
assert(process.env.VELELLA_TEST_RUNTIME, 'Pass an explicit runtime executable for isolated smoke testing');
const executable = resolve(process.env.VELELLA_TEST_RUNTIME);
const runtimeSha256 = createHash('sha256').update(readFileSync(executable)).digest('hex');
const root = fileURLToPath(new URL('..', import.meta.url));
const evidence = mkdtempSync(resolve(root, `../artifacts/creation-dialog-20261024/runtime-${Date.now()}-`));
const data = resolve(evidence, 'data'), parent = resolve(evidence, 'projects');
mkdirSync(data); mkdirSync(parent);
const projectPath = resolve(parent, 'creation-smoke');
const allowed = new Set([
  'workbench.snapshot', 'project.create', 'workbench.create', 'workbench.creation.status',
  'sandbox.capabilities', 'worktree.list', 'sources.list', 'workbench.layout',
  'workbench.sessions', 'workbench.files', 'workbench.git',
]);
const requests = [], verificationRequests = [], creations = [], gitEvidence = [];
const consoleErrors = [], pageErrors = [], networkFailures = [], rpcErrors = [], escapedRpc = [], cleanupErrors = [];
let daemon, daemonClosed, daemonError, daemonLog = '', daemonExit;
let server, browser, page, receipt, failure;
const git = (path, ...args) => execFileSync('git', ['-C', path, ...args], {
  encoding: 'utf8', windowsHide: true, timeout: 10000,
}).trim();

try {
  const endpoint = JSON.parse(execFileSync(executable, ['ipc-endpoint', data, 'daemon'], {
    encoding: 'utf8', windowsHide: true, timeout: 5000,
  }));
  daemon = spawn(executable, ['daemon', data], {windowsHide: true, stdio: ['ignore', 'pipe', 'pipe']});
  daemon.on('error', error => { daemonError = error; });
  daemon.stdout.on('data', chunk => { daemonLog += chunk; });
  daemon.stderr.on('data', chunk => { daemonLog += chunk; });
  daemonClosed = new Promise(resolve => daemon.once('close', (code, signal) => {
    daemonExit = {code, signal}; resolve();
  }));
  const deadline = Date.now() + 15000;
  for (;;) {
    if (daemonError) throw daemonError;
    assert.equal(daemon.exitCode, null, 'Owned daemon exited before becoming ready');
    let snapshot;
    try { snapshot = await socketRequest(endpoint, {cmd: 'workbench.snapshot'}, 1000); }
    catch (error) { if (Date.now() > deadline) throw error; await delay(100); continue; }
    assert.equal(snapshot.ok, true, JSON.stringify(snapshot));
    assert.deepEqual(snapshot.projects, [], 'Fresh isolated profile must contain no projects');
    assert.deepEqual(snapshot.workspaces, [], 'Fresh isolated profile must contain no workspaces');
    assert.deepEqual(snapshot.tasks, [], 'Fresh isolated profile must contain no tasks');
    break;
  }
  const rpc = async request => {
    assert(allowed.has(request.cmd), 'Unexpected verification RPC: ' + request.cmd);
    verificationRequests.push(request);
    const response = await socketRequest(endpoint, request);
    assert.equal(response.ok, true, JSON.stringify(response));
    return response;
  };
  server = await createServer({
    root, configFile: false, cacheDir: resolve(evidence, 'vite-cache'), logLevel: 'error',
    plugins: [react(), {
      name: 'isolated-runtime-fail-closed',
      configureServer(vite) {
        // If interception is missed, deny the request; no production bridge is loaded.
        vite.middlewares.use('/_velella', (req, res) => {
          escapedRpc.push({method: req.method, url: req.url});
          res.statusCode = 403;
          res.setHeader('Content-Type', 'application/json');
          res.end(JSON.stringify({ok: false, error: 'Isolated browser RPC interception required'}));
        });
      },
    }],
    server: {host: '127.0.0.1', port: 15420, strictPort: false,
      fs: {allow: [root, realpathSync(resolve(root, 'node_modules')), evidence]}},
  });
  await server.listen();
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({channel: 'msedge', headless: true});
  page = await browser.newPage({viewport: {width: 1440, height: 1000}, serviceWorkers: 'block'});
  page.setDefaultTimeout(20000);
  page.on('console', message => {
    if (message.type() === 'error') consoleErrors.push({text: message.text(), location: message.location()});
  });
  page.on('pageerror', error => pageErrors.push(error.stack || error.message));
  page.on('requestfailed', request => networkFailures.push({
    url: request.url(), method: request.method(), error: request.failure()?.errorText,
  }));
  page.on('response', response => {
    if (response.status() >= 400) networkFailures.push({url: response.url(), status: response.status()});
  });
  await page.route('**/*', async route => {
    const request = route.request(), url = new URL(request.url());
    if (url.origin !== origin) {
      networkFailures.push({url: url.href, error: 'Blocked non-test origin'});
      await route.abort('blockedbyclient'); return;
    }
    if (!url.pathname.startsWith('/_velella')) { await route.continue(); return; }
    try {
      assert.equal(url.pathname, '/_velella');
      assert.equal(request.method(), 'POST');
      const req = request.postDataJSON();
      requests.push(req);
      assert(allowed.has(req?.cmd), 'Unexpected RPC: ' + req?.cmd);
      // Check mutation scope before forwarding, including the no-Agent/no-shell contract.
      if (req.cmd === 'project.create') {
        assert.equal(resolve(req.parent), parent);
        assert.equal(req.name, 'creation-smoke');
      }
      if (req.cmd === 'workbench.create') {
        assert.deepEqual(req.execution, {mode: 'native'});
        assert.equal(req.setup, 'skip');
        for (const key of ['launch', 'launch_options', 'initial_prompt']) assert.equal(req[key], undefined);
      }
      const response = await socketRequest(endpoint, req);
      assert.equal(response.ok, true, JSON.stringify(response));
      await route.fulfill({json: response});
    } catch (error) {
      rpcErrors.push(String(error));
      await route.fulfill({status: 500, json: {ok: false, error: String(error)}});
    }
  });
  await page.goto(origin + '/');
  await page.locator('.ide-project-heading').getByRole('button', {name: '新建工作区', exact: true}).click();
  const projectDialog = page.getByRole('dialog', {name: '创建项目文件夹'});
  await projectDialog.getByLabel('项目名称').fill('creation-smoke');
  await projectDialog.getByLabel('保存位置').fill(parent);
  await projectDialog.getByRole('button', {name: '创建项目', exact: true}).click();
  const composer = page.getByRole('dialog', {name: '新建工作树'});
  await composer.waitFor();
  assert.equal(git(projectPath, 'branch', '--show-current'), 'main');
  const initialCommit = git(projectPath, 'rev-parse', 'HEAD');
  assert.match(initialCommit, /^[0-9a-f]{40}$/);
  const commonDir = realpathSync(resolve(projectPath, git(projectPath, 'rev-parse', '--git-common-dir')));
  await composer.getByLabel('创建后', {exact: true}).selectOption('none');
  await composer.locator('summary').click();
  await composer.getByLabel('准备脚本策略', {exact: true}).selectOption('skip');
  await composer.locator('summary').click();
  await composer.getByRole('tab', {name: '名称', exact: true}).click();
  await composer.getByRole('textbox', {name: '名称', exact: true}).fill('真实工作树');
  await composer.getByRole('switch', {name: '创建更多'}).check();
  await composer.getByRole('button', {name: '创建并打开', exact: true}).click();
  await composer.getByRole('status').filter({hasText: '可以继续创建'}).waitFor();
  assert.equal(await composer.getByLabel('创建后', {exact: true}).inputValue(), 'none');
  await composer.getByRole('tab', {name: '名称', exact: true}).click();
  await composer.getByRole('textbox', {name: '名称', exact: true}).fill('第二棵工作树');
  await composer.getByRole('switch', {name: '创建更多'}).uncheck();
  await page.keyboard.press('Control+Enter');
  await composer.waitFor({state: 'hidden'});

  const snapshot = await rpc({cmd: 'workbench.snapshot'});
  const trees = snapshot.workspaces.filter(workspace => workspace.kind === 'worktree');
  assert.equal(snapshot.projects.length, 1);
  assert.deepEqual(snapshot.tasks, []);
  assert.equal(trees.length, 2);
  assert.notEqual(trees[0].id, trees[1].id);
  assert.deepEqual(trees.map(tree => tree.name).sort(), ['真实工作树', '第二棵工作树'].sort());
  for (const tree of trees) {
    assert.equal(tree.project_id, snapshot.projects[0].id);
    assert.equal(resolve(tree.path), resolve(projectPath, '.velella/worktrees', tree.id));
    const actual = {id: tree.id, path: tree.path, head: git(tree.path, 'rev-parse', 'HEAD'),
      branch: git(tree.path, 'branch', '--show-current'),
      commonDir: realpathSync(resolve(tree.path, git(tree.path, 'rev-parse', '--git-common-dir'))),
      status: git(tree.path, 'status', '--porcelain')};
    assert.equal(actual.head, initialCommit);
    assert.equal(actual.branch, 'codex/workspace-' + tree.id);
    assert.equal(actual.commonDir, commonDir);
    assert.equal(actual.status, '');
    gitEvidence.push(actual);
    const {creation} = await rpc({cmd: 'workbench.creation.status', request_id: tree.id});
    assert(creation, 'Missing durable creation journal');
    assert.equal(creation.request_id, tree.id);
    assert.deepEqual(creation.request, requests.find(req => req.cmd === 'workbench.create' && req.request_id === tree.id));
    assert.deepEqual(creation.frozen.workspace, tree);
    assert.equal(creation.frozen.base.oid, initialCommit);
    assert.equal(creation.frozen.branch, actual.branch);
    assert.equal(creation.frozen.setup.decision, 'skip');
    assert.equal(creation.frozen.launch_request, null);
    assert.deepEqual(creation.steps.map(({kind, state}) => ({kind, state})), [
      {kind: 'checkout', state: 'succeeded'}, {kind: 'setup', state: 'skipped'},
      {kind: 'launch', state: 'skipped'}, {kind: 'prompt', state: 'skipped'},
    ]);
    const ready = creation.steps.every(step => ['succeeded', 'skipped'].includes(step.state));
    assert(ready, 'Creation journal must be ready');
    assert(creation.steps.every(step => step.descriptor === null && step.error === null));
    const {sessions} = await rpc({cmd: 'workbench.sessions', workspace_id: tree.id});
    assert.deepEqual(sessions, [], 'No agents, shells or setup processes may be launched');
    creations.push({ready, creation, sessions});
  }
  await page.waitForLoadState('networkidle');
  await page.screenshot({path: resolve(evidence, 'real-created.png')});
  assert.equal(requests.filter(req => req.cmd === 'project.create').length, 1);
  assert.equal(requests.filter(req => req.cmd === 'workbench.create').length, 2);
  for (const errors of [consoleErrors, pageErrors, networkFailures, rpcErrors, escapedRpc]) assert.deepEqual(errors, []);
  receipt = {ok: true, executable, runtimeSha256, daemonPid: daemon.pid, data, origin, projectPath,
    initialCommit, commonDir, trees, gitEvidence, creations,
    scope: 'Real local project and two native no-Agent worktrees through browser UI with isolated daemon. No real SSH/agent CLI or Electron coverage.'};
} catch (error) {
  failure = error;
  await page?.screenshot({path: resolve(evidence, 'failure.png')}).catch(() => {});
} finally {
  // Closing the browser intentionally cancels outstanding requests; stop recording teardown traffic.
  page?.removeAllListeners('requestfailed');
  try { await browser?.close(); } catch (error) { cleanupErrors.push(String(error)); }
  try { await server?.close(); } catch (error) { cleanupErrors.push(String(error)); }
  try {
    if (daemon?.pid && !daemonExit) {
      daemon.kill(); // Only the exact child created above, never a shared service or process-name kill.
      let timer;
      try {
        await Promise.race([daemonClosed, new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error('Owned daemon did not exit')), 5000);
        })]);
      } finally { clearTimeout(timer); }
    }
  } catch (error) { cleanupErrors.push(String(error)); }
  writeFileSync(resolve(evidence, 'daemon.log'), daemonLog);
}
const diagnostics = {requests, verificationRequests, consoleErrors, pageErrors, networkFailures, rpcErrors, escapedRpc, cleanupErrors, daemonExit};
if (!failure) {
  try { for (const errors of [consoleErrors, pageErrors, networkFailures, rpcErrors, escapedRpc, cleanupErrors]) assert.deepEqual(errors, []); }
  catch (error) { failure = error; }
}
if (failure) {
  writeFileSync(resolve(evidence, 'failure.json'), JSON.stringify({ok: false, error: failure.stack || String(failure), ...diagnostics, daemonLog}, null, 2));
  console.error(JSON.stringify({ok: false, evidence}));
  throw failure;
}
writeFileSync(resolve(evidence, 'result.json'), JSON.stringify({...receipt, ...diagnostics}, null, 2));
console.log(JSON.stringify({ok: true, evidence, projectPath, trees: receipt.trees.map(tree => tree.name)}));
