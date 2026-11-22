# 验证记录

## 状态

实施中。本机隔离 setup、拦截 RPC 的浏览器场景，以及一次真实 Electron 原生终端连续性场景已有通过记录；执行主机路由和渲染层目标纪律有主进程/渲染层合同用例。完整功能、真实 CLI、setup 脚本真实执行、daemon 重启恢复和真实 SSH 尚未验证，不构成交付或视觉验收。

## 环境与隔离

- 工作目录：D:/orcas；既有大量未提交改动保持原状。
- 不使用正式 runtime/profile，不停止现有会话，不更改 sshd、系统服务或凭据。
- Rust 测试使用独立临时目录；后续 runtime 测试须显式设置隔离可执行文件和数据根。

## 工作流检查

- `git --no-optional-locks status --short`：退出 0；确认现有修改、删除和未跟踪文件，未清理。
- `node C:/Users/developer/.agents/skills/design-pipeline/scripts/designer-pipeline.cjs next --project-root D:/orcas`：退出 0；返回既有工作台 A 版的 done 记录，visualAcceptance=not-evaluated。该历史记录不证明本次变更已完成，不修改原工作流。

## 自动化

- `cargo test -p velella --lib workbench::creation_tests -- --nocapture`：父级新增用例先退出 101（2 个失败），实现后父级/迁移 4 个通过，退出 0。
- `cargo test -p velella --lib workbench:: -- --nocapture`：最初模块整合缺 native_launch 声明，补齐后 7 个通过，退出 0。
- `cargo test -p velella --lib workbench::creation_tests::migration_backs_up -- --nocapture`：升级前备份用例先失败（无备份目录）；加入 VACUUM INTO 和配置复制后通过，包含未 checkpoint 的 WAL 行及旧 schema。
- `cargo test -p velella --lib desktop::creation_tests -- --nocapture`：项目 patch 的三项先失败（省略 setup 清空、错误类型未拒绝、保存失败污染内存），修复后 3/3 通过；随后创建基准/去重两项先失败，接入日志与共享基准后通过。
- `cargo test -p velella --lib creation -- --nocapture`：39/39 通过，退出 0。包含 15 项日志 SQLite 用例、12 项真实临时 Git 基准用例、普通/受管创建回放及已有相关测试。不证明真实 Agent 或 SSH。
- `cargo test -p velella --lib native_launch -- --nocapture`：9/9 通过，退出 0。
- `cargo test -p velella --lib launch_options -- --nocapture`：3/3 通过，退出 0。检查 descriptor argv 与恢复不重发提示词；没有启动真实 CLI。
- 子任务记录：core TaskSpec YAML 25 项通过；共享 argv 独立 harness 9 项通过；主进程已检查实际改动并重新运行集成用例。
- 编译仍有既有 Windows cfg 下未使用项告警；不做无关清理。后续改动需要重新验证。

## 真实功能与限制

