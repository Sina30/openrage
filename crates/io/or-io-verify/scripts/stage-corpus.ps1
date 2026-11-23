<#
.SYNOPSIS
    Stage a test corpus of one or more extensions from a real GTA V install.

.DESCRIPTION
    Defaults to the three animation-support formats beside .ycd — expression
    dictionaries, frame-filter dictionaries and the pose-matcher database —
    and takes any list of extensions instead.

    Game files are copyrighted and never committed. The default destination is
    the sibling `openrage-corpus/or-asset` directory that `or_test_support`
    finds without configuration; pass -OutDir to stage in-tree instead.

    Drives `openrage-cli serve` over its NDJSON protocol (see
    docs/reference/CLI_Server_Protocol.md) so the install is mounted once for the batch.

    A whole-population run matters more for the small formats than it does
    for the large ones: a retail Legacy install ships 4 `.yfd` in total, so
    "some of them" and "all of them" are the same command.

.PARAMETER Game
    GTA V install root. Defaults to $env:OPENRAGE_GTA5_DIR, then to whatever
    `serve` resolves on its own (the app's selected game from prefs.json).

.PARAMETER OutDir
    Destination. Defaults to the sibling openrage-corpus/or-asset directory.

.PARAMETER Extensions
    Which extensions to stage. Defaults to yed, yfd and ypdb.

.PARAMETER Take
    How many files of each extension to keep. Default 0, meaning all of them —
    the whole population is 296 files and a few megabytes.

.EXAMPLE
    ./stage-corpus.ps1
    ./stage-corpus.ps1 -Extensions fxc -Game "D:\Games\Rockstar Games\Grand Theft Auto V"
#>
[CmdletBinding()]
param(
    [string]$Game = $env:OPENRAGE_GTA5_DIR,
    [string]$OutDir,
    [int]$Take = 0,
    [string[]]$Extensions = @('yed', 'yfd', 'ypdb')
)

$ErrorActionPreference = 'Stop'

$extensions = $Extensions

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$crateRoot = Split-Path -Parent $scriptDir
# crates/<layer>/<crate>/scripts -> the workspace root is three levels above the crate.
$workspaceRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $crateRoot))

# The sibling corpus is found by walking the workspace root's ancestors, not
# only its parent: a git worktree lives one level deeper than the checkout it
# came from, so the parent-only form staged in-tree from a lane and shadowed
# the shared corpus for every other suite.
if (-not $OutDir) {
    $probe = $workspaceRoot
    while ($probe -and -not $OutDir) {
        $probe = Split-Path -Parent $probe
        if (-not $probe) { break }
        $sibling = Join-Path $probe 'openrage-corpus/or-asset'
        if (Test-Path $sibling) { $OutDir = $sibling }
    }
    if (-not $OutDir) { $OutDir = Join-Path $workspaceRoot 'crates/io/or-io-verify/tests/fixtures' }
}

$cli = Join-Path $workspaceRoot 'target/release/openrage-cli.exe'
if (-not (Test-Path $cli)) {
    $cli = Join-Path $workspaceRoot 'target/debug/openrage-cli.exe'
}
if (-not (Test-Path $cli)) {
    throw "openrage-cli not built. Run: cargo build --release --bin openrage-cli"
}

