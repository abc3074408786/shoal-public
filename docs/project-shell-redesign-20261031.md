# 项目界面外壳修改 · 2026-10-31

用户可在开发预览中体验：工作台项目卡片 → 真实 Workbench 组件 → 原终端创建流程、会话标签与文件工具。空工作区按选定参考图显示单一“新建终端”入口。

## 范围

- 顶部改为侧栏开关、返回/前进、文件/编辑/视图/帮助、窗口控制。
- 侧栏保留四个主导航；项目切换移入中间区域并默认整栏收起。完整工作区树仍从“文件 → 项目与工作区”打开。当前正式组件和真实终端验证见 `project-terminal-live-20261031.md`。
- 项目工具独立于全局标题栏；按后续要求移除中间的项目/鱼群标题，有会话时显示原会话标签与操作工具。
- 工作台使用已选鱼群标识和玻璃项目卡片，创建/打开项目沿用真实流程。
- 状态与资源、执行主机、主题与工作区工具迁入视图菜单。
- 原终端、Agent 会话、文件、Git、浏览器、分屏与后端实现继续使用，未替换为静态终端。
- 项目内容列表通过 Portal 使用 Workbench 原有会话状态和打开回调，不新增会话轮询；选择已有终端仅切换或重开标签，关闭侧栏时不会把列表回落到中间区域。

## 验证与边界

- 实现：已集成到 `desktop/src`。工作区原有未提交变更保留；修改前源文件快照在 `artifacts/project-shell-baseline-20261031`。
- 开发验证：`npm run build` 通过；`node --test tests/shell-navigation.test.mjs tests/workspace-context.test.mjs` 共 17 项通过；变更文件 whitespace 检查通过。
- 浏览器验证：真实 React 组件加内存 RPC 夹具。检查了工作台/项目卡片、Codex 创建对话框→模拟会话标签、文件工具、收起侧栏后项目管理、工作区选择、返回/前进、关闭最后标签后恢复，以及原生模态弹窗中的“显示/隐藏已归档”菜单。
- 项目内容分组增量验证：从侧栏创建两个模拟 Codex 终端、切换终端、关闭标签再从侧栏重开、切换到另一空项目再返回、收起/展开侧栏，以及 Git 审查归入“其他”。最终仅两次启动请求和两个会话，真实 RPC 转发为 0；控制台无警告/错误。此增量未做独立测试或独立技术验收；夹具未模拟工作记录 API，终端显示“记录暂不可用”。
- 独立复核：只读代理发现菜单 Portal 层级、主机切换历史 scope、空态恢复标签入口三处问题。主线程修正并验证前后两项 UI 流程；主机 scope 与工作区重置改为同批更新，未做真实 SSH 主机切换 UAT。修正后未再次独立复核。
- 集成：正式前端源码及 dist 已构建；开发预览为 `http://127.0.0.1:8797/tests/project-shell.html`。
- 安装/部署：未更新当前安装版。后续已在独立 profile 中验证真实 PowerShell 命令与原生 Codex 启动，未提交模型任务；桌面窗口控制、SSH、模型回答及业务 UAT 尚未验证。8798 为真实终端开发页面，8797 为隔离内存夹具。

## 证据

- `artifacts/project-shell-build-20261031.log`
- `artifacts/project-shell-20261031/browser-validation.json`：真实 RPC 转发计数为 0，所有会话为内存模拟。
- `artifacts/project-shell-20261031/project-shell-light.png`：最终空态。
- `artifacts/project-shell-20261031/terminal-and-files.png`：原终端组件与文件工具。
- `artifacts/project-shell-20261031/project-with-terminals.png`：本次项目、终端与其他内容分组。
- `artifacts/project-shell-20261031/project-contents-validation.json`：本次内存夹具结果（省略轮询请求日志）。

当前关键源码 SHA256（含后续对比预览接入点，说明见 `project-layout-options-20261031.md`）：

| 文件 | SHA256 |
| --- | --- |
| WorkspaceApp.tsx | CF95B2CB99CA4DBC1F62A78816F4A71E41E4208823D9BD872223EB8274468EDB |
| Workbench.tsx | 0B8CBA7114D27D1C55543EBD2EB5F062B06BAD6179C5DCD485D2C61032059380 |
| project-shell.css | 8B28CE6ACB834C9968BED78409F61A6BC5B8B61B374069C01F9A53600468FC61 |
| ActionMenu.tsx | 735351768587EF2BB7AEB896F3D5A365C24E3F2E6D8128D5C6253047B2D4E7D8 |
