// Real production Electron/preload/UI and owned native PTY; no renderer RPC mocks.
// NODE_PATH=<existing Playwright modules> VELELLA_TEST_RUNTIME=D:/orcas/target/debug/velella-runtime.exe node <this file>
import assert from 'node:assert/strict';
import {spawn, execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {createHash, randomUUID} from 'node:crypto';
import {existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, statSync, writeFileSync} from 'node:fs';
import {basename, dirname, resolve} from 'node:path';
import {fileURLToPath, pathToFileURL} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';

const require = createRequire(import.meta.url);
const {_electron} = require('playwright');
// Transport only: no discovery, fallback daemon launch, or production profile access.
const {socketRequest} = require('../electron/runtime.cjs');
assert.equal(process.platform, 'win32', 'This regression verifies Windows native PTY process continuity');
const root = realpathSync(fileURLToPath(new URL('../..', import.meta.url)));
assert.equal(root.toLowerCase(), resolve('D:/orcas').toLowerCase(), 'Unexpected workspace');
assert(process.env.VELELLA_TEST_RUNTIME, 'VELELLA_TEST_RUNTIME must be explicit');
const executable = realpathSync(resolve(process.env.VELELLA_TEST_RUNTIME));
assert.equal(executable, realpathSync(resolve(root, 'target/debug/velella-runtime.exe')), 'Unexpected runtime target');
const electronExecutable = realpathSync(require('electron'));
const entry = resolve(root, 'desktop/electron/main.cjs');
const preload = resolve(root, 'desktop/electron/preload.cjs');
const dist = resolve(root, 'desktop/dist/index.html');
assert(statSync(entry).isFile() && statSync(preload).isFile() && statSync(dist).isFile());
const sha256 = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const runtimeSha256 = sha256(executable);
const artifacts = resolve(root, 'artifacts');
assert(statSync(artifacts).isDirectory(), 'Artifact parent must already exist');
assert.equal(realpathSync(artifacts), artifacts, 'Artifact parent must not redirect');
const evidence = mkdtempSync(resolve(artifacts, 'worktree-creation-native-'));
const data = resolve(evidence, 'data'), parent = resolve(evidence, 'projects');
mkdirSync(data); mkdirSync(parent);
const projectName = `native-${randomUUID().slice(0, 8)}`;
const projectPath = resolve(parent, projectName), worktreeName = '原生创建回归';
const env = Object.fromEntries(Object.entries(process.env).filter(([key]) =>
  !/^VELELLA_/i.test(key) && !['ELECTRON_RUN_AS_NODE', 'NODE_OPTIONS'].includes(key.toUpperCase())));
Object.assign(env, {VELELLA_RUNTIME_PATH: executable, VELELLA_DATA_DIR: data, VELELLA_DESKTOP_BACKGROUND: '0'});
const receipt = {ok: false, evidence, data, parent, projectPath, executable, runtimeSha256,
  electronExecutable, entry, dist, distSha256: sha256(dist), startedAt: new Date().toISOString(),
  scope: 'Actual built Electron window, sandboxed production preload, local native no-Agent terminal, setup skip; no typed shell commands.',
  notTested: ['Real agent CLI/authentication/model turns', 'SSH/remote execution', 'Setup script execution', 'Daemon restart/recovery']};
const consoleMessages = [], pageErrors = [], networkFailures = [], unexpectedRequests = [], verificationRequests = [], cleanupErrors = [];
const cleanup = {appQuit: false, serviceStop: false, retainedArtifacts: true};
let endpoint, daemon, daemonClosed, daemonExit, daemonError, daemonLog = '';
let electronApp, appProcess, appClosed, appExit, appMainPid, appStdout = '', appStderr = '', page, failure;
let tree, session, launchRecord, statusPath, ownedProcesses = [], appWasClosed = false;
const git = (path, ...args) => execFileSync('git', ['-C', path, ...args], {
  encoding: 'utf8', windowsHide: true, timeout: 10000,
}).trim();
const save = (name, value) => writeFileSync(resolve(evidence, name), JSON.stringify(value, null, 2));
function trackProcess(child, onClose) {
  return new Promise(resolve => child.once('close', (code, signal) => {
    onClose({code, signal}); resolve();
  }));
}
async function bounded(promise, label, timeout = 15000) {
  let timer;
  try { return await Promise.race([promise, new Promise((_, reject) => {
    timer = setTimeout(() => reject(Error(`Timed out: ${label}`)), timeout);
  })]); } finally { clearTimeout(timer); }
}
async function until(read, accept, label, timeout = 20000) {
  const deadline = Date.now() + timeout;
  for (;;) {
    const value = await read();
    if (accept(value)) return value;
    assert(Date.now() < deadline, `Timed out: ${label}: ${JSON.stringify(value)}`);
    await delay(100);
  }
}
// Read-only OS queries scoped to exact owned PIDs or their direct children.
// status.pid is the runtime host PID, NOT the shell PID; record both identities.
function processes(ids, children = false) {
  assert(ids.length && ids.every(id => Number.isSafeInteger(id) && id > 0));
  const filter = ids.map(id => `${children ? 'ParentProcessId' : 'ProcessId'} = ${id}`).join(' OR ');
  const script = `$ErrorActionPreference='Stop'; $rows=@(Get-CimInstance Win32_Process -Filter '${filter}' | ForEach-Object { [pscustomobject]@{pid=[int]$_.ProcessId;parentPid=[int]$_.ParentProcessId;name=$_.Name;path=$_.ExecutablePath;commandLine=$_.CommandLine;createdAt=$_.CreationDate.ToUniversalTime().ToString('o')} }); ConvertTo-Json -InputObject $rows -Compress`;
  const stdout = execFileSync(resolve(process.env.SystemRoot, 'System32/WindowsPowerShell/v1.0/powershell.exe'),
    ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')],
    {encoding: 'utf8', windowsHide: true, timeout: 15000});
  // Windows PowerShell 5.1 emits a bare object for a single-element -InputObject array.
  const rows = JSON.parse(stdout.trim() || '[]');
  return Array.isArray(rows) ? rows : (rows && rows.pid ? [rows] : []);
}
function remember(rows) {
  for (const row of rows) {
    if (!ownedProcesses.some(item => item.pid === row.pid && item.createdAt === row.createdAt)) ownedProcesses.push(row);
  }
}
function liveSession(value) {
  assert.equal(value.id, receipt.sessionId);
  assert.equal(value.kind, 'terminal');
  assert.equal(value.program, 'powershell');
  assert.equal(value.execution_mode, 'native');
  assert.equal(value.native_chat, false);
  assert.equal(value.task_id, null);
  assert(value.status?.pid > 0);
  assert.equal(value.status.exit, null);
  assert.notEqual(value.status.state, 'host_lost');
}
const allowed = new Set(['snapshot', 'workbench.snapshot', 'workbench.creation.status', 'workbench.sessions', 'service.stop']);
async function rpc(request) {
  assert(allowed.has(request.cmd), `Unexpected verification command: ${request.cmd}`);
  assert.equal(realpathSync(data), data, 'Isolated data root changed');
  assert(daemon?.pid && !daemonExit, 'Only the live owned daemon may be queried');
  if (request.workspace_id) assert.equal(request.workspace_id, tree?.id);
  if (request.request_id) assert.equal(request.request_id, tree?.id);
  verificationRequests.push({at: new Date().toISOString(), request});
  const response = await socketRequest(endpoint, request, 10000);
  assert.equal(response.ok, true, JSON.stringify(response));
  if (request.cmd === 'snapshot') assert.equal(response.pid, daemon.pid, 'Unexpected daemon identity');
  return response;
}
async function quitApp() {
  if (!electronApp || appWasClosed) return;
  // main.cjs hides window.close(); ElectronApplication.close invokes app.quit instead.
  await bounded(electronApp.close(), 'owned Electron app.quit');
  await bounded(appClosed, 'owned Electron process exit');
  if (appMainPid && appMainPid !== appProcess.pid) {
    await until(async () => processes([appMainPid]), rows => rows.length === 0, 'owned Electron main process exit');
  }
  appWasClosed = true;
  cleanup.appQuit = true;
  cleanup.appExit = appExit;
  if (!appMainPid || appMainPid === appProcess.pid) assert.equal(appExit.code, 0, JSON.stringify(appExit));
}

try {
  endpoint = JSON.parse(execFileSync(executable, ['ipc-endpoint', data, 'daemon'], {
    encoding: 'utf8', windowsHide: true, timeout: 5000, env,
  }));
  assert.equal(typeof endpoint, 'string');
  assert.match(endpoint, /^\\\\\.\\pipe\\velella-[0-9a-f]+-daemon$/);
  receipt.endpoint = endpoint;
  // Refuse an occupied endpoint before starting; never adopt an existing daemon.
  let absent = false;
  try { await socketRequest(endpoint, {cmd: 'snapshot'}, 1000); }
  catch (error) { assert(['ENOENT', 'ECONNREFUSED'].includes(error.code), String(error)); absent = true; }
  assert(absent, 'Fresh isolated endpoint is unexpectedly occupied');
  daemon = spawn(executable, ['daemon', data], {env, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe']});
  receipt.daemonPid = daemon.pid;
  daemon.on('error', error => {daemonError = error;});
  daemon.stdout.on('data', chunk => {daemonLog += chunk;});
  daemon.stderr.on('data', chunk => {daemonLog += chunk;});
  daemonClosed = trackProcess(daemon, value => {daemonExit = value;});
  await until(async () => {
    if (daemonError) throw daemonError;
    assert(!daemonExit, 'Owned daemon exited before readiness');
    try { return await socketRequest(endpoint, {cmd: 'snapshot'}, 1000); }
    catch (error) { if (['ENOENT', 'ECONNREFUSED'].includes(error.code)) return null; throw error; }
  }, value => {
    if (!value) return false;
    assert.equal(value.ok, true); assert.equal(value.pid, daemon.pid, 'Wrong readiness PID'); return true;
  }, 'owned daemon readiness');
  const fresh = await rpc({cmd: 'workbench.snapshot'});
  for (const key of ['projects', 'workspaces', 'tasks']) assert.deepEqual(fresh[key], []);
  const daemonIdentity = processes([daemon.pid]);
  assert.equal(daemonIdentity.length, 1);
  assert.equal(realpathSync(daemonIdentity[0].path), executable);
  assert(daemonIdentity[0].commandLine.includes(data));
  remember(daemonIdentity);

  electronApp = await _electron.launch({executablePath: electronExecutable, args: [entry], env, cwd: root, timeout: 30000});
  appProcess = electronApp.process();
  receipt.appPid = appProcess.pid;
  appClosed = trackProcess(appProcess, value => {appExit = value;});
  appProcess.stdout?.on('data', chunk => {appStdout += chunk;});
  appProcess.stderr?.on('data', chunk => {appStderr += chunk;});
  const attach = target => {
    target.on('console', message => consoleMessages.push({type: message.type(), text: message.text(), location: message.location()}));
    target.on('pageerror', error => pageErrors.push(error.stack || String(error)));
    target.on('requestfailed', request => networkFailures.push({url: request.url(), error: request.failure()?.errorText}));
    target.on('request', request => {
      const url = request.url();
      if (!['file:', 'data:', 'blob:'].some(protocol => url.startsWith(protocol))) unexpectedRequests.push(url);
      if (url.startsWith('file:')) {
        const path = resolve(fileURLToPath(url));
        if (!path.startsWith(resolve(root, 'desktop/dist') + '\\')) unexpectedRequests.push(url);
      }
    });
  };
  const attached = new Set();
  const attachOnce = target => {if (!attached.has(target)) {attached.add(target); attach(target);}};
  electronApp.on('window', attachOnce);
  for (const target of electronApp.windows()) attachOnce(target);
  page = await electronApp.firstWindow();
  attachOnce(page);
  page.setDefaultTimeout(20000);
  await page.waitForURL(pathToFileURL(dist).href);
  receipt.app = await electronApp.evaluate(({app, BrowserWindow}) => {
    const windows = BrowserWindow.getAllWindows();
    return {pid: process.pid, executable: process.execPath, argv: process.argv,
      runtime: process.env.VELELLA_RUNTIME_PATH, data: process.env.VELELLA_DATA_DIR,
      userData: app.getPath('userData'), windows: windows.map(window => ({id: window.id,
        url: window.webContents.getURL(), visible: window.isVisible(), prefs: window.webContents.getLastWebPreferences()}))};
  });
  // Playwright attaches with --inspect, and Electron may re-exec that launcher as its main process.
  // Ownership is proven by the OS process lineage and command line, never by assuming the spawned PID is the main PID.
  appMainPid = receipt.app.pid;
  const lineage = [...processes([appProcess.pid]), ...processes([appProcess.pid], true)];
  receipt.appLaunch = {spawnedPid: appProcess.pid, mainPid: appMainPid,
    lineage: lineage.map(({pid, parentPid, name, commandLine}) => ({pid, parentPid, name, commandLine}))};
  const main = lineage.find(row => row.pid === appMainPid);
  assert(main, 'Electron main process is neither the owned process nor its direct child');
  assert(String(main.commandLine).includes(entry), 'Owned Electron main does not run the expected entry');
  assert.equal(realpathSync(main.path), electronExecutable);
  assert.equal(realpathSync(receipt.app.executable), electronExecutable);
  assert.equal(receipt.app.runtime, executable); assert.equal(receipt.app.data, data);
  assert.equal(resolve(receipt.app.userData), resolve(data, 'electron-profile'));
  assert.equal(receipt.app.windows.length, 1);
  const window = receipt.app.windows[0];
  assert.equal(window.url, pathToFileURL(dist).href);
  assert.equal(window.visible, true);
  assert.equal(window.prefs.sandbox, true);
  assert.equal(window.prefs.contextIsolation, true);
  assert.equal(window.prefs.nodeIntegration, false);
  assert.equal(window.prefs.webSecurity, true);
  receipt.preloadPathReported = window.prefs.preload ?? null;
  if (window.prefs.preload) assert.equal(realpathSync(window.prefs.preload), realpathSync(preload));
  // getLastWebPreferences() may omit the preload path, so the attached script is proven by its own bridge contract.
  receipt.preloadBridge = await page.evaluate(async () => ({
    keys: Object.keys(window.velellaDesktop ?? {}).sort(),
    unknownCommand: await window.velellaDesktop.invoke('not_a_command', {}).then(() => null, error => String(error.message)),
    unknownEvent: (() => { try { window.velellaDesktop.listen('not-an-event', () => {}); return null; } catch (error) { return String(error.message); } })(),
  }));
  assert.deepEqual(receipt.preloadBridge.keys, ['invoke', 'listen']);
  assert.equal(receipt.preloadBridge.unknownCommand, 'Unknown desktop command');
  assert.equal(receipt.preloadBridge.unknownEvent, 'Unknown desktop event');
  receipt.preload = await page.evaluate(() => ({require: typeof require, process: typeof process,
    invoke: typeof window.velellaDesktop?.invoke}));
  assert.deepEqual(receipt.preload, {require: 'undefined', process: 'undefined', invoke: 'function'});
  remember(processes([appProcess.pid, appMainPid]));
  const appChildren = processes([appMainPid], true);
  receipt.appChildren = appChildren; remember(appChildren);

  await page.locator('.ide-project-heading').getByRole('button', {name: '新建工作区', exact: true}).click();
  const projectDialog = page.getByRole('dialog', {name: '创建项目文件夹'});
  await projectDialog.getByLabel('保存位置').fill(parent);
  await projectDialog.getByLabel('项目名称').fill('..');
  const siblingsBefore = readdirSync(evidence).sort();
  assert.deepEqual(readdirSync(parent), []);
  await projectDialog.getByRole('button', {name: '创建项目', exact: true}).click();
  await projectDialog.getByRole('alert').waitFor();
  const invalidError = await projectDialog.getByRole('alert').innerText();
  assert.match(invalidError, /名称|name|非法|无效/i);
  const afterInvalid = await rpc({cmd: 'workbench.snapshot'});
  for (const key of ['projects', 'workspaces', 'tasks']) assert.deepEqual(afterInvalid[key], []);
  assert.deepEqual(readdirSync(parent), []);
  assert.deepEqual(readdirSync(evidence).sort(), siblingsBefore);
  for (const path of [parent, evidence]) {
    assert(!existsSync(resolve(path, '.git')), 'Invalid name initialized Git outside requested project');
    assert(!existsSync(resolve(path, '.velella')), 'Invalid name created runtime metadata outside requested project');
  }
  receipt.invalidName = {name: '..', error: invalidError, resolvedTarget: resolve(parent, '..'),
    parentStillEmpty: true, escapedTargetUnchanged: true, projects: afterInvalid.projects};
  await page.screenshot({path: resolve(evidence, 'invalid-project.png')});

  await projectDialog.getByLabel('项目名称').fill(projectName);
  await projectDialog.getByRole('button', {name: '创建项目', exact: true}).click();
  const composer = page.getByRole('dialog', {name: '新建工作树'});
  await composer.waitFor();
  assert.equal(realpathSync(projectPath), projectPath);
  assert.equal(git(projectPath, 'branch', '--show-current'), 'main');
  receipt.initialCommit = git(projectPath, 'rev-parse', 'HEAD');
  assert.match(receipt.initialCommit, /^[0-9a-f]{40}$/);
  receipt.commonDir = realpathSync(resolve(projectPath, git(projectPath, 'rev-parse', '--git-common-dir')));
  await composer.getByLabel('创建后', {exact: true}).selectOption('terminal');
  assert.equal(await composer.getByLabel('创建后', {exact: true}).inputValue(), 'terminal');
  assert.equal(await composer.getByLabel('智能体运行方式', {exact: true}).count(), 0);
  await composer.locator('summary').click();
  await composer.getByLabel('准备脚本策略', {exact: true}).selectOption('skip');
  assert.equal(await composer.getByLabel('准备脚本策略', {exact: true}).inputValue(), 'skip');
  await composer.locator('summary').click();
  await composer.getByRole('tab', {name: '名称', exact: true}).click();
  await composer.getByRole('textbox', {name: '名称', exact: true}).fill(worktreeName);
  await composer.getByRole('switch', {name: '创建更多'}).uncheck();
  await composer.getByRole('button', {name: '创建并打开', exact: true}).click();
  await composer.waitFor({state: 'hidden', timeout: 30000});
  const snapshot = await rpc({cmd: 'workbench.snapshot'});
  receipt.snapshot = snapshot;
  assert.equal(snapshot.projects.length, 1); assert.deepEqual(snapshot.tasks, []);
  assert.equal(resolve(snapshot.projects[0].path), projectPath);
  assert.deepEqual(readdirSync(parent), [projectName]);
  const trees = snapshot.workspaces.filter(workspace => workspace.kind === 'worktree');
  assert.equal(trees.length, 1);
  tree = trees[0]; receipt.tree = tree;
  assert.equal(tree.name, worktreeName); assert.equal(tree.project_id, snapshot.projects[0].id);
  assert.equal(realpathSync(tree.path), resolve(projectPath, '.velella/worktrees', tree.id));
  receipt.git = {head: git(tree.path, 'rev-parse', 'HEAD'), branch: git(tree.path, 'branch', '--show-current'),
    commonDir: realpathSync(resolve(tree.path, git(tree.path, 'rev-parse', '--git-common-dir'))),
    status: git(tree.path, 'status', '--porcelain')};
  assert.equal(receipt.git.head, receipt.initialCommit);
  assert.equal(receipt.git.branch, `codex/workspace-${tree.id}`);
  assert.equal(receipt.git.commonDir, receipt.commonDir); assert.equal(receipt.git.status, '');
  const {creation} = await rpc({cmd: 'workbench.creation.status', request_id: tree.id});
  receipt.creation = creation;
  assert.equal(creation.request_id, tree.id);
  assert.deepEqual(creation.frozen.workspace, tree);
  assert.equal(resolve(creation.frozen.project_path), projectPath);
  assert.equal(creation.frozen.base.oid, receipt.initialCommit);
  assert.equal(creation.frozen.branch, receipt.git.branch);
  assert.equal(creation.frozen.setup.decision, 'skip');
  assert.equal(creation.request.cmd, 'workbench.create');
  assert.equal(creation.request.name, worktreeName);
  assert.equal(creation.request.request_id, tree.id);
  assert.equal(creation.request.project_id, tree.project_id);
  assert.deepEqual(creation.request.execution, {mode: 'native'});
  assert.equal(creation.request.setup, 'skip'); assert.equal(creation.request.launch, 'terminal');
  assert.deepEqual(creation.request.launch_options, {}); assert.equal(creation.request.initial_prompt, undefined);
  assert.deepEqual(creation.steps.map(({kind, state}) => ({kind, state})), [
    {kind: 'checkout', state: 'succeeded'}, {kind: 'setup', state: 'skipped'},
    {kind: 'launch', state: 'succeeded'}, {kind: 'prompt', state: 'skipped'},
  ]);
  assert(creation.steps.every(step => step.error === null));
  for (const index of [0, 1, 3]) assert.equal(creation.steps[index].descriptor, null);
  const launch = creation.frozen.launch_request;
  receipt.sessionId = launch.request_id;
  assert.match(receipt.sessionId, /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i);
  assert.notEqual(receipt.sessionId, tree.id, 'Creation and launch/session identities must differ');
  assert.deepEqual(launch, {cmd: 'workbench.terminal.open', workspace_id: tree.id,
    request_id: receipt.sessionId, launch: 'terminal', launch_options: {}, sandbox: {mode: 'native'}});
  const descriptor = creation.steps[2].descriptor;
  assert.equal(descriptor.id, receipt.sessionId);
  assert.equal(creation.steps[2].receipt.session_id, receipt.sessionId);
  assert.equal(descriptor.spawn.program, 'powershell');
  assert.deepEqual(descriptor.spawn.args, ['-NoLogo', '-NoProfile']);
  assert.deepEqual(descriptor.spawn.sandbox, {mode: 'native'});
  assert.equal(resolve(descriptor.spawn.cwd), realpathSync(tree.path));
  assert.equal(resolve(descriptor.directory), resolve(projectPath, '.velella/sessions', receipt.sessionId));
  const launchPath = resolve(descriptor.directory, 'launch.json');
  statusPath = resolve(descriptor.directory, 'status.json');
  launchRecord = {path: launchPath, sha256: sha256(launchPath), mtimeMs: statSync(launchPath).mtimeMs};
  assert.deepEqual(JSON.parse(readFileSync(launchPath, 'utf8')), descriptor);
  receipt.launchRecord = launchRecord;
  const sessions = await until(async () => (await rpc({cmd: 'workbench.sessions', workspace_id: tree.id})).sessions,
    rows => rows.length === 1 && rows[0].status?.pid > 0 && rows[0].status?.exit === null,
    'one original live native shell');
  assert.equal(sessions.length, 1);
  session = sessions[0]; liveSession(session);
  receipt.session = session; receipt.sessionHostPid = session.status.pid;
  const host = processes([session.status.pid]);
  assert.equal(host.length, 1); assert.equal(realpathSync(host[0].path), executable);
  // The daemon may hand the host a mixed-separator path; compare the same file identity, not one spelling.
  const canonical = value => String(value).replaceAll('/', '\\').toLowerCase();
  assert(canonical(host[0].commandLine).includes(canonical(launchPath)), 'Host process does not run the recorded launch descriptor');
  remember(host);
  const hostChildren = processes([session.status.pid], true);
  receipt.hostChildren = hostChildren; remember(hostChildren);
  const shells = hostChildren.filter(child => child.name.toLowerCase() === 'powershell.exe');
  assert.equal(shells.length, 1, 'Must identify the actual PTY shell, not mistake status.pid for its PID');
  const shell = shells[0]; receipt.shell = shell; receipt.shellPid = shell.pid;
  assert.equal(shell.parentPid, receipt.sessionHostPid);
  assert.match(shell.commandLine, /-NoProfile/i); assert.match(shell.commandLine, /-NoLogo/i);
  assert(!/-Command|-EncodedCommand/i.test(shell.commandLine), 'No shell script may be injected');
  await page.locator('.terminal-canvas').waitFor();
  await page.screenshot({path: resolve(evidence, 'native-created.png')});

  // A real foreign WebContents uses the unchanged sandboxed production preload.
  // This read-only request must be rejected by the actual main IPC sender guard.
  receipt.untrustedSender = await electronApp.evaluate(async ({BrowserWindow}, preload) => {
    const foreign = new BrowserWindow({show: false, webPreferences: {
      preload, sandbox: true, contextIsolation: true, nodeIntegration: false, webSecurity: true,
    }});
    try {
      await foreign.loadURL('data:text/html,<title>Untrusted sender probe</title>');
      return await foreign.webContents.executeJavaScript(`(async () => {
        try { await window.velellaDesktop.invoke('rpc', {request: {cmd: 'workbench.snapshot'}}); return {denied: false}; }
        catch (error) { return {denied: true, error: String(error), require: typeof require}; }
      })()`);
    } finally { foreign.destroy(); }
  }, preload);
  assert.equal(receipt.untrustedSender.denied, true);
  assert.match(receipt.untrustedSender.error, /Untrusted desktop IPC sender/);
  assert.equal(receipt.untrustedSender.require, 'undefined');

  // Exercise hide-on-close without terminating the process or terminal.
  receipt.hiddenWindow = await electronApp.evaluate(({BrowserWindow}, id) => {
    const window = BrowserWindow.fromId(id); window.close();
    return {destroyed: window.isDestroyed(), visible: window.isVisible()};
  }, window.id);
  assert.deepEqual(receipt.hiddenWindow, {destroyed: false, visible: false});
  assert.equal(processes([appMainPid])[0]?.pid, appMainPid, 'Closing the window terminated the owned Electron main process');
  await quitApp();
  await rpc({cmd: 'snapshot'}); // Assert the original owned daemon, after Electron has exited.
  const afterQuit = await rpc({cmd: 'workbench.sessions', workspace_id: tree.id});
  assert.equal(afterQuit.sessions.length, 1);
  liveSession(afterQuit.sessions[0]);
  assert.equal(afterQuit.sessions[0].status.pid, receipt.sessionHostPid);
  const surviving = processes([receipt.sessionHostPid, receipt.shellPid]);
  assert.equal(surviving.length, 2);
  for (const original of [host[0], shell]) assert.deepEqual(surviving.find(row => row.pid === original.pid), original);
  assert.equal(sha256(launchRecord.path), launchRecord.sha256);
  assert.equal(statSync(launchRecord.path).mtimeMs, launchRecord.mtimeMs);
  const afterCreation = await rpc({cmd: 'workbench.creation.status', request_id: tree.id});
  assert.deepEqual(afterCreation.creation, creation);
  receipt.afterElectronQuit = {at: new Date().toISOString(), daemonPid: daemon.pid,
    appExited: true, sessions: afterQuit.sessions, surviving, originalLaunchRecordUnchanged: true};
  assert.equal(sha256(executable), runtimeSha256, 'Runtime changed during regression');
  assert.deepEqual(pageErrors, []);
  assert.deepEqual(consoleMessages.filter(message => message.type === 'error'), []);
  assert.deepEqual(networkFailures, []); assert.deepEqual(unexpectedRequests, []);
} catch (error) {
  failure = error;
  if (page && !page.isClosed()) await page.screenshot({path: resolve(evidence, 'failure.png')}).catch(() => {});
} finally {
  try { await quitApp(); } catch (error) { cleanupErrors.push(`Electron quit: ${error.stack || error}`); }
  try {
    if (daemon?.pid && !daemonExit) {
      // Revalidate ownership immediately before the only teardown mutation.
      await rpc({cmd: 'snapshot'});
      const snapshot = await rpc({cmd: 'workbench.snapshot'});
      assert(snapshot.projects.every(project => resolve(project.path) === projectPath), 'Refuse cleanup of unexpected project');
      assert.deepEqual(snapshot.tasks, [], 'Refuse cleanup of unexpected agent tasks');
      await rpc({cmd: 'service.stop'});
      cleanup.serviceStop = true;
      await bounded(daemonClosed, 'owned daemon service.stop exit');
      assert.equal(daemonExit.code, 0, JSON.stringify(daemonExit));
    }
    cleanup.daemonExit = daemonExit;
    if (ownedProcesses.length) {
      cleanup.remainingOwnedProcesses = await until(async () => {
        const current = processes([...new Set(ownedProcesses.map(row => row.pid))]);
        return current.filter(row => ownedProcesses.some(original => original.pid === row.pid && original.createdAt === row.createdAt));
      }, rows => rows.length === 0, 'owned app, bridge, daemon, host and shell cleanup');
    }
    if (statusPath && existsSync(statusPath)) {
      cleanup.finalSessionStatus = JSON.parse(readFileSync(statusPath, 'utf8'));
      assert.notEqual(cleanup.finalSessionStatus.exit, null, 'Stopped shell must have durable exit evidence');
    }
    if (endpoint && daemonExit) {
      try { await socketRequest(endpoint, {cmd: 'snapshot'}, 1000); throw Error('Stopped daemon endpoint still responds'); }
      catch (error) { assert(['ENOENT', 'ECONNREFUSED'].includes(error.code), String(error)); }
      cleanup.endpointClosed = true;
    }
  } catch (error) { cleanupErrors.push(`Owned service cleanup: ${error.stack || error}`); }
  writeFileSync(resolve(evidence, 'daemon.log'), daemonLog);
  writeFileSync(resolve(evidence, 'electron.stdout.log'), appStdout);
  writeFileSync(resolve(evidence, 'electron.stderr.log'), appStderr);
}
if (!failure && cleanupErrors.length) failure = Error(cleanupErrors.join('\n'));
receipt.ok = !failure;
Object.assign(receipt, {finishedAt: new Date().toISOString(), cleanup, cleanupErrors, ownedProcesses,
  consoleMessages, pageErrors, networkFailures, unexpectedRequests, verificationRequests,
  error: failure ? failure.stack || String(failure) : null});
save('result.json', receipt);
console.log(JSON.stringify({ok: receipt.ok, evidence, appPid: receipt.appPid,
  appMainPid: receipt.appLaunch?.mainPid, daemonPid: receipt.daemonPid,
  sessionHostPid: receipt.sessionHostPid, shellPid: receipt.shellPid, error: failure?.message}));
if (failure) process.exitCode = 1;
