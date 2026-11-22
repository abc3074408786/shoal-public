<!-- bmad:context -->
<!-- Verified 2026-11-12 against HEAD 3cdb9a40b6c289b7a3836ef39cc28eb70141f235 and the current uncommitted working tree. Managed by bmad-project-context. Preserve additional instructions outside these markers. -->

# Velella 项目上下文

Velella 是运行原生 Agent CLI 的本地桌面工作台。
界面使用 React/TypeScript，Electron 提供桌面桥接，Rust 提供运行时。
需求与变更依据保留在 openspec/changes/；设计依据见 PRODUCT.md、DESIGN.md 和 MOTION.md。

## 项目规则

- 保留已有 OpenSpec 和项目约束；BMAD 不替代交付负责人、授权或验收规则。
- 修改前写清用户最终可感知结果、使用路径、验收标准和本次范围。
- 需求有歧义时，先列出已知事实、缺失信息和建议解释；影响行为或验收的歧义须确认后再实现。
- 沿用对应的 OpenSpec 变更，不另建相互竞争的需求或交付计划。
- 保留已有项目数据、工作区布局、终端历史、CLI 设置和运行进程；会改变这些状态的操作须在任务范围中明确。
- 区分实现、开发验证、独立测试、技术验收、集成、部署、当前在线和业务 UAT；总目标未达到时明确报告未完成。

## 职责与入口

- 前端入口：desktop/src/main.tsx → WorkspaceApp.tsx；界面与交互在 desktop/src/。
- 桌面请求入口：desktop/src/api.ts；修改接口时沿 desktop-platform、Electron preload/main 和 Rust 分发链核对参数、返回值与错误行为。
- Electron 桥接：desktop/electron/preload.cjs、main.cjs；保留命令白名单和调用方校验。
- 主机路由：desktop/electron/execution-hosts.cjs；远端请求不得静默回退至本机。
- Rust 桌面服务：crates/velella/src/desktop.rs；请求分发入口为 desktop/terminal_io.rs，编排与协作继续进入各自分发模块。
- 桌面 CLI：crates/velella/src/desktop_cli.rs；命令语义见 docs/velella-cli.md。
- 修改前说明状态由哪一层持有、请求经过哪些边界、哪些接口需同步调整；不得只修改界面就宣称完整功能已实现。

## 运行与验证

- 桌面 daemon 与旧 headless daemon 分开核验；velella-cli 控制桌面服务，旧 velella task/session 命令不能替代桌面验证。
- 常规 CI 的 Rust workspace 检查不覆盖桌面前端、Electron 或独立 Tauri workspace。
- desktop 的 npm test 只匹配 tests/*.test.mjs；浏览器与原生脚本须按所改用户路径单独选择。
- 运行浏览器或原生脚本前读取其入口，确认浏览器依赖、运行时产物和数据目录。
- Vite 开发预览可以连接真实本地 daemon；需要隔离验证时使用已确认的 fixture 配置和独立数据目录。
- 按当前 package.json 和 electron-builder.config.cjs 构建桌面包；验证对应 Rust 二进制和打包资源，不能用前端 build 代替安装包验收。

## 设计与实现约定

- 涉及界面时先读取 PRODUCT.md、DESIGN.md、MOTION.md 和对应 OpenSpec；发现冲突时指出具体条目并确认适用范围。
- 实现前明确页面目标、主操作、信息层级，以及加载、空态、错误、禁用和恢复状态。
- 复用 desktop/src/ui.tsx 的组件和 theme.css 的语义变量；用户文案使用简体中文，未知或未验证状态不得伪装成成功。
- 终端原始字节记录与展示帧分开处理；terminal-frames.ts 的视图副本不得替代或改写原始日志。
- 界面调整不得改变原生 CLI 内容、终端输入、历史或用户保存的字体与布局偏好。

<!-- /bmad:context -->
