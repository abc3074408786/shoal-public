# v2 续接 D：整体未完成

接手者亲读本文件和 handoff-20261025.md、-b.md、-c.md。工作区 D:/orcas，HEAD 62769b4a8ea0ec4c7f347c16726888cbaf3db64a，分支 codex/integrate-collaboration-20261022，脏改动须保留。旧主会话交接后停止修改。研发已授权；安装、发布、PR、全局账户改变、自动 trust/login 未授权。

本会话新增/修改：collaboration/orchestration/v2.rs 的 McpServer 契约；providers.rs 的外部 MCP 配置、角色默认模型；mcp.rs external_serve 代理；lib.rs external-mcp 入口；OrchestrationV2.tsx MCP 表单、预检和死亡回执；desktop/orchestration/v2.rs shutdown；v2_tests.rs 新回归。多数文件原为未跟踪，git diff 无法代表完整任务差异。

## 最新可靠测试

cargo test -p velella --lib -q 已取得退出码 0：197 passed / 0 failed / 3 ignored，200 total。最近新增测试真实使用 fixture IPC 和 owner.lock：exit 状态但锁仍持有时拒绝死亡回执；释放锁后恢复持久回执；缺注册阻断；缺启动结果以 request_id 找已注册会话。不是五CLI模型验收。最新前端 build 在上轮通过，本轮未改前端。runtime 曾构建通过但本轮恢复修复后未重建。

此前助手说“191通过”的全库工具输出没有终态，不能引用；最新 197 通过取代该说法。此前独立审查都是静态源码阅读，不是独立测试执行。限定 git diff --check 通过但不覆盖未跟踪文件。

## 已有实现与真实缺口

外部 MCP 目前是角色表单手工声明 command/args/catalog/tools，并非已核验可信注册源。五 provider 写会话配置、代理过滤发现与调用。静态审查修复通知等待死锁、Kimi工具白名单遗漏、Windows attach失败直接子进程清理。不能声称全部隔离或端到端完成：CLI 配置合并优先级未验证；browser_agent::inject 在 configure 后另注入控制浏览器；proxy 尚为同步逐请求读一行，无完整双向通知/请求ID关联、超时、输入大小边界；descriptor信任/不可变性和直接terminal.open绕过计划校验应核对。前端“现有MCP不会继承”等文案证据不足，需收紧。此前窄审查通过不冻结这些未验证能力。

Codex 空 model 由 effective_model 按kind派生：coordinate/implement/integrate 为 gpt-6-terra，其余为 gpt-6-luna；该具体ID的本机可用性未验证，不能当官方支持列表。显式model优先。preflight记录source和requested_unverified。features.fast_mode=false已有；execution_mode的enforced措辞仅依据写配置，实际服务模式未验证。role_fingerprint现在包装model_policy，须关注旧会话重放兼容性。

shutdown 从本run start/launch effects收集 workspace/session，缺result时使用原request_id（workbench launch明确以request_id为session ID）。缺注册阻断。stop/accept调用共用shutdown后才结束；锁确认通过写session_shutdown[workspace][session]。effect key改为JSON二元组避免分隔碰撞。最新生命周期fixture验证通过。但收集缺workspace或缺所有ID仍跳过，失败一项会提前返回而非继续停止其它自有会话；启动inflight与锁尚未创建的竞态、未知启动结果无注册时如何安全恢复仍需收口。已有成功stop effect重放不重新kill，应结合恢复语义核验。现有大多数v2 fixture手工open仅写result、无request，故全流程fixture通过不证明该shutdown真正遍历全部会话。

非最终integrate冲突接管和baseline安全恢复是-c已关闭切片，别再作为待实现缺口。当前会话错误重查了一次非最终冲突fixture，通过后未改动。

## 下一步

优先收口上述真实安全/协议缺口与端到端fixture，避免再做纯展示微调。之后仍有四CLI原生Skills约束、Kimi相对配置/OAuth恢复、产品40%上下文来源、五真实CLI与双CLI双实现混合集成、完整独立验收。native-3/4不得原样重跑；外部信任/登录保持人工处理。整体未完成、未安装、未部署、在线与业务UAT未验证。

遵循用户AGENTS：主代理修改/最终验证；探子只读单轮，fork_turns=none，不传其它可选角色/模型参数，派出即等待；同因首次加一次基于新证据重试；默认Terra/Standard但不可虚称切换当前模型。当前没有在途测试或探子。
