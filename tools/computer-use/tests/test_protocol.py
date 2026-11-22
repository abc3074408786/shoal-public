"""MCP 协议行为：初始化、工具清单、无租约时拒绝输入。"""

import asyncio
import json
import os
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from mcp import ClientSession, StdioServerParameters  # noqa: E402
from mcp.client.stdio import stdio_client  # noqa: E402

from velella_computer_use import catalog  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]


def _server_parameters(state_dir: Path, allow_input: bool = True) -> StdioServerParameters:
    env = dict(os.environ)
    env["PYTHONPATH"] = str(ROOT / "src") + os.pathsep + env.get("PYTHONPATH", "")
    return StdioServerParameters(
        command=sys.executable,
        args=["-m", "velella_computer_use.server", "--state-dir", str(state_dir)] + (["--allow-input"] if allow_input else []),
        env=env,
    )


class ProtocolTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        import tempfile

        self._temp = tempfile.TemporaryDirectory(prefix="velella-computer-use-proto-")
        self.state = Path(self._temp.name)

    async def asyncTearDown(self):
        self._temp.cleanup()

    async def test_initialize_list_and_refuse_input_without_lease(self):
        async with stdio_client(_server_parameters(self.state)) as (read, write):
            async with ClientSession(read, write) as session:
                init = await session.initialize()
                self.assertTrue(init.serverInfo.name)
                listed = await session.list_tools()
                names = {tool.name for tool in listed.tools}
                self.assertIn("computer_screenshot", names)
                self.assertIn("computer_click", names)
                self.assertTrue(names.issubset(set(catalog.CONTROL_TOOLS)))

                status = await session.call_tool("computer_control_status", {})
                self.assertFalse(status.isError)
                payload = json.loads(status.content[0].text)
                self.assertIn("lease", payload)

                # 没有租约时必须拒绝，且不得注入任何输入。
                refused = await session.call_tool("computer_move_mouse", {"x": 10, "y": 10, "lease_token": "x" * 32})
                self.assertTrue(refused.isError)
                message = json.dumps([block.model_dump() for block in refused.content], ensure_ascii=False)
                self.assertIn("控制", message)

    async def test_default_observation_cannot_acquire_or_inject(self):
        async with stdio_client(_server_parameters(self.state, allow_input=False)) as (read, write):
            async with ClientSession(read, write) as session:
                await session.initialize()
                listed = await session.list_tools()
                self.assertEqual([tool.name for tool in listed.tools], catalog.OBSERVE_TOOLS)
                refused = await session.call_tool("computer_acquire_control", {})
                self.assertTrue(refused.isError)
                self.assertFalse((self.state / "control.lease").exists())


if __name__ == "__main__":
    unittest.main()
