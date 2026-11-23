# The shader definition database

What a material's parameters are (names, types, defaults, samplers and render
bucket), extracted from an install of either edition rather than kept by hand.

A drawable stores a material as a shader hash and `(hash, register, bytes)`
entries, and nothing in it says that `0xF1FE2B71` is the texture
`DiffuseSampler`; the game knows from the `.fxc` it loaded. The database is
`crates/io/or-io/src/gta5/shaders/shader_defs/`, in `or_io::gta5::shaders`
with the `.fxc` reader behind the `fxc` feature, and
`crates/io/or-io/src/gta5/shaders/shader_defs/materials.rs` reads the
materials out of the owned drawables, fragments and particle lists.

## Inputs

A retail Legacy install's mount holds 375 effects under `shaders/win32_40_final/`
and 415 presets under `shaders/db/` (`8d0f0dce` with the mount read): `common.rpf`
holds 321 and 391 of them, and `update/update2.rpf` adds 54 effects and 26
presets and replaces 314 of the 321 effects, so both are read through the mount, not
from `common.rpf` alone. They are read by
`crates/io/or-io/src/gta5/shaders/fxc/grc_effect.rs` and
`crates/io/or-io/src/gta5/shaders/sps.rs` and fetched by
`crates/title/or-title/src/install/shader_defs.rs`.

⚠ Shaders are keyed by `atStringHash` of the preset's name, which is what
`GrmShader::effect_hash` carries, and 124 of the 415 presets (`8d0f0dce` with this change) bind
an effect spelled differently: `cutout` is `default` with its draw bucket pinned to 3. A
database keyed by effect misses those materials and loses the bucket.

An Enhanced install ships neither `.fxc` nor most `.sps`. Its effects are two
containers in `update/update2.rpf`, `common/shaders/fxdb/sga_win32_60_final.awc`
and its `_init` sibling, holding 388 and 9 effects over 12,405 and 259
compiled programs (`01592c10`), read by `crates/io/or-io/src/gta5/shaders/sga.rs`. Beside
them are 26 `.sps`, of the 411 the install's own `shaders/db/preload.list`
names; 119 of the missing presets share no effect's name either. The `common:/`
mount reaches `update2.rpf` (`build_vfs_for_install`), and
`crates/title/or-title/src/install/shader_defs.rs` reads both through it when
the target is `gen9`.

⚠ The container states no vertex type, no effect-level draw bucket and no
annotation name: annotations and properties are keyed by a hash that is not
`atStringHash` of `UIName` or `__rage_drawbucket` in either case, so a Gen9
entry carries vertex type 0, a bucket only from its preset, and no `UIName`.

## Building and checking

```bash
or-shader-tool defs build --input <INSTALL-OR-DIR> --out <FILE> [--title T] [--target T]
or-shader-tool defs show  <FILE> [SHADER]
or-shader-tool defs find  <FILE> <NAME-OR-0xHASH>
or-shader-tool defs check <FILE> <YDR|YDD|YFT|YPT|DIR>...
or-shader-tool defs diff  <BEFORE> <AFTER> [--title T] [--target T]
```

