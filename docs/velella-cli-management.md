# Velella CLI — 鱼群管理指南

`velella-cli` 管理本机 Velella 鱼群的项目、工作区、Agent 终端、任务和 v2 协作编排。它是本仓库构建的独立程序，通过当前用户的本地 IPC 连接已有桌面运行时。先启动 Velella；CLI 不会自行启动、重启或升级桌面运行时。

产品名称为 Velella（鱼群），管理命令为 `velella-cli`，可独立安装。

## 开始使用

```powershell
velella-cli --version
velella-cli status --json
velella-cli project list --json
velella-cli project add "D:\my-project" --json
velella-cli workspace list --json
velella-cli task list --json
velella-cli swarm list --json
velella-cli swarm capabilities --json
velella-cli skills bundled --json
velella-cli skills get velella-cli
velella-cli skills get velella-collaboration
```

所有 ID 都从 list/show 的实际返回复制。`--data-dir <目录>` 指定本地配置目录；默认遵循 `VELELLA_DATA_DIR`，再使用 `%LOCALAPPDATA%\Velella`。它不连接原版 Orca，也不跟随桌面的 SSH 主机选择。

`--json` 输出单个 JSON：成功为 `{"ok":true,"result":...}`，失败为 `{"ok":false,"error":...}`。成功退出 0，运行失败退出 1，参数错误退出 2。`--timeout-ms` 范围 1–60000，默认 45000。无自动写入重试。

`skills bundled/get` 离线读取当前 CLI 随版本内置的指南，不连接或更改运行时；SSH 参数不改变指南来源。会话消息、成员和工作回执使用直接 IPC 的 `velella-cli collab`（别名 collaboration），完整用法由 `skills get velella-collaboration` 提供；原 `velella-runtime collab` 继续兼容。collab 拒绝 SSH 参数，保留会话身份、人工加入确认、稳定请求 ID 和角色权限，不通过 MCP 转发普通 CLI 操作。

## 管理鱼群编排

```powershell
velella-cli swarm preflight --workspace <工作区ID> --json
velella-cli swarm create --workspace <工作区ID> --goal "实现目标" --acceptance "明确的总体验收条件" --agent codex --request-id <UUID> --json
velella-cli swarm show <编排ID> --json
velella-cli swarm members <编排ID> --json
velella-cli swarm work <编排ID> --json
velella-cli swarm events <编排ID> --json
```

create 启动规划 Agent，需要已有 Git 提交和可用的 CLI 登录/信任配置。支持主控 `codex`、`claude-code`、`omp`、`opencode`。不要通过管理命令自动同意原生登录或信任提示。`swarm show` 返回方案、revision、步骤、阶段、阻塞和成员状态；实际可用性以当前运行时回执为准。

可以把 show 返回的 `result.run.plan` 保存为 JSON 编辑，再按当前 revision 提交；保存会递增 revision，应再次 show，然后确认新 revision：

```powershell
velella-cli swarm plan save <编排ID> --revision <当前版本> --file plan.json --request-id <UUID> --json
velella-cli swarm plan refine <编排ID> --revision <当前版本> --instruction "调整方案的要求" --request-id <UUID> --json
velella-cli swarm plan confirm <编排ID> --revision <当前版本> --confirm --request-id <UUID> --json
velella-cli swarm pause <编排ID> --request-id <UUID> --json
velella-cli swarm resume <编排ID> --request-id <UUID> --json
velella-cli swarm stop <编排ID> --confirm --request-id <UUID> --json
```

确认方案后才派发实现工作。pause 暂停新派发，正在进行的工作可完成；stop 请求停止该编排拥有的会话，完成停止需再次 show 确认 `state=stopped`。已经启动的方案由运行时冻结。

