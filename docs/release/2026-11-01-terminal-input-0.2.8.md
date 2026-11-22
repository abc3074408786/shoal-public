# Velella 0.2.8：原生终端输入兼容

用户现在可在原有 Codex 输入区继续使用中文输入、删词和换行。总体验收为：真实输入法落字一次且候选窗跟随光标，中文组合与修改回车有序提交，候选选择不会误触发旧换行；Windows 本机与 SSH/Unix 按各自键盘协议处理，既有会话和历史保留。关键路径已完成源码修复、开发回归、打包、本机桌面部署和正式窗口的未发送草稿验证。用户主观体验验收仍待反馈。

## 修改与依据

对照本机 Orca 参考源码 `D:\orca\src\renderer\src\lib\pane-manager\terminal-keyboard-protocol.ts` 和 `components\terminal-pane\terminal-shortcut-policy.ts`，新增 `desktop/src/terminal-keyboard.ts`。本机 Windows ConPTY 不再统一广告 Kitty CSI-u，SSH/Unix 保留协商；Shift+Enter 按协商选择 CSI-u 或 ESC CR，Ctrl+Enter 在未协商的本机 Windows 使用 CR；Ctrl+Backspace 使用 Ctrl+W 删词。仅在自有编辑动作上拦截，普通字符、光标键、复制和粘贴沿既有 xterm 链路处理。此处是本机源码参照，不声称已对所有 Orca 二进制版本做交互逐项对照。

`terminal-composition.ts` 保存组合输入期间的修改回车，在提交中文后按顺序发送一次。普通回车选词不追加提交；Escape、失焦、只读或销毁时不发送待处理动作；按键归属到释放并设短时上限，避免 Chromium 再派发相同 Enter 时重复提交。独立只读核验发现“先按修改回车、再用空格选词”可能遗留换行的边界，已加入显式与隐式组合清理及回归。原生 Codex CLI 和用户配置不改写，继续使用真实终端输入区。

## 开发与独立核验

在修改前，真实 TerminalView 隔离页面复现中文组合的 Shift/Ctrl+Enter 只发送中文、丢失后续动作；Ctrl+Backspace 发出 Backspace 而非删词；Windows 查询收到 Kitty 支持答复。修复后，同一页面得到预期字节顺序和协议行为。

39 项测试通过：组合事务、修改回车、防重、后续候选键清理、取消和失焦、Windows/Unix/SSH 协议、输入来源、队列、IME 锚点、Unicode 和本地/远端剪贴板。日志为 `artifacts/terminal-input-parity-20261101/tests.log`。独立探子逐项审查并重跑了当时 38 项测试，指出一处遗留请求缺陷；新增回归先红后绿，最终 39 项由主线程验证，未另做完整独立技术验收。

Windows Electron + 搜狗的独立测试窗口通过 CUA 操作，所有 RPC 都是隔离 fixture：候选窗在光标下方、空格落字一次、落字后的 Shift+Enter 产生一次 ESC CR、删词为 Ctrl+W、左右键正常、多行括号粘贴先于随后的 Enter。搜狗候选面板自身截获组合中的 Shift+Enter 时保持面板，其后空格选词没有意外换行。未绕过输入法自身的键位规则。证据为上述目录的 `before.json`、`after.json`、`native-ime.json` 和两份本机截图；测试窗口已关闭。

## 打包、集成与当前桌面

`npm run build`、NSIS 打包和相关 diff 空白检查通过。126 个 dist/Electron 文件与包内逐字节一致，4 个外部 Codex 更新保护资源哈希核对一致。后端继续使用冻结二进制 SHA256 `8d7b64d01bc825466a3ff7385d875adfff55d1936088b624b03298638e275c44`，未重编译其他未提交的 Rust 改动。

桌面和开始菜单指向 `D:\orcas\artifacts\installed-ui-20261101-terminal-input\Velella.exe`；GUI PID 24184，后台 PID 55264 未重启。部署时登记的 77 个终端相关进程、43 个会话和 43 份历史前缀核对保留。原版本、数据库和快捷方式备份保留。

正式窗口仍显示原 Codex 0.166.0 会话。通过本机搜狗在原本为空的底部 composer 输入 pinyin，观察候选窗跟随底部光标；空格确认一个“你”，Ctrl+Backspace 删除后恢复原空草稿。正式会话未按回车、未发送模型提示词。截图及回执位于 `artifacts/velella-terminal-input-20261101/installed-ime-candidate.png`、`installed-unsent-draft.png`、`installed-input-cleared.png` 和 `installed-input-verification.json`。

安装包 `artifacts/velella-terminal-input-20261101/Velella Setup 0.2.8.exe`，122,420,319 字节，SHA256 `ffa38e1852f53ea9bdafc5930c8f500f10b2d7d21764ec2a0bbae573378a8775`。本机使用同一发行产物并列部署，未另跑 NSIS 注册安装向导。完整原生 CLI 提交任务、其他输入法和远端真实设备的业务 UAT 未执行，不据此声称所有终端行为均已完全一致。
