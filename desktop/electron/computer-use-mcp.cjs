const fs = require('node:fs/promises');
const {existsSync} = require('node:fs');
const path = require('node:path');
const {execFile} = require('node:child_process');
const {promisify} = require('node:util');
const {randomUUID} = require('node:crypto');

// No renderer-supplied paths or commands. Only the reviewed catalog generator runs.
function createComputerUseMcp({profileRoot, serviceRoot, platform=process.platform, resourcesPath=process.resourcesPath,
                               run=promisify(execFile)}={}) {
  const development = path.resolve(__dirname, '../../tools/computer-use');
  const source = serviceRoot || (existsSync(path.join(development, 'pyproject.toml'))
    ? development : path.join(resourcesPath || '', 'plugins/computer-use'));
  const state = path.join(profileRoot, 'extensions/computer-use');
  let pending;
  async function prepare() {
    if (platform !== 'win32') throw Error('Computer Use 目前仅支持本机 Windows。');
    const generator = path.join(source, 'src/velella_computer_use/catalog.py');
    const localPython = path.join(source, '.venv/Scripts/python.exe');
    const python = existsSync(localPython) ? localPython : path.join(state, 'venv/Scripts/python.exe');
    try { await fs.access(generator); }
    catch { throw Error('Computer Use 服务文件缺失，请检查应用资源或源码目录。'); }
    try { await fs.access(python); }
    catch {
      const guide = existsSync(path.join(source,'PLUGIN.md')) ? path.join(source,'PLUGIN.md')
        : path.resolve(__dirname,'../plugins/computer-use/README.md');
      throw Error('Computer Use 的 Python 环境尚未准备。请按接入说明创建环境后重试：' + guide);
    }
    try {
      // Import dependencies and locate the package only; never import/run the desktop server.
      await run(python, ['-I','-B','-c',
        "import importlib.util,pathlib,sys; import mcp,PIL,jsonschema; spec=importlib.util.find_spec('velella_computer_use.server'); assert spec and pathlib.Path(spec.origin).resolve()==pathlib.Path(sys.argv[1]).resolve(), 'service package location mismatch'",
        path.join(source,'src/velella_computer_use/server.py')],
        {windowsHide:true,timeout:15000,maxBuffer:65536});
    } catch {
      throw Error('Computer Use 的运行依赖未就绪或环境指向旧目录，请按接入说明重新执行 uv sync 后重试。');
    }
    await fs.mkdir(state, {recursive:true, mode:0o700});
    const temporary = path.join(state, 'catalog.' + randomUUID() + '.tmp');
    const output = path.join(state, 'catalog.json');
    try {
      // -I avoids ambient Python paths; catalog.py uses only the standard library.
      await run(python, ['-I', '-B', generator, '--output', temporary, '--python', python],
                {windowsHide:true, timeout:15000, maxBuffer:65536, env:{...process.env, PYTHONDONTWRITEBYTECODE:'1'}});
      const bytes = await fs.readFile(temporary);
      const generated = JSON.parse(bytes.toString('utf8'));
      if (generated.version !== 1 || generated.entries?.[0]?.id !== 'velella-computer-use-observe'
          || generated.entries?.[1]?.id !== 'velella-computer-use-control'
          || generated.entries.length !== 2 || generated.entries.some(entry => entry.command !== python))
        throw Error('Computer Use 目录生成结果不符合预期。');
      let previous;
      try { previous = await fs.readFile(output); } catch (error) { if (error.code !== 'ENOENT') throw error; }
      if (!previous?.equals(bytes)) await fs.rename(temporary, output);
      return {path:output};
    } finally {
      await fs.unlink(temporary).catch(error => { if (error.code !== 'ENOENT') throw error; });
    }
  }
  return {prepare() { return pending ??= prepare().finally(() => { pending=undefined; }); }};
}
module.exports = {createComputerUseMcp};
