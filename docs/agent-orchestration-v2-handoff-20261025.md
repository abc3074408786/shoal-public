# Agent 编排 v2 — 实施交接（2026-10-25）

状态：**整体未完成；候选未独立验收、未安装。** 用户已明确要求实施下述完整方案；这不是新的需求讨论。因用户 AGENTS.md 要求长线程约 40% 上下文时主动交接，交接新主会话继续工程工作，不得把交接视为完成。

## 已批准的总目标

自然语言描述复杂 Git 研发目标 → 生成可编辑角色/步骤/依赖/验收流程 → 用户确认 → 独立 Git 工作树并行 → 独立审查 → 集成 → 完整目标验证 → 交付可运行、可审阅、可验收的集成分支和工作区。

首版明确包含 Codex、Claude Code、Gemini CLI、OpenCode、Kimi 五种 CLI；每角色配置模型、已有 Skills/工具和职责。逐阶段可配置人工确认。运行中主控提出变更、用户批准，已通过候选冻结。工作树捕获当前 dirty/staged/untracked/deleted 源码而不修改原目录或 index。分支 `codex/orch/<run>/...`。默认不回写原目录、不创建远端 PR、不发布；正式安装另行验收。复用当前唯一 A 版外壳、Rust 调度、SQLite 和原生终端。

接口要求：版本化 RoleDefinition / WorkflowTemplate / PlanRevision / GateDecision / Artifact/Evidence；跨 run-owned 工作区的 collab.launch；capabilities/templates/revision/gate RPC；旧 v1 记录继续运行，不原地迁移。保留请求幂等和实际 candidate 身份校验。进程退出、静默、Agent 自报均不是验收。

验收要求：五种真实 CLI 各完成启动、领取、回报、独立审查和恢复；再完成至少两种 CLI、两个实现角色的混合并行/集成；Rust、前端、浏览器、原生回归；独立核验整体目标。部署、当前在线和业务 UAT 不能被技术测试替代。

## 工作区、权限与协作规则

- 唯一开发/构建源码 `D:/orcas`，分支 `codex/integrate-collaboration-20261022`，开始 HEAD `62769b4a8ea0ec4c7f347c16726888cbaf3db64a`。
- 大量未提交工作早于本次任务。不能 reset/clean/stash/格式化全仓、不能覆盖其他改动、不能重启生产 app/daemon 或现有用户会话。已完成一些安装/终端工作仍属于别人。
- 本次文件基线：`artifacts/orchestration-v2-20261025/baseline.json` 和 `baseline/`。新文件没有旧基线。collaboration.rs 的额外本次修改为 modules mcp/providers 和 collab.revision.propose 路由；其他 dirty 内容不属于本任务。
- 用户限制：只有主代理改代码和作最终验证；子代理只读探索/检索/核验；派生必须 fork_turns=none，省略所有其他可选参数；不能递归、不能复用/追派；派发后立即等待，仍有相关探子在运行时不自行继续工作；最多五个。每个探子一轮终态。
- 一个根因首次执行+一次基于新证据重试；没有新证据不得第三次。24h 无目标增量停止派卡并收敛。原生权限不由编排运行时自动批准，真实测试不得暗中 bypass。
- 主线程默认 Terra；默认 Standard。不要宣称自己切换了不可控制的服务档位。无外部 GitHub 写入、安装、发布、账户切换授权。

## 当前实现（全部尚需独立审查）

新增：

- `crates/velella/src/collaboration/orchestration/v2.rs`：角色/步骤/方案类型，校验、系统集成/验证节点、人工 gate、计划修订冻结已派发节点。
- `crates/velella/src/collaboration/providers.rs`：五CLI会话局部配置编译、Skills摘要/发现、工具清单、Gemini hooks、OpenCode临时plugin、Kimi临时profile/agent/skills；此模块还有重要缺口见下。
- `crates/velella/src/collaboration/mcp.rs`：stdio MCP 通过 session binding 注入凭据调用既有受限 collab 协议。
- `crates/velella/src/desktop/orchestration/v2.rs`：复用旧 effect journal 的 v2 调度、独立写工作树、报告审查、阶段gate、集成/最终验证、停止。
- `crates/velella/src/desktop/orchestration/workspaces.rs`：dirty 基线、run-owned工作树、依赖增量输入；最终 integration 由实现角色合入并处理冲突。
- `crates/velella/src/desktop/orchestration/v2_tests.rs`：真实临时Git+fixture host 的隔离/模板/变更/人工gate/审查/集成候选测试。
- `desktop/src/OrchestrationV2.tsx`：创建、自然语言调整、角色配置、步骤表单、运行列表、确认/变更、模板保存、交付状态。
- `desktop/tests/orchestration-v2.native.cjs`：真实CLI启动/规划前提检查（不是完整E2E），只拥有独立profile；结束停止其会话与daemon，保留证据。

