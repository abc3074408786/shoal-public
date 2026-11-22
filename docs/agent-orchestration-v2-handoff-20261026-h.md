# 编排 v2 续接 H：用户明确四 CLI，OMP 替代 Kimi/Gemini

整体未完成、未安装。源主会话交接后停止源码修改。接任者先亲读本文和 G（`docs/agent-orchestration-v2-handoff-20261026-g.md`）；G 的范围被下述用户决定覆盖，其余冻结边界与证据索引继续有效。

## 当前用户合同与授权

本会话用户明确说“kimi不用管，目前就只需要omp，claude，codex，opencode”，并确认 OMP 是 Oh My Pi，再说“可以”。所以必须交付 Oh My Pi、Claude Code、Codex、OpenCode 四种 CLI。Kimi/Gemini 不再是本次实现或验收前置条件，保留既有代码/记录，不删除用户数据。不要继续拿它们阻挡交付。

父目标：自然语言 Git 研发目标 → 可编辑角色/步骤/依赖/验收 → 人工确认 → 独立工作树并行 → 独立审查固定候选 → 集成 → 独立总体验证 → 可运行可审阅分支/工作区。四 CLI 各走完启动、领取、回报、审查、恢复；至少两种 CLI 两个实现角色混合并行集成；之后按已有授权受控安装。授权出处及生产保护见 G。禁止把实验/单测替代总体验收；不得只回复小增量完成后停下等待用户继续。

用户明确要求主会话约40%主动交接到新会话；本次为该规则下续接。新会话唯一修改与最终验证；探子只读、单轮 fork_turns=none、不得递归，派出立即等待。大量不相关 dirty/untracked/deleted 保留；HEAD仍为62769b4a8ea0ec4c7f347c16726888cbaf3db64a，D:/orcas，codex/integrate-collaboration-20261022。

## H 前两轮实际源码增量（尚未冻结）

文件 `crates/velella/src/collaboration/providers.rs`、`providers/skill_policy.rs`；新增 `scripts/fixtures/opencode-native-isolation.cjs`。

- OpenCode 新角色使用 role/opencode.json，私有 role/opencode-home/{config,data,cache} XDG 根；OPENCODE_CONFIG_CONTENT显式设为{}覆盖宿主继承。设置 DISABLE_PROJECT_CONFIG、DISABLE_EXTERNAL_SKILLS、PURE、DISABLE_DEFAULT_PLUGINS、DISABLE_AUTOUPDATE=1。完整选定Skill正文注入并绑定资源/正文digest；permission.skill={"*":"deny"}；恢复核对上述开关、配置路径、三XDG路径和inline空对象。role/opencode.json进入文件摘要列表。
- `located("opencode")` 特殊解析 where.exe 返回 shim 同前缀的 node_modules/opencode-ai/bin/opencode.exe，要求普通非symlink；失败返回空，configure使用该exe。此限制仅支持npm安装，原生独立exe和非Windows支持应审查，不能误称普适能力。
- 新增 launcher合成测试、skill_policy恢复测试、真实configure生成角色产物并verify_mcp_bindings的测试。
- 本轮用户确认范围后尚未改代码：PROVIDERS和UI仍是旧5种，OMP尚未接入。需要调整新建范围，考虑旧计划兼容，不简单破坏历史读取。

## 必须纠正的证据/实现缺口

1. **OpenCode PURE很可能禁掉自身lifecycle插件**。G引用同版本官方 plugin/index.ts:170-187明确PURE跳过外部插件，当前代码仍生成lifecycle.mjs却设置PURE。前轮审查仅说未确定，未解决！不要冻结，要亲读官方同版本源码并验证真正回调。私有XDG无法证明无managed/home .opencode，缺OPENCODE_TEST_HOME等绑定；空私有data也丢掉现有认证，不可默认为已能认证工作流。
2. permissions初始skill deny后，role.tools循环可能把skill重新改为ask；task也应核对，需行为测试。
3. fixture仅合成配置，不消费Rust真实configure产物。输出已降级为“能启动解析且不列出注入external skill”，无MCP/lifecycle/permission通过结论，不能放大。
4. **完整Rust回归未保留最终退出码/汇总**：前模型只text(r.output)，exec_command 30秒返回session_id时被丢掉；两次返回只到测试列表/点号，随后误称215passed/4ignored。当前只确认provider定向24passed/1ignored（后来新增角色产物测试明确1passed），不能沿用215通过。先只读查有无残留本会话cargo/test，不杀未知进程；下一次测试必须保留整个返回含session_id并用write_stdin等到exit，日志输出可重定向供证据。
5. 前轮两次launcher fixture失败都是启动前：Node找不到shim，再PATH找不到exe；后明确where+同prefix来源后通过。不要无变化重跑。独立静态复核两轮发现问题，修后主测通过不等于独立测试/整体独立验收。

