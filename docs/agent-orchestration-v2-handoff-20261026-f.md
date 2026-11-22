# 编排 v2 续接 F：原生配置缺口已部分闭合，继续整体验收后安装

整体 **未完成、未安装**。这是当前自足交接；接任者先亲读全文及同目录证据索引，无须重新全量载入 A–E。交接源线程停止源码修改，由新会话接任唯一修改主代理。没有在途测试或探子。

## 用户最终结果与授权

自然语言复杂 Git 研发目标 → 可编辑角色/步骤/依赖/验收 → 用户确认 → 独立 Git 工作树并行 → 独立审查固定候选 → 集成 → 独立总体验证 → 可运行、可审阅的集成分支/工作区。

首版含 Codex、Claude Code、Gemini CLI、OpenCode、Kimi 五种真实 CLI。角色可选模型、已有 Skills/MCP 工具和职责；逐阶段人工门；主控提出变更、用户批准；已通过候选冻结。捕获原项目 dirty/staged/untracked/deleted，不改原目录或 index。保留 v1、A 版外壳、Rust/SQLite、原生终端。总体验收：五 CLI 各走完启动、领取、回报、独立审查、恢复，再完成至少两 CLI、两个实现角色的混合并行/集成；Rust、前端、浏览器、原生及完整独立验收通过。

原用户会话 `01a17391-fb42-7ff3-83db-0e1dc22e4e94` 先说“安装”，在说明候选尚未验收后又说“那继续啊”。E 交接的前主代理已亲读确认。本阶段沿用这项授权：继续必要研发和整体验收，通过后安装，不能跳过验收安装测试版。安装前做固定候选、备份/回滚和生产会话/进程保护。无发 PR、发布、全局账户变更、自动 trust/login、重启生产会话授权。不要停在小增量后要求用户再次说继续。

## 基线与执行规则

- 源码 `D:/orcas`；分支 `codex/integrate-collaboration-20261022`；HEAD `62769b4a8ea0ec4c7f347c16726888cbaf3db64a` 未变。
- 大量无关 dirty/untracked/deleted；禁止 reset/clean/stash/全仓格式化。v2 许多文件未跟踪，git diff 无法表示完整任务。
- 初始基线 `artifacts/orchestration-v2-20261025/baseline.json` 和 `baseline/`。E 指纹 `continuation-20261026-mcp-shutdown.json`。本次新指纹 `continuation-20261026-native-policy.json`。本次结束重查 E 所列文件，只有本次有意改的 providers.rs 不同，其余全部匹配。
- 用户规则：主代理独占修改、取舍和最终测试；探子只读单轮，不复用、不递归，`fork_turns="none"`，省略其他可选参数。默认 0/1，最多 5；派出立即等待，仍在途时不推进其他工作或修改范围内文件。给探子明确父目标缺口、范围、出处和验收。
- 同根因最多首次加一次有新证据重试；通过的切片冻结，只有新可复现缺陷/需求变化才重开。约 40% 上下文主动交接新会话。不能声称切换无法控制的模型或 Standard/Fast。没有可用 FastCtx，当前用 rg/Get-Content；基础文档及即将修改的代码由主代理亲读。
- 每次汇报先说用户现在能做什么；区分实现、开发验证、独立测试、技术验收、集成、部署、在线和业务 UAT。子任务通过不等于整体完成。

## 已完成且保持冻结的切片

- v2 角色/步骤/依赖、版本化计划/模板、人工确认/变更、系统集成和验证人工门、跨节点两次失败 ledger、受控 reopening 新 run。
- 独立工作树和 dirty 基线捕获/恢复；增量 cherry-pick、来源 trailer、同补丁空提交、冲突交 integrate 角色、最终从 immutable baseline 合入。实际 Git fixture 和独立静态复核已过，不重做。
- 结构化 Evidence 的 requirement 覆盖、固定候选、工作树内 `.velella/evidence` 路径/大小/数量/链接/hash 校验。系统核对 Agent 记录，不伪称执行了这些命令。
- Skills 整目录快照/资源指纹；MCP 外部双向 stdio relay，1 MiB 帧/128 in-flight/120 秒、双向ID、通知、白名单/能力裁剪、descriptor hash、环境剥离及子孙进程清理。真实 Node fixture 已过；不重做 relay。
- CLI 实际配置文件指纹、恢复/重放检查；浏览器注入不再补未选 MCP。
- 停止遍历所有确认归属会话，in-flight 不误报死亡，幂等停止与 shutdown blockers；结束后 receive/status 竞态已修。
- A 外壳角色/步骤图、人工门、证据及运行列表。旧 v2 浏览器 20 断言、前端 build 已过；本次未改前端。

