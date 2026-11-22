// portable-pty prefers conpty.dll beside the runtime. The system ConPTY can
// consume OSC 10/11 without forwarding them to xterm, disabling CLI palettes.
const {createHash} = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const {execFileSync} = require('node:child_process');

const version = '1.24.261001001';
const url = `https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/${version}/microsoft.windows.console.conpty.${version}.nupkg`;
const archiveSha256 = '4d6aaddc1d2385c9f5897df28f33879f699f8f2783315d5204cf3d8c3616ac5f';
const hashes = {
  x64: {'conpty.dll':'5afea6c9480e2c7dfd195376dd10996b0ae25cdc6f014e6247fd8aedc8c34852', 'OpenConsole.exe':'6f8e68dec4e8e5e15a54ecc8afa127b0e62139edc004ffbb4892125ff01d5737'},
  arm64: {'conpty.dll':'015cb55e4f82b0b2bf008dfd98a0398c8239db186b9a6846f66eb918cfe3dd17', 'OpenConsole.exe':'5e4370eb99c591e2e68ed094079b56a16c9c5a00d1b76f684c46c9139439c303'},
};
const license = `Copyright (c) Microsoft Corporation. All rights reserved.

MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED *AS IS*, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
`;
const files = ['conpty.dll', 'OpenConsole.exe', 'conpty-LICENSE.txt', 'conpty-provenance.json'];
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
function directory(arch = process.arch) {
  if (!hashes[arch]) throw Error(`Unsupported ConPTY architecture: ${arch}`);
  return path.resolve(__dirname, '../vendor/conpty', `win32-${arch}`);
}
function verify(folder, arch = process.arch) {
  const expected = hashes[arch];
  if (!expected) throw Error(`Unsupported ConPTY architecture: ${arch}`);
  for (const [name, sha256] of Object.entries(expected)) {
    if (digest(fs.readFileSync(path.join(folder, name))) !== sha256)
      throw Error(`ConPTY integrity check failed: ${name}`);
  }
  if (fs.readFileSync(path.join(folder, 'conpty-LICENSE.txt'), 'utf8') !== license)
    throw Error('ConPTY license missing or changed');
  const provenance = JSON.parse(fs.readFileSync(path.join(folder, 'conpty-provenance.json'), 'utf8'));
  if (provenance.version !== version || provenance.archiveSha256 !== archiveSha256 || provenance.arch !== arch)
    throw Error('ConPTY provenance does not match the pinned package');
  return folder;
}
function resources(arch = process.arch) {
  return files.map(name => ({from:path.join(directory(arch), name), to:`runtime/${name}`}));
}
async function prepare({arch = process.arch, archive, output = directory(arch)} = {}) {
  try { return verify(output, arch); } catch { /* Prepare missing or incomplete generated assets. */ }
  if (process.platform !== 'win32') throw Error('Prepare ConPTY on the target Windows host');
  fs.mkdirSync(output, {recursive:true});
  const cache = archive || path.join(path.dirname(output), `conpty-${version}.nupkg`);
  if (!fs.existsSync(cache)) {
    if (archive) throw Error(`ConPTY archive missing: ${archive}`);
    const response = await fetch(url, {signal:AbortSignal.timeout(30000)});
    if (!response.ok) throw Error(`ConPTY download failed: HTTP ${response.status}`);
    const bytes = Buffer.from(await response.arrayBuffer());
    if (digest(bytes) !== archiveSha256) throw Error('ConPTY archive integrity check failed');
    fs.writeFileSync(cache, bytes, {flag:'wx'});
  }
  if (digest(fs.readFileSync(cache)) !== archiveSha256) throw Error('ConPTY archive integrity check failed');
  const stage = fs.mkdtempSync(path.join(output, 'extract-'));
  try {
    const entries = {'conpty.dll':`runtimes/win-${arch}/native/conpty.dll`, 'OpenConsole.exe':`build/native/runtimes/${arch}/OpenConsole.exe`};
    // Extract only these two known entries; no archive-controlled output paths.
    execFileSync('powershell.exe', ['-NoLogo','-NoProfile','-NonInteractive','-Command',
      '$ErrorActionPreference="Stop"; Add-Type -AssemblyName System.IO.Compression.FileSystem; $zip=[IO.Compression.ZipFile]::OpenRead($env:VELELLA_CONPTY_ARCHIVE); try { $entries=$env:VELELLA_CONPTY_ENTRIES|ConvertFrom-Json; foreach($entry in $entries.PSObject.Properties) { $source=$zip.GetEntry($entry.Value); if(!$source){throw "ConPTY archive entry missing"}; [IO.Compression.ZipFileExtensions]::ExtractToFile($source,(Join-Path $env:VELELLA_CONPTY_STAGE $entry.Name),$false) } } finally { $zip.Dispose() }'],
      {windowsHide:true, timeout:30000, env:{...process.env,VELELLA_CONPTY_ARCHIVE:path.resolve(cache),VELELLA_CONPTY_ENTRIES:JSON.stringify(entries),VELELLA_CONPTY_STAGE:stage}});
    fs.writeFileSync(path.join(stage, 'conpty-LICENSE.txt'), license);
    fs.writeFileSync(path.join(stage, 'conpty-provenance.json'), JSON.stringify({version,url,archiveSha256,arch,files:hashes[arch]},null,2)+'\n');
    verify(stage, arch);
    for (const name of files) fs.copyFileSync(path.join(stage, name), path.join(output, name));
    return verify(output, arch);
  } finally {
    for (const name of files) { const file=path.join(stage,name); if(fs.existsSync(file))fs.unlinkSync(file); }
    fs.rmdirSync(stage);
  }
}
module.exports = {version, url, archiveSha256, hashes, files, directory, verify, resources, prepare};
if (require.main === module) {
  const args = process.argv.slice(2), options = {};
  for (let i=0;i<args.length;i+=2) {
    if (!['--archive','--output','--arch'].includes(args[i]) || !args[i+1]) throw Error('Usage: conpty-runtime.cjs [--archive path] [--output directory] [--arch x64|arm64]');
    options[args[i].slice(2)] = args[i+1];
  }
  prepare(options).then(folder=>console.log(JSON.stringify({version,folder}))).catch(error=>{console.error(error);process.exitCode=1});
}
