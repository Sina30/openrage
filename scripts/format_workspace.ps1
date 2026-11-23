# Format the workspace without putting every target on one command line.
#
# `cargo fmt --all` collects every target root in the workspace and hands the
# whole list to a single `rustfmt` process. Windows caps a command line at
# 32,766 characters -- CreateProcessW documents 32,767 and that figure counts
# the terminating null; past it the spawn fails with error 206, "the filename
# or extension is too long". Measured at 709c1431 over 271 targets: 24,647
# characters from a 29-character checkout root, 30,880 from a 52-character
# lane, and 32,506 -- 260 short of the cap -- from the 58-character lane roots
# this machine is already carrying. Every target added spends its own path
# again, so the margin is "how many test files nobody has written yet", and it
# only ever falls.
#
# This runs the same rustfmt over the same target roots in as many processes
# as it takes to stay under -Budget. The bound is per invocation, so adding a
# target adds a path to a chunk or, eventually, one more chunk -- and never
# brings the limit closer.
#
# ⚠ The set of roots has to be the set `cargo fmt --all` uses, or this lands
# unformatted code under a green check. It is derived the way cargo-fmt
# derives it: `cargo metadata --no-deps` over the workspace, every target of
# every member, plus the same recursion into path dependencies that are not
# members, deduplicated by path. -ListTargets prints it, so the two are
# comparable rather than arguable.
#
#   ./scripts/format_workspace.ps1              # rewrite
#   ./scripts/format_workspace.ps1 -Check       # report, change nothing
#   ./scripts/format_workspace.ps1 -ListTargets # the file set, one per line
#   ./scripts/format_workspace.ps1 -SelfTest    # falsify both halves

[CmdletBinding()]
param(
    [switch]$Check,
    [string]$ManifestPath,
    [int]$Budget = 30000,
    [int]$Concurrency = 0,
    [switch]$ListTargets,
    [switch]$SelfTest,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$RustfmtArgs
)

$ErrorActionPreference = 'Stop'

# Ordinal, because this reproduces a `BTreeSet<PathBuf>`: cargo-fmt orders by
# the OS string's bytes, and a culture-aware sort disagrees with that on
# punctuation, which is most of a path.
function Sort-Ordinal {
    param([string[]]$Values)
    $copy = [string[]]@($Values)
    [Array]::Sort($copy, [StringComparer]::Ordinal)
    , $copy
}

