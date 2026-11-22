"""Start/reuse the npm OpenTabs bridge without leaking first-run credentials."""
import json
from pathlib import Path
import os
import shutil
import subprocess
import time


def cli_entry():
    located = shutil.which("opentabs")
    node = shutil.which("node")
    if not located or not node:
        raise RuntimeError("Compatible npm-installed opentabs and Node.js are required")
    resolved = Path(located).resolve()
    script = resolved if resolved.suffix == ".js" else resolved.parent / "node_modules/@opentabs-dev/cli/dist/cli.js"
    if not script.is_file():
        raise RuntimeError("Cannot locate the npm CLI entry; inspect the installed executable")
    return node, str(script)


def status_fields(health):
    return {key: health.get(key) for key in ["version", "extensionConnections", "mcpClients",
                                           "browserToolCount", "pluginToolCount", "skipPermissions"]}


def main():
    try:
        node, script = cli_entry()
    except RuntimeError:
        print(json.dumps({"ok": False, "reason": "runtime_unavailable"}))
        return 1
    env = dict(os.environ, OPENTABS_TELEMETRY_DISABLED="1", DO_NOT_TRACK="1")
    env.pop("OPENTABS_DANGEROUSLY_SKIP_PERMISSIONS", None)

    def run(args, timeout=10):
        return subprocess.run([node, script, *args], env=env, capture_output=True,
                              encoding="utf-8", errors="replace", timeout=timeout,
                              creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))

    def health():
        done = run(["status", "--json"])
        if done.returncode:
            return None
        try:
            return json.loads(done.stdout)
        except ValueError:
            return None

    try:
        current = health()
        reused = current is not None
        if current is None:
            started = run(["start", "--background"], timeout=40)
            # Captured output may contain real local tokens. Never display/store it.
            if started.returncode:
                print(json.dumps({"ok": False, "reason": "startup_failed_inspect_diagnostics"}))
                return 1
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                current = health()
                if current is not None:
                    break
                time.sleep(0.5)
        if current is None:
            print(json.dumps({"ok": False, "reason": "health_not_ready_do_not_restart_blindly"}))
            return 1
        if current.get("skipPermissions"):
            print(json.dumps({"ok": False, "reason": "existing_permission_bypass_enabled"}))
            return 1
        print(json.dumps({"ok": True, "reused": reused, "health": status_fields(current)}))
        return 0
    except (OSError, subprocess.TimeoutExpired):
        print(json.dumps({"ok": False, "reason": "runtime_error_inspect_diagnostics"}))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
