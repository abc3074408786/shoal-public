# Velella 0.2.3 终端加载修复

当前桌面入口已更新到 `D:\orcas\artifacts\installed-ui-20261031-load-final\Velella.exe`。用户截图中的 Claude 欢迎框重复、边框和正文交叉，已用同一真实会话的 69,944 字节记录复现；最终桌面重新打开同一会话后，当前视图为一份完整欢迎框和原有对话。

原因与实现：

- Windows ConPTY 会在尺寸变化时输出已定位的完整重绘。普通 xterm 同时把旧宽画面重排进滚动历史，导致旧框被折成碎片，和新框同时出现。`terminalAppearance` 对本机 Windows 选择 xterm 固定网格兼容行为（`windowsPty` 的 18309 是行为选择值，不是实际 Windows 版本声明）。Linux、macOS 和 SSH 不启用此行为。
- TerminalView 初始回放期间保留布局尺寸但隐藏中间画面，显示静态恢复状态；完成后启用输入并显示稳定画面。历史协议回复仍被原有输入来源逻辑抑制；没有替用户发送模型任务。
- 恢复完成时，先显示画面，再在下一动画帧同步滚动条到底部，防止隐藏期间的 DOM 滚动状态把视图拉回旧标题行。此动作只在初始恢复完成时触发，后续用户上滚不会被持续拉回。
- 原始终端日志、geometry 记录和进程没有删除。固定网格下，旧宽画面不再自动折行；完整的旧尺寸画面继续由既有“查看历史画面”保存，原始记录仍可回放。

验证：

- 回归用例在修复前得到两份 Welcome back，修复后为一份；覆盖普通已有 Shell 历史保留及非 Windows/SSH 维持原行为。
- 尺寸、输入来源、历史回放、原始画面等 27 项测试通过；`npm run build` 通过。
- 同一真实记录离线对比：Welcome back 从 2 份降至 1 份；记录文件保持原样。
- 隔离界面阻塞首次输出时，`aria-busy=true`、canvas `visibility=hidden`；释放记录后为 busy=false、visible，offset=69944。滚动位置验证为 viewport=2、base=2。
- Windows NSIS 构建成功；121 个 dist/Electron 文件与打包归档逐字节一致。
- 最终正式窗口截图 `artifacts/terminal-load-fix-20261031/installed-final.png` 显示单一完整欢迎框及原对话。中间 0.2.2 版本暴露的滚动位置问题已在 0.2.3 修复。

部署采用并列目录，仅使已识别 GUI 的 relay 退出并触发正常 app.quit，保留 daemon PID 55264；新 GUI PID 37928。桌面和开始菜单两个原快捷方式已指向最终版本。42 个会话 ID、42 份原始记录前缀、90 个已登记的终端相关进程身份均验证保留。上一版本目录和本次数据库/快捷方式备份保留，未覆盖运行中的旧终端二进制。

安装包：`D:\orcas\artifacts\velella-terminal-load-final-20261031\Velella Setup 0.2.3.exe`，122,410,687 字节；SHA256 `ba4f7e5f66175cfe333055a4fe265a76636d37b028fe2a2ac3d20e5496d36161`。NSIS 向导未另做注册安装 UAT，本机验证的是同一打包产物的并列目录部署。

独立核验：只读探子确认了后端输出/geometry 记录的锁与页边界；未做最终源码的独立测试或正式技术验收。跨系统、SSH、模型业务任务及用户主观闪屏 UAT 未做，不以本次截图代表所有历史日志均可无损视觉重建。

证据目录：`artifacts/terminal-load-fix-20261031`（复现探针、私有原始记录、回放对比、加载/滚动验证、最终截图）；`artifacts/velella-terminal-load-final-20261031`（安装包、package-verification.json、私有部署回执与备份）。临时置于 tests 供隔离回放的私有 JSON 已移除，不进入发行包。
