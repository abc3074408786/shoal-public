"""本地 Windows 桌面观察与输入适配层。

只做三件事：捕获虚拟桌面、把图像坐标映射为物理桌面坐标、按需注入鼠标与键盘。
不写剪贴板，不执行 shell，不做任何 macOS/Linux 实现。
"""

from __future__ import annotations

import ctypes
import os
import sys
import time
from dataclasses import dataclass
from typing import Iterable, Sequence

if os.name != "nt":  # pragma: no cover - 非 Windows 主机显式失败
    raise RuntimeError("velella-computer-use 仅支持 Windows 桌面（当前平台不受支持）")

from ctypes import wintypes  # noqa: E402  (必须在 Windows 分支之后导入)

user32 = ctypes.WinDLL("user32", use_last_error=True)

# --- Win32 常量 ---------------------------------------------------------------
SM_CXSCREEN = 0
SM_CYSCREEN = 1
SM_XVIRTUALSCREEN = 76
SM_YVIRTUALSCREEN = 77
SM_CXVIRTUALSCREEN = 78
SM_CYVIRTUALSCREEN = 79

INPUT_MOUSE = 0
INPUT_KEYBOARD = 1

MOUSEEVENTF_MOVE = 0x0001
MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004
MOUSEEVENTF_RIGHTDOWN = 0x0008
MOUSEEVENTF_RIGHTUP = 0x0010
MOUSEEVENTF_MIDDLEDOWN = 0x0020
MOUSEEVENTF_MIDDLEUP = 0x0040
MOUSEEVENTF_WHEEL = 0x0800
MOUSEEVENTF_HWHEEL = 0x1000

KEYEVENTF_EXTENDEDKEY = 0x0001
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_UNICODE = 0x0004

WHEEL_DELTA = 120

VK_CONTROL = 0x11
VK_SHIFT = 0x10
VK_MENU = 0x12
VK_LWIN = 0x5B
VK_RWIN = 0x5C

_EXTENDED_KEYS = {
    0x21,  # VK_PRIOR
    0x22,  # VK_NEXT
    0x23,  # VK_END
    0x24,  # VK_HOME
    0x25,  # VK_LEFT
    0x26,  # VK_UP
    0x27,  # VK_RIGHT
    0x28,  # VK_DOWN
    0x2D,  # VK_INSERT
    0x2E,  # VK_DELETE
    0x90,  # VK_NUMLOCK
    0xA3,  # VK_RCONTROL
    0xA5,  # VK_RMENU
}

_KEY_ALIASES = {
    "ctrl": VK_CONTROL,
    "control": VK_CONTROL,
    "shift": VK_SHIFT,
    "alt": VK_MENU,
    "win": VK_LWIN,
    "meta": VK_LWIN,
    "super": VK_LWIN,
    "cmd": VK_LWIN,
    "enter": 0x0D,
    "return": 0x0D,
    "tab": 0x09,
    "space": 0x20,
    "esc": 0x1B,
    "escape": 0x1B,
    "backspace": 0x08,
    "delete": 0x2E,
    "insert": 0x2D,
    "home": 0x24,
    "end": 0x23,
    "pageup": 0x21,
    "pagedown": 0x22,
    "up": 0x26,
    "down": 0x28,
    "left": 0x25,
    "right": 0x27,
}

