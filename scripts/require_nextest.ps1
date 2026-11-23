# Refuse a run that needs cargo-nextest when it is not installed, and say so.
#
# Without it cargo fails with `error: no such command: nextest`, which reads
# as "my change broke the build" rather than "a tool is missing".

$ErrorActionPreference = "Stop"

if (Get-Command cargo-nextest -ErrorAction SilentlyContinue) {
    exit 0
}

Write-Host ""
Write-Host "cargo-nextest is not installed, and this recipe needs it." -ForegroundColor Red
Write-Host ""
Write-Host "    cargo install cargo-nextest --locked"
Write-Host ""
Write-Host "It is a local dependency only. CI runs cargo test directly and never"
Write-Host "installs it, so a green CI says nothing about whether you have it."
Write-Host ""
Write-Host "To run the workspace tests without it:"
Write-Host ""
Write-Host "    just test-cargo"
Write-Host ""
Write-Host "There is deliberately no such fallback for test-lite. It ends by"
Write-Host "reporting what it did not run, and that listing comes from nextest"
Write-Host "list -- so a cargo-only tier would check less and report the same green."
Write-Host ""

exit 1
