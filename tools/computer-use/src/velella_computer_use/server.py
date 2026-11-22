"""Velella 鱼群 Computer Use：本地 Windows MCP 服务（stdio）。

观察与输入分离：computer_screenshot 不需要控制权；其余工具必须持有有效控制租约。
截图帧受真实中继的 1 MiB 整帧上限约束，超限时缩放/重编码，仍超限则显式失败。
"""

from __future__ import annotations

import asyncio
import base64
import io
import json
import os
from pathlib import Path
from typing import Any, Optional

from mcp import types
from mcp.server import Server
from mcp.server.stdio import stdio_server

from . import catalog as catalog_module
from . import windows
from .control import ControlBusy, ControlLease, ControlStopped, ControlUnavailable

SERVER_NAME = "velella-computer-use"
SERVER_VERSION = "0.1.0"
FRAME_LIMIT = 1024 * 1024
MAX_IMAGE_PIXELS = 64_000_000

_STATE: dict[str, Any] = {}


def _lease() -> ControlLease:
    lease = _STATE.get("lease")
    if lease is None:
        lease = ControlLease()
        _STATE["lease"] = lease
    return lease


def _owned_pids() -> list[int]:
    return _STATE.get("owned_pids", [])


def _guard_foreground() -> Optional[int]:
    """前台窗口守卫：声明了拥有窗口时，只有前台属于拥有窗口才允许注入。"""
    owned = _owned_pids()
    if not owned:
        return None
    return windows.require_owned_foreground(owned)


def _text(payload: dict) -> types.TextContent:
    return types.TextContent(type="text", text=json.dumps(payload, ensure_ascii=False, sort_keys=True))


def _image_bytes(image, budget: int) -> tuple[bytes, str, dict]:
    """把截图编码到整帧预算之内。返回 (data, mime, meta)。"""
    if image.width * image.height > MAX_IMAGE_PIXELS:
        raise windows.DesktopUnavailable("截图像素总数过大，拒绝返回超大帧")
    working = image.convert("RGB")
    for level in range(12):
        formats = [("PNG", {})] if level == 0 else []
        formats += [("JPEG", {"quality": quality}) for quality in (85, 65, 40, 25)]
        for format_name, options in formats:
            buffer = io.BytesIO()
            working.save(buffer, format=format_name, optimize=True, **options)
            data = buffer.getvalue()
            encoded = base64.b64encode(data)
            if len(encoded) <= budget:
                mime = "image/png" if format_name == "PNG" else "image/jpeg"
                return encoded, mime, {"bytes": len(data), "encoded_bytes": len(encoded), "mime": mime, "size_px": list(working.size)}
        if min(working.size) <= 160:
            break
        working = image.resize((max(1, int(working.width * 0.75)), max(1, int(working.height * 0.75))), _RESAMPLE).convert("RGB")
    raise windows.DesktopUnavailable(
        "截图在最低质量下仍超过中继整帧上限（%d 字节）；拒绝发送超大帧" % FRAME_LIMIT
    )


try:  # Pillow >= 9.1
    from PIL.Image import Resampling as _Resampling

    _RESAMPLE = _Resampling.LANCZOS
except Exception:  # pragma: no cover
    from PIL import Image as _Image

    _RESAMPLE = _Image.LANCZOS


def _frame_budget(text_payload: dict) -> int:
    """计算除图片之外可用的字节预算（结果块 + JSON-RPC 外壳 + 换行）。"""
    text_block = _text(text_payload)
    result = types.CallToolResult(content=[text_block], isError=False)
    payload = json.dumps(result.model_dump(by_alias=True, exclude_none=True, mode="json"), ensure_ascii=False).encode("utf-8")
    envelope = len('{"jsonrpc":"2.0","id":1,"result":}') + 1
    return FRAME_LIMIT - len(payload) - envelope - 8192


