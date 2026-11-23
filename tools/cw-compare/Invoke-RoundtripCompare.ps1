#Requires -Version 5.1
<#
.SYNOPSIS
  For each YDR/YDD/YFT/YTD/YPT/YBN/YLD/YCD/YMT: re-encode with openrage-cli, then cw-compare (CodeWalker XML) against the original.

.PARAMETER Resource
  Optional. One or more explicit file paths (repeat -Resource for multiple). Implies directory scan is skipped.

.PARAMETER ResourcesPipe
  Optional. Several paths separated by | (pipe).

.PARAMETER JustTail
  Optional. Used by `just cw-roundtrip` only: one argument string routed as extension list (ytd,ydr,ydd,yft,ypt,ybn,yld), pipe-separated paths, or a single file path. Prefer -Resource / -ResourcesPipe / -Extension when calling the script directly.

.PARAMETER IncludeHtmlReports
  If set, writes .html reports alongside .json. Omit for faster batch runs (JSON only).

.PARAMETER Extension
  Optional (directory mode only). One or more extensions to include: ydr, ydd, yft, ytd, ypt, ybn, yld (with or without leading dot).
  Examples: -Extension ytd | -Extension ytd,ydr | -Extension @('ypt')
  If omitted, all of them are scanned.

.NOTES
  Requires `convert` binary with matching format support (YDR/YDD/YFT/YTD/YPT/YBN).
#>
param(
    [string] $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path,
    [string] $SearchPath = "",
    [string] $OutputDir = "",
    [string] $Allowlist = "",
    [double] $Epsilon = 1e-5,
    [string[]] $Extension = @(),
    [string[]] $Resource = @(),
    [string] $ResourcesPipe = "",
    [string] $JustTail = "",
    [switch] $IncludeHtmlReports
)

$ErrorActionPreference = "Stop"

# Avoid huge or_io tracing from `openrage-cli convert`
if (-not $env:RUST_LOG) {
    $env:RUST_LOG = "error"
}

if (-not $SearchPath) {
    $SearchPath = Join-Path $RepoRoot "crates\io\or-io-verify\tests\fixtures"
}
if (-not $OutputDir) {
    $OutputDir = Join-Path $RepoRoot "target\cw-compare-batch"
}
if (-not $Allowlist) {
    $Allowlist = Join-Path $PSScriptRoot "cw-compare-allowlist.json"
}

# Single-string entry point for `just cw-roundtrip <one-arg>` (see repo justfile).
if ($JustTail -match '\S') {
    $jt = $JustTail.Trim()
    if ($jt.Contains('|')) {
        $ResourcesPipe = $jt
        $Resource = @()
        $Extension = @()
    }
    elseif ([regex]::IsMatch($jt, '^\s*\.?(ydr|ydd|yft|ytd|ypt|ybn|yld)(\s*,\s*\.?(ydr|ydd|yft|ytd|ypt|ybn|yld))*\s*$', [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)) {
        $Extension = @($jt -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })
        $Resource = @()
        $ResourcesPipe = ""
    }
    else {
        $Resource = @($jt)
        $ResourcesPipe = ""
        $Extension = @()
    }
}

$explicitMode = ($Resource.Count -gt 0) -or (-not [string]::IsNullOrWhiteSpace($ResourcesPipe))