- 2026-10-29：`VELELLA_TEST_RUNTIME=D:/orcas/target/debug/velella-runtime.exe node desktop/tests/worktree-contract.runtime.mjs` 退出 0。证据位于 `artifacts/worktree-contract-1793245251957-1788b350-d742-4bcf-97f9-5b34d75150f7/result.json`；记录可执行文件 SHA-256、独立数据根、两个测试 daemon PID 和持久回执。
- 本机真实 Git/PTY：中文及空格路径、冻结 main OID/实际分支、skip/run/inherit、重复请求不重跑、失败阻断 Agent、审阅后 retry 新 attempt、daemon 重启后原 descriptor/计数保持通过。只停止本轮测试 daemon，没有连接生产 profile 或启动 Agent。
- 真实测试曾发现 PTY child 退出码先于 owner.lock 释放；后端保留严格重试条件，并新增只读 retry_ready 供测试/界面等待原 host 确认结束，不重试不明写入。
- `npm --prefix desktop test`、`npx --prefix desktop tsc -b desktop --pretty false`、`npm --prefix desktop run build`：退出 0；不等同于完整功能验收。
- `desktop/tests/prompt-draft.browser.mjs`：Edge + Vite mock runtime，7 项交互通过。覆盖权限等待不输入、默认草稿一次投递、多会话隔离、显式提交、序号冲突不发送、响应丢失不重发、sending 只读查回执、旧草稿保留；证据 `artifacts/prompt-draft-1793246013680/`。截图为独立组件 fixture，不构成整个工作台视觉验收；已记录 Vite HMR loopback WebSocket 警告（应用断言通过）。
- `desktop/tests/task-sources.browser.mjs`：退出 0，四来源列表/详情/创建关联、手动草稿恢复、分页和安全 Markdown 等 23 项记录通过；全部来源操作为 mock，没有外部发布或账号调用。
- `desktop/tests/workspace-hierarchy.browser.mjs`：真实 Vite/Edge、fail-closed mock runtime，13 项通过；证据 `artifacts/workspace-hierarchy-browser-1793246444203/`。核对项目设置局部更新/null 清除、冻结异步目标、嵌套/搜索/归档父级/跨项目链接、改父级、顶层与子级创建、Escape/键盘、深浅主题及 850px。记录无应用错误、意外请求或 HMR 警告；主进程已查看整页截图。
- `desktop/tests/creation-dialog.browser.mjs`：旧的 18 项记录仅对应客户端启动合同；改接服务端创建后续流程后重新通过 22 项。证据 `artifacts/creation-dialog-workbench-20261024/browser-1793247742488/`，主进程检查测试及深色/窄屏截图。覆盖冻结请求、关闭/重载恢复、仅查询自动发送回执、setup retry_ready、受限提示词阻断、失败/不明启动禁止重启；全部服务端效果为 mock，记录无应用异常。
- 新合同下 `desktop/tests/task-sources.browser.mjs` 24 项通过，证据 `artifacts/task-sources-20261018/browser-1793247779472/`；父级浏览器 13 场景重跑通过，证据 `artifacts/workspace-hierarchy-browser-1793247794579/`。主进程检查新增请求断言，来源测试没有真实账号或外部发布。
- `desktop/tests/prompt-draft.browser.mjs` 新增服务端投递观察/重开恢复后 9 项通过；修正 fixture HTML 和 Vite 端口后证据为 `artifacts/prompt-draft-1793247859904/`。仅有主动中断反馈响应产生的预期 ERR_FAILED，无 HMR 或应用异常；主进程查看独立组件截图。
- `npm --prefix desktop test && npm --prefix desktop run build`：166/166 单测及 tsc/Vite 生产构建通过；仍有既有超大 bundle 提示，不作为 UI 验收。
- 受管 setup owner 重试保护：新增用例先失败，修复原 host confirmed_dead 检查；随后修正 legacy fixture，使其在 owner 锁仍持有时提供实际 IPC 退出状态。定向用例 1/1、普通 native_capabilities 6/6、来源注册恢复 1/1 均通过。
- `cargo test -p velella --lib managed_feedback_ -- --test-threads=1`：7/7 fake-host 用例通过，包括持久化回执、跨任务/内容冲突、权限/人工控制、不明输入、重启和保存中断；旧权限用例 1/1 通过。退出或未知 host 不会被反馈接口重启，submitted 只表示 PTY 输入确认。
- 最新 `cargo test -p velella --lib creation -- --test-threads=1`：88/88 通过；`cargo build -p velella --bin velella-runtime` 退出 0。完整 workspace 测试及真实 CLI 尚未取得通过证据。
- 2026-10-29 服务端续跑真实测试重跑通过：`artifacts/worktree-contract-1793248402297-4b9dc110-3a83-4a66-a4d0-00aa03bb46f0/`。6 个工作区、5 次预期 setup（计数总和 5）、2 个 shell、0 Agent；无客户端请求期间后台继续，重启后没有重复创建会话。前一次失败仅为旧错误文案断言，已改为匹配禁止从普通入口恢复 setup 的现有守卫并核对计数不变。
- 真实创建浏览器测试已加固隔离：`configFile:false`、有限 RPC 白名单、漏网请求拒绝、阻断外部地址、非零 Vite 端口。`artifacts/creation-dialog-20261024/runtime-1793248525419-9FcTCq/result.json` 通过；运行时 SHA-256 `2ed395362c74186fb2e0798fdc778bbabbf694e396bf05845bcd80f779902541`。UI 创建项目和两棵工作树，实际 HEAD/分支/Git common-dir、ready/skipped 日志、零会话均核对；控制台/页面/网络/RPC/清理错误均为空。主进程审阅测试和截图。
- 最新 TaskFeedback 改动后 `npm --prefix desktop test && npm --prefix desktop run build` 再次通过（166 项、生产构建）；不等同于 Electron 或 CLI 验收。
- SSH JS 传输合同：测试先因缺模块失败，实现后 `node --test desktop/tests/ssh-runtime.test.mjs` 13/13 通过。覆盖严格系统 SSH 参数、帧限制、显式身份确认、乱序/重复响应、背压、断线不重放、握手迟到不恢复连接；仅 fake SSH 子进程，尚未接入主机路由或工作台。
- 独立测试 Electron：2026-10-29 首次取得本机原生终端连续性通过记录，见下方“阶段 C 追加证据”。范围仅限普通终端 + setup skip，不含真实 Agent、setup 脚本执行、daemon 重启恢复或 SSH。
- 本机真实 CLI：未运行。
- Windows → Linux SSH：未运行。
- Windows → Windows SSH：未运行。
- 主机断线后持续运行：未验证。
- 所有者视觉验收：未进行。

