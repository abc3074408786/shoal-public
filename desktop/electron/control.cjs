const { spawn, execFile } = require("node:child_process");
const { promisify } = require("node:util");
const { createInterface } = require("node:readline");
const { socketRequest } = require("./runtime.cjs");
async function endpoint(executable, root) {
  const { stdout } = await promisify(execFile)(
    executable,
    ["ipc-endpoint", root],
    { windowsHide: true, timeout: 5000 },
  );
  return JSON.parse(stdout);
}
async function activateExisting(executable, root) {
  try {
    const reply = await socketRequest(
      await endpoint(executable, root),
      { op: "activate_main" },
      1500,
    );
    if (reply.ok) return true;
    throw Error("Desktop activation refused");
  } catch (e) {
    if (["ENOENT", "ECONNREFUSED"].includes(e.code)) return false;
    throw e;
  }
}
function startControl(executable, root, dispatch) {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, ["desktop-bridge", root], {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    let ready = false,
      errors = "";
    child.stderr.on("data", (chunk) => {
      errors = (errors + chunk).slice(-2000);
    });
    child.on("error", reject);
    child.on("exit", (code) => {
      if (!ready) reject(Error(errors || "Desktop control exited: " + code));
    });
    child.stdin.on("error", () => {});
    const lines = createInterface({ input: child.stdout });
    lines.on("line", async (line) => {
      try {
        const message = JSON.parse(line);
        if (message.ready) {
          ready = true;
          resolve(child);
          return;
        }
        let response;
        try {
          response = await dispatch(message.request);
        } catch (e) {
          response = { ok: false, error: e.message };
        }
        if (!child.stdin.destroyed)
          child.stdin.write(
            JSON.stringify({ id: message.id, response }) + "\n",
          );
      } catch (e) {
        if (!ready) reject(e);
      }
    });
  });
}
module.exports = { activateExisting, startControl };