- `--input` is an install root or a directory of loose `.fxc` and `.sps`; an
  install's NG keys come from `OPENRAGE_KEY_SNAPSHOT` or else the bundled key
  snapshot ([Keys](../direction/OR_IO_Specification.md#keys-d4)). `--target gen9` reads an
  Enhanced install's containers instead of `.fxc`.
- `build` exits non-zero on an effect it cannot parse or read to the end: a
  `.fxc` has no table of contents, so only leftover bytes show a misread table.
  The layout follows CodeWalker's `FxcFile.cs`; this tree has no specification.
- The output, `*.shaderdefs.json`, is game content and gitignored;
  `OPENRAGE_SHADER_DEFS` points the editor and the corpus suites at one. A
  schema-1 file, which lacks `semantic_hash`, is refused rather than read short.
- `diff` takes a `.json`, install root or loose directory on each side, keys
  parameters by name (one insertion shifts every register after it), reports a
  header difference as a row, prints `no change` when nothing moved, and
  exits non-zero on any change.
- `check` reads the materials of real files of both targets through
  `materials_in`, the layout picked by each file's RSC7 version, and exits
  non-zero on residue: a hash another shipped effect declares means a material
  names the wrong shader, and one no effect declares is drift that reading
  more effects cannot fix. An Enhanced material is keyed by its `name` hash,
  and every one of its parameters other than a constant counts as a binding.

## Name, then semantic

A `.fxc` variable has a declared name and a semantic, and a file may carry
either: `ptfx_sprite` declares `wrapLigthtingTerm`, typo included, with the
semantic `WrapLightingTerm`, which a `.ypt` binds. 12,395 of a retail Legacy
install's 34,641 parameters hash differently under the two (`8d0f0dce` with this change), so
`ShaderDef::parameter` tries the name, then `semantic_hash`.

⚠ The order is load-bearing: `gCSMShadowTextureSamp` states the semantic
`gCSMShadowTexture`, which is also a variable's name, so a semantic-first
lookup answers with the sampler. No shader declares a semantic twice.

Writing picks one spelling per target, in `Spelling::of`
(`crates/io/or-io/src/gta5/shaders/shader_defs/remap.rs`): Gen8 the name, as all 9,871
resolvable drawable entries in the staged corpus carry
(`a_drawable_hashes_a_parameters_declared_name_and_not_its_semantic`,
`e696cece`), and Gen9 the semantic. Gen9 keys a material by the same
preset-name hash, so this database names Gen9 materials too.

## What an entry holds

Per shader: preset name and hash, effect, draw bucket, vertex type and trait
flags. Per parameter: name, semantic where it differs, their hashes, type,
array length, register, constant buffer, `UIName`, and two defaults, `default`
from the effect and `preset_default` from the `.sps`, because resetting to the
file and to the preset are different commands. `ShaderParamValue` is tagged:
untagged, a sampler's small integers (`FogRaySampler`) came back as floats.
Type codes 17 to 20, CodeWalker's `Unused1` to `Unused4`, are used by 3,112 of
the 34,641 parameters (`8d0f0dce` with this change), each with a default as wide as the code
fifteen below or none; `ShaderParamKind::Unknown` keeps the code.

An Enhanced entry's constant register is the 16-byte row its byte offset falls
in, its arrays reach 1,280 elements (`01592c10`), and a sampler is kind `sampler`. ⚠ The
container numbers types on its own scale, so an `Unknown` code means nothing
across targets.

## Coverage and limits

Over the staged Gen8 corpus, 128 files and 2,813 materials, the database names
47,683 of 47,795 parameter entries, every material's shader and every texture
binding (`crates/asset/or-asset/tests/a_material_names_its_parameters.rs`, at
`e696cece`). Another shipped effect declares five of the six residue hashes;
none declares `0x6D16F26D`, bound 51 times by 12 shaders beside
`g_Puddle_ScaleXY_Range`.

Built from a retail Enhanced install, the Gen9 database holds 398 shaders
and 48,710 parameters. Over the staged Gen9 corpus, 14 files (5 `.ydr`, 2
`.ydd`, 5 `.yft`, 2 `.ypt`) and 1,704 materials, it names 48,498 of 48,741
parameter entries (`or-shader-tool defs check`, measured when the reader
landed on `f154c77d`). ⚠ The residue is the materials' samplers: 154 entries
bind `DiffuseSampler`, `TrilinearClamp` and three more samplers that only another
effect declares, because an Enhanced `default` reaches its samplers through
`gDynamicSamplerIndex_*` constants rather than declaring them; 89 texture
entries over 3 hashes no effect declares. Reading the containers is
[R40e](../direction/Roadmap.md#r40e-the-enhanced-shader-container).

## Editing a material

- ⚠ **Gen8 texture slots are a prefix** whose length `texture_count` states,
  so a sampler appended after the constants writes a file OpenRage reads
  back perfectly and the game misreads. `GrcInstanceData::insert_entry` puts
  a sampler at the end of the texture run and a constant last, and a save
  writes the count from the list; all 977 corpus materials stating a count
  agree with it, and every edit is written and re-read on every staged `.ydr`
  (`crates/io/or-io-verify/tests/drawable.rs`,
  `e696cece`). A file that already states a sampler after its constants, as
  add-on ped garments from some tools do, opens with its textures moved ahead
  of them and saves as the game reads it.
- `as_float_registers` refuses an integer default rather than write `3.0` where
  the shader may read the bits of `3`: 1,049 of a retail install's 24,840
  non-binding parameters declare one, all `int`, `int4` or `bool` (`e696cece`).
- A Gen9 material states one count per region and reaches each by an explicit
  index, so it has no prefix. A save writes the counts from its lists
  (`a_materials_counts_are_its_lists_lengths`) and refuses a parameter whose
  slot or range lies outside its block.
- `src/ui/panels/inspector/material.rs` edits Gen8 materials.
  `src/ui/panels/inspector/gen9.rs` names each Gen9 parameter in both spellings
  and edits constant-buffer values, but does not re-point textures, edit
  sampler state or add a parameter, though a Gen9 database now lists what an
  effect declares.

## Where the code is

| | |
|---|---|
| `crates/io/or-io/src/gta5/shaders/shader_defs/mod.rs` | the database, its JSON form and loader |
| `crates/io/or-io/src/gta5/shaders/sga.rs` | Enhanced's shader container |
| `crates/io/or-io/src/gta5/shaders/shader_defs/diff.rs`, `coverage.rs`, `materials.rs`, `remap.rs` | `defs diff`, `defs check`'s residue split, every material an owned file declares, the per-target spelling and a material in the shape the remap derivation compares |
| `crates/title/or-title/src/install/shader_defs.rs` | fetching the effects and presets out of an install |
| `crates/tools/or-shader-tool/src/defs.rs` | the `defs` commands |
| `src/state/shader_defs.rs` | loading a database into the editor, and saying so when there is none |
| `src/state/document/material_baseline.rs` | each entry's value when the document arrived, for resetting to the file |
