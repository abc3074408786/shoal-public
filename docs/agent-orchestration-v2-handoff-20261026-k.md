# 四 CLI 编排续接 K：Codex Skills 启动/恢复边界已验证，继续 MCP 来源与总体验收

**整体未完成、未安装。** 用户在本根会话再次说“继续”。本轮已主动解除上一个接任会话停在“请指定唯一可信 manifest”的假阻塞，并完成 Codex Skills 的替代实现。达到约40%上下文交接点，创建接任聊天后源会话停止源码修改。

接任者亲读本文和 J；J 已包含 OMP 当前实现与证据。I/G 的冻结切片按明确缺口索引查阅，不需重新广读 A–H。

## 用户结果、范围、授权、所有权

父目标：自然语言 Git 目标 → 可编辑角色/步骤/依赖/验收 → 人工确认 → 独立工作树并行 → 独立审查固定候选 → 集成 → 独立总体验证 → 可运行可审阅分支/工作区。

用户明确本次仅 Oh My Pi(OMP)、Claude Code、Codex、OpenCode；Kimi/Gemini 保留历史，不阻塞本次。四 CLI 各需启动/领取/回报/审查/恢复，至少两种 CLI 两个实现角色混合集成，完整独立验收后受控安装。已有授权继续研发及验收，通过才安装，不能交小增量后等用户继续。

禁止自动 trust/login/更新、全局账户更改、重启生产、PR/发布。安装要固定候选、备份回滚并保护生产会话。人工前提只在准备好明确隔离入口/动作后一次提出，不能把偏好或可由产品承载的配置选择抬成全局阻塞。

工作区 `D:/orcas`；分支 `codex/integrate-collaboration-20261022`；HEAD `62769b4a8ea0ec4c7f347c16726888cbaf3db64a` 未变。大量原有 dirty/untracked/deleted 保留。本轮未提交。接任主代理唯一写入、取舍和最终测试；探子只读、单轮、fork_turns=none、不递归不复用，派出立即等。无 FastCtx，使用 rg/Get-Content。不虚报模型或Fast控制。本轮无自有在途CLI/测试/探子。

前接任聊天 `01a1782e-8342-7c02-a30c-e0e2a38fae0d` 已 idle/completed，无写入；它的探索配置已撤回至J基线。本轮根会话重新接任唯一写入者后才修改。

## Codex 新事实与已实现方案

旧方案 `skip_host_skill_discovery` 不可用：extension registry 无skill contributors时保留host discovery。前会话已失败，不得再原样尝试。

当前真实 desktop exe：`C:/Users/developer/AppData/Local/OpenAI/Codex/bin/2993767bd1dfb009/codex.exe`，仍 `0.164.0-alpha.11.1`，SHA256 `1103e9e09c71fa2cd0afd6d8f3b9a2d63174fe8efd7cb5bd09f1cb14ef58a098`。旧 `8925276705115dea` 路径不存在；不要将目录轮换误判版本不同。

新增 `crates/velella/src/collaboration/providers/codex_skills.rs`、`scripts/fixtures/codex-skill-denials.cjs`；修改 providers.rs 和 skill_policy.rs 的 Codex 分支。其余冻结切片未改。

- `app-server initialize → skills/list(forceReload:true)` 只读实际目录，不创建thread/模型/登录/信任操作。只转发角色已有 `-c/--config/-p/--profile`，不转发prompt/resume。
- Windows owned Job、10秒总期限、单记录1MiB/总4MiB边界；停止并回收自有进程树。原始native输出不进日志/回执；只保存path/name/enabled。
- 依据实际目录，去重name生成 `skills.config=[{name=...,enabled=false}]` SessionFlags。所有原生 user/project/bundled/plugin目录项都禁用，选定Skill完整冻结正文放入developer_instructions（与Claude/OMP的完整上下文方案一致，不保留原生slash UI）。
- **列出不等于启用**：原生 `skills/list` 会保留disabled项，必须检查 enabled=false。回执持久记录排序后的path/name/false列表；启动第二次查询及恢复强制reload严格比较，新增路径、启用项、发现errors或读取失败均拒绝。
- 使用name而非path的原因是实测本机289项，path规则31041 UTF16字符，name规则11292；name禁用同名所有项，实际path集合仍绑定。Windows按quote/backslash/UTF16保守估算总命令<28000，为后续activity hooks留空间；过长明确拒绝并要求减少Skill正文/MCP数量，不截断。此限制仍是产品可见限制。
- 恢复同时验证完整正文文件SHA、资源快照SHA和最后生效的developer_instructions配置值。
- 当前receipt明确 `native_catalog_disabled_at_launch_and_recovery; running_discovery_changes_unverified; authenticated_workflow_unverified`。**没有声称运行中动态新增发现被持续隔离，也没声称恶意进程文件系统沙箱。** 整体真实认证/模型回合/显式技能调用仍需工作流层证据，不要把目录探针当整体通过。

