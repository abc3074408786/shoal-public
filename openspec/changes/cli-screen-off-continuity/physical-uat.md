# 安装后实机验证

这是原有关屏/合盖需求的验证补充，不改变已冻结实现或已安装版本。

用户结果：接电关屏、锁屏或合盖期间，CLI 持续执行；重新打开后仍是原进程和连续原始输出。当前接电合盖、Windows 锁定与自然超时熄屏均已有完整实机通过证据；直接 SC_MONITORPOWER 关屏仍会待机、断网及暂停约185秒，该路线未达成，全部真实 Agent 业务 UAT 未完成。自动保活、部署、连续输出基线已在前序记录验证，不重复实现。

最新结果：`artifacts/cli-power-20261115/windows-lock-reader-retry/RESULT.md`。Windows 锁定/解锁完整区间 193.2446757 秒，两 CLI 原 PID、连续序号、全程接电和解锁后输出均通过；锁定场景收口，不再重复。

最新自然熄屏结果：`artifacts/cli-power-20261115/display-idle-uat/RESULT.md`。用户另行授权的临时接电 VIDEOIDLE=60 自然熄屏完整区间约185秒，两 CLI 各186条跨区间连续输出、最大间隔约1.016秒，全程接电及点亮后输出通过。本轮无 WLAN 断开/重连事件；原 AC/DC 显示超时0/0已恢复并读回，10个自有进程全部退出。此前直接 SC_MONITORPOWER 失败结论保持；全部真实 Agent 业务 UAT 仍未完成。

### 用户继续授权的单独关屏验证

在已明确下一步为单独关屏约 90 秒后，用户要求“继续”。当前仅关闭显示关闭/点亮的运行证据缺口；合盖和锁定不重测，产品及安装保持冻结。沿用 v4 观察器，`--until-feature display`，固定输出 `artifacts/cli-power-20261115/display-off-uat`，两个隔离 CLI、正式 runtime 指纹不变，600 秒硬上限。

ready 后才以 `WM_SYSCOMMAND/SC_MONITORPOWER=2` 请求显示器关闭一次，用户保持接电和盖子打开，约 90 秒后移动鼠标或按 Shift 点亮。辅助脚本仅请求关屏，不改电源/安全/显示超时策略、不持有保活、不自动点亮、不输入凭据；记录 API 返回但不以它代替真实电源事件。

验收条件仍为 session display=0 后返回1、至少60秒、全程接电、原 host、两 CLI 序号连续且最大间隔不超过3秒，首尾跨越关闭及点亮；窗口 console display 事件并列保留作交叉核对。不以黑色画面、用户回执或 API 成功宣告通过。采集仅在完整合格区间与点亮后输出到齐才自动结束；失败先定位，最多一次改变前提后的重试。

实测失败并已停止：09:56:44.945 Kernel-Power 506 明确记录 SC_MONITORPOWER 进入 Modern Standby，随后 WLAN 8003 记录断开；09:59:50.505 键盘唤醒，10:00:09.488 WLAN 重连。关屏约185.68秒，两 CLI 最大心跳间隔约185.09秒，超过3秒，判失败。9个测试进程已清理，正式安装和原 GUI/daemon 保持，但不能声称真实业务请求在本次待机期间完全未受影响。直接关屏脚本已禁用，证据见 `artifacts/cli-power-20261115/display-off-uat/RESULT.md`。

只读查得接电/电池 VIDEOIDLE 均为0（从不）。自然熄屏候选重试需要暂时将接电值改为60秒、最终恢复原值，电池不写入。已准备默认只读的 `artifacts/cli-power-20261115/test-display-idle.ps1`，语法解析和只读计划查询通过，Apply 未执行。因这改变了前述不修改显示超时的范围，须待用户明确同意后才可应用；自然熄屏仍可能再次触发短时待机/断网，尚不能保证。完整恢复条件写入失败报告，不重复 SC_MONITORPOWER。

用户随后明确同意“允许临时修改并测试，完成后恢复”。本轮仅按该授权启动 `display-idle-uat` 隔离采集，ready 后应用临时接电 VIDEOIDLE=60；用户在自然熄屏后约90秒点亮，既定至少60秒、原进程、连续输出和返回后输出条件不变。配置辅助脚本最长运行300秒并在 finally 恢复和读回原值；若系统再次待机，恢复须待唤醒后执行。采集上限600秒。此为根据已查明直接命令触发主动待机而改走自然超时的一次重试；再次失败即停止，不重跑或改已冻结产品。

