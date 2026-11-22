param(
    [string]$Source = (Join-Path $PSScriptRoot '..\target\release\velella-cli.exe'),
    [string]$Destination = (Join-Path $env:LOCALAPPDATA 'Programs\Velella CLI'),
    [switch]$SkipPath
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
$sourceFile = (Resolve-Path -LiteralPath $Source).Path
$destinationDirectory = [IO.Path]::GetFullPath($Destination)
$destinationFile = Join-Path $destinationDirectory 'velella-cli.exe'
# Validate identity before copying or making a command available on PATH.
$version = & $sourceFile --version
if ($LASTEXITCODE -ne 0 -or $version -notmatch '^velella-cli ') {
    throw 'Source does not identify itself as velella-cli'
}
New-Item -ItemType Directory -Path $destinationDirectory -Force | Out-Null
$backupFile = $null
if (Test-Path -LiteralPath $destinationFile) {
    $sourceHash = (Get-FileHash -LiteralPath $sourceFile -Algorithm SHA256).Hash
    $previousHash = (Get-FileHash -LiteralPath $destinationFile -Algorithm SHA256).Hash
    if ($sourceHash -ne $previousHash) {
        $backupFile = "$destinationFile.$([DateTime]::Now.ToString('yyyyMMdd-HHmmss')).bak"
        Copy-Item -LiteralPath $destinationFile -Destination $backupFile
    }
}
if ($sourceFile -ne $destinationFile) {
    Copy-Item -LiteralPath $sourceFile -Destination $destinationFile -Force
}
$pathAdded = $false
if (-not $SkipPath) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $previousDirectories = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Orca Fish CLI'),
        (Join-Path $env:LOCALAPPDATA ('Programs\' + [string][char]0x9c7c + [char]0x7fa4))
    )
    $previousEntries = @($userPath -split ';' | Where-Object { $_ })
    $entries = @($previousEntries | Where-Object { $entry=$_.TrimEnd('\'); -not ($previousDirectories | Where-Object { $_.TrimEnd('\') -ieq $entry }) })
    if (-not ($entries | Where-Object { $_.TrimEnd('\') -ieq $destinationDirectory.TrimEnd('\') })) {
        $entries += $destinationDirectory
        $pathAdded = $true
    }
    if (($entries -join ';') -cne ($previousEntries -join ';')) {
        [Environment]::SetEnvironmentVariable('Path', ($entries -join ';'), 'User')
    }
    $env:Path = (($env:Path -split ';' | Where-Object { $entry=$_.TrimEnd('\'); $_ -and -not ($previousDirectories | Where-Object { $_.TrimEnd('\') -ieq $entry }) }) -join ';')
    if (-not (($env:Path -split ';') | Where-Object { $_.TrimEnd('\') -ieq $destinationDirectory.TrimEnd('\') })) {
        $env:Path = "$destinationDirectory;$env:Path"
    }
}
$installedVersion = & $destinationFile --version
if ($LASTEXITCODE -ne 0) { throw 'Installed CLI could not run' }
[pscustomobject]@{
    executable = $destinationFile
    sha256 = (Get-FileHash -LiteralPath $destinationFile -Algorithm SHA256).Hash
    version = $installedVersion
    userPathAdded = $pathAdded
    backup = $backupFile
    note = 'Existing desktop processes are unchanged. New terminals inherit the updated user PATH.'
} | ConvertTo-Json
