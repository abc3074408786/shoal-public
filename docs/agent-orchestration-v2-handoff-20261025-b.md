# Agent 编排 v2 — 第二轮实施交接（2026-10-25）

整体未完成；本候选未安装、未发布、未经过五CLI真实端到端与混合协作验收。用户明确要求持续实施。此次是按用户全局 AGENTS.md 的上下文约40%交接规则转移唯一实现主代理，不是交付完成。

## 权威、基线和授权

先亲读 docs/agent-orchestration-v2-handoff-20261025.md 全文，那里是批准的完整总目标和原始边界；本文提供之后两轮增量事实。随后读 artifacts/orchestration-v2-20261025/continuation-b.json（当前21文件指纹与本轮测试回执），原始 baseline.json / baseline/ 仍是任务前的限定比较基线。原 handoff-files.json 已过时，不能当当前源码。

唯一代码工作目录 D:/orcas，分支 codex/integrate-collaboration-20261022，HEAD 仍为 62769b4a8ea0ec4c7f347c16726888cbaf3db64a。大量其他脏改动属于用户/其他任务；不要 reset/clean/stash/全仓格式化，不要把全仓diff当本任务。本轮没有提交、安装、重启生产进程、改全局配置或发PR。

用户授权实施完整 v2 与约定测试：自然语言可编辑角色/步骤/依赖/验收，Codex/Claude/Gemini/OpenCode/Kimi五CLI，逐角色模型/已有Skills/MCP工具，人工阶段gate，独立Git工作树并行、独立审查固定候选、集成与整体验证，交付集成分支与工作区。旧v1和当前A版外壳必须保留。外部原生授权保持人工处理。

子代理规则：只读探索/检索/核验；主代理改代码、选方案和最终测试。派生只传 task_name,message,fork_turns=none，禁止递归、一轮不复用。默认0或1探子；派出后立即 wait_agent，仍有相关探子时不自行推进工作。失败最多首次+一次基于新证据的重试。主线程默认 Terra，Standard；别宣称切换不可控服务档位。

## 继承上一轮的改动

- 工作树 __integrate 不再跳过依赖导入；fixture 从手工cherry-pick改为核对真实自动导入。
- providers::role_fingerprint 与 workbench_commands::ensure_matching_launch 校验角色配置指纹，绑定模型/技能/工具设置。
- OrchestrationV2 增加 WorkflowGraph。
- 旧 orchestration.browser.mjs 等待能力探测异步降级再找按钮，旧版20断言通过；该结果不证明v2。
- Rust库上一轮155 passed / 3 ignored，前端build通过。

## 本轮完成的实现与行为证据

### 结构化证据与验收语义

新文件 collaboration/orchestration/evidence.rs：
- Evidence version=1，含 candidate、checks、deliverables、artifacts、gaps；checks/deliverables 用 requirement_index 对冻结合同逐项覆盖。
- completed/passed 必须无缺口、每项实际passed、带method/detail，引用存在的artifact。
- artifact只允许分配工作树 .velella/evidence 下普通文件；拒绝路径逃逸/符号链接，限制数量/大小，校验SHA-256。
- 这是 Agent 提供的检查记录，运行时核对合同/候选/文件指纹，不声称运行时执行过其命令。独立审查仍是独立会话。
- collaboration.rs assign为v2 work固定 evidence_contract；report/review/accept解析验证并持久化证据来源。v1保持兼容。
- desktop/collaboration_commands.rs 在DB事务外核对证据文件；审查请求、通过审查和验收都会重查原报告；验收也重查独立审查证据。
- CLI report/review-report新增 --evidence JSON，MCP工具schema提供evidence及合同说明；所有v2角色（含只读）须先取得当前candidate。
- 最终开发验证/独立测试/技术验收显示 evidence_verified（证据已核对），不再统一标成运行时实测passed。
- 核对最终候选通过后才追加证据，按attempt_id去重；失败不刷新进度或重复增加记录。恢复时按节点顺序核对accepted，避免HashMap乱序。
- 独立验证候选还必须与集成候选的源码revision和Git HEAD一致（工作区路径可不同）。

### 依赖合入、冲突和恢复