## 验证入口与来源

- 安装基线：`docs/release/2026-11-15-cli-power-installed.md`。
- 产品入口：固定源码的 `crates/velella/src/sessions.rs::spawn_launch_recorded`、`power_continuity.rs::Lease`。状态归实际 host 进程所有，生命周期不依赖 GUI。
- 已有自动入口：`scripts/verify-cli-power-native.py`，验证已安装 runtime 的独立 host 与连续输出。
- 新增的同路径实机观察器：`scripts/verify-cli-power-physical.py`。两个隔离 PTY 使用当前已安装 runtime；观察器只读 Windows 硬件事件和会话锁定状态，不持有保活请求、不注入输入、不修改电源策略、不访问正式 profile。
- 证据：raw `terminal.bin`、OS `events.jsonl`、逐秒 `samples.jsonl`、固定 PID/运行文件指纹 `run.json`、按场景判定 `result.json` 和隔离进程清理 `cleanup.json`。

## 功能映射与固定验收条件

| 场景 | 系统独立观测 | 活体验收条件 | 当前状态 |
| --- | --- | --- | --- |
| 接电合盖 | GUID_LIDSWITCH_STATE_CHANGE=0，之后重新打开=1 | 连续接电区间至少 60 秒，两 CLI 序号连续、首尾输出跨越区间，最大输出间隔不超过 3 秒；原 host PID 保持 | 通过：74.68、189.69 秒；本轮再完整记录约 350 秒 |
| 接电关屏 | GUID_SESSION_DISPLAY_STATUS=0，之后重新点亮=1 | 同上；不能将黑色截图或最小化当作关屏 | 自然超时通过：约185秒、两CLI各186条、最大间隔约1.016秒；SC_MONITORPOWER 直接命令仍失败 |
| 接电锁屏 | WTSInfoEx Level1 SessionFlags=0，之后解锁=1；WM_WTSSESSION_CHANGE 7/8 为直接事件，Security 4800/4801 可交叉核对 | 同上；不能用“用户不在场”事件替代锁定状态 | 通过：窗口直接事件完整区间 193.2446757 秒，两 CLI 各 194 条跨区间连续输出，最大间隔约 1.028 秒 |

建议每项实际保持 90 秒，留出事件和逐秒采样边界。合盖通常也能覆盖关屏，但必须有实际屏幕关闭事件才计入该项。供电不明或电池区间不纳入接电验收。物理操作由用户完成，监测本身没有成功结论。

## 本次实测与收敛结果

2026-11-15，用户确认进行了合盖及锁屏操作。`artifacts/cli-power-20261115/physical-uat-live/result.json` 保存两段接电合盖的通过证据：第一段 74.6849715 秒，两 CLI 各 77 条跨区间输出；第二段 189.6918475 秒，各 190 条；最大相邻输出间隔分别不超过 1.016 秒。host 始终为 A=46072、B=58124，全程各记录 441 条输出，无重启和模型调用。

原先将两轮情况概括为“WTS 锁定状态全为 false”的文字结论有误。09:12 经用户授权读取 Security 日志后，确认 Session 1 在 01:22:12.4385790—01:25:22.2714855 锁定 189.8329065 秒；原始采样末尾确有 34 条 locked=true，两个 CLI 在 01:22:46 就结束，未跨越解锁，故不足以通过既定标准。display 仍全为 1。观察器、测试 host 与正式桌面均在 Session 1；不据此否定用户操作或归因为产品故障。原始记录和旧分析文件不改，补充关联证据见 `artifacts/cli-power-20261115/lock-audit-readonly/correlation.json`。

精确未关闭缺口：覆盖完整锁定/解锁区间且满足至少 60 秒条件的 CLI 输出，以及显示器关闭/点亮区间。锁屏时间的日志读取权限缺口已关闭。接电合盖持续执行已证实，完整的锁屏/独立关屏与全部真实 Agent 业务 UAT 未完成；不发起第三轮相同实测。

Pstack 结果：harness/map 已补充并活体验证；整体覆盖未完成。未发现本次产品回归，也没有把缺失事件改判为成功。原实现与安装文件保持冻结。

