# velella 开源产品方案：面向开发者的 ADE 运行时

> 2026-10-11 · 承接 [ade-direction.md](./ade-direction.md) · 前提：目标用户是开发者，对标并超越 Orca，开源发布
> 项目定名 **velella**（原工作名 orcas，定名依据见 §4）

## 1. 前提变化：Orca 已开源且已成势

调研确认：Orca 已在 GitHub 开源（"the ADE for working with a fleet of parallel agents"），**40k+ stars**，桌面/移动/远程 runtime 全平台，自带订阅接入，20+ agent 支持。社区、发版节奏、生态位都已站稳。

这推翻了"开源即差异"的假设——在这个品类里**开源是门槛不是卖点**。也因此，"做一个更好的 Orca"（功能表对齐 + 更好用）几乎必输：你面对的不是闭源公司的迭代速度，而是一个 42k 星社区的网络效应。

**结论：不做"开源版 Orca"，做这个品类还缺的那一层。**

## 2. 战略定位：app-first 之外，runtime-first 是空的

Orca 的结构是 **app-first**：桌面应用是引力中心，CLI/远程/移动都长在 app 上（`orca serve` 只是辅助）。这留下一个结构性空位：

> **headless 的、repo 原生的、可被任何 UI/CI/脚本驱动的 agent 任务运行时**——"agent 任务的 systemd"。

一句话定位（README 第一行候选）：

> **Run coding-agent tasks in the background, verify them automatically, get pinged only when needed. A daemon + TUI that drives the CLIs you already pay for.**

与 Orca 的分工叙事：Orca 是驾驶舱（cockpit app），velella 是任务运行时（task runtime）。驾驶舱可以有很多个（Orca、Warp、编辑器插件、纯 TUI、CI），运行时应该只有一个标准。

## 3. "比 Orca 更好"的正确解法：结构性差异，不是功能表

| 维度 | Orca（结构使然） | velella（差异化主张） |
|---|---|---|
| 架构中心 | 桌面 app 为引力中心 | **daemon-first**，UI 可选可换；`velella serve` 可独立部署在服务器/NAS/CI |
| 任务定义 | app 内配置状态 | **任务即代码**：`.velella/tasks/` 进 git，可 review、可分享、可在 CI 重放 |
| 完成判定 | agent 自报 + 人看 diff | **verification-first**：done-criteria 引擎（测试/build/lint/自定义），失败回喂有界重试，超限转人工 gate |
| 可编程性 | CLI 面向 app 操作 | **开放协议**：文档化 daemon API（JSONL/WebSocket event stream），任何 UI 可接，CI 一等公民 |
| 信任 | 需信任发行版 | **可审计**：权限/沙箱策略代码开源；本地优先，无中间服务器 |
| 扩展 | 官方节奏 | **adapter SDK**：社区按周跟上 agent CLI 发版 |
| 自托管"云 agent" | 绑定其远程 runtime 方案 | 自己的服务器就是自己的 Terragon：daemon 暴露远程 runtime + pairing |
| UI 形态 | 桌面 GUI | **TUI 旗舰**（开发者住在终端；lazygit/k9s/btop 的路线）+ Web 后置 |

**v1 刻意不做**（Orca 已做好的、结构上属于 app 层的）：浏览器自动化、computer use、iOS 模拟器、移动端。这些是插件位，不是核心。

## 4. 定名：velella（2026-10-11 定）

**意象**：velella（帆水母，*Velella velella*）不是一只动物，而是一个群落——成千上万个水螅体各司其职（帆、浮漂、捕食、繁殖），却作为一个单一生命体，仅凭风力在远洋自主航行数月。这是多 agent 协同的完美隐喻：一群分工的 agent 组成一个"任务生命体"，在你不看海的时候自己横渡大洋。tagline 候选：*"A colony of agents, sailing while you sleep."* 中文气质：维莱拉，轻而稳。

**可用性实测**（2026-10-11，五面全清，本项目迄今最干净的名字）：

| 面 | 结果 |
|---|---|
| npm | **空闲** |
| PyPI | **空闲** |
| crates.io | **空闲** |
| Homebrew | **空闲** |
| GitHub | 仅 8 个同名仓库，最高 1★，无任何开发工具 |

**命名资产约定**：CLI 二进制 `velella`；headless 模式 `velella serve`（不设独立 daemon 二进制）；配置目录 `.velella/`（任务放 `.velella/tasks/`）；GitHub org 建议 `velella-dev`；域名 `velella.dev` / `velella.run` 待购（非阻塞）。*Velella velella* 是生物学双名，难以被独占为通用商标，对开源使用友好。

**淘汰过程**（2026-10-11 定名战全记录）：首版 nightpod（当时五面全清）因音感不佳被否；moonpool（npm/PyPI/brew/GitHub 清空、crates.io 被活跃库占用）经用户复审后换下；shoal 经数据评估否决（npm 占用者为同类 process manager、GitHub 有 248★ 同名 autonomous agents 项目、词义含"浅滩/搁浅"）；landfall 因知名游戏工作室 Landfall Games 出局；`dorado`/`pelorus`/`mooring`/`baleen`/`thalassa`/`canopus`/`nocturne`/`vesper`/`vigil`/`halcyon`/`boreal`/`orrery` 等 20 余个候选均有注册表占用；`pelago`/`moondock` 五面全清，留作备选。

## 5. 核心资产：两个"公共标准"+ 一个引擎

开源 infra 项目赢在定义格式与协议。这个项目应该公开拥有的：

1. **任务格式**（repo 原生，可进 CI）：

