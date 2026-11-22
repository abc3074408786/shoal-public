# Agent 页面三版样本

打开 http://127.0.0.1:8816/tests/agent-page-design/index.html 。沿用 tests/agent-picker-design/vite.config.ts 的 8816 隔离预览服务。

左侧导航已按用户选定的 A 接入正式源码 app-settings.css，并在完整样本中固定为 A。Agent 内容仍为三个待选方案：01 紧凑列表、02 双列卡片、03 工具与详情；可进入完整设置上下文。保留已确认的 02 默认 Agent 双列菜单。所有样本交互仅更新 React 内存，不读写用户偏好，不访问真实检测、终端或后台。权限页仅展示原有权限目录。

验证：预览 fixture 单独 TypeScript 检查通过；A 导航源码变更的 npm run build 通过，日志在 D:/orcas/artifacts/settings-navigation-design-20261101/approved-a-build.log。浏览器验证 02 设置 Kimi 默认，再禁用 Kimi 回退自动；03 点击 Claude Code 只变详情，默认仍为 Codex，随后 Enter 设为默认后变 Claude Code；没有混淆工具选择和默认选择。1440px 桌面对比无横向溢出、Logo 全部加载；480×820 完整 03 侧栏为 62px、scrollWidth 470 小于视口 480；控制台 error 为空。

截图：D:/orcas/artifacts/agent-page-design-20261101/comparison.png。

本轮源码只接入 A 导航。正式 Agent 内容没有替换，等待用户选择。未更新安装、未操作真实会话，无独立测试或原生安装后验收结论。
