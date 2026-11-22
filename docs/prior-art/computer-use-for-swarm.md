# Prior art: Computer Use for Velella fish

- Date: 2026-11-17
- Mode: quick
- Searches used: 2 batched web queries, direct public-source checks for three candidates, and local integration inspection
- Saturated: no

## Need

鱼群通过 MCP 获取截图、操作鼠标和键盘；Windows 本地、多鱼互斥、中文输入，并适配现有代理的 1 MiB JSON-RPC 整帧上限。

## Verdict

| Part | Verdict | Project | Why |
|---|---|---|---|
| Fish integration | Keep | Velella external MCP/catalog | 已有来源确认、角色工具白名单和图片转发；无需另做工具总线 |
| MCP/capture | Compose | Official Python MCP SDK + Pillow | 复用协议及图像能力；实际依赖版本由候选 lock 固定，不采用浮动 latest |
| Windows control | Compose | Native Windows APIs | Unicode 输入及坐标语义由窄适配层负责；补充跨进程控制租约、停止机制、整帧预算 |

不完整采用大型第三方 MCP 服务：现有候选的被检查截图路径未处理 Velella 的整帧上限，跨进程独占也未被证实；因此尚不能直接推荐其作为完整、已审计服务。协议、图像编解码不自建。预计自有适配及测试为数百行量级，后续需维护 Windows 输入 ABI、DPI/多屏坐标、租约及依赖更新；这不是完成后的实际代码量。

## Read

Read-only scout checked README and the named core sources, not complete repositories or actual desktop execution. Root checked the actual Velella relay/catalog and Pillow documentation. No external service was installed or run.

| Project | Link / revision | Covers | Lacks / not verified | Licence | Last commit | Cost to adopt | Access it needs |
|---|---|---|---|---|---|---|---|
| Windows-MCP 0.8.9 | [CursorTouch](https://github.com/CursorTouch/Windows-MCP/tree/02950fa1331f5b9e112259f0be5cd442a4bb0ba9) | Tool whitelist, desktop actions, scaled PNG | `desktop/service.py:312–339` has no observed serialized-frame budget; README describes best-effort control ownership, not proven cross-process exclusivity | MIT | HEAD pinned; timestamp not retrieved | Python ≥3.14, FastMCP/dxcam/pywin32 and other dependencies | Desktop; broader services need full review |
| windows-computer-use-mcp 0.1.1 | [hiphopanda](https://github.com/hiphopanda/windows-computer-use-mcp/tree/e22aac24268ed190675fa282c4ddb52da281ee37) | Screenshots and keyboard/mouse | `src/utils/image.ts:12–40` base64 PNG without observed budget; `src/index.ts:35–58` broad tool set; `src/tools/keyboard.ts:88–105` uses SendKeys, Chinese unverified | MIT | HEAD pinned; timestamp not retrieved | Bun ≥1.3.11 and PowerShell | Desktop, application launch, clipboard |
| PyAutoGUI | [asweigart](https://github.com/asweigart/pyautogui/tree/b4255d0be42c377154c7d92337d7f8515fc63234) | Mouse/key primitives and capture composition | README limits reliable support to main monitor; `_pyautogui_win.py` ASCII key mapping cannot directly satisfy reliable Chinese typing | BSD-3-Clause | HEAD pinned; timestamp not retrieved | Additional library and dependencies; native Unicode/multi-monitor adaptations still needed | Desktop |
| Pillow | [ImageGrab docs](https://pillow.readthedocs.io/en/stable/reference/ImageGrab.html) | Windows capture including all-screen and window options | Not an input controller, lease manager or MCP service | Not audited in this pass | Not retrieved | Existing default Python has 10.4.0; isolated candidate must lock its own verified version | Screenshot access |

## Seen, not read

Search results also named `ahmetdenizyilmaz/desktop-control-mcp`, `felenko/WindowsOSMCP` and other desktop MCP projects. Their whole services were not inspected; no suitability claim is made.

## Ruled out

No universal rejection of a project. Direct unmodified adoption of the checked services is not established for this contract; PyAutoGUI alone does not cover Chinese/multi-monitor requirements.

## How it was searched

- Computer terms: Computer Use, 电脑操作, 桌面自动化, Windows automation, GUI automation, RPA, screenshot mouse keyboard.
- Integration terms: MCP server, stdio tools, 工具目录, role tools, external MCP, shared desktop control.
- Queries: `Windows computer use MCP pyautogui screenshot desktop automation MCP server`; `CursorTouch Windows MCP computer use PyAutoGUI MCP alternatives`.
- Sources: connected web search, public GitHub README/config/core files, Pillow official docs; existing Velella Rust relay/catalog/provider and catalog UI.
- Failed/unavailable: initial `pyautogui/pyautogui` URL returned 404; corrected to `asweigart/pyautogui` with source pin above. No blind retry of the bad URL.
- Seeds: CursorTouch/Windows-MCP and hiphopanda/windows-computer-use-mcp; neighboring underlying PyAutoGUI checked.

## Not covered

No issue tracker review, full service audit, runtime installation, maintenance timestamp audit or empirical desktop tests. This bounded survey stops at an implementation-plan decision; it does not claim saturation, dependency safety certification or native acceptance. All executable adoption remains subject to fixed dependency review and the plan's tests.
