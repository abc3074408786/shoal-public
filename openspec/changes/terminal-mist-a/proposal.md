# 鱼群终端 A「海雾」落地

用户于本轮明确选择预览 A。引用 artifacts/velella-terminal-identity-20261025/A-mist.png、preview.css、preview.js 的 mist 方向；不再沿用 Orca 默认视觉，也不将 B/C 纳入正式应用。

用户最终可感知结果：现有唯一前端里，灰绿色终端与会话标签形成一个完整工作面，薄荷标识区分当前会话；原生 Claude/Codex 的内容、ANSI 配色、输入和历史正常保留。默认 Cascadia Mono 14、常规字重 400/600、行高 1.16、16px 内边距；用户已有显式外观偏好保留，安装后核验实际设置。如当前存有旧行高 1，因本次用户已选择 A，可通过设置界面仅将本次行高改至 1.16，不进行静默通用迁移。

总体验收：灰绿 #232d2a，正文 #dfe8e3，薄荷光标/选区；整合式标签、8px 工作面圆角及 16px 留白；浅色遵循现有主题可读性；单窗口/分屏、其他 tab、搜索/设置/工作记录不丢失或遮盖；唯一前端与已有 A 工作台保持。候选包验证、独立只读验收、正式安装、真实窗口检查及数据保留均完成后才能报告本轮交付完成。用户选择预览不等于真实桌面的后续业务 UAT。

当前关键路径：定位标签和分屏结构 → 最小源代码改动并保留 preimage → 基于真实组件浏览器验证及构建 → 固定包/源码哈希 → 独立核对 → renderer-only 安装 → 原生窗口与会话保留核验。

边界：不改 native CLI 输出、PTY/ANSI、终端宿主、daemon、Electron 主进程、账号/模型/权限或生产数据库；只重开 GUI/bridge。安装前冻结候选，通过后仅新缺陷允许重开。使用现有受保护安装器，不安装预览服务器，不将示例数据带入生产。

基线：app.asar cf344d2297b5060ca634b6c2304fc4b20ae7b14b2e111333b0d0649b96821c2f，证据 artifacts/orca-terminal-appearance-20261025。

本轮证据目录 artifacts/terminal-mist-a-20261025。技术验收预先约定：真实 TerminalView 的灰底/留白/字体与图层，Workbench 单窗/分屏标签操作、原生回放/input queue/resize 基础回归；已安装非 renderer 文件及 runtime 哈希不变；截图来自正式进程。长期业务和真实系统 IME 不由合成测试代替。