$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("openrage-corpus-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Write-Host "Staging $($extensions -join '/') corpus into $OutDir" -ForegroundColor Cyan

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $cli
$psi.Arguments = 'serve'
$psi.RedirectStandardInput = $true
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.UseShellExecute = $false
$psi.WorkingDirectory = $workspaceRoot

$proc = [System.Diagnostics.Process]::Start($psi)
# Drain stderr on a background task so the pipe buffer never fills and blocks.
$errTask = $proc.StandardError.ReadToEndAsync()

function Invoke-Route([hashtable]$Request) {
    $json = $Request | ConvertTo-Json -Compress -Depth 10
    $proc.StandardInput.WriteLine($json)
    $proc.StandardInput.Flush()
    $line = $proc.StandardOutput.ReadLine()
    if ($null -eq $line) { throw "serve closed the stream (cmd=$($Request.cmd))" }
    $resp = $line | ConvertFrom-Json
    if (-not $resp.ok) { throw "$($Request.cmd) failed: $($resp.error)" }
    return $resp
}

try {
    Invoke-Route @{ id = 1; cmd = 'ping' } | Out-Null

    $mount = @{ id = 2; cmd = 'mount' }
    if ($Game) { $mount.game = $Game }
    $m = Invoke-Route $mount
    Write-Host "  mounted $($m.gameRoot) [$($m.edition)] - $($m.packs) packs" -ForegroundColor DarkGray

    $hits = (Invoke-Route @{ id = 3; cmd = 'search'; ext = $extensions }).hits
    $paths = @($hits | ForEach-Object { $_.path })
    Write-Host "  extracting $($paths.Count) candidates to scratch..." -ForegroundColor DarkGray

    $written = 0
    $failed = 0
    # Chunked so a request line stays well under the protocol's 16 MiB cap.
    for ($i = 0; $i -lt $paths.Count; $i += 500) {
        $end = [Math]::Min($i + 499, $paths.Count - 1)
        $r = Invoke-Route @{ id = 100 + $i; cmd = 'extract-many'; paths = $paths[$i..$end]; outDir = $scratch }
        $written += $r.written.Count
        $failed += $r.errors.Count
    }
    Write-Host "  extracted $written, failed $failed" -ForegroundColor DarkGray

    Invoke-Route @{ id = 999; cmd = 'shutdown' } | Out-Null
}
finally {
    if (-not $proc.HasExited) {
        try { $proc.StandardInput.Close() } catch { }
        if (-not $proc.WaitForExit(5000)) { $proc.Kill() }
    }
    $errTask.Wait(2000) | Out-Null
}

# A file whose first bytes are not its format's marker is an extraction
# failure or an LFS pointer. Staging one makes a reader test fail for a reason
# that is not the reader.
#
# Three markers, because the formats this stages are not all paged: RSC7 for a
# resource, `rgxe` for a shader effect, and the single byte 0x1A the engine's
# own deserializer reads as "this stream is binary" for a pose-matcher
# database.

function Test-Staged([System.IO.FileInfo]$File) {
    $fs = [System.IO.File]::OpenRead($File.FullName)
    try {
        $h = New-Object byte[] 4
        $n = $fs.Read($h, 0, 4)
    }
    finally { $fs.Dispose() }
    if ($n -lt 4) { return $false }
    $magic = [System.Text.Encoding]::ASCII.GetString($h)
    switch ($File.Extension.ToLowerInvariant()) {
        '.ypdb' { return $h[0] -eq 0x1A }
        '.fxc' { return $magic -eq 'rgxe' }
        '.cut' { return $magic -eq 'PSIN' }
        # ⚠ `.ymt` is not one format. PSO, RBM and RBF0 all use the extension,
        # so the only honest header check is "one of the three", and anything
        # dispatching on the extension alone is already wrong.
        '.ymt' { return $magic -in @('PSIN', 'RSC7', 'RBF0') }
        default { return $magic -eq 'RSC7' }
    }
}

$all = @(Get-ChildItem -Path $scratch -Recurse -File)
$candidates = @($all | Where-Object { Test-Staged $_ })
$rejected = $all.Count - $candidates.Count

foreach ($ext in $extensions) {
    Get-ChildItem -Path $OutDir -File -Filter "*.$ext" -ErrorAction SilentlyContinue | Remove-Item -Force
}

$staged = @{}
foreach ($ext in $extensions) {
    $ofExt = @($candidates | Where-Object { $_.Extension -ieq ".$ext" } |
        Sort-Object -Property @{Expression = 'Length'; Descending = $true }, 'Name')
    if ($Take -gt 0 -and $ofExt.Count -gt $Take) { $ofExt = $ofExt[0..($Take - 1)] }

    $n = 0
    foreach ($f in $ofExt) {
        # Prefix with the source device so same-named files from different packs
        # do not collide in the flat fixtures directory.
        $rel = $f.FullName.Substring($scratch.Length).TrimStart('\', '/')
        $parts = $rel -split '[\\/]'
        $device = if ($parts.Count -gt 1) { $parts[0] } else { 'root' }
        $dest = Join-Path $OutDir ("{0}__{1}" -f $device, $f.Name)
        $k = 1
        while (Test-Path $dest) {
            $dest = Join-Path $OutDir ("{0}__{1}__{2}" -f $device, $k, $f.Name)
            $k++
        }
        Copy-Item -LiteralPath $f.FullName -Destination $dest
        $n++
    }
    $staged[$ext] = $n
}

Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue

Write-Host ""
foreach ($ext in $extensions) {
    Write-Host ("  .{0,-5} {1}" -f $ext, $staged[$ext]) -ForegroundColor Green
}
Write-Host "Staged into $OutDir" -ForegroundColor Green
if ($rejected -gt 0) {
    Write-Host "  ($rejected extracted files failed the header check and were skipped)" -ForegroundColor DarkYellow
}
Write-Host ""
Write-Host "Next: cargo test -p or-io-verify --test expression_dict -- --nocapture" -ForegroundColor Cyan
