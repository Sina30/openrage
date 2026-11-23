$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot

# ⚠ Not `$repoRoot/.git/hooks`. In a `git worktree` — which is every agent
# lane — `.git` is a *file* pointing at the real directory, so that path does
# not exist and the installer threw. Ask git, which resolves it to the common
# directory: one hook for the repository, shared by every worktree of it.
$hooksDir = & git -C $repoRoot rev-parse --path-format=absolute --git-path hooks
if (-not $hooksDir) { throw "git could not say where hooks live; is this a checkout?" }
$sourceHook = Join-Path $repoRoot ".githooks/pre-commit"
$targetHook = Join-Path $hooksDir "pre-commit"

if (-not (Test-Path $sourceHook)) {
    throw "Hook template not found: $sourceHook"
}

New-Item -ItemType Directory -Force -Path $hooksDir | Out-Null
Copy-Item -Force -Path $sourceHook -Destination $targetHook

Write-Host "Installed pre-commit hook to $targetHook"
Write-Host "Commits will now run the chunked formatter, the encoding lint over the staged"
Write-Host "files, and the provenance lint -- under ten seconds, nothing compiled."
Write-Host "'just ready' adds clippy and the tests before a merge."
