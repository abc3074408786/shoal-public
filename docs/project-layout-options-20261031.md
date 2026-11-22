# 项目中间区域的两版布局预览

用户已选择方案一，并要求进一步精简界面及动效。当前 `layout=tabs` 显示精简后的第一种布局；原方案二保留在独立参数入口供追溯：

- 方案一：项目位于中间顶部，下面一行合并终端标签与新建/文件/更多三个常驻入口，其余工具收入更多。移除方案选择栏，统一中性选中态、圆角和留白。
- 方案二：中间按项目并排分栏，每个项目有自己的终端标签、工具与内容。窄窗口可横向滚动各栏。

入口：`http://127.0.0.1:8797/tests/project-shell.html?layout=tabs`，也可直接以 `layout=columns` 打开方案二。

已选定的单项目布局现位于正式 `desktop/src/ProjectWorkspace.tsx` / `project-workspace.css`，默认项目路由已接入真实 Workbench。历史分栏预览组件保留在 `desktop/tests/project-layout-options.tsx`，样式引用正式共享样式。桌面安装版未更新。真实终端验证说明见 `project-terminal-live-20261031.md`。

开发验证：正式前端构建通过；包含预览及其夹具的 TypeScript 检查通过；现有导航/工作区测试 17 项通过；改动文件 whitespace 检查通过。浏览器中验证项目切换、终端切换、并排两项目、快捷键仅修改当前栏、从分栏返回时保留刚操作的项目。控制台无警告/错误。

会话均由夹具预置：两个项目各两个模拟终端，启动请求数 0、真实 RPC 转发数 0。精简版夹具对工作记录返回空记录，不再显示人为的“记录暂不可用”。本次没有独立测试、独立技术验收、真实运行时/模型/SSH 或业务 UAT；已选中第一种设计方向，未替换桌面安装版。

精简版增量检查：项目与终端切换、文件工具、更多中的搜索及设置、Git 审查均通过；执行搜索或设置时自动收起更多面板。修正了 Radix 菜单内容脱离测量流导致二级菜单超出窗口的问题，修正后菜单位置和点击流程通过。控制台无警告/错误。构建、预览 TypeScript 检查及 17 项导航/工作区测试通过。

动效按用户指定的 Emil animations 规范收敛：高频项目/终端及按钮交互不做位移缩放；菜单 130ms 淡入、90ms 淡出，对话框 160ms 淡入；支持 reduced-motion。已读浏览器计算样式确认菜单/对话框采用上述淡入和时长，标签 transition 为 0s；没有做 reduced-motion 的浏览器仿真或设备 UAT。

用户截图明确需要收起整条项目切换栏。当前默认收起整栏，终端标签旁保留一个上下箭头按钮，可随时展开、选择项目、再次收起。收起时移除该栏的全部高度，浏览器实测终端增加 54px；原 Workbench 不随收起/展开重新挂载，同一终端的标签、会话与内容保持。状态通过 `velella:ui:project-bar-open` 偏好保存，夹具使用隔离内存，刷新重置。

此前单项目隐藏功能已撤回，其辅助模块和 5 项测试已移除。当前增量通过预览 TypeScript 检查和 whitespace 检查，浏览器验证默认收起、展开/收起、展开后切换项目；单纯切换栏显隐时布局读取次数保持 1，启动/停止请求均为 0。旧的单项目隐藏截图与结果仅供过程追溯，不代表当前行为。菜单中性色及越界修复保留。本增量未做独立测试、真实运行时测试或桌面安装更新。

证据：

- `artifacts/project-shell-20261031/layout-option-1-tabs.png`
- `artifacts/project-shell-20261031/layout-option-2-columns.png`
- `artifacts/project-shell-20261031/layout-options-validation.json`
- `artifacts/project-shell-20261031/project-layout-tsconfig.json`
- `artifacts/project-shell-20261031/project-tabs-refined.png`：当前精简版。
- `artifacts/project-shell-20261031/project-tabs-refined-validation.json`：精简版模拟会话及交互记录。
- `artifacts/project-shell-20261031/project-bar-collapsed.png`：当前整栏收起的界面。
- `artifacts/project-shell-20261031/project-bar-validation.json`：整栏展开/收起的内存夹具结果。
