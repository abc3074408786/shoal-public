# 编排 v2 续接 I：OpenCode 生命周期与恢复闭合，下一接 OMP

整体未完成、未安装。源会话因用户约 40% 上下文交接规则移交，交接后不再修改源码。接任者亲读本文、H 和 G；不必重载 A–F。下一关键路径是 OMP 实现及四 CLI 发布范围，然后完整工作流和独立验收，不要重复 OpenCode 的已通过检查。

## 用户结果与授权

自然语言 Git 研发目标 → 可编辑角色/步骤/依赖/验收 → 人工确认 → 独立工作树并行 → 独立审查固定候选 → 集成 → 独立总体验证 → 可运行可审阅分支/工作区。

用户已明确本次仅 **Oh My Pi (OMP)、Claude Code、Codex、OpenCode**，确认 OMP 身份后说“可以”；Kimi/Gemini 不阻塞本次，保留历史方案与既有实现。用户已授权继续必要研发、整体验收通过后受控安装，不能只交小增量后等“继续”。禁止自动 trust/login/更新、全局账户变更、重启生产、PR/发布。安装须固定候选、备份回滚、保护生产会话。

总体验收：四 CLI 各有启动、领取、回报、独立审查、恢复；至少两 CLI 两实现角色混合并行集成；完整独立验收通过。合成 IPC / 单测 / 源码核验不可冒充认证工作流或总体验收。

## 基线与所有权

- `D:/orcas`，分支 `codex/integrate-collaboration-20261022`，HEAD `62769b4a8ea0ec4c7f347c16726888cbaf3db64a` 未变。
- 大量原有 dirty/untracked/deleted 保留，禁止 reset/clean/stash/全仓格式化。本轮未提交。
- 主代理独占代码、取舍、最终测试；探子仅只读、单轮，显式 `fork_turns=none`，省略其余可选参数，不递归、不复用；派出立即等待。
- 无 FastCtx 可用，使用 rg/Get-Content。不能声称切换了无法控制的模型或 Fast。
- G 冻结切片继续冻结。本轮没有生产修改，没有遗留自有 CLI/测试进程。

## 本轮代码与确认的行为

修改 `crates/velella/src/collaboration/providers.rs`、`providers/skill_policy.rs`；新增 `providers/opencode_policy.rs`、`providers/opencode_lifecycle.mjs`、`scripts/fixtures/opencode-role-lifecycle.cjs`。未修改前端或 OMP；`PROVIDERS`/UI 仍旧五种，接任必须调整新建范围并保留历史读取。

OpenCode 1.18.35：

- 修复 PURE=1 跳过自身外部 lifecycle plugin：设置 PURE=0，仅显式配置生成的 file URL 插件。
- **保留内置认证插件**（DISABLE_DEFAULT_PLUGINS=0）。同tag `plugin/index.ts:72-89,170-187` 明确内置 Codex/OAuth 等认证支持，全部禁用会令已有 OAuth 无法正常工作。它们由 exe 内置导入，与私有目录外部插件发现不同。
- 工具循环之后强制 `task=deny`、`skill.*=deny`，旧方案选择 skill 不再覆写限制；所选外部 MCP 的 name_* 权限同时保留。
- 私有 XDG config/data/cache/state、TEST_HOME、TEST_MANAGED_CONFIG_DIR、CONFIG_DIR、DB、TUI_CONFIG；覆盖继承 CONFIG_CONTENT/AUTH_CONTENT/PERMISSION/PLUGIN_META_FILE/WORKSPACE_ID。路径与 flag 都纳入恢复核对。
- 私有 profile 先 owner-only ACL，再复制现有 auth.json 的 api/oauth 记录，**排除 wellknown**（会下载远端配置）；不复制组织账户数据库，不写回真实凭据。文件 schema/大小/非symlink 检查，绑定 ready/fingerprint 使半成品不能复用。真实账户认证流程尚未执行；自定义 provider 配置未继承，不能宣称所有账户组合可用。
- auth_source 目前只从 XDG_DATA_HOME 或 USERPROFILE/HOME `.local/share/opencode/auth.json` 读取，不从宿主 OPENCODE_AUTH_CONTENT 捕获；仅依赖该 inline 变量的账户仍是明确未覆盖情况。不可为补证据自动登录/打印密钥。
- lifecycle: chat.message → UserPromptSubmit；created → SessionStart；idle → Stop；deleted → Interrupt；permission.asked → PermissionRequest。事件异步异常原生不等待，不声称绝对运行时沙箱；chat.message 回调错误会抛出。子进程有 10s/1MiB 限制。
- file URL 用 Url::from_file_path 正确编码空格。Windows 支持直接 opencode.exe 与 npm shim 同prefix exe；不再仅限 npm。非Windows launcher不强制.exe，但本次只验证 Windows。
- effective skill 回执改为 `native_skill_access_denied`，不把原生目录不可用误写成完全不发现。
- **独立复核发现并修复**：OpenCode loadConfig 会补写 `$schema` 改变绑定摘要。现生成时含 `https://opencode.ai/config.json`，原生启动前后文件摘要一致，真实 Rust 恢复校验通过。

