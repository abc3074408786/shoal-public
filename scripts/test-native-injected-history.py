#!/usr/bin/env python3
"""Verify pinned native inject_items persistence across app-server restart.

Uses a fresh temporary CODEX_HOME, a deliberately unreachable local fixture
provider, and no turn/start, account, login or model request. It never reads the
user's Codex profile. Pass the fixed native 0.153.4 ELF using --codex.
"""
import argparse
import asyncio
import json
import os
from pathlib import Path
import tempfile
import uuid


class App:
    def __init__(self, executable, root):
        self.executable, self.root = executable, root
        self.sequence = 0
        self.process = None

    async def start(self):
        env = {
            "PATH": "/usr/bin:/bin", "LANG": "C.UTF-8",
            "HOME": str(self.root / "home"), "CODEX_HOME": str(self.root / "codex"),
            "TMPDIR": str(self.root / "tmp"),
        }
        self.process = await asyncio.create_subprocess_exec(
            self.executable, "app-server", "--listen", "stdio://",
            cwd=self.root / "work", env=env,
            stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.DEVNULL, limit=2 * 1024 * 1024,
        )
        await self.call("initialize", {
            "clientInfo": {"name": "velella_offline_history_fixture", "version": "1"},
            "capabilities": {"experimentalApi": True},
        })
        self.process.stdin.write(b'{"method":"initialized"}\n')
        await self.process.stdin.drain()
        return self

    async def call(self, method, params):
        self.sequence += 1
        request_id = self.sequence
        self.process.stdin.write((json.dumps({
            "id": request_id, "method": method, "params": params,
        }) + "\n").encode())
        await self.process.stdin.drain()
        while True:
            line = await asyncio.wait_for(self.process.stdout.readline(), 20)
            if not line:
                raise RuntimeError("native app-server closed before fixture response")
            message = json.loads(line)
            if message.get("id") == request_id:
                if "error" in message:
                    raise RuntimeError(f"{method} failed: {message['error']}")
                return message["result"]

    async def close(self):
        if self.process is not None and self.process.returncode is None:
            self.process.kill()
            await asyncio.wait_for(self.process.wait(), 5)


async def check(executable):
    with tempfile.TemporaryDirectory(prefix="vl-native-history-") as directory:
        root = Path(directory)
        for name in ("home", "codex", "tmp", "work"):
            (root / name).mkdir(mode=0o700)
        (root / "codex/config.toml").write_text(
            'model="offline-fixture"\nmodel_provider="offline_fixture"\n'
            'disable_response_storage=true\n'
            '[features]\nhooks=false\nplugins=false\nshell_snapshot=false\n'
            '[model_providers.offline_fixture]\nname="Offline persistence fixture"\n'
            'base_url="http://127.0.0.1:1/v1"\nwire_api="responses"\n'
            'requires_openai_auth=false\nrequest_max_retries=0\nstream_max_retries=0\n'
        )
        app = App(executable, root)
        marker = "offline-history-" + uuid.uuid4().hex
        record = ("<user_shell_command>\n<command>\nprintf fixture\n</command>\n"
                  "<result>\nExit code: 0\nDuration: 0.0100 seconds\nOutput:\n"
                  + marker + "\n</result>\n</user_shell_command>")
        try:
            await app.start()
            started = await app.call("thread/start", {
                "cwd": str(root / "work"), "runtimeWorkspaceRoots": [str(root / "work")],
                "approvalPolicy": "never", "sandbox": "danger-full-access",
                "ephemeral": False, "historyMode": "paginated",
            })
            thread = started["thread"]["id"]
            await app.call("thread/inject_items", {
                "threadId": thread,
                "items": [{"type": "message", "role": "user",
                           "content": [{"type": "input_text", "text": record}]}],
            })
            rollouts = list((root / "codex/sessions").rglob("*.jsonl"))
            assert len(rollouts) == 1, "first injection did not materialize one rollout"
            first = rollouts[0].read_text()
            assert marker in first, "injection ACK preceded disk history"
            await app.close()
            app = App(executable, root)
            await app.start()
            looked_up = await app.call("thread/read", {"threadId": thread, "includeTurns": False})
            assert looked_up["thread"]["id"] == thread
            resumed = await app.call("thread/resume", {
                "threadId": thread, "cwd": str(root / "work"),
                "runtimeWorkspaceRoots": [str(root / "work")],
                "approvalPolicy": "never", "sandbox": "danger-full-access",
            })
            assert resumed["thread"]["id"] == thread, "resume created a different model thread"
            assert marker in rollouts[0].read_text(), "resume lost the persisted injected item"
            # A second injection tests the reopened writer, without contacting a model.
            await app.call("thread/inject_items", {
                "threadId": thread,
                "items": [{"type": "message", "role": "user",
                           "content": [{"type": "input_text", "text": "after-resume-" + marker}]}],
            })
            resumed_text = rollouts[0].read_text()
            assert "after-resume-" + marker in resumed_text
            original_records = [json.loads(line) for line in resumed_text.splitlines() if marker in line]
            assert len(original_records) == 2, "recovery duplicated or dropped injected history"
            print(json.dumps({
                "passed": True, "same_thread_after_restart": True,
                "first_shell_injection_persisted_before_model_turn": True,
                "reopened_writer_persisted_new_item": True,
                "injected_records": len(original_records), "model_requests": 0,
                "auth_profile_read": False,
            }))
        finally:
            await app.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", required=True)
    arguments = parser.parse_args()
    asyncio.run(check(str(Path(arguments.codex).resolve(strict=True))))
