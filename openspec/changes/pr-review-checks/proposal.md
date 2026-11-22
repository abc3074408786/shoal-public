# 评审检查

## 用户结果与验收

鱼群工作区右侧工具栏新增「检查」，进入独立「评审检查」页。用户能查看当前分支关联的 GitHub PR、当前 head SHA、代码评审状态和自动检查，手动刷新，并打开 PR/检查详情。保留海雾 A 界面和 Claude/Codex 原生终端。

总体验收：安装后的桌面应用提供可用入口；非 Git、无远程、未关联 PR、无检查、未登录和网络错误均有明确状态；实际 GitHub 数据经只读查询验证；安装前后的终端进程、会话、数据与账号配置保留。业务私有 PR 的 UAT 单独记录，不用公共测试样例代替。

## 范围和接口

- 首版支持 github.com，复用现有 GitHub CLI 登录。使用 origin 作为源仓库，存在 upstream 时查询其 PR，并过滤同名分支的无关 fork。
- 新增受现有主窗口/主框架校验保护的 Electron IPC `pr_checks`。工作区路径仅从运行时快照取得，不接受 renderer 路径或任意命令。
- 固定 Git/gh 只读命令，禁用 shell 和交互提示，限制执行时间、输出体积和并发。错误信息不透出命令 stderr 或凭证。
- 进行中的检查仅在面板挂载且页面可见时每 30 秒刷新。切换工作区后忽略旧响应。
- 不创建/修改 PR，不重跑 CI，不批准评审，不推断必需检查或可合并性。GitHub Enterprise/GitLab 不在本次范围。
- 不修改 Rust runtime；更新 Electron 应用代码与 renderer，只重开 GUI/bridge，保留 daemon/terminal host/CLI。

## 固定验证合同

开发验证：生产构建、pr-checks 单元测试、Electron IPC 安全回归、实际工作区入口的浏览器测试（状态、链接、切换工作区、1440/900 布局），以及真实 Git/gh 公共 PR 只读查询。

独立核验：只读接口/命令边界、状态映射、异步响应隔离、相对已安装基线的包差异与来源。只允许 `electron/main.cjs`、`preload.cjs`、`security.cjs` 和新增 `pr-checks.cjs` 发生非 renderer 差异；保留现有 candidate wrapper、Electron 可执行文件和 Rust runtime。

安装验证：以准备时新鲜快照检查受保护进程身份、会话 ID、数据表、历史内容前缀、账号/配置文件。原生界面打开「检查」、确认当前非 Git 工作区的说明、执行刷新并再次核对资源保留情况。

证据目录：`artifacts/pr-review-checks-20261025/`。独立核验通过后固定候选，不因部署或 UAT 缺口反复修改已通过代码。
