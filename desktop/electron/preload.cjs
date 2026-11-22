const { contextBridge, ipcRenderer } = require("electron");
const commands = new Set([
  "rpc",
  "execution_hosts",
  "desktop_extensions",
  "desktop_resources",
  "resource_manager",
  "resource_space_scan",
  "resource_space_clean",
  "project_clone",
  "project_clone_cancel",
  "pr_checks",
  "browser_command",
  "browser_layout",
  "terminal_copy",
  "terminal_clipboard",
  "terminal_open_link",
  "terminal_performance_export",
  "terminal_switch_result",
  "window",
  "hud",
  "dialog_open",
  "dialog_confirm",
  "notify",
]);
const events = new Set([
  "velella:browser-open",
  "velella:terminal-switch",
  "velella:hud-state",
  "velella:hud-open-task",
  "window-resized",
  "window-visibility",
  "notification-action",
]);
contextBridge.exposeInMainWorld("velellaDesktop", {
  invoke(command, args) {
    if (!commands.has(command))
      return Promise.reject(Error("Unknown desktop command"));
    return ipcRenderer.invoke("velella:command", command, args);
  },
  listen(event, callback) {
    if (!events.has(event)) throw Error("Unknown desktop event");
    const handler = (_event, payload) => callback({ payload });
    ipcRenderer.on(event, handler);
    return () => ipcRenderer.removeListener(event, handler);
  },
});
