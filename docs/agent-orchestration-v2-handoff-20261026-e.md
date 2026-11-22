# 编排 v2 续接 E：继续实施与验收，之后安装

整体未完成，未安装。本文件是当前自足交接；先亲读全文。旧 a/b/c/d 交接只在需要历史细节时查，不必再次全量载入。原主会话交接后停止源码修改。

## 用户目标、授权与总体验收

自然语言复杂 Git 研发目标 → 可编辑角色/步骤/依赖/验收 → 用户确认 → 独立 Git 工作树并行 → 独立审查固定候选 → 集成 → 独立总体验证 → 可运行、可审阅的集成分支/工作区。

首版包含 Codex、Claude Code、Gemini CLI、OpenCode、Kimi 五种真实 CLI。每角色可选模型、已有 Skills/MCP 工具和职责；逐阶段人工门；主控提出变更、用户批准；已通过候选冻结。捕获原项目 dirty/staged/untracked/deleted 而不修改原目录或 index。v1 保留；沿用 A 版外壳、Rust/SQLite、原生终端。五 CLI 各完成启动/领取/回报/独立审查/恢复，再完成至少两 CLI、两个实现角色的混合并行/集成；Rust、前端、浏览器、原生及独立验收通过才完成。

**最新安装授权**：原用户会话 `01a17391-fb42-7ff3-83db-0e1dc22e4e94` 用户先说“安装”，随后在候选未验收的说明后说“那继续啊”。本主代理已用 read_thread 亲读这两条。继续必要研发及整体验收，然后推进安装；不能视为允许跳过验收或安装未验收测试版。安装前准备固定候选、备份回滚、保护生产会话/进程。仍无发 PR、发布、全局账户改动、自动 trust/login、重启生产会话授权。不要反复停在小增量汇报并要求用户说“继续”。

## 固定基线与协作

- 唯一源码 D:/orcas；分支 codex/integrate-collaboration-20261022；HEAD 62769b4a8ea0ec4c7f347c16726888cbaf3db64a。
- 大量无关脏改动：不能 reset/clean/stash/全仓格式化。很多 v2 文件未跟踪，git diff 不代表完整任务差异。
- 初始任务基线 `artifacts/orchestration-v2-20261025/baseline.json`、`baseline/`；最新本轮指纹 `continuation-20261026-mcp-shutdown.json`。
- 当前无在途探子、测试或构建。旧主线程停止修改，由新线程接任唯一修改和最终测试者。
- 用户规则：探子只读单轮、fork_turns=none、省略其余可选参数、不递归不复用；派出立即等待，仍在途时主代理不推进别的工作。默认0或1，最多5。每次委派说明关闭哪个总体缺口。同因首次加一次有新证据重试。约40%上下文主动交接。不可声称切换无法控制的模型/Standard/Fast。

## 已具备能力，保持已验证切片冻结

- v2 角色/步骤/依赖、版本化计划/模板、人工确认与变更、系统集成和验证人工门、失败根因 ledger 跨节点限制两次失败、受控 reopening 新 run 不改变 accepted 前任。
- 独立工作树、dirty 基线、捕获中断的严格恢复；单父增量 cherry-pick、来源 trailer、同补丁空提交、冲突交给计划内 integrate 角色、最终从 immutable baseline 合入。基线和中间集成经过实际 Git fixture 和独立静态复核；不要再当待办重做。
- 结构化 Evidence，逐合同 requirement 覆盖、固定候选、artifact 限定工作树 .velella/evidence、大小/数量/链接/路径/hash 核对；报告、独立审查、接受时复核。系统核对 Agent 提供的证据，不声称执行过 Agent 记录的命令。
- Skills 全目录快照和资源指纹，Kimi selected skills-dir；其余四 CLI catalog 仍为 instruction_only。
- A 外壳的角色/步骤/依赖图、人工门、交付证据和运行列表。上一轮 v2 浏览器20断言通过；本轮仅收紧无证据保证的文案，build通过。

## 本轮新增实际闭合

### 外部 MCP 协议与配置

`collaboration/mcp/external.rs` 新双向 stdio relay，替代 mcp.rs 的逐请求同步读一行：
- 两侧独立读取和有界输出队列；1MiB 帧、128 in-flight、120秒请求期限；通知不占响应；字符串/整数 ID 分别关联、同 ID 双向请求互不冲突；拒绝重复和未知响应。
- 只允许工具合同：initialize/ping/tools/list/tools/call/logging 和 roots/ping 回调，转发对应通知；资源/prompts/sampling/elicitation不在此合同。初始化能力同步收紧；只有匹配 tools/list 的响应被过滤。非选中 tool 在发给子进程前拒绝。
- 启动前核对 descriptor SHA-256；移除子进程继承的 VELELLA_* 控制路由环境；Windows job/Unix私有进程组清理子孙。是可信工具的合同限制，不是任意恶意程序的安全沙箱。
- providers selected_mcp 创建后不覆盖 descriptor，命令参数携带预期SHA。角色校验由计划与直接 terminal.open 共用，拒绝目录外工具/重复保留ID。
- browser_agent::inject 对带角色指纹的会话不再追加未选浏览器 MCP，旧已注入角色恢复明确拒绝。
- 新 `VELELLA_ROLE_FILE_BINDINGS` 绑定 CLI 实际读的 instructions、Claude mcp.json、Gemini配置/恢复wrapper、OpenCode plugin、Kimi agent/config/mcp 等文件；恢复和同request重放核对这些文件与外部descriptor。凭据目录不进回执。旧角色无此绑定明确拒绝，要求新会话。
- UI 已移除“现有用户 MCP 不会继承”绝对保证；Standard回执改 configured_unverified。CLI全局/项目配置合并语义仍需原生核验。

