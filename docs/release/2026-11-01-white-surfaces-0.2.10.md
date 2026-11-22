# Velella 0.2.10：全部白色背景

用户现在看到的项目界面、终端背景、Codex 历史输入块和底部输入区均为白色。当前实际安装窗口已核对，不再保留原来的灰绿输入条；文字按白底计算对比度，语义彩色文字仍可显示。光标使用深色竖线，反色选中项使用下划线而不是深色填充。

## 改动

浅色 palette 的外层、终端、编辑器、标签和工作记录区域改为 `#ffffff`。xterm 原生 ANSI RGB/256 色背景不会随普通 theme 自动变化，因此新增显示层投影：WebGL 在颜色解析前投影 cell，DOM 在创建行时投影 scratch cell。背景统一为默认白底，反色和 dim 归一化后再计算白底对比度；选区/搜索装饰不重新引入深色填充。原始 buffer、终端录制、PTY 字节和会话输入不改写。切回深色仍可读取原配色。

渲染器重建、WebGL/DOM 切换和销毁时重新挂接或解除投影。浅色时拦截程序设置 OSC 11/111 背景，颜色查询继续沿 xterm 原路径处理。实现依赖当前固定 xterm 6.1.0-beta.303 / addon-webgl 0.20.0-beta.299 私有渲染字段；升级这两个依赖必须重新验证。

## 验证和安装

| 层级 | 结果 |
| --- | --- |
| 实现/开发验证 | 4 项专项测试通过：RGB/冒号 RGB/索引/反色、原 buffer 保留、深色恢复、渲染器替换及 OSC；TypeScript/Vite 构建通过 |
| 独立测试 | 独立只读静态审阅未发现阻断；未执行独立动态测试 |
| 技术验收/集成 | 真实浏览器 fixture 的 DOM 与 WebGL 背景检查均通过，WebGL 使用实际像素采样；buffer 原属性保留。截图检查去除所有测试深色条并保留红绿文字 |
| 安装 | 0.2.10 同一发行内容并列安装到 `D:\orcas\artifacts\installed-ui-20261101-white`，桌面和开始菜单快捷方式已更新；未运行 NSIS 注册安装向导 |
| 当前在线 | GUI PID 2292，daemon PID 29452 未重启。CUA 核对原知识蒸馏 Codex 历史：两个旧输入块、底部输入区及外层均为白底；未向正式 Agent 发送输入或重新启动它 |
| 数据 | 43 个会话 ID、43 份录制前缀、现有后台进程身份及两份原生 config/auth 文件 hash 核对一致；原桌面、数据库和快捷方式备份保留 |
| 业务 UAT | 本轮只验收显示行为，不代表真实模型执行或所有 TUI 交互验收；用户主观视觉反馈另计 |

包内 24 个 Electron 文件与 0.2.9 逐字节一致。CLI 和 runtime 继续使用冻结二进制，未编译或发布其他 Rust 改动。旧 Agent 已退出的状态保持原样；白色渲染不恢复或重建它。

## 固定产物和证据

- 安装包：`artifacts/velella-white-surfaces-20261101/release/Velella Setup 0.2.10.exe`。
- SHA256：`d721082634e3f04577c9a148440a0bca335f86694fccac75089ceb21767aac72`。
- runtime SHA256：`8d7b64d01bc825466a3ff7385d875adfff55d1936088b624b03298638e275c44`。
- CLI SHA256：`c6d35fb92e4746a2e3ff5511ed09d5b4ea2109f827c7093a4bcd82290c5cbd99`。

第一次归档解压的临时目录重命名被 Windows 拒绝；改用本机已安装的同版本 Electron distribution、独立的 `release` 输出目录后打包通过。失败 `package` 目录不是交付物，不复用。

证据目录 `artifacts/velella-white-surfaces-20261101/` 包含 `electron-audit.json`、`browser-render-verification.json`、`independent-review.md`、`before-install.json`、`installation.json`、`installed-verification.json` 及 manifest。测试入口为 `desktop/tests/terminal-white-surface.html`，正式安装不包含该测试入口。

回退仅恢复 `backup/shortcut-*.lnk` 并打开原 `installed-ui-20261101-terminal-switch/Velella.exe`。本轮没有数据 schema 或原始录制变更，不为界面回退覆盖数据库或历史。
