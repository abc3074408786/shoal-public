#!/usr/bin/env python3
"""Private native-TUI bridge for pinned Codex 0.153.4.

The original Codex TUI speaks its app-server protocol over a Unix WebSocket.
Only its local-host !shell escape hatch is replaced: commands run through the
same fixed restricted exec-server as model tools. This is a trusted controller,
not a sandbox. No model endpoint or authentication is implemented here.
"""
import argparse
import asyncio
import base64
import codecs
import hashlib
import json
import os
from pathlib import Path
import signal
import select
import socket
import stat
import struct
import subprocess
import sys
import time
import uuid

MAX_FRAME = 2 * 1024 * 1024
MAX_OUTPUT = 256 * 1024
MAX_REQUESTS = 256
MAX_COMMANDS = 64
MAX_QUEUED = 16
INTERNAL = "velella-bridge-"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def encode(value):
    data = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode()
    require(len(data) <= MAX_FRAME, "JSON frame exceeds limit")
    return data


def canonical_uuid(value):
    require(isinstance(value, str) and str(uuid.UUID(value)) == value, "invalid canonical UUID")
    return value


class RpcFailure(Exception):
    pass


class AuthSourceError(Exception):
    """All authentication failures deliberately use a fixed, non-secret message."""
    def __init__(self):
        super().__init__("Existing local ChatGPT authentication is unavailable; reopen the terminal after signing in locally.")


class Child:
    """Keep the group leader unreaped until its owned group is cleaned up."""
    def __init__(self, argv, env, bridge, name):
        self.bridge, self.name = bridge, name
        self.process = subprocess.Popen(argv, env=env, stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=sys.stderr,
                                        close_fds=True)
        os.set_blocking(self.process.stdin.fileno(), False)
        self.writer_lock = asyncio.Lock()
        self.pending = {}
        self.pending_methods = {}
        self.reader = None
        self.reader_task = None
        self.closed = False

    async def start(self):
        self.reader = asyncio.StreamReader(limit=MAX_FRAME + 1)
        protocol = asyncio.StreamReaderProtocol(self.reader)
        await asyncio.get_running_loop().connect_read_pipe(lambda: protocol, self.process.stdout)
        self.reader_task = asyncio.create_task(self.read())

    async def send(self, message):
        data = encode(message) + b"\n"
        async with self.writer_lock:
            offset = 0
            while offset < len(data):
                require(not self.closed, f"{self.name} is closed")
                try:
                    offset += os.write(self.process.stdin.fileno(), data[offset:])
                except BlockingIOError:
                    loop = asyncio.get_running_loop()
                    ready = loop.create_future()
                    fd = self.process.stdin.fileno()
                    loop.add_writer(fd, lambda: not ready.done() and ready.set_result(None))
                    try:
                        await asyncio.wait_for(ready, 5)
                    finally:
                        loop.remove_writer(fd)

    async def call(self, method, params, timeout=10):
        require(len(self.pending) < MAX_REQUESTS, "internal request limit exceeded")
        request_id = INTERNAL + uuid.uuid4().hex
        future = asyncio.get_running_loop().create_future()
        self.pending[request_id] = future
        self.pending_methods[request_id] = method
        try:
            await self.send({"id": request_id, "method": method, "params": params})
            return await asyncio.wait_for(future, timeout)
        finally:
            self.pending.pop(request_id, None)
            self.pending_methods.pop(request_id, None)

    async def read(self):
        try:
            while line := await self.reader.readline():
                require(len(line) <= MAX_FRAME and line.endswith(b"\n"), "invalid child JSONL frame")
                message = json.loads(line)
                request_id = message.get("id")
                if request_id in self.pending:
                    future = self.pending[request_id]
                    if not future.done():
                        if "error" in message:
                            if self.pending_methods.get(request_id) == "account/login/start":
                                future.set_exception(AuthSourceError())
                            else:
                                future.set_exception(RpcFailure(str(message["error"].get("message", "RPC failed"))))
                        else:
                            future.set_result(message.get("result"))
                elif isinstance(request_id, str) and request_id.startswith(INTERNAL):
                    # Late replies to a timed-out private call never reach the TUI.
                    continue
                elif self.name == "app-server":
                    await self.bridge.upstream(message)
            if not self.bridge.stopping:
                raise EOFError(f"{self.name} closed its output")
        except asyncio.CancelledError:
            return
        except Exception as error:
            self.bridge.fail(error)
        finally:
            for future in self.pending.values():
                if not future.done():
                    future.set_exception(RpcFailure(f"{self.name} disconnected"))

    def exited(self):
        try:
            return os.waitid(os.P_PID, self.process.pid,
                             os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None
        except ChildProcessError:
            return True

    def signal_process(self, sig):
        # No poll()/wait() occurs before cleanup, so this direct child PID is pinned.
        try:
            os.kill(self.process.pid, sig)
        except ProcessLookupError:
            pass

    async def close(self, graceful=True):
        if self.closed:
            return
        self.closed = True
        self.process.stdin.close()
        self.signal_process(signal.SIGTERM if graceful else signal.SIGKILL)
        deadline = time.monotonic() + (5 if graceful else 2)
        while not self.exited() and time.monotonic() < deadline:
            await asyncio.sleep(.05)
        self.signal_process(signal.SIGKILL)
        try:
            self.process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            pass
        if self.reader_task:
            self.reader_task.cancel()
            await asyncio.gather(self.reader_task, return_exceptions=True)
        self.process.stdout.close()


class WebSocket:
    def __init__(self, reader, writer):
        self.reader, self.writer = reader, writer
        self.lock = asyncio.Lock()

    async def handshake(self):
        header = await asyncio.wait_for(self.reader.readuntil(b"\r\n\r\n"), 5)
        require(len(header) <= 8192, "WebSocket header exceeds limit")
        lines = header.decode("ascii").split("\r\n")
        require(lines[0] == "GET /rpc HTTP/1.1", "invalid WebSocket request path")
        values = {}
        for line in lines[1:]:
            if line:
                key, value = line.split(":", 1)
                require(key.lower() not in values, "duplicate WebSocket header")
                values[key.lower()] = value.strip()
        require(values.get("upgrade", "").lower() == "websocket"
                and "upgrade" in values.get("connection", "").lower().split(", ")
                and values.get("sec-websocket-version") == "13", "invalid WebSocket handshake")
        key = values.get("sec-websocket-key", "")
        require(len(base64.b64decode(key, validate=True)) == 16, "invalid WebSocket key")
        accept = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()).decode()
        self.writer.write(("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n"
                           "Connection: Upgrade\r\nSec-WebSocket-Accept: " + accept + "\r\n\r\n").encode())
        await asyncio.wait_for(self.writer.drain(), 5)

    async def frame(self, opcode, data):
        require(len(data) <= MAX_FRAME, "WebSocket frame exceeds limit")
        if len(data) < 126:
            header = bytes((0x80 | opcode, len(data)))
        elif len(data) < 65536:
            header = bytes((0x80 | opcode, 126)) + struct.pack("!H", len(data))
        else:
            header = bytes((0x80 | opcode, 127)) + struct.pack("!Q", len(data))
        async with self.lock:
            self.writer.write(header + data)
            await asyncio.wait_for(self.writer.drain(), 5)

    async def send(self, message):
        await self.frame(1, encode(message))

    async def receive(self):
        message, initial = bytearray(), None
        while True:
            header = await self.reader.readexactly(2)
            final, opcode = bool(header[0] & 0x80), header[0] & 15
            require(not header[0] & 0x70 and header[1] & 0x80, "invalid WebSocket frame flags")
            size = header[1] & 127
            if size == 126:
                size = struct.unpack("!H", await self.reader.readexactly(2))[0]
            elif size == 127:
                size = struct.unpack("!Q", await self.reader.readexactly(8))[0]
            require(size <= MAX_FRAME and len(message) + size <= MAX_FRAME, "WebSocket message exceeds limit")
            mask = await self.reader.readexactly(4)
            payload = await self.reader.readexactly(size)
            payload = bytes(value ^ mask[index % 4] for index, value in enumerate(payload))
            if opcode >= 8:
                require(final and size <= 125, "invalid WebSocket control frame")
                if opcode == 8:
                    return None
                if opcode == 9:
                    await self.frame(10, payload)
                else:
                    require(opcode == 10, "unknown WebSocket control frame")
                continue
            if initial is None:
                require(opcode == 1, "only JSON text messages are accepted")
                initial = opcode
            else:
                require(opcode == 0, "invalid WebSocket continuation")
            message.extend(payload)
            if final:
                result = json.loads(message.decode("utf-8"))
                require(isinstance(result, dict), "JSON-RPC message must be an object")
                return result


