# 按 Orca 改终端主体外观

用户明确选择：“按 Orca 改终端外观：灰色底、字体、间距和布局，保留 Claude/Codex 原生内容”。先前只处理外壳/工具栏不足以满足此次要求；本轮直接改变终端主体。

参考：用户提供的 Orca 终端截图。只读核对已安装 Orca resources/app.asar：out/renderer/assets/client-DNGCzCE6.js 的默认字体为 Windows Cascadia Mono、14px、字重500/700、行高1；默认 Ghostty Default Style Dark 为背景 #282c34、前景/光标 #ffffff、选区 #5a7898。Settings-YeGKrgWK.js 和 client-DroM3lag.css 的 X/Y padding 默认4px，滚动条14px，滑块 #79797966、hover #797979b3。默认值并不证明用户当前 Orca 的全部个人配置；截图灰底与该默认相符。

用户可见结果：当前近黑终端变为明确可辨的深灰面板，白色中等字重文本、紧凑四周4px留白与参考滚动条。保留已选工作台外壳、同一标签栏及按钮位置，不新增标题行。已有字体/字号/行高及光标偏好仍可使用；安装后通过设置界面确认当前字体字号与目标一致。浅色主题未获得新参考，沿用已有浅色画布。

不重写原生CLI输出，不改变ANSI指令/内容、会话、输入队列、IME、回放协议或后台。仅替换前端资源并保留生产数据。

验收：DOM真实画布颜色rgb(40,44,52)、4px padding、字体/字重/滚动条配置符合参考；复制粘贴/搜索/设置与DOM/WebGL光标回归；当前运行中的Claude原生窗口实际灰底与留白确认、会话/后台保留。业务与真实系统输入法UAT不由合成测试替代。

基线包 3c2af398b88b674edd011fc05f1da6a4f80cb257386dc74bc7994ce2615909e1；证据 artifacts/orca-terminal-appearance-20261025。

## 2026-11-15 紧凑布局延续

用户要求按 BMAD 方法让终端小巧一些，并指定参考本机 Orca；已同意仅调整当前脏工作区中的终端样式，保留其他修改。正文边距统一 4px，分屏标题栏收至 24px，终端标签栏收紧，减小外框留白与圆角；不改变原生内容、字体/行高偏好、会话、日志、布局结构和接口。仅交付源码候选与隔离预览，不更新安装版。

验收环境：真实 WorkspaceApp/TerminalView 浏览器夹具，内存存储、模拟 RPC、拒绝实际后台请求；明暗主题、1440/1100/900 宽度，标签切换、搜索、预置分屏展开/还原和键盘调宽。另执行前端构建、独立只读审查和截图检查。技术通过与安装、在线状态、真实 CLI/IME 和业务 UAT 分别记录。证据 `artifacts/terminal-compact-20261115`；BMAD 记录 `_bmad-output/initiative-velella-standards/plan-terminal-compact-orca.md`。
