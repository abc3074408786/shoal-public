# Velella 鱼群 Computer Use（本地 Windows）

本目录提供一个**本地 Windows MCP 服务**，让 Velella 的鱼通过 MCP 观察与操作这台桌面：截图、鼠标、键盘。
它是**显式启用**的本地集成：默认服务只提供观察；控制目录带 `--allow-input`，输入还必须先取得控制租约及本次凭据。

- MCP 协议：官方 Python MCP SDK（stdio）
- 图像与编解码：Pillow
- 输入注入：Windows 原生 `user32` API（`SetCursorPos` / `SendInput`）
- 不写剪贴板、不执行 shell、不跨主机、不做 macOS/Linux

## 1. 安装

需要 Python 3.12 与 `uv`：

```powershell
cd tools/computer-use
uv sync --frozen --python 3.12
uv run --frozen python -m unittest discover -s tests
```

环境被完全隔离在 `tools/computer-use/.venv`，不修改全局 Python、用户配置或 Velella 实时配置。

## 2. 生成鱼群目录清单

```powershell
uv run --frozen python -m velella_computer_use.catalog `
  --output D:\path\to\velella-computer-use-catalog.json
```

生成的清单包含两个条目，均为绝对路径：

| 条目 | 暴露范围 |
|---|---|
| `velella-computer-use-observe` | 仅 `computer_screenshot`（只读观察） |
| `velella-computer-use-control` | 截图 + 控制租约 + 鼠标键盘工具 |

清单只含 `id/command/args/catalog`，没有凭据、环境变量或账户字段，符合现有 fish 管理的来源格式
（`version: 1`，每条最多 64 个参数，工具名不超过 128 字节）。

## 3. 在鱼群中启用

1. 在 Velella 界面载入上面的目录清单路径 → 预览 → 明确勾选工具 → 确认来源条目。
2. 把确认后的 MCP 条目加到**某一个本地角色**；远程角色仍不支持自定义 MCP（保持既有边界，不会静默回退到本机）。
3. 只有被选中的工具会通过真实的鱼群外部 MCP 中继暴露给该角色。

目录清单变化后必须重新载入、预览并确认；旧的确认记录不再有效。

## 4. 坐标语义

截图覆盖**整块虚拟桌面**（多显示器拼成的桌面）。返回的元数据包含：

```json
{
  "origin_px": [-1920, -120],
  "size_px": [3840, 1080],
  "physical_size_px": [3840, 1080],
  "scale_to_physical": [1.0, 1.0],
  "coordinate_space": "image pixel",
  "transform": "physical = origin_px + floor(image_px * scale_to_physical)",
  "negative_origin": true
}
```

- 你给工具的坐标是**图像像素**坐标，原点是图像左上角（`(0,0)`）。
- `size_px` 是实际返回图片的尺寸；压缩时可能缩小，`physical_size_px` 保留物理桌面尺寸。物理坐标按 `origin_px + floor(image_px * scale_to_physical)` 换算。
- 当副显示器位于主显示器左侧或上方，物理原点可能为负数。
- 同一个服务先调用 `computer_screenshot`，再按该图片的像素坐标点击。显示器布局变化会拒绝旧坐标，须重新截图。
- 越界坐标在注入前被拒绝，不会产生越界点击。

## 5. 控制租约与恢复

- 一台 Windows 桌面同时只允许**一个控制进程**（跨鱼角色、跨工作流）。
- 租约有界（默认 120 秒；启动参数 `--lease-ttl` 可在 5–900 秒之间调整）。到期后需要重新获取，不隐式续期。
- `computer_acquire_control` 返回 `lease.token`；所有鼠标键盘工具必须传入 `lease_token`。重新获取会生成新凭据，旧值不能继续使用。
- 拥有者进程退出、崩溃或租约过期后，控制自动回到"不可用"，需要**显式重新获取**。
- 另一个鱼持有控制时，请求返回"忙"，**不会**转移所有权，也不会注入任何输入。
- `computer_stop_control` 是本地停止：释放租约并写入停止标记；之后必须先
  `computer_resume_control`，再重新 `computer_acquire_control`。
- 本地 `--stop` 的权限高于鱼的工具调用：鱼不能通过 `computer_resume_control` 或再次停止来撤销它，须由本地 `--resume` 恢复。工具发起的普通停止仍可由工具显式恢复。
- 这些是**协作式流程控制**，不是针对同用户原生程序的安全沙箱。

可在工具环境中通过命令立即停止整台桌面的控制，不需要模型配合：

```powershell
.\.venv\Scripts\python.exe -m velella_computer_use.server --stop
# 确认可以恢复后执行；恢复不会自动获得控制权
.\.venv\Scripts\python.exe -m velella_computer_use.server --resume
```

默认控制状态位于 `%LOCALAPPDATA%\velella\computer-use`，各鱼共享。`--state-dir` 仅用于显式隔离的验证环境；日常角色应共享默认目录，不要配置多个独立控制池。服务进程退出后，新的鱼可显式回收已退出拥有者的租约。状态文件不是活跃进程本身。

给鱼的使用顺序：截图 → 获取控制 → 用返回的 `lease.token` 调用输入工具 → 截图核对效果 → 释放控制。输入回执不代表应用已经完成业务任务。

## 6. 验证

```powershell
# 协议模式：真实鱼群中继 + 临时数据目录
uv run --project tools/computer-use --frozen python scripts/verify-computer-use.py `
  --mode protocol --evidence D:\path\to\evidence

# 原生模式：会短暂显示并操作自有夹具窗口
uv run --project tools/computer-use --frozen python scripts/verify-computer-use.py `
  --mode native --evidence D:\path\to\evidence
```

验证器记录源码、运行时与目录清单哈希，以及每项检查与失败原因。

## 7. 边界与限制

- 仅 Windows；其他平台显式失败，不会静默降级。
- 不写剪贴板：中文等 Unicode 文本以键盘事件注入。
- 截图整帧受真实中继的 **1 MiB** 上限约束；超限时自动缩放/重编码，仍超限则明确报错，不发送超大帧。
- 采集上限为 6400 万像素，覆盖双 4K 桌面；编码输出仍遵守 1 MiB 上限。真实多屏硬件尚待使用验收，负原点与双 4K 高熵图像已有合成回归。
- 不执行任意 shell 命令；工具集是固定枚举。
- 截图与输入都需要一个已登录、有交互的桌面会话；锁屏或断开会明确报错。
- 输入回执只证明"已注入动作"，**不证明业务成功**。
