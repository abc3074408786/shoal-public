# 验收结果

- 前端 build 通过；沿用已有 bundle size 提示。
- npm test：91/91。
- automation-interface.browser.mjs：26/26，隔离模拟 RPC，未执行真实 Agent。
- workbench-interface.browser.mjs：35/35。
- 原生候选 native-candidate.png：1440×900，独立数据目录。
- 安装版 native-after.png 与 installed-state.json 确认独立自动化页面。
- deployment.json：候选/安装 binary hash 一致，工作区/会话及运行后台进程存活，runtime 未修改。
- 回退版本：/home/admin/.local/share/shoal/backups/automation-20261021-004012/velella-desktop。
- 已清理候选窗口和其独立 daemon，保留用户安装版与活跃会话。
- 未创建真实自动化。工作日/cron 未支持；模板为固定分钟间隔。

证据目录：artifacts/automation-interface/。视觉结果见根目录 design-qa.md。

## Electron 真实模型验收（2026-10-21）

使用已安装 Electron 配套的 Rust runtime、独立临时数据目录和 Git 项目，
通过原有 automation.save / scheduler / task 协议完成两次真实 Codex
（gpt-6-astra）定时单次执行。没有向主应用添加自动化。

- 第一轮：到期秒触发；测试者确认新建仓库的首次目录信任，并交还执行。
- 第二轮：在日程到期前重启后台，仍按时触发；原生会话人工输入计数为 0。
- 两轮均只修改 clamp.py；测试文件未变，4 项验证通过，自动进入 review。
- 两轮均没有提交、推送或部署；完整完成事件来自真实 Codex hooks。
- 真实检查：上一轮待审查时拒绝重复运行；暂停后越过到期时间不执行；
  暂停状态跨后台重启保留；单次执行后日程自动停用。
- Electron 原生界面显示真实运行记录，“打开任务”可进入对应待审查工作区。
- 临时日程已暂停、任务/后台/测试窗口已停止，新建仓库的临时信任项已移除。
- 生产应用的自动化与运行记录仍为空，原有终端未停止。
- 本次执行采用 Codex 原生 workspace-write 权限；不是自动化接入自研
  Restricted 工具沙箱的验收。真实模型未覆盖重复分钟间隔、休眠唤醒、
  Windows/macOS；补跑/持久化逻辑另有单元测试。

证据：artifacts/automation-real-20261021/result.json、change.diff、
second-change.diff、ui-runs.json、ui-task.json、scheduler-tests.log。
