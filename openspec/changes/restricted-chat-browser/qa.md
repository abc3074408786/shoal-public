# 浏览器受限聊天验收

2026-10-20，在内置浏览器中操作最新 WorkspaceApp，视口约 1055 × 926。使用独立临时 Git 项目、普通文件夹、runtime 和 Codex CLI 0.153.4。本次未登录、未调用真实模型，也未替换已安装的桌面应用。

## 真实前端和后台

- 复现用户截图中的入口问题：选择“待处理”时，没有待处理工作区的项目会从侧栏隐藏。切换“全部工作区”后，项目名称右侧的新建工作区按钮可见。
- 实际点击路径：“全部工作区” → Git 项目右侧“＋” → “新建工作树” → 创建后“Codex 聊天” → 执行模式“受限工具（实验性/Linux）”。
- 在浏览器内创建工作树与聊天，后台确认 session.execution_mode=restricted，工具网络策略为 denied，controller 私有目录位于工具工作区外，配置审计通过。
- 从聊天输入框提交一次消息并看到固定本地模拟回复。Responses 测试服务恰好收到一次请求，未向外部转发。
- 普通文件夹的受限模式不可选，并显示只支持新建 Git 工作树的原因。

## 完整 WorkspaceApp 的模拟错误状态

错误测试使用 `desktop/tests/workbench-chat-ui.html` 和对应 TSX，存储在内存中，RPC 全部由夹具接管。

- 首次聊天启动失败后工作树被保留；点击“继续打开”成功，创建工作树仅一次，打开聊天两次，重试使用相同 request_id 并保持 restricted 模式。
- 能力复查失败后仍保留 restricted 选择，同时禁止提交；未创建工作树或启动聊天。
- 环境能力不可用时受限选项禁用，显示明确原因；未创建工作树或启动聊天。
- 真实页面及错误夹具采集的浏览器 warning/error 日志均为空。

## 证据与范围

完整记录位于 `artifacts/browser-restricted-chat-20261020-v1/`：

- `browser-ui-result.json`：浏览器结果汇总。
- `browser-backend-evidence.json`、`result.json`：真实后台状态及固定回复计数。
- `launch-retry-ui.json`、`reprobe-ui.json`、`unavailable-ui.json`：模拟错误状态及请求断言。
- `01-inbox.png` 至 `08-unavailable.png`：入口、选择器、聊天回复及错误状态截图。
- `browser-console.json`、`cleanup-verification.json`：控制台与后台清理记录。

此次只验收单个桌面视口和上述流程；未进行移动端、完整无障碍、性能基准或真实模型验收。测试发现入口受侧栏过滤影响，未修改生产界面。夹具已通过 TypeScript 严格检查。

测试完成后关闭验收页面，停止 Vite，确认 1427 端口关闭。后台全部捕获的自有进程退出，临时项目和数据目录已删除，源码与二进制哈希保持不变。
