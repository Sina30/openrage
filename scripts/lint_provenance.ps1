<#
.SYNOPSIS
    Reports private-tier content that has reached the committed tree.

.DESCRIPTION
    Three checks. Two ask what is in a clone that should not be; the third
    asks what is one `git add -A` away from becoming one.

    **Patterns.** Leaked-source path shapes and source-file-and-line
    citations, **fatal in every tracked file**, because the whole repository
    is published with the crate (D8) and the history rewrite (D3) scrubs such
    references before it is. Engine names are not a pattern here: by the
    owner's decision D15, or-io declares GTA V under the names the engine's
    own source uses.

    Most of what OpenRage knows about RAGE's binary layouts is independently
    available from public references — CodeWalker, Materia's Python stubs
    are published by Cfx.re, GTAMods documents the containers, and shader
    parameter names come out of the user's own install. A citation pointing at
    `dev_ng` instead is both a worse citation, because a reader almost certainly
    cannot follow it, and an unnecessary claim about where the knowledge came
    from.

    **Game content.** Fixture corpora are copyrighted game files and are never
    committed. That was enforced only by `.gitignore`, which names paths — and
    when the V2 split moved the fixture roots out of `crates/format/`, the
    patterns kept naming directories that no longer existed. Nothing was ignored
    any more, a `git add -A` swept in 414 MB of resources and 6,000 metadata
    files, and it went unnoticed for 24 commits because no check looked.

    This is that check. It keys on file extension rather than on path, so it
    keeps working across a crate move — which is precisely what the ignore rules
    did not do.

    Both scan `git ls-files`, because what is committed is what a clone gets.

    **Game content nobody is ignoring.** A third check scans untracked files
    that no ignore rule covers — the set a `git add -A` would stage — and
    warns. It recognises one by extension *or* by the four bytes it opens
    with, and the signature is the half that earns the check its keep; the
    note on `Test-RageContainer` says why an extension list alone cannot.

    It never fails. The working tree is not a clone, and a lint that failed
    for data sitting on disk would trip the pre-commit hook on every
    unrelated commit.

    ⚠ Ignored paths are skipped, and that is the design rather than an
    oversight. A corpus staged under the fixture root, or junctioned in at
    `corpus/`, is ignored and so stays silent. A warning that fires on every
    run for an arrangement the ignore rules already bless is one a reader
    learns to scroll past, and then it is not a warning at all.

    **History.** `-History <repository>` runs none of the above. It scans
    every commit message and every blob reachable from any ref of that
    repository, a mirror included, with the pattern check's expressions and
    those of `-Expressions`, a `git filter-repo --replace-text` file, and
    names each hit by commit, path and line. The pre-commit hook never
    passes it.

.EXAMPLE
    ./scripts/lint_provenance.ps1

.EXAMPLE
    ./scripts/lint_provenance.ps1 -History $env:TEMP/rewritten.git -Expressions $env:TEMP/expressions.txt
#>
[CmdletBinding()]
param(
    [string]$History,
    [string]$Expressions
)

$ErrorActionPreference = 'Stop'

if ($History) { $History = (Resolve-Path -LiteralPath $History).Path }
if ($Expressions) { $Expressions = (Resolve-Path -LiteralPath $Expressions).Path }

$repoRoot = (& git rev-parse --show-toplevel 2>$null)
if (-not $repoRoot) {
    Write-Error "not inside a git repository"
    exit 2
}
Set-Location $repoRoot

# Only this script, which cannot avoid naming the patterns it looks for.
#
# `AGENTS.md` and `CLAUDE.md` used to be exempt, on the reasoning that a file
# whose job is to orient a reader may name where things came from. That was
# wrong for a plain reason: both are **tracked**, so the exemption did not keep
# the references out of a clone, it only kept them out of the report.
$exempt = @(
    'scripts/lint_provenance.ps1'
)

