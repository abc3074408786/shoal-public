$ErrorActionPreference = 'Stop'
$candidateRoot = Split-Path -Parent $PSScriptRoot
$candidateGui = Join-Path $candidateRoot 'desktop\src-tauri\target\release\velella-desktop.exe'
$candidateRuntime = Join-Path $candidateRoot 'target\release\velella-runtime.exe'
$candidateData = Join-Path $candidateRoot 'artifacts\task-sources-20261018\candidate-profile'
if (!(Test-Path -LiteralPath $candidateGui) -or !(Test-Path -LiteralPath $candidateRuntime)) {
    throw 'Candidate build is not available. See artifacts/task-sources-20261018/continuation/desktop-release.log.'
}
New-Item -ItemType Directory -Path $candidateData -Force | Out-Null
$previousData = $env:VELELLA_DATA_DIR
$previousRuntime = $env:VELELLA_RUNTIME_PATH
$previousBackground = $env:VELELLA_DESKTOP_BACKGROUND
try {
    $env:VELELLA_DATA_DIR = $candidateData
    $env:VELELLA_RUNTIME_PATH = $candidateRuntime
    $env:VELELLA_DESKTOP_BACKGROUND = '0'
    # Interactive candidate requested by the person running this script.
    Start-Process -FilePath $candidateGui -WorkingDirectory $candidateRoot -WindowStyle Normal | Out-Null
} finally {
    $env:VELELLA_DATA_DIR = $previousData
    $env:VELELLA_RUNTIME_PATH = $previousRuntime
    $env:VELELLA_DESKTOP_BACKGROUND = $previousBackground
}
Write-Output "Opened isolated candidate. Data: $candidateData"
