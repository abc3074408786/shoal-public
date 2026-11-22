import assert from 'node:assert/strict';
import {spawn, execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {createHash, randomUUID} from 'node:crypto';
import {existsSync, mkdirSync, readFileSync, statSync, writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';

const require = createRequire(import.meta.url);
const {socketRequest} = require('../electron/runtime.cjs');
assert(process.env.VELELLA_TEST_RUNTIME, 'Pass VELELLA_TEST_RUNTIME explicitly; no production runtime discovery');
const executable = resolve(process.env.VELELLA_TEST_RUNTIME);
const root = fileURLToPath(new URL('../..', import.meta.url));
const evidence = resolve(root, 'artifacts', `worktree-contract-${Date.now()}-${randomUUID()}`);
const data = resolve(evidence, 'isolated-data');
const projectPath = resolve(evidence, '中文 项目');
mkdirSync(data, {recursive: true});
mkdirSync(projectPath);
const git = (...args) => execFileSync('git', ['-C', projectPath, ...args], {
  encoding: 'utf8', windowsHide: true, input: '',
}).trim();
git('init', '--quiet', '--template=', '-b', 'feature');
const tree = git('mktree');
const commit = message => git('-c', 'user.name=Creation Fixture', '-c', 'user.email=creation@example.invalid', 'commit-tree', tree, '-m', message);
const mainOid = commit('main fixture');
const featureOid = commit('feature fixture');
git('update-ref', 'refs/heads/main', mainOid);
git('update-ref', 'refs/heads/feature', featureOid);
const endpoint = JSON.parse(execFileSync(executable, ['ipc-endpoint', data, 'daemon'], {encoding: 'utf8', windowsHide: true}));
const requests = [], daemons = [], receipts = [], observations = [], clientAbsences = [];
const shellProgram = process.platform === 'win32' ? 'powershell' : 'sh';
let daemon, daemonLog = '';
async function raw(request) {
  requests.push(request);
  return socketRequest(endpoint, request);
}
async function rpc(request) {
  const response = await raw(request);
  assert.equal(response.ok, true, JSON.stringify(response));
  return response;
}
async function until(read, accept, label, timeout = 20000) {
  const deadline = Date.now() + timeout;
  for (;;) {
    const value = await read();
    if (accept(value)) return value;
    assert(Date.now() < deadline, `Timed out waiting for ${label}: ${JSON.stringify(value)}`);
    await delay(100);
  }
}
async function start() {
  daemon = spawn(executable, ['daemon', data], {windowsHide: true, stdio: ['ignore', 'pipe', 'pipe']});
  daemons.push(daemon);
  let spawnError;
  daemon.on('error', error => {spawnError = error;});
  daemon.stdout.on('data', chunk => {daemonLog += chunk;});
  daemon.stderr.on('data', chunk => {daemonLog += chunk;});
  await until(async () => {
    if (spawnError) throw spawnError;
    assert.equal(daemon.exitCode, null, 'Isolated daemon exited before becoming ready');
    return socketRequest(endpoint, {cmd: 'workbench.snapshot'}, 500).catch(() => null);
  }, value => value?.ok === true, 'isolated daemon');
}
function counter(path) {
  const file = resolve(path, 'setup-count.txt');
  return existsSync(file) ? readFileSync(file, 'utf8').trim().split('\n').filter(Boolean).length : 0;
}
function setupScript({fail = false, duration = 1500} = {}) {
  const code = `const fs=require('node:fs');fs.appendFileSync('setup-count.txt','run\\n');setTimeout(()=>process.exit(${fail ? "fs.existsSync('allow-retry')?0:7" : '0'}),${duration});`;
  const quote = value => process.platform === 'win32'
    ? `'${value.replaceAll("'", "''")}'`
    : `'${value.replaceAll("'", "'\\''")}'`;
  return `${process.platform === 'win32' ? '& ' : ''}${quote(process.execPath)} -e ${quote(code)}${process.platform === 'win32' ? '; exit $LASTEXITCODE' : ''}`;
}
async function operation(id) { return (await rpc({cmd: 'workbench.creation.status', request_id: id})).creation; }
async function settled(id, expected) {
  const result = await until(() => operation(id), op => ['succeeded', 'skipped', 'failed'].includes(op.steps[1].state), 'setup exit');
  assert.equal(result.steps[1].state, expected);
  receipts.push(result);
  return result;
}
async function rejects(request, pattern) {
  const result = await raw(request);
  assert.equal(result.ok, false, JSON.stringify(result));
  assert.match(String(result.error), pattern);
}
async function sessionsFor(id) { return (await rpc({cmd: 'workbench.sessions', workspace_id: id})).sessions; }
function frozenTerminal(response, script) {
  const {workspace, creation} = response;
  assert.equal(creation.request_id, workspace.id);
  assert.deepEqual(creation.frozen.workspace, workspace);
  assert.equal(resolve(workspace.path), resolve(projectPath, '.velella/worktrees', workspace.id));
  assert.equal(resolve(creation.frozen.project_path), projectPath);
  assert.deepEqual(creation.frozen.base, {reference: 'main', oid: mainOid, inferred: false});
  assert.equal(creation.frozen.branch, `codex/workspace-${workspace.id}`);
  assert.equal(creation.frozen.setup.decision, 'run');
  assert.equal(creation.frozen.setup.script, script);
  const launch = creation.frozen.launch_request;
  assert.match(launch.request_id, /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i);
  assert.notEqual(launch.request_id, creation.request_id);
  assert.deepEqual(launch, {cmd: 'workbench.terminal.open', workspace_id: workspace.id,
    request_id: launch.request_id, launch: 'terminal', launch_options: {}, sandbox: {mode: 'native'}});
  return creation.frozen;
}
function frozenGit(frozen) {
  const run = (...args) => execFileSync('git', ['-C', frozen.workspace.path, ...args], {encoding: 'utf8', windowsHide: true}).trim();
  assert.equal(run('rev-parse', 'HEAD'), frozen.base.oid);
  assert.equal(run('branch', '--show-current'), frozen.branch);
}
function launchRecord(frozen) {
  const path = resolve(projectPath, '.velella/sessions', frozen.launch_request.request_id, 'launch.json');
  return {path, descriptor: JSON.parse(readFileSync(path, 'utf8')), mtimeMs: statSync(path).mtimeMs};
}
async function withoutClient(frozen, label) {
  const requestCount = requests.length;
  const startedAt = new Date().toISOString();
  const path = resolve(projectPath, '.velella/sessions', frozen.launch_request.request_id, 'status.json');
  const status = await until(async () => {
    try { return JSON.parse(readFileSync(path, 'utf8')); }
    catch (error) { if (error.code === 'ENOENT') return null; throw error; }
  }, value => value?.pid > 0 || value?.exit != null, 'shell host evidence without client RPC');
  assert(status.pid > 0, JSON.stringify(status));
  assert.equal(status.exit, null, JSON.stringify(status));
  assert.equal(requests.length, requestCount, 'Disconnected interval must send no RPC');
  clientAbsences.push({label, request_id: frozen.workspace.id, startedAt, endedAt: new Date().toISOString(),
    requestOffset: requestCount, rpcRequests: requests.length - requestCount, statusPath: path, observedStatus: status});
}
async function noTerminal(id, label) {
  const response = await rpc({cmd: 'workbench.creation.status', request_id: id});
  const sessions = await sessionsFor(id);
  observations.push({label, ...response, sessions});
  assert.equal(response.creation.steps[2].state, 'pending');
  assert.equal(response.creation.steps[2].descriptor, null);
  assert(sessions.every(session => session.kind === 'command'), JSON.stringify(sessions));
  assert.equal(sessions.filter(session => session.kind === 'terminal').length, 0);
  return response;
}
async function launched(frozen, setupAttempts, label) {
  const id = frozen.workspace.id;
  const creation = await until(() => operation(id), op => ['succeeded', 'failed'].includes(op.steps[2].state), 'server-owned launch');
  receipts.push(creation);
  assert.equal(creation.steps[1].state, 'succeeded');
  assert.equal(creation.steps[2].state, 'succeeded', JSON.stringify(creation));
  assert.equal(creation.steps[2].attempt, 0);
  assert.equal(creation.steps[3].state, 'skipped');
  assert.deepEqual(creation.frozen, frozen);
  assert.equal(creation.steps[2].descriptor.id, frozen.launch_request.request_id);
  assert.notEqual(creation.steps[2].descriptor.id, id);
  assert.notEqual(creation.steps[2].descriptor.id, creation.steps[1].descriptor.id);
  assert.equal(creation.steps[2].receipt.session_id, frozen.launch_request.request_id);
  assert.equal(creation.steps[2].descriptor.spawn.program, shellProgram);
  assert.equal(resolve(creation.steps[2].descriptor.spawn.cwd), resolve(frozen.workspace.path));
  const sessions = await until(() => sessionsFor(id), rows => rows.some(session =>
    session.id === frozen.launch_request.request_id && session.status?.pid > 0 &&
    session.status.exit === null && session.status.state !== 'host_lost'), 'live local shell');
  observations.push({label, creation, sessions});
  const terminals = sessions.filter(session => session.kind === 'terminal');
  assert.equal(terminals.length, 1);
  assert.equal(terminals[0].id, frozen.launch_request.request_id);
  assert.equal(terminals[0].program, shellProgram);
  assert.equal(terminals[0].execution_mode, 'native');
  assert.equal(terminals[0].task_id, null);
  assert.equal(terminals[0].native_chat, false);
  assert.equal(sessions.length, setupAttempts + 1);
  assert.equal(sessions.filter(session => session.kind === 'command').length, setupAttempts);
  assert.equal(counter(frozen.workspace.path), setupAttempts);
  frozenGit(frozen);
  const record = launchRecord(frozen);
  assert.deepEqual(record.descriptor, creation.steps[2].descriptor);
  return {creation, sessions, record};
}
try {
  await start();
  const project = (await rpc({cmd: 'project.add', path: projectPath})).project;
  const projectId = project.id;
  const script = setupScript();
  await rpc({cmd: 'project.update', project_id: projectId, default_base: 'main', setup_default: 'run', setup: script});
  await rpc({cmd: 'workbench.snapshot'});
  const request = (name, extra = {}) => ({cmd: 'workbench.create', project_id: projectId, request_id: randomUUID(), name, ...extra});
  const skipped = await rpc(request('跳过准备', {setup: 'skip'}));
  await settled(skipped.workspace.id, 'skipped');
  assert.equal(counter(skipped.workspace.path), 0);
  await rejects(request('未审阅', {setup: 'run'}), /审阅/);
  await rejects(request('受限冲突', {setup: 'run', reviewed_setup: script, execution: {mode: 'restricted'}}), /native|skip/);

  const firstRequest = request('冻结与去重', {setup: 'inherit', reviewed_setup: script, parent_workspace_id: `main:${projectId}`});
  const first = await rpc(firstRequest);
  const workspacePath = first.workspace.path;
  const workspaceGit = (...args) => execFileSync('git', ['-C', workspacePath, ...args], {encoding: 'utf8', windowsHide: true}).trim();
  assert.equal(workspaceGit('rev-parse', 'HEAD'), mainOid);
  assert.equal(workspaceGit('branch', '--show-current'), `codex/workspace-${first.workspace.id}`);
  assert.equal(first.workspace.parent_workspace_id, `main:${projectId}`);
  await rejects({cmd: 'workbench.terminal.open', workspace_id: first.workspace.id, launch: 'terminal'}, /准备/);
  await rpc({cmd: 'project.update', project_id: projectId, default_base: 'missing', setup: 'exit 99'});
  const replays = await Promise.all([rpc(firstRequest), rpc(firstRequest), rpc(firstRequest)]);
  assert(replays.every(reply => reply.workspace.id === first.workspace.id));
  await rejects({...firstRequest, name: '同 ID 不同内容'}, /请求|request/);
  const completed = await settled(first.workspace.id, 'succeeded');
  assert.equal(completed.frozen.setup.script, script);
  assert.equal(counter(workspacePath), 1);
  await rpc(firstRequest);
  assert.equal(counter(workspacePath), 1);

  await rpc({cmd: 'project.update', project_id: projectId, default_base: 'main', setup: script});
  const ownedRequest = request('后台独立启动', {setup: 'run', reviewed_setup: script,
    parent_workspace_id: first.workspace.id, launch: 'terminal', launch_options: {}});
  const ownedCreation = await rpc(ownedRequest);
  const ownedFrozen = frozenTerminal(ownedCreation, script);
  assert.equal(ownedCreation.creation.steps[2].state, 'pending');
  await withoutClient(ownedFrozen, 'after single create; client sends no RPC until shell starts');
  frozenGit(ownedFrozen);
  git('update-ref', 'refs/heads/main', featureOid);
  const ownedLaunch = await launched(ownedFrozen, 1, 'reconnected after server-owned launch');
  const ownedRequests = () => requests.filter(req => req.request_id === ownedRequest.request_id || req.workspace_id === ownedRequest.request_id);
  const ownedInitialCreateRequests = ownedRequests().filter(req => req.cmd === 'workbench.create').length;
  assert.equal(ownedInitialCreateRequests, 1);
  assert(ownedRequests().every(req => ['workbench.create', 'workbench.creation.status', 'workbench.sessions'].includes(req.cmd)));
  assert.equal(git('rev-parse', 'refs/heads/main'), featureOid);
  frozenGit(ownedFrozen);
  git('update-ref', 'refs/heads/main', mainOid);

  const failureScript = setupScript({fail: true});
  await rpc({cmd: 'project.update', project_id: projectId, default_base: 'main', setup: failureScript});
  const failureRequest = request('失败后重试', {setup: 'run', reviewed_setup: failureScript,
    parent_workspace_id: first.workspace.id, launch: 'terminal', launch_options: {}});
  const failed = await rpc(failureRequest);
  const failedFrozen = frozenTerminal(failed, failureScript);
  await noTerminal(failed.workspace.id, 'setup has not succeeded');
  const failedSetup = await settled(failed.workspace.id, 'failed');
  assert.equal(failedSetup.steps[1].receipt.exit_code, 7);
  await noTerminal(failed.workspace.id, 'failed setup blocks terminal');
  const retryReady = await until(() => rpc({cmd: 'workbench.creation.status', request_id: failed.workspace.id}), response => response.retry_ready, 'original setup host exit');
  observations.push({label: 'backend confirms failed setup owner is dead', ...retryReady});
  assert.equal(retryReady.retry_ready, true);
  assert.deepEqual(retryReady.creation.steps[1].descriptor, failedSetup.steps[1].descriptor);
  assert.equal(counter(failed.workspace.path), 1);
  await rejects({cmd: 'workbench.terminal.open', workspace_id: failed.workspace.id, launch: 'terminal'}, /准备/);
  await rpc(failureRequest);
  assert.equal(counter(failed.workspace.path), 1);
  await noTerminal(failed.workspace.id, 'replay after owner death does not retry or launch');
  const retry = {cmd: 'workbench.creation.retry', request_id: failed.workspace.id, confirmed: true, reviewed_setup: failureScript};
  await rejects({...retry, confirmed: false}, /确认/);
  await rejects({...retry, reviewed_setup: failureScript + ' '}, /审阅/);
  await noTerminal(failed.workspace.id, 'rejected retry approvals do not launch');
  assert.equal(counter(failed.workspace.path), 1);
  writeFileSync(resolve(failed.workspace.path, 'allow-retry'), 'fixture permission for second setup attempt');
  await rpc({cmd: 'project.update', project_id: projectId, setup: 'exit 98'});
  await rpc(retry);
  await withoutClient(failedFrozen, 'after confirmed retry and owner death; no launch RPC');
  const retried = await settled(failed.workspace.id, 'succeeded');
  assert.equal(retried.steps[1].attempt, 1);
  assert.notEqual(retried.steps[1].descriptor.id, failedSetup.steps[1].descriptor.id);
  assert.equal(counter(failed.workspace.path), 2);
  assert.deepEqual(retried.frozen, failedFrozen);
  const retryLaunch = await launched(failedFrozen, 2, 'terminal only after successful confirmed retry');
  assert(retryLaunch.sessions.some(session => session.id === failedSetup.steps[1].descriptor.id));

  const restartScript = setupScript({duration: 6000});
  await rpc({cmd: 'project.update', project_id: projectId, setup: restartScript});
  const restartRequest = request('守护进程重启', {setup: 'run', reviewed_setup: restartScript});
  const restart = await rpc(restartRequest);
  await until(async () => counter(restart.workspace.path), count => count === 1, 'setup counter before daemon restart');
  const beforeRestart = await operation(restart.workspace.id);
  assert.equal(beforeRestart.steps[1].state, 'running');
  const owned = daemon;
  const exited = new Promise(resolve => owned.once('exit', resolve));
  owned.kill();
  await exited;
  await start();
  const reconnected = await launched(ownedFrozen, 1, 'read-only reconnect after daemon restart');
  assert.deepEqual(reconnected.creation.steps[2], ownedLaunch.creation.steps[2]);
  assert.deepEqual(reconnected.record, ownedLaunch.record, 'Restart must not rewrite launch.json or replace its descriptor');
  const ownedReplay = await rpc(ownedRequest);
  assert.equal(ownedReplay.recovered, true);
  assert.deepEqual(ownedReplay.workspace, ownedCreation.workspace);
  assert.deepEqual(ownedReplay.creation.frozen, ownedFrozen);
  const replayedLaunch = await launched(ownedFrozen, 1, 'same-content replay after daemon restart');
  assert.deepEqual(replayedLaunch.creation.steps[2], ownedLaunch.creation.steps[2]);
  assert.deepEqual(replayedLaunch.record, ownedLaunch.record, 'Replay must not restart the recorded launch');
  assert.equal(ownedRequests().filter(req => req.cmd === 'workbench.create').length, 2);
  assert(ownedRequests().every(req => ['workbench.create', 'workbench.creation.status', 'workbench.sessions'].includes(req.cmd)));
  const retainedRetry = await launched(failedFrozen, 2, 'retried setup terminal retained across daemon restart');
  assert.deepEqual(retainedRetry.creation.steps[2], retryLaunch.creation.steps[2]);
  assert.deepEqual(retainedRetry.record, retryLaunch.record);
  await rpc(restartRequest);
  const resumed = await settled(restart.workspace.id, 'succeeded');
  assert.equal(resumed.steps[1].descriptor.id, beforeRestart.steps[1].descriptor.id);
  assert.equal(resumed.steps[1].attempt, 0);
  assert.equal(counter(restart.workspace.path), 1);
  const snapshot = await rpc({cmd: 'workbench.snapshot'});
  assert.deepEqual(snapshot.workspaces.find(w => w.id === ownedCreation.workspace.id), ownedCreation.workspace);
  assert.equal(snapshot.workspaces.find(w => w.id === failed.workspace.id).parent_workspace_id, first.workspace.id);
  const sessions = (await Promise.all(snapshot.workspaces.map(w => sessionsFor(w.id)))).flat();
  assert.equal(new Set(sessions.map(session => session.id)).size, sessions.length);
  assert(sessions.every(session => ['command', 'terminal'].includes(session.kind) && session.program === shellProgram), 'Only local setup shells and interactive shells may be launched');
  assert.deepEqual(sessions.filter(session => session.kind === 'terminal').map(session => session.id).sort(),
    [ownedFrozen.launch_request.request_id, failedFrozen.launch_request.request_id].sort());
  await rejects({cmd: 'workbench.terminal.recover', workspace_id: first.workspace.id, session_id: completed.steps[1].descriptor.id}, /original task or creation controls/);
  assert.equal((await sessionsFor(first.workspace.id)).length, sessions.filter(session => session.id === completed.steps[1].descriptor.id).length);
  assert.equal(counter(workspacePath), 1);
  const counts = {workspaces: snapshot.workspaces.length, sessions: sessions.length,
    setupAttempts: sessions.filter(session => session.kind === 'command').length,
    setupCounterTotal: [skipped, first, ownedCreation, failed, restart].reduce((sum, response) => sum + counter(response.workspace.path), 0),
    terminalSessions: sessions.filter(session => session.kind === 'terminal').length,
    agentSessions: sessions.filter(session => !['command', 'terminal'].includes(session.kind)).length,
    ownedInitialCreateRequests,
    ownedReplayRequests: ownedRequests().filter(req => req.cmd === 'workbench.create').length - ownedInitialCreateRequests,
    ownedTerminalOpenRequests: ownedRequests().filter(req => req.cmd === 'workbench.terminal.open').length,
    explicitTerminalOpenRequests: requests.filter(req => req.cmd === 'workbench.terminal.open').length,
    disconnectedRpcRequests: clientAbsences.reduce((sum, absence) => sum + absence.rpcRequests, 0)};
  assert.equal(counts.ownedReplayRequests, 1);
  assert.equal(counts.setupAttempts, 5);
  assert.equal(counts.setupCounterTotal, counts.setupAttempts);
  assert.equal(counts.terminalSessions, 2);
  assert.equal(counts.agentSessions, 0);
  assert.equal(counts.ownedTerminalOpenRequests, 0);
  assert.equal(counts.explicitTerminalOpenRequests, 2);
  assert.equal(counts.disconnectedRpcRequests, 0);
  writeFileSync(resolve(evidence, 'result.json'), JSON.stringify({ok: true, executable,
    runtimeSha256: createHash('sha256').update(readFileSync(executable)).digest('hex'), data, projectPath,
    daemonPids: daemons.map(child => child.pid), mainOid, featureOid, receipts, requests, observations, clientAbsences, counts,
    launchRecords: {beforeRestart: ownedLaunch.record, afterRestart: reconnected.record, afterReplay: replayedLaunch.record},
    scope: 'Real isolated local Git, counted PTY setup, and server-owned local shells. One initial create, then filesystem observation with no client RPC, then read-only reconnect; same-content replay only after daemon restart. Two explicit terminal.open calls test preparation rejection only. Client absence is an RPC gap, not a browser/Electron close. Session/descriptor and launch-file retention are checked; no OS PID continuity claim. No agents, auth, UI, SSH, or production profiles.'}, null, 2));
  console.log(JSON.stringify({ok: true, evidence, ...counts}));
} catch (error) {
  writeFileSync(resolve(evidence, 'failure.json'), JSON.stringify({error: String(error), stack: error.stack, requests, receipts, observations, clientAbsences, daemonLog}, null, 2));
  throw error;
} finally {
  if (daemon && daemon.exitCode === null && daemon.signalCode === null) {
    await socketRequest(endpoint, {cmd: 'service.stop'}, 10000).catch(error => {daemonLog += `\nCleanup error: ${error}`;});
  }
  writeFileSync(resolve(evidence, 'daemon.log'), daemonLog);
}