修改：

- `collaboration/orchestration.rs`：v2状态路由、模板存储/版本、公开过滤沿用；v1兼容。
- `collaboration.rs` / `collaboration/cli.rs`：新增模块/修订/MCP命令和五CLI枚举。
- `desktop/orchestration.rs`：v2分流、跨所属工作区状态查找、capabilities/templates接口、v2暂停仍核对已派发结果。
- `desktop/collaboration_commands.rs`：受控跨workspace启动和五CLI成员，capture_baseline可在同模块调用。
- `desktop/workbench_commands.rs`：五CLI打开、role_config编译；旧普通终端行为保持。
- `lib.rs`：collaboration-mcp 和 orchestration-hook 入口。
- `desktop/orchestration_tests.rs`：fixture可绑定不同工作区，加载v2测试。
- `desktop/src/OrchestrationPage.tsx`：能力探测选择v2入口，旧版访问保留。
- `desktop/src/orchestration.ts`：成员workspace_id可选。

没有修改 CSS；复用现有 orchestration.css。需要对新布局做浏览器视觉检查。

## 已执行验证与限制

- `cargo check -p velella --lib` 通过，输出有既有 restricted/chat_sandbox 等 warning，未碰这些模块。
- `npm run build`（desktop）通过；仍有既有大 bundle warning。
- `cargo test -p velella --lib orchestration_`：先 11 通过，再新增后 15 通过。
- 单独 `cargo test -p velella --lib orchestration_v2_review_gate_integration_and_final_candidate` 通过：独立写入→独立审查→人工gate→集成→独立验证→最终候选防漂移，使用真实Git与fixture host，**不是模型验收**。
- `cargo build -p velella --bin velella-runtime` 通过，最新 debug runtime 用于 native-2。
- 目前没有跑新增 v2 浏览器测试、全工作区测试、独立源码审查，也没有真实任务端到端通过。最后的基线修正后尚需重跑相关 unit tests。

## 原生尝试与不可重复的事实

证据根：`artifacts/orchestration-v2-20261025/`。

1. `native-1/`：五CLI均在模型启动之前失败于基线一致性。根因：snapshot 位于 D:/orcas 的 artifacts 子目录，调用通用 candidate(snapshot) 会探测外层Git。修复：按 capture manifest 的存在文件直接计算snapshot hash，防止借外层repo枚举；同时先移除隔离基线中已 staged 删除的源文件。该缺陷已修复，原site保留，没有覆写/重发。
2. `native-2/`：修复后五CLI都建立了一名原生主控成员。结束观察前全部仍在planning：Codex/Gemini/OpenCode/Kimi 为 hooks 未就绪；Claude 先hooks未就绪，随后明确 `等待原生授权，请打开会话处理`。**没有已确认方案、没有执行节点或完整模型交付**。

两次脚本均已退出（exit 0仅代表诊断脚本完成，绝非验收通过）。脚本finally尝试停止所有自有协作及daemon；应在继续原生检查前根据 ownership.json 和独立profile状态核对清理结果，不能按进程名杀生产进程。每provider JSON是停止前观察snapshot。result.json记录事件；source项目以及 `.velella/sessions/<id>/terminal.bin` 可用于只读定位，避免输出凭据。

不能直接跑 native-3。先从 native-2 的终端输出/描述符/配置诊断每个CLI的确切阻塞；如果是信任/登录/网络/模型不可用，沿对应层处理，不能伪造hooks或关闭授权。基线根因已在第二次关闭，若新证据证明另一个源代码缺陷，则只修该缺陷并明确登记新前提再决定验证。

## 已核实的外部CLI资料

- Kimi0.36.1安装bundle：`C:/home/zpq/.npm-global/node_modules/@moonshot-ai/kimi-code/dist/main.mjs`。
  - 233635-233639 `resolveKimiHome` 使用 `KIMI_CODE_HOME`，config.toml固定在home；315494-315507 auth用同home/configPath。
  - 233661-233683 MCP合并home/mcp.json、Git root/.mcp.json、cwd/.kimi-code/mcp.json。
  - 253803-253853 TOML `[[hooks]]` event/matcher/command/timeout；SessionStart/UserPromptSubmit/Stop/PermissionRequest等。
  - 147639 credentials目录默认home/credentials。
  - 角色文件 tools/disallowedTools/subagents 见 https://www.kimi.com/code/docs/en/kimi-code-cli/customization/agents.html 。源码存在这些字段，`Agent/AgentSwarm`过滤需要真实证明。