## 阶段 C 追加证据（2026-10-29，执行主机路由与真实 Electron）

### 自动化

- `cargo test --workspace`：退出 0（384 + 34 + 25 通过，8 项既有 ignored）。此前 `remote_stdio.rs`、`execution_hosts.rs`、`creation_launch.rs`、`creation_setup.rs` 从未参与编译，现已编译并有内联用例。
- 阻塞整个 workspace 测试套件的死锁已修复：`explicit_task_resume_does_not_wait_for_host_under_global_state` 的 fake host 仍按旧的四次连接假设编写，而新的 fail-closed `resume_task` 只查一次 `status` 就因缺少 `native_id` 返回错误。改为记录实际命令序列并断言只有 `status`、没有任何 `kill`/重启，守卫没有被削弱；断言改用 `format!("{error:#}")` 匹配 anyhow 因果链。
- `crates/velella/src/desktop/execution_hosts.rs`：4/4 通过。覆盖身份/远端数据根持久化且不本地规范化、过期审阅拒绝、SSH profile 变化使确认失效、`profiles` 行被外键保留、未来 schema 拒绝读取、迁移前 VACUUM INTO 备份。
- `desktop/tests/execution-hosts.test.mjs`：新增 12/12 通过（主进程路由）。覆盖仅本机命令拒绝、远端命令白名单、注册表只写本机、检查→确认→连接与 revision/fingerprint 复核、profile 漂移即断开、取消确认后重试、结果不确定的远端写不重放、身份不匹配在任何 I/O 前拒绝、断开/销毁、本机透传与远端白名单差异。
- `desktop/tests/execution-target.test.mjs`：新增 5/5 通过（渲染层目标纪律）。覆盖目标形状/冻结、已发出请求不被切换主机改写、远端在开发桥直接拒绝且不回落本机、远端失败不重试、本机桥接仍为 `{request}`。
- `npm --prefix desktop test`：196/196 通过；`npx --prefix desktop tsc -b desktop` 与 `npm --prefix desktop run build` 退出 0。

### 真实浏览器与 Electron

- `desktop/tests/creation-dialog.browser.mjs`（`PLAYWRIGHT_MODULE=D:/orca/node_modules/playwright`，Edge + 隔离 Vite，RPC 全部拦截）：退出 0，24 项通过。加入执行主机芯片、远端隐藏编排/自动化/任务来源、AddProjectDialog/ProjectCreator 远端分支后，本机创建/自动发送仅观察/受限入口/setup 失败与不确定态等既有断言全部保持。
- `desktop/tests/task-sources.browser.mjs`（`NODE_PATH=D:/orca/node_modules`）：退出 0，四来源列表、草稿、创建关联与层级检查通过；无真实账号或外部发布。
- `desktop/tests/worktree-creation.native.mjs` 首次实际运行（真实构建 Electron + 生产沙箱 preload + 自有隔离 daemon/数据根 + 真实原生 PowerShell，`VELELLA_TEST_RUNTIME=D:/orcas/target/debug/velella-runtime.exe`，先 `cargo build -p velella --bin velella-runtime` 刷新，旧 exe 已过期）：退出 0，证据 `artifacts/worktree-creation-native-cWi8HW/result.json`。核对：非法项目名 `..` 被拒且零副作用；UI 创建项目与工作树后 Git 实际 HEAD/分支 `codex/workspace-<id>`/common-dir/干净状态；创建日志 checkout succeeded、setup skipped、launch succeeded、prompt skipped，冻结请求/OID/descriptor 与磁盘 launch.json 一致；会话与创建 ID 不同；宿主 PID 与真正的 shell PID 分别识别且命令行无注入 `-Command`；同一沙箱 preload 的外来 WebContents 被主进程真实 IPC sender 守卫拒绝；关闭窗口仅隐藏；`app.quit` 退出 0 后自有 daemon 仍服务，原 host/shell 进程 PID 与创建时间不变、launch.json 摘要与 mtime 不变；页面异常/网络失败/意外请求为 0；清理后自有进程为空、端点关闭、退出状态有持久回执。
- 该 harness 自身的四处 Windows 假设已修正（不改产品行为）：`spawn` 经 `cmd.exe` 包装，故 Electron 主进程 PID 改为“自有进程或其直接子进程 + 命令行含入口 + 可执行文件 realpath”三重溯源；PowerShell 5.1 单行结果不再是数组，`processes()` 归一化；`getLastWebPreferences()` 不回报 preload 路径，改为按桥接口契约（`invoke`/`listen` 与两条 Unknown 拒绝文案）证明生产 preload；descriptor 目录存在混合分隔符，改为同一文件身份比较而非单一拼写。

