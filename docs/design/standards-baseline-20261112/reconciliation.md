# 输入对照与历史冲突

本轮用户决定：保留当前界面，先统一规范及适用范围；首条路径为“打开已有项目，继续 Agent 终端任务”。下表记录候选如何使用既有输入，不删除原文，不修改 OpenSpec，不推导新的产品功能。

## 输入去向

| 原输入 | 保留内容 | 候选去向 / 处理 |
| --- | --- | --- |
| 根 DESIGN：Product context、Overview | 本地桌面工作台、明确执行范围、紧凑界面、状态/恢复优先、简体中文 | DESIGN.Brand & Style；EXPERIENCE.Foundation、Voice and Tone |
| 根 DESIGN：Colors | 语义色、明暗主题、错误/成功/焦点分开 | DESIGN.Colors；补充分层作用域，不改源码色值 |
| 根 DESIGN：Typography | 继承现有字体、表单可读、路径不撑宽弹窗 | DESIGN.Typography；终端保存偏好继续优先 |
| 根 DESIGN：Layout | 复用现有布局、弹窗内容滚动、操作可达、窄宽度表单适配 | DESIGN.Layout & Spacing；EXPERIENCE.Responsive & Platform |
| 根 DESIGN：Components | Button、Input、Textarea、NativeDialog、小型 native select、可见标签、role=alert/status、键盘焦点 | 两份文档的同名组件表及 Accessibility Floor |
| 根 DESIGN：Do's and don'ts | 权限明确、不静默降级、不把 worktree 当安全边界、不加设置装饰动效或依赖 | DESIGN.Do's and Don'ts；EXPERIENCE.Interaction Primitives |
| 根 DESIGN：Source decisions | 沿用已有语义系统与组件，不重新设计 | DESIGN.Brand & Style、Colors；原文保留在根文件与 imports 快照 |
| 根 DESIGN：Desktop light workbench | 终端浅色参考、完整 Tango ANSI、输入/选区层次、仅展示适配、保留历史和暗色、退出后不留空工作记录条、用户终端字体不变 | DESIGN.Colors、Typography、TerminalSurface；由现有实现继续控制，不扩展为全局导航或设置布局 |
| 根 DESIGN：Agent settings 段落 | 整页设置、分类导航、返回、底部操作、项目保持、状态真实、现有会话可达 | SettingsPage 组件规则；细化“未来启动设置”和“即时显示偏好”的边界，见冲突 C4 |
| 根 DESIGN：Orca project navigation | 首页独立、侧栏范围与用户偏好、状态/主机归属、focus/disabled、终端历史/输入/进程/布局保留 | DESIGN.Layout & Spacing、组件表；旧字体/颜色/菜单尺寸作为有出处的历史差异，见 C2/C3 |
| PRODUCT | Velella 名称和三鱼、现有与新项目都重要、真实数据、简体中文、本机桌面场景、保留项目状态 | 两份文档 Foundation / Brand / ProjectHome；不把 web 单字误当独立网站产品需求 |
| MOTION | 静态设置、已有 CSS 过渡、减少动态、权限/警告立即显示、不加动画库 | DESIGN.Elevation & Depth；EXPERIENCE.Accessibility Floor；根 MOTION 不改 |
| AGENTS | 目标/路径/验收、既有 OpenSpec、授权、分层验证、数据与进程保留 | 候选的边界与验收表；全局治理不复制成另一套规则 |
| 架构研究 | React/Electron/Rust 责任分离、目标主机、会话归属、原始记录与显示偏好分开 | EXPERIENCE.Foundation、Component/State Patterns、Flow 1；研究只提供源码事实，不授予设计验收 |
| 旧单一界面决定 | 唯一前端、原生 CLI、数据保留、重复终端标题已移除、历史方向与包记录 | 保留行为约束；历史方向按用户本轮“当前界面”决定分层解释，不恢复旧版或改安装程序 |

原文件和原文没有因候选整理被删减。新候选压缩的是重复叙述，不代表原有定量像素被永久废弃；其历史原文仍可追溯，采纳时须保留此对照。

## 冲突与解释

| ID | 已核查差异 | 本候选如何处理 | 仍未证明的部分 |
| --- | --- | --- | --- |
| C1 终端颜色 | theme.css:37 默认 #232d2a；surface-themes.ts:16 与 TerminalView.tsx:100/270 选择 #282c34 | 终端专用 tokens 按当前组件源码；基础 UI token 不伪称全局已删除 | 当前安装版窗口像素与完整视觉验收 |
| C2 项目导航字体/侧栏 | 根 DESIGN 旧段落 Georgia/SimSun 与固定侧栏色；project-orca.css:3–12 当前使用 var(--font-sans) 与局部 OKLCH 变量 | 当前导航继承现有 UI 字体与局部变量，旧要求保留为历史差异 | 实际窗口字体 fallback、DPI 与视觉观感 |
| C3 菜单与卡片几何 | 根 DESIGN 旧段落菜单 208px/11px；project-orca.css:103 当前 224px/10px，卡片有局部 5px | 只记录项目菜单/卡片局部值，不外推全局菜单或新密度系统 | 所有菜单、窗口宽度与键盘操作的运行回归 |
| C4 设置生效范围 | 根 DESIGN 写“Settings only affect future launches”；TerminalView.tsx:86 监听显示偏好变化，terminal-preferences.ts:37–40 广播变更 | Agent 启动参数与终端显示偏好分开，不把“只影响未来”泛化到所有设置 | 各 Agent、全部设置项的实际生效时间 |
| C5 当前与历史 | 旧决定含 D:/orcas、多个“当前包”哈希及海雾/Orca方向迭代 | 作为历史，不作为当前源码位置、安装状态或重新设计授权 | 哪个安装包正在运行，本轮未检查 |
| C6 已存在与已验收 | 源码有组件、状态分支与测试脚本 | 文档只写对应事实；验收清单保持待执行 | 产品测试、独立功能测试、技术验收、集成、部署、在线与业务 UAT |

## 现状证据索引

- 首页与导航：desktop/src/ProjectHome.tsx:24–31；WorkspaceApp.tsx:148–152、180、299。
- 分层样式：desktop/src/theme.css:4–75；project-shell.css:2–13；project-orca.css:3–15、68–76、103。
- 终端：desktop/src/surface-themes.ts:9–17；terminal-preferences.ts:11–16；TerminalView.tsx:70–86、100、270–283。
- 设置：desktop/src/AppSettings.tsx:64–79；具体设置项内部交互未作全量运行核验。
- 完整架构出处与固定快照：[研究报告](../../../_bmad-output/research-velella-architecture/research-velella-architecture.md)。

## 当前产物状态

两份规范已接入根目录 DESIGN.md 与 EXPERIENCE.md；本目录保留冻结记录，后续只维护根目录。当前可做：按页面找到视觉/交互规则，并用主路径验收表描述下一次修改。用户已选择运行一次 rubric 覆盖核验，四个表面均只用规范表达；不生成新 mock。真实界面验收和业务代码实现尚未开展。
