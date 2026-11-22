# 2026-11-01：原生对话搜索与按请求执行回执

后续状态：2026-11-02 已完成真实 Codex/Claude 文本样本验收，并上线实测发现的 Claude 空闲状态修复；当前 daemon PID 5328、runtime a4bd44…。本页以下保留11月1日交付基线，当前状态与首次初始化限制以[11月2日验收记录](2026-11-02-cli-real-native-uat.md)为准。

用户现在可以用已安装的 `velella-cli search "关键词" --path D:\orcas --json` 搜索本机 Codex/Claude 原生对话。搜索是有界 JSONL 扫描，尚无持久全文索引；必须检查 complete 和省略/错误回执。

用户确认后，启动确认后台已部署并通过主线程及独立只读在线核验。本轮搜索和启动确认的上线已完成，完整 Orca CLI 对齐与真实模型业务 UAT 仍未完成。新后台只对显式创建的 `--track-submission` 原生 Codex/Claude 会话提供证明，旧会话不自动注入新钩子。

| 层级 | 实际结果 |
| --- | --- |
| 实现 | 本机原生对话 search，默认可见消息、显式 all 工具范围；opt-in 跟踪会话、send --wait-submit、只读 terminal receipt；旧后台/旧会话前置拒绝新跟踪操作 |
| 开发验证 | 最终 74 项 CLI 回归、35 项 terminal_io、7 项 host、2 项 submission_receipts，共 118 项通过；release CLI 与后台构建通过 |
| 独立测试 | 未运行独立动态功能测试。独立静态核验发现的深度截断问题已修复并回归；最终超时修复由主线程验证。部署后另一个只读 Agent 独立核对现场进程、产物 hash、协议、SQLite、44 份历史前缀、账户/配置和两个快捷方式，全部通过 |
| 技术验收 | 主线程隔离验证：原生搜索 6 组；新 CLI/冻结旧后台核心 7 组；旧后台跟踪前置拒绝 2 组；基于线上冻结源码的新后台执行回执 5 组；旧 CLI/新后台核心 7 组，共 27 组通过 |
| 集成 | 真 daemon、owner-only IPC、PTY、持久回执；Agent 为本轮不联网的 Rust 假程序。覆盖确认、防重放、权限阻塞、忙碌、超时、错误提示词及 host 停止后回读；不等于真实模型验收 |
| CLI 安装 | 用户级 velella-cli.exe 已更新并备份；安装 hash、指南 revision 和已安装原生搜索只读探针通过 |
| 部署 | 用户明确确认后完成后台切换，两个快捷方式已指向冻结桌面目录。86 个程序文件仅 runtime 与内置 CLI 相对原版改变；7 份 SQLite 已用 backup API 备份并通过完整性检查，无 schema 迁移。切换前 GUI 未运行，切换后保持关闭 |
| 当前在线 | 新 daemon PID 50860、冻结 runtime hash、submission_tracking:1 和用户 CLI ready 经主线程及独立核验通过；44 个保存会话及 document、3 个布局、schema v3、44 份历史前缀、4 个账户文件和2 个 profile 配置文件保持一致。切换前无运行中的 Agent 宿主，未自动启动旧 Agent |
| 业务 UAT | 未请求真实模型、登录、信任、账户切换或真实协作任务。原生 Hook 版本兼容、实际钩子延迟和真实业务 UAT 尚待执行 |

## 固定产物与来源

- 用户 CLI：`C:\Users\developer\AppData\Local\Programs\Velella CLI\velella-cli.exe`。
- CLI SHA256：`9f1ba6c1e63883dc548c7003b0eb70494d67554937cd4bae67bc427f6931dc21`。
- 指南 revision：`2026-11-01.search-submission.1`；Cargo CLI 版本仍为 0.1.0。
- CLI 备份：同目录 `velella-cli.exe.20261101-232436.bak`。
- 切换前 runtime SHA256（原目录保留）：`aff6d94f67da4ea7a4d06a5710f422375a16b72e5ffeb60c2265a0921acb5b72`。
- 当前生产干净 runtime SHA256：`09c997514feb80dd18a4c817c73af4c2ea0ef159c7402bddd2de1158c42e3b4c`。
- 当前后台及快捷方式目标目录：`D:\orcas\artifacts\velella-cli-search-submission-20261101\desktop-candidate`。

