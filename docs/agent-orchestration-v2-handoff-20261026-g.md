# 编排 v2 续接 G：Claude/Gemini 技能切片闭合，继续整体验收后安装

整体 **未完成、未安装**。本文件自足；接任者先亲读本文及 `artifacts/orchestration-v2-20261025/continuation-20261026-skill-context.json`，不必全量重载 A–F。源主会话停止源码修改，新主会话接任唯一修改者。没有在途探子、测试或自有原生会话。

## 最终结果、验收与已有授权

自然语言复杂 Git 研发目标 → 可编辑角色/步骤/依赖/验收 → 用户确认 → 独立 Git 工作树并行 → 独立审查固定候选 → 集成 → 独立总体验证 → 可运行、可审阅的集成分支/工作区。

首版含 Codex、Claude Code、Gemini CLI、OpenCode、Kimi 五种真实 CLI。每角色可选模型、已有 Skills/MCP 工具与职责；逐阶段人工门；主控提变更、用户批准；通过候选冻结。捕获原项目 dirty/staged/untracked/deleted，不改原目录或 index。保留 v1、A 版外壳、Rust/SQLite、原生终端。总体验收：五 CLI 各走完启动、领取、回报、独立审查、恢复；至少两 CLI、两个实现角色混合并行/集成；Rust、前端、浏览器、原生及完整独立验收通过。

原用户会话 `01a17391-fb42-7ff3-83db-0e1dc22e4e94` 先说“安装”，说明候选尚未验收后又说“那继续啊”。E 的前主代理已亲读确认；F/G 沿用：继续必要研发和整体验收，通过后安装，不能安装未验收测试版。不要停在小增量后再要求用户说继续。安装须固定候选、备份回滚、保护生产会话/进程。无 PR、发布、全局账户变更、自动 trust/login/更新模态、重启生产会话授权。

## 基线、执行规则与冻结边界

- 工作区 `D:/orcas`；分支 `codex/integrate-collaboration-20261022`；HEAD `62769b4a8ea0ec4c7f347c16726888cbaf3db64a` 未变。
- 大量无关 dirty/untracked/deleted。禁止 reset/clean/stash/全仓格式化。v2 多个文件未跟踪，git diff 不代表完整任务。
- 初始基线 `artifacts/orchestration-v2-20261025/baseline.json` 和 `baseline/`。G 最后核对 E 指纹，只有预期的 `providers.rs`、`lib.rs` 不同；其余 E 所列文件全匹配。F 的 `providers/native_policy.rs` 与 `native-config-isolation.cjs` 也全匹配。
- 用户规则：主代理独占修改、方案取舍和最终测试；探子只读单轮、禁止递归/复用，必须 `fork_turns="none"`、省略其他可选参数。默认 0/1，最多 5；派出立即等待，不自行推进或修改范围内文件。每派明确父缺口/新能力/验收/来源。
- 同根因最多首次加一次基于新证据的重试；通过切片冻结，只有新可复现源码缺陷/明确需求才重开。约 40% 上下文主动交接。不能声称改变不可控制的模型或 Fast；用户默认主线程 Terra，Standard。
- 无 FastCtx 工具可用，当前用 rg/Get-Content。基础文档与要改的代码主代理亲读。每次状态先说用户现在能做什么；分开实现/开发验证/独立测试/技术验收/集成/部署/在线/UAT；总目标不够直说未完成。

已通过且保持冻结：v2 角色/步骤/依赖与模板/版本化方案、人工门/变更、跨节点两次失败 ledger、受控新 run reopening；独立工作树/dirty 基线、增量 cherry-pick/来源 trailer/同补丁空提交、冲突转集成角色、immutable baseline 最终集成；结构化 Evidence 覆盖/候选/路径/大小/数量/链接/hash；外部双向 stdio MCP relay（1 MiB/128 in-flight/120秒/双向ID/通知/白名单/环境剥离/子孙进程清理）；停止所属会话遍历/in-flight/幂等与 blockers；A 外壳/依赖图/人工门/证据/列表。不要重做这些已通过切片。

## F 已闭合的原生 MCP 策略（保持冻结）

