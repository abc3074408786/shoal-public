# 接电时关屏、锁屏和合盖继续开发

适用范围：本次 Windows 候选 runtime 管理的全部原生 CLI，包括普通 shell、不同 Agent、自动任务、旧 headless PTY 和 hybrid 的本机 SSH 端。不是某一个模型的开关。

## 日常使用

1. 使用包含本变更的 runtime 启动新终端。CLI 存活且确认接电时自动保活；无需保持项目页或桌面窗口可见。
2. 屏幕可以按原来的超时熄灭，也可以锁屏。保活不会模拟鼠标、键盘，不阻止锁屏，不强制显示器常亮。
3. 需要合盖运行时，先检查并显式启用下面的 Windows 配置。打开盖子后仍是原会话及原始输出。
4. CLI 退出、失败或停止后解除其保活；历史记录不会继续保活。多个 CLI 各自持有请求，结束一个不会撤销另一个。

已有运行中的 host 不会因为源码更新而自动获得新功能。本次不重启或迁移已有终端；安装新 runtime 后，新建的终端才使用该机制。

本机实测已通过接电合盖、Windows 锁定及自然超时熄屏。日常需要自动熄屏时，在 Windows 显示设置中选择超时；本次测试的临时1分钟已恢复为原来的“从不”。

本机 `SC_MONITORPOWER` 强制关屏命令会触发主动待机，已实测导致约185秒 CLI 暂停和 Wi-Fi 断开，该测试命令已禁用。自然超时熄屏约185秒的测试中，CLI 输出持续且无 WLAN 断开记录，不能将两种路径等同。全部真实 Agent 业务任务仍需单独验收，详见 `artifacts/cli-power-20261115/display-idle-uat/RESULT.md`。

## 合盖配置

在源码目录运行，默认命令**只读**，不会改变系统：

```powershell
powershell -NoProfile -File D:\Velella\scripts\configure-cli-power.ps1
```

需要接电合盖运行时主动启用：

```powershell
powershell -NoProfile -File D:\Velella\scripts\configure-cli-power.ps1 -Mode Enable
```

启用前保存当前电源方案 GUID 和原始数值；只把**接电**合盖设为“不采取任何操作”、接电空闲睡眠设为“从不”。电池、屏幕超时、锁屏、临界电量和散热策略不修改。默认备份文件为 `%LOCALAPPDATA%\Velella\cli-power-policy.json`，应保留。

这两项是持续生效的 **Windows 系统设置**，也影响其他应用，退出 Velella 或结束全部 CLI 后不会自动恢复。恢复命令：

```powershell
powershell -NoProfile -File D:\Velella\scripts\configure-cli-power.ps1 -Mode Restore
```

脚本核对写入结果；失败时尽可能恢复本次已尝试修改的值，并报告失败步骤和备份位置。重复启用不覆盖原备份，重复恢复不覆盖恢复后的新修改。用户之后修改的数值存在冲突时保留该值并报错；切换电源方案后不会擅自切回旧方案。厂商工具或组策略改动电源方案后，应重新检查，不能沿用过去的“已就绪”。

遇到权限拒绝时停止，按实际错误处理；本功能不会自行提升权限、关闭组策略或反复重试。`powercfg /requests` 通常要求管理员权限，不能用它不可读来推断没有保活请求。

## 电池与使用边界

- 用户已选择仅接电持续运行。运行时每秒检查供电，拔电或供电未知时解除保活；电池供电启动时不会持有请求。
- **解除保活不等于立即睡眠。** 保留原电池策略，不模拟再次合盖。如果已经合盖后才拔电，不能承诺电脑立即睡眠；携带前应主动睡眠或关机。
- 接电运行仍会发热，温度长期偏高会加快电池老化。保持散热，合盖运行时不要装进包内。充电由电脑原有电源管理控制，本功能不调整充电阈值。
- 主动睡眠、休眠、关机、低电量或温控保护可以暂停/终止工作；程序不能在真正睡眠后继续计算。远端主机、网络、账号和模型服务状态仍分别决定远端任务可用性。
- 本次其他系统仅保持源码兼容，不声称已实现 macOS/Linux 保活或合盖行为。

## 验证与物理验收

开发检查：

```powershell
cargo test -p velella --lib power_continuity -- --nocapture --test-threads=1
cargo test -p velella --lib sessions::tests::spawn_echo_session_reaches_terminal_state
powershell -NoProfile -File scripts\verify-cli-power.ps1
```

Rust 测试检查 Windows 原生请求确实包含 system/execution、不包含 display，多会话、拔电/未知供电、启动失败、退出、停止和历史保留。配置测试注入独立 powercfg 模拟器，不修改真实电源策略。

物理验收需要候选版本实际安装并启用合盖配置后进行：启动两个不同 CLI，运行带时间戳的连续输出，分别测试锁屏、关屏、接电合盖至少数分钟；重新打开后核对同一会话身份、时间戳连续性及原始日志。结束一个 CLI 时另一个仍推进；最后退出全部 CLI 后不再持有请求。再确认电池合盖仍按原策略，拔电后保活解除。记录硬件、供电、电源方案、runtime 哈希和时间区间。

单元测试、进程测试或配置读回不能代替物理关屏/合盖，也不能代替真实模型任务的业务验收。安装、当前在线和业务 UAT 结论见本变更的 `openspec/changes/cli-screen-off-continuity/qa.md`。
