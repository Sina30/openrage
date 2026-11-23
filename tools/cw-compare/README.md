# cw-compare

A Windows tool that loads an original and a re-encoded GTA V file with [CodeWalker](https://github.com/dexyfex/CodeWalker), exports both to XML and diffs them field by field, to answer whether CodeWalker loads what OpenRage wrote.

It is a compatibility diagnostic, not a gate: no CI job runs it, and
CodeWalker misreads some formats, so never use it to decide what the bytes
mean. [docs/guides/Oracle_Testimony.md](../../docs/guides/Oracle_Testimony.md) says what
does check a layout.

## Prerequisites

- Windows, the .NET 8 SDK or later, and the Rust toolchain.
- The CodeWalker submodule at the commit this repository pins
  (`git submodule update --init --recursive external/CodeWalker`), or another
  checkout named with `-p:CodeWalkerRepoRoot=<path>` or in
  [CwCompare.csproj](CwCompare.csproj).
- A staged corpus ([the fixtures README](../../crates/io/or-io-verify/tests/fixtures/README.md)).

## Compare two files

```powershell
dotnet build tools/cw-compare/CwCompare.csproj -c Release
dotnet run --project tools/cw-compare -c Release -- compare original.ydr converted.ydr `
  --allowlist tools/cw-compare/cw-compare-allowlist.json --out report.json --html report.html
```

`--format` overrides detection from the extension: `ydr`, `ydd`, `yft`,
`ybn`, `ytd`, `ypt`, `yld`, `ycd` or `meta` (`ymt`, `ymf` and `pso` mean
`meta`); `--epsilon` is the float tolerance, `1e-5` by default. The exit code
is 0 when both files parse and no issue is an `unexpected_mismatch`, 1
otherwise, and 2 on a usage error. The Gen8 or Gen9 layout is chosen per file
from its RSC7 version, so a mixed corpus compares in one run.

## Batch: re-encode, then compare

`Invoke-RoundtripCompare.ps1` builds `cw-compare` and `openrage-cli` in
Release once, re-encodes each file with `openrage-cli convert` (or
`openrage-cli meta rewrite` for `.ymt`, `.ymf` and `.pso`) and compares it
with the original, writing JSON reports to `-OutputDir`
(`target/cw-compare-batch`; `-IncludeHtmlReports` adds HTML). It scans
`-SearchPath` (`crates/io/or-io-verify/tests/fixtures`) for `-Extension` (any of `ydr`
`ydd` `yft` `ytd` `ypt` `ybn` `yld` `ycd` `ymt` `ymf` `pso`, all by default),
unless `-Resource` (repeatable) or `-ResourcesPipe` (`a|b`) names files.
`-Allowlist` and `-Epsilon` pass through. From the checkout's root, with the
corpus beside it:

```powershell
.\tools\cw-compare\Invoke-RoundtripCompare.ps1 -SearchPath ..\openrage-corpus\or-asset -Extension ydr,ycd
.\tools\cw-compare\Invoke-RoundtripCompare.ps1 -Resource ..\openrage-corpus\or-asset\des_bridge_root.ydr
```

- ⚠ The default `-SearchPath` holds no corpus when the corpus is the sibling
  `openrage-corpus`, and a scan that finds nothing warns and exits 0: a green
  run that compared nothing. Pass the corpus directory
  (`openrage-corpus/or-meta/binary` for metadata).
- The script runs `target\release\openrage-cli.exe` under the repository, so
  a `CARGO_TARGET_DIR` pointing elsewhere stops it before the first file.
- `just cw-roundtrip` takes zero or one argument: nothing scans, a list of
  extensions filters the scan, one path compares that file, and `a|b`
  compares several. Its extension form knows only `ydr` `ydd` `yft` `ytd`
  `ypt` `ybn` `yld`; for `.ycd` or metadata, call the script with
  `-Extension`.

## Reading a diff

**YLD.** CodeWalker reads three of the four per-LOD slots (`LOD_COUNT` is 4)
in `clothController.m_Cloth`, `phMorphController.m_MapData` and each
`clothBridgeSimGfx` `atRangeArray` group. Both sides of the diff share that
blind spot, so a clean compare says nothing about the fourth slot; the strict
parse and the fixed point in `crates/io/or-io-verify/tests/cloth_dict.rs`
cover it.

**YCD.** A `kLinear` channel is exported with a `QuantizeFloat` type label, as
the TODO beside it in CodeWalker's `Clip.cs` says, so type differences on
linear channels are CodeWalker's. `crAnimation::m_Name` never appears:
CodeWalker reads offsets 0x20 to 0x38 as six unused words, the first two of
which are the name pointer. Neither is in the allowlist, so a compare that
meets one exits 1. Block payloads are carried verbatim, so the diff cannot test
the channel decoders; the unit tests in `crates/io/or-io/src/codec/anim/`
and the pose baseline `every_corpus_clip_poses_as_recorded` do.
The local [YCD specification](../../docs/extra/YCD_File_Structure_Specification.md)
lists every place CodeWalker misreads `.ycd`.

**Metadata.** PSO (`PSIN`), RBM (`RSC7` with `PRD0`) and RBF0 share `.ymt`, so
both sides route on the leading magic. RBM file bytes change on write, because
its raw DEFLATE output is not reproducible; the harness compares decoded XML
and is unaffected, and `openrage-cli meta roundtrip` checks the decompressed
payload. A name on one side and `hash_HHHHHHHH` on the other is a dictionary
difference, not a decode one: CodeWalker names hashes from its `JenkIndex`,
OpenRage from `binary::public_names` and the schema snapshot, if the build
found one ([the snapshot README](../../crates/io/or-io/src/meta/README.md)).

## The allowlist

`cw-compare-allowlist.json` is `{"version": 1, "entries": [...]}`, each entry
`{"pathPattern", "regex", "classification", "note"}` with `classification`
usually `allowed_divergence`. A plain pattern matches an issue path by
case-insensitive prefix; a regex matches anywhere in the full path, and one
that does not compile is skipped silently. Comments and trailing commas are
allowed. The shipped list is empty: the known divergences above are
field-level and no path pattern expresses them.

## Adding a format

A new format touches `Invoke-RoundtripCompare.ps1` (`$allowed` and the
`-JustTail` extension regex), `CodeWalkerLoader.cs` (`FormatFromExtension` and
`LoadXml`), `Program.cs` (`ParseFormat` and the usage text) and, for any
divergence a path can express, the allowlist.
