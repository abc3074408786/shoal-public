# Computer Use 插件接入说明

在「插件 → 发现 → Computer Use · 电脑操作」点击「添加 Computer Use」。桌面端只生成本机 MCP 目录，不启动控制服务。载入预填目录后，默认只读观察条目仅有截图；切换控制条目才会看到输入工具。所有工具初始未勾选，必须确认来源与工具后才加入个人清单。

「已添加」可配置、启停和移除来源引用；鱼群角色在外部 MCP 中选用时会重新预览与确认。清单启停只影响后续选用，不停止已有会话。需要立即停止输入时，在相同 Windows 用户下使用服务的本机 `--stop`；只有本机 `--resume` 可恢复，旧凭据不会恢复。

## 运行环境

源码版复用 `tools/computer-use/.venv/Scripts/python.exe`。若尚未准备，使用已有 uv 执行 `uv sync --project tools/computer-use --frozen --python 3.12`，再重试添加。

Windows 安装包携带服务源文件、`pyproject.toml` 和 `uv.lock`，不携带开发机虚拟环境或自动下载安装 Python。服务位于应用 `resources/plugins/computer-use`；目录按应用 profile 隔离。

打包版需显式准备一次环境。以下 PowerShell 命令中的安装资源路径按实际位置填写，使用已有 uv：

```powershell
$computerUseService = '实际安装目录/resources/plugins/computer-use'
$computerUseProfile = if ($env:VELELLA_DATA_DIR) { $env:VELELLA_DATA_DIR } else { Join-Path $env:LOCALAPPDATA 'Velella' }
$env:UV_PROJECT_ENVIRONMENT = Join-Path $computerUseProfile 'extensions/computer-use/venv'
uv sync --project $computerUseService --frozen --python 3.12
Remove-Item Env:UV_PROJECT_ENVIRONMENT
```

未准备环境、资源缺失、非 Windows 和远端主机均明确失败，不回退本机其他环境。添加操作只调用冻结服务的标准库目录生成器，不调用截图或输入工具。依赖准备需显式执行，不是本次源码接入的部署动作。

服务使用和停止说明详见安装资源同目录 `README.md`，或源码树 `tools/computer-use/README.md`。插件接线不改变原服务租约、工具白名单、来源摘要复核或远程角色边界。