```powershell
velella-cli swarm retry <编排ID> --node <步骤ID> --cause-key config-mismatch --new-evidence "已定位根因并修正前置条件的证据" --request-id <UUID> --json
velella-cli swarm gate <编排ID> --node <步骤ID> --revision <已确认版本> --decision approve --summary "人工确认依据" --request-id <UUID> --json
velella-cli swarm accept <编排ID> --summary "总体验收结论" --confirm --request-id <UUID> --json
```

重试仍受同根因两次失败上限约束；人工 gate 仍需技术验收；accept 仍需独立总体验证和候选未漂移。管理 CLI 不替代这些验收。accept 不发布、不推送或合并代码。

每次写入建议预先指定 UUID。传输结果不确定时，检查 show、events 和保存的回执；仅以相同 UUID 和完全相同参数恢复同一个操作。accept 使用运行时已有候选检查与接受状态；它没有独立的 request_id 持久回放契约，响应丢失时先检查状态。不要换 UUID 重建编排或重复发送提示。

`project add` 按规范化路径识别同一个项目，不使用 UUID。响应丢失时，错误回执保留 path，可用 `project list` 核对该路径。

## 管理单个任务

```powershell
velella-cli task template > task.yaml
# 编辑 task.yaml 的名称、指令以及 done 验证命令
velella-cli task create --project <项目ID> --file task.yaml --request-id <UUID> --json
velella-cli task show <任务ID> --json
velella-cli task events <任务ID> --since 0 --json
velella-cli task read <任务ID> --cursor 0 --json
velella-cli task diff <任务ID> --json
velella-cli task verify <任务ID> --json
velella-cli task feedback <任务ID> --text-file review.txt --request-id <UUID> --json
velella-cli task feedback-status <任务ID> --request-id <原反馈UUID> --json
velella-cli task resume <任务ID> --json
velella-cli task cancel <任务ID> --confirm --json
```

任务文件为 UTF-8 YAML/JSON（64 KiB 以内）；Windows PowerShell 5 的 `>` 默认可能产生 UTF-16，请用支持 UTF-8 的编辑器保存，或使用 PowerShell 7。任务默认只本地审查 `on_done: review`，CLI 创建时跳过项目准备脚本。若项目需要依赖准备，请在任务运行前完成。创建会进入队列并启动 Codex 或 Claude Code；独立任务暂只支持这两个 provider。

`task show` 包含验证命令及其结果；verify 的成功响应只表示启动验证，不代表验证已通过。未配置 done 检查时，运行时会明确标为未验证。events 每页最多 500 条，继续传最后一条 seq；read 游标为字节偏移，使用返回的 next_cursor 继续，完整保存时拼接 base64 data 再解码 UTF-8。`--target command` 读取验证命令，`--target shell` 读取辅助 shell。

feedback 成功仅表示输入到达 PTY，不能证明模型已开始执行。反馈被拒绝或不确定时 CLI 返回失败并携带回执。resume、verify、cancel 没有持久重放契约，响应丢失后先检查状态，再决定下一步。

## 管理 Agent 终端

`agent` 是 `terminal` 的别名，使用现有工作区，不重复建立 Agent 注册表：

```powershell
velella-cli agent list --workspace <工作区ID> --json
velella-cli agent create --workspace <工作区ID> --launch codex --request-id <UUID> --json
velella-cli agent show --workspace <工作区ID> --terminal <会话ID> --json
velella-cli agent read --workspace <工作区ID> --terminal <会话ID> --cursor 0 --json
velella-cli agent send --workspace <工作区ID> --terminal <会话ID> --text-file prompt.txt --enter --request-id <UUID> --json
velella-cli agent stop --workspace <工作区ID> --terminal <会话ID> --confirm --json
```

create 的 launch 支持 terminal、codex、claude-code。原生提示投递支持 Codex/Claude；普通 shell 发送不用 request-id，使用 `--expected-sequence` 防止过期输入。停止任务所属终端会停止整个任务。详细终端游标和输入语义参见 `docs/velella-cli.md`。

## 更多管理入口

`velella-cli schema --json`（别名 `agent-context`）提供离线命令发现。子命令的具体参数可通过 `--help` 查看。

