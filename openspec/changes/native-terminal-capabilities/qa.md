# 原生终端能力修复验收 — 2026-10-28

用户现在可以在已打开的新 Codex 标签继续交流：原生输入区及已发送消息的底色已经恢复，底部模型栏与快捷提示完整显示。旧 PowerShell 标签和已有后台会话保留。

本次两项可复现终端缺陷已修复并安装。用户所说的“回答能力与直接用 PowerShell 差距很大”尚未完成一般性验收；一次问候不能证明复杂任务能力相同，也不能证明模型降级。

## 分层状态

| 层级 | 结果与边界 |
| --- | --- |
| 实现 | `terminal-fit.ts` 按内容区计算网格，扣除父容器和 xterm 自身留白及滚动条；TerminalView 接入。Windows 打包准备固定版本的微软 ConPTY 配套文件、MIT 许可和 provenance。 |
| 开发验证 | 尺寸回归先在旧实现失败：900px 容器的字符网格到 916px，可用内容底部仅 884px。修复后 DOM/WebGL 请求模式、1/1.5/2 DPR、两种窗口尺寸共 12 场景通过；29 项相关单测通过；`npm run build` 通过；真实打包 preflight 的四个资源均存在且校验通过。 |
| 独立测试 | 原生只读探子 `/root/terminal_candidate_review` 独立运行 3 项打包测试，重算归档和 helper 哈希，核对 124 个包路径及方法体重构。它没有重跑真实 CLI/浏览器测试。 |
| 技术验收 | 独立结论 pass，没有发现阻止固定 x64 候选安装的实质缺陷。arm64 只固定了官方包哈希，没有原生 arm64 验证。 |
| 集成 | 安装候选以当前正式包为基线，仅替换唯一 FitAddon `proposeDimensions` 方法体，使用已测 helper；其余包文件逐字节相同。未安装整个共享工作区的重新构建输出。Rust runtime 可执行文件完全不变。 |
| 部署 | 固定候选已安装；向实际 runtime 目录及默认 runtime 目录添加四个配套文件。只重开 GUI，daemon PID 59196 保持不变。 |
| 当前在线 | 实际新会话 `20f6a0c4-7121-4e1d-ae0d-6fb2af21460f` 的 host PID 77756 加载了安装目录的 conpty.dll；记录到两项颜色查询及原生背景指令。实际窗口显示完整底栏。输入“你好”后完整回复并完成该轮。安装后再次确认 30 个受保护进程身份、6 份历史固定前缀保持不变。 |
| 业务 UAT | 未完成复杂真实任务、长期多会话和一般回答能力对照。当前通过的是原生终端协议、尺寸、启动及问候输入/回复样本。 |

## 根因及测试中的区别

系统 ConPTY 在同一查询程序中吞掉 OSC 10/11；官方 ConPTY 能转交并收到两种 RGB 回复。该区别通过同一个未修改 Velella runtime 的隔离 host 复测成立。

Codex 0.164.0 的 Windows 颜色探测会等到受保护启动界面完成。最初两次真实 CLI 检查过早读取了启动画面，不能作为修复验收；停止重复后，核对匹配版本的官方启动源码，改为等待协议探测实际发生。最终真实 CLI 测试收到前景/背景回复、发出原生背景、完整显示 footer、无启动驱动输入，并传递两次 Ctrl+C。

生产新终端先停在更新提示，再停在 hooks 审核。测试跳过本次更新，并选择 **Continue without trusting**，没有批准新增 hooks，也未修改全局信任配置。工作记录能力不在此次验收结论中。随后已安装窗口恢复原生输入区，完成一次问候检查。

FitAddon 当前版本按父容器 CSS 高度计算，却只扣 xterm 自身 padding；Velella 的 16px 留白在父容器上，因此底部超出。新的内容区测量保留现有字体、字号、行高和主题。

## 固定候选和回退

- 安装前 app.asar：`90d356ca8d78e290fdea884af0cf35807c1929061f4d0b8cce9287b7544c630e`。
- 已安装 app.asar：`74878d21ad0afc01da31ec403b451a8643b1a19cc97028c671d3cb911730c1ee`。
- 不变的 runtime：`01c24d59951e96c204b41e02a58059bb0b18ca1157fc31910cb557f0966a123b`。
- Microsoft.Windows.Console.ConPTY：`1.24.261001001`，从官方 NuGet 下载，归档 SHA256 `4d6aaddc1d2385c9f5897df28f33879f699f8f2783315d5204cf3d8c3616ac5f`。x64 DLL 和 OpenConsole.exe 的微软 Authenticode 签名有效。
- 回退备份：`artifacts/terminal-reply-parity-20261028/installation-backup/app.asar` 及数据库备份。回退 GUI 包应先正常退出 GUI，保留 daemon/hosts；已加载的配套 DLL 不应直接移除。
- 现有 host 仍使用创建时的后端；配套 ConPTY 对新 host 生效。旧历史不会补绘缺失的背景色。

## 证据索引

目录：`artifacts/terminal-reply-parity-20261028/`。

- `candidate-package.json`、`independent-acceptance.json`：范围、固定候选和独立核验。
- `host-palette-comparison.json`、`dependency-manifest.json`：系统/官方后端比较和固定依赖。
- `fit/result.json`：12 个尺寸场景。
- `native/startup-result.json`、`native/native-codex-startup.png`：隔离真实 Codex 协议/画面/中断证据。
- `installation.json`、`online-verification.json`、`live-capabilities.json`：安装、实际模块加载、问候、进程与历史保留。
- `live-session.json`：为当前验收创建并留在前台的新标签；没有创建或派发业务开发任务。

源码及原生探针：`desktop/tests/terminal-fit.browser.mjs`、`desktop/tests/terminal-capabilities.native.cjs`、`desktop/tests/conpty-package.test.mjs`。实际运行需使用记录的固定 runtime、CLI 及本机测试依赖；不要对已有用户会话重复发送测试输入。
