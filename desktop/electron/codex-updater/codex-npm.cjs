// Velella Codex update guard v1. Ordinary npm commands are delegated unchanged.
const fs = require('node:fs');
const path = require('node:path');
const {spawn} = require('node:child_process');
const {randomUUID} = require('node:crypto');

function packageSpec(argv) {
  if (!['install', 'i'].includes(argv[0]) || argv.length !== 3) return null;
  const rest = argv.slice(1), global = rest.find(arg => ['-g', '--global'].includes(arg));
  const spec = rest.find(arg => /^@openai\/codex(?:@[a-zA-Z0-9.+_-]+)?$/.test(arg));
  return global && spec ? spec : null;
}
const isCodexUpdate = argv => packageSpec(argv) !== null;
const marker = 'Velella Codex update guard v1';
function managedDirectory(directory) {
  fs.mkdirSync(directory, {recursive: true});
  if (fs.realpathSync(directory).toLowerCase() !== path.resolve(directory).toLowerCase()) throw Error('Codex update directory must not redirect outside the npm prefix');
}
function guardedLauncher(content) {
  const text = content.toString();
  if (text.includes('.velella-codex-tools/codex-launch.cjs') || text.includes('.velella-codex-tools\\codex-launch.cjs')) return content;
  if (!/node_modules[\\/]@openai[\\/]codex[\\/]bin[\\/]codex\.js/.test(text)) throw Error('Unrecognized Codex launcher; refusing to replace it');
  return Buffer.from(text.replace(/node_modules([\\/])@openai[\\/]codex[\\/]bin[\\/]codex\.js/g, (_, separator) => `.velella-codex-tools${separator}codex-launch.cjs`));
}
function resolveActivePackage(prefix) {
  const store = path.join(prefix, '.velella-codex-updates'), pointer = path.join(store, 'active.json');
  if (!fs.existsSync(pointer)) return path.join(prefix, 'node_modules/@openai/codex');
  if (fs.lstatSync(pointer).isSymbolicLink()) throw Error('Invalid Codex active pointer');
  const active = JSON.parse(fs.readFileSync(pointer, 'utf8'));
  if (active.schema !== 1 || typeof active.relative !== 'string' || typeof active.version !== 'string') throw Error('Invalid Codex active pointer');
  const directory = path.resolve(store, active.relative);
  if (!/^version-[^/\\]+[/\\]node_modules[/\\]@openai[/\\]codex$/.test(active.relative)
      || !directory.startsWith(path.resolve(store) + path.sep)
      || fs.realpathSync(directory).toLowerCase() !== directory.toLowerCase()) throw Error('Codex active pointer must select a retained version inside this prefix');
  if (readPackage(directory).version !== active.version) throw Error('Codex active pointer version mismatch');
  return directory;
}
function installLauncherTools(prefix, source = __dirname, launcherPrefix = prefix) {
  const current = resolveActivePackage(prefix);
  const available = fs.existsSync(current) ? current : path.join(launcherPrefix, 'node_modules/@openai/codex');
  if (!fs.existsSync(available)) return false;
  readPackage(available);
  const launchers = ['codex', 'codex.cmd', 'codex.ps1'].map(name => {
    const file = path.join(prefix, name), previous = fs.existsSync(file) ? fs.readFileSync(file) : fs.readFileSync(path.join(launcherPrefix, name));
    return {file, previous, next: guardedLauncher(previous)};
  });
  const directory = path.join(prefix, '.velella-codex-tools');
  managedDirectory(directory);
  for (const name of ['npm.cmd', 'npm.ps1', 'codex-npm.cjs', 'codex-launch.cjs']) {
    const file = path.join(directory, name), next = fs.readFileSync(path.join(source, name));
    if (!next.toString().includes(marker)) throw Error('Invalid Codex launcher resource');
    if (fs.existsSync(file) && !fs.readFileSync(file, 'utf8').includes(marker)) throw Error('Unrelated Codex launcher resource already exists');
    const temp = `${file}.${randomUUID()}.tmp`;fs.writeFileSync(temp, next);fs.renameSync(temp, file);
  }
  for (const launcher of launchers) {
    if (launcher.previous.equals(launcher.next)) continue;
    const backup = path.join(directory, `${path.basename(launcher.file)}.original`);
    if (!fs.existsSync(backup)) fs.writeFileSync(backup, launcher.previous, {flag: 'wx'});
    const temp = `${launcher.file}.${randomUUID()}.tmp`;fs.writeFileSync(temp, launcher.next);fs.renameSync(temp, launcher.file);
  }
  return true;
}
function execute(file, args, {env = process.env, cwd, stdout, capture = false, timeout} = {}) {
  return new Promise((resolve, reject) => {
    const piped = capture || typeof stdout === 'function';
    const child = spawn(file, args, {env, cwd, windowsHide: true, stdio: ['inherit', piped ? 'pipe' : 'inherit', 'inherit']});
    let output = '', timer;
    child.once('error', reject);
    if (piped) child.stdout.on('data', bytes => {
      if (capture) output += bytes;
      else stdout(bytes.toString());
      if (capture && output.length > 65536) {child.kill(); reject(Error('CLI response exceeds 64 KiB'));}
    });
    if (timeout) timer = setTimeout(() => {child.kill(); reject(Error('CLI verification timed out'));}, timeout);
    child.once('close', (code, signal) => {clearTimeout(timer); resolve({code: code ?? 1, signal, output});});
  });
}
async function checked(file, args, options) {
  const result = await execute(file, args, options);
  if (result.code !== 0) throw Error(`CLI command failed (exit ${result.code})`);
  return result.output.trim();
}
function findNpmCli(env = process.env, ownDirectory = __dirname) {
  const pathKey = Object.keys(env).find(key => key.toUpperCase() === 'PATH');
  for (const directory of (env[pathKey] || '').split(path.delimiter)) {
    if (!directory || path.resolve(directory).toLowerCase() === path.resolve(ownDirectory).toLowerCase()) continue;
    const cli = path.join(directory.replace(/^"|"$/g, ''), 'node_modules/npm/bin/npm-cli.js');
    if (fs.existsSync(cli)) return cli;
  }
  throw Error('Cannot locate the original npm CLI on PATH; repair the Node.js/npm installation.');
}
function cleanEnvironment(env, ownDirectory = __dirname) {
  const result = {...env}, key = Object.keys(result).find(key => key.toUpperCase() === 'PATH');
  if (key) result[key] = result[key].split(path.delimiter).filter(directory => path.resolve(directory).toLowerCase() !== path.resolve(ownDirectory).toLowerCase()).join(path.delimiter);
  return result;
}
async function runNpm({npmCli, argv, env = process.env, cwd, stdout}) {
  return (await execute(process.execPath, [npmCli, ...argv], {env, cwd, stdout})).code;
}
function readPackage(directory) {
  const pkg = JSON.parse(fs.readFileSync(path.join(directory, 'package.json'), 'utf8'));
  if (pkg.name !== '@openai/codex' || !/^\d+\.\d+\.\d+(?:[-+][\w.-]+)?$/.test(pkg.version)) throw Error('Unexpected Codex package metadata');
  return pkg;
}
async function verify(directory, options) {
  const pkg = readPackage(directory);
  const output = await checked(process.execPath, [path.join(directory, 'bin/codex.js'), '--version'], {...options, capture: true, timeout: 20000});
  if (output !== `codex-cli ${pkg.version}`) throw Error(`Codex version mismatch: ${output}`);
  return pkg.version;
}
function atomicJSON(file, value) {
  const temp = `${file}.${randomUUID()}.tmp`;
  fs.writeFileSync(temp, JSON.stringify(value, null, 2));
  fs.renameSync(temp, file);
}
function recover(journal, current, store) {
  if (!fs.existsSync(journal)) return;
  const prior = JSON.parse(fs.readFileSync(journal, 'utf8'));
  // Format 2 never moves packages: the atomic pointer is authoritative after a crash.
  if (prior.format === 2) return;
  if (typeof prior.backup !== 'string' || path.dirname(prior.backup) !== store || !path.basename(prior.backup).startsWith('previous-')) throw Error('Invalid Codex update recovery record');
  if (!fs.existsSync(current) && fs.existsSync(prior.backup)) fs.renameSync(prior.backup, current);
  if (!fs.existsSync(current) && prior.previousVersion) throw Error('Codex package missing; inspect the retained update recovery record');
}
async function updateCodex({npmCli, argv, env = process.env, cwd, stdout = text => process.stdout.write(text)}) {
  const spec = packageSpec(argv);
  if (!spec) throw Error('Only a standalone global @openai/codex install is supported');
  const options = {env, cwd, stdout};
  const prefixText = await checked(process.execPath, [npmCli, 'prefix', '-g'], {...options, capture: true, timeout: 20000});
  if (!path.isAbsolute(prefixText)) throw Error('npm global prefix is not an absolute path');
  const prefix = fs.realpathSync(prefixText), current = path.join(prefix, 'node_modules/@openai/codex');
  const store = path.join(prefix, '.velella-codex-updates'), journal = path.join(store, 'transaction.json'), lock = path.join(store, 'update.lock');
  managedDirectory(store);
  let handle;
  try {handle = fs.openSync(lock, 'wx');}
  catch (error) {
    if (error.code !== 'EEXIST') throw error;
    const owner = JSON.parse(fs.readFileSync(lock, 'utf8'));
    if (!Number.isInteger(owner.pid) || owner.pid <= 0) throw Error('Invalid Codex update lock; inspect it before retrying');
    try {process.kill(owner.pid, 0);}
    catch (probe) {if (probe.code === 'ESRCH') {fs.unlinkSync(lock); handle = fs.openSync(lock, 'wx');} else throw probe;}
    if (handle === undefined) throw Error('Another Codex update is running; this request was not replayed');
  }
  const token = randomUUID();
  fs.writeFileSync(handle, JSON.stringify({pid: process.pid, token}));
  const pointer = path.join(store, 'active.json');
  let previousPointer = null, transaction, switched = false, retainedFiles;
  try {
    recover(journal, current, store);
    previousPointer = fs.existsSync(pointer) ? JSON.parse(fs.readFileSync(pointer, 'utf8')) : null;
    if (fs.existsSync(current) && fs.lstatSync(current).isSymbolicLink()) throw Error('A linked Codex installation must be updated with its owning package manager');
    const selected = resolveActivePackage(prefix);
    const old = fs.existsSync(selected) ? readPackage(selected) : null;
    const target = JSON.parse(await checked(process.execPath, [npmCli, 'view', spec, 'version', '--json'], {...options, capture: true, timeout: 60000}));
    if (typeof target !== 'string' || !/^\d+\.\d+\.\d+(?:[-+][\w.-]+)?$/.test(target)) throw Error('npm returned an unexpected Codex version');
    if (old?.version === target) {
      await verify(selected, options);
      installLauncherTools(prefix);
      stdout(`Codex ${target} is already installed. Existing sessions keep running; new sessions use this version.\n`);
      return {version: target, unchanged: true};
    }
    const id = randomUUID(), stage = path.join(store, `version-${target}-${id}`);
    transaction = {format: 2, id, current: selected, stage, previousPointer, previousVersion: old?.version, targetVersion: target, phase: 'installing'};
    atomicJSON(journal, transaction);
    stdout(`Installing Codex ${target} alongside the running version…\n`);
    await checked(process.execPath, [npmCli, 'install', '-g', `@openai/codex@${target}`, '--prefix', stage, '--no-audit', '--no-fund'], options);
    const stagedPackage = path.join(stage, 'node_modules/@openai/codex');
    if (await verify(stagedPackage, options) !== target) throw Error('The staged version differs from the requested version');
    // Keep the complete npm prefix (including platform dependencies) at its final path.
    // Only the small pointer changes; no installed package directory is moved.
    const tools = path.join(prefix, '.velella-codex-tools');
    retainedFiles = [...['codex', 'codex.cmd', 'codex.ps1'].map(name => path.join(prefix, name)),
      ...['npm.cmd', 'npm.ps1', 'codex-npm.cjs', 'codex-launch.cjs'].map(name => path.join(tools, name))]
      .map(file => ({file, previous: fs.existsSync(file) ? fs.readFileSync(file) : null}));
    installLauncherTools(prefix, __dirname, stage);
    transaction.phase = 'switching'; atomicJSON(journal, transaction);
    atomicJSON(pointer, {schema: 1, relative: path.relative(store, stagedPackage), version: target}); switched = true;
    transaction.phase = 'complete'; atomicJSON(journal, transaction);
    stdout(`Codex ${target} is ready. Existing sessions were not stopped.\n`);
    return {version: target, directory: stagedPackage, previousDirectory: old ? selected : undefined};
  } catch (error) {
    if (transaction && transaction.phase !== 'complete') {
      if (switched) {
        if (previousPointer) atomicJSON(pointer, previousPointer);
        else fs.unlinkSync(pointer);
      }
      for (const {file, previous} of (retainedFiles || []).reverse()) {
        if (previous !== null) {
          const temp = `${file}.${randomUUID()}.tmp`; fs.writeFileSync(temp, previous); fs.renameSync(temp, file);
        }
        else if (fs.existsSync(file)) fs.unlinkSync(file);
      }
      transaction.phase = 'rolled-back'; transaction.error = error.message; atomicJSON(journal, transaction);
    }
    throw error;
  } finally {
    fs.closeSync(handle);
    if (JSON.parse(fs.readFileSync(lock, 'utf8')).token === token) fs.unlinkSync(lock);
  }
}
async function main(argv) {
  const env = cleanEnvironment(process.env), npmCli = findNpmCli(env);
  if (argv.length === 1 && argv[0] === '--setup-launchers') {
    const prefix = await checked(process.execPath, [npmCli, 'prefix', '-g'], {env, capture: true, timeout: 20000});
    if (!path.isAbsolute(prefix)) throw Error('npm global prefix is not an absolute path');
    console.log(JSON.stringify({installed: installLauncherTools(fs.realpathSync(prefix))})); return 0;
  }
  if (process.platform === 'win32' && isCodexUpdate(argv)) {
    await updateCodex({npmCli, argv, env}); return 0;
  }
  return runNpm({npmCli, argv, env});
}
module.exports = {isCodexUpdate, updateCodex, runNpm, findNpmCli, cleanEnvironment, installLauncherTools, resolveActivePackage};
if (require.main === module) main(process.argv.slice(2)).then(code => {process.exitCode = code;}, error => {console.error(`Velella: ${error.message}`); process.exitCode = 1;});