```powershell
velella-cli project configure <项目ID> --default-base main --setup-default skip --confirm --json
velella-cli workspace update <工作区ID> --name "开发工作区" --favorite true --json
velella-cli workspace update <工作区ID> --no-parent --json
velella-cli workspace archive <工作区ID> --confirm --json
velella-cli workspace restore <工作区ID> --json
velella-cli workspace layout <工作区ID> --json
velella-cli workspace layout-save <工作区ID> --file layout.json --json
```

归档仍要求会话已停止。layout-save 接受 `workspace layout` 返回的完整 layout 对象及 revision，版本冲突时应重新读取后编辑。项目准备脚本可通过 `project configure --setup-file` 保存；配置不立即执行脚本。

```powershell
velella-cli terminal wait --workspace <工作区ID> --terminal <会话ID> --for exit --deadline-ms 30000 --json
velella-cli terminal wait --workspace <工作区ID> --terminal <会话ID> --for idle --json
velella-cli terminal wait --workspace <工作区ID> --terminal <会话ID> --for output --contains "完成标记" --json
velella-cli terminal screen --workspace <工作区ID> --terminal <会话ID> --json
velella-cli terminal rename --workspace <工作区ID> --terminal <会话ID> --title "后端开发" --json
velella-cli terminal switch --workspace <工作区ID> --terminal <会话ID> --json
velella-cli terminal split --workspace <工作区ID> --terminal <会话ID> --direction vertical --request-id <UUID> --json
velella-cli terminal recover --workspace <工作区ID> --terminal <会话ID> --confirm --json
```

wait 不发送输入；idle 依据运行时的 waiting_input 状态，权限菜单不算空闲；output 从已保存输出开始匹配，支持跨字节分页。screen 使用 VT100 重放和原生尺寸日志，返回逻辑画面、捕获截止游标和 complete_history；不提供图像或像素截图。缺少尺寸日志时拒绝猜测，单次最多重放 16 MiB，历史不完整会明确标记。

switch 需要同 profile 的兼容桌面在线，切换项目、标签和分屏并确认终端输入焦点；已关闭的标签可重新展示，终端进程不创建、不恢复、不停止。归档工作区、原生聊天界面、SSH 路由和打开的对话框/菜单会拒绝；桌面当前为 SSH 时需先切回本机。成功只表示界面焦点，不代表 PTY 输入就绪或模型执行。超时后选择可能已改变，不自动重试。旧桌面不支持时应升级桌面界面，而不是重启后台。

rename 修改工作区的终端**标签标题**，可在 workspace layout 中读回；会话自身的 title 元数据保持原值。split 在当前标签内创建终端分屏，vertical 为左右、horizontal 为上下。若新会话已经创建而布局保存冲突，会返回 session_id/request_id 并保留会话；重新读取布局，以同一个 UUID 恢复，不盲目另建进程。recover 沿用运行时的会话恢复限制，不自动处理登录/信任提示。

```powershell
velella-cli file list --workspace <工作区ID> --json
velella-cli file read --workspace <工作区ID> src/main.rs --json
velella-cli file search --workspace <工作区ID> "关键字" --json
velella-cli file create --workspace <工作区ID> note.txt --file content.txt --json
velella-cli file write --workspace <工作区ID> note.txt --file next.txt --previous-file previous.txt --json
velella-cli file rename --workspace <工作区ID> note.txt renamed.txt --json
velella-cli file remove --workspace <工作区ID> renamed.txt --confirm --json
velella-cli git status --workspace <工作区ID> --json
velella-cli git diff --workspace <工作区ID> --staged --json
velella-cli git stage --workspace <工作区ID> src/main.rs --json
velella-cli git unstage --workspace <工作区ID> src/main.rs --json
velella-cli git commit --workspace <工作区ID> --message "本地审查后的提交" --confirm --json
```

