# 原生 Codex 终端验收

用户要求保留原有终端。使用真正的 Codex CLI 0.153.4 原生 ELF、PTY 和已有 TerminalView。已有工作区可从首页或顶部“＋”→“新建 Codex 终端”进入；项目“＋”创建新工作树的原入口也保留。

## 最新增量：已有工作区直接新建终端

2026-10-20，新增已有工作区的原生 Codex 入口，默认选择受限工具。普通“新建终端”继续打开 shell；本机 Codex 必须在新窗口中显式选择“本机执行”。已有及导入的 Git 工作树均可使用受限模式，不再要求为此新建工作树。任务归属、Linux 内核能力和后端启动检查仍生效，没有放宽工具权限。

请求首次提交时固定 UUID 和执行模式。启动报错后先查询同 ID 的会话；若记录已经存在，明确说明尚未确认启动成功，提供“查看已有会话”，不重新启动。创建成功但布局失败时提供“继续打开”，每次重读最新 revision 并按会话去重，也能处理布局已经保存但响应报错的情况。打开前核对记录中的程序、会话类型和实际执行模式。已有会话列表与首页统一显示“Codex · 工具受限”，正常停止后的“可恢复”优先于“已结束”。

本轮浏览器夹具覆盖 **10 个场景**：已有工作树直接创建、启动失败重试、会话已保存后启动报错、布局保存前/后报错，以及 Git 主目录、普通文件夹、已取消任务保留的工作树、内核能力不可用、运行中任务。全部没有创建新工作树；两次启动请求只出现在未保存会话的重试场景，UUID 和受限模式保持一致。其余成功场景都只有一次启动、一条会话和一个标签；运行中任务两种模式均没有发出启动请求。6 条受限/本机不可用状态均实际禁用提交。任务阶段的浏览器样本是 cancelled 和 running，没有将 queued/delivered 声称为本轮实测。

真实浏览器从已经存在的验收工作树，经顶部“＋”打开新入口，使用原生 Codex 0.153.4 和本机已有登录调用 OpenAI gpt-6-astra。模型检查并仅修改 clamp.py，原有六项测试通过；固定后台独立六项测试再次通过，14 项实际内核/环境边界检查通过。后台仍只有原来的两个工作区（一个主目录、一个工作树）和一条受限 terminal 会话。正常 Stop 后显示历史回放，返回首页显示“可恢复”；本轮未再次执行恢复，恢复证据仍见后文 v3。

浏览器还验证菜单入口和首页入口的 Esc 关闭均回到各自触发按钮；存在一个已关闭标签时，在创建弹窗内按 Ctrl+Shift+T、Ctrl+Tab 不会恢复背景标签。发现的菜单焦点丢失和弹窗期间背景快捷键生效问题已修正。这些检查不代表完整无障碍或所有快捷键验收。

证据：

- `artifacts/browser-terminal-launcher-20261020-v1/result.json`：10 个模拟 RPC 浏览器场景及各自 JSON，含请求编号、次数、布局 revision 和禁用状态；没有调用真实模型或后台。
- `artifacts/browser-existing-terminal-20261020-v1/result.json`：真实后台 **16/16** 检查，`real_model_verified=true`；runtime 与上一轮相同，源码/二进制未变、工作区外 fixture 未变、网络探针 0 次连接、临时目录已删除、清理错误为空。
- 同目录 `browser-ui-result.json`：**18/18** 浏览器及清理检查；模型提示和手动边界命令从浏览器键盘输入，独立验证由 `verify.request` 触发固定后台脚本。`existing-workspace-result.json` 记录原工作区和唯一会话，`keyboard-result.json` 记录焦点及背景标签状态。
- `01-new-terminal-menu.png`、`02-restricted-launch-dialog.png` 展示新入口；`04-real-model-and-independent-tests.png` 展示实际修复和独立测试；`06-workspace-entry-and-recoverable-session.png` 展示首页入口及可恢复记录。
- 全部前端 **91/91** 测试通过，TypeScript 和生产构建通过，仍有既有大 chunk 提示。日志保存在真实验收目录。

