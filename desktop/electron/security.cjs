const COMMANDS = new Set([
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
  "workspace_open_folder",
  "terminal_performance_export",
  "terminal_switch_result",
  "window",
  "hud",
  "dialog_open",
  "dialog_confirm",
  "notify",
]);
function safeURL(value) {
  const url = new URL(value);
  if (
    !["http:", "https:"].includes(url.protocol) ||
    url.username ||
    url.password
  )
    throw Error("Only HTTP(S) URLs without credentials are allowed");
  return url.href;
}
function trustedSender(event, contents, url) {
  return (
    event.sender === contents &&
    event.senderFrame === contents.mainFrame &&
    event.senderFrame.url === url
  );
}
module.exports = { COMMANDS, safeURL, trustedSender };