清理已核对：只终止本次测试的 2 个 host 及其隔离 PTY，共 8 个自有进程，全部确认退出；正式 GUI PID 7728、daemon PID 26712 仍在，已安装 runtime SHA256 不变。正式 profile 没有接入测试或新增任务。电源读回仍为 AcLid=0、DcLid=1、AcIdle=0、DcIdle=0。

## 运行与收敛

### 用户授权继续后的独立观测核对（2026-11-15）

用户在两轮缺失事件后明确要求继续单独排查。独立只读核对确认：原回调注册、232 字节 WTSINFOEX 结构、字段偏移和锁定值解释均符合 Windows 定义，未发现确定的 ABI 错位。测时系统日志也不足以还原完整锁定区间，不能归因为产品或否定用户操作。活动显示设备只有一个内置面板，不能假设是外接显示器导致。

确认的验证工具缺口有两项：原 `not_exercised` 只表示“没有观测到合格区间”，不能解释为用户未执行；原分析只从逐秒最新采样建立区间，会忽略同一秒内已记录在 raw events 的关/开对。后者可由代码构造，但原日志无 display 变化，不能用它解释本次缺口。

只修改验证工具，不改产品或已安装文件：

- 新增 `scripts/cli_power_window_notifications.py`，用消息专用窗口被动订阅 `WM_WTSSESSION_CHANGE` 和 `WM_POWERBROADCAST`，并列记录 session display、console display、lid、source；无可见窗口、无键鼠输入、无锁定或关屏操作、无保活请求。
- `verify-cli-power-physical.py` 同时保留原回调/轮询和窗口事件，逐来源分析原始事件，不以多个来源的“最新值”互相覆盖。直接锁定事件 7、解锁事件 8 可独立建立区间。判定时长、供电、PID 和连续输出标准未降低。
- 缺乏合格观测的新结果名为 `not_observed`，旧结果文件原样保留。直接事件与轮询有差异时作为诊断证据，不预设一定发生漏报。
- `python scripts/test-cli-power-observation.py` 的 5 个合成判定测试通过；这些不是物理 UAT。`observation-v2-smoke` 真实安装版本自检通过，两 CLI 各 8 条输出，窗口消息循环私有 ping 和原生订阅正常，物理场景仍明确未通过。

### 独立来源观测收尾（2026-11-15 01:48）

`artifacts/cli-power-20261115/observation-v2-live/RESULT.md` 保存本轮终态。600 秒上限内共 595 次采样，首尾跨度 599.326 秒，全部接电；两个原 host 各 596 条输出，序号连续，最大间隔不超过 1.016 秒。原回调和窗口来源只有 7 条初始状态事件，没有 session change 或显示关闭/点亮事件，WTS 轮询始终未锁定。没有完整状态区间，三项均为 `not_observed`；本轮结果不撤销此前已通过的两段合盖证据。

新增来源未取得可对照的锁定区间，因此不能认定原 WTS 轮询漏报，也不能否定用户先前的锁屏操作或断定产品失败。确切根因仍未知。整体目标未完成；当前缺口仍是锁屏/关屏的独立系统状态证据，以及全部真实 Agent 业务 UAT。没有新证据前不追加相同操作、不改冻结产品、不降低标准。当前不需要用户再作一次重复操作的决定。

观察器自动退出后，按 PID 与创建时间复核本轮 9 个自有进程全部退出，正式 GUI 7728、daemon 26712 继续运行；runtime 指纹不变。新增摘要为 `observation-summary.json`，清理复核为 `cleanup-verification.json`。此前开发验证、独立 quick 源码审查、集成、部署结论保持；本轮没有新增独立运行技术验收或业务通过结论。

后续用户回复本轮问题：“已锁屏约90秒并解锁”。2026-11-15 09:09 本地核对时已补记至本轮 `RESULT.md`。回复未附实际操作时间，不能确认与 01:37:49—01:47:49 检测窗口重叠，因此也不能将无事件归因为系统漏报。保留用户操作确认，原始结果不改，不重启相同检测。

用户再次要求继续后，09:10 只读查询发现明确权限限制：Security 日志元数据查询要求管理员权限，`auditpol /get` 返回 `0x00000522`。先前筛选返回无匹配不能视为安全日志中不存在锁屏记录。其他可读日志仍不足以还原操作时间。已准备并完成语法检查的只读脚本 `artifacts/cli-power-20261115/read-lock-audit.ps1`，未以管理员执行，不改变安全策略、不启动新实测。精确缺失输入为管理员读取受限日志的权限；详见同目录 `LOCK-AUDIT-DIAGNOSIS.md`。产品和原测试判定保持冻结。

