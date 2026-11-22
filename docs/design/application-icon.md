# 桌面应用图标 · 2026-11-01

Windows 应用搜索、快捷方式、可执行文件、窗口和托盘统一使用界面内已确认的三鱼标识 `desktop/src/assets/velella-logo-v2.png`。不再使用旧彩色圆环鱼群图标。

`desktop/scripts/generate-app-icons.py` 从原图导出桌面资源：仅裁切几乎透明的留白并缩放，保留原图形与颜色；标识占方形画布82%。Windows ICO包含16/24/32/48/64/128/256px，另生成现有Windows PNG尺寸、macOS ICNS和Linux/Electron PNG。生成需要Pillow；已导出的图标直接随现有打包流程使用，不增加应用运行依赖。

Electron窗口与托盘读取 `desktop/electron/icon.png`，electron-builder使用 `desktop/src-tauri/icons/icon.ico`。当前便携安装的桌面和开始菜单快捷方式显式指向安装目录 `resources/velella-school-v2.ico,0`，避免继续读取历史安装目录的图标。快捷方式目标、参数、工作目录保持原值。