_BUTTON_FLAGS = {
    "left": (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
    "right": (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
    "middle": (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
}


class DesktopUnavailable(RuntimeError):
    """桌面不可用（无会话、被锁定、非 Windows 等）。"""


class InvalidInput(ValueError):
    """坐标、按键或文本不合法；调用前即拒绝，绝不注入。"""


class ForegroundNotOwned(RuntimeError):
    """前台窗口不属于本进程拥有的窗口，拒绝注入。"""


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [
        ("dx", wintypes.LONG),
        ("dy", wintypes.LONG),
        ("mouseData", wintypes.DWORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.c_size_t),
    ]


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [
        ("wVk", wintypes.WORD),
        ("wScan", wintypes.WORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.c_size_t),
    ]


class _INPUTUNION(ctypes.Union):
    _fields_ = [("mi", MOUSEINPUT), ("ki", KEYBDINPUT)]


class INPUT(ctypes.Structure):
    _anonymous_ = ("u",)
    _fields_ = [("type", wintypes.DWORD), ("u", _INPUTUNION)]


user32.SendInput.argtypes = (wintypes.UINT, ctypes.POINTER(INPUT), ctypes.c_int)
user32.SendInput.restype = wintypes.UINT
user32.SetCursorPos.argtypes = (ctypes.c_int, ctypes.c_int)
user32.SetCursorPos.restype = wintypes.BOOL
user32.GetSystemMetrics.argtypes = (ctypes.c_int,)
user32.GetSystemMetrics.restype = ctypes.c_int
user32.GetForegroundWindow.restype = wintypes.HWND
user32.GetWindowThreadProcessId.argtypes = (wintypes.HWND, ctypes.POINTER(wintypes.DWORD))
user32.GetWindowThreadProcessId.restype = wintypes.DWORD
user32.WindowFromPoint.argtypes = (wintypes.POINT,)
user32.WindowFromPoint.restype = wintypes.HWND
user32.OpenInputDesktop.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
user32.OpenInputDesktop.restype = wintypes.HANDLE
user32.GetUserObjectInformationW.argtypes = (wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD))
user32.GetUserObjectInformationW.restype = wintypes.BOOL
user32.CloseDesktop.argtypes = (wintypes.HANDLE,)
user32.CloseDesktop.restype = wintypes.BOOL

_DPI_AWARE = False


def enable_dpi_awareness() -> bool:
    """把进程标记为 per-monitor DPI aware，使截图与鼠标坐标都使用物理像素。"""
    global _DPI_AWARE
    if _DPI_AWARE:
        return True
    try:
        user32.SetProcessDpiAwarenessContext.argtypes = (ctypes.c_void_p,)
        user32.SetProcessDpiAwarenessContext.restype = wintypes.BOOL
        if user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4)):
            _DPI_AWARE = True
            return True
    except AttributeError:
        pass
    try:
        user32.SetProcessDPIAware.restype = wintypes.BOOL
        _DPI_AWARE = bool(user32.SetProcessDPIAware())
    except AttributeError:
        _DPI_AWARE = False
    return _DPI_AWARE


@dataclass(frozen=True)
class DesktopGeometry:
    """虚拟桌面几何：图像左上角对应的物理桌面原点与尺寸。"""

    origin_x: int
    origin_y: int
    width: int
    height: int

    @property
    def physical_bounds(self) -> tuple[int, int, int, int]:
        return (self.origin_x, self.origin_y, self.origin_x + self.width, self.origin_y + self.height)

    def to_physical(self, x: int, y: int) -> tuple[int, int]:
        """图像像素坐标 -> 物理桌面坐标（物理坐标可能为负）。"""
        return (self.origin_x + int(x), self.origin_y + int(y))

    def to_image(self, physical_x: int, physical_y: int) -> tuple[int, int]:
        """物理桌面坐标 -> 图像像素坐标。"""
        return (int(physical_x) - self.origin_x, int(physical_y) - self.origin_y)

    def contains(self, x: int, y: int) -> bool:
        return 0 <= int(x) < self.width and 0 <= int(y) < self.height

    def describe(self) -> dict:
        return {
            "origin_px": [self.origin_x, self.origin_y],
            "size_px": [self.width, self.height],
            "coordinate_space": "image pixel",
            "transform": "physical = origin_px + image_px",
            "negative_origin": self.origin_x < 0 or self.origin_y < 0,
        }


def virtual_desktop() -> DesktopGeometry:
    """读取虚拟桌面几何；不可用时显式失败。"""
    require_interactive_desktop()
    if not enable_dpi_awareness():
        raise DesktopUnavailable("无法启用物理像素 DPI 坐标；拒绝使用不确定的坐标")
    left = int(user32.GetSystemMetrics(SM_XVIRTUALSCREEN))
    top = int(user32.GetSystemMetrics(SM_YVIRTUALSCREEN))
    width = int(user32.GetSystemMetrics(SM_CXVIRTUALSCREEN))
    height = int(user32.GetSystemMetrics(SM_CYVIRTUALSCREEN))
    if width <= 0 or height <= 0:
        raise DesktopUnavailable(
            "无法读取虚拟桌面尺寸；桌面会话可能已锁定或不可访问（session=%s）" % _session_name()
        )
    return DesktopGeometry(origin_x=left, origin_y=top, width=width, height=height)


def require_interactive_desktop() -> None:
    """只检查当前输入桌面，不切换桌面，也不绕过锁屏。"""
    handle = user32.OpenInputDesktop(0, False, 0x0001)  # DESKTOP_READOBJECTS
    if not handle:
        raise DesktopUnavailable("无法访问交互桌面；会话可能锁定或断开")
    try:
        name = ctypes.create_unicode_buffer(256)
        needed = wintypes.DWORD()
        if not user32.GetUserObjectInformationW(handle, 2, name, ctypes.sizeof(name), ctypes.byref(needed)):
            raise DesktopUnavailable("无法确认交互桌面状态")
        if name.value.lower() != "default" or not user32.GetForegroundWindow():
            raise DesktopUnavailable("当前不是可操作的普通交互桌面")
    finally:
        user32.CloseDesktop(handle)


def _session_name() -> str:
    try:
        import ctypes as _c

        return str(_c.windll.kernel32.WTSGetActiveConsoleSessionId())
    except Exception:  # pragma: no cover - 仅用于错误信息
        return "unknown"