## 本次新增实现与实证

修改 `crates/velella/src/collaboration/providers.rs`；新增 `providers/native_policy.rs` 和 `scripts/fixtures/native-config-isolation.cjs`。

### Codex 原生有效 MCP 目录

本机原生可执行文件实际为 `C:/Users/developer/AppData/Local/OpenAI/Codex/bin/8925276705115dea/codex.exe`，版本 **0.164.0-alpha.11.1**，SHA256 `c6144bc19b2705f25a40d8488b3d57d17a223c5df3c5f65e3c58d72601956c1e`。npm 包 0.164.0 不能代替这个真实执行来源。

隔离 `CODEX_HOME` 实测证明：`-c mcp_servers={selected...}` 会保留全局不同名服务器；同一 CLI 的后一个整表 override 会清掉此前子键 override；CLI dotted path 不解析引号键。修复把所有已发现未选服务器的 `enabled=false` 与选中 proxy 配置合为 **一个 TOML 表**。

启动前用原生 `codex mcp list --json` 读取并核对有效目录；恢复/同request重放再查。要求每个选中服务器启用、stdio command/args 匹配、无继承 env/env_vars/cwd，未选者全部关闭。新加未选服务器或同名 transport 冲突拒绝恢复/启动，不改源配置。原生列表可能含凭据，因此只在内存处理，不写日志/错误/回执；回执只存本系统选中的 proxy 配置。探针 5 秒/1 MiB/Windows owned Job；非 Windows 明确未支持此原生核验，不能宣称跨平台完整。

首次显式原生 fixture 因 override 顺序失败（`continuation-l-codex-native.log`）；诊断后改成单表，唯一重试通过（`continuation-n-codex-native-retry.log`）。没有第三次盲重试。后续 JSONC 修改未改 Codex 实现。库测试新增 ignored 原生测试，已明确运行通过；默认 ignored 数量从 3 变 4 不代表此原生测试没执行。

### Gemini 原生白名单与策略

本机完整残留 bundle `C:/home/zpq/.npm-global/node_modules/@google/.gemini-cli-eEbuV9on/bundle/`，版本 0.47.0。原 launcher 损坏仍是旧前提；**未**再次运行损坏 launcher。

实际导入安装包 `loadSettings` 并在假 HOME 和内存 scopes 测试：MCP 不同名跨层合并；多个 `mcp.allowed` 的空交集被后续过滤器当不限制。新增检查 system、system-defaults、user、workspace 四处策略，既有 allowed 必须容许所有选择，excluded 不能排斥选择；传 `--allowed-mcp-server-names` 的单个逗号分隔参数，避免空交集旁路（yargs `nargs:1`）。保留原 system-defaults 路径，保存存在/不存在文件的原始指纹，恢复时拒绝漂移。旧无策略绑定 Gemini 角色要求新会话。

独立复核指出 JSONC 兼容损失，已修：按原生 strip-json-comments+JSON.parse 语义识别字符串/转义并去注释，错误 JSON/未闭合块注释/尾随逗号拒绝。读取限 1 MiB 普通文件；指纹仍取原始字节。安装包 loader 的假 HOME JSONC 读取断言实际通过。第二个新探子只复核该修复，范围内无新可复现缺陷。

边界：尚未验证 Gemini 云端/远端管理策略、策略环境变量展开的完整兼容，以及运行中用户改策略的热加载；不把本次离线配置验证说成完整原生工作流。

### Kimi 项目 MCP 路径

安装包 `C:/home/zpq/.npm-global/node_modules/@moonshot-ai/kimi-code/dist/main.mjs`，0.36.1。原生从 cwd 找最近 `.git`（目录或 worktree文件），读 `<repo root>/.mcp.json` 和 `<cwd>/.kimi-code/mcp.json`。修复原来只检查 cwd 的漏口；启动和恢复均重新查，拒绝非空/格式错误项目 MCP。测试覆盖子目录、Git worktree 指针、恢复后新增、两处路径、空文件。

Skills 回执不再把 Kimi `--skills-dir` 说成完整封闭：它仍可合入 extraDirs、插件及内置技能。其他四 CLI 仍 instruction_only，不能说原生 Skills 目录约束已完成。所有 provider 的 MCP 回执也明确 inherited catalog 仍需原生验证。

## 最新验证与证据索引

根目录 `artifacts/orchestration-v2-20261025/`：

