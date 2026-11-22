const {
  app,
  BrowserWindow,
  ipcMain,
  dialog,
  clipboard,
  nativeImage,
  shell,
  Notification,
  Tray,
  Menu,
} = require("electron");
const { join, resolve, dirname } = require("node:path");
const { homedir } = require("node:os");
const { mkdirSync, existsSync } = require("node:fs");
const { pathToFileURL } = require("node:url");
const { createRuntime } = require("./runtime.cjs");
const { prepareCodexUpdateTools } = require("./codex-update-tools.cjs");
const { activateExisting, startControl } = require("./control.cjs");
const { COMMANDS, safeURL, trustedSender } = require("./security.cjs");
const {
  createExecutionHosts,
  requireLocalTarget,
} = require("./execution-hosts.cjs");
const { BrowserHost } = require("./browser.cjs");
const { createHud } = require("./hud.cjs");
const { createProjectCloner } = require("./project-clone.cjs");
const { createPrChecks } = require("./pr-checks.cjs");
const { createTerminalSwitcher } = require("./terminal-switch.cjs");
const { createWindowVisibility } = require("./window-visibility.cjs");
const { copyTerminalText, readTerminalClipboard } = require("./terminal-clipboard.cjs");
const { exportTerminalPerformance } = require("./terminal-performance-export.cjs");
const { openWorkspaceFolder } = require("./workspace-folder.cjs");
const { createDesktopResources } = require("./desktop-resources.cjs");
const desktopResources = createDesktopResources(app);
const { createResourceManager } = require("./resource-manager.cjs");
const { createResourceSpace } = require("./resource-space.cjs");
const { createExtensions } = require("./extensions.cjs");
const root = resolve(
  process.env.VELELLA_DATA_DIR ||
    join(process.env.LOCALAPPDATA || homedir(), "Velella"),
);
mkdirSync(root, { recursive: true, mode: 0o700 });
app.setName("Velella");
app.setPath("userData", join(root, "electron-profile"));
app.enableSandbox();
let window,
  hud,
  browser,
  tray,
  server,
  quitting = false;
