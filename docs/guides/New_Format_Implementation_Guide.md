# Adding a file format

How a GTA V file format is brought up in OpenRage, from reconnaissance through
its declarations, gates, CLI, app and specification, with the traps each step
has already cost.

Declaration mechanics belong to
[the or-io specification](../direction/OR_IO_Specification.md#declaring-a-structure), the
app's side to [Crate_Reference.md](../reference/Crate_Reference.md#what-a-format-costs-the-app),
and the choice of work to [the roadmap](../direction/Roadmap.md): R66d for formats
nothing reads, and [the deferred list](../direction/Roadmap.md#deferred-and-known-gaps).

## Where the work goes

- **A GTA V format is declared in `or-io`**: `#[record]` declarations in a
  module of `crates/io/or-io/src/gta5/`, a row in
  `crates/io/or-io/src/gta5/formats.rs`, and suites in `crates/io/or-io/tests/`
  and `crates/io/or-io-verify/tests/`. A migrated declaration is in the owned
  form (D16) and named from the GTA source (D15).
- **The app and the CLI open it as an owned value** through `or_io::open`
  and its `Asset`, with no pack; [the CLI and the app](#the-cli-and-the-app)
  is the wiring.
- **A file that is not an RSC7 resource** is ordinary code over declared
  records, shaped like `crates/io/or-io/src/gta5/text.rs`; a declared
  file-relative offset waits for the field types the specification lists as
  planned.

## What done means

A format is done for a target when every row holds; parsing is only the first.

| # | Criterion | Held by |
|---|---|---|
| 1 | Declared from the GTA source: every field named, no offset written, a root per target at the RSC7 versions measured over both installs | `#[record]`'s compile-time proof and the format's `FORMATS` row |
| 2 | The or-io-verify gates hold for every target the format ships | the corpus suite, [below](#gating-it) |
| 3 | Real files read back equal in value after the first canonical save, and a second save is byte-identical | the fixed-point gate; byte-identity with the shipped file is reported, never required |
| 4 | A file authored from scratch, never touched by a reader, saves, reopens and reaches a fixed point | the synthetic suite |
| 5 | What the game would misread is refused before a byte is written | the format's `validate`, and a test that breaks a file on purpose |
| 6 | `openrage-cli convert info`, `convert verify` and `inspect` handle it | [the CLI](#the-cli-and-the-app) |
| 7 | The editor detects it, names it, opens it and describes it | `a_synthetic_format_is_fully_usable` is the shape |
| 8 | A specification sits in the checkout's docs/extra/ | [the specification](#the-specification) |
| 9 | `just test` ran the corpus suites, and the skip ledger shows none skipped | [verifying](#verifying-beyond-the-gates) |

## Sources of truth

**Names come from the GTA source (D15)**: names, field order and types, bases,
padding and enum values follow the engine's declarations in Rust casing
([names come from the source](../direction/OR_IO_Specification.md#names-come-from-the-source)),
and no new or migrated field is `unknown_*` or `unk*`. A citation of the source
tree by path, file or line stays in the gitignored docs/extra/ tier;
`just lint-provenance` fails on one in any tracked file.

**What the bytes are, when sources disagree**, is settled in this order:

1. **The retail files, measured**: a census over both installs, the recorded
   testimony, the canonical fixed point, an identity every file holds. A layout
   the source declares still has to hold here.
2. **CodeWalker** (`external/CodeWalker`), a strong cross-check of field order
   and sizes and not ground truth. When the two disagree, measure: OpenRage
   once read `Mat34V` as 48 bytes against CodeWalker's 64, and CodeWalker was
   right; [where CodeWalker is wrong](#where-codewalker-is-wrong) lists the
   opposite cases.
3. **This repository's documents**: orientation, never a check on the code,
   because most were written from it. A document that agrees with the code is
   one observation, not two.

## Reconnaissance

Answer these before writing a declaration. Every estimate made before a format
was measured has come out low, worst where the layout was assumed understood.

### Is it a resource

`openrage-cli deflate --info <file>` prints an RSC7 header's version and flag
words and fails on anything else, and `--split` dumps the two heaps. In code
`Rsc7::detect` answers it, and `Rsc7::open` inflates without judging versions.
Find the engine's writer, not only its reader: the loose `.gxt2` is
gxtmaker2's unpaged format and the paged `fwTextAsset` another encoding of the
same text, so work on one says nothing about the other.

### Versions and targets

- `openrage-cli versions` censuses the RSC7 versions an install ships; run it
  over both editions. `crates/io/or-io/src/container/resource/versions.rs`
  states the observed versions, and each root in `FORMATS` names its version.
- A file's target is its own RSC7 version (`Format::target_of_version`), never
  its install's edition: an Enhanced install ships `.yfd` at the Gen8 version
  and some `.ytd` at Gen8's 13 beside Gen9's 5. Versions are labels, not an
  order. The flag nibbles derive from the version (`version_nibbles_for`);
  compare them with a shipped file's through `deflate --info`.
- ⚠ A version registered for a layout nobody measured at it turns a clean
  refusal into a silent misparse: a Gen9 `.ytd` read with the Gen8 layout
  returns 0×0 textures with no format and no warning. `.ybd`'s dictionary
  envelope is identical at three versions and only one version's bounds are
  described, so its refusal sits at the version. A version nothing declares
  stays refused by name.

### Widths, bases and ownership

| Type | Width | Why |
|---|---:|---|
| `Vec2V`, `Vec3V` | 0x10 | backed by a full SIMD register |
| `Vec3` in `or_io::decl` | 0x0C | three floats and no fourth lane |
| `Mat34V` | 0x40 | four columns, not three |
| a BVH's quantize factor and box | 0x10 | stored as four floats |

- Decide per struct whether it starts with an eight-byte vtable: a type
  deriving `datBase` or `pgBase` does (buffers, geometries, models, shader
  groups, skeletons, drawables); `grcFvf`, `grmShader`, `rmcLodGroup` and
  `rmcLod` do not.
- Check offsets, not only sizes. `PgBase` once had the right size, 0x10, and a
  `pg_base_field` that never existed beside a four-byte vtable, where the
  layout is an eight-byte vtable and an eight-byte `m_FirstNode`; nothing
  failed until a field was used. `#[record]`'s tiling proof and the gates check
  offsets; a pinned `offset_of!` literal is not the check.
- `datOwner<T>` and `pgOwner<T>` own their child and `datRef<T>` and
  `pgRef<T>` point at it, in the same eight bytes: an owner written as a
  reference dangles, a reference written as an owner duplicates the object.
- An `atArray` is a 0x10 header (pointer, count, capacity) over elements held
  elsewhere, declared `Vec<T>`; a `pgArray` is a 0x20 header with its elements
  inline after it, declared as a `#[trailing]` run.
- [RAGE_Struct_Binary_Layout_Specification.md](../extra/RAGE_Struct_Binary_Layout_Specification.md)
  holds the base classes and alignment rules.

### Tagged bases, derived arrays and computed regions

- A tagged base's tag selects the layout: bounds by kind, behaviours by type
  hash, frame filters by type. A tag may be a bit range (`tag_bits`). Declare
  only variants a file can check, so an undeclared tag is refused by name.
- Find every derived parallel array before writing: a particle rule's five
  filtered behaviour slots are subsets of its behaviour list, and a reader and
  writer that handle only the list drop the other five.
- Measure a container's element stride, because a declared container type
  names the base class ([the YDD bring-up](#the-ydd-bring-up)).
- A region whose layout is computed rather than placed, such as a material's
  parameter blob, takes a
  [codec](../direction/OR_IO_Specification.md#codecs-for-computed-regions). The test is
  whether there is a constant to write down; a region merely undescribed wants
  a declaration.

### A corpus

- Stage real files with `crates/io/or-io-verify/scripts/stage-corpus.ps1`,
  which mounts one install and writes to the sibling `openrage-corpus/or-asset`;
  run it once per edition, with `-OutDir` at that directory's `gen9` for
  Enhanced files. Take the whole population for a small format, and never
  commit one. Suites find them through `or_test_support::Corpus`, which files
  everything under `gen9` as Gen9.
- ⚠ `gen9` holds what the Enhanced install ships, not only Gen9 resources:
  its `.yed`, `.yfd`, `.ynd` and `.ynv` are Gen8 versions and one `.ytd` is
  neither target's, and a suite that trusts the directory counts them as Gen9.
  A suite that needs Gen9 bytes checks each file's RSC7 version, as
  `or-io-verify`'s do.
- A zero in the corpus is not a zero in the game: the staged fragments held no
  glass pane, while `openrage-cli gaps --game` over a retail install found
  panes in both editions. Census the install (`gaps`, or `formats` scoped with
  `--ext`) before calling a layout unreachable.

## Declaring it

`just scaffold-format MODULE EXT ROOT VERSION` writes a starting point for
one Gen8 root at one RSC7 version: the module, with a placeholder
`entries` field on the root and a `validate` that refuses nothing, its
`mod.rs` line, its `FORMATS` row (`DatResourceFileType::Custom` where the enum
has no variant for the extension), its `Asset` variant, a row in each of
the two census suites that `every_declared_format_is_censused` holds to
`FORMATS`, a synthetic suite and a corpus suite that is one
`declared_gates!` invocation. It refuses to overwrite anything. Replace the
placeholder with the engine's fields, add a Gen9 root by hand, and give
`validate` its invariants.
`a_scaffolded_format_compiles_and_its_gates_skip_without_a_corpus` scaffolds
an invented format into a copy of the tree under `$TEMP` and checks that it
compiles, that its gates record corpus skips and that every format is still
censused. It builds a second workspace, so it is ignored and no recipe or CI
job runs it; run it by name with
`cargo test -p or-io-verify --bin scaffold -- --ignored a_scaffolded_format_compiles_and_its_gates_skip_without_a_corpus`.

The steps, each owned in detail by
[the specification](../direction/OR_IO_Specification.md#declaring-a-structure):

1. Add a module to `crates/io/or-io/src/gta5/` with a `//!` header of a
   sentence or two, list it in `crates/io/or-io/src/gta5/mod.rs`, and put a
   type two formats share in `crates/io/or-io/src/gta5/shared.rs`.
2. Declare each struct with `#[record(size = N)]` in the
   [owned form](../direction/OR_IO_Specification.md#the-owned-form-d16): the engine's fields
   in the engine's order, in the types a user edits. Offsets are computed;
   padding is a field under the source's name, or `#[pad_after(N)]` where the
   compiler would pad; `#[at(0x..)]` is an optional assertion; `#[len(...)]`
   says where a run's length lives. `decl::refusals` lists what fails to
   compile, `#[record(raw, ...)]` among them: it is retired.
3. Declare enums with `#[record(enum, width = ...)]`, held as `Open<E>`, and a
   tagged base's variants on the base
   ([tagged bases](../direction/OR_IO_Specification.md#tagged-bases-and-enums)). Declare a
   type Gen8 and Gen9 lay out alike once, the four Gen9 changed once per
   target, and a type differing only in what it points at with a target
   parameter ([Gen8 and Gen9](../direction/OR_IO_Specification.md#gen8-and-gen9)).
4. Give the root an `impl Resource` naming `EXT` and `TARGET`; it gets `open`,
   `from_bytes`, `to_bytes`, `save`, `save_as` and `peek`.
5. Add a row to `FORMATS`: the extension, its `DatResourceFileType`, a root per
   target and measured version, a `validate` function and any codecs.
6. Make `validate` refuse what the game assumes and never checks.
   `crates/io/or-io/src/gta5/frame_filter_dict.rs` refuses an unsorted track
   table, which the engine binary-searches, and a weight index past its table;
   `.yed` refuses a program longer than its expression set's
   `max_packed_size`, because one scratch buffer of that size serves every
   program.

⚠ A field whose name starts with `_pad` is left out of the description, and
every `#[pad_after(N)]` is described as padding; both are written as zero on
every save. A pad the source
declares is an ordinary, carried field under the source's name (`padding` in
`TrackIdIndex`); named `_pad`, its live bytes are lost on the first save.

## Gating it

A format needs two suites:

- **Synthetic**, in `crates/io/or-io/tests/`, shaped like
  `crates/io/or-io/tests/gta5_frame_filter_dict.rs`: a value authored in Rust
  with invented content saves, reopens, edits, reaches a fixed point, is
  refused where the game would misread it, and has a description equal to
  `offset_of!`. It needs no game file, so hosted CI runs it.
- **Corpus**, in `crates/io/or-io-verify/tests/`, built on `declared_gates!` from
  `crates/io/or-io-verify/src/gates.rs` and shaped like
  `crates/io/or-io-verify/tests/frame_filter_dict.rs`, or
  `crates/io/or-io-verify/tests/texture_dict.rs` for two targets.

[The gates per format](../direction/OR_IO_Specification.md#the-gates-per-format) define
the corpus suite; `.yfd`'s tests show each one:

| Gate | `.yfd`'s test |
|---|---|
| Typed reads equal the tree's | `every_yfd_reads_typed_as_the_tree_reads` |
| The owned value saves as its tree saves | `every_yfd_opens_owned_and_saves_as_its_tree_saves` |
| Value-equivalent first save, byte-identical second | `every_yfd_saves_to_a_fixed_point` |
| A file the game would misread is refused | `a_yfd_the_game_would_misread_is_refused_before_it_is_written` |
| No instance sits off its alignment | `every_yfd_instance_is_aligned_for_its_view` |
| The testimony, re-keyed by offset, agrees and reaches everything | `the_yfd_testimony_holds_through_its_declarations` |

- Each gate has a falsifier that must fail beside it:
  `a_declared_field_one_byte_late_is_named_by_the_testimony` and
  `an_owned_field_one_byte_late_does_not_save_as_the_tree` are the shape.
- The testimony exists only for a format a compiled reader read
  ([Oracle_Testimony.md](Oracle_Testimony.md)). Without one, the independent
  evidence is an identity every file states about itself:
  `every_filter_checksum_is_the_crc_of_its_tables`,
  `every_region_closes_its_chains`.
- The format's figures go in [R66d](../direction/Roadmap.md#r66d-declare-gta-v-format-by-format).

## Verifying beyond the gates

- **`just ready` and `just test-lite` skip the format suites:** the corpus
  suites of or-io-verify are in the slow tier, which no gate runs. Run
  `just test-format MODULE` before claiming a read or write path works, and
  `just test-strict`, which fails instead of skipping, where a corpus is
  staged. `.ycd`'s suites once passed with nothing staged;
  the first corpus run found the channel directory read from the wrong field
  (`every_animations_blocks_agree_with_its_frames` now holds it).
- **A round trip passes when reader and writer are wrong the same way.** Add a
  from-scratch test, an identity the file states, and a canvas that cannot be
  echoed: the metadata writers' `write_with_coverage` and the resource heap's
  `POISON_CANVAS_FILL` fill it with a poison byte, so a byte a writer forgot
  shows. A red and blue swap survives any symmetric round trip, so a texture
  path is checked per channel on a known image.
- **Run once under `ErrorMode::Strict`** and read the diagnostics: a lenient
  read skips what it cannot parse and still returns `Ok`.
- **Track the mismatch count and the size delta per fixture**, debugging on
  the smallest failing file; a generous size tolerance once hid a writer
  dropping data. A file overlaid from `update.rpf` can differ from the base
  pack's copy of the same name, so know which you read.
- **Feed it corrupted input** (truncated, a byte flipped, a Git LFS pointer)
  and expect a clear error; RSC7 has no checksum, so not every flip is
  detectable, and none may panic.
- **Run the CLI twice and compare.** Rust seeds `HashMap` per process, so an
  in-process repeat cannot catch map order reaching an output buffer.
- **Install-gated tests** take roots from `or_test_support::game_roots()`
  (`OPENRAGE_GAME_ROOT`) and record absence with `skip_install`; NG-encrypted
  archives open with the bundled key snapshot unless `OPENRAGE_KEY_SNAPSHOT`
  names another. A missing install
  recorded as a missing corpus is a skip `just test-strict` fails.
- **CodeWalker is a diagnostic, not a gate**
  ([its README](../../tools/cw-compare/README.md)): it tells whether CodeWalker
  loads what OpenRage wrote. A format needs four edits: `$allowed` in
  `tools/cw-compare/Invoke-RoundtripCompare.ps1`, the mapping in
  `tools/cw-compare/CodeWalkerLoader.cs`, `--format` in
  `tools/cw-compare/Program.cs`, and accepted divergences in
  `tools/cw-compare/cw-compare-allowlist.json`. `just cw-roundtrip` scans
  `crates/io/or-io-verify/tests/fixtures`, so point the script's `-SearchPath` at the
  corpus. A file CodeWalker cannot load at all points at the RSC7 flags, the
  root pointer, then page packing, which `Rsc7::author` without size info
  bisects; a writer that falls back on its page flags logs
  "Flag encoding fallback triggered".

## The CLI and the app

The CLI and the app reach a format through its owned root and `or_io::open`,
and [what a format costs the app](../reference/Crate_Reference.md#what-a-format-costs-the-app)
owns the detail:

- Every paged GTA V format already has a `ResourceType`
  (`crates/io/or-io/src/container/resource/versions_registry.rs`) and a
  `DatResourceFileType` (`crates/io/or-io/src/container/resource/header.rs`).
- A `FormatHandler` in `src/state/format_handlers.rs` names the extensions
  `DocumentKind::from_path`, the Open dialog and the archive tree use, and
  `or_io::open` says what a file holds. A `PayloadHandler` in
  `src/state/payload_handlers.rs` opens the file, as
  `DocumentKind::Registered` until it has a kind of its own; a
  `FormatCapabilities` row in `src/state/capabilities.rs` gives it panels; a
  named kind adds a `DocumentKind` variant and a `KindInfo` row in
  `src/state/kinds.rs`, and its `resource_type` returns the format, or the
  description-derived grid has no bytes to read.
- A file that is not a resource and shares its extension is routed by name
  or by content: a binary world table by its file name ahead of the text
  `.dat` route (`DocumentKind::from_path`), metadata in a text extension by
  its leading bytes (`DocumentKind::refine_with_bytes`). A file nothing routes
  opens as text when its bytes are text, and otherwise read-only as
  `DocumentKind::Hex`, so a format with no reader shows its bytes rather than
  an error.
- `convert` info and verify helpers and the `inspect` subcommands in
  `crates/tools/or-cli/src/inspect.rs` are per-format work.
- Name completion has two producers: a file name that is itself the
  completion goes in `NAME_KIND_BY_EXTENSION`
  (`crates/title/or-title/src/install/names.rs`), names inside the file in
  `names_in` (`src/state/name_index.rs`), and a new category names its
  producer in `PRODUCER_OF`.

## The specification

- Write it at the depth of
  [YDR_File_Structure_Specification.md](../extra/YDR_File_Structure_Specification.md),
  named like its neighbours in the gitignored docs/extra/ tier, and give every
  measured decision its numerator, denominator, suite and revision
  ([Documentation_Standard.md](Documentation_Standard.md)).
- Copy it into the checkout's own docs/extra/ before calling the lane done: it
  is in no commit and dies with the worktree, and the janitor holds such a lane
  but cannot save the file
  ([Development_Workflow.md](Development_Workflow.md#cleaning-up-the-janitor)).

## The YDD bring-up

A drawable dictionary looked cheap, its parts already read and written, and it
held most of the general lessons ([YDD_File_Structure_Specification.md](../extra/YDD_File_Structure_Specification.md)).

- **The entry type is not the declared one.** The engine declares a dictionary
  of `rmcDrawable`; each entry is a `gtaDrawable`, 0xD0 rather than 0xB0, by
  the measured stride. Read as declared, a file keeps its count and hashes and
  loses every entry's lights, tint data and bound.
- **A generic path defaulted to a cheap mode.** A drawable's `FromResource`
  reads reference mode (`from_resource_with_options` with full parsing off),
  so the generic dictionary reader wrote empty models;
  the dictionary reader took the entry reader instead.
- **Two orders conflict.** A round trip keeps the file's entry order, and an
  authored dictionary must be sorted by hash for the engine's binary search,
  so the writer takes an explicit `EntryOrder`. Byte-identity was abandoned on
  evidence: shipped files place their child blocks in several orders.
- **Unknown fields want opposite treatments.** The build-specific vtable is
  carried; `m_FirstNode` is the defragmenter's list head, a runtime word, so it
  is nulled.
- **A block referenced twice is fixed up twice.** The game's fixup walk has no
  visited set, so a buffer shared by two LOD levels is rebased twice and the
  file rejected; the aliasing check in every drawable-bearing format's
  `validate` refuses it by identity, for both targets. Detection
  stays on vertex and index buffers: a `datRef` may be shared, and every
  fragment's child drawables share one skeleton
  ([Drawable_Fixup_Validation_Specification.md](../extra/Drawable_Fixup_Validation_Specification.md)).

## The trap list

Ranked by what each has cost; each is a reading or an action that looks right
and is not.

1. **Every pointer read must be written or explicitly nulled.** A writer that
   copies a source header and patches some fields leaves pointers into the old
   layout; this caused most of the particle writer's mismatches. The canonical
   writer writes every pointer slot itself, so the exposure left is a
   hand-written writer or a codec, which must rebase every pointer it carries.
2. **A lossy model on the save path destroys fidelity.** An editor model
   between reader and writer dropped property ids, random indices, curves and
   ref counts. Narrow nothing: `!= 0` destroys a value that can be 2, C++
   `char` is signed, and a null pointer is not an empty collection.
3. **A field named as padding can hold live data.** `CLightAttr`'s seven
   "padding" bytes held two flag bits and a live `s16`, and a `pad_after`
   zeroed them on every save. Measure the bytes, and carry them under the
   source's name.
4. **Check widths against the files.** An expression's spring row is one
   sixteen-byte `Vec3V` whose fourth word holds something else; declared as a
   vector plus a word, it made the descriptor 0xC8 where the file holds 0xA0,
   and every corpus file still round-tripped. The pack comparison caught it.
5. **A version without its layout is a silent misparse** ([above](#versions-and-targets)).
6. **Capacity is a promise.** CodeWalker walks a pointer table by capacity, and
   one advertising more than it allocated crashed it. The owned form carries a
   capacity and the canonical writer keeps the larger of it and the length.
7. **A writer never swallows an error or invents data.** `unwrap_or(0)` on an
   unknown size or an unresolved name wrote a corrupt file instead of an
   error, and a keyframe property synthesised where the file had none showed
   CodeWalker an extra element in every behaviour.
8. **The engine binary-searches hash tables.** A file can round-trip and be
   unsearchable: sort on authoring and refuse an unsorted
   table in `validate`. Index under exactly the hash the format stores;
   `atStringHash` lowercases, and adding a normalised hash beside it doubled
   the collisions and dropped entries.
9. **On-disk enums are not the engine's.** A texture's format field holds
   D3D9 `D3DFORMAT` values, so uncompressed formats matched against the engine
   enum fell through to none while FourCC formats hid it. The precedence is
   FourCC, D3D9, then DXGI, and `"BC7 "` ends in a space.
10. **A zero vtable crashes the game.** `Default` and an empty description
    leave a root's vtable zero, so a file authored for the game sets
    `DEFAULT_VFT`; a zeroed buffer patched at chosen offsets has defaulted
    every byte it did not touch. A `ref_count` follows from the object graph.
11. **A lenient read returns success with nothing in it.** A lenient string read
    gives no string for a pointer in neither heap, and a lenient child read
    substitutes a default (a slot array keeps a meaningful null as `None`). A wrong offset base reads
    zero counts; byte-swapped ones read huge.
12. **A read-back proves the reader agrees with the writer, not the engine.**
    A Gen9 material's parameter block holds one copy per worker thread, and
    writing one copy passes any read-back.
13. **`after_write` cannot fix bytes already committed.** In the compiled
    writers R66f deleted, `build_raw` wrote no side data, so strings and
    children went into the builder before the parent's raw struct, and an
    embedded fixup was the parent's offset plus `offset_of!`, never a literal.
    Cross-field state travels through `ResourceHeap::record_shared` and
    `shared_object`, and those writers wrote pointer fields in declaration
    order, so a shader group's texture dictionary is declared above its shaders.
    `a_material_keeps_its_texture_bindings_across_a_save` now holds the
    bindings on the owned drawable's save, not on these writers.
14. **`ResourceHeap` never aligns for you**: strings to 4, `atArray` data to
    16, texture payloads to 256, and a run walked by stride stays contiguous.

## Where CodeWalker is wrong

Recorded so that checking OpenRage against CodeWalker does not blame OpenRage.

| CodeWalker | The files |
|---|---|
| `Clothes.cs`: cloth per-LOD arrays have 3 slots | `LOD_COUNT` is 4; a missed slot is an owned child, and a CodeWalker diff shares the blind spot ([YLD](../extra/YLD_File_Structure_Specification.md)) |
| `Clothes.cs`: `ClothController.BlockLength` is 0x80 | 0x78; the last two fields belong to `characterClothController` |
| `Clothes.cs`: `phMapData` is 0x190 | 0x188 |
| `Sequence.Unused_08h` is unused | `m_CompactSize`, the compression flag ([YCD](../extra/YCD_File_Structure_Specification.md)) |
| `AnimationBoneId.Unk0` is unknown | `Dof::m_Type`, the format type that decides channel grouping |
| `Frag.cs`: `FragGlassWindow` is 0x70, and 0x1A0 for Gen9 | 0x40 in both, measured on multi-pane carriers `openrage-cli gaps` found in both installs |

## Checklist

- [ ] Versions censused over both installs, and a target read from each file's version
- [ ] A module in `crates/io/or-io/src/gta5/` with owned `#[record]` declarations: source names, no `unknown_*`, no written offset
- [ ] `impl Resource` on the root, a `FORMATS` row per target and version, and `validate` for every invariant the engine assumes
- [ ] A synthetic suite in `crates/io/or-io/tests/` and a corpus suite in `crates/io/or-io-verify/tests/`, every gate with its falsifier
- [ ] `just test` run with the corpus staged and no corpus suite skipped; figures in R66d
- [ ] `FormatHandler`, `PayloadHandler`, `FormatCapabilities` row, a `kinds.rs` row if named; `convert`, `inspect` and name completion
- [ ] The specification in the checkout's docs/extra/, and the roadmap item moved