真实浏览器警告/错误为空；测试标签、两个 Vite 进程及独立后台均已关闭。本轮更新开发代码及前端构建，未替换已安装的桌面应用；Windows/macOS 和其他既有能力限制没有变化。

## 当前实现：系统工具与同一会话恢复

原生 Codex CLI 0.153.4、PTY 和已有终端界面继续保留。本轮补齐可信系统工具，以及正常停止后恢复同一模型 thread 的流程；仍是实验性 Linux 支持。**真实模型的浏览器恢复验收已通过**，本轮后台 48/48、浏览器 16/16，证据与此前固定回复回归分别记录。未替换已安装的桌面应用。

`rg`、`node`、`git`、`python3` 从固定系统安装位置发现，实际目标及上级目录必须由 root 所有且普通用户不可写；不继承宿主 PATH。工具别名位于沙箱可写范围之外，通过独立只读规则提供。别名被替换、出现额外文件、目录链接或可写目录重叠时拒绝启动；不会自动修补后执行。新增工具可见性没有开放网络或宿主配置，工具 `OPENSSL_CONF` 固定为 `/dev/null`。

恢复使用原来的 Stop / Recover 控件，复用同一工作树、私有 CODEX_HOME 和原生 thread ID。私有、原子写入的 `terminal-resume.json` 绑定 thread、generation、工作区、CODEX_HOME、固定 CLI 版本和 ChatGPT 账号哈希；配置、环境注册、执行器配置和嵌入脚本必须与固定内容一致。账号变更在私有认证登录前拒绝，provider 配置变化也不能静默沿用旧历史。

恢复前只特许对指定 ID 的元数据读取；原生 `thread/resume` 成功后才允许执行。任意 thread、history/path 导入、fork、外部历史注入、配置或权限覆盖仍被阻止。固定环境保留 `include_local=false` 与 `velella-restricted` 默认值，每个后续模型 turn 再显式指定该环境。

手动 `!命令` 执行前将历史标记为 dirty；只有 `thread/inject_items` 确认保存命令和结果后才清除，后续模型请求也等待这个 ACK。首次手动命令即使发生在任何模型 turn 之前，其历史也能落盘。另加 `history_ready`：空终端只收到 `thread/start` 时不会被认为已有可恢复历史；手动注入完成，或模型完成后经内部持久化读取 ACK 确认，才允许恢复。恢复不会重跑尚未完成的命令。PTY 原有录制继续保留；这里不承诺 app-server 会重新生成每张桥接合成的命令卡片。

正常停止先冻结请求生产者，再清理工具进程组。执行器只有确认所记录进程和操作全部结束后才写私有 cleanup receipt，Rust 按 PID 与 `/proc` 启动时间核对；之后才写与本轮 generation 匹配的 `terminal-stopped.json`。下一次启动先失效旧 stop marker。SIGKILL、dirty 历史、空历史、缺少清理确认或 generation 不匹配均不开放恢复。

前端在确认退出或 host PID 变化时销毁旧输入队列，重建终端、重放原有尺寸记录，再协商新尺寸。输入、resize、control 带旧视图所见的 expected host PID，由 host 拒绝过期请求。host 退出后仍可能留下 Unix socket；恢复进程先持有 `owner.lock`，再通过 `listen_recovering` 仅回收当前用户拥有且已不可连接的旧 socket，保留描述符和历史。bridge 的私有 socket 在正常清理时删除。

### 已完成的最终离线回归

以下三份集成结果使用同一个最终 runtime，SHA-256 为 `f0aa1804e048ca3e8c51b263dfc61303c1a53da58110b21c9ede35b5833d8421`。