# cargo-fmt canonicalizes, which on Windows yields a `\\?\` device path. The
# prefix names the same file and is not part of a comparison.
function Get-ComparablePath {
    param([string]$Value)
    $trimmed = $Value.Trim()
    if ($trimmed.StartsWith('\\?\')) { $trimmed = $trimmed.Substring(4) }
    ([System.IO.Path]::GetFullPath($trimmed)).ToLowerInvariant()
}

# ⚠ `$ErrorActionPreference = 'Stop'` turns a native command's *stderr* into a
# terminating error, and cargo writes "Blocking waiting for file lock on
# package cache" there whenever another build holds it. Without this a busy
# machine fails the pre-commit hook with a PowerShell stack trace about a
# message that is not an error.
function Invoke-Native {
    param([string]$Exe, [string[]]$Arguments)
    $ErrorActionPreference = 'Continue'
    $output = & $Exe @Arguments 2>&1
    $isError = { param($line) $line -is [System.Management.Automation.ErrorRecord] }
    [PSCustomObject]@{
        Code   = $LASTEXITCODE
        Stdout = @($output | Where-Object { -not (& $isError $_) })
        Stderr = @($output | Where-Object { & $isError $_ } | ForEach-Object { "$_" })
    }
}

function Get-Targets {
    param([string]$Manifest)

    $byPath = @{}
    $seenManifest = @{}
    $seenDependency = @{}
    $queue = [System.Collections.Queue]::new()
    $queue.Enqueue($Manifest)

    while ($queue.Count -gt 0) {
        $current = [string]$queue.Dequeue()
        if ($seenManifest.ContainsKey($current)) { continue }
        $seenManifest[$current] = $true

        $result = Invoke-Native cargo @('metadata', '--no-deps', '--format-version', '1', '--manifest-path', $current)
        if ($result.Code -ne 0) {
            throw "cargo metadata failed for ${current}:`n$($result.Stderr -join "`n")"
        }
        $metadata = ($result.Stdout -join '') | ConvertFrom-Json

        $members = @{}
        foreach ($package in $metadata.packages) { $members[$package.manifest_path] = $true }

        foreach ($package in $metadata.packages) {
            foreach ($target in $package.targets) {
                $path = [System.IO.Path]::GetFullPath($target.src_path)
                if (-not $byPath.ContainsKey($path)) { $byPath[$path] = $target.edition }
            }

            # cargo-fmt follows a path dependency that is not itself a member,
            # and skips a dependency name it has already followed.
            foreach ($dependency in $package.dependencies) {
                if (-not $dependency.path) { continue }
                if ($seenDependency.ContainsKey($dependency.name)) { continue }
                $manifest = [System.IO.Path]::GetFullPath((Join-Path $dependency.path 'Cargo.toml'))
                if ((Test-Path -LiteralPath $manifest) -and -not $members.ContainsKey($manifest)) {
                    $seenDependency[$dependency.name] = $true
                    $queue.Enqueue($manifest)
                }
            }
        }
    }

    $byPath
}

# The command line is built here rather than left to the caller's quoting
# rules, because the budget below is only meaningful if it counts the string
# that CreateProcessW will actually see.
function Format-Argument {
    param([string]$Value)
    if ($Value -match '[\s"]') { return '"' + $Value.Replace('"', '\"') + '"' }
    $Value
}

function Split-IntoChunks {
    param([string[]]$Files, [int]$Head, [int]$Limit, [int]$MaxPerChunk)

    $chunks = [System.Collections.Generic.List[object]]::new()
    $current = [System.Collections.Generic.List[string]]::new()
    $length = $Head

    foreach ($file in $Files) {
        $cost = 1 + (Format-Argument $file).Length
        # A chunk always takes at least one file, so a single path longer than
        # the budget is passed rather than dropped: rustfmt refusing it is a
        # better failure than this quietly formatting less.
        $full = $current.Count -gt 0 -and
            (($length + $cost) -gt $Limit -or ($MaxPerChunk -gt 0 -and $current.Count -ge $MaxPerChunk))
        if ($full) {
            $chunks.Add($current.ToArray())
            $current = [System.Collections.Generic.List[string]]::new()
            $length = $Head
        }
        $current.Add($file)
        $length += $cost
    }
    if ($current.Count -gt 0) { $chunks.Add($current.ToArray()) }

    Write-Output -NoEnumerate $chunks
}

function Resolve-Rustfmt {
    if ($env:RUSTFMT) { return $env:RUSTFMT }
    $command = Get-Command rustfmt -ErrorAction SilentlyContinue
    if (-not $command) {
        throw 'rustfmt was not found on PATH. Install it with: rustup component add rustfmt'
    }
    $command.Source
}

# Both streams are read asynchronously, so a process filling a pipe while
# this waits on a different one cannot deadlock the run.
function Start-Rustfmt {
    param([string]$Exe, [string]$Arguments)
    $process = [Diagnostics.Process]::new()
    $process.StartInfo.FileName = $Exe
    $process.StartInfo.Arguments = $Arguments
    $process.StartInfo.UseShellExecute = $false
    $process.StartInfo.RedirectStandardOutput = $true
    $process.StartInfo.RedirectStandardError = $true
    [void]$process.Start()
    [PSCustomObject]@{
        Process = $process
        Stdout  = $process.StandardOutput.ReadToEndAsync()
        Stderr  = $process.StandardError.ReadToEndAsync()
    }
}

function Invoke-Format {
    param([string]$Manifest, [int]$Limit, [int]$Lanes, [switch]$CheckOnly, [string[]]$Extra)

    $rustfmt = Resolve-Rustfmt
    $targets = Get-Targets -Manifest $Manifest

    # ⚠ Formatting nothing and reporting success is the one outcome that looks
    # exactly like a clean tree. Whatever went wrong upstream, say so.
    if ($targets.Count -eq 0) {
        throw "no targets found under $Manifest, so nothing would have been formatted"
    }

    $flags = @()
    if ($CheckOnly) { $flags += '--check' }
    # Output is captured, so rustfmt sees a pipe and drops its colours unless
    # told otherwise. Ask for them back only when this run's own output is a
    # terminal, which the pre-commit hook's is not.
    if (-not [Console]::IsOutputRedirected) { $flags += @('--color', 'always') }
    foreach ($extraArgument in $Extra) { if ($extraArgument) { $flags += $extraArgument } }

    # `"<exe>" --edition NNNN` plus the flags: what every chunk pays before a
    # single path is added. Every edition is four digits.
    $head = $rustfmt.Length + 2 + ' --edition'.Length + ' 2015'.Length
    foreach ($flag in $flags) { $head += 1 + (Format-Argument $flag).Length }

    $perChunk = 0
    if ($Lanes -gt 1) { $perChunk = [Math]::Max(1, [Math]::Ceiling($targets.Count / $Lanes)) }

    $work = [System.Collections.Generic.List[object]]::new()
    foreach ($edition in (Sort-Ordinal ([string[]]@($targets.Values | Select-Object -Unique)))) {
        $files = Sort-Ordinal ([string[]]@($targets.Keys | Where-Object { $targets[$_] -eq $edition }))
        foreach ($chunk in (Split-IntoChunks -Files $files -Head $head -Limit $Limit -MaxPerChunk $perChunk)) {
            $parts = @("--edition", $edition) + $flags + $chunk
            $arguments = ($parts | ForEach-Object { Format-Argument $_ }) -join ' '
            $work.Add([PSCustomObject]@{
                    Arguments = $arguments
                    Length    = $rustfmt.Length + 3 + $arguments.Length
                })
        }
    }

    $worst = 0
    $failure = 0
    $index = 0
    while ($index -lt $work.Count) {
        $batch = @()
        while ($batch.Count -lt [Math]::Max(1, $Lanes) -and $index -lt $work.Count) {
            $item = $work[$index]
            if ($item.Length -gt $worst) { $worst = $item.Length }
            $batch += Start-Rustfmt -Exe $rustfmt -Arguments $item.Arguments
            $index++
        }
        foreach ($running in $batch) {
            $out = $running.Stdout.Result
            $err = $running.Stderr.Result
            $running.Process.WaitForExit()
            if ($out) { [Console]::Out.Write($out) }
            if ($err) { [Console]::Error.Write($err) }
            if ($running.Process.ExitCode -ne 0) { $failure = $running.Process.ExitCode }
            $running.Process.Dispose()
        }
    }

    Write-Verbose "$($targets.Count) targets, $($work.Count) rustfmt invocation(s) $Lanes at a time, longest command line $worst characters"
    $failure
}

# Falsify both halves against workspaces this builds itself. Hermetic, no
# network. The first assertion is the one that matters: a synthetic workspace
# big enough to break `cargo fmt --all` has to actually break it, or the
# second assertion is a fix for nothing.
function Invoke-SelfTest {
    $root = Join-Path ([System.IO.Path]::GetTempPath()) "fmt-selftest-$PID"
    if (Test-Path $root) { Remove-Item -Recurse -Force $root }
    $hostExe = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
    $utf8 = New-Object System.Text.UTF8Encoding $false
    $failures = @()

    # Unformatted on purpose, so rustfmt names every file it reaches and a run
    # that reached fewer than it should cannot look like one that did.
    $unformatted = "pub fn f( ) ->u32{let  x=1;x}`n"

    function New-Workspace {
        param([string]$Base, [int]$Tests, [int]$Pad)
        $dir = Join-Path $Base ('p' * $Pad)
        New-Item -ItemType Directory -Force -Path (Join-Path $dir 'src') | Out-Null
        New-Item -ItemType Directory -Force -Path (Join-Path $dir 'tests') | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $dir 'Cargo.toml'),
            "[package]`nname = `"selftest`"`nversion = `"0.0.0`"`nedition = `"2021`"`n`n[workspace]`n", $utf8)
        [System.IO.File]::WriteAllText((Join-Path $dir 'src/lib.rs'), $unformatted, $utf8)
        for ($i = 0; $i -lt $Tests; $i++) {
            $name = 'a_synthetic_integration_target_{0:d4}.rs' -f $i
            [System.IO.File]::WriteAllText((Join-Path $dir "tests/$name"), $unformatted, $utf8)
        }
        $dir
    }

    $runScript = {
        param([string]$Manifest, [int]$Limit)
        $ErrorActionPreference = 'Continue'
        , @(& $hostExe -NoLogo -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath `
                -Check -ManifestPath $Manifest -Budget $Limit 2>&1 | ForEach-Object { "$_" })
    }

    # The whole point of the second half is a `cargo fmt --all` that fails, so
    # the oracle is captured where stderr is expected rather than fatal.
    $runCargo = {
        param([string]$Dir)
        Push-Location $Dir
        $ErrorActionPreference = 'Continue'
        try {
            $out = @(& cargo fmt --all --check 2>&1 | ForEach-Object { "$_" })
            [PSCustomObject]@{ Output = $out; Code = $LASTEXITCODE }
        }
        finally { Pop-Location }
    }

    # rustfmt heads each file it would change with "Diff in <path>:<line>:".
    $filesNamed = {
        param($Lines)
        $set = [System.Collections.Generic.HashSet[string]]::new()
        foreach ($line in $Lines) {
            if ("$line" -match '^Diff in (.+):\d+:\s*$') {
                [void]$set.Add((Get-ComparablePath $Matches[1]))
            }
        }
        , $set
    }

    try {
        # --- One: the two reach the same files, at a size both can run.
        $small = New-Workspace -Base $root -Tests 40 -Pad 12
        $fromCargo = & $filesNamed (& $runCargo $small).Output
        # A tiny budget, so this is compared against a run that really chunked.
        $fromScript = & $filesNamed (& $runScript (Join-Path $small 'Cargo.toml') 2000)

        if ($fromCargo.Count -ne 41) {
            $failures += "the oracle named $($fromCargo.Count) files of 41, so the comparison below is not over what it claims"
        }
        if (-not $fromCargo.SetEquals($fromScript)) {
            $onlyCargo = @($fromCargo) | Where-Object { -not $fromScript.Contains($_) }
            $onlyScript = @($fromScript) | Where-Object { -not $fromCargo.Contains($_) }
            $failures += "the file sets differ: $($onlyCargo.Count) only cargo fmt --all, $($onlyScript.Count) only this script"
        }

        # --- Two: at a size that breaks `cargo fmt --all`, this survives.
        $big = New-Workspace -Base (Join-Path $root 'big') -Tests 220 -Pad 110
        $cargo = & $runCargo $big

        # The message is localised; `os error 206` is not.
        $tooLong = @($cargo.Output | Where-Object { $_ -match 'os error 206' }).Count -gt 0
        if ($cargo.Code -eq 0 -or -not $tooLong) {
            $failures += "the synthetic workspace did not break cargo fmt --all (exit $($cargo.Code)), so nothing below is evidence"
        }

        $reached = (& $filesNamed (& $runScript (Join-Path $big 'Cargo.toml') 30000)).Count
        if ($reached -ne 221) {
            $failures += "this script reached $reached of 221 files where cargo fmt --all reached none"
        }
    }
    finally {
        Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
    }

    if ($failures.Count -gt 0) {
        foreach ($failure in $failures) { Write-Host "FAIL: $failure" -ForegroundColor Red }
        exit 1
    }
    Write-Host 'format_workspace self-test: the file sets match, and the size that breaks cargo fmt --all does not break this.' -ForegroundColor Green
    exit 0
}

if ($SelfTest) { Invoke-SelfTest }

if (-not $ManifestPath) {
    $ManifestPath = Join-Path (Split-Path -Parent $PSScriptRoot) 'Cargo.toml'
}
$ManifestPath = [System.IO.Path]::GetFullPath($ManifestPath)

if ($ListTargets) {
    Sort-Ordinal ([string[]]@((Get-Targets -Manifest $ManifestPath).Keys)) | ForEach-Object { $_ }
    exit 0
}

# ⚠ Only the read-only half runs concurrently. Two roots can reach one
# module -- a `mod` two integration tests share, a file a lib and a bin both
# declare -- and `cargo fmt --all` formats such a file twice today, serially,
# inside one process. Twice at once is a truncate racing a write.
if ($Concurrency -le 0) {
    $Concurrency = if ($Check) { [Math]::Min([Environment]::ProcessorCount, 8) } else { 1 }
}

exit (Invoke-Format -Manifest $ManifestPath -Limit $Budget -Lanes $Concurrency -CheckOnly:$Check -Extra $RustfmtArgs)
