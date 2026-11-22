# Velella 0.2.5 圆角灰色滚动条

按用户提供的滚动条参考图，保持终端滚动条宽度 6 CSS 像素，滑块两端改为圆角；浅色主题使用 #dedede，深色主题使用中性 #666666，并保留悬停/拖动的灰色状态。只修改终端滑块样式和配色。

构建、NSIS 打包通过，121 个发行文件逐字节核对一致；正式窗口已观察到圆角灰色细条，截图位于 `artifacts/velella-scrollbar-rounded-20261031/installed-rounded-scrollbar.png`。此次可逆样式改动未新增测试，未重复终端逻辑测试，未做独立验收或拖动 UAT。

本机桌面和开始菜单入口已指向 `D:\orcas\artifacts\installed-ui-20261031-scrollbar-rounded\Velella.exe`。后台 PID 55264 未重启，42 个会话、42 份历史记录前缀及 90 个已登记终端相关进程身份验证保留。旧版本及本次数据库、快捷方式备份保留。

安装包为 `artifacts/velella-scrollbar-rounded-20261031/Velella Setup 0.2.5.exe`，122,410,369 字节，SHA256 `93ea0593de30876d87cc314bf370795bdbae6bb75ca71eec0f0ab5845d1d1703`。本机为并列目录部署，未另跑注册安装向导。