# Substrings that identify a reference to the leaked tree. Deliberately narrow:
# `rmptfx` or `grcTexture` on their own are RAGE vocabulary that appears in
# public references, and flagging them would bury the real hits.
#
# ⚠ These are matched with `-SimpleMatch`, so every character here is literal
# and **nothing is an escape**. The list held only the doubled spelling
# `TheftAuto\\gta5\\src` for most of its life — written as though it were a
# regex, matched as though it were not — so the one spelling a Windows-primary
# project actually produces, the single backslash anybody types in prose or in
# a path, was the one spelling the guard could not see. Do not re-escape it.
#
# Both backslash spellings stay, because neither is a substring of the other:
# after `TheftAuto\` the single expects `g` and the doubled expects `\`, so
# dropping either loses a real hit. That is measured rather than reasoned —
# planting all three spellings in a tracked file, the single-only set misses
# the doubled line and the doubled-only set misses the single one. The doubled
# form is not hypothetical: it is what a path looks like inside a Rust string
# literal or escaped JSON.
$patterns = @(
    'dev_ng'
    'TheftAuto/gta5/src'
    'TheftAuto\gta5\src'
    'TheftAuto\\gta5\\src'
)

# Extensions that only ever belong to game content. Nothing in this repository
# authors one, so a tracked file carrying any of them came out of an install.
#
# By extension and not by directory, deliberately: the rule that failed was
# path-shaped, and paths move.
#
# ⚠ It grows by measurement, never by transcribing the engine's extension
# table. `.yed`, `.ypdb` and `.yfd` joined it because a real corpus junctioned
# into a checkout held 590 of them and no rule here or in `.gitignore` named
# one; the rest of that table is absent because nothing has been seen carrying
# it, and a list wider than the evidence buries the hits that matter.
$gameContentExtensions = @(
    '.ypt', '.ytd', '.ydr', '.ydd', '.yft', '.ybn', '.yld', '.ycd'
    '.yed', '.ypdb', '.yfd'
    '.ymt', '.ymf', '.ymap', '.ytyp', '.pso'
    '.rpf', '.gxt2', '.fxc'
)

# Files that are not game content by extension but must not be committed anyway.
#
# The extension list above cannot reach these: the metadata schema snapshot is a
# `.bin`, and a `.bin` is an ordinary thing for a repository to contain. It is a
# machine-extracted derivative of a source tree that is not ours, which is a
# fact about where it came from and not about how it is named -- so it is named
# here instead.
#
# By basename, because this one has already moved three times: `ptfx-meta`,
# `or-meta`, `asset/or-meta`. There was an allowlist here holding it and a `.ydr`
# fixture open as "already published, retracting means rewriting history". Both
# were retracted; this is what keeps them out.
$forbiddenNames = @(
    'schema_snapshot.bin'
)

# One predicate for both game-content checks, so the fatal half and the
# advisory half cannot come to disagree about what game content is.
function Test-GameContent([string]$path) {
    $extension = [System.IO.Path]::GetExtension($path).ToLowerInvariant()
    $name = [System.IO.Path]::GetFileName($path)
    ($gameContentExtensions -contains $extension) -or ($forbiddenNames -contains $name)
}

# The four bytes a RAGE container opens with. Every paged resource is `RSC7`,
# every PSO metadata container is `PSIN`, an archive is `RPF7` -- measured over
# the corpus and stated independently by `or_rage_core`'s own error text.
$containerMagics = @('RSC7', 'PSIN', 'RPF7')

# ⚠ This is what keeps the untracked check from being dead code, and the reason
# is not obvious. Every extension `$gameContentExtensions` names, `.gitignore`
# also ignores -- so an extension-keyed check that skips ignored paths can only
# ever fire while those two lists disagree. The hole that put 593 stageable
# game files in a checkout was an extension **neither** list named, which is
# precisely the case an extension list cannot be extended fast enough to cover.
# A signature needs no list: an unrecognised extension is unignored by
# construction, and the bytes behind it still say what the file is.
#
# ⚠ Not every RAGE file has one -- a `.ypdb` opens `1A 02 00 00` -- so this and
# the extension test are a union rather than a replacement.
function Test-RageContainer([string]$path) {
    try {
        $stream = [System.IO.File]::OpenRead($path)
    } catch {
        return $false
    }
    try {
        $head = New-Object byte[] 4
        if ($stream.Read($head, 0, 4) -lt 4) { return $false }
        $containerMagics -contains [System.Text.Encoding]::ASCII.GetString($head)
    } finally {
        $stream.Dispose()
    }
}

