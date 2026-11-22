# 2026-11-01：统一 CLI 协作入口

用户现在可以通过 `velella-cli collab` 直接调用已有会话协作，通过 `skills bundled/get` 读取当前客户端的内置指南。完整 Orca CLI 功能对齐未完成；本轮关闭“现有协作入口分散、公共 CLI 缺少离线指南发现”的缺口。

## 冻结候选

- CLI Cargo 版本：0.1.0；指南 revision：`2026-11-01.collab.1`。桌面版本仍为 0.2.8。
- 候选：`artifacts/velella-cli-collaboration-20261101/candidate/velella-cli.exe`。
- SHA256：`49d9eca0e7a15c985e2a939101bd3d7eff25a05168679aef696f6cd1d9cd91b3`。
- 兼容性测试 runtime SHA256：`8d7b64d01bc825466a3ff7385d875adfff55d1936088b624b03298638e275c44`。本轮未构建或替换 runtime。
- 工作区原有大量未提交变更继续保留。CLI 从当前工作区链接生成；该候选的验收不授权将同一工作区的 backend 打包上线。

## 实现

`collab`（别名 collaboration）复用 runtime CLI 的命令枚举和执行函数。保持凭据优先级、成功加入后的会话绑定、原生 session ID、稳定 mutation request ID、脱敏回执以及无自动重试；公共 IPC 超时传入共享客户端。SSH 参数在连接及凭据读取前拒绝。旧 runtime collab 保持原有输出和 stdio MCP；公共 collab mcp 报错并给出正确传输入口。

`skills bundled/get` 返回随二进制编入的指南及客户端版本/revision，不访问 daemon、不修改配置。`skills list/show` 继续发现用户技能，installed 是 list 别名；schema 增加协作命令及内置指南发现。

## 验证层级

| 层级 | 实际结果 |
| --- | --- |
| 开发验证 | `cargo test -j 1 -p velella --lib desktop_cli -- --test-threads=2`：63 passed；`cargo test -j 1 -p velella --lib collaboration::cli::tests -- --test-threads=2`：3 passed，2 个 subprocess entrypoint ignored；发布构建通过，既有警告未顺便修改 |
| 独立测试 | 未执行独立动态测试；只读探子独立静态审阅未发现本轮阻断问题 |
| 技术验收/集成 | 主线程以冻结旧 runtime 运行隔离测试：9 组核心 CLI 集成、4 组协作集成通过；协作使用真实 daemon/host/授权 adapter/SQLite/IPC，Agent 程序为不联网、不接受任务的惰性夹具 |
| CLI 安装 | 仅更新 `%LOCALAPPDATA%/Programs/Velella CLI/velella-cli.exe`；PATH 解析及安装 SHA256 核对通过；不同旧版已备份 |
| 部署 | 未更新桌面安装包、未替换其内置 CLI、未重启生产后台 |
| 当前在线 | 新 CLI 只读 status 成功；daemon PID 55264、43 个保存会话 ID 和 15 个 runtime 进程的 PID/启动时间/路径保持；冻结 runtime 文件 hash 不变 |
| 业务 UAT | 未向正式 Agent 发消息或提示词；未执行真实 Agent 协作、登录/信任、MCP 服务或外部业务验收 |

隔离协作测试覆盖 create 回放、凭据不回显、确认前不能加入、send 防重、receive/ack、旧入口读取同一 run、跨 session/profile 拒绝、缺 request ID 拒绝、SSH/MCP 分流和离线不落盘。它证明客户端与旧运行时协议兼容，不证明真实模型按指南完成业务。

## 使用与回退

```powershell
velella-cli skills bundled --json
velella-cli skills get velella-cli
velella-cli skills get velella-collaboration
velella-cli collab --help
```

已有终端可使用 `C:\Users\developer\AppData\Local\Programs\Velella CLI\velella-cli.exe`。桌面 0.2.8 自带的 CLI 与当前用户 PATH CLI 不同；若手动调用旧 resources/bin，需要改用上述用户级入口。未改已有会话的环境或角色配置。

旧 CLI 备份：`C:\Users\developer\AppData\Local\Programs\Velella CLI\velella-cli.exe.20261101-011042.bak`；另有本轮 `baseline/velella-cli.exe`，SHA256 为 `4e0e45670601e48970e02934a1f67bb607425d649886f6ca726269b0309868ba`。需要回退时仅恢复用户级 CLI，无需数据库恢复或终端重启。

证据目录：`artifacts/velella-cli-collaboration-20261101/`，含 `legacy-runtime-integration.json`、`collaboration-compatibility.json`、`before-install.json`、`cli-install.json` 和 `installed-verification.json`。

剩余差距继续按 `docs/velella-cli-parity.md` 跟踪：terminal switch、账号管理、computer 等尚未补齐，原生 Agent 的完整协作流程还需业务验收。本轮候选冻结，后续外部/UAT 缺口不重复重建这个已通过的客户端。
