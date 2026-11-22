# 验收记录

## 结果

已实现并安装。变更编号 `workbench-interface-refresh`。原后台 runtime 未替换，原工作区和会话保留。详见 `artifacts/workbench-interface/deployment.json`。

- 前端 TypeScript + Vite 构建通过。保留既有的大 chunk 提示。
- `npm test`：91/91 通过。
- `node tests/workbench-interface.browser.mjs`：35/35 通过。
- `node tests/terminal-execution.browser.mjs`：26/26 通过，覆盖现有启动、权限、失败重试流程。
- Tauri release 构建通过；存在原有 clipboard dead_code 警告。
- 本轮修改的源码 `git diff --check` 通过；整个仓库的其他未提交改动存在原有 CRLF/尾随空白，未改动它们。

## 画面与交互

实际组件在隔离 RPC 夹具中检查：浅/深色主题、1440×900、1100×760、900×700、多标签、长名称、双分屏、左右侧栏、工作区切换、空工作区、等待授权、菜单键盘、Esc 后焦点回到触发按钮、刷新恢复布局。浏览器异常为零，没有真实 RPC/模型调用。

截图已逐张核对主要状态：`dark-1440.png`、`light-1440.png`、`dark-900.png`、`tools-900.png`、`split-900.png`、`empty-900.png`。窄分屏中的终端文本按终端自身宽度裁剪，滚动及会话内容未改写。

发现并修复 xterm 的默认黑色 viewport 在浅色主题下露出：仅让背景使用现有 `--terminal-bg`。键盘焦点测试最初早于菜单退场动画读取，修正为等待菜单关闭后检查；没有因此改写生产焦点逻辑。

本机候选版使用独立数据目录 `/tmp/velella-interface-preview`，且以已安装 runtime 执行。真实普通 shell 经界面创建、关闭标签后 PID 保留，随后仅停止本次测试会话。没有启动付费 Agent。候选桌面与隔离 daemon 已退出，测试数据留证。

本机桌面在 1440×900 客户区实测并已安装；900px 验收属于浏览器组件层，原生应用既有最小宽度 960px 未调整。

## 前后截图与部署

- `artifacts/workbench-interface/native-before.png`：原安装版、原有 Codex 会话。
- `artifacts/workbench-interface/native-after.png`：新版、同一工作区与 Codex 会话、同尺寸。
- Native accessibility tree 确认新导航、新工作区信息行和原有 Codex 标签。
- Orca 截图工具在 Linux 返回边框坐标偏移 37px；核对目标 X11 窗口后使用同一窗口实际客户区截图，未重绘画面。
- 回退二进制：`/home/admin/.local/share/shoal/backups/interface-20261021-002711/velella-desktop`。
- 已安装 SHA256：`ac9805aa64c1faadb35485151d545814aa35e0bfe13abc21e26c487a91eed3b2`。
- 如需回退，仅关闭桌面程序并恢复备份二进制，再启动 `/home/admin/.local/bin/shoal`；不要停止后台 runtime。

## 设计流程与限制

采用现有 DESIGN.md / MOTION.md；两项基础检查均 ready。无新依赖、无新动效、无外部发布。路由已保存；可选 companion 检测报告不可用，使用现有组件和原生窗口/浏览器测试。没有声明运行完整流水线门禁、像素一致性评分或完整可访问性认证。

本轮视觉与交互验证通过；不是 Orca 全界面复刻。Linux 浏览器自动化限制及普通 shell 标签显示 PowerShell 属于已有问题，未在本轮扩展处理。