- OpenCode1.18.35：OPENCODE_CONFIG_CONTENT合并会话配置；plugin支持绝对路径/file URL；插件返回 event({event})、chat.message(input,output)。session.created/idle/permission.asked与原生状态对应需实测，不得以plugin加载当成输入就绪。
- Gemini0.47.0：GEMINI_CLI_SYSTEM_SETTINGS_PATH覆盖、BeforeAgent/AfterAgent等hooks。原有管理配置需保持；现实现merge可能覆盖同名hook数组，必须审查。
- Codex官方 https://learn.chatgpt.com/docs/config-file/config-reference 支持技能启停、MCP tools allowlist、shell/tool/feature配置。当前实现仍不完整，见下一节。

## 必须继续完成/审查的已知缺口（不要隐瞒）

1. **五CLI原生就绪/回报/恢复**：native-2未通过。OpenCode事件是否会在空TUI创建session尚未知；Kimi private-home复制同账户config/credentials，虽然不回写全局且effective.json不含凭据，但凭据目录保护、相对配置路径、OAuth刷新、MCP覆盖顺序和恢复行为尚未验证。不能将“配置已生成”标成接入通过。
2. **能力配置不完整**：当前工具UI只列原生工具，还没有已有MCP服务器/工具选择。Skills目前大多作为角色指引引用原路径（Kimi有skills-dir），未完整快照引用资源或按role限制原生catalog。model为文本/空值继承CLI，没有真实支持列表或有效性预检、未记录CLI版本。Terra/Luna预设、Standard完整约束未完成。
3. **权限保持需审查**：OpenCode选定工具用ask，禁止task；其他provider的子Agent禁用覆盖与外部MCP/tool旁路未完整验证。Gemini `tools.core`字段与native配置版本需核实。不能宣称prompt限制等于硬权限。Kimi资料没有单独配置路径参数，当前private-profile方案是待独立审查实现。
4. **质量证据不完整**：回报仍是旧summary/candidate。Artifact/Evidence强类型和执行检查结构尚未完整实现；v2最终把多个阶段标passed的依据还需严格核对，不能将泛化completed自报当测试实测。UI不能误报完成。
5. **变更/失败治理**：已派发节点和role冻结（包含failed节点），可以原节点一次重试；但“可复现缺陷/明确需求后重开已验收候选”流程未实现。同根因跨不同节点/版本的失败ledger未完成，不能只依赖attempt_count。40%上下文交接机制未接入产品。
6. **集成和依赖**：最终integration从baseline开始由集成Agent按依赖commit/input_head合入增量，成功路径fixture通过。应测试多个实现节点/重复依赖/提交范围/冲突处理/分支非祖先/合入中断。非final节点输入的cherry-pick冲突目前保留现场阻塞，没有自动集成角色处理。输入工作树创建后的崩溃恢复/已started import恢复要审查。
7. **暂停/停止/恢复**：用户接管和不确定输入保持旧语义，但generic新CLI是否正确识别授权需要真实验证。role_config的请求幂等一致性未纳入ensure_matching_launch，可能同request_id不同配置返回旧session；需检查修复。止后所有own session死亡证据需要补齐。
8. **前端**：目前是执行卡列表+文字依赖，缺计划要求的依赖图；更改proposal批准后dirty草稿重载、反复选择模板、旧版返回按钮布局等需浏览器验证。使用实际现有A版外壳，不另造产品外壳。
9. **兼容与安全**：stdio MCP输入大小/非法JSON/权限边界/响应没有密钥应有真实协议测试；模版ID/路径约束、root path/symlink、敏感字段UI投影应独立审查。没有运行全仓diff审查，因为很多dirty属于别人。

## 下一步（唯一关键路径）

先只读诊断 native-2 的实际就绪阻塞，同时对上述新模块做独立缺陷核验；主代理修复具体缺陷、补齐明确未实现能力及测试。授权内代码工作继续，不因外部CLI信任阻塞就停止所有研发。等待原生输入/授权时仅冻结依赖它的测试；只有其余可做工作已完成且需要人介入时，提出具体唯一决定。最终如五CLI或混合测试仍缺，明确整体未完成，并给精确原因，不安装未验收候选。
