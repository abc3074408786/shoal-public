# Velella CLI — 会话协作指南

通过 `velella-cli collab` 直接连接已有桌面的本机 IPC，发送会话消息、加入成员、分派工作和提交验收回执。`collaboration` 是别名。它复用 `velella-runtime collab` 的身份、权限和请求防重逻辑，无需启动 MCP server；旧入口仍兼容。

`swarm`（别名 `orchestration`）负责用户侧的方案确认与编排控制；`collab` 负责参与会话的 Agent 协作。两者沿用运行时的角色与人工确认边界，不能互相替代权限。公共入口拒绝 `--ssh-profile`，在连接或读取凭据前报错，不回退到本机操作。

## 发现和身份

```powershell
velella-cli skills bundled --json
velella-cli skills get velella-collaboration
velella-cli collab --help
velella-cli collab status --json
```

内置指南随当前 CLI 二进制提供；返回 `cli_version` 和 `guide_revision`，不连接 daemon、不安装技能、不修改 Agent 配置。指南描述客户端能力，当前运行时是否支持仍以实际回执为准。`skills list` / `skills installed` / `skills show` 则查询 daemon 发现的用户技能。

身份优先级：`--credentials <文件>`，其次 `VELELLA_COLLAB_CREDENTIALS`，再按 `VELELLA_SESSION_ID` 查找当前 profile 的成功 create/join 绑定。不猜测活动会话，不读取别的 profile。普通调用携带原生 session ID，让运行时继续校验会话撤销和角色权限。

新协调者可以在属于自己的 Velella 原生会话里执行：

```powershell
velella-cli collab create --goal "明确的总目标" --request-id <UUID> --json
```

已有终端没有 `VELELLA_SESSION_ID` 时，create/join 必须明确提供真实 `--session <会话ID>`；后端会核验身份。成功回执只输出 `credentials_file` 路径，不输出凭据 token；文件应留在当前用户 profile 内，不粘贴到提示词、日志或仓库。

## 邀请、消息和回执

```powershell
velella-cli collab invite --session <接收方会话ID> --name "审阅者" --request-id <UUID> --json
# 接收方审阅邀请后，在自己的会话内加入：
velella-cli collab join --run <run_id> --member <member_id> --invitation <邀请值> --confirm --request-id <UUID> --json
velella-cli collab send --to <member_id> --text "消息内容" --request-id <UUID> --json
velella-cli collab receive --wait-ms 0 --json
velella-cli collab ack --seq <已处理消息序号> --request-id <UUID> --json
```

`send` 写入 mailbox，成功不代表接收方已执行。receive 不自动确认消息；处理后再 ack，重复读取保留未确认消息。默认等待 25000 ms，上限 25000；`--timeout-ms` 控制整个 IPC 截止期限，默认 45000。主动通知 `notify` 受运行时的安全空闲和原生输入检查约束，不能当作模型已经执行的证据。邀请值本身敏感，只交给预期接收方。

## 工作和验收

```powershell
velella-cli collab assign --assignee <member_id> --workspace "D:\project" --title "明确任务" --instruction "范围和验收条件" --work-key <稳定逻辑工作标识> --write --request-id <UUID> --json
velella-cli collab started --attempt <attempt_id> --request-id <UUID> --json
velella-cli collab candidate --work <work_id> --json
velella-cli collab report --attempt <attempt_id> --outcome completed --summary "实现与验证结果" --candidate candidate.json --evidence evidence.json --request-id <UUID> --json
velella-cli collab review-request --work <work_id> --reviewer <独立审阅成员ID> --request-id <UUID> --json
velella-cli collab review-report --review <review_id> --verdict passed --summary "独立验收结论" --candidate candidate.json --evidence evidence.json --request-id <UUID> --json
velella-cli collab accept --work <work_id> --summary "接受依据" --request-id <UUID> --json
```

使用真实回执中的 ID。candidate.json 保存 candidate 返回的指纹对象或含 candidate 字段的完整 JSON。v2 evidence 使用后端要求的结构化检查、产物指纹和剩余缺口；失败报告须带稳定 `--cause-key`。参数、角色及证据不满足时由运行时拒绝。完成报告不等于独立验收，通过技术验收不等于用户业务验收，accept 不执行发布。

所有写入必须预先分配稳定 `--request-id`（原样 JSON 请求也适用）。未知响应后先检查 status/reconcile 和回执；只能以完全相同 ID 与内容恢复同一次操作，不能新建 ID 重发输入。CLI 不自动重试。读取 status/receive/reconcile/candidate 不要求请求 ID。

更多命令通过 `collab --help` 查询，包括 writer claim/release、取消、离开、结束和方案建议。`collab request <UTF-8 JSON文件>` 仅接受 `collab.*`，上限 1 MiB，仍执行相同身份及请求 ID 校验。公共 `collab mcp` 明确报错，stdio MCP 传输仍使用 `velella-runtime collab mcp`，不会混入 CLI JSON 输出。
