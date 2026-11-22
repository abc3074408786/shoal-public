# velella 方向分析：ADE 与长时间后台 Coding 任务

> 2026-10-11 · 基于当前竞品调研 + 对 Orca 实际命令面的分析
>
> **续篇**：若决定做开源产品（面向开发者、对标 Orca），见 [open-source-plan.md](./open-source-plan.md)——其中包含关键前提更新：Orca 本身已开源（40k+ stars），打法需相应调整。
> **定名**：项目已定名 **velella**（原工作名 orcas；npm/PyPI/crates.io/Homebrew/GitHub 五面实测全清空，定名战全记录见 open-source-plan.md §4）。本文中的 "orcasd" 均指 velella 的 daemon。

## 1. 命题与结论

**命题**：ADE（Agent Development Environment）这类产品的方向里，"长时间后台运行 coding 任务"是强需求，其技术核心是驱动 agent 的 CLI/TUI。

**结论（TL;DR）**：

1. 命题成立，但要加一个修正：**驱动 CLI/TUI 不是目的，"可信的无人值守执行"才是**。驱动 TUI 只是手段之一，产品的核心资产是架在驱动层之上的**归一化状态机 + 完成判定（verification）闭环**。
2. 正确的架构是**双层驱动**：PTY/TUI 作为通用底座（任何 agent、永远适用、人类可接管），结构化协议（stream-json / ACP / hooks）作为可用时的加速器。不要二选一。
3. 竞品都在做"worktree 里跑 agent + 看 diff"，普遍没做好的是：**机器可校验的完成判定、会话自愈、环境供给、成本治理**。这是新进入者的空位。
4. 建议切入点：先做"单任务的后台可信运行时"（daemon + PTY + 状态分类 + 通知 + 自动验证闭环），再扩展到并行与编排。UI 最后做，daemon 先做。

## 2. 为什么"长时间后台运行"是强需求

- **经济结构变了**：agent 的 wall-clock 很便宜，人的注意力是瓶颈。价值 = 每小时人类监督时间能产出多少个完成任务。前台阻塞式会话（IDE 聊天模型）把这个比例锁死在 1:1。
- **任务本身变长了**：多文件重构、迁移、修测试循环、依赖升级，30 分钟到数小时是常态。人守着 spinner 等待是纯粹浪费。
- **并行是自然的下一步**：1 个人 N 个 agent 并行跑任务，单元从"我坐在这里的会话"变成"我委派出去的任务"。谁拥有"可信的无人值守执行"，谁就拥有这个工作流。
- **异步阶梯**：同机后台 → 过夜任务 → 云端 → 手机上审 diff/批权限。每上一级，对"状态感知 + 完成判定 + 安全闸门"的依赖都更重。

一个佐证：本机正在运行的 ZCode（agent harness）本身就是这个模式的实现——后台 bash 任务、cron 自动化、闲时队列、任务完成通知。CLI agent 的运营化（operationalization）已经在发生，缺的是一个面向开发者的产品化载体。

## 3. "驱动 CLI/TUI"的正确打开方式：双层驱动

### 3.1 为什么必须保留 TUI/PTY 层

- **生态即能力**：Claude Code、Codex CLI、Gemini CLI、ZCode 等的能力都长在 CLI 里，skills/hooks/MCP/记忆/权限系统全在其中，且每周更新。驱动真实 CLI = 免费继承整个生态；自研 agent 运行时 = 永远追赶。
- **TUI 是功能首发渠道**：交互式权限确认、消息排队、plan mode、TODO 展示等往往先落地在 TUI，headless 模式存在 parity 缺口（已知案例：`claude -p --output-format stream-json` 存在流式响应无限挂起的 issue）。
- **TUI 是信任与接管通道**：人最终审查的是"屏幕上真实发生了什么"。接管 = attach 到同一个 PTY，无缝转人工，会话不换地方。
- **Resume 靠会话不靠进程**：Claude Code/Codex 都有自己的 transcript 和 `--resume`。daemon 崩了可以重启进程并 resume 会话——长任务的"自愈"有了抓手。

