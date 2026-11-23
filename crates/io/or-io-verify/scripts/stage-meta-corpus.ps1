<#
.SYNOPSIS
    Stage a binary-metadata test corpus from a real GTA V install.

.DESCRIPTION
    WS7's acceptance criteria require a corpus of real .ymt / .ymf binaries
    spanning PSO, RBM and RBF0. Game files are copyrighted and are never
    committed, so this script pulls them out of the user's own install into
    the sibling corpus.

    Drives `openrage-cli serve` over its NDJSON protocol (see
    docs/reference/CLI_Server_Protocol.md) so the install is mounted once for the whole
    batch rather than per file.

    A file's container format is only knowable from its leading magic, which
    means every candidate has to be extracted before it can be classified.
    Extraction is cheap (the whole ~6000-file set takes a few seconds), so the
    script stages everything to a scratch directory, censuses the magics, then
    copies a balanced, deterministic subset into the corpus.

    On a GTA V Legacy install the split is roughly:
      PSIN (PSO)  ~3500   most .ymt/.ymf/.pso
      RSC7 (RBM)  ~2300   ped variation .ymt (mp_*_freemode_*.ymt)
      RBF0 (RBF)   ~200   creature metadata .ymt

.PARAMETER Game
    GTA V install root. Defaults to $env:OPENRAGE_GTA5_DIR, then to the app's
    selected game from prefs.json (whatever `serve` resolves on its own).

.PARAMETER OutDir
    Destination. Defaults to the sibling openrage-corpus/or-meta/binary.

.PARAMETER PerFormat
    How many files to keep per container format. Default 120 (360 total),
    comfortably above WS7's "at least 20" bar while keeping the suite quick.

.PARAMETER All
    Keep every candidate instead of sampling. Useful for a full-corpus sweep
    before claiming a format is done.

.EXAMPLE
    ./stage-meta-corpus.ps1
    ./stage-meta-corpus.ps1 -Game "D:\Games\Rockstar Games\Grand Theft Auto V" -All
#>
[CmdletBinding()]
param(
    [string]$Game = $env:OPENRAGE_GTA5_DIR,
    [string]$OutDir,
    [int]$PerFormat = 120,
    [switch]$All
)

$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$crateRoot = Split-Path -Parent $scriptDir

# Walk up to the manifest that declares the workspace, rather than counting
# directories. A fixed number of `..` is right until a crate moves between
# layer directories, and then it silently resolves to `crates/` and every path
# built from it is wrong.
$workspaceRoot = $crateRoot
while ($workspaceRoot -and -not (
        (Test-Path (Join-Path $workspaceRoot 'Cargo.toml')) -and
        (Select-String -LiteralPath (Join-Path $workspaceRoot 'Cargo.toml') -Pattern '^\[workspace\]' -Quiet)
    )) {
    $workspaceRoot = Split-Path -Parent $workspaceRoot
}
if (-not $workspaceRoot) {
    throw "could not find the workspace root above $crateRoot"
}

# ⚠ Not this crate's `tests/fixtures/binary`: anything staged under
# `tests/fixtures/` here shadows the shared corpus for every format.
if (-not $OutDir) {
    $corpus = $workspaceRoot
    while ($corpus -and -not (Test-Path (Join-Path (Split-Path -Parent $corpus) 'openrage-corpus'))) {
        $corpus = Split-Path -Parent $corpus
    }
    if (-not $corpus) {
        throw "no openrage-corpus beside $workspaceRoot or any ancestor; pass -OutDir"
    }
    $OutDir = Join-Path (Split-Path -Parent $corpus) 'openrage-corpus/or-meta/binary'
}

$cli = Join-Path $workspaceRoot 'target/release/openrage-cli.exe'
if (-not (Test-Path $cli)) {
    $cli = Join-Path $workspaceRoot 'target/debug/openrage-cli.exe'
}
if (-not (Test-Path $cli)) {
    throw "openrage-cli not built. Run: cargo build --release --bin openrage-cli"
}