class Bridge:
    def __init__(self, args):
        self.args = args
        self.socket = Path(args.socket)
        config_path = Path(args.config).resolve(strict=True)
        meta = config_path.stat()
        require(stat.S_ISREG(meta.st_mode) and meta.st_uid == os.geteuid()
                and not meta.st_mode & 0o022 and meta.st_size <= 32768, "unsafe executor config")
        config = json.loads(config_path.read_text())
        self.work = str(Path(config["work"]).resolve(strict=True))
        self.tool_home = str(Path(config["home"]).resolve(strict=True))
        require(self.socket.is_absolute() and not self.socket.exists(), "socket must be new and absolute")
        parent = self.socket.parent.stat()
        require(parent.st_uid == os.geteuid() and not parent.st_mode & 0o077,
                "socket parent must be private to this user")
        self.socket_parent_identity = (parent.st_dev, parent.st_ino)
        require(len(os.fsencode(self.socket)) < 104, "Unix socket path too long")
        self.stop = asyncio.Event()
        self.stopping = False
        self.client = None
        self.accepted = False
        self.app = self.executor = self.server = None
        self.tasks = set()
        self.thread_ids = set()
        self.active_model = {}
        self.shells = {}
        self.pending_client = {}
        self.inflight = set()
        self.server_requests = set()
        self.client_tasks = set()
        self.initialized = False
        self.commands = 0
        self.failure = None
        self.lease_fd = None
        self.methods_seen = set()
        self.blocked_methods = set()
        self.auth_source = getattr(args, "chatgpt_auth_source", None)
        self.auth_account_id = None
        self.auth_ready = asyncio.Event()
        if not self.auth_source:
            self.auth_ready.set()
        self.external_chatgpt_auth = False
        self.auth_failed = False
        self.generation = canonical_uuid(args.generation)
        self.state_directory = config_path.parent
        self.codex_home = str((self.state_directory / "controller" / "codex").resolve(strict=True))
        state_meta = self.state_directory.stat()
        require(state_meta.st_uid == os.geteuid() and stat.S_IMODE(state_meta.st_mode) == 0o700,
                "terminal state directory must be private")
        for writable in (self.work, self.tool_home):
            require(os.path.commonpath((str(self.state_directory), writable)) != writable,
                    "terminal state is inside a tool-writable directory")
        self.checkpoint_path = self.state_directory / "terminal-resume.json"
        self.checkpoint = None
        self.resume_thread = getattr(args, "resume_thread", None)
        self.resume_inflight = False
        self.start_inflight = False
        self.history_confirmation_pending = False
        if self.resume_thread is not None:
            canonical_uuid(self.resume_thread)
            self.checkpoint = self.read_checkpoint()
            require(self.checkpoint["thread_id"] == self.resume_thread, "resume thread does not match checkpoint")
            require(not self.checkpoint["shell_history_dirty"], "previous shell history was not confirmed persisted")
            require(self.checkpoint["history_ready"], "thread history was not confirmed persisted")
            require(self.auth_source is not None or self.checkpoint["account_sha256"] is None,
                    "resume requires the original ChatGPT account")

    def read_checkpoint(self):
        fd = os.open(self.checkpoint_path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK)
        try:
            before = os.fstat(fd)
            require(stat.S_ISREG(before.st_mode) and before.st_uid == os.geteuid()
                    and stat.S_IMODE(before.st_mode) == 0o600 and before.st_nlink == 1
                    and 0 < before.st_size <= 8192, "unsafe terminal checkpoint")
            data = os.read(fd, 8193)
            after = os.fstat(fd)
            require(len(data) == before.st_size == after.st_size
                    and before.st_mtime_ns == after.st_mtime_ns
                    and before.st_ctime_ns == after.st_ctime_ns, "terminal checkpoint changed during read")
            value = json.loads(data)
        finally:
            os.close(fd)
        require(isinstance(value, dict) and type(value.get("schema")) is int and value["schema"] == 1
                and value.get("work") == self.work and value.get("codex_home") == self.codex_home
                and value.get("cli_version") == "0.153.4"
                and isinstance(value.get("shell_history_dirty"), bool)
                and isinstance(value.get("history_ready"), bool), "terminal checkpoint profile mismatch")
        canonical_uuid(value.get("thread_id"))
        canonical_uuid(value.get("generation"))
        account = value.get("account_sha256")
        require(account is None or isinstance(account, str) and len(account) == 64
                and all(character in "0123456789abcdef" for character in account), "invalid checkpoint account binding")
        return value

    def persist_checkpoint(self, thread, dirty=False, history_ready=None):
        """The private binding is durable before its successful RPC becomes visible."""
        canonical_uuid(thread)
        try:
            previous = self.checkpoint_path.lstat()
        except FileNotFoundError:
            previous = None
        if previous is not None:
            require(stat.S_ISREG(previous.st_mode) and previous.st_uid == os.geteuid()
                    and stat.S_IMODE(previous.st_mode) == 0o600 and previous.st_nlink == 1
                    and 0 < previous.st_size <= 8192, "unsafe existing terminal checkpoint")
        account = (hashlib.sha256(self.auth_account_id.encode()).hexdigest()
                   if self.auth_account_id is not None else None)
        if history_ready is None:
            history_ready = bool(self.checkpoint and self.checkpoint.get("thread_id") == thread
                                 and self.checkpoint.get("history_ready"))
        value = {"schema": 1, "thread_id": thread, "work": self.work,
                 "codex_home": self.codex_home, "cli_version": "0.153.4",
                 "generation": self.generation, "account_sha256": account,
                 "shell_history_dirty": dirty, "history_ready": history_ready}
        temporary = self.checkpoint_path.with_name(".terminal-resume-" + uuid.uuid4().hex)
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW, 0o600)
        try:
            with os.fdopen(fd, "wb") as stream:
                stream.write(encode(value))
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, self.checkpoint_path)
            parent_fd = os.open(self.state_directory, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
            try:
                os.fsync(parent_fd)
            finally:
                os.close(parent_fd)
        finally:
            temporary.unlink(missing_ok=True)
        self.checkpoint = value

    async def confirm_history(self, thread):
        """Paginated thread/read(includeTurns) persists the upstream live writer."""
        try:
            # Run outside Child's stdout reader: an internal RPC needs that
            # reader to remain available to deliver its response.
            result = await self.app.call("thread/read", {"threadId": thread, "includeTurns": True})
            require(isinstance(result, dict) and isinstance(result.get("thread"), dict)
                    and result["thread"].get("id") == thread
                    and result["thread"].get("historyMode") == "paginated",
                    "history persistence was not confirmed")
            if not self.stopping and not self.failure and thread in self.thread_ids:
                # A user shell may have been admitted while the RPC was in
                # flight. Its unacknowledged injection must remain dirty.
                dirty = bool(self.checkpoint and self.checkpoint.get("shell_history_dirty"))
                self.persist_checkpoint(thread, dirty=dirty, history_ready=True)
        except asyncio.CancelledError:
            raise
        except Exception as error:
            # Failure leaves recovery unavailable; it is never evidence that
            # a thread/start or turn/start ACK produced a saved session.
            self.diagnostics("history-unconfirmed", error)
        finally:
            self.history_confirmation_pending = False

    def diagnostics(self, phase, error=None):
        path = Path(self.args.config).with_name("terminal-bridge-diagnostics.json")
        report = {"phase": phase, "methods": sorted(self.methods_seen),
                  "blocked_methods": sorted(self.blocked_methods),
                  "error_class": type(error).__name__ if error is not None else None,
                  "external_chatgpt_auth": self.external_chatgpt_auth}
        try:
            temporary = path.with_suffix(".tmp")
            fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd, "w") as stream:
                json.dump(report, stream)
            os.replace(temporary, path)
        except OSError:
            pass

    def read_chatgpt_auth(self, previous_account_id=None):
        """Read only an explicitly selected host file; never persist or forward refresh tokens."""
        descriptor = None
        try:
            require(isinstance(self.auth_source, str), "auth source missing")
            source = Path(self.auth_source)
            require(source.is_absolute() and str(source.resolve(strict=True)) == str(source), "auth source is not canonical")
            for writable in (self.work, self.tool_home):
                require(os.path.commonpath((str(source), writable)) != writable,
                        "auth source is inside a tool-writable directory")
            descriptor = os.open(source, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK)
            before = os.fstat(descriptor)
            require(stat.S_ISREG(before.st_mode) and before.st_uid == os.geteuid()
                    and stat.S_IMODE(before.st_mode) == 0o600 and before.st_nlink == 1
                    and 0 < before.st_size <= 65536, "unsafe auth source")
            raw = bytearray()
            while len(raw) <= 65536:
                block = os.read(descriptor, min(8192, 65537 - len(raw)))
                if not block:
                    break
                raw.extend(block)
            after = os.fstat(descriptor)
            require(len(raw) <= 65536 and before.st_size == after.st_size == len(raw)
                    and before.st_mtime_ns == after.st_mtime_ns
                    and before.st_ctime_ns == after.st_ctime_ns and after.st_nlink == 1,
                    "auth source changed during read")
            source_data = json.loads(raw)
            require(isinstance(source_data, dict) and source_data.get("auth_mode") == "chatgpt",
                    "source is not managed ChatGPT authentication")
            tokens = source_data.get("tokens")
            require(isinstance(tokens, dict), "managed tokens missing")
            access, account = tokens.get("access_token"), tokens.get("account_id")
            require(isinstance(access, str) and 0 < len(access) <= 32768
                    and access.isascii() and not any(character.isspace() or ord(character) < 33 for character in access),
                    "invalid access token")
            require(isinstance(account, str) and 0 < len(account) <= 256
                    and account.isascii() and not any(character.isspace() or ord(character) < 33 for character in account),
                    "invalid account identifier")
            require(previous_account_id is None or previous_account_id == account, "account mismatch")
            require(self.auth_account_id is None or self.auth_account_id == account, "account changed")
            if self.resume_thread is not None:
                require(self.checkpoint["account_sha256"] == hashlib.sha256(account.encode()).hexdigest(),
                        "resume account changed")
            # Drop the full source object before returning the two required fields.
            # In particular, refresh_token and id_token never leave this reader.
            del tokens, source_data
            raw[:] = b"\0" * len(raw)
            return {"accessToken": access, "chatgptAccountId": account, "chatgptPlanType": None}
        except Exception:
            raise AuthSourceError() from None
        finally:
            if descriptor is not None:
                os.close(descriptor)

    async def authenticate_initialize(self, request_id, params):
        try:
            result = await self.app.call("initialize", params)
            credentials = self.read_chatgpt_auth()
            account = credentials["chatgptAccountId"]
            try:
                await self.app.call("account/login/start", {"type": "chatgptAuthTokens", **credentials})
            finally:
                credentials.clear()
            self.auth_account_id = account
            self.external_chatgpt_auth = True
            self.diagnostics("authenticated")
            await self.response(request_id, result)
        except Exception:
            self.auth_failed = True
            error = AuthSourceError()
            self.fail(error)
            raise error from None
        finally:
            self.auth_ready.set()

    async def refresh_chatgpt_auth(self, message):
        request_id = message.get("id")
        try:
            require(self.external_chatgpt_auth and self.auth_source is not None, "external authentication inactive")
            params = message.get("params")
            require(isinstance(params, dict) and params.get("reason") == "unauthorized", "invalid refresh request")
            credentials = self.read_chatgpt_auth(params.get("previousAccountId"))
            try:
                await self.app.send({"id": request_id, "result": credentials})
            finally:
                credentials.clear()
        except Exception:
            # Never send source contents, IDs, decoder errors or provider errors.
            await self.app.send({"id": request_id, "error": {"code": -32001,
                                 "message": str(AuthSourceError())}})

    def fail(self, error):
        if not self.stopping:
            self.failure = str(error)[:2048]
            self.diagnostics("failed", error)
            self.stop.set()

    async def send(self, message):
        require(self.client is not None, "native terminal is disconnected")
        await self.client.send(message)

    async def notify(self, method, params):
        await self.send({"method": method, "params": params})

    async def upstream(self, message):
        result = message.get("result")
        request_id = message.get("id")
        admitted = self.pending_client.pop(request_id, None)
        if admitted:
            self.inflight.discard(request_id)
            if admitted[0] == "turn/start" and "error" in message:
                self.active_model.pop(admitted[1], None)
            if admitted[0] == "thread/start":
                self.start_inflight = False
            elif admitted[0] == "thread/resume":
                self.resume_inflight = False
        if message.get("method") == "account/chatgptAuthTokens/refresh" and request_id is not None:
            await self.refresh_chatgpt_auth(message)
            return
        if message.get("method") and request_id is not None:
            params = message.get("params") or {}
            if message["method"] != "item/tool/requestUserInput" or params.get("threadId") not in self.thread_ids:
                await self.app.send({"id": request_id, "error": {"code": -32601,
                                     "message": "Unsupported server request in restricted terminal"}})
                return
            require(len(self.server_requests) < MAX_REQUESTS, "server request limit exceeded")
            self.server_requests.add(request_id)
        if isinstance(result, dict):
            thread = result.get("thread")
            if admitted and admitted[0] in {"thread/start", "thread/resume"} and "error" not in message:
                require(isinstance(thread, dict), "thread response is missing its identity")
                thread_id = canonical_uuid(thread.get("id"))
                require(admitted[0] != "thread/resume" or thread_id == self.resume_thread,
                        "app-server returned a different resumed thread")
                try:
                    self.persist_checkpoint(thread_id, history_ready=admitted[0] == "thread/resume")
                except Exception as error:
                    self.fail(error)
                    raise
                self.thread_ids.add(thread_id)
        method, params = message.get("method"), message.get("params", {})
        if method == "turn/started":
            self.active_model[params["threadId"]] = params["turn"]["id"]
        elif method == "turn/completed":
            self.active_model.pop(params["threadId"], None)
            thread = params["threadId"]
            if (thread in self.thread_ids and self.checkpoint is not None
                    and not self.checkpoint["history_ready"] and not self.history_confirmation_pending):
                self.history_confirmation_pending = True
                task = asyncio.create_task(self.confirm_history(thread))
                self.tasks.add(task)
                task.add_done_callback(self.tasks.discard)
        if self.client is not None:
            await self.send(message)

    async def response(self, request_id, result):
        await self.send({"id": request_id, "result": result})
        self.inflight.discard(request_id)

    def policy(self, method, params):
        """Only fixed-session operations can reach the host app-server."""
        require(isinstance(params, dict), "RPC parameters must be an object")
        if method == "initialize":
            require(not self.initialized, "already initialized")
            self.initialized = True
            return {"clientInfo": {"name": "codex_tui", "title": "Codex terminal", "version": "0.153.4"},
                    "capabilities": {"experimentalApi": True}}
        require(self.initialized, "initialize required")
        if method == "account/read":
            return {"refreshToken": False}
        if method in {"model/list", "configRequirements/read", "collaborationMode/list",
                      "experimentalFeature/list", "account/rateLimits/read"}:
            return {key: params[key] for key in ("cursor", "limit", "includeHidden") if key in params}
        if method == "config/read":
            return {"cwd": self.work, "includeLayers": bool(params.get("includeLayers"))}
        if method in {"environment/info", "environment/status"}:
            return {"environmentId": "velella-restricted"}
        if method == "skills/list":
            return {"cwds": [self.work], "forceReload": False}
        if method in {"hooks/list", "mcpServerStatus/list"}:
            return {}
        if method == "thread/start":
            require(self.resume_thread is None and not self.thread_ids and not self.start_inflight,
                    "this terminal is bound to one model thread")
            allowed = {"model", "serviceTier", "personality", "ephemeral", "historyMode", "sessionStartSource",
                       "threadSource", "developerInstructions"}
            result = {key: value for key, value in params.items() if key in allowed}
            result.update(cwd=self.work, runtimeWorkspaceRoots=[self.work], approvalPolicy="never",
                          sandbox="danger-full-access", environments=self.environments(), ephemeral=False,
                          historyMode="paginated")
            return result
        thread = params.get("threadId")
        if method == "thread/resume":
            require(thread == self.resume_thread and self.checkpoint is not None
                    and not self.thread_ids and not self.resume_inflight, "unknown or already resumed thread")
            require(params.get("history") is None and params.get("path") is None,
                    "history and path overrides are not permitted")
            # Native TUI sends its own defaults. Reconstruct the entire request;
            # none of its config, permissions, provider or host paths are trusted.
            result = {key: params[key] for key in ("excludeTurns", "initialTurnsPage") if key in params}
            result.update(threadId=thread, cwd=self.work, runtimeWorkspaceRoots=[self.work],
                          approvalPolicy="never", sandbox="danger-full-access")
            return result
        if (method == "thread/read" and self.resume_thread is not None
                and thread == self.resume_thread and self.checkpoint is not None
                and thread not in self.thread_ids):
            # Native `resume UUID` resolves the exact UUID before requesting resume.
            # The pinned Rust protocol omits this field when its value is false.
            require(params.get("includeTurns", False) is False, "pre-resume lookup cannot read history")
            return {"threadId": thread, "includeTurns": False}
        require(isinstance(thread, str) and thread in self.thread_ids, "unknown or unrelated thread")
        if method == "turn/start":
            allowed = {"threadId", "clientUserMessageId", "input", "model", "effort", "summary", "personality",
                       "serviceTier", "serviceTierForTurn", "outputSchema"}
            result = {key: value for key, value in params.items() if key in allowed}
            content = result.get("input")
            require(isinstance(content, list) and 0 < len(content) <= 64, "invalid turn input")
            # Local image paths, skill paths and arbitrary capability mentions are host reads.
            require(all(isinstance(part, dict) and part.get("type") == "text" for part in content),
                    "restricted terminal currently accepts text input only")
            result.update(cwd=self.work, runtimeWorkspaceRoots=[self.work], approvalPolicy="never",
                          sandboxPolicy={"type": "externalSandbox", "networkAccess": "restricted"},
                          environments=self.environments())
            return result
        fields = {
            "turn/interrupt": {"threadId", "turnId"},
            "thread/read": {"threadId", "includeTurns"},
            "thread/turns/list": {"threadId", "cursor", "limit", "sortDirection", "itemsView"},
            "thread/items/list": {"threadId", "turnId", "cursor", "limit", "sortDirection"},
            "thread/name/set": {"threadId", "name"},
            "thread/unsubscribe": {"threadId"},
            "thread/backgroundTerminals/clean": {"threadId"},
            "thread/backgroundTerminals/list": {"threadId"},
            "thread/backgroundTerminals/terminate": {"threadId", "processId"},
        }
        require(method in fields, f"unsupported operation in restricted terminal: {method}")
        return {key: value for key, value in params.items() if key in fields[method]}

    def environments(self):
        return [{"environmentId": "velella-restricted", "cwd": self.work,
                 "runtimeWorkspaceRoots": [self.work]}]

    async def dispatch(self, message):
        request_id = message.get("id")
        try:
            await self.handle(message)
        except asyncio.CancelledError:
            raise
        except Exception as error:
            self.blocked_methods.add(str(message.get("method", ""))[:100])
            self.diagnostics("request-rejected", error)
            if request_id is not None and not self.stopping:
                await self.send({"id": request_id, "error": {"code": -32602, "message": str(error)[:2048]}})
                self.inflight.discard(request_id)
            else:
                self.fail(error)

    async def handle(self, message):
        request_id, method = message.get("id"), message.get("method")
        if isinstance(method, str):
            self.methods_seen.add(method[:100])
            self.diagnostics("running")
        require(not self.stopping and not self.failure, "terminal bridge is stopping")
        if self.auth_source and method != "initialize":
            await self.auth_ready.wait()
            require(not self.auth_failed and self.external_chatgpt_auth
                    and not self.stopping and not self.failure, "local ChatGPT authentication failed")
        if method is None:
            require(request_id in self.server_requests and ("result" in message or "error" in message),
                    "unsolicited server response")
            self.server_requests.remove(request_id)
            await self.app.send(message)
            return
        if request_id is None:
            require(method == "initialized" and self.initialized, "unsupported client notification")
            await self.app.send({"method": "initialized"})
            return
        params = message.get("params") or {}
        if method == "thread/shellCommand":
            require(self.initialized and isinstance(params, dict), "invalid shell parameters")
            thread = params.get("threadId")
            require(thread in self.thread_ids, "unknown shell thread")
            command = params.get("command")
            require(isinstance(command, str) and "\0" not in command
                    and 0 < len(command.encode()) <= 65536, "invalid shell command")
            timeout = params.get("timeoutMs")
            require(timeout is None or isinstance(timeout, int) and not isinstance(timeout, bool)
                    and 0 <= timeout <= 600000, "invalid shell timeout")
            require(len(self.shells) < MAX_QUEUED, "shell queue limit exceeded")
            require(thread not in self.shells, "another shell command is pending for this thread")
            # Any stop after admission but before the injection ACK leaves a
            # conservative marker; the command is never replayed on recovery.
            self.persist_checkpoint(thread, dirty=True)
            state = {"thread": thread, "command": command, "timeout": 120 if timeout is None else timeout / 1000,
                     "turn": uuid.uuid4().hex, "item": uuid.uuid4().hex,
                     "process": uuid.uuid4().hex, "cancel": asyncio.Event(), "done": asyncio.Event(),
                     "rejected": self.commands >= MAX_COMMANDS}
            self.shells[thread] = state
            self.commands += 1
            await self.response(request_id, {})
            task = asyncio.create_task(self.shell(state))
            self.tasks.add(task)
            task.add_done_callback(self.tasks.discard)
            return
        params = self.policy(method, params)
        if method == "initialize" and self.auth_source:
            await self.authenticate_initialize(request_id, params)
            return
        thread = params.get("threadId")
        if method == "turn/interrupt" and thread in self.shells:
            self.shells[thread]["cancel"].set()
            await self.response(request_id, {})
            return
        if method == "turn/start":
            if thread in self.shells:
                await self.shells[thread]["done"].wait()
            require(not self.stopping and not self.failure, "shell history persistence failed; turn was not sent")
            require(thread not in self.active_model, "another model turn is pending")
            self.active_model[thread] = "pending"
        elif method == "thread/start":
            self.start_inflight = True
        elif method == "thread/resume":
            self.resume_inflight = True
        self.pending_client[request_id] = (method, thread)
        await self.app.send({"id": request_id, "method": method, "params": params})

    async def shell(self, state):
        thread, turn_id, item_id = state["thread"], state["turn"], state["item"]
        started, output, exit_code = time.monotonic(), "", 1
        turn_started = False
        item = {"type": "commandExecution", "id": item_id, "command": state["command"],
                "cwd": self.work, "processId": None, "source": "userShell", "status": "inProgress",
                "commandActions": [], "aggregatedOutput": None, "exitCode": None, "durationMs": None}
        turn = {"id": turn_id, "items": [], "itemsView": "full", "status": "inProgress", "error": None,
                "startedAt": int(time.time()), "completedAt": None, "durationMs": None}
        process_started = False
        try:
            # !shell is not allowed to race a model turn or its history writes.
            while thread in self.active_model and not state["cancel"].is_set():
                require(time.monotonic() - started < 120, "waiting for active model turn timed out")
                await asyncio.sleep(.05)
            await self.notify("turn/started", {"threadId": thread, "turn": turn})
            turn_started = True
            await self.notify("item/started", {"threadId": thread, "turnId": turn_id, "item": item,
                                               "startedAtMs": int(time.time() * 1000)})
            if state["cancel"].is_set():
                raise RpcFailure("Command cancelled before execution.")
            if state.get("rejected"):
                raise RpcFailure("This terminal reached its 64-command limit. Start a new restricted terminal.")
            await self.executor.call("process/start", {
                "processId": state["process"], "argv": ["/bin/bash", "--noprofile", "--norc", "-c", state["command"]],
                "cwd": Path(self.work).as_uri(), "env": {}, "tty": False, "pipeStdin": False,
                "sandbox": None})
            process_started = True
            deadline, sequence = time.monotonic() + state["timeout"], 0
            decoders = {name: codecs.getincrementaldecoder("utf-8")("replace") for name in ("stdout", "stderr")}
            count = 0
            while True:
                if state["cancel"].is_set() or time.monotonic() >= deadline:
                    await self.executor.call("process/terminate", {"processId": state["process"]})
                    raise RpcFailure("Command cancelled." if state["cancel"].is_set() else "Command timed out.")
                response = await self.executor.call("process/read", {
                    "processId": state["process"], "afterSeq": sequence, "maxBytes": 65536, "waitMs": 50})
                sequence = response["nextSeq"]
                for chunk in response["chunks"]:
                    data = base64.b64decode(chunk["chunk"], validate=True)
                    count += len(data)
                    require(count <= MAX_OUTPUT, "command output limit exceeded")
                    text = decoders[chunk["stream"]].decode(data)
                    output += text
                    if text:
                        await self.notify("item/commandExecution/outputDelta", {
                            "threadId": thread, "turnId": turn_id, "itemId": item_id, "delta": text})
                require(not response.get("failure"), response.get("failure") or "executor failed")
                if response["closed"] and not response["chunks"]:
                    output += "".join(decoder.decode(b"", final=True) for decoder in decoders.values())
                    exit_code = response["exitCode"]
                    break
                await asyncio.sleep(.02)
        except asyncio.CancelledError:
            state["cancel"].set()
            output += "\nCommand cancelled: terminal disconnected.\n"
            exit_code = 130
        except Exception as error:
            output += "\n" + str(error)[:2048] + "\n"
            exit_code = 130 if state["cancel"].is_set() else 1
        finally:
            if process_started:
                try:
                    await self.executor.call("process/terminate", {"processId": state["process"]}, 2)
                except Exception:
                    pass
            try:
                if not self.stopping:
                    duration = int((time.monotonic() - started) * 1000)
                    record = ("<user_shell_command>\n<command>\n" + state["command"] + "\n</command>\n<result>\n"
                              + f"Exit code: {exit_code}\nDuration: {duration / 1000:.4f} seconds\nOutput:\n"
                              + output + "\n</result>\n</user_shell_command>")
                    # Persist before releasing queued turn/start. This never calls the model.
                    await self.app.call("thread/inject_items", {"threadId": thread, "items": [
                        {"type": "message", "role": "user", "content": [{"type": "input_text", "text": record}]}]})
                    self.persist_checkpoint(thread, history_ready=True)
                    item.update(status="completed" if exit_code == 0 else "failed",
                                aggregatedOutput=output, exitCode=exit_code, durationMs=duration)
                    if not turn_started:
                        await self.notify("turn/started", {"threadId": thread, "turn": turn})
                    await self.notify("item/completed", {"threadId": thread, "turnId": turn_id, "item": item,
                                                         "completedAtMs": int(time.time() * 1000)})
                    turn.update(items=[item], status="completed", completedAt=int(time.time()), durationMs=duration)
                    await self.notify("turn/completed", {"threadId": thread, "turn": turn})
            except Exception as error:
                self.fail(error)
            finally:
                self.shells.pop(thread, None)
                state["done"].set()

    async def connection(self, reader, writer):
        if self.accepted:
            writer.close()
            await writer.wait_closed()
            return
        self.accepted = True
        try:
            ws = WebSocket(reader, writer)
            await ws.handshake()
            self.client = ws
            while not self.stopping:
                message = await ws.receive()
                if message is None:
                    break
                request_id = message.get("id")
                if message.get("method") is not None and request_id is not None:
                    require(isinstance(request_id, (str, int)) and not isinstance(request_id, bool), "invalid RPC id")
                    require(not isinstance(request_id, str) or not request_id.startswith(INTERNAL), "reserved RPC id")
                    require(request_id not in self.inflight and len(self.inflight) < MAX_REQUESTS,
                            "duplicate RPC id or client request limit exceeded")
                    self.inflight.add(request_id)
                require(len(self.client_tasks) < MAX_REQUESTS, "client task limit exceeded")
                task = asyncio.create_task(self.dispatch(message))
                self.client_tasks.add(task)
                task.add_done_callback(self.client_tasks.discard)
                # Admission tasks get a turn before accepting the next frame.
                await asyncio.sleep(0)
        except (asyncio.IncompleteReadError, ConnectionError):
            pass
        except Exception as error:
            self.fail(error)
        finally:
            self.stop.set()
            writer.close()
            try:
                await writer.wait_closed()
            except ConnectionError:
                pass

    def read_lease(self):
        try:
            if not os.read(self.lease_fd, 4096):
                self.stop.set()
                asyncio.get_running_loop().remove_reader(self.lease_fd)
        except BlockingIOError:
            pass
        except OSError:
            self.stop.set()

    async def stop_model_adapter(self):
        path = Path(self.args.config).with_name("exec-server.pid.json")
        if not path.exists():
            return
        receipt = json.loads(path.read_text())
        pid, expected = receipt["pid"], str(receipt["start"])
        require(isinstance(pid, int) and pid > 1, "invalid model adapter receipt")
        def identity():
            try:
                return Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()[19]
            except FileNotFoundError:
                return None
        if identity() != expected:
            return
        try:
            fd = os.pidfd_open(pid)
        except ProcessLookupError:
            return
        try:
            if identity() != expected:
                return
            signal.pidfd_send_signal(fd, signal.SIGTERM)
            deadline = time.monotonic() + 6
            poller = select.poll()
            poller.register(fd, select.POLLIN)
            while not poller.poll(0):
                require(time.monotonic() < deadline, "model adapter cleanup timed out")
                await asyncio.sleep(.05)
        except ProcessLookupError:
            pass
        finally:
            os.close(fd)

    async def freeze_app(self):
        if not self.app:
            return
        self.app.signal_process(signal.SIGSTOP)
        deadline = time.monotonic() + 2
        while True:
            result = os.waitid(os.P_PID, self.app.process.pid,
                               os.WSTOPPED | os.WEXITED | os.WNOHANG | os.WNOWAIT)
            if result is not None:
                return
            require(time.monotonic() < deadline, "app-server suspend timed out")
            await asyncio.sleep(.01)

    async def run(self):
        loop = asyncio.get_running_loop()
        self.lease_fd = sys.stdin.fileno()
        os.set_blocking(self.lease_fd, False)
        loop.add_reader(self.lease_fd, self.read_lease)
        for sig in (signal.SIGTERM, signal.SIGINT):
            loop.add_signal_handler(sig, self.stop.set)
        try:
            script = Path(__file__).resolve().with_name("exec-server.py")
            if not script.exists():
                script = Path(__file__).resolve().with_name("restricted-exec-server.py")
            require(script.is_file(), "fixed executor script missing")
            self.executor = Child(["/usr/bin/python3", "-I", str(script), "--config", self.args.config],
                                  {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8"}, self, "shell-executor")
            await self.executor.start()
            await self.executor.call("initialize", {"clientName": "velella-native-terminal", "resumeSessionId": None})
            self.app = Child([self.args.codex, *self.args.codex_arg, "app-server", "--listen", "stdio://"],
                             dict(os.environ), self, "app-server")
            await self.app.start()
            self.server = await asyncio.start_unix_server(self.connection, path=str(self.socket), limit=MAX_FRAME + 1)
            os.chmod(self.socket, 0o600)
            def starttime(pid):
                return Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()[19]
            print(json.dumps({"ready": True, "socket": str(self.socket), "pid": os.getpid(),
                              "start": starttime(os.getpid()),
                              "app_server_pid": self.app.process.pid,
                              "app_server_start": starttime(self.app.process.pid),
                              "shell_adapter_pid": self.executor.process.pid,
                              "shell_adapter_start": starttime(self.executor.process.pid)}), flush=True)
            self.diagnostics("ready")
            await self.stop.wait()
        finally:
            self.stopping = True
            loop.remove_reader(self.lease_fd)
            if self.server:
                self.server.close()
                await self.server.wait_closed()
            for task in list(self.client_tasks):
                task.cancel()
            await asyncio.gather(*self.client_tasks, return_exceptions=True)
            for state in self.shells.values():
                state["cancel"].set()
            for task in list(self.tasks):
                task.cancel()
            if self.tasks:
                await asyncio.gather(*self.tasks, return_exceptions=True)
            try:
                await self.freeze_app()
                await self.stop_model_adapter()
            except Exception as error:
                self.failure = self.failure or str(error)
            finally:
                if self.executor:
                    await self.executor.close()
                if self.app:
                    await self.app.close(graceful=False)
            if self.client:
                self.client.writer.close()
            if self.socket.exists():
                self.socket.unlink()
            # The Rust owner normally removes this directory. A host SIGKILL
            # instead closes our stdin lease, so remove only our now-empty,
            # unchanged private transport directory, never a directory tree.
            parent = self.socket.parent
            if parent.parent == Path("/tmp") and parent.name.startswith("vl-tui-"):
                try:
                    meta = parent.lstat()
                    if (stat.S_ISDIR(meta.st_mode) and meta.st_uid == os.geteuid()
                            and not meta.st_mode & 0o077
                            and (meta.st_dev, meta.st_ino) == self.socket_parent_identity):
                        parent.rmdir()
                except OSError:
                    pass
        self.diagnostics("failed" if self.failure else "closed")
        if self.failure:
            raise RuntimeError(self.failure)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--socket", required=True)
    parser.add_argument("--codex", required=True)
    parser.add_argument("--config", required=True)
    parser.add_argument("--codex-arg", action="append", default=[])
    parser.add_argument("--chatgpt-auth-source")
    parser.add_argument("--generation", required=True)
    parser.add_argument("--resume-thread")
    args = parser.parse_args()
    asyncio.run(Bridge(args).run())


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"restricted terminal bridge: {error}", file=sys.stderr)
        sys.exit(1)
