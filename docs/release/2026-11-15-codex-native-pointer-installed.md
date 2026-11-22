# Codex 0.169.0 原生入口已安装

2026-11-15 14:55（北京时间）完成。此记录接续当天的导航与JS更新器修复；此前power.1路径保留作回退，当前正式桌面入口为下列native-pointer.1侧装目录。

- 安装：D:/Velella/artifacts/codex-native-pointer-20261115/installation-0.2.15-native-pointer.1/desktop。
- 固定源码：D:/Velella/artifacts/codex-native-pointer-20261115/source，以导航部署source叠加已接受JS更新器，再仅补充Rust原生解析器。根工作树较旧且有既有脏工作，不能直接重建覆盖本安装。
- runtime SHA256：4e4f5039def087b1ea58e0c2b870831be2a9954d5efa3956c8d612a84c62eff0。
- CLI SHA256：dfcfc4cb66116e5608d6f3e94f95e7940ed7295e46bfd2b449b4cc6b026d5acd。
- ASAR SHA256：e9e8aff68f84e271ba9539209b84379944f60f63307d2ead652d080a6214f657，和本次补充安装前相同。
- 备份：C:/Users/developer/AppData/Local/Velella-backups/cli-power-20261115-145459；包含准备时备份和停止旧GUI/daemon后的最新quiescent元数据。旧安装、旧npm包和全部版本目录保留。

实现及开发验证通过，独立审查为只读，最后安装脚本修正由主代理核验；7项原生解析检查、release --locked、隔离真实ConPTY和新旧daemon交接/回滚通过。集成包87项中仅CLI/runtime二进制变化。用户授权补充准备检查后，prepare/apply均成功，桌面与开始菜单快捷方式及PATH CLI均指向新安装。

安装完成时17个保护进程、63份原始日志前缀、14工作区、5布局、63旧会话及设置保持。正式新终端6efde68e-97c0-4ce6-89d9-17a1a5c1dc0c已选中，原生Codex36964来自active.json的0.169.0完整前缀；实际Velella窗口显示版本与输入提示。旧失败会话没有建立原生对话ID，继续保留其历史，不冒用其他对话恢复。

后续使用变化与部署隔离：旧诊断终端8155ba54收到输入后，更新器确认0.169.0已安装并正常exit0，其4个进程退出；不改写安装时的保留收据。新终端也出现后续输入，验证未发送输入或模型任务。业务UAT未执行；窗口的1个warning徽标未检查，启动成功不代表全部外部集成验收通过。

完整证据：[RESULT](../../artifacts/codex-native-pointer-20261115/RESULT.md)、[安装收据](../../artifacts/codex-native-pointer-20261115/installation.json)、[在线核验](../../artifacts/codex-native-pointer-20261115/online-verification.json)。原导航和更新器的记录继续作为各层证据保留。