文件路径相对目标工作区，由后端检查路径越界、受保护目录和写权限。文本输入为 UTF-8，文件内容最多 2 MiB；write 必须提供你之前读取并保存的完整内容，磁盘内容变更时拒绝覆盖。file search 搜索代码文件，不搜索 Agent 会话。Git 提交仅在本地执行，不推送、不发布。

## 搜索终端录制

```powershell
velella-cli terminal search "完成标记" --workspace <工作区ID> --json
velella-cli terminal search "完成标记" --workspace <工作区ID> --terminal <会话ID> --json
velella-cli terminal search "完成标记" --workspace <工作区ID> --terminal <会话ID> --cursor <next_cursor> --snapshot-end <snapshot_end> --json
```

`terminal search`（也可用 `agent search`）对指定工作区的 Velella PTY 录制做区分大小写的 UTF-8 字节精确匹配，返回 `representation:raw-terminal-recording`、会话 ID、`byte_start/byte_end`、片段及每会话扫描回执。它不读取原生 Codex/Claude 对话文件，不建立原生对话全文索引，也不从渲染屏幕推断文字；ANSI 控制码保留，所以肉眼连续显示但中间夹有控制码的文字可能无法按连续字节找到。片段只是有界的 UTF-8 预览；可用 `terminal read --cursor <byte_start> --limit <命中UTF-8字节数>` 的 base64 `data` 验证准确原文。

每个会话的首次读取固定 `snapshot_end`；搜索期间新增输出不纳入该范围。跨 IPC 页、跨 UTF-8 字节边界和重叠命中均保留。默认每会话最多比较 1 MiB（`--max-bytes`，上限 16 MiB），最多扫描 50 个会话（`--max-sessions`，上限 200），总共最多返回 100 个命中（`--max-matches`，上限 500），总观察期限为 30 秒（`--deadline-ms`，上限 60 秒）。IPC 仍按现有 128 KiB 页读取；会话按 ID 排序。`--max-bytes` 必须不小于查询长度，查询为 1–4096 UTF-8 字节。

必须检查 `complete`、`omitted_sessions` 和 `sessions[].error/range_complete`。`complete:true` 仅表示本次指定会话和指定字节范围已搜索完；指定 `--terminal` 时范围只有该会话，指定 `--cursor` 时不包含其前缀，固定 end 也不包含后续追加内容。`complete:false` 且无命中不能当作“全部没有”。超出会话数或命中数时，不会自动搜索其余会话；可从 `terminal list` 选择具体 ID 继续。读取失败、录制缩短或游标漂移会记录错误并清除该会话的暂存命中，继续处理其他会话；错误时 `has_more:null`，成功截断时为 `true`。

对因字节数或命中数限制而截断的会话，使用其 `next_cursor` 和 `snapshot_end`，保持同一查询及精确会话 ID 续查。游标可能小于 `scanned_to`：字节限制时保留少于一个查询长度的重叠，命中限制时从最后命中起点的下一个字节继续，以保证边界命中不遗漏且不重复。`--cursor` 和 `--snapshot-end` 都要求 `--terminal`。录制被截断、重写或恢复后不要沿用旧范围；现有 API 没有录制版本标识，相同长度的外部重写不可检测，因此续查要求录制保持追加稳定。

搜索复用 daemon 的工作区归属和输出读取校验，不直接打开会话目录或数据库、不发送终端输入。已退出且仍保留录制的会话可以搜索；归档工作区须先恢复，无法读取的旧式原生聊天会话会明确报告。`native_id` 优先采用观察到的 host 状态，其次采用会话元数据，并用 `native_id_source` 标明来源；未知为 null，不把它当作原生对话文件的映射或已验证认证状态。

## 自动化

```powershell
velella-cli automation list --json
velella-cli automation create --project <Git项目ID> --file task.yaml --start-at "2026-10-30T09:00:00+08:00" --interval-minutes 60 --confirm --json
velella-cli automation show <自动化ID> --json
velella-cli automation save <自动化ID> --file reviewed-automation.json --confirm --json
velella-cli automation pause <自动化ID> --json
velella-cli automation run <自动化ID> --confirm --json
velella-cli automation runs --automation <自动化ID> --json
```