09:12 用户明确同意只读管理员查询后，脚本成功导出 4800/4801；Other Logon/Logoff Events 成功审计已启用，未改变设置。仅有上述 01:22—01:25 的完整锁定区间；第二轮 01:37—01:47 无锁定/解锁事件。第一轮输出覆盖前约 34 秒，各 36 条跨边界输出、序号连续、最大间隔小于 1.016 秒；没有足够时长或解锁后的输出，结论为 `incomplete_coverage`。这关闭了历史锁定时间取证缺口，但没有通过锁屏验收。09:10 的权限阻塞已解决，无需再次提权或重复用户操作。完整更正见 `artifacts/cli-power-20261115/LOCK-AUDIT-DIAGNOSIS.md`。

### 已定位提前收尾后的单次修正验证

用户再次明确要求继续。当前关键路径是完整锁定/解锁期间的连续输出，以及关屏区间；目标和至少 60 秒、最大间隔 3 秒、原 PID、全程接电条件不变。新证据已明确第一轮采集在解锁前结束，本次仅针对该根因进行一次改变流程后的验证，不是再排查未经证实的 WTS 漏报。

仅修改已采用的观察器及判定测试：

- 未闭合或过短的状态区间保留为 `observed_intervals`、标记 `incomplete_coverage`，避免把“看到锁定但覆盖不足”误读为“没有锁屏”。
- 新增 `--until-feature lock` 与 `progress.json`，仅在完整区间、连续输出及解锁后输出齐全时自动收尾；用户确认回执不会触发停止。600 秒硬上限及显式 `stop` 紧急中止保留，中止原因单独记录，不能冒充通过。
- 9 项合成判定测试通过，含锁定未闭合、解锁后无输出、原始窗口事件未闭合及完整区间通过；不是物理 UAT。
- `observation-v3-smoke` 已使用正式 runtime 自检 8 秒，两 CLI 各 8 条输出，未虚报场景通过，deadline 原因及清理正确；自有 host 和子进程已核对退出。
- 只读查得本机接电锁屏关闭显示超时为 60 秒。因此本轮建议锁屏约 3 分钟，允许保持原超时的显示关闭区间达到至少 60 秒，不改任何显示设置。

新实测固定目录 `artifacts/cli-power-20261115/observation-v3-live`，正式 runtime SHA256 仍为 `88b24bf39c0384b706e963f35402aa9812f9453c419155cb2929b1ff0cd11131`，两个隔离 CLI，不接入正式 profile、不调用模型。ready 后再请用户操作。若未在上限内取得完整区间，保留原因并停止，不继续重试；未通过部分保持未完成，不重开冻结产品。

本轮收尾：用户起初回复“已锁屏约3分钟并解锁”，因记录仅有合盖事件，随后仅询问实际操作方式，用户明确为“直接合上笔记本盖子，之后打开”。本轮实际未执行 Windows 锁定，不归因为 WTS 或窗口漏报。三个来源都记录了约 350 秒完整合盖区间，两 CLI 各 349 条跨边界输出、最大间隔约 1.016 秒，全程各 522 条，合盖通过；没有独立关屏区间，锁屏及整体验收仍未完成。

用户澄清后，完整合盖区间及打开后输出已保留，主动结束本轮采集。计划目标仍如实保留 `until_feature=lock`，结束原因 `aborted_by_stop_file`，不冒充锁屏目标自动完成。9 个自有进程按创建时间确认全部退出，正式 GUI/daemon 与安装指纹保持。此轮不再追加相同操作，结果见 `artifacts/cli-power-20261115/observation-v3-live/RESULT.md`。当前无需用户再次合盖；若后续单独验收 Windows 锁定，必须明确该实际路径，不能继续把合盖当作锁定。

### 用户明确要求 Windows 锁定测试

用户随后明确要求“锁定测试”。本轮只关闭尚未完成的 Windows 锁定运行验证缺口，实际路径为接电、打开盖子、Win+L 进入需要 PIN/密码返回的界面、约 3 分钟后解锁，全程不合盖。不是重复已通过的合盖测试。

沿用已自检的 v3 观察器、正式 runtime 指纹和既定条件，输出 `artifacts/cli-power-20261115/windows-lock-uat`；两条隔离 CLI，无模型请求、正式 profile 或电源/安全设置改动。ready 后才请用户操作，完整区间及解锁后连续输出合格才自动结束；600 秒硬上限内未覆盖则保留原因并收尾，不据回执宣告通过。

