"""跨进程控制租约：同一台 Windows 桌面同时只允许一个控制进程。

租约有界（TTL）、过期可回收、拥有者退出或崩溃后可回收、本地可停止；
不会把别人的租约悄悄转交给自己。这些是协作式流程控制，不是针对同用户原生程序的安全沙箱。
"""

from __future__ import annotations

import json
import os
import socket
import time
import uuid
import math
from contextlib import contextmanager
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Optional

DEFAULT_TTL_SECONDS = 120.0
MIN_TTL_SECONDS = 5.0
MAX_TTL_SECONDS = 900.0
LEASE_FILE = "control.lease"
LOCK_FILE = "control.lock"
STOPPED_FILE = "control.stopped"


class ControlBusy(RuntimeError):
    """另一个鱼已经持有控制权；本次请求被拒绝，不注入任何输入。"""

    def __init__(self, message: str, owner: Optional[dict] = None):
        super().__init__(message)
        self.owner = owner or {}


class ControlUnavailable(RuntimeError):
    """没有有效租约（未获取、已过期、已被停止或凭据过期）。"""


class ControlStopped(RuntimeError):
    """本地已停止控制；必须显式恢复或重新获取后才能操作。"""


@dataclass(frozen=True)
class Lease:
    owner: str
    pid: int
    host: str
    role: str
    acquired_at: float
    expires_at: float
    token: str

    def expired(self, now: Optional[float] = None) -> bool:
        return (now if now is not None else time.time()) >= self.expires_at

    def to_dict(self) -> dict:
        return asdict(self)


def default_state_dir() -> Path:
    override = os.environ.get("FISH_COMPUTER_USE_STATE_DIR")
    if override:
        return Path(override)
    local = os.environ.get("LOCALAPPDATA") or os.environ.get("APPDATA")
    if local:
        return Path(local) / "velella" / "computer-use"
    return Path.home() / ".velella" / "computer-use"


def process_alive(pid: int) -> bool:
    """判断拥有者进程是否仍在运行（Windows 下用 OpenProcess 查询退出码）。"""
    if pid <= 0:
        return False
    if os.name != "nt":
        try:
            os.kill(pid, 0)
        except OSError:
            return False
        return True
    import ctypes
    from ctypes import wintypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.GetExitCodeProcess.argtypes = (wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD))
    kernel32.GetExitCodeProcess.restype = wintypes.BOOL
    kernel32.CloseHandle.argtypes = (wintypes.HANDLE,)
    kernel32.CloseHandle.restype = wintypes.BOOL
    STILL_ACTIVE = 259
    handle = kernel32.OpenProcess(0x1000, False, int(pid))  # PROCESS_QUERY_LIMITED_INFORMATION
    if not handle:
        return ctypes.get_last_error() == 5  # Fail closed on access denied.
    try:
        code = wintypes.DWORD()
        if not kernel32.GetExitCodeProcess(handle, ctypes.byref(code)):
            return False
        return code.value == STILL_ACTIVE
    finally:
        kernel32.CloseHandle(handle)


class _Mutex:
    """跨进程互斥：Windows 上用 msvcrt 字节锁，持锁进程退出即自动释放。"""

    def __init__(self, path: Path):
        self.path = path
        self._handle = None

    def __enter__(self):
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._handle = open(self.path, "a+b")
        if os.name == "nt":
            import msvcrt

            self._handle.seek(0)
            if self._handle.read(1) == b"":
                self._handle.write(b"\0")
                self._handle.flush()
            self._handle.seek(0)
            deadline = time.monotonic() + 3
            while True:
                try:
                    msvcrt.locking(self._handle.fileno(), msvcrt.LK_NBLCK, 1)
                    break
                except OSError as error:
                    if time.monotonic() >= deadline:
                        self._handle.close()
                        self._handle = None
                        raise ControlBusy("控制操作尚未结束，暂时无法获取控制锁") from error
                    time.sleep(0.01)
        return self

    def __exit__(self, *_exc):
        if self._handle is not None:
            if os.name == "nt":
                import msvcrt

                try:
                    self._handle.seek(0)
                    msvcrt.locking(self._handle.fileno(), msvcrt.LK_UNLCK, 1)
                except OSError:
                    pass
            self._handle.close()
            self._handle = None
        return False