- `artifacts/restricted-tools-20261020-v4/result.json`：**64/64**，覆盖首次及再次使用四个工具、只读别名、替换拒绝、清除宿主 PATH/环境、工作区外及 auth/网络访问拒绝；未使用真实模型。
- `artifacts/restricted-terminal-recovery-20261020-v6/result.json`：**84/84**，同一原生 thread 正常停止后恢复 **两次**。第一次仅有手动命令历史，恢复后的固定模型请求包含原输出；恢复后模型工具与手动命令各通过 **12 项内核边界检查**，`rg` 可用，provider 改动被拒绝。新增空终端不可恢复且在启动前拒绝，以及两次恢复后旧 PID 的 input、resize、control 均被拒绝。源码和二进制哈希未变，已记录进程及临时目录清理完成，无清理错误。该验收使用本地固定回复服务；此前 v5 的 59 项记录继续保留。
- `artifacts/restricted-terminal-final-20261020-v1/result.json`：**101/101** 原生终端检查，覆盖真实 ELF 的 TUI/app-server 两角色、模型及手动命令边界、中文、管道、非零退出后继续、Esc 中断三代进程、尺寸录制、正常停止和 host SIGKILL 的 stdin lease 清理。源码/二进制未变，临时目录清理完成，没有清理错误；未调用真实模型。
- 最终 Rust 库 **135 项通过、6 项显式忽略**，bridge **44/44**，executor **28 项**，相关前端 **23 项**。前端生产构建通过，保留既有 chunk-size warning；忽略项不计为通过。
- `scripts/test-native-injected-history.py`：真实 0.153.4 app-server、临时 profile、零登录/模型请求，证明首次手动注入已写入 rollout，重启后恢复同 ID，继续写入且旧记录未重复。

### 当前限制

原生 TUI 是交互式终端，但 `!命令` 自身仍按非交互方式执行，默认 120 秒、256 KiB 输出、每个 shell adapter 最多执行 64 条。达到限制显示失败项，不放宽沙箱；正常恢复会创建新的 adapter，但保持原模型 thread。没有开放工具联网、附件、MCP 或插件；没有 Windows/macOS 实现或验证，也没有通用 CPU、内存、磁盘配额。

### 最新真实浏览器恢复验收（已通过）

证据目录：`artifacts/browser-real-recovery-20261020-v3/`。本机真实 OpenAI `gpt-6-astra` 复用已有 ChatGPT 登录，使用与最终离线回归相同的 runtime。模型修复 `clamp.py` 并运行六项测试通过，独立内嵌六项测试也通过；原测试及原始仓库未修改。

随后通过真实浏览器键盘，在旧会话仅用 `!printf` 打印随机 `RECOVERY_MEMORY_` 行，不写入工作区。经原有菜单正常停止、恢复后，thread ID 保持不变，generation、host 和两个原生进程角色重新建立。恢复提示没有携带该随机值，新的 assistant 回复准确复述旧输出，并实际调用 `rg` 检查代码、运行原六项测试。函数调用及返回从首次任务的 3/3 增至 4/4；恢复时一条合并命令完成了 `rg` 与 `unittest`，不能算作两个独立工具调用。

恢复后重新执行 **14 项边界检查**及**独立六项测试**，全部通过；旧 proof 文件先移出，新一轮必须重新产生。恢复阶段的候选源码及固定 profile 哈希未变，控制 HOME 和工具 HOME 均无 auth 文件副本，工具网络仍被拒绝。

同一窗口恢复后自动重新协商到 **54 行 × 92 列**。尺寸记录保留原来的两条记录，并追加新进程的 reset 和相同窗口尺寸；未通过后端强行指定尺寸来替代界面恢复。

- `result.json`：**48/48** 后台检查，`real_model_verified=true`、`real_model_recovery_verified=true`，源码/二进制哈希未变，工作区外 fixture 未变，网络探针连接数为 0，临时目录已删除，清理错误为空。
- `browser-ui-result.json`：**16/16** 浏览器操作及清理检查。模型提示、手动边界/记忆命令和恢复后命令使用真实键盘输入；首次独立验证由 `verify.request` 触发固定后台检查。
- `recovery-verification-complete.json`、`real-browser/recovery-before.json`、`real-browser/recovery-started.json`：同 thread/new generation、前后工具计数、记忆及新边界结果。
- `geometry-result.json`：旧尺寸记录保留，新 reset 后自动恢复相同窗口尺寸。
- `05-resumed-model-memory-and-tests.png` 是恢复后记忆和工具调用的主要截图；`06-recovered-independent-tests.png` 展示恢复后的独立验证。

