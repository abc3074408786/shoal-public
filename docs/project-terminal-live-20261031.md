# 项目布局接入真实终端

最新修复：本机桌面已更新为 0.2.3，处理 Claude 加载时反复重绘、旧画面折行错位和隐藏画面滚动条不同步，详见 `docs/release/2026-10-31-terminal-load-0.2.3.md`。

当前安装情况：已于 2026-10-31 更新本机桌面入口为 0.2.1，并生成 Windows 安装包，包含下述尺寸修复。详见 `docs/release/2026-10-31-velella-ui-0.2.1.md`；下文测试过程中的“安装版未更新”描述为更新前状态。

2026-10-31 终端裁切修复：用户反馈右侧少一半，实测 655px 容器内画布宽 1184px、高 828px，超出可用区域。共享 PTY 被宽桌面窗口改宽后，本窗口仍按上次发送尺寸去重，未重新适配。`createTerminalResize.observe` 现在用已消费的主机尺寸更新去重基线；TerminalView 仅在当前窗口有焦点且可见时协商尺寸，窗口/终端获得焦点时重新检查，后台窗口继续按原始几何记录回放。没有提前重排历史输出。

开发验证：尺寸、回放、输入来源 21 项测试通过，包含其他窗口改宽后原窄窗口重新请求尺寸的回归；`npm run build` 通过。真实测试 PTY 外部请求 144×46 后，当前预览恢复 71×43；收起侧栏后适配为 98×43，835px 容器中画布宽 784px、高 774px，完整显示。几何序列和截图分别保存于 `terminal-width-regression.json`、`terminal-width-fixed.png`。仅 8798 开发预览加载了新修复；已打开的旧 Electron 测试进程和安装版尚未更新。未进行独立验收或模型业务任务 UAT。

用户现在可在 `http://127.0.0.1:8798/` 的独立测试项目中使用真实 PowerShell 和原生 Codex 终端。项目栏默认收起，原来的终端输入、复制粘贴、搜索、会话/标签、恢复与分屏实现继续使用。

实现：新增正式组件 `desktop/src/ProjectWorkspace.tsx` 和共享样式 `project-workspace.css`，由 WorkspaceApp 默认项目路由包裹原 Workbench。工作台、项目设置、归档/准备中状态、任务会话定位和工作区管理回调保留。选定布局不再仅存在于测试夹具；8797 的布局夹具也使用正式默认项目路由，只有历史分栏方案使用替代渲染入口。

开发桥改为首次解析 IPC 地址后直连本机管道，复用 `desktop/electron/runtime.cjs` 的 socketRequest。保持本机来源检查、请求大小限制、超时及断线不重放写请求。原每个 RPC 新建进程方式实测约 113–133ms；热管道调用约 13–18ms。首个地址解析仍需一次进程启动。

验证状态：

- 构建：`npm run build` 通过；预览 TypeScript、whitespace 检查通过。
- 开发测试：导航/工作区/输入队列/尺寸/剪贴板测试 27 项通过；开发桥及终端输入来源测试 11 项通过，共 38 项。
- 真实验证：从界面创建 PowerShell，键入并运行 `Write-Output 'VELELLA_TERMINAL_OK'; 2 + 2; Get-Location`，看到标记、结果 4 及独立临时目录；CLI 版本命令成功；项目栏展开/收起保持同一真实会话。
- Codex：通过原本的“新建 Codex 终端 → 本机执行”流程启动 0.165.0，原生界面和输入框已显示。首次使用旧开发桥时，颜色查询回复迟到并出现于输入框；切换直连后第二次启动未复现。此结论限本次环境，未做跨版本/多次原生启动回归，不修改协议回复分类、CLI 配置或审批设置。首次测试会话已正常退出并关闭标签。
- Claude：通过界面启动原生 Claude Code 2.1.238；确认独立空测试目录后，出现 Welcome back 和原生输入框。未发送模型任务。
- 桌面测试：使用 desktop/node_modules/electron/dist/electron.exe 加载本次 dist，独立 profile 与 8798 共用同一测试运行时；首次后台窗口通过第二实例激活后显示。桌面键盘输入执行 `Write-Output 'VELELLA_DESKTOP_INPUT_OK'` 成功；原有向右分屏创建第二个 PowerShell，执行 `Write-Output 'VELELLA_SPLIT_INPUT_OK'` 成功。展开/收起项目栏后两个分屏和输出保留，收起恢复 54px 高度。测试窗口已切换浅色主题。
- 多窗口限制：桌面保存分屏/标签状态后，旧浏览器页面切换标签触发布局版本保护，提示 `layout changed in another window; reload before saving`；刷新一次后加载最新 Claude 标签，无错误。未绕过版本保护；当前仍需手动刷新旧窗口。
- 独立复核：只读探子定位了 OSC 回复的来源及转发路径；未对最终源码作独立测试/技术验收。
- 集成/部署/在线：正式前端源码及 dist 已构建，并在独立 Electron 桌面窗口运行；8798 为当前真实运行时开发页面。桌面安装版未更新。8797 仍是内存模拟会话夹具，不能代替真实验证。
- 业务 UAT：未向 Codex/Claude 提交模型任务，未验证真实模型回答或 SSH。没有改动用户正在使用的运行时数据目录或终端。

测试环境在 `artifacts/project-shell-20261031/live-terminal/manifest.json` 中记录。它使用独立 profile、仓库 release runtime 的副本、现有固定 ConPTY sidecar，以及系统临时目录下新建的空项目。初始化前先创建 profile，避免 Windows IPC 在路径规范化前后产生不同地址。测试 PowerShell 和第二个 Codex 会话保留供界面查看；旧测试 Codex 已退出。跳过了 Codex 自带更新提示，未更新 CLI。

证据：

- `artifacts/project-shell-20261031/live-terminal-evidence.json`：真实会话 ID、PID、布局、输出验证及 runtime SHA256。
- `artifacts/project-shell-20261031/native-powershell-live.png`
- `artifacts/project-shell-20261031/native-codex-live.png`
- `artifacts/project-shell-20261031/native-claude-live.png`
- `artifacts/project-shell-20261031/native-desktop-split.png`
- `artifacts/project-shell-20261031/native-desktop-split-light.png`
- `artifacts/project-shell-20261031/native-desktop-claude.png`
- `artifacts/project-shell-20261031/setup-live-terminal.cjs`
- `artifacts/project-shell-20261031/capture-live-terminal.cjs`