## OMP本轮直接证据与定向来源

本机 `C:/Users/developer/AppData/Local/omp/omp.exe`，`--help` exit0版本18.12.3。没有启动模型、没有读取账户凭据。支持：

- --append-system-prompt=<text or file>，--config=<overlay>可重复；--no-skills关闭加载；--no-extensions关闭发现但显式-e仍可用；--no-rules；--tools逗号列表（应排除task）；--service-tier=none；--profile隔离auth/sessions/settings/cache；--mode rpc；--no-title；--approval-mode（不可默认yolo）。
- PI_CODING_AGENT_DIR改变default profile agent目录；命名profile忽略该变量。PI_CONFIG_DIR是另一泛型根，不能混用；用户认证如何安全继承未核实，不能自动login/export secrets。

已成功通过Invoke-WebRequest直接读取官方精确tag：`https://raw.githubusercontent.com/can1357/oh-my-pi/v18.12.3/`，可沿以下路径继续，无需广泛搜：

- `docs/mcp-config.md`（全文已读）：OMP原生user mcp.json + project .omp/mcp.json，以及Claude/Codex/Gemini/OpenCode/Cursor/Windsurf/VSCode、插件、root fallback。profile只隔离原生user，不隔离foreign/project；enabledServers只是强制启用名单不是封闭白名单。`mcp.enableProjectConfig=false`在去重前排除所有project MCP来源。disabledServers deny优先。
- `packages/coding-agent/src/mcp/settings.ts`（全文已读）：mcp.enableProjectConfig默认true，startupTimeoutMs默认250，其它无白名单设置。
- `packages/coding-agent/src/mcp/config.ts`（全文已读）：loadAllMCPConfigs读取getMCPConfigPath(user)，loadCapability(mcp) filter按source.level排project，disabledServers屏蔽、enabledServers强制enabled，之后Exa/browser特殊过滤。没有only-selected参数。
- `docs/config-usage.md`（本轮只读相关段落）：配置CLI overrides > runtime > PI_CONFIG_FILES及--config overlay > project > global > default。overlay可控制发现来源。原生agent dir还用于sessions/blobs/agent.db。必须亲读需要的基础部分。
- `packages/coding-agent/src/capability/index.ts`（本轮只读相关段落）：cfgDisabledProviders/cfgEnabledProviders来自`../config/model-settings`；filterProviders先剔除disabled provider。enabledProviders只是foreign user opt-in，非白名单。setDisabledProviders持久写bound settings，不能用它修改真实配置；应使用进程overlay。foreign user列表cursor/codex/claude/claude-plugins/gemini/opencode/windsurf/github；native/.agents默认读。
- `packages/coding-agent/src/discovery/index.ts`导出上述函数并导入注册模块；`src/config.ts`泛型目录查找isUserSourceEnabled也尊重disabled；project不受该helper过滤。
- `docs/extensions.md`（只读相关段落）：default factory(pi)，pi.on(session_start/before_agent_start/agent_end/session_shutdown)，pi.exec，getActiveTools/getAllTools/setActiveTools，ctx.sessionManager。before_agent_start每任务准备上下文、可能重入。event异常有被吞风险需要真实确认。没有看到registerMcpServer API，勿猜。读取`extensibility/extensions/loader.ts`和types确定回调、ID、错误语义。
- 404一次：`packages/coding-agent/src/discovery/settings.ts`不存在，不重试此路径，正确设置入口是config/model-settings.ts。

可行研究方向（尚未实现，不能当结论）：overlay disabledProviders关所有foreign/plugin/root-fallback，仅native + mcp.enableProjectConfig=false；私有native agentdir只装选定MCP，--no-skills +完整正文，显式extension生命周期。认证需要方案而不是静默丢失；native插件发现、overlay加载顺序需原生验证。另一方向受控extension桥接MCP，但不能凭猜测实现。

## 下一动作与停止边界

先解决实际OpenCode生命周期回归与OMP配置/认证接入，四种provider发布范围同步。Codex Skills仍缺原生有效目录证明；可信MCP目录来源/真实40%usage及混合工作流仍见G。可以授权内持续研发；只有必须用户trust/login时准备具体隔离会话入口及动作一次说明，不能反复重复等授权，也不能自动接受。不要重跑native-3/4。

本次没有新增生产能力，没有安装、没有PR。生产会话未停止。G已冻结Claude/Gemini切片以及其它runtime逻辑保留。源主会话交接后不再修改，后续全部归接任者。
