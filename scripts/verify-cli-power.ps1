$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'configure-cli-power.ps1')

function Assert-Power($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Expect-PowerFailure([scriptblock]$Run, [string]$Pattern) {
    $errorText = ''
    try { $null = & $Run } catch { $errorText = [string]$_ }
    Assert-Power ($errorText -match $Pattern) "Expected '$Pattern', got '$errorText'"
}
function New-PowerFixture {
    $state = @{
        Scheme = '64a64f24-65b9-4b56-befd-5ec1eaced9b3'
        AcLid = 1; DcLid = 1; AcIdle = 1800; DcIdle = 300
        Writes = 0; FailIdle = $false; FailRefresh = $false; FailReadback = $false; ConcurrentIdle = $false
    }
    $runner = {
        param([string[]]$Arguments)
        switch ($Arguments[0]) {
            '/getactivescheme' { return "Power scheme GUID: $($state.Scheme)" }
            '/qh' {
                $lid = $Arguments[3] -eq 'LIDACTION'
                $ac = if ($lid) { $state.AcLid } else { $state.AcIdle }
                $dc = if ($lid) { $state.DcLid } else { $state.DcIdle }
                return ('unlocalized setting 0x00000000 0xffffffff 0x00000001 AC 0x{0:x8} DC 0x{1:x8}' -f $ac, $dc)
            }
            '/setacvalueindex' {
                $state.Writes++
                $key = if ($Arguments[3] -eq 'LIDACTION') { 'AcLid' } else { 'AcIdle' }
                if ($key -eq 'AcIdle' -and $state.FailIdle) { $state.FailIdle = $false; throw 'Injected idle write failure' }
                if ($state.FailReadback) { $state.FailReadback = $false; return '' }
                $state[$key] = [uint32]$Arguments[4]
                if ($key -eq 'AcLid' -and $state.ConcurrentIdle) { $state.ConcurrentIdle = $false; $state.AcIdle = 0 }
                return ''
            }
            '/setactive' {
                Assert-Power ($Arguments[1] -eq $state.Scheme) 'Must not switch to a different scheme'
                if ($state.FailRefresh) { $state.FailRefresh = $false; throw 'Injected refresh failure' }
                return ''
            }
            default { throw "Unexpected command: $Arguments" }
        }
    }.GetNewClosure()
    return @{ State = $state; Runner = $runner }
}

$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('velella-cli-power-tests-' + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $testRoot
$checks = @()
try {
    $fixture = New-PowerFixture
    $path = Join-Path $testRoot 'status\backup.json'
    $result = Invoke-CliPowerPolicy Status $path $fixture.Runner
    Assert-Power (-not (Test-Path -LiteralPath (Split-Path -Parent $path))) 'Status must not create files'
    Assert-Power ($fixture.State.Writes -eq 0 -and -not $result.acPolicyReady) 'Status must be read-only and accurate'
    $checks += 'default status reads localized-independent values with no writes'

    $result = Invoke-CliPowerPolicy Enable $path $fixture.Runner
    Assert-Power ($fixture.State.AcLid -eq 0 -and $fixture.State.AcIdle -eq 0) 'AC policy must enable'
    Assert-Power ($fixture.State.DcLid -eq 1 -and $fixture.State.DcIdle -eq 300) 'Battery policy must be preserved'
    $writes = $fixture.State.Writes
    $repeat = Invoke-CliPowerPolicy Enable $path $fixture.Runner
    Assert-Power ($repeat.mode -eq 'already-enabled' -and $fixture.State.Writes -eq $writes) 'Repeated enable must be idempotent'
    $null = Invoke-CliPowerPolicy Restore $path $fixture.Runner
    $null = Invoke-CliPowerPolicy Restore $path $fixture.Runner
    Assert-Power ($fixture.State.AcLid -eq 1 -and $fixture.State.AcIdle -eq 1800) 'Exact AC values must restore'
    $fixture.State.AcLid = 0
    $null = Invoke-CliPowerPolicy Restore $path $fixture.Runner
    Assert-Power ($fixture.State.AcLid -eq 0) 'Repeated restore must preserve edits after restoration'
    $checks += 'enable, repeat and restore preserve battery values and original backup'

    foreach ($failure in @('FailIdle', 'FailRefresh', 'FailReadback')) {
        $fixture = New-PowerFixture; $fixture.State[$failure] = $true
        $path = Join-Path $testRoot "$failure.json"
        Expect-PowerFailure { Invoke-CliPowerPolicy Enable $path $fixture.Runner } 'rollback=rolled-back'
        Assert-Power ($fixture.State.AcLid -eq 1 -and $fixture.State.AcIdle -eq 1800) "$failure must restore partial writes"
        Assert-Power ($fixture.State.DcLid -eq 1 -and $fixture.State.DcIdle -eq 300) "$failure must not touch DC"
    }
    $checks += 'write, refresh and readback failures roll back partial changes'

    $fixture = New-PowerFixture; $path = Join-Path $testRoot 'conflict.json'
    $null = Invoke-CliPowerPolicy Enable $path $fixture.Runner
    $fixture.State.AcLid = 2
    Expect-PowerFailure { Invoke-CliPowerPolicy Restore $path $fixture.Runner } 'Restore incomplete'
    Assert-Power ($fixture.State.AcLid -eq 2 -and $fixture.State.AcIdle -eq 1800) 'Later user edit must survive while other owned values restore'
    Expect-PowerFailure { Invoke-CliPowerPolicy Enable $path $fixture.Runner } 'Unresolved backup'
    $checks += 'conflict preserves later user edits and blocks blind re-enabling'

    $fixture = New-PowerFixture; $path = Join-Path $testRoot 'during-enable.json'
    $fixture.State.ConcurrentIdle = $true
    Expect-PowerFailure { Invoke-CliPowerPolicy Enable $path $fixture.Runner } 'changed during setup'
    Assert-Power ($fixture.State.AcLid -eq 1 -and $fixture.State.AcIdle -eq 0) 'Rollback must not undo a setting we never wrote'
    $checks += 'concurrent enable edit survives rollback of only attempted writes'

    $fixture = New-PowerFixture; $path = Join-Path $testRoot 'scheme.json'
    $null = Invoke-CliPowerPolicy Enable $path $fixture.Runner
    $fixture.State.Scheme = '00000000-0000-0000-0000-000000000001'
    $null = Invoke-CliPowerPolicy Restore $path $fixture.Runner
    Assert-Power ($fixture.State.Scheme -eq '00000000-0000-0000-0000-000000000001') 'Restore must not activate old scheme'
    $checks += 'inactive scheme restore never switches active scheme'

    $fixture = New-PowerFixture
    $missingPath = Join-Path $testRoot 'missing.json'
    Expect-PowerFailure { Invoke-CliPowerPolicy Restore $missingPath $fixture.Runner } 'No backup'
    [IO.File]::WriteAllText($missingPath, '{"Version":99}')
    Expect-PowerFailure { Invoke-CliPowerPolicy Enable $missingPath $fixture.Runner } 'Invalid power backup'
    Assert-Power ($fixture.State.Writes -eq 0) 'Invalid backup must not change power settings'
    $checks += 'missing and invalid backups fail without power writes'

    [pscustomobject]@{ ok = $true; checks = $checks; livePowerChanges = $false } | ConvertTo-Json -Depth 4
} finally {
    # Verify the absolute target before recursive deletion; all fixtures live here.
    $resolvedRoot = [IO.Path]::GetFullPath($testRoot)
    $temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolvedRoot.StartsWith($temporaryRoot, [StringComparison]::OrdinalIgnoreCase) -or
        -not ([IO.Path]::GetFileName($resolvedRoot)).StartsWith('velella-cli-power-tests-')) {
        throw 'Refusing cleanup outside the test fixture directory'
    }
    Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
}
