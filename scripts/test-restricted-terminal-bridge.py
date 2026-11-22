#!/usr/bin/env python3
"""Offline safety regressions for the native-terminal protocol bridge.

Uses only in-memory transports and fixed subprocesses. No model, credentials,
host Codex profile, network listener, or installed desktop application is used.
"""
import asyncio
import base64
import copy
import importlib.util
import json
import os
from pathlib import Path
import struct
import tempfile
import types
import unittest
from unittest import mock

MODULE_PATH = Path(__file__).parent / "fixtures/restricted-terminal-bridge.py"
SPEC = importlib.util.spec_from_file_location("restricted_terminal_bridge", MODULE_PATH)
bridge_module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bridge_module)


class MemoryWriter:
    def __init__(self):
        self.data = bytearray()
        self.closed = False

    def write(self, data):
        self.data.extend(data)

    async def drain(self):
        await asyncio.sleep(0)

    def close(self):
        self.closed = True

    async def wait_closed(self):
        return None


class RecordingPeer:
    def __init__(self):
        self.sent = []
        self.calls = []
        self.inject_started = asyncio.Event()
        self.inject_release = asyncio.Event()
        self.inject_release.set()

    async def send(self, message):
        self.sent.append(copy.deepcopy(message))

    async def call(self, method, params, timeout=10):
        self.calls.append((method, copy.deepcopy(params)))
        if method == "thread/inject_items":
            self.inject_started.set()
            await self.inject_release.wait()
        return {}


class FixedExecutor:
    def __init__(self, chunks=None):
        self.calls = []
        self.reads = list(chunks or [])

    async def call(self, method, params, timeout=10):
        self.calls.append((method, copy.deepcopy(params)))
        if method == "process/read":
            if self.reads:
                return self.reads.pop(0)
            return {"chunks": [], "nextSeq": 3, "closed": True, "exitCode": 0, "failure": None}
        return {}


class AuthPeer(RecordingPeer):
    def __init__(self):
        super().__init__()
        self.login_started = asyncio.Event()
        self.login_release = asyncio.Event()
        self.login_release.set()
        self.login_error = None

    async def call(self, method, params, timeout=10):
        self.calls.append((method, copy.deepcopy(params)))
        if method == "initialize":
            return {"userAgent": "offline-auth-fixture"}
        if method == "account/login/start":
            self.login_started.set()
            await self.login_release.wait()
            if self.login_error:
                raise bridge_module.RpcFailure(self.login_error)
            return {"type": "chatgptAuthTokens"}
        return {}


def handshake():
    key = base64.b64encode(bytes(range(16))).decode()
    return (
        "GET /rpc HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\n"
        "Connection: Upgrade\r\nSec-WebSocket-Version: 13\r\n"
        f"Sec-WebSocket-Key: {key}\r\n\r\n"
    ).encode()


def masked_frame(payload, opcode=1, final=True):
    mask = b"\x07\x29\x53\x91"
    flag = 0x80 if final else 0
    if len(payload) < 126:
        header = bytes((flag | opcode, 0x80 | len(payload)))
    elif len(payload) <= 65535:
        header = bytes((flag | opcode, 0x80 | 126)) + struct.pack("!H", len(payload))
    else:
        header = bytes((flag | opcode, 0x80 | 127)) + struct.pack("!Q", len(payload))
    return header + mask + bytes(value ^ mask[index % 4] for index, value in enumerate(payload))


class WebSocketSafety(unittest.IsolatedAsyncioTestCase):
    def transport(self, data):
        reader = asyncio.StreamReader(limit=bridge_module.MAX_FRAME + 1)
        reader.feed_data(data)
        reader.feed_eof()
        writer = MemoryWriter()
        return bridge_module.WebSocket(reader, writer), writer

    async def test_fragmented_json_and_ping_preserve_exact_message(self):
        expected = {"id": 1, "method": "initialize", "params": {"note": "中文"}}
        data = json.dumps(expected, ensure_ascii=False).encode()
        split = data.index("中".encode()) + 1  # Split a multi-byte codepoint.
        ws, writer = self.transport(
            masked_frame(data[:split], final=False)
            + masked_frame(b"still-reading", opcode=9)
            + masked_frame(data[split:], opcode=0)
        )
        self.assertEqual(await ws.receive(), expected)
        self.assertEqual(bytes(writer.data), b"\x8a\rstill-reading")

    async def test_rejects_unmasked_reserved_binary_and_invalid_continuations(self):
        frames = [
            b"\x81\x02{}",
            b"\xc1\x82" + b"\0" * 4 + b"{}",
            masked_frame(b"{}", opcode=2),
            masked_frame(b"{}", opcode=0),
            masked_frame(b"{", final=False) + masked_frame(b"}", opcode=1),
            masked_frame(b"x", opcode=9, final=False),
            masked_frame(b"x", opcode=11),
        ]
        for frame in frames:
            with self.subTest(frame=frame[:4].hex()):
                ws, _ = self.transport(frame)
                with self.assertRaises(ValueError):
                    await ws.receive()

    async def test_oversized_frame_is_rejected_before_payload_read(self):
        ws, _ = self.transport(
            b"\x81\xff" + struct.pack("!Q", bridge_module.MAX_FRAME + 1)
        )
        with self.assertRaisesRegex(ValueError, "exceeds limit"):
            await ws.receive()

    async def test_fragment_total_cannot_exceed_message_limit(self):
        first = b"x" * bridge_module.MAX_FRAME
        ws, _ = self.transport(
            masked_frame(first, final=False) + b"\x80\x81"
        )
        with self.assertRaisesRegex(ValueError, "exceeds limit"):
            await ws.receive()

    async def test_json_must_be_utf8_object(self):
        for payload in [b"[]", b"null", b'"text"', b"\xff", b"{"]:
            with self.subTest(payload=payload):
                ws, _ = self.transport(masked_frame(payload))
                with self.assertRaises((ValueError, UnicodeDecodeError)):
                    await ws.receive()

    async def test_close_frame_finishes_transport(self):
        ws, _ = self.transport(masked_frame(b"", opcode=8))
        self.assertIsNone(await ws.receive())

    async def test_handshake_exact_path_and_unique_headers(self):
        key = base64.b64encode(bytes(range(16))).decode()
        valid = handshake()
        ws, writer = self.transport(valid)
        await ws.handshake()
        self.assertTrue(bytes(writer.data).startswith(b"HTTP/1.1 101 "))
        invalid = [
            valid.replace(b"GET /rpc ", b"GET /other "),
            valid.replace(b"Host: localhost", b"Host: localhost\r\nhost: other"),
            valid.replace(b"Version: 13", b"Version: 12"),
            valid.replace(key.encode(), b"AAAA"),
        ]
        for header in invalid:
            ws, _ = self.transport(header)
            with self.assertRaises(ValueError):
                await ws.handshake()


