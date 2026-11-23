# Falsify the agent-limits governor against processes it launches itself.
#
# Run through `agent_limits.ps1 -SelfTest`, which has already compiled the
# governor. Every hog is launched through WMI, so it is born outside this
# shell's job, at Normal priority, with exactly the environment the case
# names: an agent running this from an enrolled shell measures the same thing
# the owner would. The governor is scoped to those PIDs and their children and
# can touch nothing else, and it applies BelowNormal / I/O Low / page Low --
# values the live watcher never uses, so a pass cannot be borrowed from it.
#
# The ceilings are 256 MiB per process and 320 MiB per job, so the memory
# cases cost the machine well under a gigabyte.

param([string]$ScratchRoot = "")

$ErrorActionPreference = "Stop"

$hogSource = @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;

static class Hog
{
    static int Main(string[] a)
    {
        string mode = a.Length > 0 ? a[0] : "sleep";
        if (mode == "cpu")
        {
            var sw = Stopwatch.StartNew(); double x = 0;
            while (sw.Elapsed.TotalSeconds < int.Parse(a[1])) x += Math.Sqrt(x + 1);
            return x > 0 ? 0 : 1;
        }
        if (mode == "mem")
        {
            int total = int.Parse(a[1]), step = int.Parse(a[2]), hold = a.Length > 3 ? int.Parse(a[3]) : 0;
            Thread.Sleep(a.Length > 4 ? int.Parse(a[4]) : 0);
            int pause = a.Length > 5 ? int.Parse(a[5]) : 20;
            var keep = new List<byte[]>(); int got = 0;
            try
            {
                while (got < total)
                {
                    var b = new byte[step << 20];
                    for (int i = 0; i < b.Length; i += 4096) b[i] = 1;
                    keep.Add(b); got += step;
                    Thread.Sleep(pause);
                }
            }
            catch (OutOfMemoryException)
            {
                // Survive the refused allocation, as a process that handles it would; only the watcher ends this.
                Thread.Sleep(30000);
                return 3;
            }
            Thread.Sleep(hold * 1000);
            return keep.Count > 0 ? 0 : 1;
        }
        if (mode == "spawn")
        {
            Thread.Sleep(int.Parse(a[1]) * 1000);
            var psi = new ProcessStartInfo(Process.GetCurrentProcess().MainModule.FileName, "cpu 30");
            psi.UseShellExecute = false; psi.CreateNoWindow = true;
            var p = Process.Start(psi);
            p.WaitForExit();
            return p.ExitCode;
        }
        Thread.Sleep(int.Parse(a.Length > 1 ? a[1] : "30") * 1000);
        return 0;
    }
}
'@

$id = [guid]::NewGuid().ToString('N').Substring(0, 8)
$base = if ($ScratchRoot) { $ScratchRoot } else { $env:TEMP }

