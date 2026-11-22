// Velella Codex update guard v1. Keep the official CLI and its configuration.
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const {pathToFileURL} = require('node:url');
const {resolveActivePackage} = require('./codex-npm.cjs');
const entry = path.join(resolveActivePackage(path.resolve(__dirname, '..')), 'bin/codex.js');
process.env.PATH = `${__dirname}${path.delimiter}${process.env.PATH || ''}`;
// Velella's per-session hooks/MCP already require embedded mode. Select it
// explicitly when the installed CLI supports it, instead of automatic fallback.
// Other shells, non-interactive commands and remote connections keep their args.
function managedInteractiveOverrides(argv) {
  if (!process.env.VELELLA_ACTIVITY_DIR && !process.env.VELELLA_SESSION_ENDPOINT) return false;
  const values = new Set(['-c','--config','--enable','--disable','-m','--model','-p','--profile','-C','--cd','-s','--sandbox','-a','--ask-for-approval','--local-provider','--add-dir','-i','--image']);
  let overrides = false, interactiveCommand = false;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--') break;
    if (['--no-daemon','--remote','--remote-auth-token-env','--help','-h','--version','-V'].includes(arg) || /^--remote(?:-auth-token-env)?=/.test(arg)) return false;
    if (values.has(arg)) {
      if (++i >= argv.length) return false;
      if (['-c','--config','--enable','--disable'].includes(arg)) overrides = true;
    } else if (arg === '--search' || /^--(?:config|enable|disable)=/.test(arg) || /^-c.+/.test(arg)) overrides = true;
    else if (!arg.startsWith('-') && !interactiveCommand) {
      if (!['resume','fork'].includes(arg)) return false;
      interactiveCommand = true;
    }
  }
  return overrides;
}
if (managedInteractiveOverrides(process.argv.slice(2))) {
  const help = spawnSync(process.execPath, [entry, '--help'], {encoding:'utf8', windowsHide:true, timeout:5000, maxBuffer:128*1024});
  if (help.status === 0 && /^\s*--no-daemon(?:\s|$)/m.test(help.stdout || '')) process.argv.splice(2, 0, '--no-daemon');
}
process.argv[1] = entry;
import(pathToFileURL(entry).href).catch(error => {console.error(error.message); process.exitCode = 1;});
