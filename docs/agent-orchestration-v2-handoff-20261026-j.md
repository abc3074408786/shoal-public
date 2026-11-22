# 编排 v2 续接 J：OMP 接入和四 CLI 新建范围，继续完整验收

整体未完成、未安装。源主会话按用户约 40% 上下文规则交接；创建接任聊天后停止源码修改。接任者亲读本文、I、G；本文覆盖 I 的“OMP 尚未实现”，I 覆盖 H 的 OpenCode 未闭合问题。不要重载 A–F 或重跑已通过检查。

## 结果、授权、基线

用户已明确本次仅 **Oh My Pi (OMP)、Claude Code、Codex、OpenCode**，确认 OMP 身份后说“可以”；Kimi/Gemini 保留历史读取，不阻塞当前交付。

父目标：自然语言 Git 研发目标 → 可编辑角色/步骤/依赖/验收 → 人工确认 → 隔离工作树并行 → 独立审查固定候选 → 集成 → 独立总体验证 → 可运行可审阅分支/工作区。四 CLI 各有启动、领取、回报、审查、恢复；至少两 CLI 两个实现角色混合集成；完整独立验收后已有授权受控安装。不能交小增量后等用户继续。

禁止自动 trust/login/更新、全局账户改动、重启生产、PR/发布。安装须固定候选、备份回滚和保护生产会话。用户授权来源见 I/G。本轮未启动真实账户模型、没有生产修改。

工作区 D:/orcas，分支 codex/integrate-collaboration-20261022，HEAD 62769b4a8ea0ec4c7f347c16726888cbaf3db64a 未变；大量原有 dirty/untracked/deleted 保留。主代理独占修改/取舍/最终验证；探子只读单轮、fork_turns=none、不得递归/复用、派出立即等待。没有 FastCtx 工具，沿用 rg/Get-Content。不要声称改变不可控模型/Fast。本轮无遗留自有 CLI、测试进程或在途探子。

## 本轮实现

新增 `crates/velella/src/collaboration/providers/omp_policy.rs`、`omp_lifecycle.mjs`、`scripts/fixtures/omp-role-lifecycle.cjs`。

修改 providers.rs、providers/skill_policy.rs、collaboration/orchestration/v2.rs、desktop/workbench_commands.rs、desktop/collaboration_commands.rs、desktop/src/OrchestrationV2.tsx，以及 v2 browser fixture/test。完整 SHA 指纹在 `artifacts/orchestration-v2-20261025/continuation-20261026-omp-context.json`。

- 新建 capabilities/主控只列 codex、claude-code、omp、opencode。新方案不得引入 Kimi/Gemini；已有历史角色可保留、阅读或迁移为当前 provider，不能通过 plan save/revision 引入新的历史 provider。历史 selector 有保留 option。模板若仍含历史角色，创建时明确拒绝，不静默迁移。v1 不变。
- 桌面 workspace_launch 与 prepare_native_member 接入 omp，再由 provider helper 生成私有角色配置。
- role/omp-home 是 owner-only 目录，内有 .omp/agent。显式覆盖 HOME/USERPROFILE、OMP_PROFILE/PI_PROFILE=default、PI_CONFIG_DIR、PI_CODING_AGENT_DIR、PI_CONFIG_FILES、session-dir。`--profile default`、CLI overlay 固定优先级。Windows 本机路径已实现；其他 OS 认证源路径明确报未验证。
- MCP 保留 native 来源；overlay disabledProviders 关闭所有已知非 native 来源，`mcp.enableProjectConfig=false` 在去重前排项目。user mcp.json 只有选定的生产 proxy 和 velella 控制服务器。
- **不能禁 native**：mcp/config.ts 直接读 mcp.json 只取 disabled/forced 名单，servers 来自 loadCapability。定向探子漏列了 vscode/windsurf；主代理已亲核官方源码确认二者也注册 MCP，实际禁用名单包含它们。
- 原生 custom tools 从 .omp/tools 加载、plugin registry 另有独立加载入口，不受 MCP/--no-extensions 全部控制。启动/恢复扫描 cwd 向上各级 .omp/tools、.omp/plugins，存在即明确拒绝；不删除、不迁移项目目录。私有 home 也拒绝额外 .mcp.json/tools/plugins。这是已知配置入口约束，非恶意进程文件系统沙箱。
- --no-skills/--no-extensions/--no-rules/--no-lsp/--no-title/--no-prewalk，显式 -e lifecycle；--service-tier none；--approval-mode always-ask。选定 Skill 完整快照正文追加到系统提示，资源/正文摘要沿现有机制恢复核对。没有默认 yolo。
- 认证源按当前环境 default/named profile 求 agent.db；不支持远程 broker 环境时明确拒绝，不自动切换账户。SQLite 源以 READ_ONLY + query_only + 事务读取 WAL 当前记录，仅复制 active auth_credentials 和对应 auth_credential_blocks，保留限额阻塞、不复制 settings/usage/cache/refresh leases。固定字段、JSON/数量/大小检查；先保护私有目录再复制；ready/fingerprint 防止复用半成品。没有原库写回。真实账户/所有 provider 认证依赖仍未验证；自定义模型/provider 配置未继承，不能声称所有账户组合可用。
- lifecycle：session_start→SessionStart；input→UserPromptSubmit；tool_approval_requested→PermissionRequest；agent_end→Stop；session_shutdown→Interrupt。input 收到 runtime/IPC 拒绝返回 OMP 原生 handled:true，不依赖 before_agent_start 抛异常。每次输入 runtime 复核正文/资源。shutdown子进程预算1.5秒，其余10秒。
- getAllTools SourceInfo.source 区分 builtin/mcp/extension。只启用角色 builtin 和所选 MCP；tool_call 再拦截 task/其它扩展。
- **OMP 将 MCP 投影为 write xd://<tool>**，即使只选 read，模型工具列表仍有 read/write。已经修正拦截：只读角色只允许精确匹配已发现 MCP 的 `xd://mcp__...` 写路由，普通文件写和 xd://task 仍拒绝。直接全禁 write 会把控制 MCP 也禁掉，不能改回。

