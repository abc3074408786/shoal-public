# 四 CLI 编排续接 L：MCP 来源功能已验证，继续 usage 与真实工作流

**整体未完成、未安装。** 本会话按用户 AGENTS.md 约40%上下文规则交接。接任聊天派发后，本会话停止源码修改。接任者亲读本文和 K；J 的 OMP、I 的 OpenCode、G 的原始边界按具体剩余缺口读取，不重跑已冻结切片。

## 用户结果、授权与基线

父目标：自然语言 Git 目标 → 可编辑角色/步骤/依赖/验收 → 人工确认 → 隔离并行 → 独立审查 → 集成 → 独立总体验证 → 可运行可审阅分支/工作区。本次仅 OMP、Claude Code、Codex、OpenCode；Kimi/Gemini 保留历史读取，不阻塞。四 CLI 各须真实启动、领取、回报、审查、恢复；至少双 CLI 双实现角色混合集成；完整独立验收通过后已有授权受控安装。不要交小增量后等待用户再次说继续。

禁止自动 trust/login/更新、全局账户修改、重启生产、PR/发布。安装需固定候选、备份回滚、保护生产会话。必要人工前提先准备具体隔离入口和操作，一次提出；不再要求用户先指定全局唯一 MCP manifest。

工作区 `D:/orcas`，分支 `codex/integrate-collaboration-20261022`，HEAD `62769b4a8ea0ec4c7f347c16726888cbaf3db64a` 未变。大量 dirty/untracked/deleted 保留；本轮未提交。禁止 reset/clean/stash/全仓格式化。接任主代理唯一改写者，负责方案与最终测试；探子只读单轮 `fork_turns=none`，不递归、不复用，派发后立即等待。无 FastCtx；用 rg/Get-Content。本轮无遗留测试/CLI或在途探子。

## 本轮闭合的用户能力

角色界面的“外部 MCP”现支持填写本地 JSON 目录绝对路径 → 载入预览 → 展示命令/参数/摘要 → 勾选工具 → 明确确认来源并添加。载入不执行服务器、不扫描/导入任何原生账户配置；默认没有来源、没有已选工具。

Manifest 形式：`{"version":1,"entries":[{"id":"docs","command":"docs-mcp","args":["--stdio"],"catalog":["search","read"]}]}`。最多 1 MiB、64 条目；拒绝未知字段（包括 env/凭据字段）、重复 ID、非法名称/工具和非普通文件。格式错误不回显原始 JSON。参数仍是用户提供的命令参数，UI明确不要填凭据；没有自动检测任意字符串是否为秘密的保证。

新增 `crates/velella/src/collaboration/mcp_catalog.rs` 与 `desktop/src/McpCatalogPicker.tsx`。接入文件完整 SHA 见 `artifacts/orchestration-v2-20261025/continuation-r-source-hashes.json`（11文件）。

- `McpServer.source` 可选且 None 不序列化，保持无来源旧角色/旧 fingerprint 字节形状；手填/旧计划明确标为“自定义配置 / 未核验来源”。没有强制迁移旧计划。
- 来源记录 canonical path、原文件 SHA256、entry ID、confirmation ID；确认记录持久化在 Hub 的 `mcp_source_confirmation` 对象，包含**实际勾选 tools**。普通 `collab.*` MCP不能签发来源确认；计划提交/保存/模板/确认/变更审批核对 DB 记录。工具可缩小，不可复用确认记录扩权。
- `orchestration.mcp.preview` 只读取文件；`orchestration.mcp.confirm` 再读摘要并持久化确认。预览后文件改变会拒绝；同 request ID 改来源或工具也拒绝。
- 文件 I/O在 Hub DB 锁外。保存、计划确认、变更审批先核对文件；启动 `providers::validate` 再读；冻结 descriptor 包含 source/catalog，并在外部代理启动/恢复 `external_descriptor` 重读来源。source path/digest/entry 绑定 id、command、args、catalog，不能编辑成另一条命令。选中工具仍沿用既有 relay allowlist。
- UI源命令/ID/参数/声明目录只读；增工具或来源文件变化需重新载入确认。按钮会用同 ID 新配置替换该角色已有条目，需保存/方案确认后生效。

