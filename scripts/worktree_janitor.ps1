<#
.SYNOPSIS
    Remove finished agent worktrees and their build output, and say how much that freed.

.DESCRIPTION
    Reports by default and deletes only with -Apply, so the list can be read before
    it is acted on.

    A worktree is removed only when all four hold: its branch has landed on the
    reference, it has no uncommitted or untracked changes, nothing has written to
    it recently, and git has not locked it. Anything else is held and listed with
    the reason. The janitor never deletes a branch, and it removes a worktree only
    once its work is on the reference, so what it removes is already on `main`;
    deleting the landed branch afterwards is a separate step.

    ⚠ Landed is not the same as "is an ancestor", and reading it that way is what
    let ~200 GB accumulate. Every pull request in this repository is squash-merged,
    so the commit that lands carries a different parent and a different sha from
    anything on the branch and `git merge-base --is-ancestor` is false forever.
    Measured 2026-09-22: of the thirteen lanes this script held as "not merged",
    the ancestor test recognised none and twelve had a merged pull request.

    So landing is judged twice -- the ancestor test first, then GitHub, which is
    asked whether a pull request recorded this worktree's exact HEAD as its head
    commit, and what became of that pull request.

    ⚠ A merged pull request is not the only way work lands here. GitHub will not
    merge a `CONFLICTING` pull request, resolving a conflict requires a local
    squash, and pushing a resolution to a head branch is forbidden -- so a
    conflicting pull request is landed by local squash onto the reference and
    then *closed*. Measured 2026-09-22: fifteen pull requests landed in one
    afternoon, seven server-side and eight this way. A closed pull request whose
    head sha matches is therefore read alongside the reference's own history,
    which records the landing as a `(#N)` suffix -- see Test-ReferenceRecordsPr.

    ⚠ The sha has to match exactly, and that is the safety property rather than an
    implementation detail. Matching on branch name alone would call a reused name
    landed and delete a lane whose work never went anywhere; requiring the sha
    means one unpushed commit on top holds the lane and says why. Three cheaper
    local signals were measured against the same thirteen lanes and all three were
    rejected -- see Resolve-MergeState.

    Run with -SelfTest to falsify the detection against a throwaway repository:
    hermetic, offline, a couple of seconds, and it asserts both directions.

    Lanes live under one OpenRage-owned parent, so they are enumerated by
    listing a directory rather than by matching names. ⚠ That is the point of
    the parent and not a tidiness preference: lanes used to be named ad hoc
    beside the checkout, so nothing could find them all, and the ones nobody
    recognised are the ones that grew.

    ⚠ A target directory outside that parent is reported, never deleted, and only
    when it carries cargo's own CACHEDIR.TAG signature. The checkout's parent is
    the organisation's folder and holds a dozen unrelated projects, so a
    name-shaped rule over it is one typo away from deleting one of them.