本轮确实捕获 Windows 锁定，用户也明确确认按 Win+L 完成操作。但约 112 秒锁定记录后，观察器读取并发替换的 `B/status.json` 抛出拒绝访问，异常收尾清理自有 host，缺少解锁阶段；这不是产品运行失败证据。失败源码及完整结果保存至 `windows-lock-uat/RESULT.md`，自有进程已确认清理。

已将 host 存活核验改为原始 Popen 进程句柄，不再读取可竞争的 UI 状态快照；其余验收条件保持。11 项判定/原生检查通过，含 Windows 独占占用文件复现及已退出 host 仍失败；修正后的正式 runtime 8 秒自检通过。v4 观察器源码 SHA256 写入 run，首次通过自检时为 `ca21dcef4c1d5cd49014b21589ce8f0a07f080bf3f3f01d5e6f4ade6a5642052`。本次新发现的状态快照读取根因仅进行一次修正后重试，目录 `artifacts/cli-power-20261115/windows-lock-reader-retry`；若仍失败则停止，不更改冻结产品或降低标准。

修正后重试已通过。起初用户回执未能与新事件对上，没有据此通过或重启采集；用户另明确允许自动锁定一次后，在同一采集中于 09:46:23.9300781 调用 LockWorkStation，窗口事件在 09:46:24—09:49:37 记录完整锁定/解锁。区间 193.2446757 秒，两 CLI 原 PID、各 194 条跨区间输出、序号连续、最大间隔分别 1.027993 秒/1.020957 秒，全程接电并有解锁后输出，故通过。WTS 采样也记录到状态变化，逐来源保留，不以 API 成功或用户回执替代实际运行证据。

采集器依据合格区间及返回后输出自动完成，9 个自有进程按创建时间确认全退出，正式 GUI/daemon 原启动时间和 runtime 指纹保持。锁定场景已收口；独立关屏尚未捕获有效显示状态区间、真实模型业务 UAT 未执行，整体范围仍未全部验收。本轮无新的独立运行签收，不改变此前源码审查/集成/部署层级。详见该目录 `RESULT.md`、`result.json`、`cleanup-verification.json`、`evidence-manifest.json`。

新路线与前两轮的差别是独立的 Windows 直接事件来源，并有用户追加授权；不是无变化重复实测。先记录这一前提，再进行一次新观测。若新来源仍无有效状态，只保留证据，不追加同样操作或降低条件。

先运行同观察器的短时自检，确认 Win32 事件注册、锁屏状态读取、两个真实 PTY、raw 日志和清理可用。2026-11-15 自检运行 8 秒，两 CLI 各 8 条记录，三项物理场景均正确报告 `not_exercised`，未虚报通过。

活体观察默认有 15 分钟上限，避免遗留任务。测试目录内创建名为 `stop` 的文件可提前停止；只清理本观察器 Popen 创建的 host，其 Job 负责隔离 PTY，不向正式服务发送停止命令。证据全部保留。失败后先定位证据，仅在改变前提后允许一次重试；产品回归另报，不为使测试通过修改产品代码或降低上述标准。

读取实时/最终结果：

```powershell
python scripts/verify-cli-power-physical.py --analyze --output <本次测试目录>
```

仅将实际观测到且达到阈值的场景标记通过；未操作、区间过短或未返回的场景保持未覆盖。两个受控 Python CLI 的物理运行验证不等于全部 Agent 的真实模型业务 UAT。

## Windows 观测依据

- [PowerSettingRegisterNotification](https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powersettingregisternotification)：只注册设置变化回调。
- [电源事件 GUID](https://learn.microsoft.com/en-us/windows/win32/power/power-setting-guids)：合盖、屏幕和供电状态含义。
- [WTSINFOEX_LEVEL1_W](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ns-wtsapi32-wtsinfoex_level1_w)：Windows 11 的锁定标志 0、解锁标志 1。观察器不保存用户名或域。
- [WTSRegisterSessionNotification](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsregistersessionnotification) 与 [WM_WTSSESSION_CHANGE](https://learn.microsoft.com/en-us/windows/win32/termserv/wm-wtssession-change)：直接锁定/解锁事件。
- [RegisterPowerSettingNotification](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerpowersettingnotification)：窗口电源事件的独立来源。