class ControlLease:
    """一个进程对桌面控制权的持有者。"""

    def __init__(self, state_dir: Optional[Path] = None, ttl: float = DEFAULT_TTL_SECONDS, owner: Optional[str] = None):
        self.state_dir = Path(state_dir) if state_dir else default_state_dir()
        self.ttl = float(ttl)
        if not math.isfinite(self.ttl) or not MIN_TTL_SECONDS <= self.ttl <= MAX_TTL_SECONDS:
            raise ValueError("控制租约时限必须在 5–900 秒之间")
        self.owner = owner or ("fish-%s" % uuid.uuid4().hex[:12])
        self.host = socket.gethostname()

    # -- 路径 ---------------------------------------------------------------
    @property
    def lease_path(self) -> Path:
        return self.state_dir / LEASE_FILE

    @property
    def lock_path(self) -> Path:
        return self.state_dir / LOCK_FILE

    @property
    def stopped_path(self) -> Path:
        return self.state_dir / STOPPED_FILE

    # -- 内部 ---------------------------------------------------------------
    def _read_lease(self) -> Optional[Lease]:
        try:
            raw = self.lease_path.read_text(encoding="utf-8")
        except FileNotFoundError:
            return None
        try:
            data = json.loads(raw)
        except json.JSONDecodeError as error:
            raise ControlUnavailable("控制租约损坏；请本地停止并恢复") from error
        try:
            return Lease(
                owner=str(data["owner"]),
                pid=int(data["pid"]),
                host=str(data["host"]),
                role=str(data.get("role", "")),
                acquired_at=float(data["acquired_at"]),
                expires_at=float(data["expires_at"]),
                token=str(data["token"]),
            )
        except (KeyError, TypeError, ValueError) as error:
            raise ControlUnavailable("控制租约格式无效；请本地停止并恢复") from error

    def _write_lease(self, lease: Lease) -> None:
        self.state_dir.mkdir(parents=True, exist_ok=True)
        temporary = self.lease_path.with_suffix(".tmp-%s" % uuid.uuid4().hex[:8])
        temporary.write_text(json.dumps(lease.to_dict(), ensure_ascii=False, sort_keys=True), encoding="utf-8")
        os.replace(temporary, self.lease_path)

    def _clear_lease(self) -> None:
        try:
            self.lease_path.unlink()
        except FileNotFoundError:
            pass

    def stopped(self) -> bool:
        return self.stopped_path.exists()

    # -- 公开 API -----------------------------------------------------------
    def acquire(self, role: str = "") -> Lease:
        """获取控制租约。已有有效租约时抛出 ControlBusy，绝不转移所有权。"""
        with _Mutex(self.lock_path):
            if self.stopped():
                raise ControlStopped("本地已停止控制；请先显式恢复或重新获取控制权")
            current = self._read_lease()
            now = time.time()
            if current is not None and not current.expired(now) and current.host == self.host:
                if current.owner != self.owner and process_alive(current.pid):
                    raise ControlBusy(
                        "控制权已被 %s 持有（pid=%s，角色=%s）；本次请求未注入任何输入"
                        % (current.owner, current.pid, current.role or "-"),
                        current.to_dict(),
                    )
            lease = Lease(
                owner=self.owner,
                pid=os.getpid(),
                host=self.host,
                role=role,
                acquired_at=now,
                expires_at=now + self.ttl,
                token=uuid.uuid4().hex,
            )
            self._write_lease(lease)
            return lease

    def refresh(self, role: Optional[str] = None) -> Lease:
        """续期。必须由当前持有者调用。"""
        with _Mutex(self.lock_path):
            if self.stopped():
                raise ControlStopped("本地已停止控制")
            current = self._read_lease()
            now = time.time()
            if current is None or current.expired(now):
                raise ControlUnavailable("控制租约已过期；请重新获取控制权")
            if current.owner != self.owner:
                raise ControlUnavailable("控制租约属于 %s，当前进程无权续期" % current.owner)
            lease = Lease(
                owner=self.owner,
                pid=os.getpid(),
                host=self.host,
                role=role if role is not None else current.role,
                acquired_at=current.acquired_at,
                expires_at=now + self.ttl,
                token=current.token,
            )
            self._write_lease(lease)
            return lease

    def require(self, token: Optional[str] = None) -> Lease:
        """在任何输入注入之前调用；没有有效租约就拒绝。"""
        if self.stopped():
            raise ControlStopped("本地已停止控制；控制不可用，直到显式恢复")
        current = self._read_lease()
        now = time.time()
        if current is None:
            raise ControlUnavailable("当前进程未持有控制租约；请先获取控制权")
        if current.expired(now):
            raise ControlUnavailable("控制租约已过期；请重新获取控制权")
        if current.owner != self.owner:
            raise ControlUnavailable("控制租约属于 %s；当前进程无权操作桌面" % current.owner)
        if current.pid != os.getpid():
            raise ControlUnavailable("控制租约不属于当前进程")
        if token is not None and token != current.token:
            raise ControlUnavailable("控制凭据已失效；请显式重新获取控制权")
        if current.host != self.host:
            raise ControlUnavailable("控制租约属于其他主机")
        if not process_alive(current.pid):
            raise ControlUnavailable("控制租约的拥有者进程已退出")
        return current

    @contextmanager
    def action(self, token: str):
        """校验与整个输入动作持同一把锁；过期回收或停止不能穿插输入。"""
        with _Mutex(self.lock_path):
            current = self.require(token)
            yield current

    def release(self) -> bool:
        """释放自己持有的租约；不持有时不报错。"""
        with _Mutex(self.lock_path):
            current = self._read_lease()
            if current is None:
                return False
            if current.owner != self.owner:
                return False
            self._clear_lease()
            return True

    def stop(self, local: bool = False) -> dict:
        """本地停止控制：释放租约并写入停止标记，之后必须显式恢复。"""
        with _Mutex(self.lock_path):
            released = self.lease_path.exists()
            self._clear_lease()  # Local stop revokes every owner, including another fish.
            self.state_dir.mkdir(parents=True, exist_ok=True)
            # An Agent stop cannot downgrade an existing local-user stop.
            was_local = False
            if self.stopped_path.exists():
                was_local = json.loads(self.stopped_path.read_text(encoding="utf-8")).get("local", False)
            self.stopped_path.write_text(json.dumps({"stopped_at": time.time(), "by": self.owner,
                                                    "local": bool(local or was_local)}, ensure_ascii=False), encoding="utf-8")
            return {"stopped": True, "revoked_lease": released, "owner": self.owner}

    def resume(self, local: bool = False) -> dict:
        """显式解除本地停止标记（不自动获得控制权，仍需 acquire）。"""
        with _Mutex(self.lock_path):
            existed = self.stopped_path.exists()
            if existed and json.loads(self.stopped_path.read_text(encoding="utf-8")).get("local", False) and not local:
                raise ControlStopped("本地用户已停止控制；只能由本地 --resume 命令恢复")
            if existed:
                self._clear_lease()  # Resuming never resurrects a previous credential.
            try:
                self.stopped_path.unlink()
            except FileNotFoundError:
                pass
            return {"stopped": False, "was_stopped": existed, "owner": self.owner}

    def status(self) -> dict:
        current = self._read_lease()
        now = time.time()
        return {
            "owner": self.owner,
            "pid": os.getpid(),
            "state_dir": str(self.state_dir),
            "stopped": self.stopped(),
            "ttl_seconds": self.ttl,
            "lease": {k: v for k, v in current.to_dict().items() if k != "token"} if current else None,
            "lease_active": bool(current and not self.stopped() and not current.expired(now) and current.host == self.host and process_alive(current.pid)),
            "lease_held_by_us": bool(current and not self.stopped() and current.owner == self.owner and current.pid == os.getpid() and not current.expired(now)),
        }