desktop/orchestration/workspaces.rs：
- 每次自动导入先写 started+before+source，cherry-pick -x 后记录done/head。
- 中断在commit成功之后、done持久化之前，恢复通过单父提交、精确来源trailer与干净工作树识别已成功，不重复执行。
- 中断在执行之前且HEAD仍为before、工作树干净，可继续原操作；未知修改/别的cherry-pick/缺旧起点则保留现场。
- 校验依赖input_head是candidate commit祖先；目前显式拒绝任务增量中的merge commit，不能假装支持任意提交图。
- 使用 --keep-redundant-commits 保留不同SHA但相同补丁的空提交与来源回执（本机git帮助确有该参数，--allow-empty单独不足）。
- 最终集成发生冲突时，返回冲突工作树并启动集成Agent，给出有序 pending_imports。Agent解决当前CHERRY_PICK_HEAD后继续、再逐个合入剩余提交，并保留精确来源trailer。
- verify_integration 拒绝未完成cherry-pick/未解决冲突、离开baseline、遗漏任何来源提交回执。
- 非final依赖冲突仍保留现场并阻塞，解决当前cherry-pick后可继续核对；自动派集成角色处理非final冲突仍未实现。

### Skills、配置保留与私有目录

providers.rs：
- skill_digest从单SKILL.md改为整个技能目录资源manifest哈希（排除.git；拒绝链接、大小/数量越界）；脚本/参考资源变更会改变指纹。
- configure把技能目录复制到role/skills/skill-N，核对所选指纹；重放复用指纹匹配的完整快照，不覆盖篡改/不完整快照。
- role指令引用冻结快照路径，effective.json有skill_bindings和skill_catalog_enforcement。
- Kimi --skills-dir指向只包含选中快照的目录，包括空选择；其他四CLI目前只是instruction_only，不能称原生catalog白名单已完成。
- Gemini追加同事件hooks数组，保留原有handlers与context.fileName；不再覆盖自定义上下文配置。
- Kimi private home在复制凭据前设置owner-only ACL；Windows用固定PowerShell脚本+环境变量传路径，通过 .NET Directory.SetAccessControl 只改DACL。Unix0700。
- 准备完成的Kimi profile有velella-binding.json绑定角色指纹，重放复用；不完整/错角色profile保留并阻塞。
- Windows本轮测试先发现Set-Acl（含SetOwner）失败，改为只写DACL后真实目录读取确认owner-only。第二个失败出现在测试Get-Acl核验路径；独立只读读取证明ACL正确，再将测试改为.NET读取同一ACL，专测和全库均通过。没有复制真实用户凭据进行测试；两处首次失败留下空的私有目录测试产物。
- Kimi用户config相对路径重定位/OAuth刷新/项目MCP覆盖仍未验证。

### MCP stdio与UI

- mcp::transport将非法JSON返回-32700、非法request返回-32600，通知静默、输入1MiB上限、未知工具不执行；绑定credentials.member_id必须匹配session binding。
- OrchestrationV2交付页显示逐项method/detail/artifact指纹和独立审查记录；明确Agent提供记录与系统核对的区别。
- proposal批准/拒绝后重载已确认方案，清除该proposal对应dirty草稿；存在其他未提交修改会使用现有确认框提示。
- 新 desktop/tests/orchestration-v2.fixture.tsx、.html、.browser.mjs 使用真实WorkspaceApp/A外壳+隔离内存RPC。
- 19个浏览器断言覆盖五CLI选择、dirty确认禁用、显式启动、4节点图及边、人工gate、变更批准重载、暂停继续、1440/1100/900无页面溢出、证据显示、部署/UAT不误报、最终人工验收、模板存在、旧版往返。
- 主代理看过execution-light与delivery-dark截图，未发现页面溢出/外壳被替换。

## 当前已实际执行的测试

