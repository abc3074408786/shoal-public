# 2026-11-02：真实原生 CLI 验收与空闲状态修复

用户现在可在已初始化的真实 Codex/Claude 会话上取得按请求执行回执、重读同一 UUID 而不重复发送，并搜索原生用户/助手文本。当前 Codex 0.166.0 首条消息前不创建原生会话，首轮初始化仍需原生终端操作；首条消息全自动发送及完整 Orca CLI 对齐未完成。

本轮真实文本样本验收通过，并修复了实测发现的 Claude 空闲通知阻塞后续 CLI 输入问题。固定候选不因 UAT 重建，只有这一处已复现源码缺陷重新打开；运行时修复只改 host.rs。

| 层级 | 实际结果 |
| --- | --- |
| 实现 | 非 initial 的 idle_prompt 仅保留已完成原生轮次、已连接钩子、已绑定 native ID、没有人工控制且已通过原有输入 guard 的 waiting_input；不会从其他状态制造就绪，agent_needs_input 保持保守处理 |
| 开发验证 | 红测明确复现原问题；修复后 9 项 host、2 项执行回执、35 项 terminal_io 共46 项相关回归通过；release 构建通过 |
| 独立核验 | 独立静态核验 host.rs 未发现本次范围内阻断；另一只读 Agent 独立核对真实原生记录、回执关联、代码/产物、当前进程/协议、正式数据及清理结果，未发现阻断；未独立重新请求模型 |
| 技术验收/集成 | 新候选12 组隔离集成通过：执行回执5 组、核心 CLI/后台7 组；正式切换前后保留条件通过 |
| 真实模型 UAT | Codex 0.166.0 选择 gpt-6.1-terra/low/默认服务层；Claude 2.1.238 选择 claude-sonnet-5/low、Fast 关闭。实际原生 assistant 记录与唯一测试文本一致，Stop 回执确认，UUID 重放没有新增对应 user 记录；原生搜索各命中 user/assistant 两条且 complete:true；工具调用0 |
| 修复真实复验 | 新 runtime 的 Claude 完成回复、自然产生 idle_prompt 后仍 waiting_input；后续无 Enter 的草稿被接受，未新增模型轮次 |
| 部署 | 干净131 文件源码仅 host.rs 相对上一线上基线变化；86 个程序文件仅 runtime 改变，用户 CLI 保持原9f1b…版本。两个快捷方式已切换到修复目录；GUI 原先关闭并保持关闭，无数据库迁移 |
| 当前在线 | daemon PID 5328、submission_tracking:1 和 runtime hash 经主线程及独立核验。45 个会话、4 个布局及 schema v3 与切换前快照一致；45 份历史前缀、4 个账户文件、2 个应用配置文件一致；7 份 SQLite 已备份，切换时8 个受保护进程身份保留 |

## 真实验收方法与边界

测试使用独立 Velella profile、空项目和原生 home，正式会话未收到测试提示词。只私有复制既有认证，未编辑源账户或配置；未继承源 Codex 的 Priority、高推理设置、原有技能、MCP 或全局钩子。运行时自己的观察钩子保留。Codex 禁用本次启动的共享服务器，Claude 禁用内置工具，均要求只回复唯一文本、不调用工具。

Codex 首次审核13 个钩子时，逐项确认只有冻结 runtime 的 hook/activity-hook，使用正常原生审核界面，未用信任或权限绕过参数。仅信任测试空目录和私有配置；首次初始化另发送一条最小消息后，才验证受保护的 CLI 发送。Claude 的首次配置/目录信任同样只作用于私有测试目录。

共4 条真实文本提示词：Codex 初始化1 条、受保护提示词1 条、修复前 Claude 1 条、修复后 Claude 1 条。同 UUID 重读、原生搜索和修复后的草稿不新增模型提示词。完成判断来自原生 assistant 消息，而非终端回显、重复的事件消息或单独的 turn_started 字段。客户端通用 receipt 的 completion_verified 仍为 false，测试业务结果由原生记录另行核对。

真实 Codex 与修复前 Claude 在09c997…基线验收；只改 host 空闲通知分支后，Claude 在a4bd44…候选重新完成真实回执/搜索/防重放/自然空闲复验。未为这一处 Claude 修复重复请求 Codex 模型；相关受保护输入反例和核心兼容通过单测/集成补齐。不把混合阶段的证据误称为全部组合均重跑。

验收期间，正式环境有一个不属于测试的 PowerShell 会话并行新增，总数44→45，布局3→4。原44 个会话 document 未变，所有测试会话仍只登记在私有 profile；本次没有恢复旧快照覆盖并行工作，切换以最新45 个会话为基线。

测试自有会话、宿主/MCP、私有 daemon 均已停止，5 份临时认证/配置副本已删除；原生测试记录和脱敏回执保留于 owner-only 的 private 目录。独立核验确认这些副本不存在、匹配的测试进程数为0。

## 实测缺陷与修复

Claude 真实 Stop 后已 waiting_input、turns=1，随后 Notification idle_prompt 将旧实现改成 interrupted。下一次不带 Enter 的 CLI 草稿被拒绝，input_sequence 未变，确认不是上游模型或账号失败。

修复保留已经成立的安全等待状态，并继续复用 require_input_ready 检查协议、退出状态、权限状态和序号。启动中、运行中、权限菜单、待处理输入、中断状态，以及有人为输入、缺 native ID/钩子/完成轮次/协议能力时，都不能借空闲通知取得就绪。initial 提示词专用逻辑保持原样。

## 固定产物与使用

- 当前程序目录：`D:\orcas\artifacts\velella-cli-real-uat-20261102\idle-fix\desktop`。
- 当前 runtime SHA256：`a4bd443ac6f363c8e6441cc2a6e9ae19afb41e894dc0014313176e9a521b969b`。
- 继续使用的 CLI SHA256：`9f1ba6c1e63883dc548c7003b0eb70494d67554937cd4bae67bc427f6931dc21`。
- 修复 host.rs SHA256：`b97d86b331137ee514c3975b48291483abd3e6f230f25ebd795a879f823b0669`。
- 原程序目录及上一 runtime 保留，SQLite 备份在 idle-fix/deployment-private，均不应拿旧数据库覆盖新写入。

使用前先在原生终端完成目录/钩子审核，并确认会话已有 native_id 且等待输入。Codex 0.166.0 的新空会话尚无 native_id 时，先手动完成一次原生初始化消息；不能靠重复创建或重复发送绕过。

```powershell
velella-cli terminal create --workspace <工作区ID> --launch codex --track-submission --json
velella-cli terminal show --workspace <工作区ID> --terminal <会话ID> --json
velella-cli terminal send --workspace <工作区ID> --terminal <已初始化会话ID> --text-file prompt.txt --enter --request-id <UUID> --wait-submit 10 --json
velella-cli terminal receipt --workspace <工作区ID> --terminal <会话ID> --request-id <同一UUID> --json
velella-cli search "关键词" --path D:\my-project --json
```

本轮证据仅支持所列原生版本、账户路由与文本样本，不替代全部模型/多账号、工具/浏览器业务、长任务或长期稳定性验收。首次初始化自动化、持久全文索引及完整 Orca CLI 对齐仍未完成。

证据：`D:\orcas\artifacts\velella-cli-real-uat-20261102` 下 CONTRACT.md、real-uat.json、baseline-real-results.json、claude-idle-reproduction.json、idle-regression-red.log、idle-fix-real-validation.json；idle-fix 下 source.json、host.diff、independent-static-review.json、clean-runtime-submission.json、old-cli-new-runtime-core.json、deployment.json、independent-final-verification.json 和 final-manifest.json。