### 既有红灯（非本轮改动引入）

- `desktop/tests/workbench-interface.browser.mjs`：失败于 `Timed out: workbench ready`。根因是工作树未提交改动已从 `desktop/src/Workbench.tsx` 删除 `.workspace-info`/`workspace-branch`（`git show HEAD` 仍有，`git diff` 显示为删除），断言引用的是旧界面；本轮按“不恢复已删除旧界面”约束未回滚。另：该文件 Chrome 路径默认写死 `/usr/bin/google-chrome`，Windows 需 `CHROME_BIN`。
- `desktop/tests/automation-interface.browser.mjs`：失败于 `checks.shared_breadcrumb`，断言 `.workbench-breadcrumb` 存在于共享标题栏。该类在 git 全历史与工作区源码中均不存在（同文件其余 `reference_a_*`、主题/宽度、编辑与运行断言全部先通过），属既有未实现设计意图，非本轮回归。两个 harness 需要所有者决定是否按其设计意图实现共享面包屑，或改为其实现有的标题栏，不在本轮擅自改动。
- `desktop/tests/{workspace-hierarchy,task-sources,...}.browser.mjs` 等硬编码 `require('playwright')`：本机 `desktop/node_modules` 无 playwright（不在 package.json 中），需 `NODE_PATH` 或 `PLAYWRIGHT_MODULE` 指向已有安装；本轮未改测试依赖声明。

### 仍未验证

- 真实 Agent CLI（Codex/Claude）启动、model/effort 实际生效与回合观察：未运行。
- setup 脚本在真实 Electron 路径中的执行（本轮仅 skip）：未运行。
- daemon 重启/恢复：未运行。
- Windows → Linux、Windows → Windows 真实 SSH、断线后远端持续运行：阻塞（未获授权启用本机 sshd 或外部主机）。
- 所有者视觉验收：未进行。

## 阶段 D 追加证据（2026-10-29，外围边界与远端界面）

### 边界改动与合同

- 评审检查本机限定：`desktop/electron/main.cjs` 的 `pr_checks` 分支加 `requireLocalTarget(args.target)`；`desktop/src/pr-checks.ts` 在 await 前取目标，非本机直接返回 `remote_unsupported` 且不发 IPC；`PullRequestChecks.tsx` 补中文标题。原因是 `pr-checks.cjs` 用本机 `workbench.snapshot` 解析 workspace 后再启动本机 `git`/`gh`，远端 ID 一旦进入即会读错仓库。
- IPC 清单合同：`desktop/tests/execution-hosts.test.mjs` 新增用例枚举 `main.cjs` 全部 19 个 case，要求 8 个仅本机命令都带 `requireLocalTarget`、`rpc`/`execution_hosts` 都经 `executionHosts`，并断言清单与实际 case 完全相等——新增 IPC 必须显式归类，不能默认放行。
- 嵌入浏览器为机器本地面：`browser-surfaces.ts` 在非本机目标下不再下发 `browser_layout`（并清空上次帧），`Workbench.tsx` 远端隐藏“新建浏览器”、菜单标题降级、`requestedBrowser` 效果短路；主进程按传入目标重复校验。
- Composer 主机标注：`WorkspaceComposer.tsx` 接收 `remote`/`hostName`，“运行于”、执行模式、setup 路径与保留路径显示真实主机与实际路径；`WorkspaceApp.tsx` 在切换执行主机时关闭创建/添加项目/新建项目对话框，避免把已失效主机的表单提交到另一台机器。
- 打包合同既有红灯根因：`desktop/electron-builder.config.cjs`（本轮工作树改动）在 `beforePack` 用 `__dirname` 解析 sidecar，而 `conpty-package.test.mjs` 的 `vm.runInNewContext` 沙箱从未提供该全局，导致 2 项失败。按真实 CommonJS 加载语义修正测试夹具（提供 `__dirname`/`__filename`/`module`/`exports` 与临时 `target/release` sidecar），未回滚或改写用户的配置；另补“缺少 sidecar 时在复制前停止”和“跨平台不得打包宿主构建产物”两条断言。