if (-not $explicitMode) {
    $SearchPath = Resolve-Path $SearchPath
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$allowed = @(".ydr", ".ydd", ".yft", ".ytd", ".ypt", ".ybn", ".yld", ".ycd", ".ymt", ".ymf", ".pso")
$normExts = @()
foreach ($ex in $Extension) {
    foreach ($part in ($ex -split ",")) {
        $p = $part.Trim()
        if (-not $p) { continue }
        if (-not $p.StartsWith(".")) { $p = "." + $p }
        $p = $p.ToLowerInvariant()
        if ($allowed -notcontains $p) {
            Write-Error "Extension '$p' is not supported for batch convert. Use one of: ydr, ydd, yft, ytd, ypt, ybn, yld, ycd, ymt, ymf, pso"
        }
        if ($normExts -notcontains $p) {
            $normExts += $p
        }
    }
}

$includePatterns = if ($normExts.Count -gt 0) {
    $normExts | ForEach-Object { "*$_" }
} else {
    @("*.ydr", "*.ydd", "*.yft", "*.ytd", "*.ypt", "*.ybn", "*.yld", "*.ycd", "*.ymt", "*.ymf", "*.pso")
}

$cwProject = Join-Path $RepoRoot "tools\cw-compare\CwCompare.csproj"
if (-not (Test-Path $cwProject)) {
    Write-Error "cw-compare project not found: $cwProject"
}

Write-Host "Repo:        $RepoRoot"
if ($explicitMode) {
    Write-Host "Mode:        explicit file(s)"
} else {
    Write-Host "Search:      $SearchPath"
}
Write-Host "Output:      $OutputDir"
Write-Host "Allowlist:   $Allowlist"
if (-not $explicitMode) {
    if ($normExts.Count -gt 0) {
        Write-Host "Extensions:  $($normExts -join ', ') only"
    } else {
        Write-Host "Extensions:  .ydr, .ydd, .yft, .ytd, .ypt, .ybn, .yld, .ycd, .ymt, .ymf, .pso (all)"
    }
}
Write-Host ""

$files = @()
if ($explicitMode) {
    $paths = New-Object System.Collections.Generic.List[string]
    foreach ($r in $Resource) {
        foreach ($part in ($r -split ",")) {
            $p = $part.Trim()
            if (-not $p) { continue }
            $paths.Add($p) | Out-Null
        }
    }
    if (-not [string]::IsNullOrWhiteSpace($ResourcesPipe)) {
        foreach ($seg in ($ResourcesPipe -split '\|')) {
            $p = $seg.Trim()
            if ($p) { $paths.Add($p) | Out-Null }
        }
    }
    $seen = @{}
    foreach ($p in $paths) {
        try {
            $rp = Resolve-Path -LiteralPath $p -ErrorAction Stop
        } catch {
            Write-Error "Resource path not found: $p"
        }
        $key = $rp.Path.ToLowerInvariant()
        if ($seen.ContainsKey($key)) { continue }
        $seen[$key] = $true
        $item = Get-Item -LiteralPath $rp.Path
        if (-not $item.PSIsContainer) {
            $ext = $item.Extension.ToLowerInvariant()
            if ($allowed -notcontains $ext) {
                Write-Error "Unsupported extension for batch convert: $ext ($($item.Name)). Use ydr, ydd, yft, ytd, ypt, ybn, yld, ycd, ymt, ymf, or pso."
            }
            $files += $item
        }
    }
} else {
    $files = @(
        Get-ChildItem -Path $SearchPath -Recurse -File -Include $includePatterns -ErrorAction SilentlyContinue
    )
}

if ($files.Count -eq 0) {
    if ($explicitMode) {
        Write-Warning "No valid resource files in -Resource / -ResourcesPipe."
    } else {
        Write-Warning "No matching files found under $SearchPath (patterns: $($includePatterns -join ', '))"
    }
    exit 0
}

$ok = 0
$fail = 0
$rows = New-Object System.Collections.Generic.List[object]

Push-Location $RepoRoot
try {
    Write-Host "Warmup: building cw-compare + openrage-cli (Release, once)..." -ForegroundColor DarkGray
    & dotnet build $cwProject -c Release --nologo -v q
    if ($LASTEXITCODE -ne 0) {
        throw "dotnet build cw-compare failed (exit $LASTEXITCODE)"
    }
    $cwDll = Join-Path $RepoRoot "tools\cw-compare\bin\Release\net8.0\cw-compare.dll"
    if (-not (Test-Path -LiteralPath $cwDll)) {
        throw "cw-compare.dll not found: $cwDll"
    }

    & cargo build --release --bin openrage-cli -q
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build --release --bin openrage-cli failed (exit $LASTEXITCODE)"
    }
    $convertExe = Join-Path $RepoRoot "target\release\openrage-cli.exe"
    if (-not (Test-Path -LiteralPath $convertExe)) {
        throw "openrage-cli.exe not found: $convertExe"
    }

    $batchSw = [Diagnostics.Stopwatch]::StartNew()

    foreach ($f in $files) {
        $name = $f.Name
        $converted = Join-Path $OutputDir ($f.BaseName + "_converted" + $f.Extension)
        $reportJson = Join-Path $OutputDir ($f.BaseName + "_report.json")
        $reportHtml = Join-Path $OutputDir ($f.BaseName + "_report.html")

        Write-Host "=== $name ===" -ForegroundColor Cyan

        # Metadata containers are not resources, so they go through
        # `meta rewrite` rather than `convert`. Both read a file and write it
        # back; only the subcommand differs.
        $isMeta = @(".ymt", ".ymf", ".pso") -contains $f.Extension.ToLowerInvariant()

        $sw = [Diagnostics.Stopwatch]::StartNew()
        if ($isMeta) {
            & $convertExe meta rewrite $f.FullName $converted
        }
        else {
            & $convertExe convert $f.FullName $converted
        }
        $sw.Stop()
        $convertMs = $sw.ElapsedMilliseconds

        if ($LASTEXITCODE -ne 0) {
            Write-Host "  convert FAILED (${convertMs}ms)" -ForegroundColor Red
            $fail++
            $rows.Add([pscustomobject]@{ File = $name; Step = "convert"; Ok = $false; ConvertMs = $convertMs; CompareMs = $null })
            continue
        }

        $compareArgs = @(
            $cwDll,
            "compare",
            $f.FullName,
            $converted,
            "--epsilon", $Epsilon.ToString([System.Globalization.CultureInfo]::InvariantCulture),
            "--out", $reportJson
        )
        if ($IncludeHtmlReports) {
            $compareArgs += @("--html", $reportHtml)
        }
        if (Test-Path $Allowlist) {
            $compareArgs += @("--allowlist", $Allowlist)
        }

        $sw.Restart()
        & dotnet @compareArgs
        $sw.Stop()
        $compareMs = $sw.ElapsedMilliseconds
        $code = $LASTEXITCODE

        if ($code -eq 0) {
            Write-Host "  cw-compare OK  (convert: ${convertMs}ms, compare: ${compareMs}ms)" -ForegroundColor Green
            $ok++
            $rows.Add([pscustomobject]@{ File = $name; Step = "compare"; Ok = $true; ConvertMs = $convertMs; CompareMs = $compareMs })
        }
        else {
            Write-Host "  cw-compare FAIL (exit $code, convert: ${convertMs}ms, compare: ${compareMs}ms) - see $reportJson" -ForegroundColor Yellow
            $fail++
            $rows.Add([pscustomobject]@{ File = $name; Step = "compare"; Ok = $false; ConvertMs = $convertMs; CompareMs = $compareMs })
        }
    }

    $batchSw.Stop()
}
finally {
    Pop-Location
}

Write-Host ""
$summaryColor = if ($fail -eq 0) { "Green" } else { "Yellow" }
$batchSecs = [math]::Round($batchSw.Elapsed.TotalSeconds, 1)
Write-Host "Summary: $ok passed, $fail failed out of $($files.Count) files (${batchSecs}s)" -ForegroundColor $summaryColor
$rows | Format-Table File, Step, Ok, ConvertMs, CompareMs -AutoSize

if ($fail -gt 0) {
    exit 1
}