- `continuation-q-rust.log`：最终 `cargo test -p velella --lib -q` **209 passed / 0 failed / 4 ignored**，213 total，退出 0。
- `continuation-q-build.log`：最终 `cargo build -p velella --bin velella-runtime -q` 退出 0，debug runtime 已更新。既有 chat_sandbox warnings 没有顺手修改。
- `continuation-n-codex-native-retry.log`：显式 `codex_native_catalog_fixture -- --ignored --nocapture` **1 passed**，真实原生目录、源配置不变和恢复新增服务器拒绝。使用的是 JSONC 修复前的测试二进制，Codex helper 之后未改变。
- `continuation-p-provider-tests.log`：JSONC后 provider 范围 **19 passed / 1 ignored**。
- `native-config-isolation/probe-o8caeo/result.json`：最终离线配置 fixture；真实 Codex 可执行文件 SHA，Gemini settings/core bundle SHA，目录合并/过滤/空交集/JSONC。不是模型请求、trust批准、登录或完整工作流。
- 两次独立只读复核：第一次覆盖原生 policy 约束及调用点，发现 JSONC 兼容损失；修后第二次只查 JSONC parser/指纹/调用，范围内无新可复现缺陷。**没有独立测试执行、没有整体独立验收。**
- 前端本次未修改/重测；沿用 E 中 `continuation-g-frontend.log` build通过与早前浏览器20断言。

## 下一条真正关键路径

1. **四 CLI 原生 Skills 约束、Kimi extra/plugin/builtin 范围、OpenCode 原生 MCP 合并。** 不再重审已闭合 Codex普通配置表/Gemini本地策略/Kimi根路径。按安装版本具体行为决定实现，不凭传入参数自报成功。
2. 可信外部 MCP 目录来源仍未实现；UI 仍手工声明 command/catalog，不能声称已从可信注册源发现。冻结的外部 relay 只管被选中工具，不是恶意进程安全沙箱。
3. Kimi private home 同账户复制与 owner-only ACL 已有，**相对配置路径重定位/OAuth恢复仍未验证**；不读出密钥、不刷新真实账户。
4. 产品内 40% 上下文交接的真实 usage 来源未接入；不能用自报冒充硬治理。模型真实可用性和 Standard 服务状态也未验证，Codex默认按角色请求 gpt-6-terra/luna，显式选择优先。
5. 五 CLI 完整原生工作流、双 CLI 双实现混合集成、完整独立验收及集成分支/工作区交付。通过后执行已有安装授权，备份/回滚、保护生产会话；在线与业务 UAT 分开。

## 避免重复探索的已知入口

- Claude 2.1.238 本机 `--help` 明确 `--strict-mcp-config` 只使用 `--mcp-config`，`--disable-slash-commands` 为 Disable all skills；`--safe-mode` 同时禁用 MCP/hooks，不可直接替代角色隔离。禁用自动 Skills 后是否以快照指令满足合同，应依据原需求完整性决策，不能悄悄缩范围。
- OpenCode 1.18.35 帮助有 `debug config`、`debug skill`、`debug agent`；仅帮助已运行。未运行可能加载全局插件的 debug 命令。应先隔离/核对其副作用，不输出真实配置中的凭据。
- Gemini settings源码 `chunk-HQ3WN6AK.js:16181-16191,16344-16382`；核心过滤器 `chunk-JW7ZLS43.js:371267`；CLI选项 `gemini-FOEJAXHE.js:8096`，配置消费 `:8536-8537`；Skills自动发现 `chunk-JW7ZLS43.js:365163-365205`。
- Kimi loader `main.mjs:195045-195137`，Skills root `:131417-131435`，内置注册 `:131395`；实际 extraSkillDirs/pluginSkillRoots 传入在 `:216073-216074`。相对路径须按具体字段核实，不能仅因搬家推断受影响。
- Codex 官方配置文档 `https://developers.openai.com/codex/config-reference/` 已直接抓取：支持 `skills.config[].path/enabled`，不是全局白名单的证明。搜索 connector 两次返回 isError，不再同路重试；直接官方页面获取成功。

## 原生外部前提（禁止原样重跑）

native-2：Codex/Claude/Kimi停trust；OpenCode进主界面后v1.19.4更新模态，hooks原因未完整定位。Gemini原安装损坏的两次version/help失败已封顶；已有会话级完整残留包wrapper，native-4到人工trust/login，零turns/hook false，已stopped，自有daemon结束。不得原样重跑native-3/4、自动接受trust/login/更新模态。若代码完成只剩人工前提，准备确切隔离入口和动作一次说明，不只重复“等授权”。

`desktop/tests/orchestration-v2.native.cjs` 仍只观察planning，不能作为五CLI完整验收。生产安装和生产状态本次未改变；无当前在线或业务UAT新增结论。