def _screenshot_payload() -> tuple[list[types.ContentBlock], dict]:
    image, geometry = windows.capture_desktop()
    meta = geometry.describe()
    meta.update(windows.platform_info())
    budget = _frame_budget(meta)
    encoded, mime, encoding = _image_bytes(image, budget)
    meta.update(encoding)
    meta["physical_size_px"] = [geometry.width, geometry.height]
    meta["scale_to_physical"] = [geometry.width / encoding["size_px"][0], geometry.height / encoding["size_px"][1]]
    meta["transform"] = "physical = origin_px + floor(image_px * scale_to_physical)"
    meta["frame_limit_bytes"] = FRAME_LIMIT
    _STATE["snapshot"] = (geometry, tuple(encoding["size_px"]))
    blocks = [
        types.ImageContent(type="image", data=encoded.decode("ascii"), mimeType=mime),
        _text(meta),
    ]
    estimated = len(json.dumps(
        types.CallToolResult(content=blocks, isError=False).model_dump(by_alias=True, exclude_none=True, mode="json"),
        ensure_ascii=False,
    ).encode("utf-8"))
    if estimated + 8192 >= FRAME_LIMIT:
        raise windows.DesktopUnavailable("序列化后整帧仍超出 1 MiB 上限，拒绝发送")
    return blocks, meta