创建默认禁用；加 `--enabled` 才自动排期。开始时间接受带时区的 RFC3339 或 Unix 秒；间隔 0 为一次，单位为分钟。恢复先读取 automation show 的 automation 对象，审查完整配置后把 enabled 改为 true，再 save。保存及立即运行需要显式确认，仍受后端的任务权限、人工审查、项目策略漂移检查约束。

自动化 save/run 没有请求级重放契约，响应不确定时先查列表及运行记录，不盲目重建或重复运行。运行历史接口最多返回全局最近 200 条，按自动化筛选不代表完整历史。当前后端没有自动化删除接口，pause 禁用后保留记录。

## SSH 主机与远程执行

```powershell
velella-cli host list --json
velella-cli host save --file ssh-profile.json --confirm --json
velella-cli host show <SSH配置ID> --json
velella-cli host inspect <SSH配置ID> --json
velella-cli host diagnose <SSH配置ID> --json
velella-cli host connect <SSH配置ID> --workspace <本机工作区ID> --reviewed-file reviewed-profile.json --confirm --json
velella-cli host remove <SSH配置ID> --confirm --json
```

profile JSON 例如 `{"name":"测试主机","host":"example.invalid","user":"dev","port":22}`。更新现有配置时包含其 id；identity 是已有本机密钥文件的绝对路径引用，不读取或保存私钥内容。inspect 返回实际目标、参数和 profile；connect 的 reviewed-file 接受已审阅的完整 profile 对象，配置变更后会拒绝旧确认。connect 创建本机 SSH 交互终端，可通过 terminal read/send 操作；diagnose 只展开本机 OpenSSH 配置，不联网。

远程管理使用另一个明确入口，先检查远端身份：

```powershell
velella-cli host inspect-runtime <SSH配置ID> --remote-root /home/dev/Velella --json
velella-cli --ssh-profile <SSH配置ID> --server-id <检查返回的server_id> --remote-data-dir <检查返回的data_root> status --json
velella-cli --ssh-profile <SSH配置ID> --server-id <server_id> --remote-data-dir <data_root> project list --json
```

远端必须已独立运行兼容的 Velella daemon，且 SSH 环境可执行 `velella-runtime remote-stdio`。每次连接校验同一个 server_id/data_root，再确认并发送一次 RPC；它不借用 Electron 的连接。使用 BatchMode、严格主机密钥校验、禁用转发，首次 SSH 信任或登录须事先由用户在系统 SSH 中完成。

`--data-dir` 始终选本机配置目录，`--remote-data-dir` 始终选远端运行目录；不能互相替代。远端 project add 的路径由远端解释；`--file` 等输入文件仍从本机读取。支持远端项目、工作区、终端、文件/Git 和任务管理；浏览器、自动化、编排、SSH/资源配置不在远端白名单，不回退到本机执行。远端进程跨 SSH/重启存活性仍以运行时的 prestarted_unverified 记录为准。超时不重放写入；先检查目标与回执。

## 原生 Agent、技能与 MCP

```powershell
velella-cli agent providers --json
velella-cli agent check --provider codex --json
velella-cli skills list --json
velella-cli skills show <技能名> --json
velella-cli mcp preview C:\catalog\mcp.json --json
velella-cli mcp confirm C:\catalog\mcp.json --digest <预览摘要> --entry <条目ID> --tool <工具名> --request-id <UUID> --confirm --json
```

agent check 仅诊断安装和认证状态，不登录或切换账号。skills 发现运行时原生技能，重名时用 list 返回的完整 `--path` 选择；show 只读取文本并标记为不可信内容，不执行。MCP 来源确认必须匹配预览摘要并明确选择工具；可重复 `--tool`。来源变化、越界工具、同一 UUID 被挪作他用时拒绝，不执行 MCP server 来“试装”。

