---
name: Velella current interface baseline
description: 当前桌面界面的视觉规范，记录作用域与继承关系，不发起重新设计。
status: final
created: 2026-11-12
updated: 2026-11-12
sources:
  - docs/design/standards-baseline-20261112/original-DESIGN.md
  - PRODUCT.md
  - MOTION.md
colors:
  terminal-canvas-light: '#f1f1f1'
  terminal-ink-light: '#2e3434'
  terminal-selection-light: '#accef7'
  terminal-canvas-dark: '#282c34'
  terminal-ink-dark: '#abb2bf'
  terminal-selection-dark: '#3e4451'
typography:
  ui:
    fontFamily: "'Geist Variable', 'Microsoft YaHei UI', 'Segoe UI', sans-serif"
  terminal-default:
    fontFamily: Cascadia Mono
    fontSize: 14px
    lineHeight: '1.16'
rounded:
  workspace-card: 5px
  project-action-menu: 10px
spacing:
  project-sidebar-top: 14px
  project-sidebar-inline: 10px
  project-sidebar-bottom: 8px
components:
  TerminalSurface:
    background-light: '{colors.terminal-canvas-light}'
    foreground-light: '{colors.terminal-ink-light}'
    background-dark: '{colors.terminal-canvas-dark}'
    foreground-dark: '{colors.terminal-ink-dark}'
  ProjectNavigation:
    font: '{typography.ui.fontFamily}'
    background: var(--worktree-sidebar)
    foreground: var(--project-nav-ink)
---

# Velella — DESIGN

## Brand & Style

保留当前 Velella 名称、三鱼标识和紧凑桌面工作台。任务状态、恢复入口与可读性优先；用户文案使用简体中文。本轮仅整理当前界面，不把旧截图或历史包重新设为全局视觉目标。

本规范采用用户于 2026-11-12 确认的“保留当前界面，先统一规范”基线。原文快照、规则去向与历史冲突见 [输入对照](docs/design/standards-baseline-20261112/reconciliation.md)。涉及交互、状态、设置或会话恢复时，继续读取同目录 EXPERIENCE.md；两份规范优先于后续 mock 或导入参考，既有授权和项目约束继续适用。

## Colors

现有内部 UI 系统是 `desktop/src/ui.tsx` 及分层样式，不改用新的组件库。未在 frontmatter 列出的 UI 颜色继承现有语义变量；不为了凑齐色板，把 OKLCH 值任意换成新的十六进制颜色。

| 作用域 | 当前颜色来源 | 使用边界 |
| --- | --- | --- |
| 基础 UI | `theme.css` 的 foreground、muted-foreground、border、danger、status-success 等语义变量 | 错误、成功、焦点分别表达真实含义；不靠颜色单独传达状态 |
| 首页 | `ProjectHome.tsx` 与 `project-home.css`，继承全局外壳变量 | 项目导航的局部规则不得外推为首页布局 |
| 项目外壳 | `project-shell.css` 的 `.project-shell` 覆盖 | 决定工作台外壳，不决定终端或编辑器的内部配色 |
| 项目导航 | `project-orca.css` 的项目页作用域与 `--worktree-sidebar`、`--project-nav-*` | 保留当前明暗主题；不照搬旧文档中的固定侧栏色 |
| 终端 | `surface-themes.ts` → `TerminalView` 的 xterm theme 与行内变量 | 画布使用本文件 terminal tokens；保留完整 ANSI 语义和原生 CLI 内容 |
| 设置 | `AppSettings.tsx` 及其专用样式 | 使用现有完整设置页；不能把浅色设置截图的白底要求套到暗色主题 |

终端浅色画布/文字为 `{colors.terminal-canvas-light}` / `{colors.terminal-ink-light}`；暗色为 `{colors.terminal-canvas-dark}` / `{colors.terminal-ink-dark}`。选区分别使用 `{colors.terminal-selection-light}` 和 `{colors.terminal-selection-dark}`。

`theme.css` 中仍存在海雾默认变量；它不是当前终端的最终取色入口。终端文字、选区、ANSI 背景及编辑器配色分别保留各自语义，不清洗原始输出，不把所有 ANSI 背景抹成画布色。

普通界面文字的可读性目标为 WCAG AA：常规文字 4.5:1、大字 3:1；这是验证目标，不代表全界面已经通过对比度或视觉验收。

## Typography

