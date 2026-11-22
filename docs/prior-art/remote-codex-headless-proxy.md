# Prior art: remote Codex headless proxy

- Date: 2026-11-14
- Mode: quick
- Searches used: 2 retained connector discovery batches; exact query count and raw query strings were not retained. No script-measured saturation claim.
- Saturated: no

## Need

让 doctor 的 Linux ARM64 主机在没有图形桌面时，为现有远程 Codex 的 `custom/deepseek-flash` 提供 `127.0.0.1:15721/v1` Responses 代理，继续接入 Velella 原有编排。

约束：不改变现用 Codex 或 GUI provider/model，不迁移原数据库，不导出凭据到 Windows，不调用真实模型；正式部署另行授权。当前安装的 Velella 0.2.15-rc.1 保持冻结。

## Verdict

| Part | Verdict | Project | Why |
|---|---|---|---|
| Responses/SSE 代理运行时 | Adopt | SaladDay/cc-switch-cli v5.11.3 | 有 Linux ARM64 固定发行包和 foreground proxy serve；隔离假上游运行通过 |
| 常驻与状态隔离 | Compose | 固定 CLI + doctor systemd user service + 独立 HOME/state | 不依赖桌面；不对原 GUI DB 执行启动迁移；现有 linger=yes |
| Velella 编排与远程工具桥 | Keep | 已安装 0.2.15-rc.1 | 当前阻塞在外部代理监听，不重开已通过源码 |

