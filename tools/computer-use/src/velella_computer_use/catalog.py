"""生成鱼群可用的 MCP 目录条目（只读观察 / 受控输入）。

目录清单遵循现有 fish 管理的来源格式：version=1、entries[]，
每条只含 id/command/args/catalog，不含凭据、环境变量或账户字段。
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Iterable, Optional

OBSERVE_ENTRY_ID = "velella-computer-use-observe"
CONTROL_ENTRY_ID = "velella-computer-use-control"

OBSERVE_TOOLS = ["computer_screenshot"]
CONTROL_TOOLS = [
    "computer_screenshot",
    "computer_acquire_control",
    "computer_release_control",
    "computer_stop_control",
    "computer_resume_control",
    "computer_control_status",
    "computer_move_mouse",
    "computer_click",
    "computer_drag",
    "computer_scroll",
    "computer_type_text",
    "computer_press_keys",
]

MAX_ENTRIES = 64
MAX_ARGS = 64
MAX_ARG_LEN = 4096
MAX_TOOL_LEN = 128


class CatalogError(ValueError):
    """目录清单不符合现有 fish 管理接受的格式。"""


def build_manifest(python_executable: Optional[Path] = None, *, server_module: str = "velella_computer_use.server") -> dict:
    """生成目录清单。command 必须是绝对路径的本地解释器。"""
    interpreter = Path(python_executable) if python_executable else Path(sys.executable)
    interpreter = interpreter.resolve()
    if not interpreter.is_absolute():
        raise CatalogError("解释器必须是绝对路径：%s" % interpreter)
    command = str(interpreter)
    args = ["-m", server_module]
    return {
        "version": 1,
        "entries": [
            {"id": OBSERVE_ENTRY_ID, "command": command, "args": list(args), "catalog": list(OBSERVE_TOOLS)},
            {"id": CONTROL_ENTRY_ID, "command": command, "args": list(args) + ["--allow-input"], "catalog": list(CONTROL_TOOLS)},
        ],
    }


def validate_manifest(manifest: dict) -> None:
    """按 Rust 侧 mcp_catalog::preview 的规则做同构校验，尽早给出明确错误。"""
    if not isinstance(manifest, dict):
        raise CatalogError("目录清单必须是对象")
    if set(manifest) - {"version", "entries"}:
        raise CatalogError("目录清单不允许额外字段")
    if manifest.get("version") != 1:
        raise CatalogError("目录清单 version 必须为 1")
    entries = manifest.get("entries")
    if not isinstance(entries, list) or not entries:
        raise CatalogError("目录清单需要非空 entries")
    if len(entries) > MAX_ENTRIES:
        raise CatalogError("目录清单最多 %d 个条目" % MAX_ENTRIES)
    seen = set()
    for entry in entries:
        if not isinstance(entry, dict):
            raise CatalogError("条目必须是对象")
        if set(entry) - {"id", "command", "args", "catalog"}:
            raise CatalogError("条目不允许账户或环境字段：%s" % sorted(set(entry) - {"id", "command", "args", "catalog"}))
        identifier = entry.get("id")
        if not isinstance(identifier, str) or not identifier:
            raise CatalogError("条目 id 无效")
        if len(identifier) > 80 or identifier.startswith("__") or identifier in {"velella", "velella_browser"} or not all(c.isascii() and (c.isalnum() or c in "_-") for c in identifier):
            raise CatalogError("条目 id 不符合鱼群规则")
        if identifier in seen:
            raise CatalogError("条目标识重复：%s" % identifier)
        seen.add(identifier)
        command = entry.get("command")
        if not isinstance(command, str) or not command.strip() or len(command) > MAX_ARG_LEN:
            raise CatalogError("条目 command 无效：%s" % identifier)
        if not Path(command).is_absolute():
            raise CatalogError("条目 command 必须是绝对路径：%s" % identifier)
        args = entry.get("args", [])
        if not isinstance(args, list) or len(args) > MAX_ARGS:
            raise CatalogError("条目 args 无效：%s" % identifier)
        for value in args:
            if not isinstance(value, str) or len(value) > MAX_ARG_LEN:
                raise CatalogError("条目 args 元素无效：%s" % identifier)
        catalog = entry.get("catalog")
        if not isinstance(catalog, list) or not catalog:
            raise CatalogError("条目 catalog 必须是非空工具列表：%s" % identifier)
        if len(catalog) > 128 or len(set(catalog)) != len(catalog):
            raise CatalogError("工具目录过长或重复")
        for tool in catalog:
            if not isinstance(tool, str) or not tool or len(tool) > MAX_TOOL_LEN:
                raise CatalogError("条目 catalog 工具名无效：%s" % identifier)
            if not all(c.isascii() and (c.isalnum() or c in "_-./") for c in tool):
                raise CatalogError("工具名不符合鱼群规则")


def write_manifest(path: Path, python_executable: Optional[Path] = None, *, server_module: str = "velella_computer_use.server") -> Path:
    manifest = build_manifest(python_executable, server_module=server_module)
    validate_manifest(manifest)
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return path.resolve()


def entry(manifest: dict, entry_id: str) -> dict:
    for item in manifest.get("entries", []):
        if item.get("id") == entry_id:
            return item
    raise CatalogError("目录条目不存在：%s" % entry_id)


def tool_names(manifest: dict, entry_id: str) -> Iterable[str]:
    return entry(manifest, entry_id)["catalog"]


def main(argv: Optional[list] = None) -> int:
    import argparse

    parser = argparse.ArgumentParser(description="生成 Velella 鱼群 Computer Use 的 MCP 目录清单")
    parser.add_argument("--output", type=Path, required=True, help="目录清单输出路径（JSON）")
    parser.add_argument("--python", type=Path, default=None, help="服务使用的解释器绝对路径（默认当前解释器）")
    parser.add_argument("--server-module", default="velella_computer_use.server")
    args = parser.parse_args(argv)
    path = write_manifest(args.output, args.python, server_module=args.server_module)
    print(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
