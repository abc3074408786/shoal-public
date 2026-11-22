# Agent 编排 v2 — 固定候选续接（2026-10-25）

整体未完成。此交接落实用户 AGENTS.md 要求的长会话约 40% 主动转移；不代表研发或验收完成。新主会话继续实施，旧主会话停止源码修改。

## 权威与边界

亲读本文件及 docs/agent-orchestration-v2-handoff-20261025.md、-b.md。前两份说明已批准完整目标及历史，不要把其中后来已关闭的缺口再当待办。最新增量证据为 artifacts/orchestration-v2-20261025/continuation-c.json、-d.json、-e.json、-f.json，f 为本轮最后三个文件指纹；e 的 workspaces/test 指纹已由 f 取代。

目录 D:/orcas；分支 codex/integrate-collaboration-20261022；HEAD 62769b4a8ea0ec4c7f347c16726888cbaf3db64a。大量用户/其他任务脏改动，禁止 reset/clean/stash/全仓格式化及覆盖。不得重启生产、改全局账户配置、自动接受 CLI trust/login、安装、发 PR 或发布。

总目标：自然语言研发目标 → 可编辑角色/步骤/依赖/验收 → 人工确认 → 独立 Git 工作树并行 → 独立审查 → 集成 → 独立整体验证。首版 Codex、Claude Code、Gemini CLI、OpenCode、Kimi。五种真实 CLI 各完成启动/领取/回报/独立审查/恢复，再至少两 CLI、两实现角色混合集成才可声称整体完成。

主线程默认 Terra、Standard；子代理只读单轮，fork_turns=none，其余可选参数省略，不复用、不递归；派出后立即等待。主代理独占修改与最终测试。派卡前说明总目标缺口和新增用户能力；同因最多首次加一次基于新证据重试。用户原 AGENTS.md 优先。

## 已关闭增量，保持冻结

continuation-c/d：系统集成与独立验证人工门；会话预检持久回执与幂等恢复；稳定 cause_key 失败账本/跨节点同因两失败限制及跨 run 归属检查；受控 reopening 新 run 保留 accepted 前任且拒绝漂移；Gemini 会话级残留完整包 wrapper；Kimi 项目同名控制 MCP 覆盖拒绝。

continuation-e：计划预先指定 kind=integrate 的非最终节点可接管真实依赖 cherry-pick 冲突，获得 pending_imports 并核对全部来源 trailer 才能验收。普通 implement 节点保持阻断。最终 __integrate 仍从 immutable baseline 汇集所有已验收增量。真实 Git fixture 回归和独立只读审查通过；该新用例只走至最终集成冲突现场，不是整个真实模型流程验收。

continuation-f：capture_baseline 新增 source_candidate，复用快照先比原始身份和声明内容；已完成提交但未最终记账可严格补记；未知工作树内容（含 ignored）拒绝；不依赖已存 path 是否存在，无条件检查 make 返回的实际工作树；原工作区未暂存删除按源指纹含 deleted 标记、提交内容不含标记分别核验。恢复要求提交恰好单父等于捕获 HEAD，消息固定，干净状态和内容一致。复制/暂存未完成或身份不明依然保留现场并阻断。旧不完整快照缺 source_candidate 明确拒绝；已有 baseline_commit 不改写。

基线独立初审发现 ignored 文件漏检、缺绑定跳过检查，两项都已用红绿回归修复；第二轮独立源码复核确认关闭。没有独立测试执行，不要把源码审查写成独立测试实跑。

## 当前验证

- cargo test -p velella --lib：186 passed，0 failed，3 ignored。
- 六条 baseline 聚焦回归全通过；完整库含中间集成、v1 兼容及其他现有测试。
- cargo build -p velella --bin velella-runtime：通过，debug runtime 已更新。
- collaboration_commands.rs 限定 diff --check（core.whitespace=cr-at-eol）通过。
- 本轮未动前端；上一轮 npm build、v2 browser 20 assertions 通过，没有重跑。
- 所有本轮已知编译/测试进程已结束，无在途探子；未发起原生 CLI 模型工作。

## 外部证据与禁止无变化重跑

native-4 是最新原生前提证据：Gemini 会话 wrapper 可到人工 trust/login；最终 stopped、exit=1、hooks_ready=false、turns=0，自有 daemon 已不存在。不得把它当接入成功，不得原样重跑 native-3/4。Codex/Claude/Kimi 亦有 trust 门；OpenCode 更新模态/hooks 问题尚需按已保存材料定位。需要新源码或新前提才可再执行相关尝试。不要访问、打印凭据或自动刷新真实 OAuth。

## 下一条关键路径

转向尚未完整实现的能力配置：可信外部 MCP 服务器/逐工具 catalog 与各 provider 实际绑定。现有 Velella 自身控制 MCP 不是外部 catalog。先核对已有配置入口及注册来源、形成具体可审阅合同；授权内可完成解析/验证/合成 fixture，涉及选择实际可信来源或执行外部服务器才提出精确缺失决定。不要继续重复验收已关闭基线和中间集成代码。

其余仍需跟踪：四 CLI 原生 Skills catalog 约束、真实模型支持与 Terra/Luna/Standard 约束；Kimi 相对配置/OAuth 恢复；上下文 40% 产品机制的真实用量来源；接受/停止后自有会话死亡证据；五 CLI 与混合集成真实验收；最终整个限定候选独立验收及交付。没有安装/上线/UAT，不得报告总目标完成。
