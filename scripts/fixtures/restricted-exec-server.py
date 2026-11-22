#!/usr/bin/env python3
"""Experimental, fixed-policy exec-server adapter for offline acceptance tests.

This is a trusted host controller, not a sandbox. Every model-supplied process
and filesystem operation runs below velella-runtime restricted-tool. It implements
the small JSONL protocol subset used by Codex 0.153.4; unsupported requests fail.
No credentials, model provider, socket listener, or host filesystem API is exposed.
"""
import argparse
import base64
from collections import deque
import errno
import json
import os
from pathlib import Path
import queue
import select
import signal
import stat
import subprocess
import sys
import threading
import time
from urllib.parse import unquote, urlsplit
import uuid


MAX_FRAME = 2 * 1024 * 1024
MAX_OUTPUT = 1024 * 1024
MAX_RECORDS = 4096
MAX_PROCESSES = 64
MAX_ACTIVE = 16
MAX_ARGV_BYTES = 64 * 1024
SCRIPT = str(Path(__file__).resolve())


class RpcError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code


def check(condition, message):
    if not condition:
        raise ValueError(message)


def string(value, name, maximum=16384):
    check(isinstance(value, str) and "\0" not in value and len(value.encode()) <= maximum,
          f"invalid {name}")
    return value


def decode_chunk(value, maximum):
    check(isinstance(value, str) and len(value) <= (maximum + 2) // 3 * 4,
          "base64 payload exceeds limit")
    result = base64.b64decode(value, validate=True)
    check(len(result) <= maximum, "decoded payload exceeds limit")
    return result


def file_path(value):
    value = string(value, "file URI")
    parts = urlsplit(value)
    check(parts.scheme == "file" and not parts.netloc and not parts.query
          and not parts.fragment, "only absolute file:/// URIs are supported")
    path = unquote(parts.path, errors="strict")
    check(path.startswith("/") and "\0" not in path, "invalid absolute file URI")
    return path


def contained(path, root):
    return os.path.commonpath((path, root)) == root


def fixed_cwd(value, work):
    path = os.path.realpath(file_path(value))
    check(contained(path, work), "cwd must stay inside the fixed work directory")
    check(os.path.isdir(path), "cwd is not a directory")
    return path


def validate_sandbox(params):
    # A caller can never change the fixed external policy. We cannot promise
    # additional, narrower managed policies either, so reject unsupported ones.
    sandbox = params.get("sandbox")
    if sandbox is not None:
        check(isinstance(sandbox, dict), "invalid sandbox")
        check(sandbox.get("permissions") == {"type": "external", "network": "restricted"},
              "only the fixed external, network-restricted policy is supported")
    check(not params.get("enforceManagedNetwork") and params.get("managedNetwork") is None
          and params.get("networkProxy") is None,
          "managed networking is unsupported; fixed policy always denies networking")


def private_cleanup_directory(path):
    """Pin the private controller directory without following a final symlink."""
    descriptor = None
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
        metadata = os.fstat(descriptor)
        check(stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.geteuid()
              and stat.S_IMODE(metadata.st_mode) == 0o700, "cleanup directory is not private")
        return descriptor
    except (OSError, ValueError):
        if descriptor is not None:
            os.close(descriptor)
        # Legacy nonprivate fixtures can still run, but cannot authorize resume.
        return None


def write_cleanup_receipt(directory, pid, start):
    """Publish only a complete, durable receipt; never replace an existing name."""
    descriptor = None
    created = False
    name = f"exec-server-cleanup-{pid}.json"
    try:
        metadata = os.fstat(directory)
        check(metadata.st_uid == os.geteuid() and stat.S_ISDIR(metadata.st_mode)
              and stat.S_IMODE(metadata.st_mode) == 0o700, "cleanup directory changed")
        check(isinstance(start, str) and start.isdecimal(), "invalid process identity")
        payload = (json.dumps({"schema": 1, "pid": pid, "start": start, "clean": True},
                              separators=(",", ":")) + "\n").encode()
        descriptor = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL
                             | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=directory)
        created = True
        os.fchmod(descriptor, 0o600)
        offset = 0
        while offset < len(payload):
            written = os.write(descriptor, payload[offset:])
            check(written > 0, "cleanup receipt write failed")
            offset += written
        os.fsync(descriptor)
        os.fsync(directory)
        return True
    except (OSError, ValueError):
        if created:
            try:
                os.unlink(name, dir_fd=directory)
                os.fsync(directory)
            except OSError:
                pass
        return False
    finally:
        if descriptor is not None:
            os.close(descriptor)