### 自动化与真实浏览器

- `npm --prefix desktop test`：203/203 通过（新增 1 项打包合同与 3 项终端剪贴板目标用例）。`npx --prefix desktop tsc -b desktop --pretty false`：退出 0。`npm --prefix desktop run build`：退出 0（25.70s，仍为既有超大 bundle 提示，不作为 UI 验收）。
- 新增 `desktop/tests/execution-hosts.browser.mjs`（真实 Edge + 隔离 Vite + 生产形状 mock 桥，非真实 daemon）：退出 0，证据 `artifacts/execution-hosts-20261029/browser/`（`result.json`、`remote-workbench-1440.png`、`local-after-remote-900.png`，页面异常为空）。覆盖：未确认指纹时“信任并连接”禁用、连接后标题栏芯片与页脚远端横幅、远端隐藏 Agent 编排/自动化/任务来源、远端项目替换本机项目列表、远端添加项目为路径输入且无克隆入口、composer 显示“远端执行主机 · 测试主机”和 `/srv/中文 项目/仓库`、检查面板显示远端拒绝且 `pr_checks` IPC 为 0、本机 `velella:workspace` 键保持历史拼写而远端为 `ssh:<host_id>:<server_id>:` 前缀、切回本机后迟到的远端快照被丢弃且“迟到的远端项目”从不出现、本机 PR 面板恢复可读且所有 `pr_checks` 都带本机目标、远端请求全部携带同一 host/server 身份。
- 该 harness 首轮暴露两处仅靠单测发现不了的真实缺陷：composer 在远端仍硬编码“本机”，以及远端工作台仍在驱动本机 `browser_layout`；两处均已修复并纳入上述断言。
- 受影响面回归：`pr-checks.browser.mjs` 1/1、`creation-dialog.browser.mjs` 22 项、`task-sources.browser.mjs` 24 项均退出 0，本机流程未被远端门控破坏。

### 追加：本机限定 IPC 补齐（同日）

- 复核 §7 阶段 D 清单时发现两处“仅 UI 隐藏”并非边界：`desktop_resources`/`resource_manager` 在主进程完全没有 target 校验；`resource_space_scan`/`resource_space_clean` 虽有守卫，但渲染层从不传 target，`checkedTarget(undefined)` 归为本机，守卫实际恒真。现三处渲染层调用（`ResourceStatus.tsx`、`ResourceManager.tsx` 的采样/扫描/清理）统一携带 `currentTarget()`，主进程对 `desktop_resources`、`resource_manager` 补 `requireLocalTarget`，IPC 清单合同把两者纳入仅本机集合（现 10/19）。
- 终端图片粘贴同样不是“远端禁用即可”：`terminal_clipboard` 返回的是本机临时目录里刚写出的 PNG 路径，粘贴以 bracketed 文本送入会话，远端会话拿到的是打不开的本机路径。改为远端只走文本剪贴板通道，本机保持原路径；新增 `desktop/tests/terminal-clipboard-target.test.mjs` 3 项（本机取图并带本机目标、远端零本机剪贴板调用且文本仍粘贴、连续手势逐次重读主机）。该用例先以 `async=>'文本'`（实为形参名为 async 的箭头函数）导致断言失败，修正为 `async()=>…` 后通过——记录以免日后误判为产品缺陷。
- `resource-manager.browser.mjs` 增加断言：扫描与清理请求体必须携带 `{kind:'local'}`，防止再次退化为无 target 调用。
- 同类缺口在其余本机面复现并一并补齐渲染层目标：`project_clone`（AddProjectDialog）、`terminal_performance_export`（TerminalView）、`browser_command` 全部 5 个调用点（BrowserPane、BrowserAnnotationPanel、BrowserAutomationPanel、CapabilitiesPanel 的浏览器能力页、Workbench 关闭标签）。此前主进程虽有 `requireLocalTarget`，但渲染层不传目标时 `checkedTarget(undefined)` 归为本机，守卫恒真。
- 新增合同用例（`execution-hosts.test.mjs` 第 14 项）：扫描上述 12 个源文件，任何 `invoke('本机限定命令')` 调用行必须携带目标；已用去掉 target 的等价行验证该扫描会报错，不是恒真断言。
- 复验：`cargo test --workspace` 退出 0；`cargo clippy --workspace --all-targets` 退出 0（仅 `collaboration/providers/native_policy.rs` 既有 `cloned_ref_to_slice_refs` 告警，与本轮文件无关，未做无关清理）；`npm --prefix desktop test` 204/204；`npx --prefix desktop tsc -b desktop` 退出 0；`npm --prefix desktop run build` 退出 0（23.36s）。真实浏览器回归 `resource-status`、`terminal-paste-order`、`terminal-performance`、`execution-hosts`、`resource-manager`、`creation-dialog`、`pr-checks` 七个 harness 全部退出 0。