同tag主代理直接核验源码：`https://raw.githubusercontent.com/anomalyco/opencode/v1.18.35/` 下 `packages/opencode/src/config/config.ts:245-249` 为 schema 补写；`config/managed.ts:20-33` 为 managed override；`packages/core/src/global.ts:10-28` 为四XDG/home；`auth/index.ts:58-67` 为 inline/file；`config/config.ts:559-564` 为 PERMISSION late merge。不要再广搜已闭合问题。

## 最终证据（真实退出码都保留）

根 `artifacts/orchestration-v2-20261025/`：

- `continuation-j-rust.log`：221 total，**216 passed / 0 failed / 5 ignored**，exit0，47.58s。本轮最后 schema/环境/receipt 变更后全库结果。
- `continuation-j-build.log`：runtime build exit0，11.36s；runtime SHA见本轮 JSON 回执。
- `continuation-j-opencode-role.json`：生产 Rust helper 生成的角色，合成空认证及 MCP 命令，未读取真实凭据。调用的是与 configure 完全相同的 opencode_policy helper；不代表完整 preflight/TUI 工作流。
- `continuation-j-native-opencode.log`：安装版真实 OpenCode serve HTTP 创建/删除会话，消费生成的 config/plugin，实际 runtime→合成 IPC；只见所选 MCP，skill/task deny，created/deleted 回调，**全部 launch 文件摘要不变**。exit0。
- `continuation-j-native-recovery.log`：`provider_opencode_post_native_recovery --exact --ignored` 1 passed，exit0。实际使用原生启动后的描述符执行 verify_mcp_bindings。
- `continuation-j-opencode-dependency.log`：测试私有目录同版本 `@opencode-ai/plugin@1.18.35` 安装 exit0。无全局安装，--ignore-scripts --no-audit --no-fund。
- 静态独立探子 `/root/opencode_fix_review` 报一个 schema 恢复缺陷，已由主代理直接核验同tag源码并修复，再经真实原生及恢复测试闭合；这是限定静态审查，**不是完整独立测试执行或父目标验收**。
- H 里无退出码的215passed不采用。此前本轮 i 的216passed/4ignored有效但被 j 覆盖。第一次生成命令误用短名+--exact跑0项，随后修为完整限定名；0项不算通过。

失败门：首次 native HTTP 超时45s，定位到全新私有 config 自动安装 plugin依赖；单独准备同版本依赖用了45s，唯一重试通过（i-native-opencode-retry）。独立 review 后新发现 schema 缺陷，修复后的 j 验证通过，这是新源码根因的首次验证。不要原样重试旧 native-3/4。

新 fixture 使用 `serve --hostname127.0.0.1 --port0`（原生实际选择4096），只结束其自有 PID树，保留 temp 产物。首启动仍有 native依赖安装延迟，本次原生证明使用已准备依赖，不代表冷启动性能或离线能力。新 fixture 无模型/登录，不证明 idle/chat.message 已真实执行。

## OMP 已确认的新事实，直接继续实现

本机 `C:/Users/developer/AppData/Local/omp/omp.exe`，help再次直接验证18.12.3。完整help支持 --append-system-prompt、--config可重复、--no-skills、--no-extensions（显式-e仍可用）、--no-rules、--tools、--service-tier=none、--session-dir、--mode rpc、--approval-mode(always-ask等)。不要默认yolo。