## 独立复核与边界

第一位静态探子 `/root/mcp_source_independent_review` 核对11文件 SHA，发现确认回执未绑定所选 tools；本主代理修复并加扩大工具拒绝/缩小允许检查。第二位干净探子 `/root/mcp_tool_confirmation_verification` 核对修正版目标文件，确认缺陷闭合，并独立执行定向测试 **6 passed / 0 failed / 0 ignored，exit0**。

另一个发现是现有桌面权限边界：native CLI 与桌面同 OS 用户，能直接访问 owner-only daemon IPC。`desktop.rs:232-237`、`ipc.rs:95-105,200-205` 已主代理亲核。**来源确认是工作流追溯，不是防恶意同用户进程伪造界面请求的安全沙箱。** 源码已明确注释；不要继续说“任何 Agent 都不能伪造确认”。普通已认证 collab MCP API 不能创建该回执，但有 shell 的同用户进程能走桌面 management API。此为现有全部桌面操作的共享权限边界，本轮没有扩张为 IPC/OS 沙箱重构。整体技术验收须保留此边界，不把切片通过说成恶意代码隔离通过。

## 最终有效证据

证据根 `artifacts/orchestration-v2-20261025/`，综合回执 `continuation-20261026-mcp-source.json`；最终固定源码 `continuation-r-source-hashes.json`。

- `continuation-r-rust.log`：`cargo test -p velella --lib`，234 total，**226 passed / 0 failed / 8 ignored**，exit0，50.45秒。
- `continuation-r-frontend.log`：tsc+vite，exit0。保留现有大bundle警告，无新编译错误。
- `continuation-r-runtime.log`：runtime build exit0，7.99秒；runtime SHA在综合回执。
- `continuation-r-browser/result.json`：实际 A WorkspaceApp+v2 UI、隔离合成 RPC **25 assertions passed**，无真实模型/运行时。新增检查：载入不确认、工具默认不选、来源变化拒绝、来源字段只读、来源摘要/单个所选工具保存。包含四providers、旧版入口、审批/证据/响应宽度等原回归。未导出截图。
- 第二位探子独立定向测试：6 passed / 0 failed（消息回执，综合JSON记载），不是整体四 CLI 独立验收。

早期 q 浏览器失败仅因测试将 summary 当 button 点击；改为实际 summary selector 后一次重试通过。之后 r 重测是基于新的可复现 tools 授权缺陷修复，不是无变化重试。首个文件编辑脚本使用旧Python不支持的 write_text(newline) 失败且未写入；修正一次成功，相关缺字段编译错误已消除。q证据由r覆盖，不要再次原样重试这些已解决问题。

## 下一关键路径（直接继续）

1. **真实 usage 来源及40%治理接入。** J已知 OMP `get_state.contextUsage(tokens/contextWindow/percent)`，但尚未产品接入；其余CLI需要按实际可用来源定向核验，缺数据不能推算成已知。不要因MCP配置偏好停住研发。
2. 四CLI各自认证后完整领取/回报/审查/恢复；至少双CLI双实现混合集成。K的Codex Skills目录方案已冻结，不再重跑失效 `skip_host_skill_discovery`；旧native-3/4失败封顶不原样重试。真实账户/用户trust/login前提留在相应工作流层，准备具体隔离入口后一次请求，不能自动同意。
3. 固定候选的独立总体执行验收，然后已有授权受控安装、当前在线核验及业务UAT。此轮没有改生产，没有开始这些后续阶段。

MCP来源目录现在是用户实际使用时可选择/确认的产品功能，研发不再等待唯一全局manifest。不得替用户认可任意已有本机配置；测试合成目录只能证明机制，不能宣称某真实外部服务已获信任或已可用。
