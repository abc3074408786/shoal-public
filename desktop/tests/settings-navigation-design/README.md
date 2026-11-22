# 设置导航三版样本

打开 http://127.0.0.1:8816/tests/settings-navigation-design/index.html 。使用已有仅提供静态/React 文件的隔离 Vite 预览服务；启动方式见 tests/agent-picker-design/vite.config.ts。

A 轻量列表、B 说明导航、C 窄图标栏。比较页可独立点击每版 Agent/权限，也可用右上角状态切换统一比较。顶部 A/B/C 或卡片「查看完整界面」可展开为带右侧设置内容的上下文，左侧返回回到样本对比。设置内容为复用真实目录、Logo、02 双列控件及权限选项的设计演示；交互只改 React 内存，不访问后端、不保存本地/原生偏好。

开发验证：本 fixture 单独 TypeScript 检查通过；浏览器检查三版 Agent 与权限选中态、B 完整权限页面，以及 Enter 切换到 Agent 后标题和选中项同步；C 在 480×820 下切换权限，页面 scrollWidth 470，小于视口 480，无横向溢出；控制台 error 为空。正式 AppSettings 导航没有替换，等待用户选定设计。本轮没有部署或独立测试。

截图：D:/orcas/artifacts/settings-navigation-design-20261101/agent-comparison.png 与 permission-comparison.png。