$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("openrage-meta-corpus-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force -Path $scratch | Out-Null
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Write-Host "Staging binary metadata corpus into $OutDir" -ForegroundColor Cyan

# --- spawn the server -------------------------------------------------------

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

    # .ymt / .ymf are the binary metadata containers; .pso shows up rarely as a
    # loose extension. XML .meta files are covered by the existing XML tests.
    $hits = (Invoke-Route @{ id = 3; cmd = 'search'; ext = @('ymt', 'ymf', 'pso') }).hits
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

# --- classify by leading magic ---------------------------------------------

function Get-Magic([string]$Path) {
    $fs = [System.IO.File]::OpenRead($Path)
    try {
        $h = New-Object byte[] 4
        $n = $fs.Read($h, 0, 4)
    }
    finally { $fs.Dispose() }
    if ($n -lt 4) { return 'other' }
    switch ([System.Text.Encoding]::ASCII.GetString($h)) {
        'PSIN' { 'pso' }
        'RSC7' { 'rbm' }
        'RBF0' { 'rbf' }
        default { 'other' }
    }
}

$buckets = @{ pso = @(); rbm = @(); rbf = @(); other = @() }
Get-ChildItem -Path $scratch -Recurse -File | ForEach-Object {
    $buckets[(Get-Magic $_.FullName)] += , $_
}

# --- select a balanced, deterministic subset --------------------------------
# Within a format, keep one file per distinct leaf name (so we get schema
# variety rather than 300 per-DLC copies of the same type), then take the
# LARGEST of those.
#
# Size is the best cheap proxy for schema coverage, and picking alphabetically
# instead produced a 360-file sample that hit zero reader gaps while the full
# 6071-file corpus hit 19,063 — a sample that cannot regress is worthless.

function Select-Sample($Files, [int]$Take) {
    if ($All) { return $Files }
    # Largest file wins for a given leaf name.
    $byLeaf = @{}
    foreach ($f in $Files) {
        $leaf = $f.Name.ToLowerInvariant()
        if (-not $byLeaf.ContainsKey($leaf) -or $f.Length -gt $byLeaf[$leaf].Length) {
            $byLeaf[$leaf] = $f
        }
    }
    # Descending size, then name, so the selection is reproducible.
    $ranked = @($byLeaf.Values |
        Sort-Object -Property @{Expression = 'Length'; Descending = $true }, 'Name')
    if ($ranked.Count -gt $Take) { $ranked = $ranked[0..($Take - 1)] }
    return $ranked
}

Get-ChildItem -Path $OutDir -File -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -ne '.gitkeep' } | Remove-Item -Force

$staged = 0
foreach ($fmt in @('pso', 'rbm', 'rbf')) {
    $sample = Select-Sample $buckets[$fmt] $PerFormat
    foreach ($f in $sample) {
        # Prefix with the format and the source device so same-named files from
        # different packs don't collide in the flat fixtures directory.
        $rel = $f.FullName.Substring($scratch.Length).TrimStart('\', '/')
        $parts = $rel -split '[\\/]'
        $device = if ($parts.Count -gt 1) { $parts[0] } else { 'root' }
        $dest = Join-Path $OutDir ("{0}__{1}__{2}" -f $fmt, $device, $f.Name)
        $n = 1
        while (Test-Path $dest) {
            $dest = Join-Path $OutDir ("{0}__{1}__{2}__{3}" -f $fmt, $device, $n, $f.Name)
            $n++
        }
        Copy-Item -LiteralPath $f.FullName -Destination $dest
        $staged++
    }
    Write-Host ("  {0}: {1} of {2} available" -f $fmt.ToUpperInvariant(), $sample.Count, $buckets[$fmt].Count)
}

Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "Corpus staged: $staged files in $OutDir" -ForegroundColor Green
if ($buckets['other'].Count -gt 0) {
    Write-Host "  ($($buckets['other'].Count) candidates had an unrecognised magic and were skipped)" -ForegroundColor DarkYellow
}
Write-Host ""
Write-Host "Next: cargo test -p or-io-verify --test meta -- meta_census:: --ignored --nocapture" -ForegroundColor Cyan