### 会话清理与结束竞态

`desktop/orchestration/v2.rs` 收集启动意图缺 workspace/session 或无request时记录阻塞；缺 result 用原request_id。遍历所有可确认归属的会话，单项失败不跳过后续；记录 shutdown_blockers。启动/操作 in-flight 时不把尚未建锁的会话当已死；再次无法确认时移除旧死亡回执。成功 stop effect 仍按幂等记录重放，不重复kill；缺注册保持阻塞不猜测。

fixture 现在给 lead、节点和审查 open 写真实 request 信息，不再用 result-only 跳过 shutdown。完整接受用例证明锁持有时拒绝，全部释放后逐个记录死亡才接受。额外覆盖：第一项缺归属但后续仍收kill、启动inflight、旧死亡回执失效。

全库还发现真实竞态：candidate_work 对所有命令先做 active-run 鉴权，导致结束时或结束后 receive/status 偶发拒绝。已改为先筛选需要候选的命令；这些命令仍 active鉴权，其余由 execute/mutate 原契约鉴权。terminal_io 测试新增确定性的结束后receive/status断言。

## 最新实际验证与失败记录

- `cargo test -p velella --lib -q` 最新 **205 passed / 0 failed / 3 ignored**，208 total。日志 `continuation-h-rust-retry.log`，退出0。
- `cargo build -p velella --bin velella-runtime -q` 最新通过，日志 `continuation-i-build.log`；debug runtime最新。
- `npm run build` 通过，`continuation-g-frontend.log`；仅既有大bundle警告。
- `node scripts/fixtures/external-mcp-protocol.cjs` 退出0，真实 Node 服务/孙进程通过双向stdio/乱序ID/通知/白名单/能力裁剪/环境剥离/客户端EOF清理、oversize清理、descriptor漂移在spawn前拒绝。结果 `external-mcp-protocol/result.json` 绑定执行二进制SHA。其后只增加配置文件绑定及候选鉴权修复；relay代码未变。不要说该次真实fixture用的是最后一次build。
- 限定 tracked diff --check通过；不覆盖所有untracked源码。
- 两个独立只读探子：第一轮发现实际CLI配置文件未绑定，已修；第二轮确认配置恢复及candidate鉴权修复范围内无可复现缺陷。**不是独立测试执行，也不是整体独立验收。**
- 失败一：新增测试 unwrap_err 要求 Descriptor:Debug，改 .err().unwrap() 后全库204通过。失败二：最新全库触发结束收信竞态，按上述真实源码根因修复后205通过。不要无变化重试。

## 剩余关键路径（不是重新审查一切）

1. 原生能力隔离：五provider实际配置合并优先级、可信外部MCP注册/目录来源（当前UI仍手工声明command/catalog，不能说可信发现已完成）；四CLI原生Skills catalog约束。可做本机源码/帮助受限核验和配置fixture；不要重复返工已冻结relay。
2. 真实模型支持/版本及Terra/Luna/Standard：Codex空值按角色派生 gpt-6-terra/luna，显式model优先；这些ID在本机账户可用性未验证；不能把回执当官方支持列表。
3. Kimi private home复制同账户配置/凭据且owner-only ACL，未验证相对路径重定位/OAuth恢复；不得读取输出真实密钥或自动刷新账户。
4. 产品内40%上下文交接的真实usage来源还未接入，不应靠自报假装硬治理。
5. 五CLI各完成完整原生工作流及双CLI双实现混合端到端；完整独立验收、集成分支/工作区交付；通过后按新增授权安装、备份回滚与保护生产会话，分开报告在线和业务UAT。

## 原生已知前提：禁止原样重跑

native-2 已保存：Codex/Claude/Kimi停在trust；OpenCode到主界面后出现v1.19.4更新模态，hooks未就绪原因未完整定位。Gemini原启动器包不完整，曾两次 --version/--help 失败；现已有会话级完整残留包wrapper，native-4到人工trust/login，仍零turns/hook false、最终 stopped、自有daemon已结束。native-3/4不得无新前提原样再跑。不能自动接受trust/login/更新模态。若代码工作完成只剩这些前提，准备具体可操作的隔离会话入口和确切人工动作，一次说明；不要只反复说“等授权”。

最新native脚本 `desktop/tests/orchestration-v2.native.cjs` 仍只观察planning，不能当完整五CLI验收脚本。原安装与生产状态未改动；没有当前在线或业务UAT结论。