最终已使用原有停止菜单结束终端；已记录后台进程、socket 和临时目录清理完成。浏览器标签已关闭，Vite 已停止；清理前浏览器 warning/error 为空。`07-cleanup-disconnect.png` 是主动停止本次 fixture daemon 后的预期失联画面，不是验证成功界面。

中间失败保留：本轮 `v1/result.json` 因误选普通“新建终端”被断言拒绝，未尝试模型；`v2` 已完成真实代码修复和模型六项测试，但超过 1024 字符的内联 `!命令` 被原生 TUI 识别为粘贴内容，独立验证超时，整体失败。两轮均已清理，不能计为成功。v3 改为 tool-home 内的 0444 固定脚本及短 `!/bin/bash "$HOME/...sh"` 命令，记录脚本哈希，完整保留 14 项边界和六项测试；对应 harness 离线测试 **8/8 通过**。这一修复针对验收输入方式，没有修改原生 Codex 或降低生产沙箱限制。

## 前次真实模型首次任务验收（已通过）

2026-10-20，在本机浏览器实际创建“真实模型＋自研沙箱”工作树，复用已有 ChatGPT 登录，使用真实 OpenAI `gpt-6-astra`。模型通过原生 Codex 终端完成读取、修改 `clamp.py`、运行测试和回复；没有使用固定回复服务。三个真实工具调用均有对应返回，完成事件及最终回复已记录。

模型将错误的 `return min(value, lower)` 修正为正确的区间截断，并增加反向边界的 `ValueError` 检查。模型自行运行六项测试通过后，又通过原生 `!命令` 在同一沙箱执行独立、内嵌的固定六项测试，全部通过。测试文件及原始仓库未变，只有工作树内的候选文件改变。

**14 项真实边界检查全部通过**，包括工作区外读写、控制目录访问、本机 auth.json 打开及网络连接均被拒绝；工具环境中没有宿主代理变量。控制进程通过本机已有代理连接模型服务，工具仍断网。控制进程与工具 HOME 均未创建 auth.json 副本。

本轮补齐了两个真实接入缺口：

- 对固定版本原生 app-server 使用私有、进程内的外部 ChatGPT 认证，仅传 access token 和账号标识，不复制完整登录文件或传递 refresh token，不自行刷新/修改本机登录。只对未覆盖的内置 OpenAI provider 启用。
- 仅向可信模型控制进程继承宿主的八个标准代理环境变量。此前本机直连模型服务超时，继承已有代理后调用成功；沙箱命令的环境和网络限制保持不变。

浏览器实际键盘输入和粘贴完成边界命令、模型请求及独立测试。原有停止菜单正常，停止后历史可见，浏览器 warning/error 为空。已记录的进程、socket 和临时目录清理成功；浏览器标签和 Vite 已关闭。

这次模型任务从 `task_started` 到 `task_complete` 为 **14.36 秒**，包含模型调用和工具执行，只是单次小任务样本。该历史版本的 `rg` 不在沙箱 PATH 中，模型改用 `ls`/`cat` 后完成任务；当前可信工具实现已处理这一缺口。原生 CLI 显示 Code Mode 未启用并回退到直接工具；本次直接工具执行成功。

证据目录：`artifacts/browser-real-model-terminal-20261020-v3/`。

- `result.json`：真实后台验收通过、15 项检查、源码/二进制哈希及清理结果。
- `browser-ui-result.json`：真实浏览器操作与截图索引。
- `observed-auth-and-policy.json`：实际模型/provider、内存认证、固定策略和任务耗时。
- `real-browser/model-evidence.json`：三个工具调用及返回、最终完成事件；仅提取计数，不保存原始认证或请求 headers。
- `real-browser/verification-output.txt`、`verification-result.json`、`clamp.py`：独立测试和真实修改结果。
- `03-real-model-fix-and-tests.png`、`04-independent-tests.png`、`05-stopped-history.png`：原生终端内的真实过程。

