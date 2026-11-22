param(
    [ValidateSet('Status', 'Enable', 'Restore')][string]$Mode = 'Status',
    [string]$BackupPath = (Join-Path $env:LOCALAPPDATA 'Velella\cli-power-policy.json')
)

# Dot-sourcing exposes the transaction for isolated tests; it never runs setup.
function Invoke-CliPowerCfg {
    param([string[]]$Arguments)
    $output = & "$env:WINDIR\System32\powercfg.exe" @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) { throw "powercfg $($Arguments -join ' ') failed ($LASTEXITCODE): $output" }
    return ($output -join "`n")
}

function Get-CliPowerScheme {
    param([scriptblock]$Runner)
    $output = & $Runner -Arguments @('/getactivescheme')
    $match = [regex]::Match([string]$output, '(?i)\b[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}\b')
    if (-not $match.Success) { throw 'Cannot determine the active power scheme.' }
    return $match.Value.ToLowerInvariant()
}

function Get-CliPowerValues {
    param([string]$Scheme, [scriptblock]$Runner)
    $values = [ordered]@{}
    foreach ($setting in @(@('Lid', 'SUB_BUTTONS', 'LIDACTION'), @('Idle', 'SUB_SLEEP', 'STANDBYIDLE'))) {
        $output = & $Runner -Arguments @('/qh', $Scheme, $setting[1], $setting[2])
        # AC and DC are the final two hexadecimal indices, independent of UI language.
        $indices = [regex]::Matches([string]$output, '(?i)0x([0-9a-f]{8})\b')
        if ($indices.Count -lt 2) { throw "Cannot read AC/DC $($setting[0]) values for $Scheme." }
        $values["Ac$($setting[0])"] = [Convert]::ToUInt32($indices[$indices.Count - 2].Groups[1].Value, 16)
        $values["Dc$($setting[0])"] = [Convert]::ToUInt32($indices[$indices.Count - 1].Groups[1].Value, 16)
    }
    return [pscustomobject]$values
}

