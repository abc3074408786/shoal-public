# 设计与实现

AutomationsPage 为现有 WorkspaceApp 的独立视图。顶部搜索与过滤在左，
运行记录和新建在右；内容面板以居中模板列展示空状态。窄窗口允许工具行换行，
内容区独立滚动。保留 Velella 语义配色、字体和侧栏尺寸，不复制截图中的更新提示。

CapabilitiesPanel 的 AutomationPanel 复用为配置对话框，接受 initial、onSaved、
onCancel。工作区工具中的入口跳转到独立页面。新建配置默认暂停，确认复选框在
表单变更后清除。手动运行使用已保存配置并经过确认。运行记录打开任务前刷新
snapshot，以避免任务列表轮询延迟。Escape 关闭后焦点返回触发按钮。

构建只更新桌面资源，使用安装版的同一 runtime；候选版用独立数据目录验收。
不执行真实自动化或调用模型；行为验收以隔离 RPC fixture 覆盖。
