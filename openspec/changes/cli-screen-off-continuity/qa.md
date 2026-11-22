# 验证记录 — 2026-11-15

## 最新安装状态

用户随后明确要求安装。已在原正式 reopen.3 源码基线上合入保活改动，完成 release 构建、包内验证、隔离双向切换/回退、正式备份和安装；新 GUI/daemon、两个快捷方式和 PATH CLI 在线核对通过，7 个原进程、61 份历史及全部业务表保持。接电合盖设置已生效，电池策略未改。**安装完成，接电合盖、Windows锁定及自然熄屏均通过；SC_MONITORPOWER直接关屏失败，全部真实业务UAT未完成，总范围仍未全部验收。**

最新锁定结果：修正观察器读取并发状态快照失败的问题后，11 项检查及正式 runtime 自检通过；同一重试采集中经用户明确许可调用一次 Windows 锁定，再由用户手动解锁。完整区间约 193 秒，两 CLI 原 PID、各 194 条跨区间连续输出、最大间隔约 1.028 秒及解锁后输出均通过，9 个自有进程已清理；正式安装不变。详见 `artifacts/cli-power-20261115/windows-lock-reader-retry/RESULT.md`，以下较早覆盖不足记录保留为历史。

最新历史取证：经用户授权以管理员只读查询 Security 4800/4801，确认 01:22:12—01:25:22 的锁屏；测试 CLI 在 01:22:46 结束，只覆盖前约 34 秒，未达到至少 60 秒且跨越解锁的标准。原始采样末尾实际有 34 条 locked=true，此前“完全未捕获锁定”的文字说明已更正。未修改系统设置、未重启实测，详见 `physical-uat.md` 及 `artifacts/cli-power-20261115/LOCK-AUDIT-DIAGNOSIS.md`。

随后维护原验证流程：未闭合区间保留为覆盖不足，新增完整区间通过后自动完成与进度记录，9 项合成判定测试及 8 秒正式 runtime 自检通过。实测时用户最终澄清操作为直接合盖、打开，故未执行 Windows 锁定；三来源完整记录约 350 秒接电合盖，两 CLI 持续输出，再次验证合盖通过。测试已清理，正式版本保持，锁屏和独立关屏仍未完成验收。详见 `artifacts/cli-power-20261115/observation-v3-live/RESULT.md`。

当前正式基线及完整证据见 [安装记录](../../../docs/release/2026-11-15-cli-power-installed.md)。下方为安装前的开发阶段记录，不能把原 debug 候选当作本次正式包。

最新关屏结果：SC_MONITORPOWER 实测触发 Modern Standby，WLAN 断开，两 CLI 心跳中断约185秒，关屏路线失败。测试已停止并清理，原命令禁用，未改安装或系统配置；自然超时候选仍未验证，临时接电显示超时方案须用户另行同意，Apply 未执行。详见 `artifacts/cli-power-20261115/display-off-uat/RESULT.md`。当前总目标未完成，不能用此前通过结果掩盖这次失败。

最新自然熄屏结果：用户明确授权的临时接电超时60秒配置下，自然关闭约185秒，两个原CLI各186条跨区间连续输出、最大间隔约1.016秒，全程接电、点亮后输出通过；WLAN查询无断开/重连事件。10:13:52已恢复并读回原AC/DC显示超时0/0，10个自有进程确认退出，正式安装不变。详见 `artifacts/cli-power-20261115/display-idle-uat/RESULT.md`。合盖、Windows锁定、自然熄屏均有实机通过证据；直接SC_MONITORPOWER路线仍失败，全部真实Agent及电池/拔电物理UAT仍未完成，不能宣告总范围全部完成。

## 安装前开发阶段记录

当前用户可新增做的事：本机已启用接电合盖“不采取任何操作”、接电空闲“不睡眠”，允许现有 CLI 在这些条件下继续运行；电池合盖“睡眠”保持原值。在隔离开发候选中另已实现随进程生命周期启停的 AC 保活。**尚未替换当前安装版，物理关屏/合盖与业务 UAT 未完成，总目标未完成。**

| 层级 | 结果与边界 |
| --- | --- |
| 实现 | Rust PTY 公共入口及直接 hybrid 入口已接入 AC-only 保活；Windows 配置与恢复脚本已实现 |
| 开发验证 | 5 项 power_continuity 测试、1 项既有真实 PTY 退出测试、配置脚本 7 组场景通过；所有 Rust bins 检查及 debug 编译通过 |
| 独立测试 | 未进行独立运行测试；独立源码审查另记，不冒充独立测试 |
| 技术验收 | 开发层验证通过；独立源码审查提出的 1 项真实问题已修复并复测，2 项已核对反证；无延期源码问题。未进行独立技术签收，物理电源行为未验收 |
| 集成 | 三个同源 debug 二进制已生成；独立 host 连续输出验证通过，无 GUI/真实模型接入 |
| 部署 | 未替换安装版；没有重启现有桌面、daemon 或 CLI。按用户批准的接电范围启用了本机系统策略，有原值备份 |
| 当前在线 | 原安装版继续运行；系统 AC 合盖配置已生效并读回，新的 runtime 保活代码未进入原有 host |
| 业务 UAT | 锁屏、物理关屏、接电合盖、电池合盖与拔电仍待用户实机验证 |