# Third-party code carries its own headers and its own citations of them.
$foreignPrefixes = @('external/')

# Leaked-source path shapes and source-file-and-line citations.
#
# ⚠ A `.h` is only a citation with a directory, a line number or backticks
# around it. `bounds.h` is a field access, and a bare `\w+\.h` flagged it.
$citationRegexes = @(
    '(?i)TheftAuto[\\/]+gta5[\\/]+src'
    '(?i)\brage[\\/]+(base|suite|framework|script|scaleform)[\\/]+src\b'
    '(?i)\bshader_source[\\/]'
    '(?i)\bgame[\\/]+[A-Za-z_]+[\\/]+[\w\\/]*\.(cpp|h|fx)\b'
    '\b\w+\.(cpp|hpp|inl|fxh)\b'
    '\b\w+\.fx(:\d+|::\w+)'
    '[\w]+[\\/][\w\\/]*\w+\.h\b'
    '\b\w+\.h:\d+'
    '`\w+\.h`'
    'DeclareStruct'
)

# Binary files have no citations to fix and produce noise when matched.
$binaryExtensions = @('.bin', '.dat', '.png', '.ico', '.zip', '.lib', '.pdb', '.exe', '.dll', '.ttf', '.otf', '.glb', '.dds', '.wav')

# --- History ----------------------------------------------------------------

if ($Expressions -and -not $History) {
    Write-Error "-Expressions is read only with -History"
    exit 2
}

