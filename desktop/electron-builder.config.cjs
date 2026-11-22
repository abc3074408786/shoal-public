const conpty = require('./scripts/conpty-runtime.cjs');
module.exports = {
  appId: "dev.velella.desktop",
  productName: "Velella",
  asar: true,
  directories: { output: "release-electron" },
  files: [
    "dist/**/*",
    "electron/**/*",
    "plugins/ontology/service.cjs",
    "plugins/ontology/manifest.json",
    "plugins/skill-center/service.cjs",
    "plugins/skill-center/manifest.json",
    "package.json",
    "!electron/smoke.cjs",
    "!node_modules/**/*",
  ],
  extraResources: [
    {
      from: "../target/release/velella-cli" + (process.platform === "win32" ? ".exe" : ""),
      to: "bin/velella-cli" + (process.platform === "win32" ? ".exe" : ""),
    },
    {
      from:
        "../target/release/velella-runtime" +
        (process.platform === "win32" ? ".exe" : ""),
      to:
        "runtime/velella-runtime" +
        (process.platform === "win32" ? ".exe" : ""),
    },
    { from: "electron/browser-agent", to: "runtime/browser-agent" },
    { from: "plugins/ontology/bin/velella-cli.exe", to: "plugins/ontology/bin/velella-cli.exe" },
    ...(process.platform === 'win32' ? [{from:'../tools/computer-use',to:'plugins/computer-use',filter:['src/**/*.py','pyproject.toml','uv.lock','README.md']},
      {from:'plugins/computer-use/README.md',to:'plugins/computer-use/PLUGIN.md'}] : []),
    ...(process.platform === 'win32' ? [{from: 'electron/codex-updater', to: 'runtime', filter: ['**/*']}] : []),
    ...(process.platform === 'win32' ? conpty.resources() : []),
  ],
  beforePack: async (context) => {
    const pluginEngine=require('node:path').resolve(__dirname,'plugins/ontology/bin/velella-cli.exe');
    if(!require('node:fs').existsSync(pluginEngine))throw Error('Bundled ontology engine missing; restore the reviewed engine before packaging');
    if(require('node:crypto').createHash('sha256').update(require('node:fs').readFileSync(pluginEngine)).digest('hex')!==require('./plugins/ontology/manifest.json').engine.sha256)throw Error('Bundled ontology engine fingerprint mismatch');
    const cli = require("node:path").resolve(__dirname, "../target/release/velella-cli" +
      (process.platform === "win32" ? ".exe" : ""));
    if (!require("node:fs").existsSync(cli))
      throw Error("Velella CLI missing; run npm run build:cli before packaging");
    const runtime = require("node:path").resolve(__dirname, "../target/release/velella-runtime" +
      (process.platform === "win32" ? ".exe" : ""));
    if (!require("node:fs").existsSync(runtime))
      throw Error("Velella runtime missing; run npm run build:cli before packaging");
    if (context.electronPlatformName !== process.platform)
      throw Error(
        "Build on the target OS: the Rust runtime must match the desktop platform",
      );
    if (context.arch !== require("builder-util").Arch[process.arch])
      throw Error("Build the Rust runtime for the target CPU first");
    if (context.electronPlatformName === 'win32') await conpty.prepare();
  },
  linux: {
    target: ["AppImage"],
    syncDesktopName: true,
    icon: "src-tauri/icons/icon.png",
    category: "Development",
    executableName: "velella-desktop",
  },
  win: { target: ["nsis"], icon: "src-tauri/icons/icon.ico" },
  mac: {
    target: ["dmg"],
    icon: "src-tauri/icons/icon.icns",
    category: "public.app-category.developer-tools",
  },
  nsis: { oneClick: false, allowToChangeInstallationDirectory: true },
};