技能包还支持本机安装、备份移除和恢复，无需 daemon 在线：

```powershell
velella-cli skills preview C:\packages\my-skill --json
velella-cli skills install C:\packages\my-skill --name my-skill --digest <预览摘要> --confirm --json
velella-cli skills inspect my-skill --json
velella-cli skills remove my-skill --digest <检查摘要> --confirm --json
velella-cli skills backups --json
velella-cli skills restore <备份UUID> --digest <备份摘要> --confirm --json
```

这些命令只操作当前 CLI 用户的原生目录，默认 `--target agents` 对应用户目录下 `.agents/skills`；`--target codex` 对应 `.codex/skills`。用户目录依次取 `USERPROFILE`、`HOME`，与 runtime 技能发现一致；`--data-dir` 不改变该目录。`skills list/show` 仍由所选 daemon 提供，daemon 与 CLI 用户环境不同时必须分别确认。新增本机命令拒绝 `--ssh-profile`，不回退到本机执行远程操作。

源必须是含 `SKILL.md` 的目录；安装复制完整资源并保持文件权限，不执行脚本、不联网下载。摘要与 runtime 原生技能摘要一致。包最多 2048 文件、32 MiB，`SKILL.md` 最多 256 KiB；忽略 `.git`，拒绝符号链接、Windows junction/reparse point 和越界名称。安装及恢复都不覆盖已有名字；升级须先检查并备份移除旧包，再安装新版。

移除把整个目录移入同一原生配置目录的 `velella-skill-backups/<UUID>/package`，摘要和原名保存在 `receipt.json`。`backups` 只列回执和包是否存在，不声称已校验内容；恢复时重新校验摘要。恢复后保留回执，`package_present:false` 表示备份包已不在原位。备份不自动清理。安装中断可能留下 `velella-skill-stage-<UUID>`，它位于技能发现目录之外；先检查状态再处理，命令不自动重试。Velella CLI 之间以目录锁互斥；不要同时用其他编辑器修改同一个包。

安装/恢复后下次原生技能扫描可发现；移除后下次扫描不再发现。现有会话冻结的技能快照保持原状。原生 Agent 是否需要重新启动才能加载新技能取决于其自身机制，此处不将目录发现等同于实际模型调用验收。

## Codex 原生 stdio MCP 配置

这组命令与角色 MCP 目录确认分开，直接管理本机 `CODEX_HOME/config.toml`；未设置 `CODEX_HOME` 时使用当前用户目录下 `.codex/config.toml`。`--data-dir` 不改变原生配置位置，`--ssh-profile` 会在读取 SSH 配置前被拒绝。不启动 MCP server、不登录、不调用模型，也不改变已经运行的 Agent；配置由后续原生 CLI 启动或配置扫描读取。

准备 UTF-8 JSON：`{"command":"uvx","args":["my-mcp"],"env_vars":["MY_API_KEY"],"enabled":true}`。只支持 stdio 的 command、args、继承环境变量名称 env_vars 和 enabled；不接收明文 env、HTTP、Claude 配置或额外字段。命令和参数不会被执行或回显，预览只给出摘要、条目名、参数数量和环境变量名称。

```powershell
velella-cli mcp codex inspect --json
velella-cli mcp codex preview my-server --file server.json --json
velella-cli mcp codex install my-server --file server.json --config-digest <config_digest> --spec-digest <spec_digest> --confirm --json
velella-cli mcp codex remove my-server --config-digest <最新config_digest> --confirm --json
velella-cli mcp codex backups --json
velella-cli mcp codex restore <备份UUID> --config-digest <该操作after_digest> --confirm --json
```

预览不写配置；首次配置不存在时摘要为 `absent`，与空文件不同。安装要求配置整份文件摘要和源 JSON 摘要均与预览一致，同名不覆盖。保留其他 TOML 字段和注释；统一 CRLF 文件继续使用 CRLF，混合换行由 TOML 编辑器规范化。配置上限 1 MiB，源 JSON 上限 64 KiB；路径拒绝符号链接与 Windows junction/reparse point，CODEX_HOME 必须为绝对路径。无效配置错误不会打印原始 TOML 或命令参数。

