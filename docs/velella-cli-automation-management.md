# 自动化移除、恢复与完整历史

本轮用户结果：通过 Velella CLI 移除不再需要的自动化、列出并恢复它，同时分页查询超过旧版全局 200 条的运行记录。

验收合同（2026-10-29，执行前确定）：

- 移除与恢复由 runtime 在事务中完成；历史保留。恢复后禁用，需重新审阅并 save 才能重新启用。
- 未结束的任务（包括待审查、待恢复）和未核对的派发记录阻止移除；移除不停止任务或终端。
- 分页支持 1–500 条、指定自动化和游标；相同时间的记录顺序稳定，后续插入不进入当前翻页集合；运行状态按每页查询时更新。
- 超过最近 200 条的任务及中断派发记录仍能同步状态。
- 旧 runtime 保留原有功能；新命令先检查能力，拒绝不支持的操作。默认 runs 在旧 runtime 上明确标注历史范围，显式分页不降级。
- 使用隔离 profile/Git 夹具，不访问真实账号、不发起模型请求。开发单元测试、真实 CLI/runtime 集成、旧 runtime 兼容和独立静态审阅分别记录。

最初验收关键路径是 runtime 接口与隔离验证；随后用户明确确认整包 runtime 升级，现已安装并完成生产在线核对。部署时正常退出桌面、备份数据库、仅切换 daemon，保留原终端；数据库从 v2 迁移到 v3。真实模型执行、业务 UAT 和独立动态测试仍未执行。完整记录见 [上线交付报告](../artifacts/velella-automation-management-20261029/deployment-final.md)。

## 使用方式

```powershell
velella-cli automation remove AUTOMATION_ID --confirm
velella-cli automation list --archived
velella-cli automation show AUTOMATION_ID --archived
velella-cli automation restore AUTOMATION_ID --confirm
velella-cli --json automation runs --automation AUTOMATION_ID --limit 100
velella-cli --json automation runs --automation AUTOMATION_ID --limit 100 --cursor NEXT_CURSOR
```

游标必须搭配原来的 `--automation` 筛选条件，直到 `has_more=false`。从头查询才会包含翻页期间新增的运行；各页状态并非时间点快照。所有命令可以指定 `--data-dir` 选择已经启动的隔离 runtime。

移除记录放入单独的 `archived_automations` 表，旧 runtime 不会调度它们；恢复是显式操作，不会恢复先前的启用状态。保留已接受的 CLI、旧 runtime 和数据备份仍是生产部署的独立前置条件。