当前代码最后一次：
- cargo test -p velella --lib：166 passed，0 failed，3 ignored（169总数）。
- npm run build（desktop）：通过；仅既有大bundle warning。
- node tests/orchestration-v2.browser.mjs：19断言通过，0浏览器异常，0真实RPC。
- 具体结果/截图：artifacts/orchestration-v2-20261025/browser-evidence/result.json、roles-light.png、execution-light.png、execution-dark.png、delivery-dark.png。
- 本轮新增后端反例含：缺证据/漏项/not_run/failed/gaps/路径逃逸/错误hash/报告后文件漂移；无证据审查；集成候选漂移连续tick不增证据；真实合入后中断恢复；非祖先提交拒绝；同补丁不同SHA来源回执；双分支同一行冲突交给集成角色并验收；Skills资源漂移/快照重放/篡改；Gemini hook合并；private ACL；MCP畸形请求。
- scoped git diff --check 使用 core.whitespace=cr-at-eol 通过（只看本任务所改tracked文件，没改全仓既有CRLF）。
- 最新debug runtime二进制尚未重新build，native-2仍是旧二进制；不能把最新库测试当最新原生运行。
- 未做完整独立技术验收：本轮只读探子审查找到3项（skill重放、证据重复、redundant commit），主代理已修复并补对应测试；不是独立测试实测。

## 原生阻塞的已核实结论与新环境变化

本会话没有重跑 native-3。

只读分析 native-2：
- Codex terminal.bin：Trust this folder? Trust and continue / Quit。
- Claude：Is this a project you created or one you trust? Yes / No；另有 global:deepseek-v4.1-flash auto-compact配置兼容警告，但非已证实启动根因。
- Gemini：Do you trust the files in this folder? Skipping project agents due to untrusted folder。
- Kimi：Trust this folder?；Project-level MCP servers are disabled until you explicitly choose Trust。
- OpenCode：已到Ask anything主界面，随后Update Available v1.19.4模态（Skip/Confirm）；未见信任/登录提示，hooks根因仍未知。
- 五个status.json均stopped、native_id=null、hooks_ready=false、turns=0、exit=1。无模型规划/执行/交付通过。

本轮另一探子只读版本/帮助：
- Codex0.164.0、Claude2.1.238、OpenCode1.18.35、Kimi0.36.1。
- Gemini当前启动器报 Missing target command: ...gemini.cmd，当前安装目录缺package.json；旁边残留bundle 0.47.0不能当当前可运行版本。
- 此Gemini新根因已发生 --version/--help 两次失败，不得原样第三次；需要只读核对安装状态/已有修复授权后再决定。没有用户授权全局安装/账户/权限改变。
- Claude --strict-mcp-config 确有“Only use MCP servers from --mcp-config, ignoring all other MCP configurations”；--disable-slash-commands禁用全部Skills，不能被当单Skill白名单。
- Kimi --skills-dir支持重复且替代auto-discovered user/project dirs。
- Gemini context.fileName绝对路径不能可靠定位instructions（原生内部path.join(home/.gemini, filename)）；当前已保留原context，角色指令仍经通知和VELELLA_ROLE_FILE提供。
- 其他工具/MCP细粒度schema仍未知，不能臆造。

## 剩余关键路径（按用户批准范围继续，不退回需求问答）

1. 能力完整性：已有MCP服务器/逐工具选择与实际角色绑定，模型支持/版本预检及持久回执；默认Terra/Luna与Standard约束；四CLI原生Skills catalog限制。不要仅添加UI开关而不验证生效；本轮只关闭目录资源冻结。
2. 治理完整性：跨节点/版本同根因失败ledger；凭可复现缺陷或明确需求受控重开已验收候选；系统集成/验证阶段人工gate配置（目前系统节点human_gate固定false）；上下文约40%交接机制。
3. 恢复/依赖：非final输入冲突的独立集成处理；baseline创建中断恢复；Kimi不完整profile恢复与相对路径；接受/停止后所有自有会话死亡证据。支持merge提交或明确可审阅的集成方案，不能默默丢依赖。
4. 五CLI真实启动/领取/回报/独立审查/恢复及至少两CLI、两实现角色的混合协作。先解决安装/信任/升级模态前提；不要伪造hooks、自动bypass信任或原样重跑脚本。代码工作可继续独立推进。
5. 最终限定diff独立技术验收、集成分支/工作区交付。没有五CLI+混合实际通过不得称整体完成。安装/发布不在当前默认授权内，部署/在线/业务UAT分别报告。

不要把本文所列明确未实现能力变成无边界审计，也不要反复测试已通过子路径。下一轮应先收敛能力配置的具体可实施合同（MCP绑定/预检），实现并验证；并继续上述必要功能，只有剩下真实外部授权/安装前提时再提出精确决定。原主会话在交接后停止修改本目录。