def capture_desktop(geometry: DesktopGeometry | None = None):
    """捕获整块虚拟桌面，返回 (PIL.Image, DesktopGeometry)。

    捕获尺寸必须与 Win32 虚拟桌面尺寸一致，否则说明 DPI 语义不一致，显式失败而不是给出错位坐标。
    """
    from PIL import ImageGrab

    geometry = geometry or virtual_desktop()
    image = ImageGrab.grab(all_screens=True)
    if image.size != (geometry.width, geometry.height):
        raise DesktopUnavailable(
            "截图尺寸 %s 与虚拟桌面尺寸 %s 不一致；DPI 感知状态异常，拒绝返回错位坐标"
            % (image.size, (geometry.width, geometry.height))
        )
    return image, geometry


def _send(inputs: Sequence[INPUT]) -> None:
    count = len(inputs)
    if count == 0:
        return
    require_interactive_desktop()
    buffer = (INPUT * count)(*inputs)
    sent = user32.SendInput(count, buffer, ctypes.sizeof(INPUT))
    if sent != count:
        native_error = ctypes.get_last_error()
        releases = []
        for event in inputs[:sent]:
            if event.type == INPUT_KEYBOARD and not event.ki.dwFlags & KEYEVENTF_KEYUP:
                releases.append(_keyboard_input(event.ki.wVk, event.ki.dwFlags | KEYEVENTF_KEYUP, event.ki.wScan))
            elif event.type == INPUT_MOUSE:
                for down, up in _BUTTON_FLAGS.values():
                    if event.mi.dwFlags & down:
                        releases.append(_mouse_input(up))
        if releases:
            recovery = (INPUT * len(releases))(*releases)
            user32.SendInput(len(releases), recovery, ctypes.sizeof(INPUT))
        raise DesktopUnavailable("SendInput 只接受了 %d/%d 个事件（GetLastError=%d）" % (sent, count, native_error))


def _mouse_input(flags: int, data: int = 0) -> INPUT:
    event = INPUT(type=INPUT_MOUSE)
    event.mi = MOUSEINPUT(0, 0, data, flags, 0, 0)
    return event


def _keyboard_input(vk: int, flags: int, scan: int = 0) -> INPUT:
    event = INPUT(type=INPUT_KEYBOARD)
    event.ki = KEYBDINPUT(vk, scan, flags, 0, 0)
    return event


def foreground_pid() -> int:
    hwnd = user32.GetForegroundWindow()
    if not hwnd:
        return 0
    pid = wintypes.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    return int(pid.value)


def require_owned_foreground(owned_pids: Iterable[int]) -> int:
    """前台窗口必须属于已拥有的进程，否则拒绝注入。"""
    allowed = {int(pid) for pid in owned_pids}
    if not allowed:
        raise ForegroundNotOwned("未声明任何拥有的窗口；拒绝注入输入")
    pid = foreground_pid()
    if pid not in allowed:
        raise ForegroundNotOwned("前台窗口不属于本进程拥有的窗口（pid=%s）；拒绝注入输入" % pid)
    return pid


def require_owned_point(x: int, y: int, owned_pids: Iterable[int]) -> None:
    hwnd = user32.WindowFromPoint(wintypes.POINT(int(x), int(y)))
    pid = wintypes.DWORD()
    if hwnd:
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    if int(pid.value) not in set(owned_pids):
        raise ForegroundNotOwned("目标坐标不属于测试夹具；拒绝输入")


def move_mouse(x: int, y: int, geometry: DesktopGeometry | None = None) -> tuple[int, int]:
    """把图像坐标移动到物理位置。越界在注入前拒绝。"""
    geometry = geometry or virtual_desktop()
    x = int(x)
    y = int(y)
    if not geometry.contains(x, y):
        raise InvalidInput(
            "坐标 (%s,%s) 超出图像范围 %sx%s（物理范围 %s..%s）"
            % (x, y, geometry.width, geometry.height, geometry.origin_x, geometry.origin_x + geometry.width - 1)
        )
    physical_x, physical_y = geometry.to_physical(x, y)
    if not user32.SetCursorPos(physical_x, physical_y):
        raise DesktopUnavailable("SetCursorPos 失败（GetLastError=%d）" % ctypes.get_last_error())
    return physical_x, physical_y


def click(x: int, y: int, button: str = "left", clicks: int = 1, geometry: DesktopGeometry | None = None) -> dict:
    if button not in _BUTTON_FLAGS:
        raise InvalidInput("未知鼠标按键：%s（支持 left/right/middle）" % button)
    clicks = int(clicks)
    if clicks < 1 or clicks > 5:
        raise InvalidInput("clicks 必须在 1..5 之间")
    physical = move_mouse(x, y, geometry)
    down, up = _BUTTON_FLAGS[button]
    events = []
    for _ in range(clicks):
        events.append(_mouse_input(down))
        events.append(_mouse_input(up))
    _send(events)
    time.sleep(0.01)
    return {"action": "click", "button": button, "clicks": clicks, "physical": list(physical)}