## 当前有效证据

证据根 `artifacts/orchestration-v2-20261025/`，全部保留真实退出码：

- `continuation-l-rust.log`：226 total，**219 passed / 0 failed / 7 ignored**，exit0，47.21秒。包含 WAL 认证快照/blocks/不复制 settings、角色隔离恢复、四 CLI 新建与历史兼容测试。
- `continuation-l-runtime.log`：最后 runtime build exit0，7.57秒。SHA见 JSON 回执。
- `continuation-k-frontend.log`：tsc + vite build exit0。之后前端未再变更。
- `continuation-k-browser/result.json`：真实 A WorkspaceApp + v2 UI、合成 RPC **20 assertions passed**，含准确四 provider values。没有真实模型/运行时。
- `continuation-l-omp-export.log`：明确 `omp_export_native_role --exact --ignored` 1 passed，Rust 与生产相同 helper 生成角色，空认证和合成 MCP，不调用完整桌面 preflight。
- `continuation-l-omp-role.json` 指向 temp 角色目录；该目录 `result.json`、`native.log` 保留安装版 **OMP18.12.3** 的真实 RPC 状态。
- `continuation-l-omp-native-retry.log` exit0：安装版原生消费 Rust 配置/扩展；真实 runtime→合成 named-pipe IPC；SessionStart、UserPromptSubmit 实际触发；IPC拒绝后 native `agentInvoked:false`；选定 MCP 实际启动且原生 systemPrompt 包含 `xd://mcp__velella_selected_probe`；完整角色正文存在（OMP只trim首尾空白）；未选项目MCP/Skill/extension未观察到；dumpTools仅read/write；fastModeEnabled/Active均false；生成launch文件摘要未变。另以扩展API合同夹具核验普通write/task/未选extension拒绝与选定MCP路由允许。
- `continuation-l-omp-recovery.log`：原生使用后 `omp_post_native_recovery --exact --ignored` **1 passed**，exit0。
- 独立探子 `/root/omp_adapter_review` 只读静态复核，未发现有明确源码依据的高价值新缺陷；没有执行测试或真实认证。这不是独立执行验收或父目标总体验收。

失败门：l-native 首次失败是 fixture 用完整尾随换行比较，而 OMP resolvePromptInput 会 trim；直接核验 exact=false、trimmed=true，调整为trim后唯一重试通过。不要同因第三次重跑。早期 k-native断言不足且本轮已由 l 覆盖，不能拿 k 代替最终检查。最初观察误将 systemPrompt 当字符串；它实际为 string[]，正文从来没有丢失。

没有真实认证模型请求；未证明 agent_end/session_shutdown 的真实执行，没有完整 OMP TUI领取/回报/审查，更没有四 CLI/混合总体验收。原生 get_state 提供 contextUsage(tokens/contextWindow/percent)，本次只观察初始化值；产品40%治理尚未接上，不能当作已实现。

## 下一关键路径

1. 当前 OMP 配置/原生启动边界已经可用并有上述证据，不重复宽搜；补真实认证、工作流和 Stop/Shutdown/恢复生命周期证据时沿对应层级收口。
2. **先闭合 Codex 原生 Skills 有效目录选择证明**（I/G 已登记，避免重复旧 MCP探针）；然后可信外部 MCP目录来源、真实usage来源仍需接入。
3. 四 CLI 各走完真实领取、回报、独立审查、恢复；双 CLI 双实现混合集成；完整独立验收。
4. 通过后按已有授权受控安装，备份回滚、保护生产会话，分别在线核验与UAT。

I 的 OpenCode 生命周期/恢复切片和 G 的既有冻结切片保持；不要为了外部trust/login重新返工通过代码。native-3/4旧尝试受阻已封顶，不原样重试、不能自动同意trust/login/更新。如确有人工前提，先准备具体隔离入口与动作，一次提出；授权内其余工作继续推进。