class OwnedProcess:
    """One pinned group leader: never reap it before descendant cleanup."""

    def __init__(self, server, process_id, spec, notify, pipe_stdin=False):
        self.server, self.process_id, self.notify = server, process_id, notify
        self.lock = threading.RLock()
        self.changed = threading.Condition(self.lock)
        self.gate = threading.Event()
        if not notify:
            self.gate.set()
        self.seq = 0
        self.chunks = deque()
        self.buffered = 0
        self.output_limit = MAX_OUTPUT if notify else MAX_FRAME
        self.failure = None
        self.exit_code = None
        self.closed = False
        self.active = True
        self.write_ids = {}
        # The native helper unblocks these only after restricted-tool confines it.
        # The controller's signal handler only sets a flag; it never interrupts
        # the Popen -> owned registration transaction with an asynchronous raise.
        old_mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM, signal.SIGINT})
        self.child = None
        try:
            self.child = subprocess.Popen(
                [server.runtime, "restricted-tool", json.dumps(spec, separators=(",", ":"))],
                cwd=server.work, env={"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"},
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                start_new_session=True, close_fds=True,
            )
            # Only our own direct child can establish this group; no process scan.
            if os.getpgid(self.child.pid) != self.child.pid or self.child.pid == os.getpgrp():
                self.child.kill()
                self.child.wait()
                raise RuntimeError("restricted child did not create a dedicated process group")
            os.set_blocking(self.child.stdin.fileno(), False)
            if not pipe_stdin:
                self.child.stdin.close()
            self.readers = [
                threading.Thread(target=self._read, args=(self.child.stdout, "stdout"), daemon=True),
                threading.Thread(target=self._read, args=(self.child.stderr, "stderr"), daemon=True),
            ]
            self.reaper = threading.Thread(target=self._reap, daemon=True)
            server.owned.append(self)
            for reader in self.readers:
                reader.start()
            self.reaper.start()
        except BaseException:
            if self.child is not None:
                # A partial thread/process startup is not a normal command
                # failure. Cleanup below is best-effort and must never certify
                # a later resume, even if the entry leaves the owned registry.
                server.cleanup_uncertain = True
                # No reaper has started if thread setup failed. Keep ownership
                # until the leader is collected; startup failure is fatal.
                try:
                    os.killpg(self.child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                self.active = False
                self.child.wait(timeout=5)
                for stream in (self.child.stdin, self.child.stdout, self.child.stderr):
                    stream.close()
                if self in server.owned:
                    server.owned.remove(self)
            raise
        finally:
            signal.pthread_sigmask(signal.SIG_SETMASK, old_mask)

    def _event(self, method, params):
        if self.notify and not self.server.stopping:
            self.server.send({"method": method,
                              "params": {"processId": self.process_id, **params}})

    def _read(self, stream, name):
        self.gate.wait()
        try:
            while block := stream.read1(16384):
                with self.lock:
                    if self.failure is not None:
                        continue
                    if (self.buffered + len(block) > self.output_limit
                            or len(self.chunks) >= MAX_RECORDS):
                        self.failure = "process output limit exceeded"
                        self.kill()
                        self.changed.notify_all()
                        continue
                    self.seq += 1
                    chunk = {"seq": self.seq, "stream": name,
                             "chunk": base64.b64encode(block).decode()}
                    self.chunks.append(chunk)
                    self.buffered += len(block)
                    self._event("process/output", chunk)
                    self.changed.notify_all()
        except (OSError, ValueError) as error:
            with self.lock:
                self.failure = self.failure or f"output read failed: {error}"
                self.changed.notify_all()
        finally:
            stream.close()

    def kill(self):
        with self.lock:
            running = self.active
            if running:
                # The only reaper holds this lock until active=False, and waits
                # with WNOWAIT first, so this PID/PGID cannot have been reused.
                try:
                    os.killpg(self.child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            return running

    def cleanup_complete(self):
        with self.lock:
            return (self.closed and not self.active and not self.reaper.is_alive()
                    and all(not reader.is_alive() for reader in self.readers))

    def _reap(self):
        self.gate.wait()
        try:
            while True:
                try:
                    os.waitid(os.P_PID, self.child.pid, os.WEXITED | os.WNOWAIT)
                    break
                except InterruptedError:
                    continue
            with self.lock:
                self.kill()
                self.active = False
            code = self.child.wait()
            if not self.child.stdin.closed:
                self.child.stdin.close()
            for reader in self.readers:
                reader.join(timeout=3)
            with self.lock:
                if any(reader.is_alive() for reader in self.readers):
                    self.failure = self.failure or "output drain did not finish"
                self.exit_code = code if code >= 0 else 128 - code
                self.seq += 1
                self._event("process/exited", {"seq": self.seq, "exitCode": self.exit_code,
                                               "sandboxDenied": False})
                self.seq += 1
                self.closed = True
                self._event("process/closed", {"seq": self.seq})
                self.changed.notify_all()
        except Exception as error:
            # An internal lifecycle failure is fatal; never silently continue
            # handling new operations with uncertain ownership.
            with self.lock:
                self.failure = f"process cleanup failed: {error}"
                self.kill()
                self.changed.notify_all()
            self.server.fatal.set()

    def read(self, params):
        after = params.get("afterSeq") or 0
        maximum = params.get("maxBytes") or MAX_OUTPUT
        wait_ms = params.get("waitMs") or 0
        check(isinstance(after, int) and 0 <= after <= 2**64 - 1, "invalid afterSeq")
        check(isinstance(maximum, int) and 1 <= maximum <= MAX_OUTPUT, "invalid maxBytes")
        check(isinstance(wait_ms, int) and 0 <= wait_ms <= 30000, "invalid waitMs")
        with self.changed:
            if self.seq <= after and not self.closed and wait_ms:
                self.changed.wait(min(wait_ms, 1000) / 1000)
            chunks, size = [], 0
            for chunk in self.chunks:
                if chunk["seq"] <= after:
                    continue
                decoded_size = len(chunk["chunk"]) * 3 // 4
                if chunks and size + decoded_size > maximum:
                    break
                chunks.append(chunk)
                size += decoded_size
            next_seq = chunks[-1]["seq"] if chunks else min(after, self.seq)
            if self.closed and (not chunks or chunks[-1] is self.chunks[-1]):
                next_seq = self.seq
            return {"chunks": chunks, "nextSeq": next_seq,
                    "exited": self.exit_code is not None, "exitCode": self.exit_code,
                    "closed": self.closed, "failure": self.failure, "sandboxDenied": False}

    def write(self, params):
        write_id = string(params.get("writeId"), "writeId", 256)
        check(bool(write_id), "writeId must not be empty")
        data = decode_chunk(params.get("chunk"), 4096)
        with self.lock:
            previous = self.write_ids.get(write_id)
            if previous:
                check(previous[0] == data, "writeId reused with different bytes")
                return {"status": previous[1]}
            check(len(self.write_ids) < 256, "stdin write receipt limit exceeded")
            if self.child.stdin.closed or not self.active:
                status = "stdinClosed"
            else:
                try:
                    # <= PIPE_BUF to make nonblocking writes atomic.
                    written = os.write(self.child.stdin.fileno(), data)
                    check(written == len(data), "partial stdin write")
                    status = "accepted"
                except BrokenPipeError:
                    status = "stdinClosed"
                except BlockingIOError:
                    raise ValueError("stdin is full; retry with the same writeId") from None
            self.write_ids[write_id] = (data, status)
            return {"status": status}


class Server:
    FS_METHODS = {"fs/readFile", "fs/writeFile", "fs/getMetadata", "fs/canonicalize",
                  "fs/readDirectory", "fs/createDirectory", "fs/remove"}

    def __init__(self, config_path):
        path = Path(config_path).resolve(strict=True)
        meta = path.stat()
        check(stat.S_ISREG(meta.st_mode) and meta.st_uid == os.geteuid()
              and not meta.st_mode & 0o022, "config must be owned by this user and not group/world writable")
        check(meta.st_size <= 32768, "config exceeds limit")
        config = json.loads(path.read_text())
        check(set(config) == {"runtime", "work", "home", "read_only"},
              "config requires exactly runtime, work, home, read_only")
        self.runtime = str(Path(config["runtime"]).resolve(strict=True))
        self.work = str(Path(config["work"]).resolve(strict=True))
        self.home = str(Path(config["home"]).resolve(strict=True))
        check(os.path.isdir(self.work) and os.path.isdir(self.home), "work/home must be directories")
        check(not contained(self.work, self.home) and not contained(self.home, self.work),
              "work and home must be separate")
        check(not contained(str(path), self.work) and not contained(str(path), self.home),
              "config must stay outside writable directories")
        check(not contained(SCRIPT, self.work) and not contained(SCRIPT, self.home),
              "adapter must stay outside writable directories")
        check(not contained(self.runtime, self.work) and not contained(self.runtime, self.home),
              "runtime must stay outside writable directories")
        check(stat.S_ISREG(os.stat(self.runtime).st_mode) and os.access(self.runtime, os.X_OK),
              "runtime must be a regular executable")
        check(isinstance(config["read_only"], list) and len(config["read_only"]) <= 60,
              "invalid read_only roots")
        roots = [str(Path(item).resolve(strict=True)) for item in config["read_only"]]
        self.read_only = []
        for root in roots:
            check(os.path.isdir(root), "read_only root must be a directory")
            check(not any(contained(root, writable) or contained(writable, root)
                          for writable in (self.work, self.home)), "read_only/writable roots overlap")
            if root not in self.read_only:
                self.read_only.append(root)
        self.lock = threading.Lock()
        self.processes = {}
        self.hidden = []
        self.owned = []
        self.cleanup_uncertain = False
        self.stopping = False
        self.fatal = threading.Event()
        self.output_done = threading.Event()
        self.output_queue = queue.Queue(maxsize=512)
        self.output_bytes = 0
        self.input_buffer = bytearray()
        self.initialized = False
        self.session_id = uuid.uuid4().hex
        self.cleanup_pid = os.getpid()
        self.cleanup_start = Path("/proc/self/stat").read_text().rsplit(") ", 1)[1].split()[19]
        self.cleanup_directory = private_cleanup_directory(path.parent)
        self.cleanup_finished = False
        os.set_blocking(sys.stdout.fileno(), False)
        os.set_blocking(sys.stdin.fileno(), False)
        self.writer = threading.Thread(target=self._write_output, daemon=True)
        self.writer.start()

    def send(self, message):
        encoded = json.dumps(message, separators=(",", ":")).encode() + b"\n"
        check(len(encoded) <= MAX_FRAME, "response frame exceeds limit")
        failed = False
        with self.lock:
            if self.output_bytes + len(encoded) > 4 * MAX_FRAME:
                failed = True
            else:
                try:
                    self.output_queue.put_nowait(encoded)
                    self.output_bytes += len(encoded)
                except queue.Full:
                    failed = True
        if failed:
            self.fail("protocol output queue exceeded limit")

    def fail(self, message):
        self.fatal_message = message
        self.fatal.set()
        self.stopping = True
        # The main loop observes fatal at most 100 ms after returning from the
        # current bounded operation, then closes all owned groups. Do not take
        # another process lock here: this may itself run under a process lock.

    def _write_output(self):
        # Never hold a process lock across transport I/O. A client which stops
        # reading cannot prevent cancellation or EOF/SIGTERM group cleanup.
        try:
            while not self.output_done.is_set():
                try:
                    encoded = self.output_queue.get(timeout=.1)
                except queue.Empty:
                    continue
                offset = 0
                deadline = time.monotonic() + 10
                while offset < len(encoded) and not self.output_done.is_set():
                    check(time.monotonic() < deadline, "protocol output stalled")
                    if not select.select([], [sys.stdout.fileno()], [], .1)[1]:
                        continue
                    try:
                        offset += os.write(sys.stdout.fileno(), encoded[offset:])
                    except BlockingIOError:
                        continue
                with self.lock:
                    self.output_bytes -= len(encoded)
                self.output_queue.task_done()
        except Exception as error:
            self.fail(str(error))

    def next_frame(self):
        while not self.fatal.is_set():
            newline = self.input_buffer.find(b"\n")
            if newline >= 0:
                frame = bytes(self.input_buffer[:newline + 1])
                del self.input_buffer[:newline + 1]
                check(len(frame) <= MAX_FRAME, "oversized JSONL frame")
                return frame
            check(len(self.input_buffer) <= MAX_FRAME, "oversized JSONL frame")
            if not select.select([sys.stdin.fileno()], [], [], .1)[0]:
                continue
            try:
                block = os.read(sys.stdin.fileno(), 65536)
            except BlockingIOError:
                continue
            if not block:
                check(not self.input_buffer, "incomplete JSONL frame")
                return b""
            self.input_buffer.extend(block)
        return b""

    def info(self):
        return {"shell": {"name": "bash", "path": "/bin/bash"},
                "cwd": Path(self.work).as_uri(), "userHomeDir": Path(self.home).as_uri(),
                "platformOs": "linux", "temporaryDirectories": [],
                "tempDir": Path(self.home, ".tmp").as_uri(), "capabilities": {
                    "networkProxyLaunch": False, "capabilityDiscoverySandbox": False,
                    "environmentConfigRead": False, "httpHeaderEnvVars": False,
                    "sandboxedFileStreaming": False, "shellSnapshotV2": False}}

    def spec(self, operation, env):
        return {"policy": {"mode": "restricted", "read_only": self.read_only,
                           "writable": [], "network": "denied", "home": self.home},
                "operation": operation, "cwd": self.work, "env": env}

    def start(self, params):
        validate_sandbox(params)
        check(params.get("tty") is False, "tty:true is unsupported by the restricted adapter")
        check(params.get("arg0") is None and params.get("shellSnapshot") is None,
              "arg0/shell snapshots are unsupported")
        process_id = string(params.get("processId"), "processId", 256)
        check(bool(process_id) and process_id not in self.processes,
              "empty or reused processId")
        check(len(self.processes) < MAX_PROCESSES, "process record limit exceeded")
        check(sum(process.active for process in self.processes.values()) < MAX_ACTIVE,
              "active process limit exceeded")
        cwd = fixed_cwd(params.get("cwd"), self.work)
        argv = params.get("argv")
        check(isinstance(argv, list) and 1 <= len(argv) <= 512, "invalid argv")
        argv = [string(item, "argv element", MAX_ARGV_BYTES) for item in argv]
        check(bool(argv[0]) and len(json.dumps(argv).encode()) <= MAX_ARGV_BYTES,
              "argv exceeds limit")
        env = params.get("env")
        check(isinstance(env, dict) and len(env) <= 256, "invalid env")
        env = {string(key, "environment key", 256): string(value, "environment value", 8192)
               for key, value in env.items()}
        # Even the runtime's explicit hook allowance is unnecessary here.
        env.pop("VELELLA_HOOK_INBOX", None)
        spec = self.spec({"kind": "process", "cwd": cwd, "argv": argv}, env)
        process = OwnedProcess(self, process_id, spec, True, bool(params.get("pipeStdin")))
        self.processes[process_id] = process
        return {"processId": process_id, "sandboxType": "linuxSeccomp"}, process

    def filesystem(self, method, params):
        validate_sandbox(params)
        # Do not resolve, stat, read, or write request paths on the host.
        file_path(params.get("path"))
        raw = json.dumps(params, separators=(",", ":")).encode()
        check(len(raw) <= MAX_FRAME, "filesystem request exceeds limit")
        process = OwnedProcess(self, uuid.uuid4().hex,
                               self.spec({"kind": "filesystem", "method": method}, {}), False, True)
        self.hidden.append(process)
        try:
            # Helpers consume only this bounded request and never accept model
            # stdin afterwards. Blocking pipe I/O occurs on a bounded worker.
            def feed():
                try:
                    os.set_blocking(process.child.stdin.fileno(), True)
                    process.child.stdin.write(raw)
                    process.child.stdin.close()
                except (BrokenPipeError, OSError, ValueError):
                    pass
            writer = threading.Thread(target=feed, daemon=True)
            writer.start()
            deadline = time.monotonic() + 15
            while process.reaper.is_alive() and time.monotonic() < deadline:
                if self.fatal.is_set():
                    raise InterruptedError("adapter is stopping")
                process.reaper.join(timeout=.1)
            check(not process.reaper.is_alive(), "filesystem operation timed out")
            check(process.failure is None, process.failure or "filesystem failed")
            output = b"".join(base64.b64decode(chunk["chunk"])
                              for chunk in process.chunks if chunk["stream"] == "stdout")
            errors = b"".join(base64.b64decode(chunk["chunk"])
                              for chunk in process.chunks if chunk["stream"] == "stderr")
            if process.exit_code != 0:
                try:
                    error = json.loads(output)["error"]
                    number = error.get("errno")
                    code = (-32004 if number == errno.ENOENT else
                            -32600 if number in (errno.EACCES, errno.EPERM, errno.EINVAL)
                            else -32603)
                    raise RpcError(code, str(error.get("message", "filesystem failed"))[:8192])
                except (ValueError, KeyError, TypeError):
                    raise RpcError(-32603, errors.decode(errors="replace")[:8192]
                                   or "restricted filesystem helper failed") from None
            return json.loads(output)
        finally:
            process.kill()
            process.reaper.join(timeout=5)
            if process.cleanup_complete():
                self.hidden.remove(process)
                self.owned.remove(process)

    def dispatch(self, method, params):
        if params is None and method in ("environment/info", "environment/status"):
            params = {}
        check(isinstance(params, dict), "params must be an object")
        if method == "initialize":
            check(not self.initialized, "already initialized")
            string(params.get("clientName"), "clientName", 256)
            check(params.get("resumeSessionId") is None, "session resume is unsupported")
            self.initialized = True
            return {"sessionId": self.session_id, "environmentInfo": self.info()}, None
        check(self.initialized, "initialize required")
        if method == "environment/info":
            return self.info(), None
        if method == "environment/status":
            return {"status": "ready"}, None
        if method == "process/start":
            return self.start(params)
        if method in self.FS_METHODS:
            return self.filesystem(method, params), None
        if method in ("process/read", "process/write", "process/terminate"):
            process_id = string(params.get("processId"), "processId", 256)
            process = self.processes.get(process_id)
            if method == "process/terminate":
                return {"running": process.kill() if process else False}, None
            if method == "process/write" and process is None:
                return {"status": "unknownProcess"}, None
            check(process is not None, "unknown processId")
            return (process.read(params) if method == "process/read"
                    else process.write(params)), None
        raise RpcError(-32601, f"unsupported restricted exec-server method: {method}")

    def close(self):
        if self.cleanup_finished:
            return
        self.stopping = True
        owned = list(self.owned)
        try:
            for process in owned:
                process.gate.set()
                process.kill()
            deadline = time.monotonic() + 3.5
            for process in owned:
                process.reaper.join(timeout=max(0, deadline - time.monotonic()))
            deadline = time.monotonic() + .5
            while self.output_bytes and time.monotonic() < deadline:
                time.sleep(.01)
            self.output_done.set()
            self.writer.join(timeout=1)
            # Command failures (including output limits) do not imply cleanup
            # failure. The proof is that every owned group has been reaped and
            # every output reader has ended, rather than a successful command.
            if (self.cleanup_directory is not None and not self.cleanup_uncertain
                    and not self.writer.is_alive()
                    and all(process.cleanup_complete() for process in owned)):
                write_cleanup_receipt(self.cleanup_directory, self.cleanup_pid, self.cleanup_start)
        finally:
            self.cleanup_finished = True
            if self.cleanup_directory is not None:
                os.close(self.cleanup_directory)
                self.cleanup_directory = None

    def run(self):
        def terminate(*_args):
            # No asynchronous exception: ownership transfers and mutex-protected
            # state updates must finish before the main loop performs cleanup.
            self.fatal.set()
            self.stopping = True
        signal.signal(signal.SIGTERM, terminate)
        signal.signal(signal.SIGINT, terminate)
        try:
            while not self.fatal.is_set():
                raw = self.next_frame()
                if not raw:
                    break
                check(len(raw) <= MAX_FRAME and raw.endswith(b"\n"), "oversized/incomplete JSONL frame")
                message = json.loads(raw)
                check(isinstance(message, dict), "JSONL message must be an object")
                if "id" not in message:
                    check(message.get("method") == "initialized" and self.initialized,
                          "unsupported notification")
                    continue
                request_id = message["id"]
                check(isinstance(request_id, (int, str)) and not isinstance(request_id, bool),
                      "invalid request id")
                release = None
                try:
                    result, release = self.dispatch(message.get("method"), message.get("params", {}))
                    self.send({"id": request_id, "result": result})
                except InterruptedError:
                    raise
                except (ValueError, KeyError, OSError, RpcError) as error:
                    code = error.code if isinstance(error, RpcError) else -32602
                    self.send({"id": request_id, "error": {"code": code, "message": str(error)[:8192]}})
                finally:
                    if release:
                        release.gate.set()
        except (InterruptedError, BrokenPipeError):
            pass
        finally:
            signal.signal(signal.SIGTERM, signal.SIG_IGN)
            signal.signal(signal.SIGINT, signal.SIG_IGN)
            self.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True)
    args = parser.parse_args()
    Server(args.config).run()


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(f"restricted exec-server: {exc}", file=sys.stderr)
        sys.exit(1)