每次实际变更先在 `CODEX_HOME/velella-mcp-backups/<UUID>` 保存整份原始文件、摘要和回执，再发布新文件并回读。该目录及新文件设为仅所有者可访问（Unix 0700/0600；Windows protected Owner Rights DACL）。原始配置可能含私密值，备份不自动清理；CLI 不回显其内容。恢复要求当前配置仍等于选定操作的 after_digest，禁止覆盖后续修改，并为本次恢复另建备份；原来无文件时恢复为无文件。恢复按原始字节进行，不声称恢复原文件的 ACL 或时间戳。

`backups` 列审计回执与 publication marker 是否存在，不宣称备份内容已验真；恢复会核对备份字节。进程中断或发布失败时保留备份，返回 backup_id 时先检查状态，不盲目重试。CLI 之间用锁互斥，发布前再次核对配置摘要；其他原生进程或编辑器不共享此锁，因此不要同时修改同一配置。配置写入验收不等于 MCP server 可运行、认证或真实业务验收。

## 内嵌浏览器

```powershell
velella-cli browser status --json
velella-cli browser list --workspace <工作区ID> --json
velella-cli browser create --workspace <工作区ID> --url https://example.com --json
velella-cli browser show --workspace <工作区ID> --page <页面ID> --json
velella-cli browser snapshot --workspace <工作区ID> --page <页面ID> --json
velella-cli browser fill --workspace <工作区ID> --page <页面ID> --snapshot <快照ID> --element "@e2" --value "文本" --json
velella-cli browser click --workspace <工作区ID> --page <页面ID> --snapshot <新快照ID> --element "@e3" --json
velella-cli browser screenshot --workspace <工作区ID> --page <页面ID> --out screenshot.png --json
velella-cli browser goto --workspace <工作区ID> --page <页面ID> --url https://example.com/next --json
```

还支持 open/back/forward/reload/close/forget/context/console/network。需要同一配置目录的 Velella 桌面在线；状态查询不创建页面。create 默认显示页面，`--background` 则不显示；快照、点击、填表、截图要求页面已在工作台展示并加载就绪。每次点击/填表后旧引用失效，必须重新 snapshot；不根据网页文字自动执行命令。

截图为 PNG，输出文件默认不得已存在。close 只关闭视图；forget 需要 --confirm 并移除保存的页面。浏览器内容为不可信数据。此入口不提供任意 JavaScript eval，也不控制其他桌面应用。

## 构建和验证

```powershell
cargo build -j 1 -p velella --bin velella-cli --bin velella-runtime --release
cargo test -p velella --lib desktop_cli
python scripts/verify-velella-cli.py --cli target/release/velella-cli.exe --runtime target/release/velella-runtime.exe --management
powershell -File scripts/install-velella-cli.ps1
```

Windows 安装器复制到 `%LOCALAPPDATA%\Programs\Velella CLI\velella-cli.exe`，备份不同的旧版本，再加入用户 PATH。已有终端可使用绝对路径；新终端继承 PATH。安装 CLI 不替换桌面 app/runtime，不重启会话。管理命令是否被在线运行时支持，以在线只读检查和实际返回为准；编译验证不等同于真实模型业务验收。

扩展验证增加 `--extended`，在隔离目录中验证文件/Git、禁用的自动化、SSH 配置、技能/MCP 和远程 framing。远程测试使用临时 SSH shim 对接真实 remote-stdio 与第二个隔离 daemon，不能当作真实 SSH 网络认证验收。`scripts/verify-velella-cli-browser.cjs` 使用隐藏 Electron 窗口、真实 BrowserHost/Chromium/CDP、同用户 IPC 和回环测试网页；工作台布局由测试夹具提供，不替代实际桌面交互验收。
