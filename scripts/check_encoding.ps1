# Report files whose bytes decode as UTF-8 but read as mojibake.
#
# The signature is a U+00E2 (a-circumflex) immediately followed by another
# Latin-1 supplement character: that is what a three-byte UTF-8 sequence looks
# like after a round trip through cp1252. Box-drawing characters and em-dashes
# are the usual casualties, since both are three-byte sequences.
#
# Also flags a UTF-8 BOM, which is not damage but is not wanted either.
#
# Defaults to this workspace. Pass -Path to check somewhere else, such as a
# subdirectory.
#
# Pass -Staged to check what is about to be committed and walk nothing. That
# is what the pre-commit hook uses, and the scope is the point as much as the
# speed: a hook asks whether *this commit* introduces damage, which is a
# question about the index rather than about the disk.
#
# Run with -SelfTest to falsify the staged scope against a throwaway
# repository. The direction worth checking is the one that would pass
# silently: a BOM the commit does not contain must *not* fail the run, or
# -Staged is scanning the tree and the scope is decoration.
#
#   ./scripts/check_encoding.ps1
#   ./scripts/check_encoding.ps1 -Path docs
#   ./scripts/check_encoding.ps1 -Staged
#   ./scripts/check_encoding.ps1 -SelfTest

[CmdletBinding()]
param(
    [string]$Path,
    [switch]$Staged,
    [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'

function Invoke-SelfTest {
    $dir = Join-Path $env:TEMP "encoding-selftest-$PID"
    if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
    New-Item -ItemType Directory -Force -Path (Join-Path $dir 'scripts') | Out-Null
    $script = Join-Path $dir 'scripts/check_encoding.ps1'
    Copy-Item -LiteralPath $PSCommandPath -Destination $script
    # The same host this is running under. `pwsh` is not on every machine and
    # Windows PowerShell is what the justfile invokes.
    $host_exe = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName

    # Not `Set-Content -Encoding utf8NoBOM`: that is PowerShell 7, and writing
    # a BOM by accident in the test for BOMs is not a mistake worth risking.
    $utf8 = New-Object System.Text.UTF8Encoding $false
    $write = { param($name, $text) [System.IO.File]::WriteAllText((Join-Path $dir $name), $text, $utf8) }

    & $write 'Cargo.toml' "[workspace]`r`nmembers = []"
    Push-Location $dir
    $failures = @()
    try {
        & git init -q 2>&1 | Out-Null
        & git config user.email 'selftest@example.invalid'
        & git config user.name 'selftest'

        $run = {
            param([switch]$All)
            if ($All) { & $host_exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File $script | Out-Null }
            else { & $host_exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File $script -Staged | Out-Null }
            $LASTEXITCODE
        }

        & $write 'clean.md' 'no damage here'
        & git add clean.md
        if ((& $run) -ne 0) { $failures += 'a clean staged file was reported as damaged' }

        # The falsifying direction. A BOM on disk that the commit does not
        # contain must pass, or -Staged reads the tree and the scope is a lie.
        & $write 'unstaged.md' ([char]0xFEFF + 'bommed but not staged')
        if ((& $run) -ne 0) {
            $failures += 'a BOM in an unstaged file failed the staged check, so the scope is the tree rather than the commit'
        }
        # And the full sweep does see it, or the scope is the only thing working.
        if ((& $run -All) -eq 0) { $failures += 'the full sweep missed a BOM on disk' }

        & git add unstaged.md
        if ((& $run) -eq 0) { $failures += 'a staged BOM was not caught' }
        & git rm --cached -q unstaged.md
        Remove-Item -LiteralPath (Join-Path $dir 'unstaged.md')

        # Mojibake is the other half and is invisible in a diff viewer.
        & $write 'mojibake.md' ([char]0x00e2 + [char]0x0080 + [char]0x0094)
        & git add mojibake.md
        if ((& $run) -eq 0) { $failures += 'staged mojibake was not caught' }
    } finally {
        Pop-Location
        Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
    }

    if ($failures.Count -gt 0) {
        foreach ($f in $failures) { Write-Host "FAIL: $f" }
        return 1
    }
    Write-Host "encoding selftest: the staged scope is the index, and both kinds of damage are caught"
    return 0
}

if ($SelfTest) { exit (Invoke-SelfTest) }

if ($Path) {
    $root = (Resolve-Path $Path).Path
} else {
    $root = $PSScriptRoot
    while ($root) {
        $manifest = Join-Path $root 'Cargo.toml'
        if ((Test-Path $manifest) -and (Select-String -Path $manifest -Pattern '^\[workspace\]' -Quiet)) { break }
        $root = Split-Path $root -Parent
    }
    if (-not $root) { throw 'no workspace root found' }
}

# Extensions worth reading. Kept in one place because the walk and the
# explicit list have to agree, or a hook would pass a file the sweep fails.
$wanted = @('.md', '.rs', '.toml', '.ps1', '.yml')

function Test-Wanted($item) {
    $wanted -contains $item.Extension -or $item.Name -eq 'justfile'
}

if ($Staged) {
    # Not `$staged`: PowerShell variable names are case-insensitive, so that
    # assigns to the `[switch]$Staged` parameter and fails its type.
    $stagedPaths = @(& git -C $root diff --cached --name-only --diff-filter=ACMR)
    $files = @(
        foreach ($given in $stagedPaths) {
            if ([string]::IsNullOrWhiteSpace($given)) { continue }
            $full = Join-Path $root $given
            # A path git lists can still be gone from disk: a rename records
            # both ends, and the hook runs before the commit exists.
            if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { continue }
            $item = Get-Item -LiteralPath $full
            if (Test-Wanted $item) { $item }
        }
    )
} else {
    # Prune rather than filter. `target/` holds hundreds of thousands of files
    # and enumerating it before discarding it was 25 of this sweep's 26
    # seconds, which is most of why the hook that ran it was too slow to keep.
    $pruned = @('target', 'external', '.git', '.claude', 'node_modules')
    $files = @(
        Get-ChildItem -LiteralPath $root -File | Where-Object { Test-Wanted $_ }
        foreach ($dir in (Get-ChildItem -LiteralPath $root -Directory)) {
            if ($pruned -contains $dir.Name) { continue }
            Get-ChildItem -LiteralPath $dir.FullName -Recurse -File -ErrorAction SilentlyContinue |
                Where-Object {
                    (Test-Wanted $_) -and
                    $_.FullName -notmatch '\\target\\|\\external\\|\\\.git\\|\\\.claude\\'
                }
        }
    )
}

$mojibake = [regex]'[\u00c2\u00c3\u00e2][\u0080-\u00bf\u2013\u2014\u2018\u2019\u201c\u201d\u20ac\u0153\u017e\u0161]'
$bad = @()

foreach ($file in $files) {
    $bytes = [System.IO.File]::ReadAllBytes($file.FullName)
    if ($bytes.Length -eq 0) { continue }

    $hasBom = $bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF
    $text = [System.Text.Encoding]::UTF8.GetString($bytes)
    $hits = $mojibake.Matches($text).Count

    if ($hits -gt 0 -or $hasBom) {
        $bad += [pscustomobject]@{
            Path     = $file.FullName.Substring($root.Length + 1)
            Mojibake = $hits
            Bom      = $hasBom
        }
    }
}

if ($bad.Count -eq 0) {
    Write-Host "clean: $($files.Count) files, no mojibake and no BOMs"
    exit 0
}

$bad | Sort-Object Mojibake -Descending | Format-Table -AutoSize
Write-Host "$($bad.Count) file(s) affected out of $($files.Count)"
# A BOM is three bytes and the fix is mechanical, so say it rather than leaving
# whoever hits this to work out that `fix_encoding.ps1` exists.
if ($bad | Where-Object { $_.Bom }) {
    Write-Host "A BOM is removable with: ./scripts/fix_encoding.ps1"
}
exit 1