### 3.2 结构化协议层（可用则用）

| 通道 | 适用 | 说明 |
|---|---|---|
| `claude -p --output-format stream-json --input-format stream-json` | Claude Code | 双向结构化流；注意挂起 bug，需要 watchdog |
| Claude Code hooks（PreToolUse/Stop/Notification 等） | Claude Code | 精确生命周期事件，做状态机的最佳信号源 |
| `codex exec --json` / proto 模式 | Codex | 结构化执行 |
| **ACP（Agent Client Protocol）** | 通用 | Zed 发起的 JSON-RPC 标准，"agent 界的 LSP"，JetBrains 已跟进支持，2026 年生态在收敛 |
| opencode server / 各家 SDK | 对应 agent | 程序化控制 |

ACP 的收敛是个重要信号：如果它成为事实标准，"结构化层"的成本会大幅下降，适配器可以统一。但即便 ACP 全胜，PTY 层依然不可替代（接管、审计、resume、覆盖不支持 ACP 的 agent）。

### 3.3 归一化状态机（真正的产品核心）

所有驱动信号（屏幕解析、hooks 事件、ACP 事件、进程退出码）都归一到同一套状态与事件流上：

```
SessionState: spawning → running → waiting_permission → waiting_input
              → verifying → done | failed | stopped | fenced(不确定)
TaskState:    queued → running(可重入) → verifying → in_review → merged | escalated
EventLog:     追加式；UI/通知/automation/DAG 编排全部消费同一条流
```

- **状态分类器**（idle/waiting/done 检测）：输出静默 N 秒 + 屏幕哈希稳定 + spinner 消失 + 各家 prompt 模式匹配 + hooks 信号。Orca 的 `terminal wait --condition tui-idle` 证明这已是成熟产品的标配原语。
- **关键设计约束**：结构化信号只是"加速器"，屏幕永远是 fallback 与仲裁者（"人看到的就是真相"）。

### 3.4 参照系：Orca 的原语面（实测 `orca --help`）

Orca 已经收敛出的原语，基本就是这类 ADE 的"元素周期表"：

