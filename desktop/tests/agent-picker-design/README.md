# Agent 下拉框三版设计预览

从 `D:\orcas\desktop` 运行：

```powershell
node node_modules/vite/bin/vite.js --config tests/agent-picker-design/vite.config.ts
```

打开 http://127.0.0.1:8816/tests/agent-picker-design/index.html 。顶部可切换三版对比和完整设置窗口；三个独立入口为 compact.html、grid.html、detail.html。

本预览没有 runtime bridge，不保存偏好、不创建终端。保留现有 AgentLogo、字体与灰白配色。正式默认 Agent 控件尚未替换，待用户选择设计。

开发验证：单独 TypeScript 检查通过；浏览器验证第一版点击 Claude Code、第二版点击 ZCode、第三版方向键进入/Enter 选择自动模式/Escape 收起；外部点击收起。1440px 三列桌面和 480px 窄窗口检查未出现横向溢出，九个选项完整。首次热更新出现重复 createRoot 提示，已改为复用预览挂载点。视觉接受由用户决定，本次没有独立测试或部署。

截图：`D:\orcas\artifacts\agent-picker-design-20261101\comparison.png`。