def _map_point(x: int, y: int) -> tuple[int, int]:
    snapshot = _STATE.get("snapshot")
    if snapshot is None:
        raise ValueError("请先获取截图，再使用该截图的图像像素坐标")
    geometry, size = snapshot
    if windows.virtual_desktop() != geometry:
        raise ValueError("显示器布局已变化；请重新获取截图")
    if not 0 <= x < size[0] or not 0 <= y < size[1]:
        raise ValueError("坐标超出最近截图范围；未注入输入")
    point = (x * geometry.width // size[0], y * geometry.height // size[1])
    owned = _owned_pids()
    if owned:
        windows.require_owned_point(*geometry.to_physical(*point), owned)
    return point


async def _tool_screenshot() -> list[types.ContentBlock]:
    blocks, _meta = _screenshot_payload()
    return blocks


async def _tool_acquire_control(role: str = "") -> list[types.ContentBlock]:
    lease = _lease()
    try:
        held = lease.acquire(role)
    except ControlBusy as error:
        raise ValueError(str(error)) from error
    except ControlStopped as error:
        raise ValueError(str(error)) from error
    _STATE["acquired"] = True
    return [_text({"action": "acquire_control", "lease": held.to_dict(), "status": lease.status()})]


async def _tool_release_control() -> list[types.ContentBlock]:
    lease = _lease()
    released = lease.release()
    return [_text({"action": "release_control", "released": released, "status": lease.status()})]


async def _tool_stop_control() -> list[types.ContentBlock]:
    receipt = _lease().stop()
    receipt["status"] = _lease().status()
    return [_text(receipt)]


async def _tool_resume_control() -> list[types.ContentBlock]:
    receipt = _lease().resume()
    receipt["status"] = _lease().status()
    return [_text(receipt)]


async def _tool_control_status() -> list[types.ContentBlock]:
    return [_text(_lease().status())]


def _require_control() -> None:
    lease = _lease()
    try:
        lease.require()
    except (ControlUnavailable, ControlStopped) as error:
        raise ValueError(str(error)) from error


async def _tool_move_mouse(x: int, y: int) -> list[types.ContentBlock]:
    _require_control()
    _guard_foreground()
    receipt = windows.move_mouse(*_map_point(x, y))
    return [_text({"action": "move_mouse", "physical": list(receipt), "image": [int(x), int(y)]})]


async def _tool_click(x: int, y: int, button: str = "left", clicks: int = 1) -> list[types.ContentBlock]:
    _require_control()
    _guard_foreground()
    receipt = windows.click(*_map_point(x, y), button, clicks)
    receipt["image"] = [int(x), int(y)]
    return [_text(receipt)]


async def _tool_drag(start_x: int, start_y: int, end_x: int, end_y: int) -> list[types.ContentBlock]:
    _require_control()
    _guard_foreground()
    start = _map_point(start_x, start_y)
    end = _map_point(end_x, end_y)
    receipt = windows.drag(*start, *end)
    receipt["start_image"] = [int(start_x), int(start_y)]
    receipt["end_image"] = [int(end_x), int(end_y)]
    return [_text(receipt)]


async def _tool_scroll(x: int, y: int, delta: int, horizontal: bool = False) -> list[types.ContentBlock]:
    _require_control()
    _guard_foreground()
    receipt = windows.scroll(*_map_point(x, y), delta, horizontal)
    receipt["image"] = [int(x), int(y)]
    return [_text(receipt)]


async def _tool_type_text(text: str) -> list[types.ContentBlock]:
    _require_control()
    _guard_foreground()
    receipt = windows.type_text(text)
    return [_text(receipt)]


async def _tool_press_keys(keys: str) -> list[types.ContentBlock]:
    _require_control()
    _guard_foreground()
    receipt = windows.press_keys(keys)
    return [_text(receipt)]


TOOLS = [
    types.Tool(
        name="computer_screenshot",
        description="捕获整块虚拟桌面并返回图像，附带物理原点、图像尺寸与坐标换算说明。无需控制权。",
        inputSchema={"type": "object", "properties": {}, "additionalProperties": False},
    ),
    types.Tool(
        name="computer_acquire_control",
        description="获取本机桌面的控制租约。一台桌面同时只允许一个控制进程；被占用时明确报错。",
        inputSchema={
            "type": "object",
            "properties": {"role": {"type": "string", "maxLength": 128, "description": "发起控制的鱼角色名（可选）"}},
            "additionalProperties": False,
        },
    ),
    types.Tool(
        name="computer_release_control",
        description="释放本进程持有的控制租约。",
        inputSchema={"type": "object", "properties": {}, "additionalProperties": False},
    ),
    types.Tool(
        name="computer_stop_control",
        description="本地停止桌面控制：释放租约并写入停止标记，之后必须显式恢复。",
        inputSchema={"type": "object", "properties": {}, "additionalProperties": False},
    ),
    types.Tool(
        name="computer_resume_control",
        description="显式解除本地停止标记；仍需重新获取控制权才能操作桌面。",
        inputSchema={"type": "object", "properties": {}, "additionalProperties": False},
    ),
    types.Tool(
        name="computer_control_status",
        description="查看当前控制租约状态、拥有者与到期时间。",
        inputSchema={"type": "object", "properties": {}, "additionalProperties": False},
    ),
    types.Tool(
        name="computer_move_mouse",
        description="把鼠标移动到截图坐标（图像像素，原点为图像左上角）。",
        inputSchema={
            "type": "object",
            "properties": {"x": {"type": "integer"}, "y": {"type": "integer"}},
            "required": ["x", "y"],
            "additionalProperties": False,
        },
    ),
    types.Tool(
        name="computer_click",
        description="在截图坐标处点击鼠标。",
        inputSchema={
            "type": "object",
            "properties": {
                "x": {"type": "integer"},
                "y": {"type": "integer"},
                "button": {"type": "string", "enum": ["left", "right", "middle"]},
                "clicks": {"type": "integer", "minimum": 1, "maximum": 5},
            },
            "required": ["x", "y"],
            "additionalProperties": False,
        },
    ),
    types.Tool(
        name="computer_drag",
        description="从起点截图坐标拖拽到终点截图坐标。",
        inputSchema={
            "type": "object",
            "properties": {
                "start_x": {"type": "integer"},
                "start_y": {"type": "integer"},
                "end_x": {"type": "integer"},
                "end_y": {"type": "integer"},
            },
            "required": ["start_x", "start_y", "end_x", "end_y"],
            "additionalProperties": False,
        },
    ),
    types.Tool(
        name="computer_scroll",
        description="在截图坐标处滚动滚轮。",
        inputSchema={
            "type": "object",
            "properties": {
                "x": {"type": "integer"},
                "y": {"type": "integer"},
                "delta": {"type": "integer"},
                "horizontal": {"type": "boolean"},
            },
            "required": ["x", "y", "delta"],
            "additionalProperties": False,
        },
    ),
    types.Tool(
        name="computer_type_text",
        description="以 Unicode 键盘事件输入文本（不写入剪贴板）。",
        inputSchema={
            "type": "object",
            "properties": {"text": {"type": "string", "minLength": 1, "maxLength": 4096}},
            "required": ["text"],
            "additionalProperties": False,
        },
    ),
    types.Tool(
        name="computer_press_keys",
        description="按下组合键，例如 ctrl+shift+s。",
        inputSchema={
            "type": "object",
            "properties": {"keys": {"type": "string", "minLength": 1, "maxLength": 64}},
            "required": ["keys"],
            "additionalProperties": False,
        },
    ),
]


async def _list_tools() -> list[types.Tool]:
    return TOOLS


async def _dispatch_tool(name: str, arguments: dict) -> list[types.ContentBlock]:
    arguments = arguments or {}
    if name == "computer_screenshot":
        return await _tool_screenshot()
    if name == "computer_acquire_control":
        return await _tool_acquire_control(str(arguments.get("role", "")))
    if name == "computer_release_control":
        return await _tool_release_control()
    if name == "computer_stop_control":
        return await _tool_stop_control()
    if name == "computer_resume_control":
        return await _tool_resume_control()
    if name == "computer_control_status":
        return await _tool_control_status()
    if name == "computer_move_mouse":
        return await _tool_move_mouse(int(arguments["x"]), int(arguments["y"]))
    if name == "computer_click":
        return await _tool_click(
            int(arguments["x"]),
            int(arguments["y"]),
            str(arguments.get("button", "left")),
            int(arguments.get("clicks", 1)),
        )
    if name == "computer_drag":
        return await _tool_drag(
            int(arguments["start_x"]), int(arguments["start_y"]), int(arguments["end_x"]), int(arguments["end_y"])
        )
    if name == "computer_scroll":
        return await _tool_scroll(int(arguments["x"]), int(arguments["y"]), int(arguments["delta"]), bool(arguments.get("horizontal", False)))
    if name == "computer_type_text":
        return await _tool_type_text(str(arguments["text"]))
    if name == "computer_press_keys":
        return await _tool_press_keys(str(arguments["keys"]))
    raise ValueError("未知工具：%s" % name)


INPUT_TOOLS = {"computer_move_mouse", "computer_click", "computer_drag", "computer_scroll", "computer_type_text", "computer_press_keys"}
for tool in TOOLS:
    if tool.name in INPUT_TOOLS:
        tool.inputSchema["properties"]["lease_token"] = {"type": "string", "minLength": 32, "maxLength": 32, "description": "最近 acquire_control 返回的 lease.token"}
        tool.inputSchema.setdefault("required", []).append("lease_token")


def enabled_tools() -> list[types.Tool]:
    return TOOLS if _STATE.get("allow_input", False) else [tool for tool in TOOLS if tool.name in catalog_module.OBSERVE_TOOLS]


async def _call_tool(name: str, arguments: dict) -> list[types.ContentBlock]:
    from jsonschema import validate

    tool = next((tool for tool in enabled_tools() if tool.name == name), None)
    if tool is None:
        raise ValueError("工具未启用；默认仅观察，输入需要显式 --allow-input")
    arguments = dict(arguments or {})
    validate(arguments, tool.inputSchema)
    if name in INPUT_TOOLS:
        token = arguments.pop("lease_token")
        with _lease().action(token):
            return await _dispatch_tool(name, arguments)
    return await _dispatch_tool(name, arguments)


def build_server() -> Server:
    server = Server(SERVER_NAME)

    @server.list_tools()
    async def list_tools() -> list[types.Tool]:
        return enabled_tools()

    @server.call_tool()
    async def call_tool(name: str, arguments: dict) -> list[types.ContentBlock]:
        return await _call_tool(name, arguments)

    return server


async def run() -> None:
    server = build_server()
    try:
        async with stdio_server() as (read_stream, write_stream):
            await server.run(read_stream, write_stream, server.create_initialization_options())
    finally:
        if _STATE.get("acquired"):
            _lease().release()


def main(argv=None) -> int:
    import argparse
    parser = argparse.ArgumentParser(description="鱼群 Windows Computer Use；默认只读观察")
    parser.add_argument("--allow-input", action="store_true")
    parser.add_argument("--state-dir", type=Path)
    parser.add_argument("--lease-ttl", type=float, default=120)
    parser.add_argument("--owned-pid", type=int, action="append", default=[])
    parser.add_argument("--stop", action="store_true", help="本地停止所有鱼的控制，不连接模型")
    parser.add_argument("--resume", action="store_true", help="解除停止；旧凭据仍失效，必须重新获取")
    args = parser.parse_args(argv)
    _STATE.clear()
    _STATE.update(allow_input=args.allow_input, owned_pids=args.owned_pid, lease=ControlLease(state_dir=args.state_dir, ttl=args.lease_ttl))
    if args.stop or args.resume:
        if args.stop and args.resume:
            parser.error("停止与恢复不能同时指定")
        print(json.dumps(_lease().stop(local=True) if args.stop else _lease().resume(local=True), ensure_ascii=False))
        return 0
    try:
        asyncio.run(run())
    except KeyboardInterrupt:
        return 130
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