UI 与当前项目导航继承 `{typography.ui.fontFamily}`。项目导航的旧 Georgia/SimSun 衬线描述保留为历史差异，不在本轮恢复。

终端默认值为 `{typography.terminal-default.fontFamily}`、`{typography.terminal-default.fontSize}`、行高 `{typography.terminal-default.lineHeight}`；用户保存的偏好和现有 fallback 栈优先，规范整理不覆盖它们。表单标签可读，长路径换行或在现有控件内呈现，不把弹窗撑宽。

## Layout & Spacing

| 表面 | 当前布局约定 | 不可外推的规则 |
| --- | --- | --- |
| ProjectHome | 保留独立首页，打开已有项目和开始新项目都能直接找到 | 不因项目侧栏参考而改写首页 |
| ProjectNavigation | 侧栏保留独立宽度偏好，既有默认 280px、范围 220–500px；当前内边距为 top/inline/bottom spacing tokens | 旧文件夹行、卡片和状态槽像素不作为新全局密度标准 |
| TerminalSurface | 使用现有终端、标签、分屏和状态区；界面布局与原始字节记录分开 | 视觉变化不得触发进程重启、历史重写或输入行为变化 |
| SettingsPage | 完整设置页、分类导航、返回入口及既有底部操作 | 不改回覆盖项目的模态设置；保留当前工作区和会话 |
| Dialog | 复用现有 NativeDialog；body 独立滚动，底部操作可达，窄宽度下表单行按现有规则排列 | 不新增一套弹窗布局库 |

这里只固化有明确作用域的值，不创建覆盖所有页面的新间距刻度。当前源码布局的存在不等于大小窗口和 DPI 已回归通过。

## Elevation & Depth

沿用现有 popover、菜单、dialog 的语义表面、边框与阴影。状态和恢复操作不依赖装饰层次；不为设置控件引入新的装饰动效或依赖。具体动态规则继续由根目录 MOTION.md 约束。

## Shapes

项目工作区卡片继承 `{rounded.workspace-card}`，项目操作菜单继承 `{rounded.project-action-menu}`。它们是局部值，不覆盖现有按钮、输入框或 dialog 的圆角。

## Components

| 组件名 | 视觉规则 | 来源 / 作用域 |
| --- | --- | --- |
| ProjectHome | 保留品牌、开始入口、已有项目卡片及真实项目内容 | ProjectHome.tsx、project-home.css |
| ProjectNavigation | UI 字体、现有状态槽/主机标记/选中反馈；选中与禁用状态可区分 | ProjectSidebar.tsx、project-orca.css |
| TerminalSurface | 本文件 terminal tokens、用户字体、原生 ANSI 内容；状态区不覆盖正文；已退出且没有可展示工作记录时隐藏空条 | TerminalView.tsx、surface-themes.ts |
| SettingsPage | 现有完整设置页与分类列表，保留未来启动设置与现有会话的区分 | AppSettings.tsx、TerminalSettings.tsx |
| StatusNotice | 简短中文状态文本；错误/未知/只读/恢复中视觉上和文案上可区分 | 现有加载、错误和终端状态区 |
| ActionMenu | 明确当前项目/会话作用域，保留 hover、focus、disabled 状态；项目菜单当前宽 224px、圆角为 project-action-menu token | 现有 Radix 菜单和 project-orca.css |
| FormControl | 复用 ui.tsx Button、Input、Textarea 与现有小型 native select；标签和错误可见 | ui.tsx、现有表单样式 |
| Dialog | 沿用 NativeDialog 及现有背景、边框、尺寸约束；长路径和滚动不遮挡操作 | ui.tsx 与主题样式 |

## Do's and Don'ts

| 保留 / 应做 | 不做 |
| --- | --- |
| 保留数据、布局、原始历史、运行进程、账号目录和用户偏好 | 用样式整理名义重建会话、迁移 profile 或替换安装版 |
| 明确权限允许什么，隔离不可用时保持真实状态 | 静默降级隔离，或把 Git worktree 称作安全边界 |
| 保留明暗主题、键盘操作和 focus-visible | 用单张截图证明所有主题及交互已通过 |
| 沿用当前组件与已批准的动效边界 | 为设置控件引入新视觉主题、装饰动画或依赖 |
| 区分当前源码值、历史参考和用户保存偏好 | 将 D:/orcas 历史路径、旧包哈希、截图或候选预览当作当前运行态证据 |
