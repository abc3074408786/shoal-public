# 接电关屏与合盖持续运行 — 已安装

2026-11-15，用户明确要求“安装”后，已安装并启动 runtime 更新 `0.2.15-reopen.3-power.1`。现有桌面界面保持原样；新创建的 CLI 使用接电保活机制，电池策略不变。**安装完成；接电合盖、Windows锁定和自然熄屏均通过；SC_MONITORPOWER直接关屏仍失败，真实模型业务UAT未完成，整体范围尚未全部验收。**

最新 Windows 锁定验证：经用户明确授权调用一次系统锁定，由用户手动解锁；记录到约 193 秒完整区间，两 CLI 原 PID、各 194 条跨区间连续输出、全程接电及解锁后输出通过，最大间隔约 1.028 秒。测试进程已全部清理，正式安装未变。详见 `artifacts/cli-power-20261115/windows-lock-reader-retry/RESULT.md`。下方较早的“锁屏未完成”描述为历史阶段，已由此结果关闭该缺口。

后续“继续”实测：两段接电合盖分别约 75 秒、3 分 10 秒，两条原生 CLI 的原 PID 和连续输出保持，最大输出间隔约 1.016 秒。锁屏覆盖不足、独立关屏无有效区间，未判通过；两轮后停止重复操作。测试进程已清理，详见 `artifacts/cli-power-20261115/physical-uat-live/RESULT.md` 和 `openspec/changes/cli-screen-off-continuity/physical-uat.md`。

用户随后授权单独排查，新增独立窗口事件来源，未修改已安装产品。该轮 600 秒观测已结束：两个隔离 CLI 各 596 条连续输出，最大间隔约 1.016 秒；两来源仍未捕获有效锁屏/关屏区间，原因未知，整体验收未完成。9 个自有测试进程全部退出，正式 GUI/daemon 继续运行，runtime 指纹不变。不追加相同实测，详见 `artifacts/cli-power-20261115/observation-v2-live/RESULT.md`。

09:12 经用户授权完成管理员只读查询，Security 日志确认 01:22:12—01:25:22 锁屏约 190 秒，但第一轮 CLI 在 01:22:46 已结束，仅有约 34 秒锁定期间连续输出；没有跨越解锁，未满足既定验收标准。此前“完全未捕获锁定”的说明有误，现已更正；原采样末尾有 34 条 locked=true。日志读取权限缺口已关闭，未更改审计策略；详情见 `artifacts/cli-power-20261115/LOCK-AUDIT-DIAGNOSIS.md`。

后续按“完整区间及返回后输出齐全才完成”的流程补测。用户最终澄清本次实际为“直接合上笔记本盖子，之后打开”，因此本轮未执行 Windows 锁定。已完整记录约 350 秒接电合盖，两 CLI 输出连续、最大间隔约 1.016 秒，再次通过合盖验证；锁屏和独立关屏仍未验收。检测已结束，9 个测试进程全部退出，正式桌面与安装文件保持；无需再重复合盖。详见 `artifacts/cli-power-20261115/observation-v3-live/RESULT.md`。

最新单独关屏结果：本机 SC_MONITORPOWER 命令触发 S0 新型待机，导致 Wi-Fi 断开及两 CLI 暂停约185秒，此路线失败；唤醒后 Wi-Fi 重连。直接命令已禁用、测试进程已清理，没有修改安装文件或系统策略。自然超时熄屏尚未验证，接电显示超时为“从不”；临时改为1分钟并恢复的方案尚待用户批准。详见 `artifacts/cli-power-20261115/display-off-uat/RESULT.md`。不能将已通过合盖/锁定扩展为所有关屏方式均通过。

最新自然熄屏验证已通过：在用户明确允许后临时将接电显示超时改为1分钟，自动关闭约185秒，两 CLI 持续输出、原 PID及点亮后输出保持，最大间隔约1.016秒；测试窗口无 WLAN 断开/重连事件。原接电/电池显示超时0/0均已恢复，10个自有进程全部清理。系统日志为 Idle Timeout 的新型待机会话，期间实际输出保持；此前 SC_MONITORPOWER 触发的主动待机、断网和CLI暂停结论仍有效，该直接命令仍禁用。详见 `artifacts/cli-power-20261115/display-idle-uat/RESULT.md`。尚未完成全部真实 Agent 业务 UAT。

## 当前基线