if ($History) {
    $historyRegexes = @($citationRegexes) + $patterns.ForEach({ [regex]::Escape($_) })

    # filter-repo's `--replace-text` format: `regex:` is an expression,
    # `literal:` or no prefix a literal, and the last `==>` starts the
    # replacement. `glob:` is refused rather than guessed at.
    if ($Expressions) {
        foreach ($rule in [System.IO.File]::ReadAllLines($Expressions)) {
            $arrow = $rule.LastIndexOf('==>')
            if ($arrow -ge 0) { $rule = $rule.Substring(0, $arrow) }
            if (-not $rule) { continue }
            if ($rule.StartsWith('regex:')) {
                $historyRegexes += $rule.Substring(6)
            } elseif ($rule.StartsWith('glob:')) {
                Write-Error "$Expressions`: a glob: expression is not supported"
                exit 2
            } else {
                if ($rule.StartsWith('literal:')) { $rule = $rule.Substring(8) }
                $historyRegexes += [regex]::Escape($rule)
            }
        }
    }

    # Each is grouped so that one's inline `(?i)` stays its own.
    $historyCombined = ($historyRegexes | ForEach-Object { "(?:$_)" }) -join '|'
    [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
    $hits = [System.Collections.Generic.List[string]]::new()

    $commit = $null
    $commits = 0
    $lineNo = 0
    foreach ($line in (& git -C $History log --all --format='%x01%H%n%B')) {
        if ($line.StartsWith([string][char]1)) {
            $commit = $line.Substring(1)
            $commits += 1
            $lineNo = 0
            continue
        }
        $lineNo += 1
        if ($line -cmatch $historyCombined) { $hits.Add(("{0}  (message):{1}" -f $commit, $lineNo)) }
    }
    if ($LASTEXITCODE -ne 0) {
        Write-Error "git log failed in $History"
        exit 2
    }

    # Every reachable blob, keyed to the commit and path that introduced it:
    # `--root` and `-m` make every blob of every tree appear as added or
    # modified somewhere, and the walk runs newest first, so the last write
    # wins with the oldest.
    $blobs = @{}
    foreach ($line in (& git -C $History -c core.quotePath=false log --all -m --root --raw --no-abbrev --no-renames --format='%x01%H')) {
        if ($line.StartsWith([string][char]1)) {
            $commit = $line.Substring(1)
        } elseif ($line.StartsWith(':')) {
            $meta, $path = $line -split "`t", 2
            $fields = $meta.Split(' ')
            if ($fields[1] -notin @('100644', '100755', '120000')) { continue }
            $blobs[$fields[3]] = @($commit, $path)
        }
    }
    if ($LASTEXITCODE -ne 0) {
        Write-Error "git log failed in $History"
        exit 2
    }

    $catFile = [System.Diagnostics.Process]::Start([System.Diagnostics.ProcessStartInfo]@{
        FileName               = 'git'
        Arguments              = "-C `"$History`" cat-file --batch"
        UseShellExecute        = $false
        RedirectStandardInput  = $true
        RedirectStandardOutput = $true
    })
    # ⚠ Windows' default `\r\n` would reach cat-file as part of the object id.
    $catFile.StandardInput.NewLine = "`n"
    $catFile.StandardInput.AutoFlush = $true
    $out = [System.IO.BufferedStream]::new($catFile.StandardOutput.BaseStream, 1MB)

    $scanned = 0
    foreach ($id in $blobs.Keys) {
        $introduced, $path = $blobs[$id]
        if ($exempt -contains $path) { continue }
        if ($foreignPrefixes | Where-Object { $path.StartsWith($_) }) { continue }
        if ([System.IO.Path]::GetExtension($path) -in $binaryExtensions) { continue }

        $catFile.StandardInput.WriteLine($id)
        $header = [System.Collections.Generic.List[byte]]::new()
        while (($byte = $out.ReadByte()) -ne 10) {
            if ($byte -lt 0) { Write-Error "git cat-file ended early in $History"; exit 2 }
            $header.Add([byte]$byte)
        }
        $fields = [System.Text.Encoding]::ASCII.GetString($header.ToArray()).Split(' ')
        if ($fields[1] -ne 'blob') { Write-Error "git cat-file: $($fields -join ' ')"; exit 2 }
        $size = [int]$fields[2]
        $bytes = New-Object byte[] $size
        $read = 0
        while ($read -lt $size) {
            $n = $out.Read($bytes, $read, $size - $read)
            if ($n -le 0) { Write-Error "git cat-file ended early in $History"; exit 2 }
            $read += $n
        }
        $null = $out.ReadByte()
        $scanned += 1

        $text = [System.Text.Encoding]::UTF8.GetString($bytes)
        if ($text -cnotmatch $historyCombined) { continue }

        $lineNo = 0
        foreach ($line in ($text -split "`n")) {
            $lineNo += 1
            if ($line -cmatch $historyCombined) { $hits.Add(("{0}  {1}:{2}" -f $introduced, $path, $lineNo)) }
        }
    }
    $catFile.StandardInput.Close()
    $catFile.WaitForExit()

    if ($hits.Count -gt 0) {
        Write-Host ""
        Write-Host "provenance history: $($hits.Count) line(s) reachable in $History carry a leak pattern."
        Write-Host ""
        $hits | ForEach-Object { "       $_" } | Write-Host
        Write-Host ""
        Write-Host "FAILED."
        exit 1
    }
    Write-Host "provenance history: clean - $commits commit message(s) and $scanned blob(s) reachable from every ref of $History."
    exit 0
}

$tracked = & git ls-files
$findings = @()

# --- Game content -----------------------------------------------------------

$leaked = @($tracked | Where-Object { Test-GameContent $_ })

if ($leaked.Count -gt 0) {
    Write-Host ""
    Write-Host "provenance: $($leaked.Count) tracked file(s) look like game content."
    Write-Host ""
    $leaked | Select-Object -First 25 | ForEach-Object { "       $_" } | Write-Host
    if ($leaked.Count -gt 25) {
        Write-Host "       ... and $($leaked.Count - 25) more"
    }
    Write-Host ""
    Write-Host "These came out of a game install or a source tree that is not ours, and must"
    Write-Host "not be committed. Check that .gitignore still names the directory they are in"
    Write-Host "- the last time this fired, a crate had moved and the ignore rules had not."
    Write-Host ""
    Write-Host "To untrack without deleting:  git rm --cached <path>"
    Write-Host "If they are already pushed, removing them needs a history rewrite. That has"
    Write-Host "been done once; docs/guides/Private_Tier.md records how and what it cost."
    Write-Host ""
    Write-Host "FAILED."
    exit 1
}

# --- Game content nobody is ignoring ----------------------------------------
#
# `--exclude-standard` is the whole design of this check: it reports what a
# `git add -A` would stage and nothing else, so an ignored corpus is silent.
#
# ⚠ Git descends into a Windows junction as though it were an ordinary
# directory -- `core.symlinks` is not involved, a junction being no kind of
# symlink -- so a corpus junctioned in at `corpus/` presents every file behind
# the link as its own untracked entry. What keeps that quiet is the rule naming
# it in `.gitignore`, not anything here.

$stray = @(
    & git ls-files --others --exclude-standard |
        Where-Object { (Test-GameContent $_) -or (Test-RageContainer $_) }
)

if ($stray.Count -gt 0) {
    Write-Host ""
    Write-Host "provenance: $($stray.Count) untracked file(s) look like game content."
    Write-Host ""
    $stray | Select-Object -First 10 | ForEach-Object { "       $_" } | Write-Host
    if ($stray.Count -gt 10) {
        Write-Host "       ... and $($stray.Count - 10) more"
    }
    Write-Host ""
    Write-Host "Nothing is committed and nothing here is wrong. But no rule is ignoring"
    Write-Host "them either, so one stage-everything commits the lot - which is how 414 MB"
    Write-Host "got in last time. If this is a corpus, .gitignore should name where it sits."
    Write-Host ""
    Write-Host "Advisory. This half never fails."
}

# --- Patterns ---------------------------------------------------------------

$combined = ($citationRegexes + $patterns.ForEach({ [regex]::Escape($_) })) -join '|'
$fatal = @()

foreach ($file in $tracked) {
    if ($exempt -contains $file) { continue }
    if ($foreignPrefixes | Where-Object { $file.StartsWith($_) }) { continue }
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { continue }

    $extension = [System.IO.Path]::GetExtension($file)
    if ($extension -in $binaryExtensions) {
        continue
    }

    $text = [System.IO.File]::ReadAllText((Join-Path $repoRoot $file))
    if ($text -notmatch $combined) { continue }

    $lineNo = 0
    foreach ($line in ($text -split "`n")) {
        $lineNo += 1
        $why = $null
        foreach ($regex in $citationRegexes) {
            if ($line -cmatch $regex) { $why = 'cites the leaked source'; break }
        }
        if (-not $why) {
            foreach ($literal in $patterns) {
                if ($line.Contains($literal)) { $why = 'cites the leaked source'; break }
            }
        }
        if (-not $why) { continue }
        $fatal += [pscustomobject]@{ File = $file; Line = $lineNo; Why = $why }
    }
}

if ($fatal.Count -gt 0) {
    Write-Host ""
    Write-Host "provenance: $($fatal.Count) tracked line(s) carry a leak pattern."
    Write-Host ""
    $fatal | Select-Object -First 40 | ForEach-Object { "       {0}:{1}  {2}" -f $_.File, $_.Line, $_.Why } | Write-Host
    if ($fatal.Count -gt 40) { Write-Host "       ... and $($fatal.Count - 40) more" }
    Write-Host ""
    Write-Host "Cite a public reference instead - CodeWalker, Materia's published API,"
    Write-Host "GTAMods, or a measurement over the corpus - or drop it. File-and-line"
    Write-Host "citations of the source tree belong in docs/extra/, which is gitignored;"
    Write-Host "names derived from it are fine (D15)."
    Write-Host ""
    Write-Host "FAILED."
    exit 1
}

if ($stray.Count -eq 0) {
    Write-Host "provenance: clean - no game content and no references to the leaked source tree."
} else {
    # Not "clean": the warning above is still on screen, and a summary line
    # contradicting it is the line a reader will believe.
    Write-Host "provenance: nothing tracked - no committed game content and no references to the leaked source tree."
}
exit 0