源码语义由只读探子 `/root/codex_skill_disable_semantics` 定向核验官方 `rust-v0.164.0`：

- `codex-rs/config/src/skills_config.rs:19-29,99-134,158-194`：path/name+enabled，User和SessionFlags后者优先；invalid selector忽略。
- `codex-rs/ext/skills/src/host_service.rs:350-372`：所有合并host entries计算disabled_paths；`app-server/src/request_processors/catalog_processor.rs:23-69,526-603`：skills/list enabled由disabled_paths推导，forceReload清缓存。
- `ext/skills/src/catalog.rs:276-287`、`tools/list.rs:95-99`：模型可见要求enabled&&prompt_visible；`skills/src/selection.rs:63-93,149-198`：显式结构化路径/文本提及也检查enabled。
- 这是同release源码核验，目标alpha原生目录行为另有直接实测；没有跑真实模型。

## 本轮最终证据

根 `artifacts/orchestration-v2-20261025/`；全部测试保留真实退出码：

- `continuation-n-codex-skill-denials.log`：目标exe按path禁用user/project/bundled目录、出现new path可检测，exit0（探索证据）。
- `continuation-n-codex-skill-names.log`：实际采用的name方案同样通过，exit0。`--inspect-current`仅打印289/31041/11292统计，不打印账户或完整真实目录。脚本每次创建合成temp，current模式不写真实workspace。
- `continuation-o-provider-tests.log`：30 passed / 0 failed / 5 ignored，exit0。
- `continuation-o-codex-native.log`：明确ignored native helper测试1passed，exit0。
- **`continuation-p-codex-role.log`**：扩大的 `codex_native_denials_and_recovery --exact --ignored` **1passed**，exit0。生产 public providers::configure 路径，临时私有HOME/CODEX_HOME，无真实凭据；技能完整正文参数、MCP绑定、完整verify_mcp_bindings、原生disabled目录与新增路径拒绝一并验证。
- **`continuation-p-rust.log`**：229 total，**221 passed / 0 failed / 8 ignored**，exit0，46.17s。
- **`continuation-p-runtime.log`**：最终 runtime build exit0，14.06s。最终源码/runtime指纹在 `continuation-20261026-codex-skills.json`。
- 独立静态探子 `/root/codex_skill_policy_review` 限定范围未发现可复现源码缺陷；明确未运行测试/真实模型。因此这是源码静态复核 + 主代理执行验证，**不是独立测试执行/技术总验收**。
- 本轮无前端修改，沿用J的frontend/browser证据；其余四CLI切片保持。没有安装/生产会话改动。

## 下一步直接推进：MCP 来源做成可使用的产品功能

前接任停下要求用户先指定唯一可信manifest，这不是必须停住所有研发的理由。用户再次说继续；主代理判断：**让用户在产品界面选择并确认来源，记录来源路径/摘要并在保存和恢复核验**，不替用户把发现的配置或任意command授信。路径选择属于实际使用流程，可以先实现清晰、可审阅的功能。

这是接任可执行的推荐方向（主代理工程决策，不冒称用户已选具体文件）：

1. 默认用户可管理的本地目录可为空；明确manifest格式/版本/entries。UI提供载入来源、预览命令与工具、显式确认/选择。可复用当前表单样式，不扩大视觉设计。
2. 将来源路径、内容digest、entry身份随计划与冻结descriptor保存；保存/启动/恢复重读并核对。只有匹配目录条目且来源已明确认可的项才称“目录来源已核验”；手填/旧计划保持“自定义或未验证来源”，不能继续无证据称“可信”。
3. 不读出或保存令牌，不自动信任任何本机CLI配置。既有来源若能够明确证明用户认可可使用；否则保留待确认候选而不是替用户批准。
4. 保留旧计划兼容及冻结的MCP relay，不用新来源字段反复返工已过代理实现。新建与旧计划的安全/迁移策略要明确、可测试。

后续顺序：可信MCP来源闭环、真实usage（OMP get_state有contextUsage，但产品40%尚未接入）、四CLI完整认证领取/回报/审查/恢复、双CLI双实现混合集成、完整独立验收，通过后受控安装。必要人工trust/login需准备具体隔离入口一次请求，旧native-3/4失败封顶不能原样重跑。

请直接实现下一个关键缺口。不要再次询问全局唯一manifest路径，也不要重搜或重测已通过Codex目录方案，除非有新的可复现缺陷。