class PolicySafety(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="vl-bridge-policy-")
        root = Path(self.directory.name)
        self.work = root / "work"
        self.home = root / "tool-home"
        self.work.mkdir(mode=0o700)
        self.home.mkdir(mode=0o700)
        (root / "controller" / "codex").mkdir(mode=0o700, parents=True)
        config = root / "exec-server.json"
        config.write_text(json.dumps({
            "runtime": "/bin/true", "work": str(self.work),
            "home": str(self.home), "read_only": [],
        }))
        config.chmod(0o600)
        args = types.SimpleNamespace(
            socket=str(root / "rpc.sock"), config=str(config),
            codex="/bin/true", codex_arg=[],
            generation="01998765-4321-7000-8000-000000000002",
        )
        self.bridge = bridge_module.Bridge(args)
        self.bridge.app = RecordingPeer()
        self.bridge.client = RecordingPeer()
        self.bridge.thread_ids.add("01998765-4321-7000-8000-000000000001")
        self.bridge.initialized = True

    async def asyncTearDown(self):
        self.bridge.stopping = True
        for task in list(self.bridge.tasks):
            task.cancel()
        for task in list(self.bridge.client_tasks):
            task.cancel()
        await asyncio.gather(
            *list(self.bridge.tasks), *list(self.bridge.client_tasks), return_exceptions=True,
        )
        self.directory.cleanup()

    async def rejected(self, message):
        before = len(self.bridge.app.sent)
        try:
            await asyncio.wait_for(self.bridge.handle(message), 1)
        except (ValueError, bridge_module.RpcFailure):
            pass
        self.assertEqual(len(self.bridge.app.sent), before,
                         f"unsafe RPC was forwarded: {message.get('method')}")

    async def test_host_execution_and_config_mutation_apis_never_forward(self):
        methods = [
            "command/exec", "command/exec/write", "fs/readFile", "fs/writeFile",
            "config/value/write", "config/batchWrite", "config/replace",
            "mcpServer/oauth/login", "plugin/install", "skills/extraRoots/set",
            "thread/inject_items", "thread/resume", "thread/fork",
            "thread/revert", "project/import", "unknown/execute",
        ]
        for number, method in enumerate(methods):
            with self.subTest(method=method):
                await self.rejected({
                    "id": number, "method": method,
                    "params": {"threadId": "01998765-4321-7000-8000-000000000001", "cwd": "/"},
                })

    async def test_internal_ids_and_invalid_types_never_forward(self):
        for request_id in [True, [], {}, bridge_module.INTERNAL + "forged"]:
            with self.subTest(request_id=request_id):
                self.bridge.accepted = False
                before = len(self.bridge.app.sent)
                await self.connection_messages([{
                    "id": request_id, "method": "model/list", "params": {},
                }])
                self.assertEqual(len(self.bridge.app.sent), before)

    async def test_unknown_thread_cannot_start_model_or_shell(self):
        for method in ["turn/start", "turn/steer", "thread/shellCommand", "turn/interrupt"]:
            with self.subTest(method=method):
                await self.rejected({
                    "id": method, "method": method,
                    "params": {"threadId": "foreign-thread", "command": "true", "input": []},
                })

    async def test_thread_start_cannot_restore_host_environment_or_configuration(self):
        message = {
            "id": 10, "method": "thread/start",
            "params": {
                "cwd": "/", "environmentId": "local", "environment": {"id": "local"},
                "sandbox": "danger-full-access", "approvalPolicy": "never",
                "config": {"features": {"hooks": True}, "mcp_servers": {"evil": {}}},
                "dynamicTools": [{"name": "host_exec"}], "experimentalRawEvents": True,
            },
        }
        try:
            await self.bridge.handle(message)
        except (ValueError, bridge_module.RpcFailure):
            pass
        if self.bridge.app.sent:
            params = self.bridge.app.sent[-1]["params"]
            self.assertEqual(params.get("cwd"), str(self.work))
            self.assertNotEqual(params.get("environmentId"), "local")
            self.assertNotEqual(params.get("environment"), {"id": "local"})
            self.assertNotIn("config", params)
            self.assertNotIn("dynamicTools", params)

    async def test_local_files_and_skills_cannot_bypass_restricted_filesystem(self):
        for part in [
            {"type": "localImage", "path": "/private/file.png"},
            {"type": "localAudio", "path": "/private/file.wav"},
            {"type": "skill", "name": "fixture", "path": "/private/SKILL.md"},
            {"type": "mention", "name": "fixture", "path": "/private/file"},
        ]:
            with self.subTest(kind=part["type"]):
                await self.rejected({
                    "id": part["type"], "method": "turn/start",
                    "params": {"threadId": "01998765-4321-7000-8000-000000000001", "input": [part]},
                })

    async def connection_messages(self, messages):
        reader = asyncio.StreamReader(limit=bridge_module.MAX_FRAME + 1)
        writer = MemoryWriter()
        reader.feed_data(handshake() + b"".join(
            masked_frame(json.dumps(message).encode()) for message in messages
        ) + masked_frame(b"", opcode=8))
        reader.feed_eof()
        await asyncio.wait_for(self.bridge.connection(reader, writer), 1)
        return writer

    async def test_duplicate_request_id_does_not_reach_a_second_operation(self):
        await self.connection_messages([
            {"id": 22, "method": "model/list", "params": {}},
            {"id": 22, "method": "thread/shellCommand",
             "params": {"threadId": "01998765-4321-7000-8000-000000000001", "command": "true"}},
        ])
        self.assertEqual([message["method"] for message in self.bridge.app.sent], ["model/list"])
        self.assertEqual(self.bridge.commands, 0)
        self.assertFalse(self.bridge.shells)

    async def test_waiting_model_turn_does_not_block_escape_or_disconnect(self):
        state = {"cancel": asyncio.Event(), "done": asyncio.Event()}
        self.bridge.shells["01998765-4321-7000-8000-000000000001"] = state
        await self.connection_messages([
            self.model_turn(31),
            {"id": 32, "method": "turn/interrupt",
             "params": {"threadId": "01998765-4321-7000-8000-000000000001", "turnId": "shell-turn"}},
        ])
        self.assertTrue(state["cancel"].is_set(), "reader blocked behind waiting turn/start")
        self.assertTrue(self.bridge.stop.is_set(), "close was not read while turn/start waited")
        self.assertFalse(self.bridge.app.sent, "model must wait for shell persistence")

    def model_turn(self, request_id):
        return {
            "id": request_id, "method": "turn/start",
            "params": {"threadId": "01998765-4321-7000-8000-000000000001",
                       "input": [{"type": "text", "text": "read previous command output"}]},
        }

    async def start_shell(self, request_id=40, timeout_ms=None):
        await self.bridge.handle({
            "id": request_id, "method": "thread/shellCommand",
            "params": {"threadId": "01998765-4321-7000-8000-000000000001", "command": "fixed command",
                       "timeoutMs": timeout_ms},
        })
        return self.bridge.shells["01998765-4321-7000-8000-000000000001"]

    async def assert_failed_shell_keeps_model_usable(self, state, diagnostic):
        await asyncio.wait_for(state["done"].wait(), 1)
        completed = [
            message["params"]["item"] for message in self.bridge.client.sent
            if message.get("method") == "item/completed"
        ]
        self.assertEqual(len(completed), 1)
        item = completed[0]
        self.assertEqual(item["status"], "failed")
        self.assertNotEqual(item["exitCode"], 0)
        self.assertIn(diagnostic, item["aggregatedOutput"])
        self.assertIsNone(self.bridge.failure)
        self.assertFalse(self.bridge.stop.is_set())
        self.assertNotIn("01998765-4321-7000-8000-000000000001", self.bridge.shells)
        self.assertEqual(self.bridge.app.calls[-1][0], "thread/inject_items")
        self.assertIn(
            diagnostic,
            self.bridge.app.calls[-1][1]["items"][0]["content"][0]["text"],
        )
        await self.bridge.handle(self.model_turn(80))
        self.assertEqual(self.bridge.app.sent[-1]["method"], "turn/start")
        self.assertEqual(self.bridge.app.sent[-1]["params"]["threadId"], "01998765-4321-7000-8000-000000000001")
        return item

    async def test_short_shell_timeout_terminates_process_and_keeps_model_usable(self):
        class PendingExecutor(FixedExecutor):
            async def call(inner_self, method, params, timeout=10):
                inner_self.calls.append((method, copy.deepcopy(params)))
                if method == "process/read":
                    return {"chunks": [], "nextSeq": 0, "closed": False,
                            "exitCode": None, "failure": None}
                return {}

        self.bridge.executor = PendingExecutor()
        state = await self.start_shell(timeout_ms=20)
        await self.assert_failed_shell_keeps_model_usable(state, "Command timed out.")
        calls = self.bridge.executor.calls
        self.assertEqual(sum(method == "process/start" for method, _ in calls), 1)
        self.assertTrue(any(method == "process/read" for method, _ in calls))
        self.assertTrue(any(
            method == "process/terminate" and params["processId"] == state["process"]
            for method, params in calls
        ), "timeout did not terminate the exact owned shell process")

    async def test_output_limit_terminates_process_and_keeps_model_usable(self):
        pages = []
        remaining = bridge_module.MAX_OUTPUT + 1
        sequence = 0
        while remaining:
            size = min(65536, remaining)
            remaining -= size
            sequence += 1
            pages.append({
                "chunks": [{"seq": sequence, "stream": "stdout",
                            "chunk": base64.b64encode(b"x" * size).decode()}],
                "nextSeq": sequence, "closed": False,
                "exitCode": None, "failure": None,
            })
        self.bridge.executor = FixedExecutor(pages)
        state = await self.start_shell()
        item = await self.assert_failed_shell_keeps_model_usable(
            state, "command output limit exceeded",
        )
        self.assertLessEqual(
            len(item["aggregatedOutput"].encode()), bridge_module.MAX_OUTPUT + 2048,
        )
        emitted = "".join(
            message["params"]["delta"] for message in self.bridge.client.sent
            if message.get("method") == "item/commandExecution/outputDelta"
        )
        self.assertEqual(len(emitted.encode()), bridge_module.MAX_OUTPUT)
        self.assertTrue(any(
            method == "process/terminate" and params["processId"] == state["process"]
            for method, params in self.bridge.executor.calls
        ), "output overflow did not terminate the exact owned shell process")

    async def test_sixty_fifth_shell_is_a_failed_item_without_execution_or_session_exit(self):
        self.bridge.commands = bridge_module.MAX_COMMANDS
        self.bridge.executor = FixedExecutor()
        state = await self.start_shell()
        await self.assert_failed_shell_keeps_model_usable(state, "64-command limit")
        self.assertEqual(self.bridge.commands, bridge_module.MAX_COMMANDS + 1)
        self.assertFalse(self.bridge.executor.calls,
                         "the shell admission limit must apply before creating a process")

    async def test_model_turn_is_reserved_before_first_upstream_notification(self):
        self.bridge.executor = FixedExecutor()
        await self.bridge.handle(self.model_turn(40))
        self.assertIn("01998765-4321-7000-8000-000000000001", self.bridge.active_model)
        state = await self.start_shell(41)
        await asyncio.sleep(.08)
        self.assertFalse(self.bridge.executor.calls, "shell raced an admitted model request")
        state["cancel"].set()
        await asyncio.wait_for(state["done"].wait(), 1)
        self.assertFalse(self.bridge.executor.calls, "cancelled queued shell executed")

    async def test_shell_drains_closed_pages_decodes_streams_and_waits_for_history_ack(self):
        def chunk(seq, stream, data):
            return {"seq": seq, "stream": stream, "chunk": base64.b64encode(data).decode()}

        self.bridge.executor = FixedExecutor([
            {"chunks": [chunk(1, "stdout", b"\xe4\xb8"), chunk(2, "stderr", b"error\n")],
             "nextSeq": 2, "closed": True, "exitCode": 0, "failure": None},
            {"chunks": [chunk(3, "stdout", b"\xad\n")],
             "nextSeq": 3, "closed": True, "exitCode": 0, "failure": None},
        ])
        self.bridge.app.inject_release.clear()
        state = await self.start_shell()
        await asyncio.wait_for(self.bridge.app.inject_started.wait(), 1)
        self.assertTrue(self.bridge.read_checkpoint()["shell_history_dirty"])
        model_task = asyncio.create_task(self.bridge.handle(self.model_turn(42)))
        self.bridge.client_tasks.add(model_task)
        await asyncio.sleep(.01)
        self.assertFalse(state["done"].is_set(), "history ACK has not arrived")
        self.assertFalse(self.bridge.app.sent, "model turn passed unpersisted shell output")
        self.bridge.app.inject_release.set()
        await asyncio.wait_for(model_task, 1)
        self.assertTrue(state["done"].is_set())
        self.assertFalse(self.bridge.read_checkpoint()["shell_history_dirty"])
        self.assertEqual(self.bridge.app.sent[-1]["method"], "turn/start")
        record = self.bridge.app.calls[-1][1]["items"][0]
        self.assertEqual(record["role"], "user")
        self.assertIn("error\n中\n", record["content"][0]["text"])
        completed = [
            item for item in self.bridge.client.sent
            if item.get("method") == "item/completed"
        ]
        self.assertEqual(completed[-1]["params"]["item"]["aggregatedOutput"], "error\n中\n")
        reads = [entry for entry in self.bridge.executor.calls if entry[0] == "process/read"]
        self.assertEqual(len(reads), 3, "closed=true is insufficient until buffered pages are drained")

    async def test_failed_history_persistence_never_releases_model_request(self):
        class FailingHistory(RecordingPeer):
            async def call(inner_self, method, params, timeout=10):
                inner_self.inject_started.set()
                await inner_self.inject_release.wait()
                raise bridge_module.RpcFailure("fixed history failure")

        self.bridge.app = FailingHistory()
        self.bridge.app.inject_release.clear()
        self.bridge.executor = FixedExecutor()
        state = await self.start_shell()
        await asyncio.wait_for(self.bridge.app.inject_started.wait(), 1)
        model_task = asyncio.create_task(self.bridge.dispatch(self.model_turn(44)))
        self.bridge.client_tasks.add(model_task)
        await asyncio.sleep(.01)
        self.bridge.app.inject_release.set()
        await asyncio.wait_for(model_task, 1)
        await asyncio.wait_for(state["done"].wait(), 1)
        self.assertFalse(self.bridge.app.sent)
        self.assertTrue(self.bridge.failure)
        self.assertTrue(self.bridge.stop.is_set())
        self.assertTrue(self.bridge.read_checkpoint()["shell_history_dirty"])


