---
title: Computer Use 插件接入
type: feature
created: '2026-11-17'
status: built
baseline_revision: '2fa5f5d705110db7fb1e7effca8d6692888ce959'
route: full
route_source: auto
risk: medium
review: quick
review_source: pinned
lenses_ran: [quick]
context: []
---

## Intent

用户已明确要求“接入”插件。用户应能从「插件 → 发现」找到 Computer Use，免填路径生成本机 MCP 目录，逐项确认观察或控制工具；添加后在「已添加」管理，鱼群通过既有选择器重新确认后使用。沿用本 OpenSpec change，不重开已验收的 MCP 服务。

本次只做源码接入和隔离验证；不安装桌面应用、不变更实时插件清单或角色、不调用业务模型、不执行真实鼠标键盘输入。主工作区脏且有并行工作，先在独立 candidate 改动，最终仅在基线哈希未变时合入本次文件，保留 index、用户数据、偏好和进程。

## Code Map / State Ownership

- `desktop/src/PluginsPage.tsx`：复用插件发现/个人清单，添加一个本地 MCP 入口，不注册成错误的本体页面插件。
- `desktop/src/ComputerUsePlugin.tsx`：显示一个服务入口、加载/失败/重试状态；观察与控制条目继续由既有 picker 区分。
- `desktop/src/McpCatalogPicker.tsx`：只增加初始目录路径；保留空工具选择、显式确认、重新预览。
- `desktop/electron/extensions.cjs` → 新的 `computer-use-mcp.cjs`：沿既有 `desktop_extensions` IPC 白名单和本机校验，仅显式 prepare 操作生成目录，不启动 MCP server。
- `tools/computer-use/src/velella_computer_use/catalog.py`：运行已冻结的目录生成器，目录 schema 与工具列表不在 JS 中重新定义。
- Electron profile 的 `extensions/computer-use` 持有生成目录与打包版 venv；renderer 的 `plugin-library.ts` 仍只存主机隔离的来源引用。Rust 仍持有 preview/digest/confirmation；编排仍自行复核。
- `desktop/electron-builder.config.cjs`：把服务源文件、锁定依赖和 README 作为 Windows extraResources；不捆绑开发机 `.venv`、artifact 路径或自动下载环境。

## Tasks & Acceptance

- [x] 本机桥接：解析源码版服务根与已准备 venv，或打包资源与 profile 内 venv；拒绝非 Windows、远端和未知插件；缺失环境返回具体说明；目录在 profile 内原子发布，同内容不改写。
- [x] 插件入口：复用现有样式，简体中文文案；添加动作只打开预填路径的导入弹窗；既有清单启停、移除与角色复核不变。
- [x] 打包资源与准备说明：包含完整运行必需 Python 文件和锁，明确尚未安装/部署；缺少环境不假装可用。
- [x] 验证所有边界、实际目录生成与真实 Rust preview/confirm；浏览器合成桥验证完整插件管理和角色选择路径；TypeScript/Vite build；检查 frozen MCP 源码哈希未变。

Given 本机 Windows 环境已准备，When 从插件发现点击添加并载入预填目录，Then 默认条目是只读观察，所有工具初始未选，只有显式确认后才写个人清单；不会启动控制服务。

Given 显式选择控制条目，When 确认所选工具并在鱼群选择器重新导入，Then 只保留来源引用、重新预览且清空工具选择，已有 Rust 来源门禁继续生效。

Given 已添加条目，When 停用/启用/移除，Then 后续角色选用随之变化，已有会话配置和服务源码保留。

Given 缺失环境、生成器失败、非 Windows 或远端，When 请求接入，Then 明确报错、恢复按钮可用，不执行桌面输入、不写角色配置、不回退到其他主机。

Given 打包资源布局，When 指定 profile 中准备好的 venv，Then 生成目录使用该实际解释器，不依赖开发机路径；环境准备需显式执行文档命令。

## Implementation Notes

遵循用户规则，源码实现和最终验证由主代理负责，不派编码代理。BMAD 的脏检出暂停与编码代理默认步骤由已授权的隔离接入方案及用户职责规则覆盖；不更改原 frozen block，不暂存主工作区。计划审批沿用用户本轮“接入”的实施授权，不新增许可循环。

## Verification

- `node --test tests/computer-use-plugin.test.mjs tests/plugin-management.test.mjs`：真实目录生成、稳定发布、隔离 profile、IPC 限制、环境缺失与错误恢复、现有插件回归。
- `node tests/computer-use-plugin.browser.mjs`：无真实 daemon 的合成桥；发现→预览→观察/控制选择→来源确认→已添加管理→角色重新确认、失败恢复、远端禁用；浅深色与窄窗口截图。
- `npm run build`：TypeScript 与 Vite。
- 冻结服务 19 文件及 runtime/CLI 哈希核对；实际目录通过固定 runtime 的隔离 daemon preview/confirm，不重新跑原生输入或业务模型。

## Review Triage Log

2026-11-17：独立只读 Quick 复核返回 partial，两项发现均单独裁决。

| 发现 | 裁决 / 处理 | 证据 |
| --- | --- | --- |
| 仅有解释器、依赖未安装仍可发布目录 | medium / patch | 复核属实。新增隔离 Python 依赖导入及 server 模块路径定位，检查失败不发布目录，不导入或启动 server；解释器空环境失败、准备后成功的回归通过。 |
| 尚无实际安装包资源证据 | 验证缺口已补齐，非代码缺陷 | 保留来源确认和已通过源码；使用原 beforePack 门禁及固定 runtime/CLI 生成未安装的 win-unpacked，13 项实际 asar/资源哈希核对通过，未捆绑 .venv。不是全应用包 GUI/UAT。 |


## Plan Change Log

## Delivery

实现、开发验证、独立测试、技术验收、集成、部署、当前在线、业务 UAT 分别记录；新的插件接入不能冒用旧服务的独立验收结论。

### 本次执行结果

12 项定向检查通过（含真实隔离 daemon 目录 preview/confirm、运行环境不完整拒绝）；7 组隔离浏览器场景通过；TS/Vite 构建通过；electron-builder --dir 通过，实际包 13 项资源与源码/固定 runtime 哈希匹配。截图已查看，浅深色和 1600/900/480 窗口无页面水平溢出。

首轮测试的 Node 退出信号断言错误、浏览器缺失 Playwright 均保留原始日志并分别基于明确新证据修正前置条件，未改变产品期望。Quick 复核后只修复运行环境缺口，冻结 MCP 服务未变。插件接线没有独立执行测试聊天；本线程测试属于开发验证，不能冒称独立测试。没有安装/启动包、启用真实角色、业务模型或原生输入。未创建提交，保留主工作区既有 index 和其他改动。
