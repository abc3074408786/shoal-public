param([Parameter(Mandatory=$true)][string]$InstallDir,[switch]$StopRunning)
$ErrorActionPreference='Stop'
try {
    $root=[IO.Path]::GetFullPath($InstallDir).TrimEnd('\')
    $runtime=Join-Path $root 'velella-runtime.exe'
    $desktop=Join-Path $root 'velella-desktop.exe'
    # Versioned sidecars may still host old sessions. Include only executables
    # physically inside this installation, and never infer another data root.
    $runtimes=@($runtime)
    $versions=Join-Path $root 'versions'
    if(Test-Path -LiteralPath $versions){
        $runtimes+=@(Get-ChildItem -LiteralPath $versions -Filter 'velella-runtime.exe' -Recurse | Where-Object { !$_.Attributes.HasFlag([IO.FileAttributes]::ReparsePoint) } | ForEach-Object {$_.FullName})
    }
    function Get-Targets {
        @(Get-CimInstance Win32_Process -Filter "Name='velella-runtime.exe' OR Name='velella-desktop.exe'" | Where-Object {
            $_.ExecutablePath -and ($_.ExecutablePath -in $runtimes -or $_.ExecutablePath -eq $desktop)
        })
    }
    $targets=Get-Targets
    if($targets.Count -gt 0 -and !$StopRunning){Write-Output ('Running processes: '+$targets.Count);exit 10}
    if($targets.Count -gt 0){
        # Resolve every daemon data root before stopping anything. Never guess a
        # default data root or kill all same-name processes on this computer.
        $roots=@()
        foreach($process in $targets | Where-Object {$_.ExecutablePath -in $runtimes}){
            if($process.CommandLine -match '\sdaemon\s+(?:"([^"]+)"|(.+))$'){
                $data=if($Matches[1]){$Matches[1]}else{$Matches[2].Trim()}
                if(![IO.Path]::IsPathRooted($data) -or $data.Contains('"')){throw 'Cannot safely identify daemon data directory'}
                $roots+=@{Data=$data;Runtime=$process.ExecutablePath}
            }elseif($process.CommandLine -notmatch '\shost\s'){
                throw 'Unrecognized runtime process; close it before upgrading'
            }
        }
        if(($targets | Where-Object {$_.ExecutablePath -in $runtimes}) -and !$roots.Count){throw 'Active session has no daemon; close its terminal before upgrading'}
        foreach($process in $targets | Where-Object {$_.ExecutablePath -eq $desktop}){
            $live=Get-Process -Id $process.ProcessId -ErrorAction SilentlyContinue
            if($live -and $live.Path -eq $desktop){Stop-Process -Id $live.Id -ErrorAction Stop}
        }
        foreach($entry in $roots){
            $data=$entry.Data
            $start=New-Object Diagnostics.ProcessStartInfo
            $start.FileName=$entry.Runtime
            $start.Arguments='rpc "'+$data+'" "{\"cmd\":\"service.stop\"}"'
            $start.UseShellExecute=$false;$start.CreateNoWindow=$true
            $start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
            $child=[Diagnostics.Process]::Start($start)
            if(!$child.WaitForExit(20000)){ $child.Kill();throw 'Service shutdown timed out; files were not replaced' }
            $reply=$child.StandardOutput.ReadToEnd() | ConvertFrom-Json
            if($child.ExitCode -ne 0 -or !$reply.ok){throw 'Service refused shutdown; files were not replaced'}
            $child.Dispose()
        }
        $until=[DateTime]::UtcNow.AddSeconds(15)
        do {
            $remaining=Get-Targets
            if(!$remaining.Count){break}
            Start-Sleep -Milliseconds 250
        }while([DateTime]::UtcNow -lt $until)
        if($remaining.Count){throw 'A process is still running; files were not replaced'}
    }
    $releaseDeadline=[DateTime]::UtcNow.AddSeconds(15)
    do {
        $lockError=$null
        foreach($file in @($desktop,$runtime)){
            if(Test-Path -LiteralPath $file){
                try {
                    $handle=[IO.File]::Open($file,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
                    $handle.Dispose()
                } catch { $lockError=$_.Exception.Message;break }
            }
        }
        if(!$lockError){break}
        if(!$StopRunning -or [DateTime]::UtcNow -ge $releaseDeadline){throw $lockError}
        Start-Sleep -Milliseconds 250
    }while($true)
    Write-Output 'Ready for upgrade';exit 0
}catch{
    Write-Output $_.Exception.Message
    exit 20
}