该轮另通过 30 项桥接协议测试、7 项 Rust 沙箱配置测试、4 项相关前端测试，runtime 构建、前端类型检查及构建通过。该轮 runtime SHA-256 为 `4175f811577384a6bd78a97ccd8a327ecf6f55d0f3e434bb7c71f7790852353c`。

中间失败证据保留在 v1/v2 目录：v1 为测试脚本未处理启动中 `status=null`，已修复；v2 已进入真实请求，但遗漏宿主代理造成超时，已修复。两次临时环境均已清理，不能将它们标成成功。

这次真实验收范围是本机 Linux、固定 CLI 和当时账号，不包含会话恢复。Windows/macOS、附件、插件没有在该轮验收；已安装的桌面应用未替换。当前恢复实现和固定回复证据见上文，下文继续保留此前模拟服务的历史基线。

## 前次修复与模拟验收

2026-10-20，修复原生 `!命令` 被拒绝后退出 TUI 的问题。固定版本的上游源码将 `thread/shellCommand` 定义为本机 shell 通路，要求本地执行环境，直接构造不经外部执行器的请求。受限模式禁用了本地环境，TUI 又把这个错误向上传播，因而退出。

现在原生 TUI 通过私有 Unix WebSocket 桥接至同一原生 app-server。手动命令进入固定自研执行器，保持与模型工具相同的目录白名单、独立 HOME 和断网策略；命令结果由原生 TUI 显示，写入会话历史成功后才放行下一次模型请求。原生 ELF 未修改。

该历史桥接基线采用显式 RPC 白名单、固定工作目录和执行环境，拒绝宿主执行/文件接口、配置写入、MCP、插件、附件、恢复和外部历史注入。当前版本仅新增上文描述的指定 thread 恢复，其他边界保留。socket 位于工具不可写的私有目录。停止或 host 意外退出均触发本次执行器、工具进程树和 socket 清理；意外退出不因此取得恢复资格。

## 实测

- 原生后台完整验收 **102/102** 通过，包括真实 Git 工作树、原生 ELF/PTY、输入输出及缩放、幂等启动、拒绝降级与恢复。
- 模型工具和手动命令各通过 **12 项真实边界检查**，覆盖工作目录、独立 HOME、移除宿主环境变量、工作区外及控制目录读写拒绝、网络拒绝。工作树恶意 provider/MCP/hooks 配置未执行、未收到连接。
- 原生 `!命令` 验证中文、管道、重定向、非零退出、失败后继续命令、Esc 中断三代进程树、中断后继续命令。真实 app-server 发往固定服务的后续请求包含手动命令结果。
- 停止会话后所有已记录的 CLI、桥接、执行器及工具进程停止，socket 和私有目录删除。另以 pidfd 向本次 host 发送 SIGKILL，验证 stdin lease 触发完整清理。
- **20/20 协议测试**覆盖 WebSocket 边界、RPC 拒绝、排队与取消、分流 UTF-8 和分页输出、历史 ACK、超时、输出上限及第 65 条命令。资源限制使用模拟执行器；超时项使用 20ms，没有将它写成原生 120 秒实测。
- Rust 库 **126 项通过、6 项显式跳过**；相关前端 **4 项单测通过**，Rust 检查与 runtime 构建、前端 TypeScript 检查与 Vite 构建通过。

本次原生运行测得 Esc 中断 0.019 秒、手动命令会话停止 0.362 秒、host SIGKILL 后清理 0.389 秒。这是单次本机验收耗时，不是性能基准或跨平台结论。

## 真实浏览器

浏览器从项目“＋”创建“终端命令修复验收”，选择原生 Codex 终端与受限工具，看到 v0.153.4 原生首屏和“Codex · 工具受限”标签。使用实际 `typeText` 和 `Return` 依次输入：

