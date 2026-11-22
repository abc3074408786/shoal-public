#!/usr/bin/env python3
"""Computer Use 自有测试窗口（仅供验证器使用）。

这是一个由验证器自己拥有的窗口：记录点击/拖拽/滚动计数与文本框内容，
通过命令文件与验证器交换状态。任何写入都只发生在本进程自己的窗口里。
"""

import json
import ctypes
import os
import sys
import tkinter as tk
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'tools/computer-use/src'))
from velella_computer_use.windows import enable_dpi_awareness, foreground_pid, user32
from ctypes import wintypes


def main() -> int:
    workdir = Path(sys.argv[1])
    workdir.mkdir(parents=True, exist_ok=True)
    ready = workdir / "ready.json"
    command = workdir / "command.txt"
    dump = workdir / "state.json"

    enable_dpi_awareness()
    root = tk.Tk()
    root.title("Velella Computer Use Fixture")
    root.geometry("680x520+140+140")
    root.attributes("-topmost", True)
    focus_requested = False

    entry = tk.Entry(root, width=48, font=("Segoe UI", 14))
    entry.pack(pady=12)
    counters = {"clicks": 0, "scrolls": 0, "drags": 0, "keys": 0, "hotkeys": 0}
    label = tk.Label(root, text="clicks=0 scrolls=0 drags=0 keys=0", font=("Segoe UI", 11))
    label.pack(pady=4)
    canvas = tk.Canvas(root, width=560, height=260, bg="#f2f2f2", highlightthickness=1)
    canvas.pack(pady=8)
    canvas.configure(scrollregion=(0, 0, 560, 1600))
    canvas.create_rectangle(60, 60, 180, 180, fill="#4682b4", tags='dragtarget')
    canvas.create_text(120, 120, text="drag me", fill="white", tags='dragtarget')
    for index in range(10):
        canvas.create_text(350, 80 + 100 * index, text='scroll row %s' % index)
    last_drag = [0, 0]

    def refresh_label() -> None:
        label.configure(text=" ".join("%s=%s" % item for item in counters.items()))

    def on_click(event):
        last_drag[:] = [event.x, event.y]
        counters["clicks"] += 1
        refresh_label()
        canvas.create_oval(event.x - 6, event.y - 6, event.x + 6, event.y + 6, outline="#c0392b")

    def on_drag(event):
        canvas.move('dragtarget', event.x - last_drag[0], event.y - last_drag[1])
        last_drag[:] = [event.x, event.y]
        counters["drags"] += 1
        refresh_label()

    def on_scroll(event):
        counters["scrolls"] += 1
        refresh_label()
        canvas.yview_scroll(-1 if event.delta > 0 else 1, "units")

    def on_key(event):
        counters["keys"] += 1
        refresh_label()

    def on_hotkey(event):
        counters['hotkeys'] += 1
        entry.selection_range(0, tk.END)
        refresh_label()
        return 'break'

    entry.bind('<Control-a>', on_hotkey)

    canvas.bind("<Button-1>", on_click)
    canvas.bind("<B1-Motion>", on_drag)
    canvas.bind("<MouseWheel>", on_scroll)
    root.bind("<Key>", on_key)

    def snapshot() -> dict:
        return {
            "pid": os.getpid(),
            "foreground_pid": foreground_pid(),
            "window_viewable": bool(root.winfo_viewable()),
            "focus_request_accepted": focus_requested,
            "window": {"x": root.winfo_x(), "y": root.winfo_y(), "w": root.winfo_width(), "h": root.winfo_height()},
            "entry_rect": {
                "x": entry.winfo_rootx(),
                "y": entry.winfo_rooty(),
                "w": entry.winfo_width(),
                "h": entry.winfo_height(),
            },
            "entry": entry.get(),
            "canvas_rect": {"x": canvas.winfo_rootx(), "y": canvas.winfo_rooty(), "w": canvas.winfo_width(), "h": canvas.winfo_height()},
            "box_bbox": list(canvas.bbox('dragtarget')),
            "scroll_view": list(canvas.yview()),
            "counters": dict(counters),
        }

    def focus_owned_window():
        nonlocal focus_requested
        root.deiconify()
        root.update()
        root.lift()
        user32.GetAncestor.argtypes = (wintypes.HWND, wintypes.UINT)
        user32.GetAncestor.restype = wintypes.HWND
        user32.SetForegroundWindow.argtypes = (wintypes.HWND,)
        user32.SetForegroundWindow.restype = wintypes.BOOL
        handle = user32.GetAncestor(root.winfo_id(), 2)  # GA_ROOT, this fixture only.
        focus_requested = bool(user32.SetForegroundWindow(handle))
        entry.focus_force()
        root.update()

    focus_owned_window()
    root.attributes("-topmost", True)
    ready.write_text(json.dumps(snapshot(), ensure_ascii=False), encoding="utf-8")

    def poll():
        if command.exists():
            action = command.read_text(encoding="utf-8").strip()
            try:
                command.unlink()
            except OSError:
                pass
            if action == "dump":
                dump.write_text(json.dumps(snapshot(), ensure_ascii=False), encoding="utf-8")
            elif action == "focus":
                focus_owned_window()
            elif action == "quit":
                root.quit()
        root.after(60, poll)

    poll()
    root.mainloop()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