- Codex 原生实际 exe：`C:/Users/developer/AppData/Local/OpenAI/Codex/bin/8925276705115dea/codex.exe`，**0.164.0-alpha.11.1**，SHA256 `c6144bc19b2705f25a40d8488b3d57d17a223c5df3c5f65e3c58d72601956c1e`。原生 `mcp list --json` 证明整表 override 合并而非替换，后一个整表会清此前子键。实现一个 TOML 表同时包含选中 proxy 与所有未选 enabled=false；启动/恢复重查有效目录，选中 transport/env/cwd精确核对，新增未选者拒绝。5秒/1MiB/Windows owned Job；原生输出仅内存，避免凭据日志。非 Windows 未支持此原生探针。第一次 fixture 因 override 顺序失败，唯一修后重试 `continuation-n-codex-native-retry.log` 1 passed；不盲重试。
- Gemini 本地 system/default/user/workspace 四scope策略预检，保留 defaults 路径，allowed/excluded不能与选中矛盾；用单个逗号分隔 `--allowed-mcp-server-names` 防空交集变无限制。原始文件存在/不存在及SHA指纹恢复核对。JSONC兼容 strip-json-comments + JSON.parse（不允许尾随逗号）。云端/远端管理策略、环境变量展开/热加载仍未完整验证。
- Kimi 从 cwd 找最近 `.git`（目录或worktree文件），检查 repo根 `.mcp.json` 与 cwd `.kimi-code/mcp.json`，启动/恢复重新拒绝非空或错误目录。不要再重查已闭合根路径问题。

## G 新增实现与设计决定

修改 `crates/velella/src/collaboration/providers.rs`、`crates/velella/src/lib.rs`；新增 `providers/skill_policy.rs`、`scripts/fixtures/native-skill-context.cjs`、`scripts/fixtures/native-skill-hook.cjs`。

### Claude/Gemini 选择技能

原合同要求“每角色已有 Skills”，没有要求保留原生 slash UI。根据真实CLI能力，采用：关闭原生自动技能发现，完整所选 SKILL.md 正文作为角色上下文，保留完整快照及明确相对资源基准。没有静默丢掉所选技能或资源；没有声称这是恶意进程文件系统沙箱。总正文限 1 MiB，超限明确拒绝而非截断。