- **Terminals**：`create / read / send / wait(tui-idle) / attach` ← 你命题的技术核心，已产品化
- **Worktrees**：CRUD + `worktree ps` 跨 worktree 编排视图
- **Orchestration**：Run/mailbox/阻塞 ask-reply/task DAG/dispatch/**supervised worker 带 fencing**（`worker-abandon`：不确定时"圈栏"而不宣称已停止——分布式系统语义）/ decision gates
- **Automations**：定时触发任务
- **Accounts / Hosts / Environments**：多 agent 账号、多机远程 runtime
- **agent-context**：给 agent 用的机器可读命令 schema（agent 是 CLI 的一等公民用户）
- 附带：browser automation、computer use、iOS 模拟器、Linear、会话全文检索

解读：单任务驱动（terminal 三件套）已不是壁垒；**监督语义（fencing/gate/budget）+ 完成判定 + 生态整合**才是第二阶段的竞争面。

## 4. 真正难、也真正值钱的六个问题

1. **完成判定与自动验证闭环**。后台任务的信任引擎。Task 需要一等公民的"done-criteria"：测试绿 / build 过 / lint / 自定义脚本 / agent 自报（需交叉核验）。验证失败 → 把失败信息作为下一条消息发回 session，有界重试，超限升级为人工 gate。这是目前所有竞品最薄弱处。
2. **会话持久性与自愈**。PTY 必须归 daemon 所有而非 UI 进程（关掉窗口、断网、睡眠后任务继续）；崩溃后自动重启 + `--resume`；长任务把"worker 死亡"当作一等状态处理（fencing，别假装知道远端状态）。
3. **干预与接管**。agent 运行中排队插入用户消息；权限询问转发到手机/通知并回复；一键升级为完整 TUI 接管（同一个 PTY）。
4. **环境问题**。worktree ≠ 可运行环境：依赖安装、DB、端口、.env 每个任务都要一份。社区已有共识批评（"每个工具都建 worktree，没人解决环境"）。谁把 per-task 环境供给（setup recipe + 健康检查）做好，谁的并行任务成功率就高一截。
5. **成本与限流治理**。N 路并行烧 token：per-task 预算上限、wall-clock 上限、429 退避、任务排队。没有预算护栏的"后台海量跑"是用户不敢开的功能。
6. **安全与爆炸半径**。无人值守 = agent 拿着 shell 写权限跑几小时。最小集：worktree 隔离 + 权限分级 profile（沙箱全放行 / 敏感操作转人工 gate）+ 全量命令审计日志 + secrets 不进任务环境。进阶：容器/VM per task。

## 5. 竞争格局（2026-10）与空位

| 玩家 | 形态 | 特点 | 短板/空位 |
|---|---|---|---|
| **Orca** | 桌面 ADE | 原语最全（见 3.4），Windows 走托管 WSL，编排语义最深 | 全而重；验证闭环未见产品化；生态绑定自身 UI |
| **Warp 2.0** | 终端型 ADE | 终端原生，agent 管理 + 后台任务 | 终端中心视角，任务/工单视角弱 |
| **Conductor / Zoku** | Mac 原生应用 | 并行 worktree 跑 Claude Code，体验精致 | Mac only、单 agent 倾向 |
| **Vibe Kanban / Parallel Code 等** | 开源看板 | 多 agent、多 worktree、review 流 | 工程拼装感，环境/验证/治理弱 |
| **Devin / Codex Cloud / Jules / Terragon** | 云端 agent | 真·后台，手机可审 | 云端信任门槛、环境不可控、按订阅计费 |
| **Agent-Manager / awslabs orchestrator / tmux 系** | TUI 工具 | 轻量、auto-resume | 无产品化监督（预算/审计/环境） |
| **ACPX / ACP 生态** | 协议层 | 超越 PTY 抓屏的结构化编排 | 协议不是产品；仍需上层状态机与验证 |

**空位判断**：
- "在 worktree 里跑 agent + 看 diff"已是红海；**机器可校验的完成判定 + 自动修复闭环**是公认缺口。
- **开放、可嵌入的 headless runtime**（`orca serve` 有雏形但 app 中心化）：一个 API-first 的 "agent 任务 daemon"（类似 agent 界的 systemd/taskd），任何 UI/CI/IDE 都能接，有可能成为被别人构建其上的基础设施。
- **Windows/WSL 优先**仍是差异点（绝大多数竞品 Mac-only）。
- 多 agent 协同（"pod"式分工）人人都在早期，但它是单任务信任解决之后的第二阶段，不宜作为起点。

## 6. 建议的切入点与路线

**定位建议**：不做 agent，做**agent 的运营层**——"让任何 agent CLI 在后台安全地跑完任何任务，并把可信的结果交回给人"。产品形态：headless daemon（核心资产）+ CLI/TUI/Web 薄 UI（可替换）。

**MVP 阶梯**（每级独立成立价值）：

1. **会话守护**（~2-4 周）：daemon + 单 agent CLI 的 PTY 会话（WSL2 优先）+ attach/detach + 崩溃自动 resume + 原始字节流留档。价值："合上笔记本任务还在跑"。
2. **状态感知**（+1-2 周）：状态分类器（waiting_permission/waiting_input/idle/done）+ 通知推送。价值："它来找我，而不是我盯它"——单这一条就是大多数人的付费点。
3. **任务闭环**（+2-3 周）：Task = worktree × agent × 权限 profile × 预算 × done-criteria；自动验证 + 失败回喂 + 有界重试 + 超限转人工 gate。产出物 = diff + 验证报告 + 会话摘要。
4. **并行与收件箱**：N 路并行 + 任务收件箱/review UI + 合并流（PR 或本地 merge）。
5. **编排与自动化**：定时任务、DAG、decision gates、多 agent 协同、手机端审批。

**刻意不做**：自研模型/自研 agent、重聊天 UI、第一步就上云端、为功能表追平 Orca。

## 7. 技术架构建议

```
┌─ 客户端（可替换）：CLI / TUI / Web / 通知 / 手机
├─ velella daemon（核心，长驻；headless 模式 `velella serve`）
│   ├─ Session Manager：PTY 池（WSL2 内 Unix PTY；ConPTY 兜底）
│   │   · 环形缓冲 + 终端仿真（vt100/avt/libghostty 类；UI 用 xterm.js 可复用其 buffer）
│   │   · 原始流留档 → 会话回放/全文检索
│   ├─ Adapter 层（数据驱动，热更新）：每 agent 一份
│   │   · 能力矩阵：resume? / headless-json? / hooks? / ACP? / 权限框模式 / idle 启发式
│   ├─ 归一化状态机 + 追加式 EventLog（SQLite）
│   ├─ Verifier：done-criteria 执行器 + 失败回喂循环
│   ├─ Policy：权限 profile / 预算 / 超时 / fencing
│   ├─ Env Provisioner：per-worktree setup recipe + 健康检查
│   └─ Worktree Manager / Git 集成
└─ 集成：git provider / Linear / Slack-push / cron
```

关键实现取舍：

- **tmux 只配当原型脚手架**（`new-session -d` + `send-keys` + `capture-pane` 一个周末能跑通 80%），产品级要自持 PTY——状态检测质量和结构化事件都依赖它。
- **适配器必须数据驱动 + 每夜兼容性测试**：agent CLI 每周发版，屏幕模式会漂移。建一套对着真实 CLI 的 record/replay 兼容测试，作为 CI 资产。这是可持续维护的关键，也是别家做不好所以弃用抓屏的根因——用工程纪律把它变成可维护的。
- **结构化路径永远带 watchdog**：stream-json 挂起类 bug 真实存在，任何"等 agent 说 done"的路径都要有超时与降级到屏幕仲裁。
- **Windows 策略**：WSL2 为权威运行时（Orca 同款选择），ConPTY 作纯 Windows 兜底。

## 8. 风险与对策

| 风险 | 对策 |
|---|---|
| TUI 模式漂移导致抓屏失效 | 数据驱动适配器 + 每夜真机兼容测试 + 屏幕仲裁只做状态不做内容 |
| ACP 等标准全面胜出，PTY 层贬值 | 接管/resume/审计价值不随协议消失；同时把 ACP 实现为又一个 adapter，两头下注 |
| headless 模式 bug（如 stream-json 挂起） | 结构化路径永不作为唯一路径；watchdog + resume |
| Orca/Warp 迭代快 | 不追功能表；楔子打在验证闭环 + 开放 daemon + Windows，做它们结构上难做的（Orca 的产品引力在自家 app 生态内） |
| 无人值守安全事故 | 权限 profile 默认最严；预算/超时硬上限；审计日志；高危操作强制 gate |
| 多 agent 编排过早投入 | 单任务信任度（验证闭环成功率）未达标前不扩展编排 |

## 9. 参考资料

- [Agent Client Protocol 官方站](https://agentclientprotocol.com) · [JetBrains 对 ACP 的支持](https://www.jetbrains.com) · [Philschmid：ACP 概览（2026-02）](https://www.philschmid.de)
- [Multi Agent Orchestration with ACPX: Beyond PTY Scraping（2026-08）](https://casys.ai)
- [Orca: The Agent Development Environment（dev.to, 2026-09）](https://dev.to) · [Warp 2.0: Agentic Development Environment](https://www.warp.dev)
- [Claude Code headless stream-json 挂起 issue（2026-04）](https://github.com/anthropics/claude-code/issues) · [amux：headless 用法指南](https://amux.io/Guides)
- [Every AI Agent Tool Creates Git Worktrees. None of Them Solve the Environment Problem（dev.to, 2026-03）](https://dev.to)
- [Parallel Code vs Vibe Kanban（2026-07）](https://parallelcode.app)
- 本机 Orca CLI `--help` 实测命令面（2026-10-11）
