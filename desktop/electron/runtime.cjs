const net = require("node:net");
const { spawn, execFile } = require("node:child_process");
const path = require("node:path");
function socketRequest(endpoint, request, timeout = 50000) {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(endpoint);
    socket.setEncoding("utf8");
    let data = "",
      finished = false;
    const finish = (error, value) => {
      if (finished) return;
      finished = true;
      socket.destroy();
      error ? reject(error) : resolve(value);
    };
    socket.setTimeout(timeout, () =>
      finish(Error("runtime timeout; request was not replayed")),
    );
    socket.on("error", finish);
    socket.on("end", () => {
      if (!finished) finish(Error("runtime connection closed"));
    });
    socket.on("connect", () => socket.write(JSON.stringify(request) + "\n"));
    socket.on("data", (chunk) => {
      data += chunk;
      if (Buffer.byteLength(data) > 16 * 1024 * 1024)
        return finish(Error("runtime response too large"));
      const end = data.indexOf("\n");
      if (end >= 0) {
        try {
          finish(null, JSON.parse(data.slice(0, end)));
        } catch (e) {
          finish(e);
        }
      }
    });
  });
}
function createRuntime(executable, root, { transport, launch } = {}) {
  let endpointPromise;
  const raw =
    transport ||
    (async (request) => {
      if (process.platform !== "win32")
        return socketRequest(path.join(root, "daemon.sock"), request);
      // Resolve Rust's platform-specific pipe name once, then use native pipe I/O
      // for every keystroke/output poll rather than starting a CLI process.
      if (!endpointPromise)
        endpointPromise = new Promise((resolve, reject) => {
          execFile(
            executable,
            ["ipc-endpoint", root, "daemon"],
            { windowsHide: true, timeout: 5000 },
            (error, stdout) => {
              if (error) return reject(error);
              try {
                resolve(JSON.parse(stdout));
              } catch (e) {
                reject(e);
              }
            },
          );
        }).catch((error) => {
          endpointPromise = undefined;
          throw error;
        });
      return socketRequest(await endpointPromise, request);
    });
  let ready = false,
    boot;
  async function ensure() {
    if (ready) return;
    if (boot) return boot;
    boot = (async () => {
      try {
        await raw({ cmd: "snapshot" });
      } catch {
        const child = launch
          ? launch()
          : spawn(executable, ["daemon", root], {
              detached: true,
              stdio: "ignore",
              windowsHide: true,
            });
        let spawnError;
        child.on("error", (e) => {
          spawnError = e;
        });
        child.unref();
        const until = Date.now() + 10000;
        for (;;) {
          if (spawnError) throw spawnError;
          try {
            await raw({ cmd: "snapshot" });
            break;
          } catch (e) {
            if (Date.now() > until) throw e;
            await new Promise((r) => setTimeout(r, 100));
          }
        }
      }
      ready = true;
    })().finally(() => {
      boot = undefined;
    });
    return boot;
  }
  return {
    executable,
    root,
    async request(request) {
      await ensure();
      try {
        return await raw(request);
      } catch (e) {
        ready = false;
        throw e;
      }
    },
  };
}
module.exports = { socketRequest, createRuntime };
