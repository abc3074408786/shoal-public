#!/usr/bin/env python3
"""No-model integration checks for the experimental restricted exec-server adapter."""
import argparse
import base64
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import queue
import select
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
from unittest import mock


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def require(condition, text):
    if not condition:
        raise AssertionError(text)


class Client:
    def __init__(self, adapter, config):
        self.process = subprocess.Popen(
            ["/usr/bin/python3", "-I", str(adapter), "--config", str(config)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"}, start_new_session=True)
        self.pidfd = os.pidfd_open(self.process.pid)
        self.cleanup_start = Path(f"/proc/{self.process.pid}/stat").read_text().rsplit(
            ") ", 1)[1].split()[19]
        self.cleanup_path = Path(config).parent / f"exec-server-cleanup-{self.process.pid}.json"
        self.events = []
        self.queue = queue.Queue(maxsize=256)
        self.read_enabled = threading.Event()
        self.read_enabled.set()
        self.stderr = bytearray()
        self.sequence = 0

        def read():
            try:
                while True:
                    self.read_enabled.wait()
                    line = self.process.stdout.readline(2 * 1024 * 1024 + 1)
                    if not line:
                        break
                    self.queue.put(json.loads(line), timeout=1)
            except Exception as error:
                self.queue.put(error)

        def errors():
            while block := self.process.stderr.read(8192):
                if len(self.stderr) < 65536:
                    self.stderr.extend(block[:65536 - len(self.stderr)])
        self.reader = threading.Thread(target=read, daemon=True)
        self.error_reader = threading.Thread(target=errors, daemon=True)
        self.reader.start()
        self.error_reader.start()

    def send(self, method, params):
        self.sequence += 1
        self.process.stdin.write(json.dumps(
            {"id": self.sequence, "method": method, "params": params}).encode() + b"\n")
        self.process.stdin.flush()
        return self.sequence

    def rpc(self, method, params, timeout=12):
        request_id = self.send(method, params)
        deadline = time.monotonic() + timeout
        while True:
            message = self.queue.get(timeout=max(.01, deadline - time.monotonic()))
            if isinstance(message, Exception):
                raise message
            self.events.append(message)
            if message.get("id") == request_id:
                return message
            require(time.monotonic() < deadline, f"{method} timed out")

    def ok(self, method, params):
        reply = self.rpc(method, params)
        require("result" in reply, f"{method}: {reply}")
        return reply["result"]

    def start(self, process_id, argv, cwd, env=None):
        return self.ok("process/start", {"processId": process_id, "argv": argv,
                                       "cwd": cwd.as_uri(), "env": env or {}, "tty": False})

    def wait_closed(self, process_id):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            result = self.ok("process/read", {"processId": process_id, "waitMs": 100})
            if result["closed"]:
                return result
            time.sleep(.03)
        raise TimeoutError(f"{process_id} did not close")

    def close(self, terminate=False):
        self.read_enabled.set()
        if not self.process.stdin.closed:
            self.process.stdin.close()
        if terminate and not select.select([self.pidfd], [], [], 0)[0]:
            signal.pidfd_send_signal(self.pidfd, signal.SIGTERM)
        try:
            self.process.wait(timeout=6)
        except subprocess.TimeoutExpired:
            signal.pidfd_send_signal(self.pidfd, signal.SIGKILL)
            self.process.wait(timeout=3)
            raise
        self.reader.join(timeout=.5)
        self.error_reader.join(timeout=.5)
        require(self.cleanup_path.is_file(), "adapter did not confirm complete cleanup")
        require(json.loads(self.cleanup_path.read_text()) == {
            "schema": 1, "pid": self.process.pid, "start": self.cleanup_start, "clean": True},
            "cleanup receipt must match the pinned adapter identity")
        metadata = self.cleanup_path.lstat()
        require(stat.S_ISREG(metadata.st_mode) and stat.S_IMODE(metadata.st_mode) == 0o600
                and metadata.st_uid == os.geteuid() and metadata.st_nlink == 1,
                "cleanup receipt must be a private single-link regular file")


TREE = r'''
import os, pathlib, signal, time
root = pathlib.Path({root!r})
signal.signal(signal.SIGHUP, signal.SIG_IGN)
signal.signal(signal.SIGTERM, signal.SIG_IGN)
root.joinpath("parent.pid").write_text(str(os.getpid()))
child = os.fork()
if child == 0:
    root.joinpath("child.pid").write_text(str(os.getpid()))
    grand = os.fork()
    if grand == 0:
        root.joinpath("grand.pid").write_text(str(os.getpid()))
    while True: time.sleep(.1)
while not root.joinpath("grand.pid").exists(): time.sleep(.01)
print("TREE_READY", flush=True)
if {natural!r}:
    time.sleep(1)
else:
    while True: time.sleep(.1)
'''


def capture_tree(directory, descriptors):
    deadline = time.monotonic() + 5
    names = ("parent.pid", "child.pid", "grand.pid")
    while not all((directory / name).exists() for name in names):
        require(time.monotonic() < deadline, "owned tree was not ready")
        time.sleep(.02)
    result = []
    for name in names:
        pid = int((directory / name).read_text())
        fd = os.pidfd_open(pid)
        descriptors.append(fd)
        result.append({"pid": pid, "fd": fd})
    return result


def exited(tree, timeout=6):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if all(select.select([item["fd"]], [], [], 0)[0] for item in tree):
            return True
        time.sleep(.03)
    return False


def filesystem_boundaries(client, owned, checks):
    """Exercise the confined operations through their actual JSONL interface."""
    work = owned / "work"
    special = work / "目录 space % # ? ü.txt"
    payload = b"\x00\xff\x80binary\r\n" + "中文内容".encode()
    client.ok("fs/writeFile", {"path": special.as_uri(),
                              "dataBase64": base64.b64encode(payload).decode()})
    read = client.ok("fs/readFile", {"path": special.as_uri()})
    require(base64.b64decode(read["dataBase64"]) == payload,
            "escaped Unicode URI and binary payload round trip")
    checks["escaped_unicode_uri_and_binary_file"] = True

    link = work / "unicode-link"
    link.symlink_to(special.name)
    followed = client.ok("fs/getMetadata", {"path": link.as_uri()})
    unlinked = client.ok("fs/getMetadata", {"path": link.as_uri(), "followSymlinks": False})
    require(followed["isFile"] and not followed["isDirectory"] and not followed["isSymlink"]
            and followed["size"] == len(payload), f"followed metadata: {followed}")
    require(unlinked["isSymlink"] and not unlinked["isFile"] and not unlinked["isDirectory"],
            f"symlink metadata: {unlinked}")
    require(all(isinstance(followed[key], int) and followed[key] > 0
                for key in ("createdAtMs", "modifiedAtMs")), "metadata timestamps")
    for method, extra in (
        ("fs/readFile", {}),
        ("fs/writeFile", {"dataBase64": "eA=="}),
    ):
        reply = client.rpc(method, {"path": link.as_uri(), "followSymlinks": False, **extra})
        require("error" in reply, f"{method} must reject final symlink with nofollow")
    require(special.read_bytes() == payload, "nofollow write changed symlink target")
    require(client.ok("fs/canonicalize", {"path": link.as_uri()})["path"] == special.as_uri(),
            "canonicalize must return escaped, resolved URI")
    entries = {entry["fileName"]: entry for entry in
               client.ok("fs/readDirectory", {"path": work.as_uri()})["entries"]}
    require(entries[special.name]["isFile"] and not entries[special.name]["isDirectory"],
            "readDirectory regular file type")
    require(entries["sub"]["isDirectory"] and not entries["sub"]["isFile"],
            "readDirectory directory type")
    require(not entries[link.name]["isFile"] and not entries[link.name]["isDirectory"],
            "readDirectory must not follow symlinks")
    checks["metadata_symlink_nofollow_and_directory_types"] = True

    nested = work / "created/parent/leaf"
    client.ok("fs/createDirectory", {"path": nested.as_uri(), "recursive": True})
    client.ok("fs/createDirectory", {"path": nested.as_uri(), "recursive": True})
    require(nested.is_dir(), "recursive mkdir did not create parents")
    require("error" in client.rpc("fs/createDirectory", {"path": nested.as_uri()}),
            "nonrecursive mkdir must reject existing directory")
    sentinel = nested / "keep"
    sentinel.write_bytes(payload)
    for recursive in (True, False):
        reply = client.rpc("fs/remove", {"path": nested.as_uri(), "recursive": recursive})
        require("error" in reply and sentinel.read_bytes() == payload,
                "unsupported recursive or nonempty directory removal changed contents")
    client.ok("fs/remove", {"path": sentinel.as_uri()})
    client.ok("fs/remove", {"path": nested.as_uri()})
    require(not nested.exists(), "empty directory removal failed")
    client.ok("fs/remove", {"path": nested.as_uri(), "force": True})
    missing = client.rpc("fs/remove", {"path": nested.as_uri()})
    require(missing.get("error", {}).get("code") == -32004, "missing remove errno")
    missing = client.rpc("fs/canonicalize", {"path": nested.as_uri()})
    require(missing.get("error", {}).get("code") == -32004,
            "canonicalize must fail for nonexistent target")
    client.ok("fs/remove", {"path": link.as_uri()})
    require(not link.is_symlink() and special.read_bytes() == payload,
            "removing a symlink must preserve its target")
    checks["mkdir_canonicalize_and_nonrecursive_remove"] = True

    fifo = work / "bounded-fifo"
    os.mkfifo(fifo, 0o600)
    for method, extra in (
        ("fs/readFile", {}),
        ("fs/writeFile", {"dataBase64": "eA=="}),
    ):
        started = time.monotonic()
        reply = client.rpc(method, {"path": fifo.as_uri(), **extra}, timeout=4)
        require("error" in reply and time.monotonic() - started < 4,
                f"{method} must reject FIFO without blocking")
        require(stat.S_ISFIFO(fifo.stat().st_mode), "FIFO must remain intact after failed operation")
    checks["fifo_read_and_write_fail_without_blocking"] = True

    oversized = work / "too-large"
    oversized.write_bytes(b"z" * (1024 * 1024 + 1))
    reply = client.rpc("fs/readFile", {"path": oversized.as_uri()})
    require("error" in reply, "oversized file read must fail")
    for data in (base64.b64encode(oversized.read_bytes()).decode(), "invalid%%base64"):
        reply = client.rpc("fs/writeFile", {"path": special.as_uri(), "dataBase64": data})
        require("error" in reply and special.read_bytes() == payload,
                "invalid or oversized write must preserve existing file")
    checks["oversized_read_and_invalid_write_preserve_files"] = True

    for name, target in (("outside-link", owned / "outside"), ("readonly-link", owned / "ro/file")):
        escape = work / name
        escape.symlink_to(target)
        original = target.read_bytes()
        reply = client.rpc("fs/writeFile", {"path": escape.as_uri(), "dataBase64": "eA=="})
        require(reply.get("error", {}).get("code") == -32600 and target.read_bytes() == original,
                f"{name} write must be denied without truncating target: {reply}")
        if name == "outside-link":
            reply = client.rpc("fs/readFile", {"path": escape.as_uri()})
            require(reply.get("error", {}).get("code") == -32600,
                    f"outside symlink content must be denied: {reply}")
        else:
            read = client.ok("fs/readFile", {"path": escape.as_uri()})
            require(base64.b64decode(read["dataBase64"]) == original, "read-only symlink read")
    checks["symlink_escape_denied_without_truncation"] = True


def cleanup_receipt_boundaries(adapter, root, checks):
    """Fault injection exercises cases which real successful cleanup cannot cover."""
    spec = importlib.util.spec_from_file_location("restricted_exec_cleanup_test", adapter)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    directory = root / "cleanup-unit"
    directory.mkdir(mode=0o700)
    alias = root / "cleanup-alias"
    alias.symlink_to(directory, target_is_directory=True)
    require(module.private_cleanup_directory(alias) is None, "symlink state must not be pinned")
    directory.chmod(0o755)
    require(module.private_cleanup_directory(directory) is None, "nonprivate state cannot authorize resume")
    directory.chmod(0o700)
    descriptor = module.private_cleanup_directory(directory)
    require(descriptor is not None, "private state must be pinned")
    checks["cleanup_requires_private_real_directory"] = True
    try:
        pid, start = os.getpid(), "123456"
        path = directory / f"exec-server-cleanup-{pid}.json"
        require(module.write_cleanup_receipt(descriptor, pid, start), "receipt creation failed")
        expected = {"schema": 1, "pid": pid, "start": start, "clean": True}
        require(json.loads(path.read_text()) == expected
                and stat.S_IMODE(path.stat().st_mode) == 0o600, "private receipt fields")
        require(not module.write_cleanup_receipt(descriptor, pid, "999")
                and json.loads(path.read_text()) == expected, "receipt must be create-new")
        path.unlink()
        target = directory / "unrelated"
        target.write_text("unchanged")
        for kind in ("symlink", "hardlink"):
            if kind == "symlink":
                path.symlink_to(target)
            else:
                os.link(target, path)
            require(not module.write_cleanup_receipt(descriptor, pid, start)
                    and target.read_text() == "unchanged", f"{kind} must not be overwritten")
            path.unlink()
        checks["cleanup_receipt_exclusive_private_identity"] = True
        for operation in ("write", "file_fsync", "directory_fsync"):
            original_fsync = module.os.fsync

            def fail_sync(fd):
                if ((operation == "file_fsync" and fd != descriptor)
                        or (operation == "directory_fsync" and fd == descriptor)):
                    raise OSError("injected durability failure")
                return original_fsync(fd)

            if operation == "write":
                patch = mock.patch.object(module.os, "write", side_effect=OSError("injected write failure"))
            else:
                patch = mock.patch.object(module.os, "fsync", side_effect=fail_sync)
            with patch:
                require(not module.write_cleanup_receipt(descriptor, pid, start),
                        "failed write/fsync must not confirm cleanup")
            require(not path.exists(), "failed receipt must be removed")
        directory.chmod(0o755)
        require(not module.write_cleanup_receipt(descriptor, pid, start) and not path.exists(),
                "state permissions must be rechecked when publishing")
        directory.chmod(0o700)
        checks["cleanup_write_and_fsync_failures_leave_no_receipt"] = True
    finally:
        os.close(descriptor)

    class Thread:
        def __init__(self, alive=False):
            self.alive = alive

        def is_alive(self):
            return self.alive

        def join(self, timeout=None):
            pass

    def close_case(name, *, closed=True, active=False, reaper=False, reader=False, writer=False,
                   failure=None, uncertain=False):
        state = directory / name
        state.mkdir(mode=0o700)
        process = module.OwnedProcess.__new__(module.OwnedProcess)
        process.lock = threading.RLock()
        process.gate = threading.Event()
        process.closed, process.active, process.failure = closed, active, failure
        process.reaper = Thread(reaper)
        process.readers = [Thread(reader), Thread()]
        process.kill = mock.Mock()
        server = module.Server.__new__(module.Server)
        server.cleanup_finished = False
        server.cleanup_uncertain = uncertain
        server.cleanup_directory = module.private_cleanup_directory(state)
        server.cleanup_pid, server.cleanup_start = os.getpid(), "123456"
        server.owned = [process]
        server.output_bytes = 0
        server.output_done = threading.Event()
        server.writer = Thread(writer)
        server.close()
        server.close()  # A second call must not reopen or replace the receipt.
        require(process.gate.is_set() and process.kill.call_count == 1, "close must cancel exactly once")
        return (state / f"exec-server-cleanup-{os.getpid()}.json").exists()

    for name, args in (
        ("not-closed", {"closed": False}),
        ("active", {"active": True}),
        ("reaper-live", {"reaper": True}),
        ("reader-live", {"reader": True}),
        ("writer-live", {"writer": True}),
        ("partial-startup", {"uncertain": True}),
    ):
        require(not close_case(name, **args), f"{name} cannot confirm cleanup")
    require(close_case("output-limit", failure="process output limit exceeded"),
            "a fully cleaned output-limit failure must still confirm cleanup")
    require(close_case("success"), "fully cleaned processes must confirm cleanup")
    checks["cleanup_incomplete_process_or_threads_refuse_receipt"] = True
    checks["cleanup_command_failure_is_not_cleanup_failure"] = True


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, default=repo / "target/debug/velella-runtime")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    require(not output.exists() or not any(output.iterdir()), "output must be new/empty")
    output.mkdir(parents=True, exist_ok=True)
    runtime = args.runtime.resolve()
    adapter = repo / "scripts/fixtures/restricted-exec-server.py"
    sources = {str(path.relative_to(repo)): digest(path) for path in (
        adapter, Path(__file__).resolve(), repo / "crates/velella/src/restricted.rs",
        repo / "crates/velella/src/restricted_fs.rs", repo / "crates/velella/src/lib.rs",
        repo / "crates/velella/src/main.rs")}
    receipt = {"schema": 1, "time": datetime.now(timezone.utc).isoformat(),
               "scope": "fixed local commands through restricted exec-server adapter",
               "no_model": True, "no_login": True, "host_credentials_read": False,
               "runtime_sha256": digest(runtime), "source_hashes": sources,
               "checks": {}, "status": "running"}
    root = Path(tempfile.mkdtemp(prefix="vl-adapter-acceptance-"))
    clients, descriptors = [], []
    cleanup_ok = True

    def create(name):
        owned = root / name
        owned.mkdir(mode=0o700)
        for directory in ("work", "home", "ro"):
            (owned / directory).mkdir()
        (owned / "work/sub").mkdir()
        (owned / "ro/file").write_text("read only fixture")
        (owned / "outside").write_text("private test marker")
        config = owned / "config.json"
        config.write_text(json.dumps({"runtime": str(runtime), "work": str(owned / "work"),
                                      "home": str(owned / "home"),
                                      "read_only": [str(owned / "ro")]}))
        config.chmod(0o600)
        client = Client(adapter, config)
        clients.append(client)
        client.ok("initialize", {"clientName": "velella-owned-acceptance"})
        return client, owned

    def tree(client, owned, name, natural=False):
        directory = owned / "work" / name
        directory.mkdir()
        client.start(name, ["/usr/bin/python3", "-I", "-c",
                           TREE.format(root=str(directory), natural=natural)], owned / "work/sub")
        return capture_tree(directory, descriptors)

    try:
        cleanup_receipt_boundaries(adapter, root, receipt["checks"])
        client, owned = create("protocol")
        work = owned / "work"
        require(client.ok("environment/info", None)["cwd"] == work.as_uri(), "environment cwd")
        require(client.ok("environment/status", None)["status"] == "ready", "environment status")
        client.ok("fs/writeFile", {"path": (work / "data").as_uri(),
                                  "dataBase64": base64.b64encode(b"owned fixture").decode()})
        read = client.ok("fs/readFile", {"path": (work / "data").as_uri()})
        require(base64.b64decode(read["dataBase64"]) == b"owned fixture", "filesystem read/write")
        maximum_file = b"b" * (1024 * 1024)
        client.ok("fs/writeFile", {"path": (work / "maximum").as_uri(),
                                  "dataBase64": base64.b64encode(maximum_file).decode()})
        read = client.ok("fs/readFile", {"path": (work / "maximum").as_uri()})
        require(base64.b64decode(read["dataBase64"]) == maximum_file, "1 MiB file round trip")
        receipt["checks"]["maximum_file_base64_round_trip"] = True
        for key, method, params in (
            ("outside_read_denied", "fs/readFile", {"path": (owned / "outside").as_uri()}),
            ("readonly_write_denied", "fs/writeFile", {
                "path": (owned / "ro/file").as_uri(), "dataBase64": "eA=="}),
        ):
            reply = client.rpc(method, params)
            require(reply.get("error", {}).get("code") == -32600, f"{key}: {reply}")
            receipt["checks"][key] = True
        missing = client.rpc("fs/getMetadata", {"path": (work / "missing").as_uri()})
        require(missing.get("error", {}).get("code") == -32004, "missing file code")
        receipt["checks"]["filesystem_and_missing_errno"] = True
        filesystem_boundaries(client, owned, receipt["checks"])
        reply = client.rpc("fs/readFile", {"path": adapter.as_uri()})
        require(reply.get("error", {}).get("code") == -32600,
                f"adapter source directory must not be implicitly readable: {reply}")
        receipt["checks"]["adapter_source_not_implicitly_readable"] = True
        require("error" in client.rpc("http/request", {}), "unsupported method must fail")
        require("error" in client.rpc("process/start", {
            "processId": "badcwd", "argv": ["/bin/true"], "cwd": owned.as_uri(),
            "env": {}, "tty": False}), "outside cwd must fail")
        require("error" in client.rpc("process/start", {
            "processId": "badtty", "argv": ["/bin/true"], "cwd": work.as_uri(),
            "env": {}, "tty": True}), "tty must fail")
        require("error" in client.rpc("process/start", {
            "processId": "badpolicy", "argv": ["/bin/true"], "cwd": work.as_uri(),
            "env": {}, "tty": False, "sandbox": {"permissions": {"type": "disabled"}}}),
            "caller policy must not replace fixed policy")
        fake_runtime = work / "runtime"
        fake_runtime.write_text("#!/bin/sh\nexit 99\n")
        fake_runtime.chmod(0o700)
        bad_config = owned / "bad-config.json"
        bad_config.write_text(json.dumps({"runtime": str(fake_runtime), "work": str(work),
                                          "home": str(owned / "home"),
                                          "read_only": [str(owned / "ro")]}))
        bad_config.chmod(0o600)
        rejected = subprocess.run(
            ["/usr/bin/python3", "-I", str(adapter), "--config", str(bad_config)],
            input=b"", capture_output=True, timeout=3,
            env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"})
        require(rejected.returncode != 0 and b"runtime must stay outside" in rejected.stderr,
                "writable runtime must be rejected at configuration")
        receipt["checks"]["unsupported_and_escalation_fail_closed"] = True
        client.start("cwd-env", ["/usr/bin/python3", "-I", "-c",
                                "import os,json,signal; print(json.dumps({'cwd':os.getcwd(),"
                                "'clean':'VELELLA_TEST_SECRET' not in os.environ and "
                                "'LD_PRELOAD' not in os.environ,"
                                "'signals_unblocked':not ({signal.SIGTERM,signal.SIGINT} & "
                                "signal.pthread_sigmask(signal.SIG_BLOCK, []))}))"],
                     work / "sub", {"VELELLA_TEST_SECRET": "synthetic", "LD_PRELOAD": "/test-only"})
        result = client.wait_closed("cwd-env")
        text = b"".join(base64.b64decode(item["chunk"]) for item in result["chunks"]
                        if item["stream"] == "stdout")
        value = json.loads(text)
        require(result["exitCode"] == 0 and value == {
            "cwd": str(work / "sub"), "clean": True, "signals_unblocked": True},
                f"cwd/env check: {result}")
        receipt["checks"]["subcwd_and_environment_cleaning"] = True
        receipt["checks"]["process_term_and_int_signals_unblocked"] = True
        outside_dir = owned / "work-shadow"
        outside_dir.mkdir()
        escape_dir = work / "escape-dir"
        escape_dir.symlink_to(outside_dir, target_is_directory=True)
        readonly_dir = work / "readonly-dir"
        readonly_dir.symlink_to(owned / "ro", target_is_directory=True)
        for index, bad_cwd in enumerate((
                outside_dir, escape_dir, readonly_dir, work / "sub" / ".." / "..")):
            reply = client.rpc("process/start", {
                "processId": f"cwd-escape-{index}", "argv": ["/bin/true"],
                "cwd": bad_cwd.as_uri(), "env": {}, "tty": False})
            require("error" in reply, f"cwd escape must not become writable root: {reply}")
        direct_marker = work / "native-entry-ran"
        direct_spec = {
            "policy": {"mode": "restricted", "network": "denied", "home": str(owned / "home"),
                       "read_only": [str(owned / "ro")], "writable": []},
            "cwd": str(work), "env": {},
            "operation": {
                "kind": "process", "cwd": str(work / "sub"),
                "argv": ["/usr/bin/python3", "-I", "-c",
                         "import pathlib; "
                         f"pathlib.Path({str(direct_marker)!r}).write_text('native entry ran')"]}}
        # Bypass controller validation. The sentinel is inside the permitted
        # work root, so its absence proves the operation was never executed.
        for bad_cwd in (outside_dir, owned / "ro", escape_dir, readonly_dir):
            direct_spec["operation"]["cwd"] = str(bad_cwd)
            rejected = subprocess.run(
                [str(runtime), "restricted-tool", json.dumps(direct_spec)],
                input=b"", capture_output=True, timeout=5, cwd=work,
                env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"}, start_new_session=True)
            require(rejected.returncode != 0 and not direct_marker.exists(),
                    "native entry must independently reject cwd escape before running command: "
                    + rejected.stderr.decode(errors="replace"))
        direct_spec["operation"]["cwd"] = str(work / "sub")
        accepted = subprocess.run(
            [str(runtime), "restricted-tool", json.dumps(direct_spec)],
            input=b"", capture_output=True, timeout=5, cwd=work,
            env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"}, start_new_session=True)
        require(accepted.returncode == 0 and direct_marker.read_text() == "native entry ran",
                "native entry must accept the same command under fixed work root: "
                + accepted.stderr.decode(errors="replace"))
        receipt["checks"]["native_entry_independently_checks_requested_cwd"] = True
        outside_target = owned / "must-not-create"
        sibling_target = work / "sibling-write"
        code = (
            "import errno,json,pathlib\n"
            f"pathlib.Path({str(sibling_target)!r}).write_text('fixed work root')\n"
            "denied = False\n"
            "try:\n"
            f" pathlib.Path({str(outside_target)!r}).write_text('escape')\n"
            "except OSError as error:\n"
            " denied = error.errno in (errno.EACCES, errno.EPERM)\n"
            "print(json.dumps({'outside_denied': denied}))\n")
        client.start("cwd-root", ["/usr/bin/python3", "-I", "-c", code], work / "sub")
        result = client.wait_closed("cwd-root")
        text = b"".join(base64.b64decode(item["chunk"]) for item in result["chunks"]
                        if item["stream"] == "stdout")
        require(result["exitCode"] == 0 and json.loads(text) == {"outside_denied": True}
                and not outside_target.exists()
                and sibling_target.read_text() == "fixed work root",
                f"requested subcwd must preserve fixed work policy: {result}")
        receipt["checks"]["requested_cwd_cannot_expand_fixed_write_root"] = True
        natural = tree(client, owned, "natural", natural=True)
        require(client.wait_closed("natural")["exitCode"] == 0 and exited(natural), "natural cleanup")
        receipt["checks"]["natural_exit_cleans_descendants"] = True
        cancelled = tree(client, owned, "cancel")
        require(client.ok("process/terminate", {"processId": "cancel"})["running"], "cancel running")
        client.wait_closed("cancel")
        require(exited(cancelled), "cancel descendants survived")
        require(not client.ok("process/terminate", {"processId": "cancel"})["running"], "repeat cancel")
        receipt["checks"]["cancel_and_repeat_cancel"] = True
        client.start("output-limit", ["/usr/bin/python3", "-I", "-c",
                                     "import os,time; os.write(1,b'x'*(2*1024*1024)); time.sleep(30)"],
                     work)
        limited = client.wait_closed("output-limit")
        require(limited["failure"] == "process output limit exceeded",
                "real output limit must stop and drain the process")
        client.start("after-output-limit", ["/bin/true"], work)
        require(client.wait_closed("after-output-limit")["exitCode"] == 0,
                "a command output limit must not poison subsequent commands")
        receipt["checks"]["output_limit_then_next_command"] = True
        eof = tree(client, owned, "eof")
        client.close()
        require(exited(eof), "EOF descendants survived")
        receipt["checks"]["eof_cleans_descendants"] = True
        receipt["checks"]["eof_cleanup_receipt_matches_process_start"] = True

        backpressure, owned = create("backpressure")
        pending = tree(backpressure, owned, "pending")
        backpressure.read_enabled.clear()
        spam_pid = owned / "work/spam.pid"
        backpressure.send("process/start", {
            "processId": "spam", "argv": ["/usr/bin/python3", "-I", "-c",
                                         "import os,time,pathlib; "
                                         f"pathlib.Path({str(spam_pid)!r}).write_text(str(os.getpid())); "
                                         "os.write(1,b'x'*600000); time.sleep(30)"],
            "cwd": (owned / "work").as_uri(), "env": {}, "tty": False})
        deadline = time.monotonic() + 4
        while not spam_pid.exists():
            require(time.monotonic() < deadline, "backpressure process not ready")
            time.sleep(.01)
        fd = os.pidfd_open(int(spam_pid.read_text()))
        descriptors.append(fd)
        pending.append({"pid": int(spam_pid.read_text()), "fd": fd})
        time.sleep(.4)
        signal.pidfd_send_signal(backpressure.pidfd, signal.SIGTERM)
        # Keep stdout paused while requiring signal cleanup to complete.
        backpressure.process.wait(timeout=5)
        require(exited(pending), "backpressure SIGTERM descendants survived")
        receipt["checks"]["backpressure_sigterm_cleans_descendants"] = True
        backpressure.close()
        receipt["checks"]["sigterm_cleanup_receipt_matches_process_start"] = True
        receipt["status"] = "passed"
    except Exception as error:
        receipt["status"] = "failed"
        receipt["error"] = f"{type(error).__name__}: {error}"
    finally:
        for client in clients:
            try:
                client.close(terminate=True)
            except Exception:
                cleanup_ok = False
        for fd in descriptors:
            if not select.select([fd], [], [], 0)[0]:
                cleanup_ok = False
                try:
                    signal.pidfd_send_signal(fd, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            os.close(fd)
        receipt["cleanup"] = {"all_captured_owned_processes_exited": cleanup_ok}
        for index, client in enumerate(clients):
            os.close(client.pidfd)
            (output / f"client-{index}-stderr.log").write_bytes(client.stderr)
            (output / f"client-{index}-events.json").write_text(json.dumps(client.events, indent=2))
        if cleanup_ok:
            shutil.rmtree(root)
        receipt["cleanup"]["temporary_directory_removed"] = not root.exists()
        receipt["source_hashes_unchanged"] = all(
            digest(repo / name) == value for name, value in sources.items())
        receipt["runtime_hash_unchanged"] = digest(runtime) == receipt["runtime_sha256"]
        if not cleanup_ok or not receipt["source_hashes_unchanged"] or not receipt["runtime_hash_unchanged"]:
            receipt["status"] = "failed"
        (output / "result.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))
    return 0 if receipt["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