- 安装：`D:/Velella/artifacts/cli-power-20261115/integration-installed/installation-0.2.15-reopen.3-power.1/desktop`。
- 数据：`C:/Users/developer/AppData/Local/Velella`，未迁移。
- 固定源码：`D:/Velella/artifacts/cli-power-20261115/integration-installed/source`。在前一正式 reopen.3 固定源码上三方合入保活差异；原 1056 文件中 1051 文件不变，另新增已审查的保活模块。保留并发、全部 Agent 原生恢复、Codex 可执行文件和 hook 信任修复。
- **后续安装应沿此正式源码与安装基线继续，主工作区尚未同步此前全部线上差异，不能直接从根目录重新构建覆盖。**
- 桌面 `app.asar` 和界面版本保持 `0.2.15-reopen.3`，此次是 runtime/CLI 更新，安装目录后缀标识 `power.1`。86 个安装文件中只替换 runtime 和 CLI，84 个文件逐一保持原 SHA256；未改界面、Electron 桥接、插件和独立的 zcode-free 账号入口。

## 在线与保留

- 新主进程 PID 7728，新 daemon PID 26712；两个桌面/开始菜单快捷方式和 PATH CLI 已同步。
- Computer Use 只读检查了新安装路径的唯一工作台窗口，仍为 AppStore 的 ZCode 终端及输入区域；没有向真实 CLI 输入内容或调用模型。
- 原有会话的 7 个 host/子进程身份保持，61 份原始历史前缀保持；3 项目、14 工作区、61 会话、5 布局保留，业务数据库表摘要一致。
- 旧 host 不会热更新其代码，因此保留它们的运行状态；新开的终端由新 daemon 启动，使用新的保活机制。当前 Windows 接电合盖不操作、接电空闲不睡眠的策略也覆盖这些旧进程。
- 独立读回：AcLid=0、DcLid=1、AcIdle=0、DcIdle=0。没有更改电池、锁屏、屏幕超时或充电策略。拔电后解除保活不代表立即强制睡眠，携带前应主动睡眠或关机。

## 验证层级

| 层级 | 结果 |
| --- | --- |
| 实现 | 已有冻结代码未重做，仅集成到正式源码基线 |
| 开发验证 | 集成后保活 5/5、恢复 16/16；`--locked` release 构建通过 |
| 独立测试/审查 | 复用原保活 quick 源码审查及其修复证据；本轮集成验证由主线程执行，没有新增独立运行验收 |
| 技术验证 | 固定 release 包内两独立 host 的 15/60 条连续输出、PID 与退出状态通过；隔离的新旧 daemon 双向切换与回退保留原 shell 变量、进程、日志及输入通过；schema 仍为 3 |
| 集成 | 保留正式基线源码及桌面包；runtime/CLI 构建指纹与安装文件匹配 |
| 部署 | 完成；原安装保留，备份已生成 |
| 当前在线 | 新 GUI、新 daemon、快捷方式及 PATH CLI 均核对通过 |
| 实机验证 | 接电合盖、Windows锁定及自然超时熄屏通过；SC_MONITORPOWER直接关屏触发待机、断网和CLI暂停，失败并禁用；临时超时设置已恢复 |
| 业务 UAT | 未完成；全部真实 Agent 模型任务、电池/拔电路径未验收 |

## 备份与证据

- 备份：`C:/Users/developer/AppData/Local/Velella-backups/cli-power-20261115-010214`，含 SQLite 在线一致备份、JSON、原 PATH CLI 和快捷方式。
- 原安装：`D:/Velella/artifacts/terminal-reopen-all-20261114/integration-rc2/installation-0.2.15-reopen.3/desktop`，完整保留。
- 系统合盖原值备份仍为 `%LOCALAPPDATA%/Velella/cli-power-policy.json`；恢复方法见 `docs/cli-screen-off-continuity.md`。
- 安装与在线收据：`artifacts/cli-power-20261115/integration-installed/installation.json`、`online-verification.json`。
- 来源与包：同目录 `source-manifest.json`、`merge.json`、`integration-proof.json`、`change.diff`、`package-manifest.json`、`package-proof.json`。
- 固定包验证：同目录 `native/result.json`、`handover.json`、`validation.json`。
- runtime SHA256：`88b24bf39c0384b706e963f35402aa9812f9453c419155cb2929b1ff0cd11131`。
- CLI SHA256：`a726ee7279b2546feb05962b19bb251982d6e3d41d415cee8b2bd1835c52319e`。

回退必须精确切换桌面 relay 和 daemon，保留 host；不能使用会停止全部会话的 `service.stop` 对正式 profile 操作。本轮在独立临时 profile 验证了新旧切换和回退，没有恢复或覆盖正式数据库。