function Save-CliPowerBackup {
    param($Record, [string]$Path)
    $temporary = "$Path.$([guid]::NewGuid().ToString('N')).tmp"
    try {
        [IO.File]::WriteAllText($temporary, ($Record | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
        Move-Item -LiteralPath $temporary -Destination $Path -Force
    } finally {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
    }
}

function Set-CliPowerValue {
    param([string]$Scheme, [string]$Key, [uint32]$Value, [scriptblock]$Runner)
    $setting = if ($Key -eq 'AcLid') { @('SUB_BUTTONS', 'LIDACTION') }
        elseif ($Key -eq 'AcIdle') { @('SUB_SLEEP', 'STANDBYIDLE') }
        else { throw "Refusing to change unowned setting $Key" }
    $null = & $Runner -Arguments @('/setacvalueindex', $Scheme, $setting[0], $setting[1], [string]$Value)
    $actual = Get-CliPowerValues $Scheme $Runner
    if ($actual.$Key -ne $Value) { throw "Readback mismatch for $Key" }
}

function Restore-CliPowerValues {
    param($Record, [scriptblock]$Runner)
    $conflicts = @()
    foreach ($key in $Record.Touched) {
        $current = Get-CliPowerValues $Record.Scheme $Runner
        if ($current.$key -eq $Record.Before.$key) { continue }
        if ($current.$key -ne 0) { $conflicts += "$key changed by another owner"; continue }
        try { Set-CliPowerValue $Record.Scheme $key $Record.Before.$key $Runner }
        catch { $conflicts += "$key restore failed: $_" }
    }
    # Never activate a different plan, even when restoring an inactive backup.
    if ((Get-CliPowerScheme $Runner) -eq $Record.Scheme) {
        try { $null = & $Runner -Arguments @('/setactive', $Record.Scheme) }
        catch { $conflicts += "Power scheme refresh failed: $_" }
    }
    return $conflicts
}

function Invoke-CliPowerPolicy {
    param(
        [ValidateSet('Status', 'Enable', 'Restore')][string]$Action,
        [string]$Path,
        [scriptblock]$Runner = { param([string[]]$Arguments) Invoke-CliPowerCfg $Arguments }
    )
    $scheme = Get-CliPowerScheme $Runner
    $before = Get-CliPowerValues $scheme $Runner
    if ($Action -eq 'Status') {
        return [pscustomobject]@{
            mode = 'status'; scheme = $scheme; settings = $before
            acPolicyReady = ($before.AcLid -eq 0 -and $before.AcIdle -eq 0)
            backup = $Path
            note = 'Read-only. AC setup is system-wide and persistent. Battery policy is not changed.'
        }
    }
    $Path = [IO.Path]::GetFullPath($Path)
    $null = New-Item -ItemType Directory -Path (Split-Path -Parent $Path) -Force
    # Serialize enable/restore using the same backup; status creates no lock/file.
    $lock = [IO.File]::Open("$Path.lock", 'OpenOrCreate', 'ReadWrite', 'None')
    try {
        $record = $null
        if (Test-Path -LiteralPath $Path) {
            $record = Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json
            if ($record.Version -ne 1 -or $record.Scheme -notmatch '^[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$' -or
                $record.Phase -notin @('pending', 'applied', 'restored', 'conflict', 'rolled-back') -or
                @($record.Owned | Where-Object { $_ -notin @('AcLid', 'AcIdle') }).Count -gt 0 -or
                $null -eq $record.Touched -or
                @($record.Touched | Where-Object { $_ -notin $record.Owned }).Count -gt 0) {
                throw 'Invalid power backup; refusing system changes.'
            }
            foreach ($key in @('AcLid', 'DcLid', 'AcIdle', 'DcIdle')) {
                if ($null -eq $record.Before.$key) { throw "Backup is missing $key" }
                $null = [uint32]$record.Before.$key
            }
            if ($record.Before.AcLid -gt 3 -or $record.Before.DcLid -gt 3) { throw 'Invalid lid action in backup.' }
        }
        if ($Action -eq 'Restore') {
            if ($null -eq $record) { throw 'No backup exists; nothing can be restored.' }
            if ($record.Phase -in @('restored', 'rolled-back')) {
                return [pscustomobject]@{ mode = 'already-restored'; scheme = $record.Scheme; settings = (Get-CliPowerValues $record.Scheme $Runner); backup = $Path }
            }
            $conflicts = @(Restore-CliPowerValues $record $Runner)
            $record.Phase = if ($conflicts.Count) { 'conflict' } else { 'restored' }
            Save-CliPowerBackup $record $Path
            if ($conflicts.Count) { throw "Restore incomplete: $($conflicts -join '; '). Backup retained: $Path" }
            return [pscustomobject]@{ mode = 'restored'; scheme = $record.Scheme; settings = (Get-CliPowerValues $record.Scheme $Runner); backup = $Path }
        }
        if ($record -and $record.Phase -notin @('restored', 'rolled-back')) {
            if ($record.Scheme -eq $scheme -and $record.Phase -eq 'applied' -and $before.AcLid -eq 0 -and $before.AcIdle -eq 0) {
                return [pscustomobject]@{ mode = 'already-enabled'; scheme = $scheme; settings = $before; backup = $Path }
            }
            throw "Unresolved backup or changed settings. Restore/review $Path before enabling again."
        }
        $record = [pscustomobject]@{
            Version = 1; Scheme = $scheme; Before = $before; Phase = 'pending'
            Owned = @(@('AcLid', 'AcIdle') | Where-Object { $before.$_ -ne 0 })
            Touched = @()
            CreatedUtc = [DateTime]::UtcNow.ToString('o')
        }
        Save-CliPowerBackup $record $Path # persist recovery information before any write
        try {
            foreach ($key in $record.Owned) {
                if ((Get-CliPowerScheme $Runner) -ne $scheme) { throw 'Active power scheme changed during setup.' }
                $current = Get-CliPowerValues $scheme $Runner
                if ($current.$key -ne $before.$key) { throw "$key changed during setup." }
                $record.Touched = @($record.Touched) + $key
                Save-CliPowerBackup $record $Path # an interrupted write may already have taken effect
                Set-CliPowerValue $scheme $key 0 $Runner
            }
            if ((Get-CliPowerScheme $Runner) -ne $scheme) { throw 'Active power scheme changed during setup.' }
            $null = & $Runner -Arguments @('/setactive', $scheme)
            $actual = Get-CliPowerValues $scheme $Runner
            if ($actual.AcLid -ne 0 -or $actual.AcIdle -ne 0) { throw 'Final AC policy verification failed.' }
            if ($actual.DcLid -ne $before.DcLid -or $actual.DcIdle -ne $before.DcIdle) { throw 'Battery settings changed externally during setup; review power policy.' }
            $record.Phase = 'applied'
            Save-CliPowerBackup $record $Path
            return [pscustomobject]@{
                mode = 'enabled'; scheme = $scheme; settings = $actual; backup = $Path
                note = 'AC lid/idle setup persists after Velella exits. Battery policy, display and lock settings are unchanged.'
            }
        } catch {
            $failure = [string]$_
            $conflicts = @(Restore-CliPowerValues $record $Runner)
            $record.Phase = if ($conflicts.Count) { 'conflict' } else { 'rolled-back' }
            Save-CliPowerBackup $record $Path
            throw "Enable failed: $failure; rollback=$($record.Phase); details=$($conflicts -join '; '); backup=$Path"
        }
    } finally { $lock.Dispose() }
}

if ($MyInvocation.InvocationName -ne '.') {
    $ErrorActionPreference = 'Stop'
    Invoke-CliPowerPolicy -Action $Mode -Path $BackupPath | ConvertTo-Json -Depth 8
}
