# 四 CLI 编排续接 M：两来源上下文观测和输入门禁已验证，继续完整交接与工作流

**整体未完成、未安装。** 本会话按用户约40%上下文规则交接。新会话创建后源会话停止源码修改。接任者亲读本文和 L；K/J/I/G 只沿明确缺口读取。不要把门禁切片当自动交接、四CLI工作流或总体验收完成。

## 用户结果、授权与所有权

父目标：自然语言 Git 目标 → 可编辑角色/步骤/依赖/验收 → 人工确认 → 隔离并行 → 独立审查 → 集成 → 独立总体验证 → 可运行、可审阅的分支/工作区。范围仅 OMP、Claude Code、Codex、OpenCode；Kimi/Gemini 保留历史，不阻塞。四CLI各需真实启动、领取、回报、审查、恢复；至少双CLI双实现角色混合集成。完整独立验收通过后已有授权受控安装，须备份、可回滚、保护生产会话。

禁止自动 trust/login/更新、全局账户更改、重启生产、PR/发布。必要人工动作先准备隔离入口再一次请求；产品已能让用户选择确认 MCP 来源，不再等唯一全局 manifest。不要交小增量后等用户再次说继续。

工作区 D:/orcas；分支 codex/integrate-collaboration-20261022；HEAD 62769b4a8ea0ec4c7f347c16726888cbaf3db64a 未变。原有 dirty/untracked/deleted 全保留；未提交，禁止 reset/clean/stash/全仓格式化。主代理独占修改、方案和最终验证；探子只读单轮 fork_turns=none，其他可选参数不传，不递归、不复用，派发立即等待。本轮无在途测试、CLI、探子。native_usage_contracts 因时间盒中断，已无执行，且没有可用终态回报。没有 FastCtx；用 rg/Get-Content。

## 本轮用户能力与明确限制

候选执行页新增“会话上下文”：按自有会话显示最近观测的 token/window、来源、时间、估算或最近API输入的区别，缺数保留未知。仅候选可用，未安装。

- 新增 crates/velella/src/collaboration/context_usage.rs：纯归一化 Receipt、40%锁存、8192字节有界文件恢复。来源由 host descriptor 的 VELELLA_PROVIDER 决定，非模型自行报字段。记录 desktop session/native session ID、provider/source/basis、counts/percent、observed_at/handoff_required；不保存原始状态栏载荷。
- OMP lifecycle 的 SessionStart/input/Stop/turn_end 会经真实 runtime hook 送 ctx.getContextUsage()。v18.12.3 官方类型明确 tokens 是 Estimated context tokens，UI标“原生估算”；窗口和比例用原生返回实际分母，不推定模型窗口。
- Claude role 的私有 claude-settings.json 保存原有 collaboration hooks，增加 statusLine command 指向本runtime context-usage；替换该会话原 --settings 路径，列入启动文件SHA绑定，不修改全局配置。归一化取 current_usage.input_tokens + cache_creation_input_tokens + cache_read_input_tokens / context_window_size，不含output、不用累计消耗。首次API前和压缩后current_usage为空时保持unknown。
- host新增 context_usage IPC、status字段和 context-usage.json；恢复读取发生在spawn前。跨native ID的直接用量请求拒绝。OMP SessionStart允许建立新的原生会话身份。原native ID的未知采样/压缩降值不能清掉已触发40%锁存。
- 40%后仅阻止后续 automated input，手动终端控制保持；正在执行的回合不强制中断。调度notify也检查，同一成员到阈值只跳过该成员，其他已上报节点继续验收。主控到阈值阻止后续新派发；24小时收敛检查仍先运行。
- **自动写交接包、转交活跃work/review/主控身份、启动新原生会话尚未实现。** UI明确“新会话交接仍需人工处理”，当前不能把这称为40%主动交接闭环。下一实现需要持久交接身份/固定基线/证据索引，不可用失败retry偷换交接或重置尝试账本。
- **Codex/OpenCode用量来源尚未接入；Claude原生statusLine实际调用未验证。** 已证明的是Claude官方schema+真实runtime statusline IPC/host合成输入；不得称真实Claude认证通过。OMP原生来源确已直接执行，但无模型请求。
- 与L一致：同OS用户native进程共享desktop/host IPC权限；用量与来源回执用于工作流追溯，不是恶意本机进程沙箱。

## 官方来源与探索结果

1. OMP v18.12.3 官方 GitHub：
   https://github.com/can1357/oh-my-pi/blob/v18.12.3/packages/tui/src/status-line/types.ts
   11–16行：ContextUsage tokens(number, estimated)、contextWindow、percent。getContextUsage()位于 packages/coding-agent/src/extensibility/extensions/types.ts；本次通过GitHub connector直接读取，blob SHA 6e2d043b62730b3c4a8abf157572e30bcb37b517。
