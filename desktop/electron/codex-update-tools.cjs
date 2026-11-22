const fs = require('node:fs/promises');
const path = require('node:path');
const {execFile} = require('node:child_process');
const {promisify} = require('node:util');
const execute = promisify(execFile);
const marker = 'Velella Codex update guard v1';
const files = ['npm.cmd', 'npm.ps1', 'codex-npm.cjs', 'codex-launch.cjs'];

async function installTools(runtimeExecutable, source = path.join(__dirname, 'codex-updater')) {
  if (path.basename(runtimeExecutable).toLowerCase() !== 'velella-runtime.exe') throw Error('Unexpected terminal runtime executable');
  const directory = path.dirname(await fs.realpath(runtimeExecutable));
  const writes = [];
  for (const name of files) {
    const file = path.join(directory, name), content = await fs.readFile(path.join(source, name));
    if (!content.toString().includes(marker)) throw Error('Codex updater resource is invalid');
    let previous;
    try {previous = await fs.readFile(file);} catch (error) {if (error.code !== 'ENOENT') throw error;}
    if (previous && !previous.toString().includes(marker)) throw Error(`Cannot replace an unrelated runtime tool: ${file}`);
    if (!previous || !previous.equals(content)) writes.push({file, content});
  }
  for (const {file, content} of writes) {
    const temp = `${file}.${process.pid}.tmp`;
    await fs.writeFile(temp, content);
    await fs.rename(temp, file);
  }
  return directory;
}
async function prepareCodexUpdateTools(runtime, {platform = process.platform, executeCommand = execute} = {}) {
  if (platform !== 'win32') return;
  await installTools(runtime.executable);
  // The npm-generated Codex entry points cover native tabs and Codex started
  // inside PowerShell, even when an older daemon is still serving the desktop.
  // External Node cannot read Electron's app.asar. Use the verified, unpacked
  // runtime resources above, including when the JS caller itself is in asar.
  return executeCommand('node', [path.join(path.dirname(runtime.executable), 'codex-npm.cjs'), '--setup-launchers'], {windowsHide: true, timeout: 30000});
}
module.exports = {installTools, prepareCodexUpdateTools};
