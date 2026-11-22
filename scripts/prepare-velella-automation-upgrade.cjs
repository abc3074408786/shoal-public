// Prepare and verify a new archive. Does not modify the installed app or processes.
const fs = require('node:fs'), path = require('node:path'), crypto = require('node:crypto');
const asar = require(path.resolve('desktop/node_modules/@electron/asar'));
const root = path.resolve('artifacts/velella-automation-management-20261029');
const installed = path.resolve('artifacts/installed-no-codex-chat-20261027/resources/app.asar');
const stage = path.join(root, 'upgrade-stage'), candidate = path.join(root, 'app.upgrade.asar');
const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
function entries(file) {
  const result = new Map();
  function walk(node, prefix = '') {
    for (const [name, stat] of Object.entries(node.files || {})) {
      const key = prefix ? `${prefix}/${name}` : name;
      if (stat.link || stat.unpacked) throw Error(`Unsupported member: ${key}`);
      if (stat.files) walk(stat, key); else result.set(key, stat);
    }
  }
  walk(asar.getRawHeader(file).header);
  return result;
}
(async () => {
  if (fs.existsSync(stage) || fs.existsSync(candidate)) throw Error('Upgrade already prepared; verify existing output before any retry');
  const originalHash = sha(fs.readFileSync(installed));
  if (originalHash !== '0b19fe7896fc4154725b1640804cf758d1ac4fce0d52d83900994a457749287d') throw Error('Installed archive drift');
  const originalMain = asar.extractFile(installed, path.normalize('electron/main.cjs')).toString('utf8');
  const from = '"runtime/terminal-reconnect-20261027"', to = '"runtime/automation-management-20261029"';
  if (originalMain.split(from).length !== 2) throw Error('Runtime path is not unique');
  const patched = originalMain.replace(from, to);
  const before = entries(installed);
  asar.extractAll(installed, stage);
  fs.writeFileSync(path.join(stage, 'electron/main.cjs'), patched);
  await asar.createPackage(stage, candidate);
  const after = entries(candidate), changed = [];
  if (JSON.stringify([...before.keys()].sort()) !== JSON.stringify([...after.keys()].sort())) throw Error('Archive member set changed');
  for (const [name, stat] of before) {
    if (Boolean(stat.executable) !== Boolean(after.get(name).executable)) throw Error(`Executable flag changed: ${name}`);
    if (!asar.extractFile(installed, path.normalize(name)).equals(asar.extractFile(candidate, path.normalize(name)))) changed.push(name);
  }
  if (changed.length !== 1 || changed[0] !== 'electron/main.cjs') throw Error(`Unexpected differences: ${changed}`);
  const browserSha = sha(asar.extractFile(candidate, path.normalize('electron/browser.cjs')));
  if (browserSha !== 'd0c881b4dae023a154c35e180c9a22c3b1ff668e15b3ef3ec4a797df48a704c3') throw Error('Accepted browser fix changed');
  const files = {};
  for (const name of ['velella-runtime.exe','velella-cli.exe','conpty.dll','OpenConsole.exe','conpty-LICENSE.txt','conpty-provenance.json']) {
    files[name] = sha(fs.readFileSync(path.join(root, 'candidate', name)));
  }
  if (files['velella-runtime.exe'] !== '8d7b64d01bc825466a3ff7385d875adfff55d1936088b624b03298638e275c44' || files['velella-cli.exe'] !== '4e0e45670601e48970e02934a1f67bb607425d649886f6ca726269b0309868ba') throw Error('Frozen binary drift');
  const receipt = {installed, candidate, originalSha256:originalHash, candidateSha256:sha(fs.readFileSync(candidate)),
    changedEntries:changed, unchangedEntries:before.size - 1, onlyMainChange:{from,to}, browserSha256:browserSha,
    executableFlagsPreserved:true, files, productionModified:false};
  fs.writeFileSync(path.join(root, 'upgrade-package.json'), JSON.stringify(receipt,null,2));
  console.log(JSON.stringify(receipt,null,2));
})().catch(error => { console.error(error); process.exitCode=1; });