def drag(start_x: int, start_y: int, end_x: int, end_y: int, geometry: DesktopGeometry | None = None) -> dict:
    geometry = geometry or virtual_desktop()
    if not geometry.contains(start_x, start_y) or not geometry.contains(end_x, end_y):
        raise InvalidInput("拖拽起点或终点超出截图范围；未注入输入")
    start = move_mouse(start_x, start_y, geometry)
    time.sleep(0.02)
    _send([_mouse_input(MOUSEEVENTF_LEFTDOWN)])
    end_physical = geometry.to_physical(int(end_x), int(end_y))
    try:
        time.sleep(0.02)
        for index in range(1, 13):
            x = start[0] + (end_physical[0] - start[0]) * index // 12
            y = start[1] + (end_physical[1] - start[1]) * index // 12
            if not user32.SetCursorPos(x, y):
                raise DesktopUnavailable("拖拽过程中鼠标移动失败")
            time.sleep(0.008)
    finally:
        _send([_mouse_input(MOUSEEVENTF_LEFTUP)])
    return {"action": "drag", "start_physical": list(start), "end_physical": list(end_physical)}


def scroll(x: int, y: int, delta: int, horizontal: bool = False, geometry: DesktopGeometry | None = None) -> dict:
    delta = int(delta)
    if delta == 0:
        raise InvalidInput("滚动量不能为 0")
    if not -50 <= delta <= 50:
        raise InvalidInput("滚动量必须在 -50..50 之间")
    physical = move_mouse(x, y, geometry)
    ticks = delta
    events = [_mouse_input(MOUSEEVENTF_HWHEEL if horizontal else MOUSEEVENTF_WHEEL, ticks * WHEEL_DELTA)]
    _send(events)
    time.sleep(0.01)
    return {"action": "scroll", "delta": delta, "horizontal": horizontal, "physical": list(physical)}


def type_text(text: str) -> dict:
    """以 Unicode 键盘事件注入文本；不触碰剪贴板。"""
    if not isinstance(text, str):
        raise InvalidInput("text 必须是字符串")
    if text == "":
        raise InvalidInput("text 不能为空")
    if len(text) > 4096:
        raise InvalidInput("单次输入长度超过 4096 字符上限")
    units = text.encode("utf-16-le")
    events = []
    for index in range(0, len(units), 2):
        code = int.from_bytes(units[index : index + 2], "little")
        events.append(_keyboard_input(0, KEYEVENTF_UNICODE, code))
        events.append(_keyboard_input(0, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, code))
    _send(events)
    return {"action": "type", "characters": len(text), "utf16_units": len(units) // 2}


def _key_code(name: str) -> int:
    key = name.strip().lower()
    if not key:
        raise InvalidInput("按键名不能为空")
    if key in _KEY_ALIASES:
        return _KEY_ALIASES[key]
    if len(key) == 1:
        if key.isdigit() and key.isascii():
            return ord(key)
        if key.isalpha() and key.isascii():
            return ord(key.upper())
        raise InvalidInput("不支持的按键：%s" % name)
    if key.startswith("f") and key[1:].isdigit():
        number = int(key[1:])
        if 1 <= number <= 12:
            return 0x6F + number
    raise InvalidInput("不支持的按键：%s" % name)


def press_keys(keys: str) -> dict:
    """按下组合键，例如 "ctrl+shift+s"。"""
    if not isinstance(keys, str) or not keys.strip():
        raise InvalidInput("keys 必须是非空字符串")
    parts = [part for part in keys.replace(",", "+").split("+") if part.strip()]
    if not parts:
        raise InvalidInput("keys 解析后为空")
    if len(parts) > 6:
        raise InvalidInput("组合键过长（最多 6 段）")
    codes = [_key_code(part) for part in parts]
    events = []
    for code in codes:
        flags = KEYEVENTF_EXTENDEDKEY if code in _EXTENDED_KEYS else 0
        events.append(_keyboard_input(code, flags))
    for code in reversed(codes):
        flags = KEYEVENTF_EXTENDEDKEY if code in _EXTENDED_KEYS else 0
        events.append(_keyboard_input(code, flags | KEYEVENTF_KEYUP))
    _send(events)
    time.sleep(0.01)
    return {"action": "press", "keys": parts, "codes": codes}


def platform_info() -> dict:
    return {
        "platform": sys.platform,
        "dpi_aware": enable_dpi_awareness(),
        "python": sys.version.split()[0],
    }
