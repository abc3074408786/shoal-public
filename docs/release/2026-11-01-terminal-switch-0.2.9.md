# Velella 0.2.9：CLI 精确切换终端

用户现在可用 `velella-cli terminal switch --workspace <ID> --terminal <ID> --json`（或 `agent switch`）让同 profile 的本机桌面选择对应项目、标签和终端分屏，并取得实际输入焦点确认。切换可重新展示已有会话，不创建、恢复或停止其进程。完整 Orca CLI 对齐仍未完成，账号管理、computer 等另行开发。

## 实现与边界

CLI 先经 daemon 检查工作区及会话归属，再向同 profile 的 owner-only 桌面 IPC 发送 `terminal_switch`。桌面用唯一请求 ID、工作区 ID、会话 ID 和截止时间匹配 renderer 回执。主进程只在 renderer 确认对应 textarea 获得焦点后返回 `velella-terminal-switch-v1`，不把发送事件当作成功。

renderer 在本机目标下读取最新工作区、会话数据，再切换页面、选择原生终端标签或分屏。请求通过持久 prop 交给 Workbench，避免跨项目首次挂载丢事件。SSH CLI 路由、桌面正在连接 SSH、已归档工作区、错误归属、原生聊天表面和打开的对话框/菜单会拒绝。一个窗口只处理一个切换请求。超时不自动重试，选择可能已经发生；成功也不证明 PTY 可输入或模型执行。

独立审阅发现的批注边界已修复：显式终端切换跳过批注聊天推断，仅复用原生终端标签，已有聊天标签不劫持导航；分屏叶仍复用原组。普通批注导航保留原有行为。

## 验证与当前状态

| 层级 | 实际结果 |
| --- | --- |
| 实现 | Switch 命令、SSH 前置拒绝、主进程焦点握手、renderer 导航与终端选择、内置指南 revision `2026-11-01.switch.1` |
| 开发验证 | 67 项 Rust CLI 回归通过；7 项握手/批注终端选择测试通过；14 项布局/批注/导航回归通过；TypeScript/Vite 与 NSIS 构建通过，共 88 项开发测试 |
| 独立测试 | 未执行独立动态测试。独立静态审阅发现批注边界；修复后的独立只读复核确认原问题已关闭，限定范围未发现新阻断 |
| 技术验收/集成 | 主线程使用冻结旧 runtime、隔离 profile、真实 Electron/PTY 验证：从工作台进入指定分屏、键盘标记只出现在指定分屏、跨项目打开无标签的已有终端、菜单打开时拒绝并保留原界面。最终发行包再次验证分屏选择及键盘输入，录制回读命中仅在指定会话 |
| 安装 | 同一发行产物并列安装到 `D:\orcas\artifacts\installed-ui-20261101-terminal-switch`，桌面/开始菜单两个快捷方式已更新。用户 PATH CLI 与桌面内置 CLI hash 一致。未运行 NSIS 注册安装向导，安装包单独保留 |
| 当前在线 | GUI PID 11520，daemon PID 29452；新 daemon 使用原冻结 runtime。正式窗口切到原知识蒸馏会话 `194671bb-c152-4e63-99ca-7f13f4e5d9ba`，焦点回执和原生窗口核对通过，未向正式 Agent 发送输入 |
| 数据保留 | 安装前后 43 个 session ID、43 份终端历史前缀一致；两份原生 Codex config/auth 文件 hash 一致；数据库及原快捷方式已有备份 |
| 业务 UAT | 未执行真实 Agent 任务或所有 TUI、远端设备、SSH 登录/认证和多账号业务 UAT；不声称完整 Orca 兼容 |

恢复工作时桌面及后台已离线，原 PID 55264 和此前终端进程均不在运行；不能把它们记作本次安装“进程保留成功”。此后正常启动了新桌面及原冻结后台。旧 Agent 会话显示“终端后台已退出，历史记录已保留”，本次未自动重建或恢复 Agent；已退出历史可以切换查看，继续执行需用户显式重新启动。

## 固定产物

- 安装包：`artifacts/velella-terminal-switch-20261101/package/Velella Setup 0.2.9.exe`。
- 安装包 SHA256：`ac8a3173afe3769b493c6da50c8668d3c2afde3c621126a393399b1259b6e60a`。
- CLI SHA256：`c6d35fb92e4746a2e3ff5511ed09d5b4ea2109f827c7093a4bcd82290c5cbd99`；Cargo CLI 版本仍为 0.1.0，与桌面 0.2.9 独立。
- runtime SHA256：`8d7b64d01bc825466a3ff7385d875adfff55d1936088b624b03298638e275c44`，未重新编译 backend，也未将其他 dirty Rust 改动发布。
- 包内 24 个 Electron 文件与 0.2.8 比较，仅本轮预期的 main/preload/security 和新增 terminal-switch 文件变化。

第一次 0.2.9 包在独立审阅发现批注问题后已撤出候选，保存在 `rejected-annotation-edge`；不能使用。本文 hash 对应修复后的 `package`，已冻结，后续非源码缺口不重复重建。

## 使用与回退

```powershell
velella-cli terminal list --workspace <工作区ID> --json
velella-cli terminal switch --workspace <工作区ID> --terminal <会话ID> --json
velella-cli skills get velella-cli
```

本机 CLI 位于 `C:\Users\developer\AppData\Local\Programs\Velella CLI\velella-cli.exe`。旧 CLI 备份为同目录 `velella-cli.exe.20261101-085739.bak`。原 0.2.8 桌面目录未覆盖。若回退，只恢复原快捷方式、用户级 CLI 并打开旧 GUI；本轮没有数据库 schema 变更，不应为 GUI 回退覆盖后续数据库数据或终端历史。

证据目录 `artifacts/velella-terminal-switch-20261101/`：`native-switch-split.json`、`native-keyboard-focus.json`、`native-switch-cross-project.json`、`native-menu-guard.json`、`packaged-switch.json`、`packaged-keyboard-focus.json`、`electron-change-audit.json`、`before-desktop-install.json`、`desktop-install.json`、`cli-install.json`、`installed-switch.json`、`installed-verification.json` 及最终 manifest。
