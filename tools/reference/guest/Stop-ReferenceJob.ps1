$completed = [bool](Test-Path -LiteralPath $ExitPath)
if ($completed) {
    [pscustomobject]@{ stopped = $false; reason = 'already completed'; identity_match = $false; stopped_ids = @(); skipped = @() } | ConvertTo-Json -Depth 6 -Compress
    return
}
$rootCim = Get-CimInstance Win32_Process -Filter "ProcessId = $PidValue"
if ($null -eq $rootCim) {
    [pscustomobject]@{ stopped = $false; reason = 'no process with original PID'; identity_match = $false; stopped_ids = @(); skipped = @() } | ConvertTo-Json -Depth 6 -Compress
    return
}
$rootProcess = Get-Process -Id $PidValue -ErrorAction SilentlyContinue
if ($null -eq $rootProcess) {
    [pscustomobject]@{ stopped = $false; reason = 'no live process with original PID'; identity_match = $false; stopped_ids = @(); skipped = @() } | ConvertTo-Json -Depth 6 -Compress
    return
}
$actualStart = $rootProcess.StartTime.ToString('o')
$cimStart = $rootCim.CreationDate.ToString('o')
$actualPath = $rootProcess.Path
$identityMatch = (
    $actualStart -ceq $ExpectedStart -and
    $cimStart -ceq $ExpectedStart -and
    $actualPath -ieq $ExpectedExecutable -and
    $rootCim.ExecutablePath -ieq $ExpectedExecutable
)
$current = [pscustomobject]@{ pid = [int]$PidValue; executable = $actualPath; created = $actualStart; cim_created = $cimStart; cim_executable = $rootCim.ExecutablePath }
if (-not $identityMatch) {
    [pscustomobject]@{ stopped = $false; reason = 'original executable/start identity mismatch; no process killed'; identity_match = $false; current_process = $current; stopped_ids = @(); skipped = @() } | ConvertTo-Json -Depth 6 -Compress
    return
}
if ($rootProcess.HasExited) {
    [pscustomobject]@{ stopped = $false; reason = 'verified original wrapper already exited'; identity_match = $true; current_process = $current; stopped_ids = @(); skipped = @() } | ConvertTo-Json -Depth 6 -Compress
    return
}

$all = @(Get-CimInstance Win32_Process)
$byParent = @{}
foreach ($item in $all) {
    $key = [string][int]$item.ParentProcessId
    if (-not $byParent.ContainsKey($key)) { $byParent[$key] = New-Object System.Collections.Generic.List[object] }
    $byParent[$key].Add($item)
}
$queue = New-Object System.Collections.Generic.Queue[object]
$queue.Enqueue([pscustomobject]@{ Id = [int]$PidValue; Depth = 0 })
$seen = @{ ([string][int]$PidValue) = $true }
$targets = New-Object System.Collections.Generic.List[object]
while ($queue.Count -gt 0) {
    $parent = $queue.Dequeue()
    $key = [string]$parent.Id
    if (-not $byParent.ContainsKey($key)) { continue }
    foreach ($child in $byParent[$key]) {
        $childId = [int]$child.ProcessId
        if ($seen.ContainsKey([string]$childId)) { continue }
        $seen[[string]$childId] = $true
        $depth = [int]$parent.Depth + 1
        $targets.Add([pscustomobject]@{ Id = $childId; Depth = $depth; Created = $child.CreationDate.ToString('o'); Executable = $child.ExecutablePath })
        $queue.Enqueue([pscustomobject]@{ Id = $childId; Depth = $depth })
    }
}
$targets.Add([pscustomobject]@{ Id = [int]$PidValue; Depth = 0; Created = $cimStart; Executable = $rootCim.ExecutablePath })
$ordered = @($targets | Sort-Object Depth -Descending)
$stoppedIds = New-Object System.Collections.Generic.List[int]
$skipped = New-Object System.Collections.Generic.List[object]
foreach ($target in $ordered) {
    $process = Get-Process -Id $target.Id -ErrorAction SilentlyContinue
    if ($null -eq $process) { continue }
    try {
        if ($process.HasExited) { continue }
        $targetStart = $process.StartTime.ToString('o')
        $targetPath = $process.Path
        $targetMatches = ($targetStart -ceq $target.Created)
        if (-not [string]::IsNullOrEmpty([string]$target.Executable)) {
            $targetMatches = $targetMatches -and ($targetPath -ieq $target.Executable)
        }
        if (-not $targetMatches) {
            $skipped.Add([pscustomobject]@{ pid = [int]$target.Id; reason = 'descendant identity mismatch'; executable = $targetPath; created = $targetStart })
            continue
        }
        $process.Kill()
        $stoppedIds.Add([int]$target.Id)
    } catch {
        $skipped.Add([pscustomobject]@{ pid = [int]$target.Id; reason = 'Kill() failed'; error = ($_ | Out-String) })
    }
}
[pscustomobject]@{
    stopped = ($stoppedIds -contains [int]$PidValue)
    reason = 'identity-verified Process.Kill handles used; no taskkill or PID-only termination'
    identity_match = $true
    current_process = $current
    stopped_ids = @($stoppedIds)
    skipped = @($skipped)
    method = 'System.Diagnostics.Process.Kill on exact live handles after executable and creation-time checks'
} | ConvertTo-Json -Depth 6 -Compress