- Claude 2.1.238 本机帮助确认 `--disable-slash-commands` = Disable all skills，`--bare`说明支持 `--append-system-prompt[-file]`。用 `--append-system-prompt-file <role/instructions.md>` 避免 Windows 命令行长度限制，`--disable-slash-commands` 与 disallowed `Agent,Task,Skill`；原严格MCP配置保持。**未跑认证Claude工作流**。
- Gemini 系统设置 `skills.enabled=false` + tools.exclude追加 `activate_skill`，保留其他禁用项；显式 tools.core 移除原生激活工具。实际导入本机 Config 初始化及 reloadSkills，目录保持空；正对照能读到合成HOME未选技能。系统设置压住 default/user/workspace 的 enabled=true。
- Gemini 的 SessionStart 在 `/clear` 后不保留 additionalContext，因此在 **BeforeAgent → UserPromptSubmit** 每次任务时注入；原生同prompt的工具续轮不重复。钩子读取角色正文同一份字节并核对SHA，先验证再向host发生命周期回执。
- 原生 Gemini 会把 additionalContext 中 `<>` 转成实体，故以 JSON 字符串 `velella_role_context` 包正文，尖括号转JSON Unicode escape。实际原生消费后 JSON.parse 能精确还原含HTML/XML/泛型、字面 `\u003c` 的文本。文件本身保持原文。
- Gemini 普通 hook 非零退出会被原生视作可继续，因此代码对 UserPromptSubmit 的正文/资源/IPC错误输出 **exit 0 + decision:block**；IPC reply.ok=false也阻止，不把拒绝当成功。未知会话或断连同样阻止该任务。没有承诺运行时进程被强杀/外部原生hook未执行的绝对安全沙箱。
- 所有provider新角色都持久保存 `VELELLA_SKILL_BINDINGS`（快照path/digest），恢复/幂等重放会重新核验整个技能目录。Claude/Gemini正文亦绑定SHA；Gemini每次任务再次核对资源。缺绑定的旧v2角色要求新会话；未配置role的普通/v1会话不被要求此标志。
- effective回执对Claude/Gemini写 selected_snapshot_context/native_skill_discovery_disabled/**authenticated_workflow_unverified**；Codex/OpenCode仍instruction_only，Kimi仍标builtin/plugin unverified。

第一次独立静态复核发现三处缺陷（资源恢复未检查、Gemini钩子fail-open、尖括号转义）；上述修复全部完成。第二个干净探子只核对三处闭合，未发现范围内新可复现缺陷；这是限定静态复核，**不是独立测试执行或整体独立验收**。本切片按约定冻结。

## 最新验证与证据

证据根 `artifacts/orchestration-v2-20261025/`：

- `continuation-w-rust.log`：**212 passed / 0 failed / 4 ignored**，216 total，exit0。G 新增3个测试，旧测试更新resource binding fixture。ignored 中 Codex真实配置fixture已在F显式1passed，本次Codex helper未改。
- `continuation-w-build.log`：runtime build exit0；`target/debug/velella-runtime.exe` SHA256 `2ebbed470ba5c124e1af1fe02ca5e472affec3b786fe4ed37c8da7e96f31bdf6`。
- `native-skill-context/probe-i9AgeR/result.json` / `continuation-t-gemini-skills-native-retry.log`：本机Gemini0.47 settings/core真实加载、Config初始化/重新发现、正对照/空目录。首次 `continuation-s-gemini-skills-native.log` 失败是原生库向stdout写提示导致外层JSON parse失败；改独立结果文件后唯一重试通过。没有第三次同因重试。
- `native-skill-hook/probe-eoD91Q/result.json` / `continuation-w-native-hook.log`：真实runtime→合成owned IPC→安装包GeminiClient.fireBeforeAgentHookSafe，验证任务上下文/原生续轮去重/正文篡改阻止/IPC拒绝及断连阻止/markup无损。合成task IDs不是真实TUI恢复或模型流程。
- `continuation-r-skills-tests.log`：中间22 provider passed/1 ignored；`continuation-v-native-hook.log`是修前fixture，不是最终证据。最终以上w为准。
- 前端未变，无需重复开发build/浏览器；沿用E `continuation-g-frontend.log` 与旧v2浏览器20断言。当前无整体五CLI、混合集成、独立测试执行、安装、在线或业务UAT新增结论。

## 下一真正关键路径与本轮只读发现

**下一先闭合 Codex/Kimi/OpenCode Skills 与 OpenCode MCP 原生能力目录，再推进其它已登记缺口及完整认证工作流。不要重开Claude/Gemini已通过技能实现，除非有新可复现缺陷。** 本轮两个只读探子完成下列来源核验，未跑任何真实账户CLI；无需重新大范围扫描。

### Kimi（本机源码）

`C:/home/zpq/.npm-global/node_modules/@moonshot-ai/kimi-code/dist/main.mjs` 0.36.1：

- `--skills-dir`可重复（416571），替代默认用户/项目目录，但 `resolveSkillRoots` 131417-131435、399373-399419 仍追加 extraDirs/plugin/builtin。不能凭该flag报only-selected。
- config `extra_skill_dirs`（62922-62923、64714-64718、216067-216075）；相对extra/explicit目录由 `resolveConfiguredDir` 相对project root解析，`~/`相对OS home（259431-259463）。`mergeAllAvailableSkills=false`仍检查generic `.agents/skills`，不是全禁。
- `builtinProductSkills=false` / `KIMI_CODE_BUILTIN_PRODUCT_SKILLS=false` 仅隐藏productSpecific内置（245738-245769、259741-259763），并非关闭全部内置。内置含mcp-config/update-config/write-goal；插件全禁设置仍未知，禁止凭猜测设置。
- KIMI_CODE_HOME确定config（64016-64020）和默认credentials（147625-147640、144045-144065）；OAuth schema支持file/keyring（62689-62703）。当前已复制credentials并做owner-only ACL，不读出真实密钥、不刷新真实账户；keyring具体行为、其它相对配置字段（provider/hook/MCP cwd等）仍未核验。

### OpenCode（同版本官方tag源码，只读探子核验；未本机执行）

本机入口 `C:/home/zpq/.npm-global/opencode.ps1` → `node_modules/opencode-ai/bin/opencode.exe`，npm package.json:7 **1.18.35**。包无可读应用源码；探子核验官方 `anomalyco/opencode` **v1.18.35** tag。后续应定向读取所需源码并跑隔离fixture，不能把tag核验当本机原生通过。

- `packages/opencode/src/config/config.ts:40-43,365-367,412-488,530-547`：mergeDeep顺序含global、OPENCODE_CONFIG、project、config dirs、OPENCODE_CONFIG_CONTENT，**之后**还有组织/managed配置。内容层不能清掉未选同名/异名mcp，未提供字段会继承；已知名称可覆enabled=false，managed还可能覆回。`mcp/index.ts:505-519` 禁用才不连接。
- `skill/index.ts:21-25,173-225`：用户/项目.claude及.agents、config dir skill/skills、cfg.skills.paths/urls。`OPENCODE_DISABLE_EXTERNAL_SKILLS=1`关闭.claude/.agents；`OPENCODE_DISABLE_PROJECT_CONFIG=1`仍留global及home .opencode（config/paths.ts:23-40）。
- `Skill.available(agent)`用permission.skill deny过滤模型可用目录（skill/index.ts:310-314）；Skill.all不滤，所以debug skill不能证明角色最终目录。agent.permission追加默认/用户规则（agent/agent.ts:267-293），permission/index.ts:28-37,186-202取最后匹配；builtin customize-opencode也需过滤。
- `OPENCODE_PURE=1`跳过外部插件；`OPENCODE_DISABLE_DEFAULT_PLUGINS=1`控制内置（plugin/index.ts:170-187）。但config会ensureGitignore并启动@opencode-ai/plugin后台依赖安装（config.ts:438-479），还有well-known/组织远端配置（370-408,492-525）。debug config/skill不等于无副作用读取真实用户配置。`OPENCODE_DISABLE_AUTOUPDATE=1`存在。
- 可用隔离fixture前提：临时cwd及XDG_CONFIG_HOME/XDG_DATA_HOME/XDG_CACHE_HOME、OPENCODE_TEST_HOME、DISABLE_PROJECT_CONFIG、DISABLE_EXTERNAL_SKILLS、PURE、DISABLE_DEFAULT_PLUGINS、DISABLE_AUTOUPDATE；content只放本地合成数据/no skills.urls。这样会隔离账户；保留真实XDG_DATA_HOME可能触发组织远端配置。**这些条件尚未本机运行验证**。

### 其它未完成项

- Codex原生Skills：官方config reference支持 `skills.config[].path/enabled`，不等于全局白名单；实际app-server/原生目录仍须定向确认。官方页面直接获取成功；search connector两次isError，禁止同路第三次盲试。
- 可信外部MCP目录来源尚未实现，UI仍手工command/catalog；relay冻结能力不代表可信注册源发现。
- 产品40%上下文交接真实usage来源未接入，不能靠agent自报冒充硬治理；模型真实可用性和Standard状态未验证。
- 五CLI完整工作流、双CLI双实现混合集成、完整独立验收及集成分支/工作区交付；通过后按已有授权安装，做备份/回滚/保护生产。

## 外部前提与禁止原样重跑

native-2：Codex/Claude/Kimi停trust；OpenCode主界面后v1.19.4更新模态且hooks未完整定位。Gemini原launcher两次version/help失败已封顶；F/G沿用已有会话级残留完整包wrapper（未改全局安装）。native-4到人工trust/login，零turns/hook false，已stopped，自有daemon结束。

不得原样重跑native-3/4，不自动接受trust/login/更新模态或改全局账户。`desktop/tests/orchestration-v2.native.cjs`只观察planning，不能当五CLI完整验收。代码完成只剩人工前提时，准备确切隔离入口与动作，一次说明，不只重复“等授权”。继续授权内研发，不因外部阻塞停止所有可做工作。