#>
[CmdletBinding()]
param(
    # Delete. Without it nothing is removed and the same report is printed.
    [switch]$Apply,

    # Fail when the lane root reaches this. 0 disables the check.
    [int]$BudgetGiB = 0,

    # A worktree written to more recently than this is treated as in use.
    [int]$IdleMinutes = 60,

    # Landing is judged against this. A branch that has not reached it is held.
    [string]$Reference = '',

    # Where lanes live. The justfile passes this so there is one definition of it;
    # the default is the same path, for running the script directly.
    [string]$LaneRoot = '',

    # Judge landing from the ancestor test alone. Every squash-merged lane is then
    # held, which is the behaviour this switch exists to be able to ask for --
    # and the report says the question went unasked rather than reporting a no.
    [switch]$NoGitHub,

    # Falsify the landing detection and exit. Touches no worktree and no network.
    [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'
$repo = (git rev-parse --show-toplevel 2>$null)
if (-not $repo) { Write-Error 'not inside a git repository'; exit 2 }
$repo = $repo.Replace('/', '\')

# `git rev-parse --show-toplevel` in a worktree names that worktree, not the
# checkout the janitor must never remove.
$mainWorktree = (git worktree list --porcelain | Select-Object -First 1) -replace '^worktree ', ''
$mainWorktree = $mainWorktree.Replace('/', '\').TrimEnd('\')
$here = (Get-Location).Path.TrimEnd('\')

if (-not $Reference) {
    $Reference = if (git rev-parse --verify --quiet origin/main) { 'origin/main' } else { 'main' }
}
$refSha = git rev-parse $Reference 2>$null
if (-not $refSha) { Write-Error "cannot resolve $Reference"; exit 2 }

$orgFolder = Split-Path $mainWorktree -Parent
if (-not $LaneRoot) { $LaneRoot = Join-Path $orgFolder 'openrage-lanes' }
# ⚠ `just`'s `/` operator joins with a forward slash, so the value arrives as
# `C:\org/openrage-lanes`. The prefix test below compares against
# real worktree paths, which are all backslashes -- unnormalised, it matches
# nothing and every lane is reported as misplaced.
$LaneRoot = $LaneRoot.Replace('/', '\').TrimEnd('\')

function Test-UnderLaneRoot {
    param([string]$Path)
    return $Path.TrimEnd('\').StartsWith($LaneRoot + '\', [StringComparison]::OrdinalIgnoreCase)
}

$CARGO_TAG = 'Signature: 8a477f597d28d172789f06886806bc55'

function Get-TreeBytes {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    # robocopy's listing mode is an order of magnitude faster than enumerating in
    # PowerShell, and its summary is localised -- so the label is not parsed. The
    # last three summary rows are directories, files and bytes, in that order.
    $out = robocopy $Path (Join-Path $env:TEMP '__janitor_nonexistent__') /L /S /NJH /NC /NFL /NDL /BYTES /XJ /R:0 /W:0
    $rows = $out | Where-Object { $_ -match '^\s*\S.*:\s+\d+\s+\d+\s+\d+\s+\d+\s+\d+\s+\d+\s*$' }
    if ($rows.Count -ge 3) {
        return [int64]([regex]::Matches($rows[2], '\d+')[0].Value)
    }
    return (Get-ChildItem -LiteralPath $Path -Recurse -File -Force -ErrorAction SilentlyContinue |
            Measure-Object -Property Length -Sum).Sum
}

function Test-Idle {
    param([string]$Path, [int]$Minutes)
    $cut = (Get-Date).AddMinutes(-$Minutes)
    $newest = Get-ChildItem -LiteralPath $Path -Recurse -Depth 3 -Force -ErrorAction SilentlyContinue |
              Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $newest) { return $true }
    return $newest.LastWriteTime -lt $cut
}

# ⚠ A status that could not be taken is not a clean status. `git -C` exits 128
# on a worktree whose gitdir has gone, printing `fatal: not a git repository`,
# and an exit code nobody reads makes that empty output mean "nothing to lose".
function Get-DirtyReason {
    param([string]$Path)
    $out = git -C $Path status --porcelain 2>$null
    if ($LASTEXITCODE -ne 0) { return 'git cannot read a status here' }
    if ($out)                { return 'uncommitted or untracked changes' }
    return ''
}

function Test-CargoTarget {
    param([string]$Path)
    $tag = Join-Path $Path 'CACHEDIR.TAG'
    if (-not (Test-Path -LiteralPath $tag)) { return $false }
    return (Get-Content -LiteralPath $tag -TotalCount 1) -eq $CARGO_TAG
}

# Is this directory cargo's build output?
#
# ⚠ `CACHEDIR.TAG` alone is not enough to ask this, and the reason is already
# written down in this repository: cargo stamps that file *once*, when it
# creates the target directory, and skips it when the directory already
# exists -- so a `target/` anything else made first never gets one and no
# later build adds it. The skip ledger's own lookup was red for a day over
# exactly this.
#
# Measured 2026-09-22: `fmt-command-line`'s `target/` had no `CACHEDIR.TAG`,
# and Get-IgnoredWorkReason reported 218.6 MiB of `.rustc_info.json` and
# `debug/gen-snapshot.exe` as work no branch carried. `.rustc_info.json` and
# `<profile>/.fingerprint` are maintained by every build rather than stamped
# by the first, which is what makes them answerable on a directory somebody
# else created.
#
# Deliberately not folded into Test-CargoTarget: the orphan scan's report says
# it identifies a directory by cargo's signature, and that claim is about a
# directory this script will never delete.
function Test-BuildOutput {
    param([string]$Path)
    if (Test-CargoTarget $Path) { return $true }
    if (Test-Path -LiteralPath (Join-Path $Path '.rustc_info.json')) { return $true }
    foreach ($profile in @('debug', 'release')) {
        if (Test-Path -LiteralPath (Join-Path $Path "$profile\.fingerprint")) { return $true }
    }
    return $false
}

# What does this worktree hold that no branch carries?
#
# ⚠ Removal is safe here because a removed worktree's work is already on the
# reference, so its tracked content survives. That is true of tracked content
# and of nothing else. An ignored file is in no commit and on no remote, so removing
# the worktree destroys it outright -- and Get-DirtyReason cannot see one,
# because `git status --porcelain` omits ignored files by design.
#
# ⚠ Measured 2026-09-22, on two lanes that were live at the time:
# `r33-paths-navmesh` held `docs/extra/YND_File_Structure_Specification.md`
# and `r34-audio` held `docs/extra/AWC_File_Structure_Specification.md`, 9.3
# and 12.8 KiB, each in zero commits on any branch. Both reported a clean
# tree and both sat on a commit the ancestor test called landed, so an hour of
# inactivity was the whole of the protection. `/docs/extra/` is gitignored by
# design and is where this project's own guide sends every format
# specification, which is why the two most exposed lanes were the two format
# workstreams.
#
# Build output is excluded by cargo's own CACHEDIR.TAG rather than by being
# called `target`, and a file the checkout already holds byte-for-byte is a
# staged copy rather than something authored here.
function Get-IgnoredWorkReason {
    param([string]$Path, [string]$CheckoutPath, [int]$MaxFiles = 200)

    $out = git -C $Path status --porcelain --ignored 2>$null
    if ($LASTEXITCODE -ne 0) { return 'git cannot list ignored files here' }

    $files = New-Object System.Collections.Generic.List[object]
    $capped = $false
    foreach ($line in @($out)) {
        if ($line -notmatch '^!!\s+(.+)$') { continue }
        $rel = $Matches[1].Trim().Trim('"').TrimEnd('/')
        if ($rel -eq '.agent-worktree') { continue }

        $full = Join-Path $Path ($rel -replace '/', '\')
        if (-not (Test-Path -LiteralPath $full)) { continue }
        $isDir = Test-Path -LiteralPath $full -PathType Container
        if ($isDir -and (Test-BuildOutput $full)) { continue }

        $found = if ($isDir) {
            @(Get-ChildItem -LiteralPath $full -Recurse -File -Force -ErrorAction SilentlyContinue)
        } else {
            @(Get-Item -LiteralPath $full -Force -ErrorAction SilentlyContinue)
        }
        foreach ($f in $found) {
            if ($files.Count -ge $MaxFiles) { $capped = $true; break }
            $under = $f.FullName.Substring($Path.Length).TrimStart('\')
            $twin = Join-Path $CheckoutPath $under
            if ((Test-Path -LiteralPath $twin -PathType Leaf) -and
                ((Get-Item -LiteralPath $twin).Length -eq $f.Length) -and
                ((Get-FileHash -LiteralPath $twin).Hash -eq (Get-FileHash -LiteralPath $f.FullName).Hash)) { continue }
            [void]$files.Add($f)
        }
        if ($files.Count -ge $MaxFiles) { break }
    }

    if (-not $files.Count) { return '' }
    # ⚠ Bytes below a kibibyte, because a 27-byte specification draft rounding
    # to "0 KiB" reads as holding nothing, which is the opposite of the point.
    $sum = ($files | Measure-Object -Property Length -Sum).Sum
    $size = if ($sum -ge 1MB) { "{0:N1} MiB" -f ($sum / 1MB) }
            elseif ($sum -ge 1KB) { "{0:N1} KiB" -f ($sum / 1KB) }
            else { "$sum B" }
    # ⚠ Both figures are floors once the walk stopped at $MaxFiles, and a floor
    # printed as a total is a measurement that overstates what it looked at.
    $least = if ($capped) { 'at least ' } else { '' }
    $noun = if ($files.Count -eq 1) { 'file' } else { 'files' }
    $named = @($files | Select-Object -First 2 |
               ForEach-Object { $_.FullName.Substring($Path.Length).TrimStart('\') -replace '\\', '/' })
    $rest = $files.Count - $named.Count
    $tail = if ($rest -gt 0) { " (+$least$rest more)" } else { '' }
    return ("holds $least$($files.Count) ignored $noun no branch carries, $least${size}: " + ($named -join ', ') + $tail)
}

# Which pull requests does the reference's own history say it landed?
#
# A squash merge writes `(#N)` at the end of the subject. GitHub's does, and so
# does the local squash a conflicting pull request has to be landed with, so one
# rule covers both. That suffix is the merging process's own marker, written at
# the moment the sha is known and sitting on the reference, where nothing but
# another commit can put it -- unlike a pull-request comment, which anyone can
# write about work that never landed.
#
# Measured 2026-09-22 against this repository: all eight pull requests landed by
# local squash and then closed carry one, and #102 -- closed because it was
# superseded, never landed -- does not.
#
# Cached per reference sha. Thirty worktrees would otherwise walk the whole
# history thirty times to answer the same question.
$script:MarkerCache = @{}
function Test-ReferenceRecordsPr {
    param([string]$RefSha, [int]$Number)
    if (-not $script:MarkerCache.ContainsKey($RefSha)) {
        $seen = New-Object System.Collections.Generic.HashSet[int]
        foreach ($subject in (git log $RefSha --format=%s 2>$null)) {
            if ($subject -match '\(#(\d+)\)\s*$') { [void]$seen.Add([int]$Matches[1]) }
        }
        $script:MarkerCache[$RefSha] = $seen
    }
    return $script:MarkerCache[$RefSha].Contains($Number)
}

# gh's JSON as a flat list of pull requests.
#
# ⚠ `@($json | ConvertFrom-Json)` is the obvious spelling and it is wrong.
# Windows PowerShell hands the whole JSON array back as a single object, so
# wrapping the *pipeline* in `@()` yields a one-element list holding the array
# -- and `.number` on that member-enumerates to every number at once. Measured
# on 5.1.22621, both directions of the same mistake: a branch with two pull
# requests reported `PR #134 111`, and a branch with none reported `PR #`
# moved, a numberless claim about a pull request that does not exist.
# Assigning first and wrapping the variable flattens all three cases.
function ConvertTo-PrList {
    param([string]$Json)
    if (-not $Json) { return @() }
    $parsed = $Json | ConvertFrom-Json
    return @($parsed)
}

# What do this branch's pull requests say about this exact commit?
#
# ⚠ Separate from the query that fetches them so the self-test exercises this
# rather than a stub of it. The sha comparison *is* the safety property, and a
# stub that hands back a status has already made the decision this makes.
function Select-PrState {
    param($Prs, [string]$Sha)
    $real = @($Prs | Where-Object { $null -ne $_.number })
    $mine = @($real | Where-Object { $_.headRefOid -eq $Sha })

    foreach ($p in $mine) { if ($p.state -eq 'MERGED') { return @{ Status = 'merged'; Number = [int]$p.number } } }
    foreach ($p in $mine) { if ($p.state -eq 'CLOSED') { return @{ Status = 'closed'; Number = [int]$p.number } } }
    foreach ($p in $mine) { if ($p.state -eq 'OPEN')   { return @{ Status = 'open';   Number = [int]$p.number } } }
    if ($real.Count) { return @{ Status = 'moved'; Number = [int]$real[0].number } }
    return @{ Status = 'none' }
}

# Has this branch landed on the reference?
#
# ⚠ Answers "no" for every question it cannot settle, and every caller must keep
# it that way. Holding a lane costs a directory nobody reclaimed; removing one
# that still holds unlanded work costs the work.
#
# Three cheaper signals were measured against the thirteen lanes on this machine
# and none of them is what this uses:
#
#   * the remote branch being gone -- pull requests here merge with
#     `--delete-branch`, so it looks decisive. It is not: `agent/r32-world-data`
#     had genuinely unlanded commits and no remote branch either, so this signal
#     alone would have deleted it. Unsafe in the one direction that matters.
#   * `git merge-tree` reporting that merging the branch leaves the reference's
#     tree unchanged. Safe, and it recognised 4 of the 12. Once anything else
#     touches the same lines the three-way merge conflicts and the lane is held.
#   * a synthesized squash commit matched by patch-id (`git cherry`). 9 of 12,
#     and it misses whenever the reference moved under the branch before merge.
#
# $PrLookup is a parameter so the self-test can falsify this without a network.
function Resolve-MergeState {
    param(
        [string]$Head,
        [string]$Branch,
        [string]$RefSha,
        [string]$Reference,
        [scriptblock]$PrLookup
    )
    git merge-base --is-ancestor $Head $RefSha 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) {
        return [pscustomobject]@{ Landed = $true; How = 'ancestor'; Reason = '' }
    }
    if (-not $PrLookup) {
        return [pscustomobject]@{ Landed = $false; How = ''; Reason = "not on $Reference, and GitHub was not asked" }
    }

    $pr = & $PrLookup $Branch $Head
    switch ($pr.Status) {
        'merged' {
            return [pscustomobject]@{ Landed = $true; How = "PR #$($pr.Number)"; Reason = '' }
        }
        'closed' {
            # Landed by local squash and closed, because GitHub will not merge a
            # conflicting pull request. ⚠ Closed proves nothing on its own --
            # #102 was closed because it was superseded -- so the reference has
            # to record the landing too. Both halves, or the lane is held.
            if (Test-ReferenceRecordsPr -RefSha $RefSha -Number $pr.Number) {
                return [pscustomobject]@{ Landed = $true; How = "PR #$($pr.Number) (closed)"; Reason = '' }
            }
            return [pscustomobject]@{ Landed = $false; How = ''; Reason = "PR #$($pr.Number) was closed without merging, and no commit on $Reference records it landing" }
        }
        'open' {
            return [pscustomobject]@{ Landed = $false; How = ''; Reason = "PR #$($pr.Number) has this HEAD and is still open" }
        }
        'moved' {
            # The branch name belongs to a pull request and this commit does
            # not. Either the lane committed on top of what landed, or the name
            # was reused for new work. Both must hold.
            return [pscustomobject]@{ Landed = $false; How = ''; Reason = "PR #$($pr.Number) records a different commit -- this HEAD did not land" }
        }
        'none' {
            return [pscustomobject]@{ Landed = $false; How = ''; Reason = "not on $Reference, and no PR has this HEAD" }
        }
        default {
            return [pscustomobject]@{ Landed = $false; How = ''; Reason = "not on $Reference, and GitHub could not be asked" }
        }
    }
}

$script:GhState = ''
$script:GhAsked = 0

function Initialize-Gh {
    if ($script:GhState) { return }
    if ($NoGitHub) { $script:GhState = 'disabled with -NoGitHub'; return }
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { $script:GhState = 'the gh CLI is not on PATH'; return }
    gh auth status 2>$null 1>$null
    if ($LASTEXITCODE -ne 0) { $script:GhState = 'gh is not authenticated -- run `gh auth login`'; return }
    $script:GhState = 'ready'
}

function Get-PrForBranch {
    param([string]$Branch, [string]$Sha)
    Initialize-Gh
    if ($script:GhState -ne 'ready') { return @{ Status = 'unavailable'; Detail = $script:GhState } }

    # Asked per branch rather than by listing every pull request, because a list
    # takes a --limit and a limit that elides an old pull request reports the
    # same thing as one that never existed.
    #
    # ⚠ Every state, not just merged. A pull request GitHub could not merge is
    # landed locally and closed, so filtering to merged here is the defect this
    # query exists to avoid -- and an open one with this head is the sharpest
    # thing to be able to say no about.
    $script:GhAsked++
    $json = gh pr list --state all --head $Branch --limit 20 --json number,state,headRefOid 2>$null
    if ($LASTEXITCODE -ne 0) {
        $script:GhState = 'the gh query failed -- offline?'
        return @{ Status = 'unavailable'; Detail = $script:GhState }
    }
    return Select-PrState -Prs (ConvertTo-PrList $json) -Sha $Sha
}

function Remove-Tree {
    param([string]$Path)
    $empty = Join-Path $env:TEMP "janitor-empty-$PID"
    New-Item -ItemType Directory -Force -Path $empty | Out-Null
    robocopy $empty $Path /MIR /MT:16 /NFL /NDL /NJH /NJS /NC /NS /NP /R:0 /W:0 | Out-Null
    Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $empty -Recurse -Force -ErrorAction SilentlyContinue
}

# What sits directly under the lane root that is not a live worktree.
function Get-LaneResidue {
    param([string]$Root, [string[]]$Live, [int]$Minutes)
    if (-not (Test-Path -LiteralPath $Root)) { return }
    foreach ($d in (Get-ChildItem -LiteralPath $Root -Force -Directory -ErrorAction SilentlyContinue)) {
        if ($Live -contains $d.FullName.TrimEnd('\')) { continue }
        if (-not (Test-Idle $d.FullName $Minutes)) { continue }
        [pscustomobject]@{ Path = $d.FullName; Kind = 'directory' }
    }
    # ⚠ A plain file is never a lane, so the rule that makes everything here
    # deletable does not reach it: it is reported whatever its age, never removed.
    foreach ($f in (Get-ChildItem -LiteralPath $Root -Force -File -ErrorAction SilentlyContinue)) {
        [pscustomobject]@{ Path = $f.FullName; Kind = 'file'; Bytes = $f.Length }
    }
}

# ---------------------------------------------------------------- self-test --

# One pull request as GitHub reports it. The field names are GitHub's, because
# Select-PrState reads the real payload and this has to be the same shape.
function New-Pr {
    param([int]$Number, [string]$State, [string]$Oid)
    return [pscustomobject]@{ number = $Number; state = $State; headRefOid = $Oid }
}

# ⚠ A lookup that runs the real classifier over injected pull requests, rather
# than a stub that hands back a status. A stub has already decided the thing the
# sha comparison decides, so a table of stubs can pass while the comparison is
# broken -- which is how a check reports green about a path it never ran.
function New-PrLookup {
    param($Records)
    return { param($b, $s) Select-PrState -Prs $Records -Sha $s }.GetNewClosure()
}

# Falsifies Resolve-MergeState in both directions against a repository built
# here, with only the GitHub payload injected. The two recognitions are the
# defects this detection exists for; every other case is a way it could delete
# work that never landed.
function Invoke-SelfTest {
    $dir = Join-Path $env:TEMP "janitor-selftest-$PID"
    if (Test-Path -LiteralPath $dir) { Remove-Tree $dir }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null

    $g = { param($rest) & git -C $dir -c user.name=janitor -c user.email=janitor@localhost @rest 2>$null | Out-Null }
    & $g @('init', '-q', '-b', 'main')
    Set-Content -LiteralPath (Join-Path $dir 'a.txt') -Value 'base'
    & $g @('add', '-A'); & $g @('commit', '-qm', 'M0')

    # A branch squash-merged exactly as GitHub does it.
    & $g @('checkout', '-qb', 'squashed')
    Set-Content -LiteralPath (Join-Path $dir 'b.txt') -Value 'landed'
    & $g @('add', '-A'); & $g @('commit', '-qm', 'F1')
    $squashedHead = (git -C $dir rev-parse squashed).Trim()
    & $g @('checkout', '-q', 'main')
    & $g @('merge', '--squash', 'squashed'); & $g @('commit', '-qm', 'squash merge (#1)')

    # The same branch, carrying one commit that never reached the pull request.
    & $g @('checkout', '-q', 'squashed')
    Set-Content -LiteralPath (Join-Path $dir 'c.txt') -Value 'never pushed'
    & $g @('add', '-A'); & $g @('commit', '-qm', 'F2')
    $aheadHead = (git -C $dir rev-parse squashed).Trim()

    # A branch with genuine unlanded work and no pull request at all.
    & $g @('checkout', '-q', '-b', 'wip', 'main')
    Set-Content -LiteralPath (Join-Path $dir 'd.txt') -Value 'in progress'
    & $g @('add', '-A'); & $g @('commit', '-qm', 'W1')
    $wipHead = (git -C $dir rev-parse wip).Trim()

    # A branch that landed as an ordinary merge commit, so it is an ancestor.
    & $g @('checkout', '-q', '-b', 'ffwd', 'main')
    Set-Content -LiteralPath (Join-Path $dir 'e.txt') -Value 'merged'
    & $g @('add', '-A'); & $g @('commit', '-qm', 'E1')
    $ffwdHead = (git -C $dir rev-parse ffwd).Trim()
    & $g @('checkout', '-q', 'main')
    & $g @('merge', '--no-ff', '-qm', 'merge ffwd', 'ffwd')

    # A branch GitHub could not merge: landed by local squash onto main, its
    # pull request closed rather than merged, and the landing recorded in the
    # subject the squash carries.
    & $g @('checkout', '-q', '-b', 'conflicted', 'main')
    Set-Content -LiteralPath (Join-Path $dir 'f.txt') -Value 'landed by local squash'
    & $g @('add', '-A'); & $g @('commit', '-qm', 'C1')
    $closedHead = (git -C $dir rev-parse conflicted).Trim()
    & $g @('checkout', '-q', 'main')
    & $g @('merge', '--squash', 'conflicted')
    & $g @('commit', '-qm', 'landed locally because GitHub would not merge it (#7)')

    # The same branch, carrying one commit that the pull request never saw.
    & $g @('checkout', '-q', 'conflicted')
    Set-Content -LiteralPath (Join-Path $dir 'g.txt') -Value 'after the landing'
    & $g @('add', '-A'); & $g @('commit', '-qm', 'C2')
    $closedAhead = (git -C $dir rev-parse conflicted).Trim()

    # A branch whose pull request was closed because it was superseded. Its head
    # is this commit and nothing on main records it -- #102's shape, and the
    # reason a closed pull request is not on its own evidence of anything.
    & $g @('checkout', '-q', '-b', 'abandoned', 'main')
    Set-Content -LiteralPath (Join-Path $dir 'h.txt') -Value 'never landed'
    & $g @('add', '-A'); & $g @('commit', '-qm', 'A1')
    $abandonedHead = (git -C $dir rev-parse abandoned).Trim()

    # A branch with an open pull request recording exactly this head.
    & $g @('checkout', '-q', '-b', 'in-review', 'main')
    Set-Content -LiteralPath (Join-Path $dir 'i.txt') -Value 'still in review'
    & $g @('add', '-A'); & $g @('commit', '-qm', 'O1')
    $openHead = (git -C $dir rev-parse in-review).Trim()

    $mainSha = (git -C $dir rev-parse main).Trim()

    # Pull requests as GitHub would report them, put through the real
    # classifier. The sha each one records is the repository's own.
    $prSquashMerged = New-PrLookup @( (New-Pr 1 'MERGED' $squashedHead) )
    $prClosedLanded = New-PrLookup @( (New-Pr 7 'CLOSED' $closedHead) )
    $prClosedNever  = New-PrLookup @( (New-Pr 8 'CLOSED' $abandonedHead) )
    $prStillOpen    = New-PrLookup @( (New-Pr 9 'OPEN'   $openHead) )
    $none           = New-PrLookup @()
    $unavailable    = { param($b, $s) @{ Status = 'unavailable'; Detail = 'offline' } }

    # GitHub is asked only about a real branch name and the real HEAD, so a
    # lookup that ignored its arguments could pass the table below while the
    # caller fed it nonsense. These record what it was handed.
    $recording = { param($b, $s) $script:seenBranch = $b; $script:seenSha = $s; @{ Status = 'merged'; Number = 1 } }

    # ⚠ Every row states the path it must take, and the run asserts each path
    # was reached. A table can lose a row to an edit or a filter and still
    # report every row it ran as green, which is the shape of green that says
    # nothing -- so the count and the coverage are asserted, not printed.
    $cases = @(
        @{ Path = 'ancestor';  Name = 'merge-commit branch is an ancestor';        Head = $ffwdHead;      Branch = 'ffwd';       Pr = $none;           Want = $true;  How = 'ancestor' }
        @{ Path = 'ancestor';  Name = 'ancestor needs no GitHub at all';           Head = $ffwdHead;      Branch = 'ffwd';       Pr = $null;           Want = $true;  How = 'ancestor' }
        @{ Path = 'ancestor';  Name = 'the reference itself has landed';           Head = $mainSha;       Branch = 'main';       Pr = $none;           Want = $true;  How = 'ancestor' }
        @{ Path = 'merged';    Name = 'squash-merged branch is recognised';        Head = $squashedHead;  Branch = 'squashed';   Pr = $prSquashMerged; Want = $true;  How = 'PR #1' }
        @{ Path = 'closed';    Name = 'squashed-and-closed landing is recognised'; Head = $closedHead;    Branch = 'conflicted'; Pr = $prClosedLanded; Want = $true;  How = 'PR #7 (closed)' }
        @{ Path = 'unrecorded';Name = 'a closed PR main never records is refused'; Head = $abandonedHead; Branch = 'abandoned';  Pr = $prClosedNever;  Want = $false; How = '' }
        @{ Path = 'moved';     Name = 'a commit past a closed landing is refused'; Head = $closedAhead;   Branch = 'conflicted'; Pr = $prClosedLanded; Want = $false; How = '' }
        @{ Path = 'moved';     Name = 'a commit past the merged head is refused';  Head = $aheadHead;     Branch = 'squashed';   Pr = $prSquashMerged; Want = $false; How = '' }
        @{ Path = 'open';      Name = 'an open PR with this HEAD is refused';      Head = $openHead;      Branch = 'in-review';  Pr = $prStillOpen;    Want = $false; How = '' }
        @{ Path = 'none';      Name = 'unlanded work with no PR is refused';       Head = $wipHead;       Branch = 'wip';        Pr = $none;           Want = $false; How = '' }
        @{ Path = 'no-lookup'; Name = 'unlanded work is refused with no GitHub';   Head = $wipHead;       Branch = 'wip';        Pr = $null;           Want = $false; How = '' }
        @{ Path = 'offline';   Name = 'an unanswerable question is refused';       Head = $wipHead;       Branch = 'wip';        Pr = $unavailable;    Want = $false; How = '' }
        # ⚠ A reused branch name over a landing that is recorded on main. The
        # marker says #7 landed and the sha says this is not it, and the sha wins.
        @{ Path = 'moved';     Name = 'a reused name over a landing is refused';   Head = $wipHead;       Branch = 'conflicted'; Pr = $prClosedLanded; Want = $false; How = '' }
    )
    $EXPECTED_CASES = 13
    $EXPECTED_TOTAL = 24
    $EXPECTED_PATHS = @('ancestor', 'merged', 'closed', 'unrecorded', 'moved', 'open', 'none', 'no-lookup', 'offline',
                        'ignored-furniture', 'ignored-authored', 'ignored-staged', 'ignored-diverged',
                        'ignored-not-cargo', 'ignored-maintained-marker', 'ignored-capped', 'stray-file')

    Write-Host ''
    Write-Host '=== janitor landing detection, falsified ===' -ForegroundColor Cyan
    $failed = 0
    $ran = 0
    $reached = New-Object System.Collections.Generic.HashSet[string]
    # Resolve-MergeState asks git about the repository it is standing in.
    Push-Location $dir
    foreach ($c in $cases) {
        $ran++
        [void]$reached.Add($c.Path)
        $got = Resolve-MergeState -Head $c.Head -Branch $c.Branch -RefSha $mainSha -Reference 'main' -PrLookup $c.Pr
        $ok = ($got.Landed -eq $c.Want) -and ($got.How -eq $c.How)
        if (-not $ok) { $failed++ }
        $mark = if ($ok) { 'ok  ' } else { 'FAIL' }
        $colour = if ($ok) { 'Green' } else { 'Red' }
        $detail = if ($got.Landed) { $got.How } else { $got.Reason }
        Write-Host ("  {0} {1,-48} -> landed={2,-5} {3}" -f $mark, $c.Name, $got.Landed, $detail) -ForegroundColor $colour
    }

    # ⚠ Through the real parse, from a real gh payload, because the shape gh
    # returns *is* the defect and a hand-built list has already dodged it.
    # The second row is the one that crashed a dry run: two pull requests on
    # one branch, whose `.number` member-enumerated to both at once.
    $payloads = @(
        @{ Name = 'no pull request is none, not a numberless moved'; Json = '[]'; Sha = $wipHead; Status = 'none'; Number = '' }
        @{ Name = 'two PRs on a branch pick one, not both numbers';  Json = "[{`"number`":134,`"state`":`"MERGED`",`"headRefOid`":`"$squashedHead`"},{`"number`":111,`"state`":`"MERGED`",`"headRefOid`":`"$wipHead`"}]"; Sha = $squashedHead; Status = 'merged'; Number = '134' }
    )
    foreach ($p in $payloads) {
        $ran++
        $list = ConvertTo-PrList $p.Json
        $got = Select-PrState -Prs $list -Sha $p.Sha
        $ok = ($got.Status -eq $p.Status) -and ("$($got.Number)" -eq $p.Number)
        if (-not $ok) { $failed++ }
        $mark = if ($ok) { 'ok  ' } else { 'FAIL' }
        $colour = if ($ok) { 'Green' } else { 'Red' }
        Write-Host ("  {0} {1,-48} -> status={2} number=[{3}] parsed={4}" -f $mark, $p.Name, $got.Status, $got.Number, $list.Count) -ForegroundColor $colour
    }

    # ⚠ What a removal would destroy that no branch carries. Built as a real
    # repository with a real .gitignore, because the defect is that
    # `git status --porcelain` omits ignored files -- a hand-built list of
    # paths has already dodged the thing under test.
    $ws = Join-Path $env:TEMP "janitor-selftest-ignored-$PID"
    $co = Join-Path $env:TEMP "janitor-selftest-checkout-$PID"
    foreach ($d in @($ws, $co)) {
        if (Test-Path -LiteralPath $d) { Remove-Tree $d }
        New-Item -ItemType Directory -Force -Path $d | Out-Null
    }
    $gw = { param($rest) & git -C $ws -c user.name=janitor -c user.email=janitor@localhost @rest 2>$null | Out-Null }
    & $gw @('init', '-q', '-b', 'main')
    Set-Content -LiteralPath (Join-Path $ws '.gitignore') -Value @('/target/', '/docs/extra/', '/.agent-worktree')
    & $gw @('add', '-A'); & $gw @('commit', '-qm', 'M0')

    Set-Content -LiteralPath (Join-Path $ws '.agent-worktree') -Value 'agent lane'
    New-Item -ItemType Directory -Force -Path (Join-Path $ws 'target') | Out-Null
    Set-Content -LiteralPath (Join-Path $ws 'target\CACHEDIR.TAG') -Value $CARGO_TAG
    Set-Content -LiteralPath (Join-Path $ws 'target\big.rlib') -Value ('x' * 4096)
    New-Item -ItemType Directory -Force -Path (Join-Path $ws 'docs\extra') | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $co 'docs\extra') | Out-Null
    Set-Content -LiteralPath (Join-Path $co 'docs\extra\Shared.md') -Value 'staged from the checkout'

    $authored = Join-Path $ws 'docs\extra\YND_File_Structure_Specification.md'
    $staged   = Join-Path $ws 'docs\extra\Shared.md'
    $ignoredCases = @(
        @{ Path = 'ignored-furniture'; Name = 'build output and the marker hold nothing'
           Do = { }; Want = '' }
        @{ Path = 'ignored-authored'; Name = 'an authored spec under docs/extra holds'
           Do = { Set-Content -LiteralPath $authored -Value 'a day of work, in no commit' }
           Want = 'YND_File_Structure_Specification.md' }
        @{ Path = 'ignored-staged'; Name = 'a byte-identical staged copy holds nothing'
           Do = { Remove-Item -LiteralPath $authored -Force
                  Copy-Item (Join-Path $co 'docs\extra\Shared.md') $staged }
           Want = '' }
        @{ Path = 'ignored-diverged'; Name = 'a staged copy edited here holds'
           Do = { Add-Content -LiteralPath $staged -Value 'and a local edit' }
           Want = 'Shared.md' }
        # ⚠ Falsifies the exclusion itself: the same directory, still called
        # `target`, holds the moment cargo's signature is not in it.
        @{ Path = 'ignored-not-cargo'; Name = 'a target without CACHEDIR.TAG is not build output'
           Do = { Remove-Item -LiteralPath $staged -Force
                  Remove-Item -LiteralPath (Join-Path $ws 'target\CACHEDIR.TAG') -Force }
           Want = 'target/big.rlib' }
        # ⚠ The same directory, still with no CACHEDIR.TAG, is build output
        # again the moment it carries a marker cargo *maintains*. Measured on
        # `fmt-command-line`, whose target cargo did not create: 218.6 MiB
        # reported as work no branch carried.
        @{ Path = 'ignored-maintained-marker'; Name = 'a target cargo did not stamp is still build output'
           Do = { Set-Content -LiteralPath (Join-Path $ws 'target\.rustc_info.json') -Value '{}' }
           Want = '' }
        # ⚠ A walk that stopped at its cap knows a floor, not a total, and a
        # floor printed as a total overstates what the walk looked at.
        @{ Path = 'ignored-capped'; Name = 'a walk that stopped at its cap says so'
           Do = { 1..4 | ForEach-Object { Set-Content -LiteralPath (Join-Path $ws "docs\extra\part$_.md") -Value 'x' } }
           Want = 'at least'; MaxFiles = 2 }
    )
    foreach ($c in $ignoredCases) {
        $ran++
        [void]$reached.Add($c.Path)
        & $c.Do
        $max = if ($c.ContainsKey('MaxFiles')) { $c.MaxFiles } else { 200 }
        $got = Get-IgnoredWorkReason -Path $ws -CheckoutPath $co -MaxFiles $max
        $ok = if ($c.Want -eq '') { $got -eq '' } else { $got -like "*$($c.Want)*" }
        if (-not $ok) { $failed++ }
        $mark = if ($ok) { 'ok  ' } else { 'FAIL' }
        $colour = if ($ok) { 'Green' } else { 'Red' }
        $shown = if ($got) { $got } else { '(nothing to lose)' }
        Write-Host ("  {0} {1,-48} -> {2}" -f $mark, $c.Name, $shown) -ForegroundColor $colour
    }
    Remove-Tree $ws
    Remove-Tree $co

    # A log written beside the lanes rather than inside one, planted
    # fresh so an idle filter cannot be what hides it.
    $ran++
    [void]$reached.Add('stray-file')
    $lr = Join-Path $env:TEMP "janitor-selftest-laneroot-$PID"
    if (Test-Path -LiteralPath $lr) { Remove-Tree $lr }
    New-Item -ItemType Directory -Force -Path (Join-Path $lr 'a-lane') | Out-Null
    $stray = Join-Path $lr 'stray-run.log'
    Set-Content -LiteralPath $stray -Value 'written beside the lanes'
    $found = @(Get-LaneResidue -Root $lr -Live @((Join-Path $lr 'a-lane')) -Minutes 60)
    $ok = ($found.Count -eq 1) -and ($found[0].Path -eq $stray) -and ($found[0].Kind -eq 'file')
    if (-not $ok) { $failed++ }
    $mark = if ($ok) { 'ok  ' } else { 'FAIL' }
    $colour = if ($ok) { 'Green' } else { 'Red' }
    $shown = if ($found.Count) { ($found | ForEach-Object { "$($_.Kind) $(Split-Path $_.Path -Leaf)" }) -join ', ' } else { '(nothing)' }
    Write-Host ("  {0} {1,-48} -> {2}" -f $mark, 'a plain file in the lane root is reported', $shown) -ForegroundColor $colour
    Remove-Tree $lr

    # The lookup above is only evidence if the caller passes it the real arguments.
    $ran++
    $script:seenBranch = ''; $script:seenSha = ''
    Resolve-MergeState -Head $aheadHead -Branch 'squashed' -RefSha $mainSha -Reference 'main' -PrLookup $recording | Out-Null
    $ok = ($script:seenBranch -eq 'squashed') -and ($script:seenSha -eq $aheadHead)
    if (-not $ok) { $failed++ }
    $mark = if ($ok) { 'ok  ' } else { 'FAIL' }
    $colour = if ($ok) { 'Green' } else { 'Red' }
    $sawSha = if ($script:seenSha.Length -ge 8) { $script:seenSha.Substring(0, 8) } else { '(nothing)' }
    Write-Host ("  {0} {1,-48} -> {2} {3}" -f $mark, 'GitHub is asked about this branch and this HEAD', $script:seenBranch, $sawSha) -ForegroundColor $colour
    Pop-Location

    Remove-Tree $dir

    # What the run exercised, asserted rather than reported. A filter or a
    # deleted row leaves every remaining case green and the count is the only
    # thing that notices.
    Write-Host ''
    if ($ran -ne $EXPECTED_TOTAL) {
        $failed++
        Write-Host ("  FAIL exercised $ran checks, expected $EXPECTED_TOTAL") -ForegroundColor Red
    } else {
        Write-Host ("  ok   exercised $ran checks, as expected") -ForegroundColor Green
    }
    $missing = @($EXPECTED_PATHS | Where-Object { -not $reached.Contains($_) })
    if ($missing.Count) {
        $failed++
        Write-Host ("  FAIL no case reached: " + ($missing -join ', ')) -ForegroundColor Red
    } else {
        Write-Host ("  ok   every path reached: " + ($EXPECTED_PATHS -join ', ')) -ForegroundColor Green
    }

    Write-Host ''
    if ($failed) {
        Write-Host "$failed case(s) failed." -ForegroundColor Red
        return 1
    }
    Write-Host "$ran checks, all as intended." -ForegroundColor Green
    return 0
}

if ($SelfTest) { exit (Invoke-SelfTest) }

# ---------------------------------------------------------------- worktrees --
$worktrees = @()
$current = @{}
foreach ($line in (git worktree list --porcelain)) {
    if ($line -match '^worktree (.+)$') {
        if ($current.Count) { $worktrees += [pscustomobject]$current }
        $current = @{ Path = $Matches[1].Replace('/', '\').TrimEnd('\'); Branch = ''; Locked = $false; Head = ''; Prunable = '' }
    }
    elseif ($line -match '^HEAD (.+)$')              { $current.Head = $Matches[1] }
    elseif ($line -match '^branch refs/heads/(.+)$') { $current.Branch = $Matches[1] }
    elseif ($line -match '^locked')                  { $current.Locked = $true }
    elseif ($line -match '^prunable (.+)$')          { $current.Prunable = $Matches[1] }
}
if ($current.Count) { $worktrees += [pscustomobject]$current }

$removed = @()
$held    = @()
# Sibling target directories already counted against the worktree that owns
# them. Without this the orphan scan below counts them a second time and the
# reclaimable total reads roughly double.
$claimed = New-Object System.Collections.Generic.HashSet[string] ([StringComparer]::OrdinalIgnoreCase)

foreach ($wt in $worktrees) {
    $reason = $null
    $landedBy = ''
    # ⚠ Landing is judged last because it is the only check that reaches the
    # network. A lane with uncommitted changes is held whatever GitHub says, so
    # asking about it spends a round trip to learn nothing.
    if ($wt.Path -ieq $mainWorktree)                { $reason = 'the checkout itself' }
    elseif ($wt.Path -ieq $here)                    { $reason = 'this is where the janitor is running' }
    elseif (-not (Test-Path -LiteralPath $wt.Path)) { $reason = 'gone from disk (prune will drop it)' }
    elseif ($wt.Locked)                             { $reason = 'git worktree lock' }
    elseif ($wt.Prunable)                           { $reason = "prunable -- $($wt.Prunable)" }
    elseif (-not $wt.Branch)                        { $reason = 'detached HEAD -- no branch would outlive it' }
    else {
        $reason = Get-DirtyReason $wt.Path
        # Beside the dirty check rather than after the timer, because both ask
        # the same question -- what would this removal destroy -- and only this
        # one can see the half that no branch would bring back.
        if (-not $reason) { $reason = Get-IgnoredWorkReason -Path $wt.Path -CheckoutPath $mainWorktree }
        if (-not $reason -and -not (Test-Idle $wt.Path $IdleMinutes)) {
            $reason = "written to within $IdleMinutes min"
        }
        if (-not $reason) {
            $state = Resolve-MergeState -Head $wt.Head -Branch $wt.Branch -RefSha $refSha `
                -Reference $Reference -PrLookup ${function:Get-PrForBranch}
            if ($state.Landed) { $landedBy = $state.How } else { $reason = $state.Reason }
        }
    }

    # A target directory the old layout put beside the worktree rather than in it.
    $siblings = @()
    if (-not $reason) {
        $parent = Split-Path $wt.Path -Parent
        $leaf   = Split-Path $wt.Path -Leaf
        foreach ($shape in @("$leaf-target", "target-$leaf", ".target-$leaf")) {
            $cand = Join-Path $parent $shape
            if ((Test-Path -LiteralPath $cand) -and (Test-CargoTarget $cand) -and (Test-Idle $cand $IdleMinutes)) {
                $siblings += $cand
                [void]$claimed.Add((Get-Item -LiteralPath $cand).FullName.TrimEnd('\'))
            }
        }
    }

    $bytes = (Get-TreeBytes $wt.Path) + (($siblings | ForEach-Object { Get-TreeBytes $_ } | Measure-Object -Sum).Sum)

    # A worktree that is neither the checkout nor under the lane root was put
    # somewhere nobody will look for it. Reported whatever else happens to it,
    # because the convention being broken is the finding -- one lane was created
    # inside `cstrip/`, an unrelated project, and nothing noticed for a day.
    $misplaced = ($wt.Path -ine $mainWorktree) -and -not (Test-UnderLaneRoot $wt.Path) -and (Test-Path -LiteralPath $wt.Path)

    # ⚠ Misplaced *inside the checkout* is the worse half and is called out
    # separately. `/.claude/` and `/.cursor/` are gitignored, so a worktree an
    # agent harness drops there is invisible to `git status` while every
    # recursive walk over the tree still descends into it. Measured: 3.93 GiB
    # under `.claude/worktrees/`, and this report is what found it.
    $inside = $misplaced -and $wt.Path.TrimEnd('\').StartsWith($mainWorktree + '\', [StringComparison]::OrdinalIgnoreCase)

    if ($reason) {
        $held += [pscustomobject]@{ Path = $wt.Path; Branch = $wt.Branch; GiB = [math]::Round($bytes / 1GB, 2); Reason = $reason; Misplaced = $misplaced; Inside = $inside }
    } else {
        $removed += [pscustomobject]@{ Path = $wt.Path; Branch = $wt.Branch; GiB = [math]::Round($bytes / 1GB, 2); Targets = ($siblings -join '; '); Misplaced = $misplaced; Inside = $inside; Landed = $landedBy }
        if ($Apply) {
            Remove-Tree $wt.Path
            foreach ($s in $siblings) { Remove-Tree $s }
        }
    }
}

if ($Apply) { git worktree prune | Out-Null }

# ------------------------------------------------------------ lane residue --
# A directory under the lane root that git no longer knows about: the worktree
# was removed and its build output was not, or a lane was abandoned half-made.
#
# ⚠ These *are* deleted, and the orphans below are not. The difference is the
# parent. This directory exists only to hold lanes, so anything in it is a lane
# by construction -- which is the whole reason for having one.
$live = $worktrees.Path
$residue = @()
$strayFiles = @()
foreach ($r in (Get-LaneResidue -Root $LaneRoot -Live $live -Minutes $IdleMinutes)) {
    if ($r.Kind -eq 'file') {
        $strayFiles += [pscustomobject]@{ Path = $r.Path; KiB = [math]::Round($r.Bytes / 1KB, 1) }
        continue
    }
    $residue += [pscustomobject]@{ Path = $r.Path; GiB = [math]::Round((Get-TreeBytes $r.Path) / 1GB, 2) }
    if ($Apply) { Remove-Tree $r.Path }
}

# ------------------------------------------------------- orphaned targets --
# A target directory outside the lane root whose worktree is already gone.
# Reported, never deleted: nothing ties it to this repository any more, and the
# organisation's folder is shared with a dozen unrelated projects.
$orphans = @()
foreach ($d in (Get-ChildItem -LiteralPath $orgFolder -Force -Directory -ErrorAction SilentlyContinue)) {
    if ($d.Name -notmatch '(^\.?target|-target$)') { continue }
    if (-not (Test-CargoTarget $d.FullName)) { continue }
    if ($live -contains $d.FullName.TrimEnd('\')) { continue }
    if ($claimed.Contains($d.FullName.TrimEnd('\'))) { continue }
    $orphans += [pscustomobject]@{
        Path = $d.FullName
        GiB  = [math]::Round((Get-TreeBytes $d.FullName) / 1GB, 2)
        Idle = (Test-Idle $d.FullName $IdleMinutes)
    }
}

# ------------------------------------------------------------------ report --
$verb = if ($Apply) { 'Removed' } else { 'Reclaimable' }
Write-Host ''
Write-Host "=== worktree janitor ===  reference $Reference, idle threshold $IdleMinutes min" -ForegroundColor Cyan
Write-Host "lane root: $LaneRoot" -ForegroundColor DarkGray

if ($removed) {
    Write-Host "`n$verb ($($removed.Count)):" -ForegroundColor Green
    $removed | Sort-Object GiB -Descending | Format-Table -AutoSize GiB, Landed, Misplaced, Branch, Path | Out-String -Width 240 | Write-Host
} else {
    Write-Host "`nNothing to reclaim." -ForegroundColor Green
}

if ($residue) {
    Write-Host "Lane residue under the lane root ($($residue.Count)):" -ForegroundColor Green
    $residue | Sort-Object GiB -Descending | Format-Table -AutoSize GiB, Path | Out-String -Width 200 | Write-Host
}

if ($strayFiles) {
    Write-Host "Stray files in the lane root -- reported, not deleted ($($strayFiles.Count)):" -ForegroundColor Yellow
    $strayFiles | Sort-Object KiB -Descending | Format-Table -AutoSize KiB, Path | Out-String -Width 200 | Write-Host
    Write-Host "  The lane root holds lanes; a file belongs inside a lane or under `$env:TEMP. Check and remove by hand." -ForegroundColor DarkGray
    Write-Host ''
}

if ($held) {
    Write-Host "Held ($($held.Count)):" -ForegroundColor Yellow
    $held | Sort-Object GiB -Descending | Format-Table -AutoSize GiB, Reason, Misplaced, Branch, Path | Out-String -Width 240 | Write-Host
}

# ⚠ "GitHub was not asked" and "GitHub said no" are the same held lane and must
# not read the same way. Every pull request here is squash-merged, so with the
# question unasked the ancestor test holds every finished lane -- which is the
# defect this detection was written for, wearing the report of a clean run.
if ($script:GhState -eq 'ready') {
    Write-Host ("Landing judged against $Reference and GitHub; {0} branch(es) asked." -f $script:GhAsked) -ForegroundColor DarkGray
} elseif ($script:GhState) {
    Write-Host "GitHub was not asked: $($script:GhState)." -ForegroundColor Yellow
    Write-Host "  Landing fell back to the ancestor test, which a squash merge never satisfies," -ForegroundColor DarkGray
    Write-Host "  so any lane above held for not being on $Reference may in fact be finished." -ForegroundColor DarkGray
    Write-Host ''
}

$misplaced = @($removed | Where-Object Misplaced) + @($held | Where-Object Misplaced)
if ($misplaced) {
    # Plain ASCII deliberately: this console is codepage 850 and renders a
    # warning sign as mojibake, which is a poor look for the warning itself.
    Write-Host "WARNING -- outside the lane root ($($misplaced.Count)):" -ForegroundColor Red
    $misplaced | ForEach-Object {
        $where = if ($_.Inside) { '  [inside the checkout]' } else { '' }
        Write-Host "    $($_.Path)$where" -ForegroundColor Red
    }
    Write-Host "  A lane belongs under $LaneRoot, created by ``just agent-worktree``." -ForegroundColor DarkGray
    Write-Host "  Anywhere else is in the organisation's folder beside unrelated projects," -ForegroundColor DarkGray
    Write-Host "  where nothing enumerates it and only a name someone recognises finds it." -ForegroundColor DarkGray
    if ($misplaced | Where-Object Inside) {
        Write-Host "  One marked [inside the checkout] is worse than misplaced: it sits under an" -ForegroundColor DarkGray
        Write-Host "  ignored directory, so git never mentions it, while every recursive walk over" -ForegroundColor DarkGray
        Write-Host "  the tree still descends into a second full checkout and its build output." -ForegroundColor DarkGray
    }
    Write-Host ''
}

if ($orphans) {
    Write-Host "Orphaned target directories -- reported, not deleted ($($orphans.Count)):" -ForegroundColor Yellow
    $orphans | Sort-Object GiB -Descending | Format-Table -AutoSize GiB, Idle, Path | Out-String -Width 200 | Write-Host
    Write-Host "  No worktree claims these. Each carries cargo's CACHEDIR.TAG, so each is" -ForegroundColor DarkGray
    Write-Host "  build output -- but the janitor will not delete what it cannot tie to a" -ForegroundColor DarkGray
    Write-Host "  worktree of this repository. Check and remove by hand." -ForegroundColor DarkGray
    Write-Host ''
}

$reclaimable = (($removed | Measure-Object -Property GiB -Sum).Sum) +
               (($residue | Measure-Object -Property GiB -Sum).Sum) +
               (($orphans | Measure-Object -Property GiB -Sum).Sum)
$reclaimable = [math]::Round($reclaimable, 2)
$free = [math]::Round((Get-PSDrive ($repo.Substring(0, 1))).Free / 1GB, 2)

# ⚠ The budget is the lane root, not the drive. Free space is a fact about the
# machine and moves for reasons nothing here controls -- a game install, a video
# render -- so a budget over it fails for other people's reasons and gets
# ignored. This directory exists only because of lanes, so its size is the
# number this convention is actually accountable for.
$laneBytes = if (Test-Path -LiteralPath $LaneRoot) { Get-TreeBytes $LaneRoot } else { 0 }
$laneGiB = [math]::Round($laneBytes / 1GB, 2)

if ($Apply) {
    Write-Host ("Reclaimed {0:N2} GiB." -f (($removed | Measure-Object -Property GiB -Sum).Sum + ($residue | Measure-Object -Property GiB -Sum).Sum)) -ForegroundColor Green
    if ($orphans) { Write-Host ("A further {0:N2} GiB sits in orphaned target directories listed above." -f (($orphans | Measure-Object -Property GiB -Sum).Sum)) -ForegroundColor Yellow }
} else {
    Write-Host ("Reclaimable {0:N2} GiB. Run with -Apply to reclaim it." -f $reclaimable) -ForegroundColor Cyan
}
Write-Host ("Lane root holds {0:N2} GiB. Free on {1}: {2:N2} GiB." -f $laneGiB, $repo.Substring(0, 2), $free) -ForegroundColor Cyan

if ($BudgetGiB -gt 0 -and $laneGiB -ge $BudgetGiB) {
    Write-Host ''
    Write-Error ("The lane root holds {0:N2} GiB, at or over the {1} GiB budget." -f $laneGiB, $BudgetGiB) -ErrorAction Continue
    Write-Host "Run ``just janitor`` to reclaim what is finished, or raise the budget deliberately." -ForegroundColor Yellow
    exit 1
}
exit 0