本轮探子 `/root/omp_adapter_sources` 仅核验官方 `can1357/oh-my-pi v18.12.3`，主代理补读基础 config-usage 和关键源码。以下是**源码事实，不是原生通过**：

- `packages/coding-agent/src/config/model-settings.ts:77-82`：overlay `disabledProviders` 字符串数组；项目数组替换global，但CLI --config更高。其配置名关的是发现来源，不是模型Provider。
- `src/discovery/index.ts` 注册：agent-plugins、agents-md、claude-md、custom-context-files、builtin、builtin-defaults、claude、claude-plugins、cline、agents、codex、cursor、gemini、opencode、github、mcp-json、omp-plugins、skillshare、ssh、vscode、windsurf。文件名未必等于provider ID，MCP已确认的IDs见下一项。
- MCP外来源IDs已确认：`agent-plugins,claude,claude-plugins,codex,cursor,gemini,opencode,vscode,windsurf,omp-plugins,mcp-json`；native ID确为 `native` (`discovery/builtin.ts`)。
- **不要错误地禁 native 后期待直接mcp.json仍加载服务器**：`mcp/config.ts:103-182` 先读user mcp.json只是用于disabled/forcedEnabled名单，服务器实际全部来自 `loadCapability(mcp)`！必须保留私有native source，禁foreign/plugin/root fallback，并 `mcp.enableProjectConfig=false` 去重前排project。全部provider穷举仍缺cline等是否还有MCP注册，要定向补而非重扫全仓。
- `docs/config-usage.md` 前140行主代理已读：PI_CODING_AGENT_DIR仅default profile native/runtime/settings，**不改变generic getConfigDirs根**；PI_CONFIG_DIR是另一根；named profile会统一native+generic但忽略PI_CODING_AGENT_DIR。foreign与project不随profile隔离。OMP_PROFILE空字符串优先覆盖PI_PROFILE并选default。启动前需明示覆盖继承profile/overlay路径，否则隔离失效。
- `packages/utils/src/dirs.ts:942-945` getAgentDbPath(agentDir) → agent.db；Windows无需XDG分支。认证在SQLite `auth_credentials`，不是JSON。
- `packages/ai/src/auth/sqlite-credential-store.ts` 建表：`id INTEGER PRIMARY KEY AUTOINCREMENT, provider TEXT NOT NULL, credential_type TEXT NOT NULL, data TEXT NOT NULL, disabled_cause TEXT DEFAULT NULL, identity_key TEXT DEFAULT NULL, created_at INTEGER NOT NULL DEFAULT(unix epoch), updated_at ...`。active查询只读 disabled_cause IS NULL。可考虑 owner-only私有目录+只读SQLite事务仅复制认证表，不复制global settings；此方向尚未实现/核验，不能用整库文件复制覆盖WAL或导出打印密钥。直接使用真实agentdir会有设置写回风险，不建议凭推断复用。
- 扩展 `types.ts:1648-1685` 支持session_start/before_agent_start/agent_end/session_shutdown；`loader.ts:311-324` pi.exec/getActiveTools/getAllTools/setActiveTools；ctx.sessionManager.getSessionId()。`runner.ts:1552-1638,2180-2195` before_agent_start 错误通常 emitError后返回undefined，不能凭throw报fail-closed；shutdown限2s。需实际扩展fixture验证生命周期/工具选择。
- 原CLI help有 `omp auth export/import`，未执行；遵守不自动login/export secrets。账户复用需受控方案，不能拿空profile冒充已认证。
- 探子未把认证源/发现来源穷举做完，报告为partial；不要把源码建议冒称原生结果。

## 尚未完成的真正缺口

1. OMP provider接入（program/preflight/role/config/MCP/selected Skills/lifecycle/auth/recovery），四CLI新建范围与历史兼容。
2. Codex Skills原生有效目录证明；可信MCP目录来源；真实40% usage来源（G指针仍有效，不能自报冒充硬治理）。
3. 四CLI认证全工作流、双CLI双实现混合集成、完整独立验收。
4. 通过后受控安装、在线核验与业务UAT（未授权自动trust/login/更新；确有人工前提时准备具体隔离入口一次请求）。

接任后直接推进OMP实现。源会话本次没有提供新生产能力，但闭合了真实OpenCode生命周期/恢复关键缺口。不要再用治理文档或宽泛调研替代实现，也不要安装未通过总体验收的候选。
