#!/usr/bin/env python3
"""Exercise trusted tool aliases in the actual Linux sandbox; no model calls."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

PROBE = r'''
import errno, json, os, pathlib, shutil, socket, subprocess, sys
work, home, outside = map(pathlib.Path, sys.argv[1:])
checks, evidence = {}, {}
def check(name, condition):
    checks[name] = bool(condition)
def denied(name, operation):
    try:
        value = operation()
        if isinstance(value, int):
            os.close(value)
        elif hasattr(value, "close"):
            value.close()
        checks[name] = False
    except OSError as error:
        checks[name] = error.errno in (errno.EACCES, errno.EPERM)
def command(name, argv, expected):
    result = subprocess.run(argv, text=True, capture_output=True, timeout=10)
    checks[name] = result.returncode == 0 and expected in result.stdout
    evidence[name] = {"returncode": result.returncode, "stdout": result.stdout[:1024],
                      "stderr": result.stderr[:2048]}
bin_dir = pathlib.Path(os.environ["PATH"].split(":")[0])
check("private_tool_path", bin_dir == home.with_name(home.name + ".velella-tools-v1"))
check("fixed_system_path_tail", os.environ["PATH"].split(":")[1:] == ["/usr/local/bin", "/usr/bin", "/bin"])
check("host_path_removed", str(outside) not in os.environ["PATH"])
check("secret_environment_removed", all(name not in os.environ for name in
    ("OPENAI_API_KEY", "CODEX_HOME", "HTTP_PROXY", "LD_PRELOAD")))
check("openssl_uses_empty_private_configuration", os.environ.get("OPENSSL_CONF") == "/dev/null")
check("aliases_only_whitelist", set(os.listdir(bin_dir)) <= {"rg", "node", "git", "python3"})
for name in ("rg", "node", "git", "python3"):
    found = shutil.which(name)
    check(name + "_found_via_private_alias", found == str(bin_dir / name))
    if found:
        evidence[name + "_target"] = str(pathlib.Path(found).resolve())
command("rg_version", ["rg", "--version"], "ripgrep ")
command("rg_search", ["rg", "-n", "FIXTURE_MATCH_721", "search.txt"], "2:FIXTURE_MATCH_721")
command("node_works", ["node", "-p", "'NODE_WORKS'"], "NODE_WORKS")
command("git_works", ["git", "--version"], "git version ")
command("python_works", ["python3", "-I", "-c", "print('PYTHON_WORKS')"], "PYTHON_WORKS")
denied("tool_directory_create_denied", lambda: os.open(bin_dir / "injected", os.O_WRONLY | os.O_CREAT, 0o600))
denied("tool_alias_delete_denied", lambda: os.unlink(bin_dir / "rg"))
denied("tool_directory_chmod_denied", lambda: os.chmod(bin_dir, 0o777))
denied("tool_binary_write_denied", lambda: os.open(bin_dir / "rg", os.O_WRONLY))
denied("tool_alias_replace_denied", lambda: os.symlink(work / "replacement", bin_dir / "new-rg"))
denied("outside_auth_open_denied", lambda: os.open(outside / "auth.json", os.O_RDONLY))
denied("outside_directory_list_denied", lambda: os.listdir(outside))
denied("system_directory_write_denied", lambda: os.open("/usr/bin/velella-tools-test-never-create", os.O_WRONLY | os.O_CREAT, 0o600))
for name, domain in (("ipv4", socket.AF_INET), ("ipv6", socket.AF_INET6), ("unix_addressable", socket.AF_UNIX)):
    denied(name + "_socket_denied", lambda domain=domain: socket.socket(domain, socket.SOCK_STREAM))
for name, directory in (("work", work), ("home", home)):
    path = directory / "writable-marker"
    path.write_text(name)
    check(name + "_still_writable", path.read_text() == name)
check("host_path_payload_not_executed", not (work / "host-path-marker").exists())
print(json.dumps({"checks": checks, "evidence": evidence}, ensure_ascii=False))
sys.exit(0 if all(checks.values()) else 1)
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    runtime = args.runtime.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    root = Path(tempfile.mkdtemp(prefix="vl-trusted-tools-"))
    result = {"status": "failed", "real_model_used": False, "checks": {},
              "runtime_sha256": hashlib.sha256(runtime.read_bytes()).hexdigest(),
              "source_sha256": {}, "elapsed_seconds": None}
    project = Path(__file__).resolve().parent.parent
    for relative in ("crates/velella/src/restricted.rs", "crates/velella/src/restricted_tools.rs",
                     "crates/velella/src/restricted_fs.rs", "scripts/verify-restricted-tools.py"):
        result["source_sha256"][relative] = hashlib.sha256((project / relative).read_bytes()).hexdigest()
    started = time.monotonic()
    try:
        work, home, outside = (root / name for name in ("work", "home", "outside"))
        for directory in (work, home, outside):
            directory.mkdir(mode=0o700)
        (work / "search.txt").write_text("first line\nFIXTURE_MATCH_721\nlast line\n")
        (outside / "auth.json").write_text('{"synthetic":"no real credentials"}\n')
        (outside / "auth.json").chmod(0o600)
        (outside / "rg").write_text("#!/bin/sh\ntouch " + str(work / "host-path-marker") + "\n")
        (outside / "rg").chmod(0o700)
        environment = {"PATH": str(outside), "LANG": "C.UTF-8", "OPENAI_API_KEY": "synthetic-only",
                       "CODEX_HOME": str(outside), "HTTP_PROXY": "http://127.0.0.1:9",
                       "LD_PRELOAD": "/synthetic/never-loaded", "OPENSSL_CONF": str(outside / "openssl.cnf")}
        spec = {"policy": {"mode": "restricted", "network": "denied", "home": str(home),
                           "read_only": [], "writable": []},
                "cwd": str(work), "env": environment,
                "operation": {"kind": "process", "argv": [
                    "/usr/bin/python3", "-I", "-c", PROBE, str(work), str(home), str(outside)],
                    "cwd": str(work)}}

        def run(specification):
            # Only pipe stdio into the sandbox; no opened host files are inherited.
            return subprocess.run([str(runtime), "restricted-tool", json.dumps(specification)],
                                  cwd=work, env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"},
                                  capture_output=True, text=True, timeout=40)

        for attempt in ("first", "reuse"):
            completed = run(spec)
            (output / f"{attempt}.stdout.json").write_text(completed.stdout)
            (output / f"{attempt}.stderr.log").write_text(completed.stderr)
            result["checks"][attempt + "_sandbox_exit_zero"] = completed.returncode == 0
            receipt = json.loads(completed.stdout)
            result["checks"].update({attempt + "." + name: passed for name, passed in receipt["checks"].items()})
            if attempt == "first":
                result["tools"] = receipt["evidence"]
            if not all(receipt["checks"].values()):
                raise RuntimeError("sandbox probe failed")
        launch_spec = {"policy": spec["policy"], "cwd": str(work), "env": environment,
                       "program": "rg", "args": ["-n", "FIXTURE_MATCH_721", "search.txt"]}
        launched = subprocess.run([str(runtime), "restricted-exec", json.dumps(launch_spec)],
                                  cwd=work, env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"},
                                  capture_output=True, text=True, timeout=20)
        result["checks"]["restricted_exec_resolves_bare_rg"] = (
            launched.returncode == 0 and launched.stdout == "2:FIXTURE_MATCH_721\n")
        (output / "restricted-exec.stdout.log").write_text(launched.stdout)
        (output / "restricted-exec.stderr.log").write_text(launched.stderr)
        alias = home.with_name(home.name + ".velella-tools-v1") / "rg"
        alias.unlink()
        alias.symlink_to(outside / "rg")
        poisoned = run(spec)
        result["checks"]["substituted_alias_rejected_before_exec"] = (
            poisoned.returncode != 0 and "trusted tool alias was replaced" in poisoned.stderr
            and not poisoned.stdout and not (work / "host-path-marker").exists())
        (output / "substituted-alias.stderr.log").write_text(poisoned.stderr)
        result["checks"]["synthetic_auth_unchanged"] = (
            (outside / "auth.json").read_text() == '{"synthetic":"no real credentials"}\n')
        result["status"] = "passed" if all(result["checks"].values()) else "failed"
    except Exception as error:
        result["failure_type"] = type(error).__name__
    finally:
        shutil.rmtree(root)
        result["checks"]["temporary_tree_removed"] = not root.exists()
        result["elapsed_seconds"] = round(time.monotonic() - started, 3)
        result["check_count"] = len(result["checks"])
        (output / "result.json").write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps({"status": result["status"], "checks": result["check_count"], "result": str(output / "result.json")}))
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