生产后台候选使用 `artifacts/settings-cli-install-20261101/source` 的 130 个已核对冻结输入，只覆盖本轮 5 个后台接线文件并新增 submission_receipts.rs，共 131 个文件，6 个允许变化。未合入当前工作区的其他 dirty 后台代码，没有数据库 schema 变更。runtime-source.json 与 runtime-only.diff 记录逐文件来源和精确差异；冻结源码在 runtime-source 下。

桌面当前安装版在旧冻结回执之后已有图标、主程序资源及 app.asar 更新。本轮逐字保留这些当前 GUI 文件，未为了回用后台基线而降回旧 GUI。desktop-candidate.json 记录当前 GUI 的完整基线及候选散列，变化严格限定 resources/bin/velella-cli.exe 和 resources/runtime/velella-runtime.exe。

## 回执的证明范围

提示词先以 UUID 防重持久化。跟踪 host 绑定实际 host PID、native session、guarded input sequence 和提示词 SHA256；精确匹配 UserPromptSubmit 后，还必须有对应的 PreToolUse 或 Stop，才能报告 turn_started_confirmed:true。SessionStart、单独 UserPromptSubmit、终端回显和 running 状态都不算。证据不会证明业务成功或上游计费；纯文本回复可能到 Stop 才能确认。

超时与读取错误保留最后有效观察，不重复发送。原生版本缺少或改写提示词/turn 关联字段时，保守返回未确认。额外同步钩子有启动成本，因此仅显式新会话开启；旧会话默认发送行为保持兼容。

## 验证中关闭的问题

1. 独立核验发现扫描深度截断没有设置 incomplete：修复并新增区分未知深层目录与显式 Claude subagents 排除范围的回归。
2. 首轮真实隔离执行发现截止时间附近的观察错误覆盖先前有效回执：修复保留最后观察，增加单测；改变条件后的唯一一次场景重跑通过。
3. 打包首次拒绝当前 GUI 与旧冻结文件不同：查明仅已有图标/GUI 资源演进，改为保存当前 GUI 的准确基线，后台仍绑定原冻结来源；第二次打包成功。

通过的候选冻结。后续后台部署与真实模型 UAT 不重复重建已通过的源码。

## 使用与下一步

```powershell
velella-cli search "关键词" --path D:\orcas --json
velella-cli search "工具输出关键词" --path D:\orcas --scope all --json
velella-cli search "关键词" --workspace <工作区ID> --agent codex --json
```

本次切换重新核验了 daemon 身份，备份 SQLite 和快捷方式后只交接后台，没有调用全局 service.stop、杀进程树或自动启动旧 Agent；切换后完成会话、历史前缀、布局、配置和新协议核对。原冻结后台与 CLI 备份作为回退输入，不使用旧数据库备份覆盖新写入。

上述切换现已完成，授权等待已消费。可按以下形式新建跟踪会话，并在发送时等待同一请求的执行证据：

```powershell
velella-cli terminal create --workspace <工作区ID> --launch codex --track-submission --json
velella-cli terminal send --workspace <工作区ID> --terminal <新会话ID> --text-file prompt.txt --enter --request-id <UUID> --wait-submit 10 --json
velella-cli terminal receipt --workspace <工作区ID> --terminal <新会话ID> --request-id <同一UUID> --json
```

未使用正式会话发测试提示词，未启动新 Agent 或请求真实模型。真实原生 Hook 的版本兼容、延迟和业务流程继续按业务 UAT 单独验收，不将只读在线核验冒充模型执行验收。

证据目录：`D:\orcas\artifacts\velella-cli-search-submission-20261101`。主要回执：CONTRACT.md、independent-static-review.md、native-search-integration.json、legacy-core-integration.json、legacy-submission-refusal.json、clean-runtime-submission.json、old-cli-new-runtime-core.json、installed-verification.json、runtime-source.json、desktop-candidate.json、deployment.json、independent-online-verification.json 和 final-manifest.json。SQLite 备份、完整上线前快照及回退输入位于仅当前用户可访问的 deployment-private。
