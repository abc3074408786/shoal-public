# Velella 0.2.15-reopen.3 已安装

用户授权安装后，正式入口与系统 CLI 已切换到包含全部 8 Agent 打开即恢复功能的集成版本，保留现有取消并发上限等线上能力。

- 正式安装：`D:/Velella/artifacts/terminal-reopen-all-20261114/integration-rc2/installation-0.2.15-reopen.3/desktop`。
- 正式数据：`C:/Users/developer/AppData/Local/Velella`。
- 固定源码基线：`D:/Velella/artifacts/terminal-reopen-all-20261114/integration-rc2/source`。这是以原线上 rc.2 独立快照合入恢复功能的源码；主仓库根工作树仍有大量历史工作，未同步全部线上差异，不得直接重打覆盖。
- 安装、回退、验证与业务验收边界：`D:/Velella/artifacts/terminal-reopen-all-20261114/INSTALLATION.md`。
- 备份：`C:/Users/developer/AppData/Local/Velella-backups/reopen-20261115-000708`；原 rc.2 安装保留。

恢复、构建、受控包内技术路径及安装在线检查通过；真实 ZCode 原对话已显示，但原生 CLI 提示保存模型 provider 不可用，真实业务可继续对话验收尚未完成。本轮未修改账号或模型配置。

后续免费入口修复：已将 zcode-free 升至匹配的 CLI 3.18.2-47，并修正其 Windows 系统密钥读取；保留免费模型、共享账号和原数据库。真实免费 GLM 请求已返回 OK，原会话已恢复且旧提示消失。最终证据见 `D:/Velella/artifacts/terminal-reopen-all-20261114/free-account-diagnosis-20261115/RESULT.md`。此修正位于独立 CLI 目录，后续覆盖安装须保留其 Windows 密钥互操作行为。