const activate = () => {
  if (window) {
    window.show();
    if (window.isMinimized()) window.restore();
    window.focus();
  }
};
if (!app.requestSingleInstanceLock()) app.quit();
else {
  app.on("second-instance", activate);
  app.on("activate", activate);
  app.on("before-quit", () => {
    quitting = true;
    browser?.dispose();
    server?.stdin.end();
  });
  app
    .whenReady()
    .then(async () => {
      Menu.setApplicationMenu(null);
      app.setAccessibilitySupportEnabled(true);
      const executable =
        process.env.VELELLA_RUNTIME_PATH ||
        (app.isPackaged
          ? join(
              process.resourcesPath,
              "runtime",
              "velella-runtime" + (process.platform === "win32" ? ".exe" : ""),
            )
          : resolve(
              __dirname,
              "../../target/release/velella-runtime" +
                (process.platform === "win32" ? ".exe" : ""),
            ));
      if (!existsSync(executable))
        throw Error("Rust runtime missing: " + executable);
      if (await activateExisting(executable, root)) {
        app.quit();
        return;
      }
      const runtime = createRuntime(executable, root),
        entry = pathToFileURL(resolve(__dirname, "../dist/index.html")).href;
      const projectCloner = createProjectCloner(runtime);
      const resourceManager = createResourceManager(app, runtime);
      const resourceSpace = createResourceSpace(runtime, target => shell.trashItem(target));
      const extensions = createExtensions({profileRoot:root,runtime,skillCli:app.isPackaged?join(process.resourcesPath,'bin/velella-cli'+(process.platform==='win32'?'.exe':'')):join(dirname(executable),'velella-cli'+(process.platform==='win32'?'.exe':'')),ontologyCli:app.isPackaged?join(process.resourcesPath,'plugins/ontology/bin/velella-cli.exe'):resolve(__dirname,'../plugins/ontology/bin/velella-cli.exe'),selectDirectory:async defaultPath=>{const result=await dialog.showOpenDialog(window,{title:'选择插件数据目录',defaultPath,properties:['openDirectory']});return result.canceled?undefined:result.filePaths[0]}});
      const prChecks = createPrChecks(runtime);
      const executionHosts = createExecutionHosts(runtime, {
        confirmConnection: async ({ host, profile, identity }) => {
          const { response } = await dialog.showMessageBox(window, {
            type: "warning",
            title: "确认执行主机",
            message: `允许 Velella 在「${host.name}」上读写项目、运行脚本和启动 Agent？`,
            detail: [
              `SSH 目标：${profile.user ? profile.user + "@" : ""}${profile.host}:${profile.port ?? 22}`,
              `远端系统：${identity.os}，数据根：${identity.data_root}`,
              `服务器身份：${identity.server_id}（runtime ${identity.runtime_version}）`,
              "只有已确认的服务器身份才会接受修改操作；SSH 断开只关闭本机通道，不会停止远端任务。",
              "远端持续运行尚未核验：请先在远端独立登录会话中启动 velella-runtime。",
            ].join("\n"),
            buttons: ["取消", "信任并连接"],
            defaultId: 0,
            cancelId: 0,
            noLink: true,
          });
          return response === 1;
        },
      });
      app.on("before-quit", () => {
        projectCloner.dispose();
        prChecks.dispose();
        executionHosts.dispose();
        void extensions.dispose();
      });
      window = new BrowserWindow({
        width: 1440,
        height: 960,
        minWidth: 800,
        minHeight: 520,
        frame: false,
        show: false,
        backgroundColor: "#151515",
        icon: join(__dirname, "icon.png"),
        webPreferences: {
          preload: join(__dirname, "preload.cjs"),
          // The terminal already controls polling cadence. Chromium's hidden-window
          // timer throttling otherwise turns short output/read steps into seconds.
          backgroundThrottling: false,
          contextIsolation: true,
          nodeIntegration: false,
          sandbox: true,
          webSecurity: true,
        },
      });
      window.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
      window.webContents.on("will-navigate", (e, url) => {
        if (url !== entry) e.preventDefault();
      });
      window.webContents.session.setPermissionRequestHandler((_w, _p, cb) =>
        cb(false),
      );
      window.webContents.session.setPermissionCheckHandler(() => false);
      window.webContents.on("will-attach-webview", (e) => e.preventDefault());
      hud = createHud({
        root,
        preload: join(__dirname, "preload.cjs"),
        isQuitting: () => quitting,
      });
      window.on("close", (e) => {
        if (!quitting) {
          e.preventDefault();
          window.hide();
        }
      });
      window.on("resize", () => window.webContents.send("window-resized", {}));
      const windowVisibility = createWindowVisibility(window);
      browser = new BrowserHost(window, runtime);
      const terminalSwitcher = createTerminalSwitcher({ activate, send: request => window.webContents.send('velella:terminal-switch', request) });
      window.on('closed', () => terminalSwitcher.dispose());
      window.webContents.on('render-process-gone', () => terminalSwitcher.dispose());
      ipcMain.handle("velella:command", async (event, command, args = {}) => {
        if (command === "hud") {
          // Only main may update/toggle; only the existing loaded HUD may
          // drive its controls. Reading the optional window never creates it.
          const fromMain = trustedSender(event, window.webContents, entry);
          const hudWindow = hud.window;
          const fromHud = !!hudWindow && trustedSender(event, hudWindow.webContents, hud.entry);
          if (["update", "toggle"].includes(args.action) ? !fromMain : !fromHud)
            throw Error("Untrusted desktop IPC sender");
          switch (args.action) {
            case "toggle":
              return hud.toggle();
            case "update":
              hud.setDigest(args.digest);
              return;
            case "ready":
              hud.flush();
              return;
            case "hide":
              hud.hide();
              return;
            case "set_height":
              hud.resize(args.height);
              return;
            case "open_task":
              activate();
              window.webContents.send("velella:hud-open-task", {
                taskId: String(args.task_id || ""),
              });
              return;
            case "open_main":
              activate();
              return;
            default:
              throw Error("Unknown hud action");
          }
        }
        if (
          !COMMANDS.has(command) ||
          !trustedSender(event, window.webContents, entry)
        )
          throw Error("Untrusted desktop IPC sender");
        switch (command) {
          case "desktop_extensions":
            requireLocalTarget(args.target);
            return extensions.request(args);
          case "terminal_switch_result":
            return terminalSwitcher.complete(args);
          case "desktop_resources":
            // Process and memory samples come from this machine, so a remote
            // workbench must never present them as its own host's state.
            requireLocalTarget(args.target);
            return desktopResources();
          case "resource_manager":
            requireLocalTarget(args.target);
            return resourceManager();
          case "resource_space_scan":
            requireLocalTarget(args.target);
            return resourceSpace.scan(args);
          case "resource_space_clean":
            requireLocalTarget(args.target);
            return resourceSpace.clean(args);
          case "rpc":
            return executionHosts.request(args.request, args.target);
          case "execution_hosts":
            return executionHosts.manage(args);
          case "project_clone":
            requireLocalTarget(args.target);
            return projectCloner.clone(args.request);
          case "project_clone_cancel":
            return projectCloner.cancel(args.request_id);
          case "pr_checks":
            requireLocalTarget(args.target);
            return prChecks.read(args);
          case "browser_command":
            requireLocalTarget(args.target);
            return browser.command(args.request);
          case "browser_layout":
            requireLocalTarget(args.target);
            return browser.layout(args.request);
          case "terminal_copy":
            return copyTerminalText(clipboard, args.text);
          case "terminal_clipboard":
            requireLocalTarget(args.target);
            return readTerminalClipboard(clipboard, nativeImage, root);
          case "terminal_open_link":
            return shell.openExternal(safeURL(args.url));
          case "workspace_open_folder":
            return openWorkspaceFolder(args,{runtime,shell});
          case "terminal_performance_export":
            requireLocalTarget(args.target);
            return exportTerminalPerformance(args.snapshot, app.getPath("downloads"));
          case "window":
            switch (args.action) {
              case "visibility":
                return windowVisibility.read();
              case "minimize":
                window.minimize();
                break;
              case "toggleMaximize":
                window.isMaximized() ? window.unmaximize() : window.maximize();
                break;
              case "close":
                window.close();
                break;
              case "show":
                window.show();
                break;
              case "setFocus":
                activate();
                break;
              case "isMaximized":
                return window.isMaximized();
              default:
                throw Error("Unknown window action");
            }
            return;
          case "dialog_open": {
            const o = args.options || {},
              r = await dialog.showOpenDialog(window, {
                title: o.title,
                defaultPath: o.defaultPath,
                properties: [
                  o.directory ? "openDirectory" : "openFile",
                  ...(o.multiple ? ["multiSelections"] : []),
                ],
              });
            return r.canceled
              ? null
              : o.multiple
                ? r.filePaths
                : r.filePaths[0];
          }
          case "dialog_confirm":
            return (
              (
                await dialog.showMessageBox(window, {
                  type: "question",
                  message: String(args.message),
                  title: args.options?.title || "确认",
                  buttons: ["取消", "确定"],
                  defaultId: 0,
                  cancelId: 0,
                })
              ).response === 1
            );
          case "notify": {
            const o =
                typeof args.options === "string"
                  ? { title: args.options }
                  : args.options,
              n = new Notification({ title: o.title, body: o.body, silent: o.silent === true });
            n.on("click", () => {
              activate();
              window.webContents.send("notification-action", {
                notification: { extra: o.extra },
              });
            });
            n.show();
            return;
          }
        }
      });
      server = await startControl(executable, root, async (r) =>
        r.op === "activate_main"
          ? (activate(),
            {
              ok: true,
              protocol: "velella-desktop-activation-v1",
              pid: process.pid,
              window: "main",
              visible: true,
              minimized: false,
            })
          : r.op === "terminal_switch" ? terminalSwitcher.request(r) : browser.command(r),
      );
      server.on("exit", () => {
        if (!quitting) {
          console.error("Desktop control relay stopped");
          app.quit();
        }
      });
      tray = new Tray(join(__dirname, "icon.png"));
      tray.setToolTip("Velella");
      tray.setContextMenu(
        Menu.buildFromTemplate([
          { label: "打开 Velella", click: activate },
          { label: "任务监视窗", click: () => {
            void hud.toggle().catch(error => dialog.showErrorBox("任务监视窗打开失败", error.message));
          } },
          { label: "退出桌面（终端继续运行）", click: () => app.quit() },
        ]),
      );
      tray.on("click", activate);
      await window.loadURL(entry);
      if (process.env.VELELLA_DESKTOP_BACKGROUND !== "1") activate();
      // Launcher maintenance may invoke npm or wait for its timeout. Start it
      // after the workbench is visible so it cannot delay opening the desktop.
      void prepareCodexUpdateTools(runtime).catch(error => console.error('Codex update guard setup failed:', error.message));
    })
    .catch((error) => {
      console.error(error);
      dialog.showErrorBox("Velella 启动失败", error.message);
      app.exit(1);
    });
}
