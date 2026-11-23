# The compiled readers' testimony

What the compiled readers read and the compiled writers wrote, how a layout is
checked against it, and what has to hold before a compiled reader is deleted.

## The record and where it lives

`openrage-cli testimony record` ran every compiled reader over the corpus and
both retail installs, writing down every field of every struct instance it read,
at its offset, and each compiled writer's output fingerprint
([R65c](../direction/Roadmap.md#r65c-record-the-oracles-testimony), #192). R65c is closed
and the command, with its `check`, is deleted: nothing new is recorded. Most of it was
located by compiled layouts rather than a description, so it checks a layout
after [R66f](../direction/Roadmap.md#r66f-delete-the-old-crates) deletes the compiled readers,
naming the file, type, instance and field a moved offset misreads. It is derived
game content and lives in `openrage-corpus/testimony` (79 files, 13.5 GiB on
2026-09-25), never here, by the owner's exception of 2026-09-24, which covers it
alone ([Project_Context.md](../direction/Project_Context.md)); the metadata schema snapshot
store also sits in `openrage-corpus`, with no ruling yet. Do not move the record
or repoint the gates, which read every `testimony/corpus*` directory beside the
staged corpus; `OPENRAGE_TESTIMONY` names another copy for a one-off check.

⚠ Each record names the commit that took it, and those commits are on neither
`main` nor origin: `agent/r65c-testimony-recorded-at` holds `35292167` and
`933ca2b4`, `agent/r65c-testimony-cda05819` holds `524f73e4`, and
`agent/r65c-testimony-bc30479a` holds `adf98b68`; the last two are checked out
as lanes under `openrage-lanes/`. Deleting one of these branches loses the code
behind a record; pin a recorder on a branch before rebasing it.

## How it was taken

`openrage-cli testimony record` took `--out` (the testimony directory),
`--corpus` for the staged corpus and `--game <label>=<dir>` per install, which
it read read-only, one file at a time, streaming each file's record to disk.
Every `#[rage_struct]` that was not `layout_only` registered its compiled field
table (name, `offset_of!`, width, type) with the capture in or-io's
`prim::testimony` module, deleted since. While a capture was open on the thread,
`DatResource::read_at_offset` reported each compiled read, walking the embedded
structs and fixed arrays a `BinRead` took in one go, and the generated
`read_raw_bound` reported each pack read; with none open, an ordinary read cost
one thread-local flag.

A compiled reading outranks a pack reading of the same instance, being
independent of the pack under check. A `pack_bound` type testifies as compiled
because a build with `debug_assertions` re-reads it at `offset_of!`, so
recording refuses any other build. A `pack_only` type has no compiled layout,
so its testimony is a pack snapshot: it catches drift from the day it was
taken, not a description already wrong. Every instance is marked with its
witness and the check counts the two apart. The record cannot see a reader that
parses a cursor by hand, a type only written, or a field of a file its reader
refused.

Each source gets a directory holding a `summary.json` of counts and one gzip
stream per format and target (`ydr.gen8.testimony.gz`); recording the same
files twice gives the same streams bar the `started` line. A stream opens with
its attribution: the revision and whether its tree was clean, the entry points
that ran, each pack's id, format version, hash and `built_by` stamp, the install's executables, and every
compiled layout. Then one block per file:

```text
file <origin>/<archive chain>/<path>
at <ext> <target> <rsc7 version> <bytes> <sha1> <virtual size>
read ok
i <Type> <offset> <c|p> <field>=<hex> <field>=<hex> ...
write compiled ok <bytes> <file sha1> <payload sha1> <pages sha1>
write generic ok ...
end
```

An offset is into the decompressed image, virtual pages then physical; a value
is the bytes as the file holds them. A write is fingerprinted three ways: the
file, the RSC7 header plus the decompressed pages, and the pages alone. The
second decides a match, since it does not depend on the deflate implementation;
the third tells a writer that packed the same allocations differently from one
that wrote different allocations.

## Checking

`crates/io/or-io-verify/src/testimony.rs` holds the record reader, the `Reading`
trait and `check_file`, and nothing in it depends on the capture. The suites
answer through `DeclaredReading`, from the `or-io` declarations; `PackReading`,
which answers from a pack's offsets over the image, only the module's own tests
use. A field agrees; differs (other bytes, another width, or past the
image), which fails; is corrected; is padding (zero bytes past a record's end,
up to its 16-byte alignment, that no record claims); or is a gap (no field of
that name, or no such type), printed per site and never dropped from the
denominator.

A correction answers a field from where the recording reader read it and still
holds it to the record: the `.ytd` suite's `MOVED` places the Gen9 texture's
view at 0x58, where every file holds it, though the record read 0x50 through
the pack of the day.

The install records are checked by
`crates/io/or-io-verify/tests/the_install_testimony_holds.rs`, through the
declarations and the frozen key as the corpus records are, with the same
Enhanced texture view correction. It runs when
`OPENRAGE_GAME_ROOT` names an install, and checks it against the records
whose headers list its executables, `at-bc30479a/` included: each recorded
file is taken again from the install by its key and must still have the
recorded SHA-1. A file that changed, moved or no longer opens fails, as does
one the declarations refuse unless its `REFUSED` list names it with why, and
any recorded field no declaration reaches, tiles or describes; a record the
compiled reader refused with nothing recorded has nothing to ask. `.ysc` has
no declared format, so its streams are left out and named.

## The gates

`crates/io/or-io-verify/tests/the_recorded_testimony_holds.rs` runs on
`layouts` for every pull request from this repository and every push to `main`,
though no merge waits for it, and records a skip where no record is staged. It
holds the records as a whole, through the built-in descriptions and the frozen
key: every recorded file is still in the corpus with its recorded SHA-1, still
an RSC7 resource, and read by a declared description
(`every_recorded_file_is_still_read_through_a_declared_description`, because a
record cannot be retaken); one misread field is named in exactly its file
(`a_reader_that_misreads_one_field_is_named_in_that_file`); and the census
(`every_described_scalar_fits_its_declared_kind`) asks each value whose
recorded type and field a built-in description names whether it fits its
declared kind: a `bool` is 0 or 1, an enum a variant, a float finite. The
census needs no second reader. Through the built-in descriptions it asked
2,640,209 values at 150 sites over the three corpus records (P62, at
`eb500483`), fewer than the 4,444,566 at 178 sites the private pack's names
reached at #192, and every misfit sits at one of the sites `RULED_MISFITS`
names with a reason: NaN sentinels and packed integers in vector lanes and a
polygon slot that is a union. A misfit anywhere else fails. Each field is read
back by its format's suite, below.

Each declared format's suite in `crates/io/or-io-verify/tests/` pairs a test like
`the_yfd_testimony_holds_through_its_declarations` with the falsifier
`a_declared_field_one_byte_late_is_named_by_the_testimony`, on the shared half
in `crates/io/or-io-verify/src/gates.rs`
([R66d](../direction/Roadmap.md#r66d-declare-gta-v-format-by-format)). `DeclaredReading`
locates a field by offset and width alone, so no recorded name reaches the public
crate, and a row fails on any recorded field unreached, untiled or undescribed,
except one the padding rule answers.
A named `FieldMoved` or `Relocated` excuses a known move and is still held to
the file's bytes; the rules are in
[OR_IO_Specification.md](../direction/OR_IO_Specification.md#the-recorded-testimony). These
suites find their records beside a staged corpus under `just test`, and
`layouts` runs them with its corpus staged.

## What checks a layout, and what does not

Only the testimony compares a description against bytes no description produced.

| Check | Compares | Does not see |
|---|---|---|
| `#[record]`'s compile-time proof and `declared_records` | a declaration's description against `offset_of!` and `size_of!` on the struct it generates | a declaration wrong about the file, which its description agrees with by construction |
| `require_coherent` | a description against its own arithmetic | a coherent misreading |
| A round trip, the canonical fixed point included | a file against itself through one description | a consistent misreading, which cancels: a Gen9 round trip stayed byte-for-byte green under two coherent, wrong gen9 packs |
| Each format's `validate` | the fields of one value against each other | a moved pointer that lands on a plausible run |
| The testimony | a description against recorded bytes, per field, scalars included | a file never recorded; a type already `pack_only` when recorded, beyond its snapshot |

⚠ Detection follows a field's kind, not how wrong its offset is. A moved physical
pointer makes every file refuse; a moved virtual pointer can land somewhere
plausible and bind, as swapping `RmcLodGroup`'s `bounds_box` and `lods` in the
gen9 pack left 5 of 5 Enhanced `.ydr` opening
(`a_moved_offset_in_the_lod_group_binds_and_only_the_compiled_reader_notices`);
and a moved scalar reads a plausible number nothing checks at read time.
Counted over both staged packs at `44913126`, the gen8 and gen9 packs described
5 and 3 physical pointers, 252 and 253 virtual pointers, and 957 and 966
scalars, inline structs and enums, out of 1,214 and 1,222 fields. No check
covers a moved offset between two scalars of a type only one pack describes, on
an unrecorded file; `the_gap_is_named_and_still_open` exercised it.

## When a compiled reader may be deleted

A check that replaces a compiled reader needs three properties:

1. It runs automatically where the code is, on every change.
2. It is falsifiable per field: move one offset and it names that field.
3. It reaches scalars, through a field-level expectation against known bytes.

The testimony has the second and third for every recorded file, and the first
where `layouts` runs `the_recorded_testimony_holds`. So each compiled reader
stayed in the tree until the declarations replacing it held its testimony, and
[R66f](../direction/Roadmap.md#r66f-delete-the-old-crates) deleted them all (P62, #332):
deleting removed the only independent side a differential could have, and the
testimony is what stands in for it.

⚠ Converting a graph to `pack_only` spent its differential, leaving no compiled
layout to be the independent side. A type testified independently only while
it kept a compiled layout, and two things had spent that before the record was
taken. `pack_only`, from `88b30a3f`: the drawable spine, `GrmShader`, `GrcTexture`, the `.ybn` bound headers, the
particle rules, the cloth graph and the clip headers. And
[R64](../direction/Roadmap.md#r64-move-the-containing-readers-onto-pack-offsets-format-by-format)
re-declaring as it moved: `.ynd` (#171) rewrote its field declarations and
`.yed` (#179) turned its spring entries into opaque runs, while `.yfd` (#175)
and #187 (`.yld`, `.ycd`, lights) changed none. The older records below recover
the compiled side for both.

`tools/cw-compare` is a diagnostic, not a gate: it needs Windows, the .NET 8
SDK, gitignored fixtures and the `external/CodeWalker` submodule, and CodeWalker
misreads `.ycd` in documented places. It answers whether CodeWalker loads what
OpenRage wrote.

## What the records say

Checked at `844d3c7c` against the packs exported from `4d025e11`, the corpus by
`the_recorded_testimony_holds` and the installs by `openrage-cli testimony check`:

| | Corpus | Legacy install | Enhanced install |
|---|---:|---:|---:|
| Recorder | `35292167` | `933ca2b4` | `933ca2b4` |
| Streams | 18 | 14 | 16 |
| Files recorded | 2,500 | 314,725 | 314,310 |
| of which read by their compiled reader | 2,496 | 314,715 | 314,305 |
| Fields recorded | 79,971,579 | 1,861,825,223 | 1,876,010,650 |
| of which pack snapshots | 3,149,669 | 126,972,403 | 136,637,492 |
| Agreed | 79,753,696 | 1,804,070,864 | 1,767,611,125 |
| Differed | 0 | 0 | 0 |
| No field of that name | 86,667 | 55,765,975 | 55,742,377 |
| Type not in the pack | 131,216 | 1,988,384 | 52,657,148 |

| Older record | Recorder | Covers | Fields | Agreed | Differ |
|---|---|---|---:|---:|---:|
| `corpus-cda05819` | `524f73e4` on `cda05819`, the last revision with no `pack_only` type | `.ybn .ycd .ydd .ydr .yft .yld .ypt .ytd`, 292 of 300 files (an Enhanced `.yft` or `.ypt` already needed a pack) | 6,657,219 | 6,506,273 | 0 |
| `corpus-bc30479a` | `adf98b68` on `bc30479a`, before `.ynd` and `.yed` were re-declared | `.ynd .yed .yfd`, `.yed`'s springs field by field; the installs' under `at-bc30479a/` | 4,675,411 | 4,672,543 | 0 |

The rest of every row is named gaps, so the pack agrees field for field with an
independent compiled reading of every type it made `pack_only`. The corpus's
four unread files are three `.ybd` and one `.ytd` at a version their reader
refuses; each install holds one `.ytd` that does not decompress. Enhanced's
larger no-type count is four Gen9 records the gen9 pack did not describe: the
shader parameter and resource-view records and the vertex declaration. Every
`.ybn`, `.ycd`, `.yld`, `.ynd`, `.ynv` and `.ysc` the Enhanced install ships is
Gen8, so the record holds the read rows
[R65c](../direction/Roadmap.md#r65c-record-the-oracles-testimony) lists, less the Gen9 rows of
`.ybn`, `.ycd` and `.yld`, which no shipped file can fill. The Legacy record
includes the install's `mods_` user archives, keyed `legacy/mods_/`. The generic
writer's pages matched the compiled writer's on none of the corpus files both
wrote, so value equivalence and the canonical fixed point, not byte identity,
are the write gate.

Through the declarations and the frozen key, `the_install_testimony_holds` at
`c5a52074`, on `main` at `395de759`, printed the same table with the pack staged
and with `OPENRAGE_PACKS=none`, with `OPENRAGE_GAME_ROOT` alone set:

| | Legacy install | Enhanced install |
|---|---:|---:|
| Streams, `at-bc30479a/` included | 16 | 18 |
| Records checked | 314,840 | 313,470 |
| Fields recorded | 1,869,898,411 | 1,879,401,052 |
| Agreed | 1,869,895,962 | 1,878,847,895 |
| Corrected (the Enhanced texture view) | 0 | 550,708 |
| Padding (the `.ynv` allocation padding) | 2,449 | 2,449 |
| Unreached, undescribed or unanswered | 0 | 0 |
| Differ | 0 | 0 |
| Files the declared read refused and set aside, at that revision | 1 | 42 |

`REFUSED` names a set-aside file by its install path, with why
([OD-P45-SETASIDES](../direction/Project_Context.md#the-install-testimony-sets-aside-what-the-declarations-cannot-yet-read-2026-09-28)),
and a listed file that reads fails the run. A set-aside file's fields are not
asked. At the revision of the table the list held 43 files. The 42 retail Gen9
`.ypt` files, in which a behaviour's `keyed_values` landed outside its
`Behaviour`, left it when the Enhanced `.ypt` was declared with the class they
hold. The other was the Legacy install's user mod,
`mods_/ped_edits/eye/mp_eye_colour.ytd`, whose texture states 0x2b400 payload
bytes and ships 0x20000 of physical pages, and which the declared read now
takes: the file, with the size and SHA-1 the record names, was read through the
declarations on 2026-10-09 (`desc::value::read`, the call this suite makes) and
read, so a listing of it would fail the run, and it left the list. `REFUSED` is
empty. The install run itself was not repeated for that change: no install is
mounted where it was made. The staged-mod check
`a_mod_texture_that_states_more_payload_than_its_pages_ship_reads_through_the_declarations`
holds the same bytes once the file is staged in the corpus's `mods/`, which a
lane cannot do. `.ysc` is left out, since it has no declared format.