# Under a watched root the live watcher enrols the hogs too, and every case then reports its values.
$m = $null
if ([System.Threading.Mutex]::TryOpenExisting('Local\OpenRage.AgentLimits.Instance', [ref]$m)) {
    $m.Dispose()
    $watched = (Get-DefaultLaneRoot).TrimEnd('\') + '\'
    if (([System.IO.Path]::GetFullPath($base).TrimEnd('\') + '\').StartsWith($watched, [StringComparison]::OrdinalIgnoreCase)) {
        Write-Host "A watcher is running and $base is under the lane root it watches; pass a -ScratchRoot outside $watched, or leave it at the default." -ForegroundColor Red
        exit 1
    }
}
$scratch = Join-Path $base "openrage-agent-limits-selftest-$id"
$laneRoot = Join-Path $scratch 'lanes'
$lane = Join-Path $laneRoot 'lane-x'
$outside = Join-Path $scratch 'outside'
New-Item -ItemType Directory -Force -Path $lane, $outside | Out-Null
$laneHog = Join-Path $lane 'hog.exe'
$outsideHog = Join-Path $outside 'hog.exe'
Add-Type -TypeDefinition $hogSource -OutputAssembly $laneHog -OutputType ConsoleApplication
Copy-Item -LiteralPath $laneHog -Destination $outsideHog

$markerName = 'OPENRAGE_LIMITS_SELFTEST'
$markerValue = $id

$gov = New-Object OpenRageLimits.Governor
$gov.Roots = [string[]]@("$laneRoot\")
$gov.Markers = [string[]]@("$markerName=$markerValue")
$gov.PriorityClass = [OpenRageLimits.Governor]::BELOW_NORMAL
$gov.IoPriority = 1
$gov.PagePriority = 2
$gov.ProcessMemoryLimit = 256MB
$gov.JobMemoryLimit = 320MB
$gov.ScopePids = [int[]]@(-1)
$gov.Open()

$script:hogs = New-Object System.Collections.Generic.List[int]
$script:procs = @{}
$script:checks = 0
$script:failed = New-Object System.Collections.Generic.List[string]
$script:reached = New-Object System.Collections.Generic.HashSet[string]
$decoy = [IntPtr]::Zero

function Check([string]$Name, [bool]$Ok, [string]$Detail = "") {
    $script:checks++
    if ($Ok) { Write-Host "  ok    $Name" -ForegroundColor Green }
    else { Write-Host "  FAIL  $Name  $Detail" -ForegroundColor Red; $script:failed.Add($Name) }
}

function Start-Hog([string]$Image, [string]$Cwd, [string]$HogArgs, [switch]$Marked) {
    $envs = @(Get-ChildItem Env: | Where-Object { $_.Name -notin @('CURSOR_AGENT', 'CLAUDECODE', $markerName) } | ForEach-Object { "$($_.Name)=$($_.Value)" })
    if ($Marked) { $envs += "$markerName=$markerValue" }
    $su = New-CimInstance -ClassName Win32_ProcessStartup -ClientOnly -Property @{ EnvironmentVariables = [string[]]$envs; ShowWindow = [uint16]0 }
    $r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{ CommandLine = "`"$Image`" $HogArgs"; CurrentDirectory = $Cwd; ProcessStartupInformation = $su }
    if ($r.ReturnValue -ne 0) { throw "Win32_Process.Create returned $($r.ReturnValue)" }
    $hogPid = [int]$r.ProcessId
    $p = [System.Diagnostics.Process]::GetProcessById($hogPid)
    [void]$p.Handle
    $script:procs[$hogPid] = $p
    $script:hogs.Add($hogPid)
    $gov.ScopePids = $script:hogs.ToArray()
    $hogPid
}

function Get-State([int]$TargetPid) {
    $h = [OpenRageLimits.Native]::OpenProcess(0x1000, $false, $TargetPid)
    if ($h -eq [IntPtr]::Zero) { return $null }
    try {
        [pscustomobject]@{
            Priority = [OpenRageLimits.Native]::GetPriorityClass($h)
            Io       = [OpenRageLimits.Governor]::GetIoPriority($h)
            Page     = [OpenRageLimits.Governor]::GetPagePriority($h)
            Job      = $gov.JobIndexOf($TargetPid)
        }
    }
    finally { [void][OpenRageLimits.Native]::CloseHandle($h) }
}

function Tick([int]$Times = 3) { for ($i = 0; $i -lt $Times; $i++) { $gov.Tick(150) } }

function Wait-Exit([int]$TargetPid, [int]$Seconds) {
    $p = $script:procs[$TargetPid]
    $deadline = (Get-Date).AddSeconds($Seconds)
    while (-not $p.HasExited -and (Get-Date) -lt $deadline) { $gov.Tick(200) }
    if ($p.HasExited) { $p.ExitCode } else { $null }
}

function Stop-Hogs {
    foreach ($s in [OpenRageLimits.Governor]::Snapshot()) {
        if ($script:hogs.Contains($s.Parent) -and -not $script:hogs.Contains($s.Pid)) { $script:hogs.Add($s.Pid) }
    }
    foreach ($h in $script:hogs) { Stop-Process -Id $h -Force -ErrorAction SilentlyContinue }
}

try {
    Write-Host "agent-limits selftest in $scratch" -ForegroundColor Cyan

    # First, as on the real machine: the first process enrolled is an agent shell inside Cursor's job,
    # so the first job opened is nested -- and every job-free process after it is tempted to join it.
    Write-Host "job hierarchies"
    $decoy = [OpenRageLimits.Native]::CreateJobObject([IntPtr]::Zero, $null)
    $foreign = Start-Hog $outsideHog $outside 'cpu 30' -Marked
    $fh = [OpenRageLimits.Native]::OpenProcess(0x1101, $false, $foreign)
    Check "a hog can be placed in a foreign job" ([OpenRageLimits.Native]::AssignProcessToJobObject($decoy, $fh))
    [void][OpenRageLimits.Native]::CloseHandle($fh)
    Tick
    $sf = Get-State $foreign
    Check "a process in a foreign job gets a nested job of its own" ($sf.Job -eq 0 -and -not $gov.IsRooted(0)) "state $($sf | ConvertTo-Json -Compress), jobs $($gov.JobCount)"

    Write-Host "recognition"
    $byImage = Start-Hog $laneHog $outside 'cpu 30'
    $byCwd = Start-Hog $outsideHog $lane 'cpu 30'
    $byEnv = Start-Hog $outsideHog $outside 'cpu 30' -Marked
    $owner = Start-Hog $outsideHog $outside 'cpu 30'
    Tick
    foreach ($case in @(@('image under a lane', $byImage), @('working directory under a lane', $byCwd), @('marker in the environment', $byEnv))) {
        $s = Get-State $case[1]
        Check "$($case[0]): enrolled and lowered" ($s.Job -ge 0 -and $s.Priority -eq 0x4000 -and $s.Io -eq 1 -and $s.Page -eq 2) "state $($s | ConvertTo-Json -Compress)"
        [void]$script:reached.Add($case[0])
    }
    $s = Get-State $owner
    Check "no lane, no marker: left alone" ($s.Job -lt 0 -and $s.Priority -eq 0x20 -and $s.Io -eq 2 -and $s.Page -eq 5) "state $($s | ConvertTo-Json -Compress)"
    [void]$script:reached.Add('left alone')
    # It spins at Normal by design, against whatever the owner is running.
    Stop-Process -Id $owner -Force -ErrorAction SilentlyContinue

    $bh = [OpenRageLimits.Native]::OpenProcess(0x1000, $false, $byImage)
    $inDecoy = $true
    [void][OpenRageLimits.Native]::IsProcessInJob($bh, $decoy, [ref]$inDecoy)
    [void][OpenRageLimits.Native]::CloseHandle($bh)
    $sl = Get-State $byImage
    # By kind, not index: a nested job is closed when its last member exits, and the indices move.
    Check "a job-free process never lands under the foreign job" ((-not $inDecoy) -and $sl.Job -ge 0 -and $gov.IsRooted($sl.Job)) "in decoy $inDecoy, state $($sl | ConvertTo-Json -Compress)"
    [void]$script:reached.Add('hierarchies')

    Write-Host "inheritance"
    $parent = Start-Hog $laneHog $outside 'spawn 1'
    Tick
    $child = $null
    $deadline = (Get-Date).AddSeconds(10)
    while (-not $child -and (Get-Date) -lt $deadline) {
        $child = [OpenRageLimits.Governor]::Snapshot() | Where-Object { $_.Parent -eq $parent } | Select-Object -First 1
        if (-not $child) { Start-Sleep -Milliseconds 50 }
    }
    Check "a child appeared" ($null -ne $child)
    if ($child) {
        $born = Get-State $child.Pid
        Check "a child is in the job and BelowNormal before any scan" ($born.Job -ge 0 -and $born.Priority -eq 0x4000) "state $($born | ConvertTo-Json -Compress)"
        Write-Host ("        (at birth it inherited I/O {0}, page {1}; the new-process message sets them)" -f $born.Io, $born.Page)
        Tick
        $s = Get-State $child.Pid
        Check "a child is fully lowered after one tick" ($s.Io -eq 1 -and $s.Page -eq 2) "state $($s | ConvertTo-Json -Compress)"
        [void]$script:reached.Add('inheritance')
    }

    Stop-Hogs
    Tick

    # Each memory hog waits 1.5 s before allocating, so it is enrolled small -- as a process born
    # inside the job is -- rather than racing the first scan.
    Write-Host "memory"
    $runaway = Start-Hog $outsideHog $outside 'mem 1024 32 0 1500' -Marked
    $bystander = Start-Hog $outsideHog $outside 'mem 512 32 0 1500'
    $code = Wait-Exit $runaway 20
    Check "a runaway inside the job is killed at the per-process ceiling" ($code -eq 137 -and $gov.Events -contains "process-limit:$runaway") "exit $code, events $($gov.Events -join ',')"
    $code = Wait-Exit $bystander 20
    Check "a 512 MiB process outside the job runs to completion" ($code -eq 0) "exit $code"
    [void]$script:reached.Add('process ceiling')

    # A census started before the watcher is found already large; it must not live to its next allocation.
    $early = Start-Hog $outsideHog $outside 'mem 320 32 30' -Marked
    $gov.ScopePids = [int[]]@($script:hogs | Where-Object { $_ -ne $early })
    $deadline = (Get-Date).AddSeconds(15)
    $eh = [OpenRageLimits.Native]::OpenProcess(0x1000, $false, $early)
    while ([OpenRageLimits.Governor]::Commit($eh) -lt 300MB -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100 }
    [void][OpenRageLimits.Native]::CloseHandle($eh)
    $gov.ScopePids = $script:hogs.ToArray()
    $code = Wait-Exit $early 10
    Check "a process found already past the ceiling is killed on arrival" ($code -eq 137 -and $gov.Events -contains "over-on-arrival:$early") "exit $code, events $($gov.Events -join ',')"
    [void]$script:reached.Add('over on arrival')
    # The job goes on counting a killed member's commit for a moment after it exits.
    Tick 6

    $holder = Start-Hog $outsideHog $outside 'mem 96 32 30 1500' -Marked
    $grower = Start-Hog $outsideHog $outside 'mem 1024 32 0 3000' -Marked
    $code = Wait-Exit $grower 20
    Check "the largest member is killed when the job passes its ceiling" ($code -eq 137 -and ($gov.Events | Where-Object { $_ -like 'job-limit:*' })) "exit $code, events $($gov.Events -join ',')"
    Check "the smaller member survives it" (-not $script:procs[$holder].HasExited)
    [void]$script:reached.Add('job ceiling')
    Stop-Process -Id $holder -Force -ErrorAction SilentlyContinue
    Tick 6

    # On the real machine every agent command is its own job, so no kernel limit sees the sum: a holder
    # inside a foreign job and a grower in the job-free one are each under the ceiling and together over it.
    $decoy2 = [OpenRageLimits.Native]::CreateJobObject([IntPtr]::Zero, $null)
    $holder2 = Start-Hog $outsideHog $outside 'mem 128 32 30 1500' -Marked
    $hh = [OpenRageLimits.Native]::OpenProcess(0x1101, $false, $holder2)
    [void][OpenRageLimits.Native]::AssignProcessToJobObject($decoy2, $hh)
    [void][OpenRageLimits.Native]::CloseHandle($hh)
    $grower2 = Start-Hog $outsideHog $outside 'mem 1024 16 0 3000 250' -Marked
    $code = Wait-Exit $grower2 30
    Check "the largest agent process is killed when the sum across jobs passes the ceiling" ($code -eq 137 -and $gov.Events -contains "aggregate:$grower2" -and $gov.JobIndexOf($holder2) -ne $gov.JobIndexOf($grower2)) "exit $code, events $($gov.Events -join ',')"
    Check "the holder in the other job survives it" (-not $script:procs[$holder2].HasExited)
    [void]$script:reached.Add('aggregate')
    [void][OpenRageLimits.Native]::CloseHandle($decoy2)

    Write-Host "stopping"
    $sleeper = Start-Hog $outsideHog $outside 'sleep 30' -Marked
    Tick
    $before = Get-State $sleeper
    $gov.Lift()
    $after = Get-State $sleeper
    $flags = 0; for ($i = 0; $i -lt $gov.JobCount; $i++) { $flags = $flags -bor $gov.Limits($i).Basic.LimitFlags }
    Check "lifting restores Normal and clears every ceiling" ($before.Priority -eq 0x4000 -and $after.Priority -eq 0x20 -and $after.Io -eq 2 -and $after.Page -eq 5 -and $flags -eq 0) "before $($before | ConvertTo-Json -Compress) after $($after | ConvertTo-Json -Compress) flags $flags"
    [void]$script:reached.Add('lift')

    $everyone = New-Object System.Collections.Generic.HashSet[int]
    foreach ($s in [OpenRageLimits.Governor]::Snapshot()) { if ($script:hogs.Contains($s.Pid) -or $script:hogs.Contains($s.Parent)) { [void]$everyone.Add($s.Pid) } }
    $strays = @(); for ($i = 0; $i -lt $gov.JobCount; $i++) { $strays += @($gov.Pids($i) | Where-Object { -not $everyone.Contains($_) }) }
    Check "nothing but the hogs was ever enrolled" ($strays.Count -eq 0) "strays $($strays -join ',')"
}
finally {
    Stop-Hogs
    $gov.DrainLog() | ForEach-Object { Write-Host "        governor: $_" -ForegroundColor DarkGray }
    $gov.Dispose()
    if ($decoy -ne [IntPtr]::Zero) { [void][OpenRageLimits.Native]::CloseHandle($decoy) }
    Start-Sleep -Milliseconds 300
    Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
}

# A case lost to an edit leaves every remaining case green, so the count and the paths are what notice.
$expectedChecks = 19
$expectedPaths = @('image under a lane', 'working directory under a lane', 'marker in the environment', 'left alone', 'inheritance', 'hierarchies', 'process ceiling', 'over on arrival', 'job ceiling', 'aggregate', 'lift')
$missing = @($expectedPaths | Where-Object { -not $script:reached.Contains($_) })
if ($script:checks -ne $expectedChecks -or $missing.Count -gt 0) {
    Write-Host "ran $($script:checks) of $expectedChecks checks; paths nothing reached: $($missing -join ', ')" -ForegroundColor Red
    exit 1
}
if ($script:failed.Count -gt 0) { Write-Host "$($script:failed.Count) of $($script:checks) checks failed" -ForegroundColor Red; exit 1 }
Write-Host "all $($script:checks) checks passed" -ForegroundColor Cyan
exit 0