### 真实 runtime 复验（Rust 树重建后）

- `cargo test --workspace` 退出 0 后，`target/release/velella-runtime.exe`（15:16 重新构建）再次通过两个真实 harness：
  - `VELELLA_TEST_RUNTIME=… node desktop/tests/creation-dialog.runtime.mjs` 退出 0，证据 `artifacts/creation-dialog-20261024/runtime-1793259145503-1SzTUm`：真实 daemon + 真实 Git，UI 建项目与两棵工作树，RPC 白名单内无漏网请求。
  - `VELELLA_TEST_RUNTIME=… node desktop/tests/worktree-contract.runtime.mjs` 退出 0，证据 `artifacts/worktree-contract-1793259164938-a714ec40-ef34-42af-985c-7a48ac06c459`：6 工作区、7 会话、5 次预期 setup（计数总和 5，无重跑）、2 终端会话、0 Agent、断线 RPC 0。
- 两者仍不含真实 Agent CLI 与任何 SSH；远端持续运行结论不变。

### 与计划的偏差（诚实记录）

- §6 承诺“将现有只读 git/gh 检查移至目标 runtime；缺少 gh 明确不可用”。目标 runtime 至今没有等价的只读 git/gh 命令，本轮未新增，因此远端评审检查改为双端明确拒绝（渲染层 `remote_unsupported` + 主进程 `requireLocalTarget`），而不是拿本机仓库结果冒充远端。这符合“远端失败不得回落到本机”，但该行能力在远端仍未交付。
- §6 的资源监控与垃圾清理：`desktop_resources`、`resource_manager` 与 `resource_space_scan`/`resource_space_clean` 现均携带渲染层目标并在主进程 `requireLocalTarget` 拒绝远端（此前只有 UI 隐藏，且 `resource_space_*` 的守卫因渲染层不传 target 而形同放行）。仍属“仅本机可用 + 远端明确拒绝”，不是“在目标主机可用”。
- §6 的图片粘贴：终端图片粘贴的实际内容是主进程写入本机临时目录后返回的**本机路径**（`terminal-clipboard.ts` → `terminal_clipboard` → `imagePaste`），远端会话无法打开。现改为远端只走文本粘贴通道（`navigator.clipboard.readText`），不再向本机剪贴板取图片路径；本机行为不变。文本粘贴按计划保留。
- 本轮远端界面证据来自 mock 桥（fixture 的 `execution_hosts`/`rpc` 应答），证明的是渲染层与主进程的目标纪律，不证明真实 SSH 行为。

### 仍未验证（阶段 D 更新）

- Windows → Linux、Windows → Windows 真实 SSH、远端添加项目/建树/setup/Agent、断线后远端持续运行：阻塞（未获授权启用本机 sshd、测试密钥或外部主机；远端也未预置兼容 `velella-runtime`）。
- 真实 Agent CLI（Codex/Claude）model/effort 回合观察、真实 setup 脚本经 Electron 执行、daemon 重启恢复：未运行。
- 所有者视觉验收：未进行。上列截图仅为主进程自查证据。
