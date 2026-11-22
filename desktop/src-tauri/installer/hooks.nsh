!define VELELLA_HOOK_DIR "${__FILEDIR__}"

!macro NSIS_HOOK_PREINSTALL
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /oname=velella-prepare-upgrade.ps1 "${VELELLA_HOOK_DIR}\prepare-upgrade.ps1"
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\velella-prepare-upgrade.ps1" -InstallDir "$INSTDIR"'
  Pop $0
  Pop $1
  ${If} $0 == 10
    IfSilent velella_upgrade_blocked
    MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION|MB_DEFBUTTON2 "Velella 后台或终端仍在运行。$\r$\n$\r$\n继续升级会停止此安装目录的终端和 Agent 进程。请先保存工作。项目文件和历史记录会保留，但正在运行的程序需要之后重新启动。$\r$\n$\r$\n确定：自动关闭旧版并继续。取消：保留会话，停止安装。" IDOK velella_upgrade_stop
    Abort
    velella_upgrade_stop:
    nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\velella-prepare-upgrade.ps1" -InstallDir "$INSTDIR" -StopRunning'
    Pop $0
    Pop $1
  ${EndIf}
  ${If} $0 != 0
    velella_upgrade_blocked:
    DetailPrint "Velella upgrade preflight: $1"
    MessageBox MB_OK|MB_ICONSTOP "旧版本尚未安全关闭，安装已在覆盖文件前停止。$\r$\n$1$\r$\n请关闭相关会话后重新运行安装器。" /SD IDOK
    SetErrorLevel 20
    Abort
  ${EndIf}
  SetOutPath "$INSTDIR"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; PREINSTALL has already stopped all installation-owned versions explicitly.
  ; A full installer ships a fresh root sidecar, so retire the old pointer only.
  Delete "$INSTDIR\active-runtime.json"
!macroend
