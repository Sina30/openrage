# Repair files that were written as UTF-8 bytes reinterpreted through cp1252.
#
# The damage happens when a tool reads a UTF-8 file as cp1252 and writes it back
# as UTF-8: every multi-byte sequence becomes two or three Latin-1 characters,
# so an em-dash turns into three garbage characters and a box-drawing character
# into three more.
#
# The repair is the inverse. Encode the damaged text back to cp1252 bytes -- if
# the damage is real, those bytes are exactly the original UTF-8 -- then decode
# them as UTF-8. Applied repeatedly, since a file can be damaged more than once.
#
# Only writes when the result is strictly better and still valid: a file whose
# mojibake count does not reach zero, or that fails to encode as cp1252, is
# reported and left alone rather than half-fixed.
#
# Defaults to this workspace; -Path aims it elsewhere. See check_encoding.ps1,
# which takes the same argument and is the one to run first.

[CmdletBinding()]
param(
    [string]$Path
)

$ErrorActionPreference = 'Stop'

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

$mojibake = [regex]'[\u00c2\u00c3\u00e2][\u0080-\u00bf\u2013\u2014\u2018\u2019\u201c\u201d\u20ac\u0153\u017e\u0161]'
$cp1252 = [System.Text.Encoding]::GetEncoding(
    1252,
    [System.Text.EncoderFallback]::ExceptionFallback,
    [System.Text.DecoderFallback]::ExceptionFallback)
$utf8 = New-Object System.Text.UTF8Encoding $false

$files = Get-ChildItem -Path $root -Recurse -File -Include *.md, *.rs, *.toml, *.ps1, *.yml, justfile |
    Where-Object { $_.FullName -notmatch '\\target\\|\\external\\|\\\.git\\' }

$fixed = 0
$skipped = @()

foreach ($file in $files) {
    $bytes = [System.IO.File]::ReadAllBytes($file.FullName)
    if ($bytes.Length -eq 0) { continue }

    $hasBom = $bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF
    $text = [System.Text.Encoding]::UTF8.GetString($bytes).TrimStart([char]0xFEFF)
    $before = $mojibake.Matches($text).Count

    if ($before -eq 0 -and -not $hasBom) { continue }

    $repaired = $text
    for ($pass = 0; $pass -lt 4 -and $mojibake.Matches($repaired).Count -gt 0; $pass++) {
        try {
            $repaired = [System.Text.Encoding]::UTF8.GetString($cp1252.GetBytes($repaired))
        } catch {
            break
        }
    }

    $after = $mojibake.Matches($repaired).Count
    if ($after -gt 0) {
        $skipped += "$($file.FullName.Substring($root.Length + 1)) ($before -> $after)"
        continue
    }

    [System.IO.File]::WriteAllText($file.FullName, $repaired, $utf8)
    $note = if ($before -gt 0) { "$before repaired" } else { 'BOM stripped' }
    Write-Host "  $($file.FullName.Substring($root.Length + 1))  -- $note"
    $fixed++
}

Write-Host ""
Write-Host "fixed $fixed file(s)"
if ($skipped.Count -gt 0) {
    Write-Host "could not fully repair, left alone:"
    $skipped | ForEach-Object { Write-Host "  $_" }
    exit 1
}