```sh
!printf 'SHELL_OK 中文命令执行正常\n' | tee shell-check.txt
!printf 'FAILED_COMMAND_VISIBLE\n'; exit 7
!printf 'AFTER_FAILURE_OK\n'
```

原生终端依次显示成功、失败、成功，中文和管道写文件正常。随后输入“请返回预设的本地模拟回复。”，看到固定回复“本地模拟回复：真实 Codex 终端已接入自研受限工具，未调用真实模型。”。

通过原有“终端操作 → 停止进程 → 确认”停止后，显示“进程已结束 · 历史回放”并保留上述记录。控制台 warning/error 为空。此验收操作实际 xterm 输入，未用设置 textarea 值代替终端提交。浏览器后台记录一次正文、零次标题请求，无服务错误。会话 host、桥接、app-server、执行器及独立 daemon 均已退出，私有 socket 和临时目录移除；浏览器标签与 Vite 已关闭。

## 验证记录

- `artifacts/desktop-restricted-terminal-shell-v3/result.json`：该次 102 项原生检查、源码和二进制哈希、固定服务请求、进程树与清理证据。
- `artifacts/browser-restricted-terminal-shell-20261020-v2/`：本次真实浏览器截图、终端二进制录制、固定输出、后台与清理证据。`browser-ui-result.json` 汇总 15 项界面及清理检查；`04-shell-and-model-reply.png` 展示手动命令和后续回复，`05-stopped-history.png` 展示停止后历史。
- `scripts/test-restricted-terminal-bridge.py`：20 项协议测试。
- `artifacts/terminal-shell-bridge-diagnostic-v1/result.json`：修复后的前期独立手动命令验收，41/41 通过。
- `artifacts/desktop-restricted-terminal-shell-v2/result.json`：保留中间失败记录。旧测试假设必有一次标题请求；加入手动命令历史后原生 CLI 可以不发标题请求。v3 允许 0 或 1 次标题请求，仍严格验证全部正文请求和输出。
- `artifacts/desktop-restricted-terminal-v1/` 与 `artifacts/browser-restricted-terminal-20261020-v1/`：前次 46 项原生基线与浏览器证据，保留历史，不代表本次 `!命令` 行为。
- `artifacts/restricted-terminal-ui/result.json`：前次 26 项完整 WorkspaceApp 模拟错误/重试检查，与原生 CLI 验收分开。

前次模拟验收 runtime SHA-256：`cb6b7491a959d7a81dff2e92d8212102e56307fbf58ce76788ac8c815eec2bf0`。

原生 Codex SHA-256：`56ef98ab4032d317ab26e9b5e5a175650717351edb16ed9cde0cb6d1734d62da`。

## 前次模拟验收的范围与限制

模型回复来自只监听 loopback 的固定服务，使用独立临时 HOME/CODEX_HOME，未登录或调用真实模型。认证隔离证据是隔离目录和最小环境，并非系统调用审计。主原生验收有三次正文请求、零次标题请求；独立手动命令和 host 崩溃场景均为零次模型请求。

该历史验收的手动命令默认每条最多 120 秒、输出 256 KiB，每个 shell adapter 最多 64 条。达到限制显示失败项，终端保持可用。这些数值仍保留；当前正常恢复会创建新 adapter。该历史版本未开放 Codex 会话恢复，只有终端录制回放；当前仅按上文条件恢复同一 thread。附件、MCP、插件仍未开放，手动命令仍按非交互执行。

目录和断网限制是沙箱边界。原生首屏可能显示 `YOLO mode`，它描述 Codex 自身权限设置，外部执行器仍强制固定策略。

这次历史模拟验收只覆盖 Linux 本机及固定 CLI 0.153.4，不覆盖真实模型、Windows/macOS、全部终端快捷键或完整无障碍。后续真实模型首次任务结果见上文，不改变该轮证据性质。未替换用户已安装的桌面应用。临时浏览器、Vite 与独立后台在结束时清理，结果见浏览器证据目录。