```yaml
# .velella/tasks/upgrade-vite.yaml
name: upgrade-vite-6
agent: claude-code            # adapter 名
base: main
worktree: auto                # 每 task 一个 worktree
env: .velella/env/setup.sh    # per-task 环境供给（解决"worktree ≠ 可运行环境"）
policy:
  permissions: ask            # sandbox | ask | allow
  budget: { tokens: 5M, wall_clock: 2h }
done:                         # 机器可校验的完成判定
  - run: pnpm test --run
  - run: pnpm build
  - diff_max_files: 30        # 防失控护栏
verify: { retries: 2 }        # 失败信息回喂 agent，有界重试，超限转 gate
notify: [desktop, slack]
on_done: pr                   # pr | local-merge
```

2. **Daemon 开放协议**：session/task/event 三组 API，event stream 为追加式 JSONL（WebSocket 订阅）。文档即协议，任何 UI/CI/编辑器插件按此接入。
3. **验证引擎**：done-criteria 执行 + 失败回喂循环。这块可以拆成独立库，被任何编排器（包括别人家的）复用——成为公共好物是获客。

配套公共资产：**每夜对着真实 agent CLI 的兼容性测试**（record/replay fixtures）公开跑、公开挂 badge——既是质量保证，也是"这个项目最认真对待抓屏漂移问题"的社区信号。

## 6. 架构与技术栈

架构沿用方向文档 §7（daemon + PTY 池 + adapter + 状态机 + verifier + policy），强调三点开源化调整：

- **协议即产品**：daemon API 全文档化，event log 对外只读订阅。
- **adapter 双轨**：声明式 YAML adapter（prompt/按键/状态模式，社区热更新）+ 编译型 adapter（第一方，用结构化协议加速）。
- **与 ACP 对齐**：公开表态兼容/映射 ACP 事件模型，降低"自造协议"的社区抵触。

技术栈建议：

- **首选 Rust**：单静态二进制（用户零依赖安装，infra 项目的生命线）、daemon 级健壮性、终端社区信誉（Zed/Ghostty 路线），PTY/仿真/TUI 组件齐备（portable-pty + vt100 + ratatui，wezterm 已验证）。
- 备选：Go（bubbletea，迭代快、单二进制也好办）或 TypeScript（node-pty + xterm.js headless，生态最近、迭代最快，但 daemon 长驻健壮性与单二进制叙事弱）。
- 判断依据：如果只有你一个人且 TS 最熟，v0.1 用 TS 抢速度完全成立；但若目标是"被依赖的基础设施"，Rust/Go 的单二进制 + 长驻稳定性在社区心智里有真实加分。
- Windows：WSL2 为权威运行时（agent 跑 WSL 内 Unix PTY），ConPTY 兜底；macOS/Linux 原生。

## 7. v0.1 范围（一个可发布的切面，约 4-6 周）

1. `velella` daemon（`velella serve` 长驻模式）：PTY 会话（detach 后存活）、worktree-per-task、崩溃自动 resume
2. 1 个第一方 adapter：Claude Code（hooks + stream-json 加速，屏幕仲裁兜底）
3. 状态机 + `tui-idle`/waiting_permission 分类器 + 桌面通知 / Slack webhook
4. `.velella/tasks.yaml` 加载 + done-criteria + 失败回喂重试 + 人工 gate
5. CLI：`velella task run / list / attach / logs / approve`；TUI 看板（任务列表 + 状态 + live attach）
6. 兼容性测试 harness v0 + README + 任务格式 RFC

**Show HN 时刻的 demo 脚本**：一条命令从 yaml 起三个任务 → TUI 看板上三个 agent 在各自 worktree 跑 → 其中一个弹权限询问，桌面通知，终端里 `approve` → 任务跑完自动跑测试验证 → 自动开 PR。GIF 30 秒讲完整个价值。

发布节奏：v0.1（Show HN / r/ClaudeAI / r/LocalLLaMA）→ v0.2 adapter SDK + 第二个 agent（Codex CLI）→ v0.3 GitHub Action（CI 分发渠道）+ 远程 daemon pairing。

## 8. 开源运营要点

- **License**：Apache-2.0（专利授予、企业友好）。若未来做托管服务再考虑分层，不要一开始 AGPL 核心阻碍被集成。
- **治理**：任务格式与 adapter 规格走 RFC 流程（这两个是要变成"标准"的东西，让社区有所有权感）；代码常规 PR 流程。
- **获客顺序**：先以"验证引擎 + 兼容测试"这种公共好物建立专业信誉，再推 runtime，最后 TUI 只是门面。infra 的信任来自工程质量证据。
- **对 Orca 的姿态**：互补不对抗（"Orca 用户也可以用 velella 跑 CI 任务"）；若 Orca 未来做 headless/任务即代码，你的壁垒在任务格式生态与兼容测试数据集。

## 9. 风险清单

| 风险 | 对策 |
|---|---|
| Orca 也做 headless/任务即代码（它已有 `serve` + automations） | 抢时间窗；把任务格式做成被第三方引用的标准；护城河从代码移到生态 |
| 单人维护长夜任务级 daemon 的健壮性 | 范围纪律（v0.1 切面）；fencing/超时等语义直接抄分布式系统成熟模式，别发明 |
| agent CLI 每周漂移 | adapter SDK + 数据驱动热更新 + 公开兼容 CI；把漂移处理变成社区贡献入口 |
| 撞名导致搜索不可见 | 已解决：定名 velella，npm/PyPI/crates.io/Homebrew/GitHub 五面实测全清空（§4）；发布周做域名/org/商标检查 |
| "抓屏"被社区视为脏活 | 公开兼容测试 + 屏幕只做状态仲裁不做内容解析 + 结构化协议优先的技术叙事 |