class ResumeSafety(unittest.IsolatedAsyncioTestCase):
    THREAD = "01998765-4321-7000-8000-000000000001"
    NEXT_GENERATION = "01998765-4321-7000-8000-000000000003"

    async def asyncSetUp(self):
        await PolicySafety.asyncSetUp(self)

    async def asyncTearDown(self):
        await PolicySafety.asyncTearDown(self)

    def prepare_resume(self):
        self.bridge.persist_checkpoint(self.THREAD, history_ready=True)
        args = types.SimpleNamespace(**vars(self.bridge.args))
        args.generation, args.resume_thread = self.NEXT_GENERATION, self.THREAD
        return args

    def resumed_bridge(self, args=None):
        bridge = bridge_module.Bridge(args or self.prepare_resume())
        bridge.initialized = True
        bridge.app, bridge.client = RecordingPeer(), RecordingPeer()
        self.bridge = bridge
        return bridge

    async def test_checkpoint_only_follows_admitted_success_before_client_response(self):
        bridge = self.bridge
        bridge.thread_ids.clear()
        result = {"thread": {"id": self.THREAD}}
        await bridge.upstream({"id": 200, "result": result})
        self.assertFalse(bridge.checkpoint_path.exists())
        await bridge.handle({"id": 201, "method": "thread/start", "params": {
            "ephemeral": True, "historyMode": "legacy"}})
        self.assertFalse(bridge.app.sent[-1]["params"]["ephemeral"])
        self.assertEqual(bridge.app.sent[-1]["params"]["historyMode"], "paginated")
        await bridge.upstream({"id": 201, "result": result})
        checkpoint = bridge.read_checkpoint()
        self.assertEqual(checkpoint["thread_id"], self.THREAD)
        self.assertEqual(checkpoint["generation"], bridge.args.generation)
        self.assertIsNone(checkpoint["account_sha256"])
        self.assertFalse(checkpoint["shell_history_dirty"])
        self.assertFalse(checkpoint["history_ready"])
        args = types.SimpleNamespace(**vars(bridge.args))
        args.resume_thread = self.THREAD
        with self.assertRaisesRegex(ValueError, "history was not confirmed persisted"):
            bridge_module.Bridge(args)
        self.assertEqual(bridge.checkpoint_path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(bridge.client.sent[-1]["result"], result)
        with self.assertRaises(ValueError):
            bridge.policy("thread/start", {})

    async def test_resume_lookup_has_no_execution_authority_until_exact_success(self):
        bridge = self.resumed_bridge()
        original = bridge.checkpoint_path.read_bytes()
        self.assertFalse(bridge.thread_ids)
        self.assertEqual(bridge.policy("thread/read", {"threadId": self.THREAD, "includeTurns": False}),
                         {"threadId": self.THREAD, "includeTurns": False})
        self.assertEqual(bridge.policy("thread/read", {"threadId": self.THREAD}),
                         {"threadId": self.THREAD, "includeTurns": False})
        for method, params in [
            ("thread/read", {"threadId": self.THREAD, "includeTurns": True}),
            ("thread/read", {"threadId": "unrelated", "includeTurns": False}),
            ("thread/turns/list", {"threadId": self.THREAD}),
            ("turn/start", {"threadId": self.THREAD, "input": [{"type": "text", "text": "hello"}]}),
            ("thread/start", {}),
        ]:
            with self.subTest(method=method, params=params), self.assertRaises(ValueError):
                bridge.policy(method, params)
        await bridge.handle({"id": 202, "method": "thread/resume", "params": {"threadId": self.THREAD}})
        self.assertFalse(bridge.thread_ids)
        self.assertEqual(bridge.checkpoint_path.read_bytes(), original)
        with self.assertRaises(ValueError):
            bridge.policy("thread/resume", {"threadId": self.THREAD})
        await bridge.upstream({"id": 202, "result": {"thread": {"id": self.THREAD}}})
        self.assertEqual(bridge.thread_ids, {self.THREAD})
        self.assertEqual(bridge.read_checkpoint()["generation"], self.NEXT_GENERATION)
        self.assertTrue(bridge.read_checkpoint()["history_ready"])
        turn = bridge.policy("turn/start", {
            "threadId": self.THREAD, "input": [{"type": "text", "text": "remember"}],
            "environments": [{"environmentId": "local"}], "cwd": "/",
        })
        self.assertEqual(turn["environments"], bridge.environments())
        self.assertEqual(turn["cwd"], str(self.work))

    async def test_resume_drops_client_configuration_and_rejects_history_path_or_foreign_thread(self):
        bridge = self.resumed_bridge()
        params = {
            "threadId": self.THREAD, "cwd": "/", "runtimeWorkspaceRoots": ["/"],
            "config": {"mcp_servers": {"unsafe": {}}}, "permissions": "unsafe",
            "sandbox": "workspace-write", "approvalPolicy": "on-request",
            "modelProvider": "evil", "baseInstructions": "host execution",
            "developerInstructions": "host execution", "environments": [{"environmentId": "local"}],
            "excludeTurns": True, "initialTurnsPage": {"limit": 15},
        }
        sanitized = bridge.policy("thread/resume", params)
        self.assertEqual(sanitized, {
            "threadId": self.THREAD, "cwd": str(self.work),
            "runtimeWorkspaceRoots": [str(self.work)], "approvalPolicy": "never",
            "sandbox": "danger-full-access", "excludeTurns": True,
            "initialTurnsPage": {"limit": 15},
        })
        for override in [{"history": []}, {"path": "/private/rollout.jsonl"},
                         {"threadId": "01998765-4321-7000-8000-000000000099"}]:
            with self.subTest(override=override), self.assertRaises(ValueError):
                bridge.policy("thread/resume", {**params, **override})
        for method in ("thread/inject_items", "thread/fork", "thread/list"):
            with self.assertRaises(ValueError):
                bridge.policy(method, {"threadId": self.THREAD})

    async def test_failed_or_mismatched_resume_does_not_advance_checkpoint(self):
        bridge = self.resumed_bridge()
        original = bridge.checkpoint_path.read_bytes()
        await bridge.handle({"id": 203, "method": "thread/resume", "params": {"threadId": self.THREAD}})
        await bridge.upstream({"id": 203, "error": {"code": -1, "message": "fixture failure"}})
        self.assertFalse(bridge.thread_ids)
        self.assertEqual(bridge.checkpoint_path.read_bytes(), original)
        await bridge.handle({"id": 204, "method": "thread/resume", "params": {"threadId": self.THREAD}})
        with self.assertRaises(ValueError):
            await bridge.upstream({"id": 204, "result": {
                "thread": {"id": "01998765-4321-7000-8000-000000000099"},
            }})
        self.assertFalse(bridge.thread_ids)
        self.assertEqual(bridge.checkpoint_path.read_bytes(), original)

    async def test_checkpoint_rejects_dirty_wrong_profile_and_noncanonical_ids(self):
        args = self.prepare_resume()
        checkpoint = self.bridge.read_checkpoint()
        for override in [
            {"schema": 2}, {"work": "/"}, {"codex_home": "/"},
            {"cli_version": "0.0.0"}, {"thread_id": "unknown"},
            {"generation": "unknown"}, {"account_sha256": "bad"},
            {"shell_history_dirty": True}, {"shell_history_dirty": None},
            {"history_ready": False}, {"history_ready": None},
        ]:
            with self.subTest(override=override):
                self.bridge.checkpoint_path.write_text(json.dumps({**checkpoint, **override}))
                with self.assertRaises((ValueError, OSError)):
                    bridge_module.Bridge(args)

    async def test_model_history_requires_persist_ack_and_preserves_pending_shell(self):
        bridge = self.bridge
        bridge.persist_checkpoint(self.THREAD)
        read_started, read_release = asyncio.Event(), asyncio.Event()

        async def persisted_read(method, params, timeout=10):
            self.assertEqual((method, params),
                             ("thread/read", {"threadId": self.THREAD, "includeTurns": True}))
            read_started.set()
            await read_release.wait()
            return {"thread": {"id": self.THREAD, "historyMode": "paginated"}}

        bridge.app.call = persisted_read
        await bridge.handle({"id": 251, "method": "turn/start", "params": {
            "threadId": self.THREAD, "input": [{"type": "text", "text": "offline fixture"}]}})
        await bridge.upstream({"id": 251, "result": {"turn": {"id": "model-turn"}}})
        await bridge.upstream({"method": "turn/started", "params": {
            "threadId": self.THREAD, "turn": {"id": "model-turn"}}})
        self.assertFalse(bridge.read_checkpoint()["history_ready"])
        await asyncio.wait_for(bridge.upstream({"method": "turn/completed", "params": {
            "threadId": self.THREAD, "turn": {"id": "model-turn"}}}), 1)
        await asyncio.wait_for(read_started.wait(), 1)
        self.assertFalse(bridge.read_checkpoint()["history_ready"])
        bridge.persist_checkpoint(self.THREAD, dirty=True)
        read_release.set()
        await asyncio.gather(*list(bridge.tasks))
        checkpoint = bridge.read_checkpoint()
        self.assertTrue(checkpoint["history_ready"])
        self.assertTrue(checkpoint["shell_history_dirty"])

    async def test_failed_or_foreign_history_ack_cannot_enable_recovery(self):
        bridge = self.bridge
        bridge.persist_checkpoint(self.THREAD)
        for result in (RuntimeError("offline persistence failure"), {},
                       {"thread": {"id": "unrelated", "historyMode": "paginated"}},
                       {"thread": {"id": self.THREAD, "historyMode": "legacy"}}):
            async def failed_read(method, params, timeout=10):
                if isinstance(result, Exception):
                    raise result
                return result
            bridge.app.call = failed_read
            await bridge.confirm_history(self.THREAD)
            self.assertFalse(bridge.read_checkpoint()["history_ready"])

    async def test_first_shell_only_enables_recovery_after_injection_ack(self):
        bridge = self.bridge
        bridge.executor = FixedExecutor()
        bridge.app.inject_release.clear()
        state = await PolicySafety.start_shell(self)
        await asyncio.wait_for(bridge.app.inject_started.wait(), 1)
        self.assertFalse(bridge.read_checkpoint()["history_ready"])
        bridge.app.inject_release.set()
        await asyncio.wait_for(state["done"].wait(), 1)
        self.assertTrue(bridge.read_checkpoint()["history_ready"])
        self.assertFalse(bridge.read_checkpoint()["shell_history_dirty"])

    async def test_checkpoint_rejects_links_modes_and_oversize(self):
        args = self.prepare_resume()
        path = self.bridge.checkpoint_path
        content = path.read_bytes()
        path.chmod(0o644)
        with self.assertRaises(ValueError):
            bridge_module.Bridge(args)
        path.chmod(0o600)
        link = path.with_name("checkpoint-hardlink")
        os.link(path, link)
        with self.assertRaises(ValueError):
            bridge_module.Bridge(args)
        path.unlink()
        path.symlink_to(link)
        with self.assertRaises(OSError):
            bridge_module.Bridge(args)
        path.unlink()
        path.write_bytes(content + b" " * 8192)
        path.chmod(0o600)
        with self.assertRaises(ValueError):
            bridge_module.Bridge(args)

    async def test_checkpoint_write_failure_never_reports_success_or_admits_thread(self):
        bridge = self.bridge
        bridge.thread_ids.clear()
        await bridge.handle({"id": 205, "method": "thread/start", "params": {}})
        with mock.patch.object(bridge, "persist_checkpoint", side_effect=OSError("fixed write failure")):
            with self.assertRaises(OSError):
                await bridge.upstream({"id": 205, "result": {"thread": {"id": self.THREAD}}})
        self.assertTrue(bridge.stop.is_set())
        self.assertFalse(bridge.thread_ids)
        self.assertFalse(any(message.get("id") == 205 for message in bridge.client.sent))

    async def test_checkpoint_writer_refuses_to_replace_unsafe_existing_file(self):
        self.prepare_resume()
        bridge, path = self.bridge, self.bridge.checkpoint_path
        original = path.read_bytes()
        path.chmod(0o644)
        with self.assertRaises(ValueError):
            bridge.persist_checkpoint(self.THREAD)
        self.assertEqual(path.read_bytes(), original)
        path.chmod(0o600)
        link = path.with_name("writer-hardlink")
        os.link(path, link)
        with self.assertRaises(ValueError):
            bridge.persist_checkpoint(self.THREAD)
        path.unlink()
        path.symlink_to(link)
        with self.assertRaises(ValueError):
            bridge.persist_checkpoint(self.THREAD)
        self.assertTrue(path.is_symlink())
        self.assertEqual(link.read_bytes(), original)

    async def test_stopping_during_shell_keeps_dirty_checkpoint_and_never_replays(self):
        bridge = self.bridge
        bridge.executor = FixedExecutor()
        bridge.app.inject_release.clear()
        state = await PolicySafety.start_shell(self)
        await asyncio.wait_for(bridge.app.inject_started.wait(), 1)
        bridge.stopping = True
        for task in list(bridge.tasks):
            task.cancel()
        await asyncio.gather(*bridge.tasks, return_exceptions=True)
        self.assertTrue(bridge.read_checkpoint()["shell_history_dirty"])
        args = types.SimpleNamespace(**vars(bridge.args))
        args.resume_thread = self.THREAD
        with self.assertRaisesRegex(ValueError, "not confirmed persisted"):
            bridge_module.Bridge(args)
        self.assertEqual(sum(method == "process/start" for method, _ in bridge.executor.calls), 1)


class AuthSafety(unittest.IsolatedAsyncioTestCase):
    TOKEN = "offline.fake-access-token.fixture"
    REFRESH = "offline-refresh-token-never-copy"
    ID_TOKEN = "offline-id-token-never-copy"

    async def asyncSetUp(self):
        await PolicySafety.asyncSetUp(self)
        self.auth_source = Path(self.directory.name) / "login" / "auth.json"
        self.auth_source.parent.mkdir(mode=0o700)
        self.write_auth()

    async def asyncTearDown(self):
        await PolicySafety.asyncTearDown(self)

    def write_auth(self, token=None, account="account-a", path=None):
        path = path or self.auth_source
        path.write_text(json.dumps({
            "auth_mode": "chatgpt", "OPENAI_API_KEY": None,
            "tokens": {"access_token": token or self.TOKEN, "account_id": account,
                       "refresh_token": self.REFRESH, "id_token": self.ID_TOKEN},
            "last_refresh": "2026-10-20T00:00:00Z",
        }))
        path.chmod(0o600)
        return path

    def authenticated_bridge(self, path=None):
        previous = self.bridge
        args = types.SimpleNamespace(**vars(previous.args))
        args.chatgpt_auth_source = str(path or self.auth_source)
        self.bridge = bridge_module.Bridge(args)
        self.bridge.app = AuthPeer()
        self.bridge.client = RecordingPeer()
        self.bridge.thread_ids.add("01998765-4321-7000-8000-000000000001")
        return self.bridge

    def public_evidence(self):
        evidence = json.dumps(self.bridge.client.sent)
        diagnostic = Path(self.bridge.args.config).with_name("terminal-bridge-diagnostics.json")
        if diagnostic.exists():
            evidence += diagnostic.read_text()
        evidence += str(self.bridge.failure or "")
        return evidence

    async def test_auth_reader_returns_only_access_credentials_and_never_modifies_source(self):
        bridge = self.authenticated_bridge()
        before = self.auth_source.read_bytes()
        before_meta = self.auth_source.stat()
        result = bridge.read_chatgpt_auth()
        self.assertEqual(result, {
            "accessToken": self.TOKEN, "chatgptAccountId": "account-a",
            "chatgptPlanType": None,
        })
        self.assertNotIn(self.REFRESH, json.dumps(result))
        self.assertNotIn(self.ID_TOKEN, json.dumps(result))
        self.assertEqual(self.auth_source.read_bytes(), before)
        after_meta = self.auth_source.stat()
        self.assertEqual(
            (before_meta.st_ino, before_meta.st_mtime_ns, before_meta.st_size),
            (after_meta.st_ino, after_meta.st_mtime_ns, after_meta.st_size),
        )
        self.assertNotIn(self.TOKEN, self.public_evidence())

    async def test_auth_reader_rejects_links_and_non_private_permissions(self):
        root = Path(self.directory.name)
        symlink = root / "login-link.json"
        symlink.symlink_to(self.auth_source)
        hardlink = root / "login-hardlink.json"
        os.link(self.auth_source, hardlink)
        for path in [symlink, hardlink, self.auth_source]:
            with self.subTest(path=path.name):
                with self.assertRaises(bridge_module.AuthSourceError):
                    self.authenticated_bridge(path).read_chatgpt_auth()
        hardlink.unlink()
        self.auth_source.chmod(0o644)
        with self.assertRaises(bridge_module.AuthSourceError):
            self.authenticated_bridge().read_chatgpt_auth()

    async def test_auth_reader_rejects_oversize_malformed_and_wrong_auth_modes_without_token_echo(self):
        invalid = [
            b"x" * (64 * 1024 + 1),
            b'{"tokens":{"access_token":"' + self.TOKEN.encode() + b'"',
            json.dumps({"auth_mode": "apikey", "OPENAI_API_KEY": self.TOKEN}).encode(),
            json.dumps({"auth_mode": "chatgpt", "tokens": {
                "access_token": self.TOKEN, "account_id": "",
            }}).encode(),
            json.dumps({"auth_mode": "chatgpt", "tokens": {
                "access_token": "", "account_id": "account-a",
            }}).encode(),
        ]
        for number, data in enumerate(invalid):
            with self.subTest(case=number):
                self.auth_source.write_bytes(data)
                self.auth_source.chmod(0o600)
                with self.assertRaises(bridge_module.AuthSourceError) as error:
                    self.authenticated_bridge().read_chatgpt_auth()
                self.assertNotIn(self.TOKEN, str(error.exception))
                self.assertNotIn(self.REFRESH, str(error.exception))

    async def test_initialize_authentication_completes_before_other_rpc_is_forwarded(self):
        bridge = self.authenticated_bridge()
        bridge.app.login_release.clear()
        initialize = asyncio.create_task(bridge.dispatch({
            "id": 100, "method": "initialize", "params": {"clientInfo": {"name": "untrusted"}},
        }))
        bridge.client_tasks.add(initialize)
        await asyncio.wait_for(bridge.app.login_started.wait(), 1)
        read = asyncio.create_task(bridge.dispatch({
            "id": 101, "method": "account/read", "params": {"refreshToken": True},
        }))
        bridge.client_tasks.add(read)
        await asyncio.sleep(.01)
        self.assertEqual([method for method, _ in bridge.app.calls],
                         ["initialize", "account/login/start"])
        self.assertFalse(bridge.app.sent, "RPC reached app-server before internal login completed")
        self.assertFalse(any(message.get("id") == 100 for message in bridge.client.sent),
                         "native TUI saw initialization success before authentication")
        bridge.app.login_release.set()
        await asyncio.wait_for(asyncio.gather(initialize, read), 1)
        self.assertEqual(bridge.app.sent[-1]["method"], "account/read")
        login = bridge.app.calls[1][1]
        self.assertEqual(login["type"], "chatgptAuthTokens")
        self.assertEqual(login["accessToken"], self.TOKEN)
        self.assertNotIn(self.REFRESH, json.dumps(login))
        self.assertNotIn(self.ID_TOKEN, json.dumps(login))
        self.assertTrue(bridge.external_chatgpt_auth)
        self.assertNotIn(self.TOKEN, self.public_evidence())

    async def test_login_failure_is_redacted_and_does_not_release_queued_rpc(self):
        bridge = self.authenticated_bridge()
        bridge.app.login_release.clear()
        bridge.app.login_error = f"upstream rejected secret {self.TOKEN} {self.REFRESH}"
        initialize = asyncio.create_task(bridge.dispatch({
            "id": 110, "method": "initialize", "params": {},
        }))
        bridge.client_tasks.add(initialize)
        await asyncio.wait_for(bridge.app.login_started.wait(), 1)
        queued = asyncio.create_task(bridge.dispatch({
            "id": 111, "method": "thread/start", "params": {},
        }))
        bridge.client_tasks.add(queued)
        await asyncio.sleep(.01)
        bridge.app.login_release.set()
        await asyncio.wait_for(asyncio.gather(initialize, queued), 1)
        self.assertFalse(bridge.app.sent)
        self.assertFalse(bridge.external_chatgpt_auth)
        self.assertFalse(any(
            message.get("id") == 110 and "result" in message
            for message in bridge.client.sent
        ))
        self.assertNotIn(self.TOKEN, self.public_evidence())
        self.assertNotIn(self.REFRESH, self.public_evidence())

    async def initialize_auth(self):
        bridge = self.authenticated_bridge()
        await bridge.dispatch({"id": 120, "method": "initialize", "params": {}})
        self.assertTrue(bridge.external_chatgpt_auth)
        return bridge

    async def test_native_client_still_cannot_replace_or_clear_controller_auth(self):
        bridge = await self.initialize_auth()
        before = len(bridge.app.calls), len(bridge.app.sent)
        for number, method in enumerate(["account/login/start", "account/logout"]):
            await bridge.dispatch({
                "id": 121 + number, "method": method,
                "params": {"type": "chatgptAuthTokens", "accessToken": "client-controlled"},
            })
        self.assertEqual((len(bridge.app.calls), len(bridge.app.sent)), before)
        self.assertTrue(bridge.external_chatgpt_auth)

    async def test_auth_refresh_rereads_same_account_without_forwarding_secrets_or_writing_source(self):
        bridge = await self.initialize_auth()
        replacement = "offline-new-access-token.fixture"
        self.write_auth(token=replacement)
        before = self.auth_source.read_bytes(), self.auth_source.stat().st_mtime_ns
        client_count = len(bridge.client.sent)
        await bridge.upstream({
            "id": "auth-refresh-1", "method": "account/chatgptAuthTokens/refresh",
            "params": {"reason": "unauthorized", "previousAccountId": "account-a"},
        })
        response = bridge.app.sent[-1]
        self.assertEqual(response["id"], "auth-refresh-1")
        self.assertEqual(response["result"], {
            "accessToken": replacement, "chatgptAccountId": "account-a", "chatgptPlanType": None,
        })
        self.assertEqual(len(bridge.client.sent), client_count)
        self.assertEqual(
            (self.auth_source.read_bytes(), self.auth_source.stat().st_mtime_ns), before,
        )
        self.assertNotIn(replacement, self.public_evidence())
        self.assertNotIn(self.REFRESH, json.dumps(response))

    async def test_auth_refresh_rejects_account_change_without_disclosing_credentials(self):
        bridge = await self.initialize_auth()
        self.write_auth(token="different-account-access", account="account-b")
        before = self.auth_source.read_bytes()
        await bridge.upstream({
            "id": "auth-refresh-2", "method": "account/chatgptAuthTokens/refresh",
            "params": {"reason": "unauthorized", "previousAccountId": "account-a"},
        })
        response = bridge.app.sent[-1]
        self.assertIn("error", response)
        self.assertNotIn("different-account-access", json.dumps(response))
        self.assertNotIn(self.REFRESH, json.dumps(response))
        self.assertEqual(self.auth_source.read_bytes(), before)
        self.assertNotIn("different-account-access", self.public_evidence())

    async def test_auth_refresh_rejects_different_previous_account(self):
        bridge = await self.initialize_auth()
        await bridge.upstream({
            "id": "auth-refresh-3", "method": "account/chatgptAuthTokens/refresh",
            "params": {"reason": "unauthorized", "previousAccountId": "account-b"},
        })
        self.assertIn("error", bridge.app.sent[-1])
        self.assertNotIn(self.TOKEN, json.dumps(bridge.app.sent[-1]))

    async def test_no_auth_source_preserves_offline_initialization_without_login(self):
        bridge = self.bridge
        bridge.initialized = False
        bridge.app = AuthPeer()
        await bridge.dispatch({"id": 130, "method": "initialize", "params": {}})
        methods = [method for method, _ in bridge.app.calls]
        methods.extend(message.get("method") for message in bridge.app.sent)
        self.assertIn("initialize", methods)
        self.assertNotIn("account/login/start", methods)
        self.assertFalse(bridge.external_chatgpt_auth)

    async def test_resume_account_binding_is_checked_before_internal_login(self):
        bridge = await self.initialize_auth()
        thread = "01998765-4321-7000-8000-000000000001"
        bridge.persist_checkpoint(thread, history_ready=True)
        original = bridge.checkpoint_path.read_bytes()
        self.write_auth(account="different-account")
        args = types.SimpleNamespace(**vars(bridge.args))
        args.resume_thread = thread
        args.generation = "01998765-4321-7000-8000-000000000003"
        bridge = bridge_module.Bridge(args)
        self.bridge = bridge
        bridge.app, bridge.client = AuthPeer(), RecordingPeer()
        await bridge.dispatch({"id": 240, "method": "initialize", "params": {}})
        self.assertEqual([method for method, _ in bridge.app.calls], ["initialize"])
        self.assertTrue(bridge.auth_failed)
        self.assertEqual(bridge.checkpoint_path.read_bytes(), original)
        self.assertNotIn("different-account", self.public_evidence())
        self.assertNotIn(self.TOKEN, self.public_evidence())

    async def test_resume_same_account_preserves_login_and_new_token_without_source_write(self):
        bridge = await self.initialize_auth()
        thread = "01998765-4321-7000-8000-000000000001"
        bridge.persist_checkpoint(thread, history_ready=True)
        self.write_auth(token="offline-rotated-access")
        source = self.auth_source.read_bytes()
        args = types.SimpleNamespace(**vars(bridge.args))
        args.resume_thread = thread
        args.generation = "01998765-4321-7000-8000-000000000003"
        bridge = bridge_module.Bridge(args)
        self.bridge = bridge
        bridge.app, bridge.client = AuthPeer(), RecordingPeer()
        await bridge.dispatch({"id": 241, "method": "initialize", "params": {}})
        self.assertTrue(bridge.external_chatgpt_auth)
        self.assertEqual(bridge.app.calls[1][1]["accessToken"], "offline-rotated-access")
        self.assertEqual(self.auth_source.read_bytes(), source)
        self.assertNotIn("offline-rotated-access", self.public_evidence())


if __name__ == "__main__":
    unittest.main(verbosity=2)