## 已执行检查

- `cargo test -p velella --lib power_continuity -- --nocapture --test-threads=1`：5 passed，包括实际 PowerSetRequest/PowerClearRequest 计数检查、不含 DisplayRequired、多会话、供电切换与查询失败、请求失败、历史保留、停止和启动失败释放。
- `cargo test -p velella --lib sessions::tests::spawn_echo_session_reaches_terminal_state`：1 passed。
- `cargo check -p velella --bins`：通过。
- `cargo build -p velella --bins`：通过。候选是 debug runtime/CLI，不是完整安装包或 release 验收。编译有既有未用导入/死代码警告，未扩展修改相关模块。
- `powershell -NoProfile -File scripts/verify-cli-power.ps1`：7 组通过；全部为隔离模拟，不改真实电源设置。
- `powershell -NoProfile -File scripts/configure-cli-power.ps1`：启用前真实只读结果为 AC/DC lid=1、idle=0；完成独立源码审查与修复复测后，按已授权的 AC-only 范围执行 `-Mode Enable` 成功，再独立只读检查为 AcLid=0、DcLid=1、AcIdle=0、DcIdle=0、acPolicyReady=true。只改变 AC 合盖一个值，电池设置未改。
- `python scripts/verify-cli-power-native.py --runtime artifacts/cli-power-20261115/candidate/bin/velella-runtime.exe --output artifacts/cli-power-20261115/native`：两个独立 host 保持原 PID；先后完整记录 15、60 条时间戳输出，前一个 host 结束后后一个仍有新增输出；均 Done/exit=0。无 GUI、模型调用或系统策略写入。

Windows `powercfg /requests` 只读查询返回“需要管理员权限”；未提升权限、未重复尝试。Windows 请求由本进程持有的真实 API 对象计数测试验证，不把此权限错误视为功能失败或不存在请求。

独立源码审查：quick，一轮 3 条意见逐项收敛。修复运行中查询错误后保活监测永久退出的问题，现在错误时释放请求、继续监测，并在重新确认接电后恢复；新增错误→接电恢复断言。writer/reader 失败路径在 Windows 会先 Drop 掉 KILL_ON_JOB_CLOSE 进程组，不存在所报的失管子进程。独立 host 脚本只提供输出/PID证据；真实 Windows 请求的证据仍来自原生 API 与真实 PTY 生命周期测试。修复后 5+1 测试、7 组脚本、bins check/build 及最终候选的两 host 验证再次通过；没有新增代码后继续扩大测试。

## 矩阵覆盖

| 场景 | 已通过的覆盖 |
| --- | --- |
| 任意适配器接电启动 | SessionManager 的 generic/unlisted adapter 实际 PTY；native API 请求 |
| 拔电、电池启动、未知供电、查询错误 | fake power source 驱动实际 lease worker，确认请求释放；物理拔电未执行 |
| 多会话独立释放 | Windows 两个真实 request object 及两种适配器实际 PTY；独立 host 连续输出 |
| 完成、停止、启动失败与历史保留 | `power_continuity_follows_processes_not_history_and_cleans_failed_spawn` |
| 只读、启用、重复、恢复 | `verify-cli-power.ps1`，包括恢复之后的用户编辑 |
| 写入/刷新/读回失败 | 同脚本注入失败并核对部分回滚 |
| 用户改值、执行中并发改值、换方案 | 同脚本确认不覆盖后续改值、不切换当前方案 |

## 失败尝试与新证据

1. 进程失败夹具首次使用不存在的 cwd，但 portable-pty 的 Windows 实现会过滤不存在的 cwd 并回退。查明该库 `cmdbuilder.rs` 的 `current_directory` 后，改为不存在程序且禁止 shell 回退；一次修正重试通过。
2. 独立 host 夹具首次正则误用单词边界，ConPTY 的 CSI 末尾 `h` 紧接首条 `first:1`，导致只读解析遗漏首行。原始字节中首行完整；修正 marker 匹配后一次重试通过。未修改原始输出和生产代码。

## 证据与保留

- `artifacts/cli-power-20261115/native/result.json`：通过的独立进程结果及 raw logs 路径；初次解析失败结果另存 `initial-parser-result.json`。
- `artifacts/cli-power-20261115/source-before/`：5 个已有脏源码的修改前副本。
- `artifacts/cli-power-20261115/change.diff`：相对于本任务开始时的精确源码变化，含新增文件；未暂存全仓或混入已有工作。
- `artifacts/cli-power-20261115/candidate/bin/`：三个候选二进制。
- `artifacts/cli-power-20261115/manifest.json`：候选及源码 SHA256、范围和验收状态；候选以此版本冻结。
- `%LOCALAPPDATA%\Velella\cli-power-policy.json`：真实系统原值备份；另复制到 `artifacts/cli-power-20261115/power-policy-backup.json`。恢复按使用说明执行，保留后续用户修改。
- `docs/cli-screen-off-continuity.md`：使用、恢复、供电边界及物理验收步骤。