Adopt 来源：[SaladDay/cc-switch-cli](https://github.com/SaladDay/cc-switch-cli)，MIT。固定 tag v5.11.3，commit `a285cd7d02fe893a72e9b47340f3893908bcabb6`，schema22。取用日期 2026-11-14；只做人工评审后的版本升级，不自动更新或打开原 GUI 数据库。

ARM64 archive SHA256：`d2bd3e792d4ca47786d2971ad5258795737057e1ba5f08cabfdb6b4ee85a0bf6`。解包后二进制 SHA256：`501d9fd9bebf67c362782b6d21a1e18df79c66914a8c987415153a7f842db393`。

## Read

| Project | Link | Covers | Lacks | Licence | Last commit | Cost to adopt | Access it needs |
|---|---|---|---|---|---|---|---|
| 官方 CC Switch | [repository](https://github.com/farion1231/cc-switch) | 已安装4.0.17的配置和代理基线；README及远端运行状态 | 当前无可用图形环境 | MIT | 本次未固定官方HEAD；已装版本单独核对 | 恢复需图形会话 | 既有GUI数据与桌面 |
| cc-switch-headless | [repository](https://github.com/tiezbro/cc-switch-headless) | README中的headless规划 | README明确尚无可用daemon build | MIT | 本次未固定HEAD | 无可直接采用的发行运行时 | 未实际安装 |
| cc-switch-web | [repository](https://github.com/Laliet/cc-switch-web) | README，v0.21.0 headless/Web管理路径 | 本任务不需要新增Web管理面；未验证本环境兼容性 | 未单独核对LICENSE，不能据README徽章确认 | 本次未固定HEAD | 引入额外管理面及独立验证 | Web管理认证及provider数据 |
| cc-switch-cli | [pinned source](https://github.com/SaladDay/cc-switch-cli/tree/a285cd7d02fe893a72e9b47340f3893908bcabb6) | README、release/tag、provider/config/proxy/DAO、协议与隔离运行 | 真实上游及当前release并发未验收 | MIT | 固定release commit见上；非最新HEAD承诺 | 独立二进制、私有state、用户unit | 只在远端读取现有选中provider；私有复制所需认证配置 |

初次探子读取的是 main `b1fa06ebabac9bc3dcb04b72cee10c91faafbc56`（schema26）；后续亲自核对实际 v5.11.3（schema22）的相关实现并执行发行二进制。两套 provenance 不混用。

关键源码：[config.rs](https://github.com/SaladDay/cc-switch-cli/blob/a285cd7d02fe893a72e9b47340f3893908bcabb6/src-tauri/src/config.rs)、[proxy.rs](https://github.com/SaladDay/cc-switch-cli/blob/a285cd7d02fe893a72e9b47340f3893908bcabb6/src-tauri/src/cli/commands/proxy.rs)、[provider.rs](https://github.com/SaladDay/cc-switch-cli/blob/a285cd7d02fe893a72e9b47340f3893908bcabb6/src-tauri/src/cli/commands/provider.rs)。不使用 `proxy enable` 或 `--takeover`。

## Seen, not read

| Project | Link | Where it came from |
|---|---|---|
| codex-deepseek-proxy | [repository](https://github.com/rodas-j/codex-deepseek-proxy) | Web发现结果；未据摘要判断功能完整性 |
| cc-switch-ui | [repository](https://github.com/huangbogeng/cc-switch-ui) | Web发现结果；未继续展开 |
| DeepSeek Responses 指南 | [docs](https://api-docs.deepseek.com/guides/responses_api/) | Web发现结果；本轮不据此切换用户上游或模型 |

## Ruled out

| Project | Reason |
|---|---|
| 原GUI直接启动 | 本次恢复预检未找到可用图形环境；停止于启动和设置变更之前 |
| cc-switch-headless | 尚无可直接运行的daemon build |
| cc-switch-web | 额外Web管理面超出当前最小恢复路径；不是认定其不可用 |
| 直接复用原GUI数据库给新CLI | schema20与22不同，新CLI可能迁移；独立state和单provider导入代替 |
| x64发行包 | 首次架构预检失败，二进制未执行；uname确认aarch64后改用ARM64通过 |

## How it was searched

- 名称族：CC Switch headless/server/daemon/CLI、无桌面/常驻代理、Codex DeepSeek Responses/SSE proxy。原始完整查询字符串未保存，不能冒充逐条精确检索日志。
- 来源：已安装远端软件及配置形状、已有Velella接入证据、Web搜索、GitHub仓库/README/源码/issue/release/tag。
- 种子：官方CC Switch及其社区headless/web/CLI分支。
- 失败或不可用来源：FastCtx本轮不可用，使用本地rg/PowerShell；公开连接器的完整失败列表未保存，不声明全部来源成功。发现、源码结果摘要在本地 discovery-evidence.json。
- Issue #351及#541属于上游用户报告，本轮没有在真实用户上游复现，不把它们当作本环境已发生故障。

## Not covered

没有真实模型调用、真实上游工具链验收、Codex+DeepSeek当前release并发验收、断电/重启/注销后的长时运行验收或业务UAT；不验证上游内部实际模型身份。现有GUI保存默认模型为gpt-5.6-sol，live Codex请求为deepseek-flash；隔离用例证明请求模型透传，不改动源行或live配置。

检索在找到固定ARM64发行包、完成隔离协议/模型透传/断连不重放及部署预检后停止。没有穷尽所有替代项目，也没有测得检索饱和。

## Validation and delivery

本地证据目录：`D:/Velella/artifacts/minimax-hybrid-20261112/integration-0.2.14-20261114/headless-candidate-96506f00-8d93-45de-93f9-1fe4e25b32bc`。

- 假上游：Responses文本、SSE function_call、deepseek-flash透传、max_retries=0且failover关闭时上游断连仅一次请求，通过；自有临时代理均已停止，原配置文件摘要不变。
- 新部署脚本：8项Python与3项Node检查通过；包括配置漂移回滚回执、服务收尾退出、并发双击只派发一次、候选和授权漂移拒绝。一次Windows测试暴露SQLite备份连接未关闭，修复后通过，未隐藏首次失败。
- 独立只读源码核验发现两项确定缺陷及一项时序风险，已由主线程修正并验证；没有宣称修正后又经过独立真实部署验收。
- 远端：ARM64、端口空闲、目标路径和同名unit不存在、linger已开启、临时unit语法通过。补充客户端检查首次因读取不相关进程exe受限失败，改读comm后仅一次改变条件重试通过；未发现Codex客户端，原配置摘要相同。
- **正式部署未执行，总目标未完成。** 具体候选及回滚见证据目录 deployment-review.md。没有重新打包或替换Velella。


## Subsequent authorized deployment

Fixed candidate was separately authorized and deployed once on 2026-11-14. User service active/enabled, exact loopback listener ownership and installed Velella custom/deepseek-flash binding verified; original configuration digests unchanged. No model request submitted. See candidate DEPLOYED.md and deployment-completion.json. Current release real-model concurrency and business UAT remain incomplete.
