# Velella 0.2.7：Windows Codex 更新文件占用修复

用户可见结果：从新启动的 Codex 中执行内置 npm 更新时，独立全局 `@openai/codex` 安装先进入同盘临时目录，验证实际 CLI 版本后再切换；其他运行中的 Codex 会话继续运行，安装或切换验证失败时恢复旧包。总体验收是运行中的 Windows 程序仍存在时完成切换，保留旧会话，并让后续升级继续经过同一保护入口。实现、开发验证、发行打包与本机部署完成。用户随后授权“你来”，已通过启动已运行应用恢复最小化窗口，并点击已结束标签的“重新启动终端”。原生 Codex 0.166.0 已显示输入提示；新进程 PATH 包含更新保护入口，未发送模型提示词。

## 实现

- `desktop/electron/codex-updater/codex-npm.cjs` 仅处理独立全局 Codex 安装（`npm install/i -g/--global @openai/codex`，可带简单版本或 tag）。普通 npm 命令委派到原始 npm CLI，保留参数、工作目录、标准输入输出和退出状态。多个包、额外安装选项和自定义 prefix 继续走原 npm。
- 同盘并列安装、实际 `--version` 验证、唯一更新锁、切换 journal 和失败回退。旧目录保留在 npm prefix 的 `.velella-codex-updates/previous-*`，不删除仍可能被会话使用的文件。已安装目标版本时直接验证后成功返回。
- 桌面启动将 npm 生成的 Codex 三种 shim 的入口路径改到 prefix 下的 `.velella-codex-tools/codex-launch.cjs`，原 shim 首次备份保留。启动器在同一 Node 进程前置更新保护目录后载入官方 Codex JS；官方 CLI 文件、用户配置、登录文件及 shell 的父进程环境不改写。保护适用于原生 Codex 标签及 PowerShell 内新启动的 npm Codex。升级成功后新 shim 继续保留保护入口。已经运行且未载入保护的旧进程需重新启动一次才会获得该环境。
- `codex-update-tools.cjs` 和打包配置将脚本放在外部 runtime 目录。独立只读审查发现的 asar 路径问题已修复：外部 Node 执行实际解包的脚本，不访问 `app.asar` 内路径。Node/npm 缺失或自定义 shim 无法识别时不阻止整个桌面启动，输出具体 setup 错误，不覆盖未知文件。

## 验证与边界

`node --test tests/codex-update.test.mjs` 12 项通过：实际 Windows 活跃 exe 覆盖产生 EBUSY/占用错误，并列切换后该进程仍存活；覆盖安装失败、阶段版本异常、切换后版本验证失败回退、并发锁、中断后目录恢复、启动器保护、CMD/PowerShell 参数传递、资源部署和外部 Node bootstrap。另 23 项 Electron 安全、runtime/SSH 和开发桥回归通过，`npm run build`、NSIS 打包及相关 diff 空白检查通过。

独立只读源码核验指出 asar 调用缺陷；修复后有开发测试和真实打包部署证据，未再做最终源码的独立运行验收。真实机器使用已安装的保护入口执行原更新命令成功退出，返回当前最新 0.166.0 已安装；真实全局入口和新的 PowerShell 均返回 `codex-cli 0.166.0`。原先更新失败的 GUI 标签已重新启动，host PID 27456、Codex PID 80272，进程运行且更新保护目录在其 PATH 中。截图及回执为 `codex-restarted-0.166.0.png`、`gui-restart-verification.json`。实际版本变化与文件占用切换在 Windows 隔离 fixture 验证，未为了测试而降级生产 Codex，未发送模型业务提示词。用户业务 UAT、完整内置更新菜单交互及未来版本的上游兼容性仍未验证。

## 本机部署

桌面与开始菜单快捷方式指向 `D:\orcas\artifacts\installed-ui-20261031-codex-update-guard\Velella.exe`，GUI PID 63596。后台 PID 55264 未重启；43 个会话、43 份历史前缀、90 个登记的终端相关进程身份核对保留。额外核对 11 个原有 Codex 相关进程身份保留；`config.toml`、`auth.json` 哈希与部署前一致。截图保留于发行证据目录 `installed-0.2.7.png`。

源码中其他未提交的 Rust 改动未重新编译或发布。后端继续使用冻结二进制 SHA256 `8d7b64d01bc825466a3ff7385d875adfff55d1936088b624b03298638e275c44`。126 个 dist/Electron 文件逐字节核对一致，4 个外部更新资源另行核对哈希。

安装包：`artifacts/velella-codex-update-guard-20261031/Velella Setup 0.2.7.exe`，122,421,414 字节，SHA256 `36f6260eb1767760a12f984c1f4977e974ade329484f1b235fe575fc4a73fab4`。本机采用并列目录部署，未另跑 NSIS 注册安装向导。证据包括同目录 `package-verification.json`、`installed-update-command.log`、`installed-guard-verification.json` 及私有部署备份。
