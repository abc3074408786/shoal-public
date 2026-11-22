# 四 CLI 编排续接 N：40% 持久交接已实现，整体工作流仍未完成

**整体未完成、未安装。** 工作区 `D:/orcas`，分支 `codex/integrate-collaboration-20261022`，基线 HEAD `62769b4a8ea0ec4c7f347c16726888cbaf3db64a`；保留全部既有脏文件，禁止 reset/clean/stash/全仓格式化。接任者必须先读 M 和本文件；不重新运行已冻结的 MCP、context 或 native 切片。

## 本轮关闭的缺口

40% latch 不再只是阻止自动输入。对于 OMP/Claude 已验证的 usage 回执，调度器会先持久保存交接包（固定 baseline、未完成节点、活动 work/review、证据、失败账本与阶段），等待旧原生回合安全空闲；随后启动同角色、同工作区的替代会话，将**同一 member、work、attempt 与 attempt_count**迁移过去，并向新会话发送一次交接通知。

旧会话的标准 runtime 请求带 `VELELLA_SESSION_ID`，迁移后不再匹配成员的 native identity，因此不能继续驱动成员；同 OS 用户可直接调用 desktop management API 的既有边界仍不被误称为恶意进程沙箱。交接绝不新建 retry，也不重置 failure ledger；恢复不重发交接输入。旧回合仍在运行时只保留 `awaiting_safe_point`，不强制中断它。替代 CLI 若出现登录或 trust 提示，必须由用户在隔离会话中处理，系统不自动确认。

UI 的“会话上下文”已改为显示交接状态和这一人工边界。

## 关键代码与验证

- `crates/velella/src/desktop/orchestration/v2.rs`：交接包、空闲点迁移与对其他已报告节点的非阻断调度。
- `crates/velella/src/collaboration.rs`、`desktop/collaboration_commands.rs`、`collaboration/cli.rs`：协调者授权的 identity 迁移与 native-session 绑定。
- `desktop/orchestration/v2_tests.rs`：身份/attempt 保留、旧会话拒绝、持久包、恢复不重放。
- `desktop/src/ContextUsage.tsx`、`OrchestrationV2.tsx`：交接状态展示。

本轮实际执行：`cargo test -p velella --lib`（**233 passed / 0 failed / 8 ignored，241 total**）以及 `desktop` 的 `npm run build`（通过；保留既有大 bundle 警告）。未运行真实认证 CLI、没有安装或发布。

## 剩余关键路径

1. 定向接入 Codex 与 OpenCode 的可信 context 来源，并以真实 Claude statusLine 消费验证；缺失必须仍显示 unknown。
2. 四 CLI 的真实认证领取、回报、独立审查与恢复，及至少双 CLI 双实现混合集成；需要登录/trust 时一次提出具体隔离入口与动作，绝不自动同意。
3. 独立总体验收后，才可按既有授权作可回滚安装、在线核验与业务 UAT。
