param(
    [Parameter(Mandatory=$true)][ValidateRange(1,2147483647)][int]$BrowserPid,
    [Parameter(Mandatory=$true)][ValidateRange(1,2147483647)][int]$ServerPid,
    [Parameter(Mandatory=$true)][string]$OutputPath,
    [Parameter(Mandatory=$true)][string]$StopPath
)
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $OutputPath) { throw 'Output already exists; refusing overwrite.' }
$outputFull = [System.IO.Path]::GetFullPath($OutputPath)
$stopFull = [System.IO.Path]::GetFullPath($StopPath)
if ($outputFull -eq $stopFull) { throw 'OutputPath and StopPath must differ.' }
$parentDirectory = [System.IO.Path]::GetDirectoryName($outputFull)
if (-not (Test-Path -LiteralPath $parentDirectory)) { New-Item -ItemType Directory -Path $parentDirectory | Out-Null }
$writer = [System.IO.StreamWriter]::new($outputFull, $false, [System.Text.UTF8Encoding]::new($false))
$writer.AutoFlush = $true
$clock = [System.Diagnostics.Stopwatch]::StartNew()
$known = @{browser=@{};server=@{}}
function Write-Record($Record) { $writer.WriteLine(($Record | ConvertTo-Json -Depth 10 -Compress)) }
function Read-Tree([string]$Group, [int]$RootProcessId, $Snapshot) {
    $identities = $known[$Group]
    # Preserve discovered children after their parent exits; creation time guards PID reuse.
    $current = @{}
    foreach ($item in $Snapshot) { $current[[int]$item.ProcessId] = $item }
    if ($identities.Count -eq 0 -and $current.ContainsKey($RootProcessId)) {
        $identities[$RootProcessId] = $current[$RootProcessId].CreationDate.ToUniversalTime().ToString('o')
    }
    $members = [System.Collections.Generic.HashSet[int]]::new()
    foreach ($key in @($identities.Keys)) {
        if ($current.ContainsKey([int]$key) -and $current[[int]$key].CreationDate.ToUniversalTime().ToString('o') -eq $identities[$key]) {
            [void]$members.Add([int]$key)
        }
    }
    do {
        $added = $false
        foreach ($item in $Snapshot) {
            $processIdValue = [int]$item.ProcessId
            if ($members.Contains([int]$item.ParentProcessId) -and -not $members.Contains($processIdValue)) {
                [void]$members.Add($processIdValue)
                $identities[$processIdValue] = $item.CreationDate.ToUniversalTime().ToString('o')
                $added = $true
            }
        }
    } while ($added)
    $processes = @()
    foreach ($processIdValue in $members) {
        try {
            $process = Get-Process -Id $processIdValue -ErrorAction Stop
            # A process may exit/reuse its PID between CIM and Get-Process.
            if ([Math]::Abs(($process.StartTime.ToUniversalTime() - $current[$processIdValue].CreationDate.ToUniversalTime()).TotalSeconds) -gt 0.01) { continue }
            $processes += @{process_id=$processIdValue;parent_process_id=[int]$current[$processIdValue].ParentProcessId;creation_utc=$identities[$processIdValue];cpu_seconds=$process.TotalProcessorTime.TotalSeconds;working_set_bytes=$process.WorkingSet64}
        } catch [Microsoft.PowerShell.Commands.ProcessCommandException] { }
          catch [System.InvalidOperationException] { }
    }
    $workingSet = 0L
    foreach ($process in $processes) { $workingSet += $process.working_set_bytes }
    return @{root_process_id=$RootProcessId;root_present=$members.Contains($RootProcessId);process_ids=@($processes | ForEach-Object process_id);processes=@($processes);sum_working_set_bytes=$workingSet}
}
try {
    Write-Record @{type='metadata';timestamp_utc=[DateTime]::UtcNow.ToString('o');interval_target_ms=500;maximum_seconds=180;browser_root_process_id=$BrowserPid;server_root_process_id=$ServerPid;limits=@('WorkingSet is resident working set, not private memory; sums double-count shared pages.','CPU is cumulative per PID plus creation time; consumers must calculate nonnegative deltas per identity, never sum cumulative samples or subtract totals across changing process sets.','Processes born and exited between samples can be missed. CIM enumeration and observer consume CPU and perturb timing.','CIM/Get-Process snapshots are not atomic. First-observed descendants persist after parent exit. No executable paths or command lines recorded.')}
    while (-not (Test-Path -LiteralPath $stopFull) -and $clock.Elapsed.TotalSeconds -lt 180) {
        $sampleStart = $clock.Elapsed.TotalMilliseconds
        $snapshot = @(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,CreationDate)
        $browserTree = Read-Tree 'browser' $BrowserPid $snapshot
        $serverTree = Read-Tree 'server' $ServerPid $snapshot
        $observer = Get-Process -Id $PID
        Write-Record @{type='sample';timestamp_utc=[DateTime]::UtcNow.ToString('o');elapsed_ms=$clock.Elapsed.TotalMilliseconds;collection_ms=$clock.Elapsed.TotalMilliseconds-$sampleStart;browser=$browserTree;server=$serverTree;observer=@{process_id=$PID;cpu_seconds=$observer.TotalProcessorTime.TotalSeconds;working_set_bytes=$observer.WorkingSet64}}
        $remaining = 500-($clock.Elapsed.TotalMilliseconds-$sampleStart)
        if ($remaining -gt 0) { Start-Sleep -Milliseconds ([int]$remaining) }
    }
    Write-Record @{type='end';timestamp_utc=[DateTime]::UtcNow.ToString('o');elapsed_ms=$clock.Elapsed.TotalMilliseconds;reason=$(if(Test-Path -LiteralPath $stopFull){'stop_file'}else{'maximum_duration'})}
} finally { $writer.Dispose() }