2. Claude官方 https://code.claude.com/docs/en/statusline ，本次直接核对Context window fields，明确current_usage三项input及null语义；不需要重搜。
3. 第一个探子 usage_source_map 已定位现有repo没有usage治理。scripts/verify-desktop-real-terminal.py:405–416 只取Codex total_token_usage.total_tokens，是累计值，不可当context占比。不要复用此错误指标。
4. 第二个探子 native_usage_contracts 同时查四provider接口，超时后催收一次仍无终态，已中断；未返回可靠结论。不要原样再次宽派四provider源研究。若需要，单个provider精确取官方类型/实际native事件，以改变接入方案为边界。

## 验证和冻结

证据根 artifacts/orchestration-v2-20261025/。最终固定16文件 SHA：continuation-t-source-hashes.json；综合JSON：continuation-20261026-context-usage.json。

- continuation-s-context-tests.log：首批3个归一化/未知/持久化测试通过。
- continuation-s-rust.log：230 passed / 0 failed / 8 ignored（修复调度缺陷前，已由t替代）。
- continuation-s-frontend.log：tsc+vite build exit0，现有大bundle警告保留，之后无前端源码变化。
- continuation-s-browser/result.json：27 assertions passed；新增原生估算/40%/unknown展示，合成RPC，无真实模型。
- continuation-s-runtime.log：首轮runtime build通过，已由t替代。
- continuation-s-omp-export.log：显式ignored omp_export_native_role 1passed，生成临时合成角色配置。
- continuation-s-omp-native.log：安装版 OMP18.12.3 使用生产Rust配置/扩展/真实runtime；UserPromptSubmit带tokens2936/window1000000，与get_state一致；无模型/真实认证，输入被合成IPC拒绝。descriptor指针在 continuation-s-omp-role.json。
- continuation-s-omp-recovery.log：显式ignored omp_post_native_recovery 1passed，原生使用后启动文件摘要未变。
- continuation-s-context-host.json/.log：scripts/fixtures/context-usage-host.cjs 真实runtime host+IPC+惰性Node PTY，合成Claude statusline数据。证明>=40自动输入拒绝且input_sequence不前进、人工输入可用、host死亡/恢复锁存、跨native ID拒绝、原始payload无落盘。无CLI账户和模型。
- 独立探子 context_usage_review 核对s的14文件SHA并独立执行 native_context 4passed/0failed；发现active成员guard提前返回阻断后续reported节点。
- continuation-t-context-red.log：主代理新增双节点用例，先直接复现上述缺陷，第二节点reported而非accepted。随后将active guard失败改为continue，lead guard移到24h检查之后。
- **continuation-t-rust.log：239 total，231 passed / 0 failed / 8 ignored，47.42秒，exit0。**
- **continuation-t-runtime.log：最终runtime build exit0，9.22秒。runtime SHA256 672fc96af73014c1bef26b20e2c2786b77e241a9da3e2792eba4f19d7297b14c。**
- 新干净探子 context_scheduler_verification 核对t版v2.rs/v2_tests.rs SHA，独立执行 native_context_blocked_member：1passed/0failed/0ignored exit0，确认同一调度缺陷闭合。未独立执行全量，非总体验收。

编辑阶段有未执行的JS包装语法错误（多行patch含嵌套模板字符串），后改为无模板嵌套的较小patch；一次apply_patch因文件hunk倒序校验失败，无部分写入，按实际文件顺序后成功。不要把这些工具输入失败当运行时缺陷。调度红测试是刻意复现新缺陷后修复，不是无变化重试。

## 下一关键路径

1. 接通完整40%交接：已有观测/锁存/输入门禁不再返工，补交接包与同一任务身份的持久迁移；旧会话停止写入，新会话沿固定基线/剩余缺口/证据接任，不能重置attempt预算。实现前明确活跃任务、主控、审查的移交合同和恢复幂等性。
2. Codex/OpenCode当前context来源；Claude私有statusLine原生消费证明。缺数保持unknown，不通过估算累计tokens/伪造窗口满足验收。必要时按provider一次定向核验，不再宽搜四项。
3. 四CLI完整认证领取/回报/审查/恢复、至少双CLI双实现混合集成。旧native-3/4原样重试已封顶；准备具体隔离入口后必要人工trust/login一次提出，绝不自动确认。沿J/I/K冻结角色配置推进工作流层证据。
4. 固定候选独立总体执行验收；通过后已有授权受控安装、当前在线验证、业务UAT。当前这些阶段仍未执行。

总目标不能由本轮230/231测试、门禁切片或handoff文档替代。请继续实质研发，直到完整交付或真实需要人工输入的边界。
