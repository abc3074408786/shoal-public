# Velella 0.2.6 短滚动滑块

用户可见结果：桌面终端滑块最长 80 CSS 像素，保持 6 像素宽、中性灰色和圆角。较长历史原本更短的比例滑块保持原长度。验收标准为滑块缩短且拖动仍可到达历史顶部、底部；关键路径已完成实现、开发验证、打包和本机桌面切换。

实现位于 `desktop/src/terminal-scrollbar.ts`，仅调整每个 xterm 实例的原生滚动条状态，同时处理拖动使用的 clone 快照。xterm 当前没有公开最大滑块长度设置；适配器检查私有字段，结构不匹配时退回原生行为，销毁时恢复方法。升级 xterm 后需重新核验此适配。

开发验证：`node --test tests/terminal-scrollbar.test.mjs` 三项通过，覆盖原生拖动快照、轨道点击、大小变化、短比例滑块和不支持结构回退。`npm run build` 和直接 electron-builder NSIS 打包通过，121 个 dist/Electron 文件与安装包逐字节核对一致。相关 diff 空白检查通过。

浏览器隔离 fixture 实测滑块 80×6 像素，432 像素轨道；真实鼠标拖动到顶部时 viewport=0，拖回底部时 viewport=base=28。证据位于 `artifacts/velella-scrollbar-short-20261031/slider-dom.json` 和 `drag-validation.json`。正式桌面窗口已观察到短灰色圆角滑块，截图为同目录 `installed-short-scrollbar.png`。未向正式终端发送输入。未做独立测试、独立技术验收或用户业务 UAT。

本机集成和部署完成：桌面及开始菜单快捷方式指向 `D:\orcas\artifacts\installed-ui-20261031-scrollbar-short\Velella.exe`，当前 GUI PID 66032。后台 PID 55264 未重启，42 个会话、42 份历史记录前缀及 90 个登记的终端相关进程身份核验保留。旧版本、数据库与快捷方式备份保留；本机采用并列目录部署，未另跑注册安装向导。

安装包：`artifacts/velella-scrollbar-short-20261031/Velella Setup 0.2.6.exe`，122,412,670 字节，SHA256 `6438a9b11b3d70bd471d27511c23337182df7170a3b3375e826182e1895fbb52`。运行时 SHA256 `8d7b64d01bc825466a3ff7385d875adfff55d1936088b624b03298638e275c44`，与运行中的后台一致。
