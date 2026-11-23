# OpenRage I/O: the `or-io` crate

The design of `or-io`, the public crate that declares, reads, edits and writes GTA V's file formats for the editor and for external programs, and the owner's decisions D1–D18 it rests on.

Status: adopted on 2026-09-24; built through R66f, and R66i done; R66g and R66h open — [R66](Roadmap.md#r66-the-or-io-crate) tracks every phase and format.

## The design

- **One crate, `or-io`, in this repository**, with its proc-macro companion `or-io-macros` and the unpublished checks in `or-io-verify`. It holds GTA V's format knowledge: the declaration of every structure, and the reading, writing and editing of every RSC7 resource format, the RSC7 header, RPF7 and GXT2. DDS, FXC, AWC, `.rel` and `.gfx` are ordinary modules behind features, and metadata (`.ymt`, `.meta`, `.ymap`, `.ytyp`) is `or_io::meta` behind a `meta` feature (D5). An external program adds one dependency and never sees the editor.
- It replaces the crates that described, read and wrote files: `or-rage-core`, `or-rage-types`, `or-format-api`'s pack half, `or-pack-build`, `or-pack-io` and `or-container` merged in R66c; `or-meta` moved in whole during R66d (D5) and `or-asset` was deleted in R66f (D10, P62, #332), with `or-macros` and `#[rage_struct]`'s four code generators (P66); the private pack is retired rather than re-authored (D17).
- Names are the engine's (D15): types, fields, enums and values are named as the GTA V source declares them, in Rust casing (`GrcTexture`, `RmcLodGroup`, `vertices_count`), and field order, types, padding and inheritance follow the source, cross-checked against CodeWalker's binary layout and the corpus. Offsets are computed, never written, and no migrated declaration has an `unknown_*` field.
- One declaration, written in owned domain types, makes the owned value a user opens, edits and saves (D16), its raw `repr(C)` layout with an opt-in zero-copy peek, and the built-in description the generic machinery — inspector, plugins, canonical writer, testimony — runs on. The `.orpack` format stays, as an export and for titles the crate does not declare. Superseded on 2026-09-26 by [OD-OTHER-TITLES](Project_Context.md#no-runtime-pack-for-another-title-2026-09-26): it stays as an export only. Superseded on 2026-10-01 by [the export's removal](Project_Context.md#openrage-packs-is-deleted-and-the-orpack-export-goes-2026-10-01): the export goes.
- The suites are the proof of a layout: typed reads against the generic tree, the recorded testimony re-keyed by offset, the canonical fixed point, the alignment census and the structural diff against the private pack. There is no provenance attribute and no leak-name denylist (D15). The structural-diff clause superseded on 2026-09-26 by D17: the proof is the typed reads, the testimony, the fixed point and the alignment, padding and variant censuses, and [gate 6](#the-gates-per-format) retires format by format.
- Keys are extracted from the user's own executable (D4), and the NG keys no executable carries come from CodeWalker's key snapshot, which `or-io` bundles behind `bundled-keys` and which decodes only with the key from the user's executable (D18).
- The pack-native design's machinery — type plans, records, packed runs, member pointers, generic layouts and the canonical writer — is the storage and the generic path under the typed API.
- Publication waits on a rewrite of this repository's history (D3) and then publishes the crate and the editor together (D8). The lawyer questions are parked and gate only that last step (D14).

## Owner decisions

The owner's decisions for `or-io`, given on 2026-09-24 (D1–D14), 2026-09-25 (D15, D16), 2026-09-26 (D17) and 2026-09-28 (D18), with D12 taken on 2026-10-01, and the times he gave them where he did. An entry is not rewritten; one a later decision overturns says so. The project's other dated decisions are in [Project_Context.md](Project_Context.md).

| # | Decision | Effect here |
|---|---|---|
| D1 | **Supersede** "no RAGE structs" (2026-09-23) and the private pack holding the names. | The design is adopted: typed public declarations, and `openrage-packs` stops being the layout and name authority ([descriptions](#descriptions)). Its naming half, OpenRage's own names, superseded on 2026-09-25 by D15. |
| D2 | **The crates are `or-io` and `or-io-macros`, taken on crates.io now.** At 15:02 the owner chose `openrage-io`; at 15:07–15:10 he changed it to `or-io`. Both names were free on crates.io when he checked at 15:09 (crates.io treats `-` and `_` as the same, so `or_io` is the same name and also free). Published crates and internal editor crates both use the `or-` prefix. | [The name](#the-name). The owner reserves the names; nothing is published before P7. |
| D3 | **Publish on this repository**, not a fresh one. Its history is rewritten to remove comments and documents about leaked code. | [Rewriting the history](#rewriting-the-history) and P6; lawyer question 8. |
| D4 | **Keys come from extraction from the user's own game executable**, not from user-supplied key files only. No keys ship in the repository or the crate. | [Keys](#keys-d4); lawyer question 4. "No keys ship" superseded for the key snapshot on 2026-09-28 by D18. |
| D5 | **Metadata goes into `or-io`** (15:07–15:10, in the owner's words ".ymt and .meta should probably be put in" the crate). | `or-meta` becomes `or_io::meta` behind a `meta` feature, with no schema file bundled ([metadata](#metadata)). |
| D6 | **Engine names: keep them for now as they are**; the optional display overlay stays. | Superseded on 2026-09-25 by D15: the engine names are the public names, and no overlay is built. |
| D7 | **"We have permissions"** (15:07–15:10): permission from the authors of the CodeWalker- and CodeX-derived code is held. At 15:36: no written grants are needed ("Not needed ignore"). | The ported code stays. `LICENSE-third-party.md` names the ported modules in an attribution entry and quotes no grant. |
| D8 | **The crate and the editor are published at the same time.** | P7 publishes both together. |
| D9 | **The particle simulation ships with 100% fidelity to GTA V.** It is not re-derived and not cut. | Kept as it is; the highest legal-risk item and lawyer question 3. |
| D10 | **`or-asset` merges into `or-io` in full and is deleted** (15:07–15:10). | [The `or-asset` module map](#the-or-asset-module-map). |
| D11 | **Texture authoring is `or_io::authoring` behind an `authoring` feature, plus `bc7-encode`** (15:17, confirming the recommendation). | The module map and [features](#features). |
| D12 | **Engine-named code in old revisions: deferred to the history rewrite (P6).** In the owner's words: "We'll see later but i guess when we rewrite history we will probably try to neutralize a bit more the types but later." (15:17) | Taken on 2026-10-01: old revisions keep the game engine's names when the history is rewritten, in the owner's words "not codewalker names and not neutral names" ([recorded](Project_Context.md#old-revisions-keep-the-engines-names-2026-10-01-d12)). P6 renames no identifier in an old revision, and the intent to neutralise more of them is superseded. |
| D13 | **D9 reaches the renderer's fallback shaders, the VFS device code and the particle reset values** (15:17): they are kept as they are, at full fidelity, like the particle simulation. | Lawyer question 3. |
| D14 | **The lawyer questions are parked** (15:36, "ignore the lawyer questions for now"). | [Questions for a lawyer](#questions-for-a-lawyer) gate P7 only. |
| D15 | **2026-09-25, 05:48 and 05:54: declarations follow the GTA V source.** In the owner's words: "we have the gta source code … you need to derive the structs layout from it, you can compare the binary layout of the struct with codewalker but the names of fields, types and all should be based on gta source code truth … we should not have to write the offsets of fields this should be auto calculated with correct fields placements and padding as in any rage structs we implemented before with the rage_struct macros, we should have no 'unk' fields"; and of the provenance attribute, "remove this we dont care". | Supersedes D1's own-names rule, the functional renames, the `unknown_*` convention, D6's overlay, the provenance attribute, the provenance lint's engine-name rule and the leak-name denylist. [Names come from the source](#names-come-from-the-source). |
| D16 | **2026-09-25, 06:45: the target developer experience.** The owner approved the calls in [the target](#the-target-d16) — `GtaDrawable::open(path)`, plain field reads and assignments, `Vec` operations for structural edits, `save()` / `save_as()`, `or_io::open` into an `Asset` enum, `Gta5::find()` / `gta.open()` / `gta.save_to_mod()`, `to_png` / `from_png` / `mesh()`, and typed metadata such as `Ymap::open` — and asked that the backend be adapted to them and every format migrated once, to that shape. | A declaration is written in owned domain types ([the owned form](#the-owned-form-d16)); the opened file is an ordinary owned value, errors come only at open and save, and engine type names are the API with no aliases or wrapper types. The view, the record and the generic tree stay, as the peek path and for the inspector. Amended on 2026-09-26 by [OD-10.8](Project_Context.md#drawables-are-one-generic-declaration-per-class-2026-09-26), taken under the owner's standing instruction for R66: drawables are one generic declaration per class, `GtaDrawable` generic over `DrawableTarget` with `Gen8` as the default; GtaDrawable::open opens a Gen8 drawable and refuses a Gen9 one by name, pointing at or_io::open; a caller that does not know the target matches or_io::open's `Asset` for either drawable variant; GtaDrawable::\<Gen9\>::open is the typed Gen9 call; field access is unchanged ([the target](#the-target-d16)). |
| D17 | **2026-09-26: `openrage-packs` is retired.** In the owner's words, "I agree with your recommendations on the decisions", answering the plan whose goal he set on 2026-09-25: everything into `or-io`, one macro crate, every crate no longer needed deleted, and `openrage-packs` removed completely. Seven rulings were taken with it (OD-PACKVER, OD-TESTIMONY, OD-BASELINES, OD-RUNTIME, OD-REFS, OD-CONCURRENCY, OD-LANES), each [recorded](Project_Context.md#openrage-packs-is-retired-2026-09-26) in Project_Context.md. | GTA V has one description, the built-in one. The proof of a layout is the typed reads, the testimony, the fixed point and the alignment, padding and variant censuses; [gate 6](#the-gates-per-format) retires per format at its owned migration; the testimony's pack-witnessed fields are read through a frozen key; `PACK_VERSION` stays 12; GTA V pack discovery is deleted and `.orpack` stays as an export. Supersedes D15's "pack conformance" clause, the structural-diff clause of [the design](#the-design)'s proof line, and R66f's and P5's plan that the pack stays as the structural diff's second description; answers [what the pack is written in](#open-questions-for-the-owner): nothing. [R66i](Roadmap.md#r66i-retire-openrage-packs) retires the repository, and R66h waits on it. "`.orpack` stays as an export" superseded on 2026-10-01 by [the export's removal](Project_Context.md#openrage-packs-is-deleted-and-the-orpack-export-goes-2026-10-01); the owner deleted the repository the same day. |
| D18 | **2026-09-28: `or-io` ships the key snapshot where archives are opened.** In the owner's words: "its a key that needs a pattern of the game executable of user to get it so thats fine we should ship it", and on where: "We should put it in what opens rpf i think". | CodeWalker's snapshot, masked and encrypted under the game's AES key, ships as `crates/io/or-io/data/magic.dat` behind the default-on `bundled-keys` feature ([keys](#keys-d4)); a snapshot of the user's still comes first. The published crate will carry it permanently; lawyer question 4 still gates P7 (D14). Supersedes D4's "no keys ship" for the snapshot, R66b's removal of it from the tree and R66g's removal of it from history. |

## Scope and non-goals

The crate exists for four things:

1. **One code path for the editor and external programs.** A batch converter, a web viewer, a map tool and OpenRage itself depend on the same crate, and none of them needs egui, wgpu, a layout file or a corpus.
2. **The format knowledge in one place.** A layout is declared once, in Rust, and that declaration is the owned value, the typed view, the editable record and the machine-readable description.
3. **Distributable.** Everything in the crate can be published once the cleanup in [legal and provenance](#legal-and-provenance) is done.
4. **Fast and native.** Real fields, autocomplete, a compile error for a misspelt field, zero-copy reads on request, and a deterministic save.

| Family | Formats | How | Where it stands |
|---|---|---|---|
| RSC7 resources | every paged format the corpus holds | declared, both targets | every format `FORMATS` in `gta5::formats` lists; `.ysc` parked with no corpus, and ordinary code under a time-limited exception until one is staged ([OD-YSC](Project_Context.md#ysc-is-ordinary-code-until-a-corpus-exists-2026-09-26)) ([R66d](Roadmap.md#r66d-declare-gta-v-format-by-format)) |
| Containers | the RSC7 header, RPF7 | declared headers and entry tables; compression, encryption and page arithmetic stay code | the RSC7 header is declared owned under the source's names (`DatResourceFileHeader`, #241); RPF7's header and entry table are declared (`FiPackHeader`, `FiPackEntry`, #262), and its reader and builder in `container::rpf` are code |
| Text | GXT2 | declared, owned | `gta5::text`, `Gxt2` with no shim (#265) |
| Metadata | PSO, RBM, RBF and their XML canonical form | `or_io::meta` behind `meta`, schema supplied as data | merged whole as `or_io::meta`, which takes its schema as data (#243, #267) |
| Shader definitions | `.fxc`, `.sps`, Enhanced's SGA shader container, the shader definition database | `gta5::shaders`, `.fxc` and SGA behind `fxc` | moved into `gta5::shaders` (#270); `.fxc` off by default, matching this table; Enhanced's container is `gta5::shaders::sga` (#384) |
| Ordinary formats | DDS, FXC, AWC, `.rel`, `.gfx` | modules behind features | DDS, AWC, `.rel` and `.gfx` are `or_io::dds`, `or_io::awc`, `or_io::rel` and `or_io::gfx` behind `dds`, `awc`, `rel` and `gfx` (#373, `.gfx` #434); FXC moved into `gta5::shaders::fxc`, also behind `fxc` (#270) |
| Pose-matcher database | YPDB | ordinary code in the crate, not declared ([OD-YPDB](Project_Context.md#the-pose-database-is-ordinary-code-2026-09-26)) | `gta5::pose_db`, `PoseMatcherDatabase` (#254) |
| Move network definitions | `.mrf` | ordinary code in the crate behind no feature, not declared | `or_io::mrf`, `MvNetworkDef` (#446) |
| Data files read from an install | name tables, clip-region tables, material tables, HDR images, the terrain height map, distant lights, the water height map, the world blanket, the nav precalc, the streaming cache files, `metadata.dat`, the audio world sector grid | parsers in `gta5::data` and `gta5::hdri`; the binary world tables read and write in `height_map`, `distant_lights`, `water_map`, `world_blanket` and `nav_precalc`, and the cache files, `metadata.dat` and the sector grid in `cache_loader`, `social_club_metadata` and `audio_world_sectors`; nothing bundled | in `or-io` (#232); `height_map` and `distant_lights` (#428); `water_map`, `world_blanket` and `nav_precalc` (#441); `cache_loader`, `social_club_metadata` and `audio_world_sectors` (#447) |
| Authoring | BCn encode, mip chains, PNG, texture import | `or_io::authoring` (D11) | moved in, both editions' `to_png`/`from_png` (#268) |

Codecs in scope: vertex declarations and vertex decoding to a neutral mesh, index buffers, BCn decode, texture format tables, animation channel decoding and packing, the regions whose layout is computed rather than placed (a material's parameter blob, a skeleton's bone-tag table), DEFLATE for RSC7, and RPF7 decryption given the user's keys. A decode rule — what a vertex format code means, how a Rice-packed channel unpacks — stays code; it is an algorithm over bytes a declaration locates.

Out of scope:

- **No editor model.** Selection, undo, cameras, panels, previews and the particle simulation stay in the app and in the domain and runtime crates; the crate stops at "these bytes, this value, this record".
- **No install model.** Finding installs, DLC overlay order and the VFS stay in `or-title`, which depends on the crate. The crate opens a file, a byte slice or an archive entry.
- **No game data, no leaked code or citations, and no key material but the bundled key snapshot (D18)** ([legal and provenance](#legal-and-provenance)), and nothing that talks to a network.
- **No other titles yet.** The module layout leaves room for `rdr2`; a runtime `.orpack` gives a generic tree for a title the crate does not declare. Superseded on 2026-09-26 by [OD-OTHER-TITLES](Project_Context.md#no-runtime-pack-for-another-title-2026-09-26): no runtime `.orpack` is loaded, and a title gets the generic tree from its own declarations.
- **No byte-identity promise with shipped files.** The first save may move bytes; saving again is byte-identical (ruling 6 of 2026-09-24).

## Crates and layers

[Crate_Reference.md](../reference/Crate_Reference.md) describes each crate; this section is what `or-io` absorbs and where the rest goes.

### The name

The crates are `or-io` (library `or_io`) and `or-io-macros` (D2). Every crate in the workspace uses the `or-` prefix, published or not. The workspace sets `publish = false`; `or-io` and `or-io-macros` flip it at P7, and `or-io-verify`, `or-title`, `or-editor` and the rest stay internal until a decision says otherwise.

```toml
[dependencies]
or-io = "0.1"
```

### Layers

```text
foundation -> io -> title -> domain -> runtime -> app
```

`tools` sits outside the chain, and only the app depends on it. `crates/io/` holds `or-io`, `or-io-macros`, `or-io-verify` and `or-media`, which decodes the audio and video in them and is not part of the published crate; the asset layer is gone, its last crates deleted (`or-asset` in P62, #332; `or-ptx` in P70, #333; `or-format-api` in P63, #335). No edge points up: `or-test-support` dropped its dependency on `or-io` (P64, #334). Each move updates its importers in the same change; there is no re-exporting facade, so an import names the layer it needs.

### What merges into or-io

| Crate | Becomes | Where it stands |
|---|---|---|
| `or-rage-core` | the crate root: hashing, `IoError`, diagnostics, generations, address constants | merged in R66c |
| `or-rage-types` | `or_io::prim`, the binary primitives under their engine names (`PgPtr`, `AtArray`, `DatBase`, the math types) | merged; ruling 2 of 2026-09-24 kept them, and `prim`, the classic layer the compiled readers were written in, was deleted with its `binrw` paging (P65, #336) |
| `or-format-api` | `or_io::desc`: the pack format, type plans, records, `Image`, the value tree | merged; `names`, the vocabulary for consumers that still bind by name, stays in `crates/asset/or-format-api` and goes with its last consumer; the app no longer depends on it (#327), and `or-asset` and `or-title`'s tests still do |
| `or-pack-build` | `or_io::desc::export` | merged; its `inventory` registration and `build_pack` are deleted (P65, #336), and `audit` stays |
| `or-pack-io` | `or_io::write`: `build`, `patch`, `edit`, `compare` | merged; the testimony check went to `or-io-verify` |
| `or-container` | `or_io::container`: `resource`, `paging`, `rpf` with key extraction | merged |
| `or-macros` | `or-io-macros` (`#[record]`) | deleted in R66f with `#[rage_struct]` and `#[rage_enum]` (P66); `or-io-macros` is the one proc-macro crate |
| `or-asset` | everything, by [the module map](#the-or-asset-module-map) | deleted in R66f (D10, P62, #332), once its last importers, all tests, had moved onto the owned types |
| `or-meta` | `or_io::meta` behind `meta` | merged and deleted (#267, D5); its schema is data installed at run time, not a build input (#243) |
| `or-ptx` | its codec half into `gta5::particle`, its editor half into `or-particle` | the editor half moved and `YptAsset` is deleted (OD-PTX, P52, #320); `or-ptx` is deleted (P70, #333) |

`or-test-support` stays, unpublished. `or-title` is unchanged and depends on `or-io`: code that needs to know where a file lives belongs there, and code that needs only the bytes belongs in `or-io`.
### The or-asset module map

Every `or-asset` module had a destination; none was left behind in another crate (D10). P62 deleted the crate (#332), so the first column is what it held.

| `or-asset` held | Went to | How |
|---|---|---|
| The format readers and writers `ybn`, `ybd`, `ycd`, `ydd`, `ydr`, `yed`, `yfd`, `yft`, `yld`, `ynd`, `ynv`, `ypt`, `ytd` | `gta5::bounds`, `bounds_dict`, `clip_dict`, `drawable_dict`, `drawable`, `expression_dict`, `frame_filter_dict`, `fragment`, `cloth_dict`, `path_region`, `navmesh`, `particle`, `texture_dict` | all declared (`FORMATS`); the compiled readers were the oracle until P62 deleted them (#332), and the testimony and the behaviour baselines stand in |
| `ysc` | `gta5::script` | moved (#289): the disassembler, native naming and the reader (`ScrProgram::from_bytes`) are ordinary Rust in `gta5::script`, as is the decompiler (`gta5::script::decompile`, #442), with no `#[record]`, no pack binding and no `or-asset` shim left, a time-limited exception until a corpus is staged lets it declare (OD-YSC) |
| `rage_types`, the domain glue (`grcore`, `grmodel`, `rmcore`, `gta`), `gen9`, `target` | replaced by declarations, deleted in R66f | a decode rule inside them moved to `codec` first; all of them, the lossy `gen9::convert` included, went with `or-asset` (P62, #332) |
| Vertex declarations and decode, BCn decode, texture formats, channel unpacking | `or_io::codec::{mesh, texture, anim}` | texture decode moved (#268); vertex decode and the AABB rules moved into `codec::mesh` over any `VertexSource`, so an owned geometry meshes itself for both editions (#300, P41) |
| `mesh_coherence`, `fixup`, `bound_validate` | each format's `validate` in `gta5` | moved: a bound is checked by `gta5::bounds::validate`, and a mesh defect and an aliased buffer by `gta5::drawable::each_defect` and the aliasing check, which every `.ydr`, `.ydd`, `.yft` and `.ypt` `validate` runs at save for both targets; the repair is `aliased_lods` and `RmcDrawable::remove_lods` over the owned value, and `mesh_coherence` and `fixup` are deleted (P50, #323) |
| `bound_build`, `bvh` | `gta5::bounds::build` | constructing a bound from a mesh |
| `cloth_edit`, `texture_embed`, the two `material_edit` modules | edits over records in `gta5` | the material edits and texture embedding are field and `Vec` operations on the owned drawable, and both `material_edit` modules are deleted (P51, #318); `texture_embed`, with no caller outside `or-asset` since the app's embed editor moved onto `GrmShaderGroup::embed` (P56, #324), went with the crate (P62, #332) |
| `resource_file.rs`, `generation`, `rage_constants` | `container` and the crate root | `resource_file.rs` went with `or-asset` (P62, #332), and `prim` itself was deleted (P65, #336) |
| `format_registry` | detection into or_io::open and its Asset; `FormatHandler`, `FormatId` and `Tier` into `src/state/format_handlers.rs` | deleted with no shim (#286); editor policy stays in the app (OD-REGISTRY) |
| `gxt2`, `ypdb` | `gta5::{text, pose_db}` | `gxt2` is declared and owned as `Gxt2`, with no shim (#265); the pose database stays ordinary code (OD-YPDB) |
| `hdri`, `ptx_clip_regions`, `materials_palette`, `nametable`, `asset_name` | `gta5::{hdri, data}` | moved in #232 with no re-export left; parsers only, nothing bundled |
| `dds`, `awc` | `or_io::{dds, awc}` behind features | ordinary code |
| `fxc`, `sps`, `shader_defs`, `spirv_pack`, `shader_names` | `gta5::shaders`, `.fxc` behind `fxc` | moved (#270); a material's parameters, read from the title's own shader files; the materials walk and the remap's compiled side read the owned roots (P51, #318) |
| `grcore/encode`, `grcore/mipmap`, `grcore/png`, `texture_import` | `or_io::authoring` behind `authoring`, BC7 through `bc7-encode` | moved, both editions' `to_png`/`from_png` (#268) |
| `pack_required`, `pack_walk`, `layout_inventory` and the layout suites in the library | deleted in R66f | deleted with `or-asset` (P62, #332), `the_recorded_testimony_holds` moving into `or-io-verify`; `layout_inventory` went with `prim` (P65, #336) |

### Metadata

`or-meta` moves in as `or_io::meta` behind a `meta` feature (D5), because external map tools want `.ymap` and `.ytyp` more than anything else and RBM shares the RSC7 envelope. It reads PSO, RBM and RBF schema-driven: a document carries its own type table, so the module declares no resource layouts, and its hand-parsed container headers (RBM's PRD0 root, the PSO sections) move with it and are declared under D15 when they are. Typed roots for the world family (`CMapData`, `CEntityDef`, `CMapTypes` and the archetypes), scenario regions and the ped variation family (`CPedVariationInfo`, `CCreatureMetaData`) may be declared by hand, each matched against the document's own type table, because a typed root is a declaration like any `#[record]` and not a bundled schema ([OD-META-TYPED](Project_Context.md#world-metadata-roots-may-be-declared-by-hand-2026-09-26)). Its binary readers are ported from CodeX, with the authors' permission (D7).

No schema file is bundled: the snapshot is machine-extracted from material that is not ours. The module takes a schema as data. Without one it reads, writes and round-trips binary documents, showing a name outside `or_io::meta::binary::public_names` as `hash_XXXXXXXX`; XML import of PSO and RBM takes a type's layout from a supplied schema where one declares it, else from the type tables of a donor or of the install's metadata files (R37). The editor and the CLI take it from `OPENRAGE_SCHEMA`, then the user's configuration directory beside the keys cache, then, from a source tree only, the ancestor openrage-corpus/or-meta/v2 ([OD-SCHEMA-LOC](Project_Context.md#the-editor-and-the-cli-find-the-schema-snapshot-by-a-fixed-order-2026-09-26)). `or_io::meta` does this today (#243): it has no build script and compiles no snapshot in, the editor and the CLI install the snapshot `or_title::schema` finds as the process default on first use (`or_io::meta::install_snapshot`), the calls that need a schema read that default or take one explicitly, and `openrage-cli meta snapshot` builds the store's file. A redistributable snapshot built from retail-derived sources is an owner and lawyer decision. `or-world` and `or-depends` stay domain crates and read `or_io::meta`.

### Descriptions

A description is a title and target's layouts as data, and the generic machinery runs on it: type plans and records over an `Image`, the value tree (`desc::value`) the inspector and the canonical writer walk, the typed peek's `View`, the censuses and the testimony. Each declared format's description is built from its declarations (`decl::describe`, through `Format::description`), and GTA V has no other (D17): nothing loads a runtime `.orpack`, for GTA V or for a title the crate does not declare, which gets the generic tree from its own declarations ([OD-OTHER-TITLES](Project_Context.md#no-runtime-pack-for-another-title-2026-09-26)).

The model is `or_io::desc`. A `Pack` holds `PackType`s, each a size, its `PackField`s at their offsets and, for a tagged base, a `PackSelector`, and the `PackEnum`s they name. A field's `FieldRepr` says how it is stored: a scalar, an embedded record or run of them, padding, opaque bytes, a trailing run, an enum, or a pointer. A pointer's `Pointee` says what it leads to (one record, a run, a table of pointers, bytes, a string, strings, a member of its owner) and its `CountSource` where a run's length comes from (`Field`, `CountAndCapacity`, `MaskedField`, `Product`, `Sum`, `Fixed`, `PrefixedAboveOne`, and `MaskedOrMipChain` in the built-in descriptions only). `PackField::bits` names bit ranges inside a field, never what they mean, and `PackField::space` records the heap a pointer leads into.

- **Two heaps in one buffer.** System memory starts at `0x50000000` and graphics memory at `0x60000000`, which holds only vertex, index and pixel bytes. A writer has no address to read the heap from, so the description records it. ⚠ A payload in the wrong heap is silent: every pointer still resolves and OpenRage reads the file back, and the game's memory manager rejects it.
- **Capacity is not count.** A capacity is an allocation the game sized, carried beside the count; recomputing it shrinks what the game expects at its stated size.
- **A vtable word is not a layout fact.** `value::empty` writes zero, and a `pgBase` carrying a zero crashes the game on load, so a root authored from nothing takes `or_io::constants::DEFAULT_VFT`; the padding census fails on a saved one that is zero.
- **A round trip through one description cancels a consistent misreading**, and a description shifted wholesale, every field moved four bytes and the size grown to match, is coherent and wrong (`a_description_shifted_wholesale_is_coherent_and_wrong`). What proves a layout is [the verification below](#verification), never a round trip.

A description has no file form: the `.orpack` export, its encoding and its version went by the owner's ruling of 2026-10-01 ([recorded](Project_Context.md#openrage-packs-is-deleted-and-the-orpack-export-goes-2026-10-01)), so a description exists only as the value `or-io` builds from its declarations.

`openrage-packs`, the private pack repository, stopped being the layout and name authority at D1 and is read by nothing in the tree, the workflows or the recipes since P61 (#331) and P62 (#332); the owner archived it offline and deleted it on 2026-10-01 ([R66i](Roadmap.md#r66i-retire-openrage-packs), D17). `or_io_verify::diff::structural` stays as a public tool for comparing two descriptions ([the structural diff](#the-structural-diff)). Leak-derived notes live in the gitignored tier, for example [OR_IO_Leak_Cleanup.md](../extra/OR_IO_Leak_Cleanup.md).

### Dependencies

No egui, wgpu, winit or anything else the editor needs. `crates/io/or-io/Cargo.toml` states the rest, and [features](#features) make `bincode`, `aes`, `sha1`, `memmap2` and `rayon` optional; `or-io-macros`, `flate2`, `serde`, `serde_json`, `thiserror`, `tracing`, `byteorder` and the texture decoders are unconditional; `binrw`, `inventory` and `or-macros` went with the classic layer (P65, #336). The plan leaves `flate2` and `thiserror` required and puts the rest behind [features](#features).

## Module organisation

The module tree is the absorbed crates' today; the planned split is marked.

```text
or_io
├── (root)       hash, error, diagnostics, generation, names, needle, constants, half,
│                asset (open, from_bytes, Asset), height_map, distant_lights,
│                water_map, world_blanket, nav_precalc, mrf, cache_loader,
│                social_club_metadata, audio_world_sectors
├── container
│   ├── resource RSC7: the header (declared owned), envelope, compression, versions
│   ├── paging   PgRscBuilder, allocator, chunk map, pointer fixups
│   └── rpf      RPF7 reader and builder; crypto/ with key extraction
│                [planned: split into archive and crypto]
├── desc         the description model, type plans, records, the value tree, Image,
│                export/
├── write        build (the canonical writer), patch, edit, compare, canvas
├── decl         #[record]'s vocabulary: View, RecordMut, Owned, Ref, RefPath, Codec, File,
│                Resource, Peek, describe, refusals
├── gta5         one module per format, shared, skeleton, drawable_codecs, formats,
│                data, hdri, text, pose_db, shaders (shaders::fxc behind feature fxc),
│                script, retarget, and the gen8 and gen9 preludes
├── codec        texture, anim, mesh
├── meta         feature meta
├── authoring    feature authoring
└── dds, awc, rel, gfx  features dds, awc, rel and gfx
```

- A type lives with the format that owns it, and in `gta5::shared` when two formats use it: a drawable's levels of detail are `gta5::drawable::DetailLevels` until that format migrates to the engine's `RmcLodGroup`, and the Gen8 texture is `gta5::texture_dict::gen8::GrcTexture`.
- Code and documents say Gen8 and Gen9; messages a user reads say Legacy and Enhanced, through `Generation::edition_label`.
- The preludes `gta5::gen8` and `gta5::gen9` re-export declared types resolved for one target; the migrated roots are exported from `gta5` itself.
- A codec in the `codec` module never names a title: it takes anything implementing its source trait, and `gta5` implements that per target, so a second title can reuse it.

## Declaring a structure

A declaration is an ordinary Rust struct under `#[record]`, which proves its layout at compile time and generates everything else from it.

### The owned form (D16)

A declaration is written in the types a user edits, and the on-disk layout is derived from them. From `.yfd`:

```rust
#[record(size = 0x40, recompute = CrFrameFilterMultiWeight::sign)]
pub struct CrFrameFilterMultiWeight {
    pub base: CrFrameFilter,
    pub track_id_indices: Vec<TrackIdIndex>,
    pub weights: Vec<f32>,
    skel_data: Ptr<Opaque>,
}
```

| Domain type | On disk | Read | Written |
|---|---|---|---|
| a number, an `Open<E>`, an array of them | itself | copied | copied |
| a record by value | its raw layout, embedded | converted in place | converted in place |
| `String` | a pointer at NUL-terminated bytes | empty for null, and for a word that is not an address, which is carried | null for empty, or the carried word while it stays empty |
| `Option<Box<T>>` | a pointer | `None` for null, and for a word that is not an address, which is carried | null for `None`, or the carried word while it stays `None` |
| `Box<T>` | a pointer the game never leaves null | refused if null | always a target |
| `NullIsEmpty<T>` | a pointer whose null the file means as `T`'s empty value | `T::default()` for null; a target equal to it is noted | null for a value equal to `T::default()`, unless the file pointed at one |
| `Vec<T>` | a counted array (`atArray`) | the elements | count from `len()`; capacity carried |
| `#[len(...)] Vec<T>` | a pointer whose length another field states | empty for a live pointer stated empty, and for an alias of another pointer's allocation beside a length of zero, which are carried; refused so where it may share another's allocation | the count fields written from `len()`, and the carried live pointer while it stays empty |
| `#[phys] #[len(...)] Vec<u8>` | the same, in the graphics half | the bytes; refused if live and stated empty, but a `masked_or_mip_chain` length reads a zero word at the mip chain's length, and a chain the file holds only the first levels of at those levels | the bytes |
| `#[trailing] #[len(...)] Vec<T>` | a run directly after its owner, no pointer | the elements | after the owner |
| `Vec<Ptr<Opaque>>` | a counted array of pointers nothing follows | the addresses | as the canonical writer writes them |
| `Ptr<Opaque>` | a runtime pointer nothing follows | the address | as the canonical writer writes it |
| `Member<T>` | a pointer at a member of the same record | the member's name | placed with its owner |
| `Back<T>` | a pointer at the nearest record around it that is a `T` | nothing, or that the file held null; refused if it points elsewhere | derived |
| `Keyed<T>` | a pointer at a record declared with `key = "field"`, or at one a map entry owns through `#[keyed_by]` | the target's key, or its entry's; `None` for null | at the record holding that key, wherever it lands |
| `#[keyed_by(key)] Box<T>` or `Option<Box<T>>` | a map entry's pointer at the value its `key` field keys | the target, its entry's key noted for any `Keyed<T>` at it | the target, standing at that key |
| `#[keyed_by(keys)] Vec<Option<Box<T>>>`, `keys` a `Vec` | a dictionary's pointers, element `i` at the value `keys[i]` keys | the targets, each key noted as above | each target, standing at its key |
| `OwnedOrKeyed<T>` | a pointer at a target of its own, or at one a `#[keyed_by]` map elsewhere in the tree owns | `Own` of its own target or null; `Key` of the entry's key otherwise | its own target, or at the record holding that key; refused if nothing holds it |
| `#[refs]` on a private unsigned field | a reference count | carried | the number of pointers at its record: its owner and every `Keyed<T>` at it |
| `Ref<P>`, `P` one of `Box<T>`, `Option<Box<T>>` or `String` | a pointer at a single target, owned either here or elsewhere in the tree | `Own(P)` reading as `P` does; `To(RefPath)` naming, with no public address, a path from the root to the record, run element or string another pointer owns, resolved from the alias by `Rx`'s address index, the `RefPath` carrying hidden the address the file held its target at | `Own` writes as `P` does; `To` writes a stand-in that `__settle` replaces, at the root, with an alias of the target's finished layout, before the `#[refs]` counts run, after following a target the file held to its new path through edits to the runs around it; refused at save through a cycle, a path that leads nowhere or to a null pointer, or a target an edit removed |
| `#[select(...)] U`, `U` a `#[record(union, size = N)]` enum | an inline union: N bytes laid out as one of several pointer-free records | the arm a sibling field or bit range selects; refused if none does | the arm's bytes, and the selecting bits it implies; refused if they would read as another arm |
| `#[shares(other)] Shared<V>` | a pointer that may point where `other` does; `other` a field, or a path from one through embedded records or into the record a `Box<T>` field points at, ending `[N]` for element `N` of a run of pointers (`strip.bytes`, `levels.lods[0]`) | `Same` where the two point at one allocation, whichever of them the file made the owner, and where the slot holds a word that is not an address, which is carried; `Own(V)` where they do not | `Same` where `other` points, null where it is null or absent, or the carried word unless the save repairs (`Repair::Lossless`), which points the slot where `other` does and notes it; the target is laid out at the turn of whichever of the two a reader reaches first, a record's own pointers in declaration order before an embedded record's |
| `#[len(...)] Vec<String>` | a table of string pointers whose length another field states | the strings | the table and its strings |
| a tagged base behind a pointer | the base, its tag choosing the layout | an `XAny` enum, one variant per declared layout | the variant's layout |
| `#[codec(RawType)] C` | an allocation whose regions are computed ([codecs](#codecs-for-computed-regions)) | `C::open` over the blob the codec's reader made; refused if not its own layout | by `C::save`, after every other field, with the owner fields it implies |

From one declaration the macro generates:

1. **The owned struct**, with the declared public fields plus a hidden `__carry` holding what the file says and those fields do not: where it was opened from, each counted array's capacity, each private field's value, and whether an empty array or `#[len]` run still points at an allocation, as a `.ynd` junction map does in 154 of 518 shipped regions (#220) and a one-bone skeleton's child and parent indices do. Two owned values compare equal whatever they carry. `Default` is implemented for every owned type, whatever the length of its arrays.
2. **The raw layout**, a `#[repr(C)]` struct reached as `Raw<T>`, with the compiler's offsets and the layout proof below.
3. **The reader and the canonical writer.** `Owned::from_element` builds the owned value from the record tree the description reads, and `Owned::to_element` builds the tree the canonical writer writes and the format's validation judges — a second front door onto the machinery every fixed-point gate already holds.
4. **The built-in description**: names, offsets, representations, padding as padding.
5. **The opt-in peek**: the raw layout's `View` and accessors, for a tool that only looks (`Resource::peek`).

A root adds one `impl Resource` naming its extension and target (`impl Resource for PgDictionary<CrFrameFilterAny>` with `EXT = "yfd"` and `TARGET = Generation::Gen8`) and gets `peek` and the `File` calls every file type shares: `open`, `from_bytes`, `to_bytes`, `save` and `save_as`. A file type outside RSC7 implements `File` itself, stating its `EXTENSION`, `from_bytes` and `to_bytes`, and opens and saves through the same calls (`a_file_type_outside_rsc7_opens_and_saves_through_the_same_calls`, #240).

**The owned gate** is that every corpus file opened owned saves byte for byte as its tree saves, and that the owned value is a fixed point of its own save (`declared::owned_saves_as_tree`). The canonical form nulls runtime pointers, so the value is held to its own save rather than to the shipped file. The fixed point is judged on bytes, not on `PartialEq`: a shipped float can be a NaN, unequal to itself however faithfully it was carried. A file both routes refuse counts as refused.

Two refusals are the owned form's own, at open: a null pointer where the game never leaves one (`Box<T>`), and a live pointer beside a stated length of zero in the graphics half, or at a run that may share another's allocation, which an owned `Vec` can hold only as empty so that a save would drop what it points at. In the graphics half it is a payload the length does not state; in the virtual half it reads as an empty run and a save points at an empty allocation again. A Legacy texture's pixel run is the exception: its `masked_or_mip_chain` length reads a zero word at the length the mip chain implies, so `server_assets.ytd` and the four Legacy `des_*.ydr` open (OD-IMPLIED-PIXEL-LEN, #308), and only a texture whose chain implies nothing either is refused, by the tree's save too (`a_live_pixel_pointer_with_no_stated_or_implied_length_is_refused`). A codec's blob at a field that declares no `#[codec]`, or that is not its codec's own layout, is refused at open rather than dropped ([codecs](#codecs-for-computed-regions)); so are two pointers at one run that no `#[shares]` declares, such as a composite bound's shared matrices.

What an owned value shows is ruled (OD-RUNTIME, 2026-09-26) and built (#236). Padding, element counts, reference counts, vft words and runtime pointers are declared, so the raw layout and the description name them, and left private: the owned struct does not have them, `__carry` keeps what the file held and a save writes it back, and `#[init(expr)]` is what a value built from scratch saves one as (each reference count on the four owned roots is `#[init(1)]`). A bit field sharing its word with a count is reached through methods, as a Legacy texture's template type is through `template_type` and `set_template_type`. A derived field is recomputed at save by its record's `recompute` hook: the `.yfd` signature, an `AtString`'s length and, where the string outgrows it, its allocation, and a `PgDictionary`'s code order, which a save sorts, moving each entry with its code. A private field declared `#[derived(f)]` saves `f(&self)`, written with the other fields and before the count fixups that divide by it, as a vertex buffer's stride is (OD-VB-STRIDE, P50, #323): a value built from scratch saves what it derives, and a value opened from a file whose stated value disagrees with its derivation saves the stated value while the derivation is what it was at open, and is refused once an edit moves it (`synthetic::derived` in `declared_records`). `no_owned_record_shows_padding_a_count_a_reference_count_a_vft_or_a_pointer`, in `decl::refusals`, refuses a public padding, count, reference count, vft or pointer field on every owned record a format reaches and on the shared primitives, and a doctest there pins assigning a count on an owned value as E0609.

A pointer at something another pointer owns (a reference-counted reference, a composite bound's shared matrices, a texture variable's texture, an effect spawner's rule) has no public address (OD-REFS, 2026-09-26): it is derived at save where it is structural (`Back<T>`), held by key where its target is keyed (`Keyed<T>`), held once where its target is shared (`Shared<V>`), and held by path where its target is owned elsewhere in the tree with no local key (`Ref<P>`, #301, P79), as the table above gives them (#236). `declared_records` holds each on synthetic types, and no GTA V declaration uses them yet: each shipped pointer is measured on the corpus against its derivation in its format's owned migration, and carried only where the two differ. An uncommitted corpus measurement of `Ref` on the current trees confirms every scoped shape resolves by path, and shows that in `.yft` the composite's `bounds.children[]` are the aliases and the child drawables' `bounds` own them, so `PhBoundComposite.bounds` is the field that needs it (#301).

A pointer the file aims at a target another pointer owns elsewhere in the tree keeps the type its format declares (OD-ALIAS-BY-VALUE, 2026-09-29), so a fragment's drawable reads its skeleton as `if let Some(skel) = &car.skeleton` whether or not the common drawable owns it. An `Option<Box<T>>` or `Box<T>` field, a `String`, an element of a counted or `#[len]` run of them, or a whole counted or `#[len]` run, as a modded ped's geometries share one `matrix_palette`, reads as a copy of what the owner holds, and `__carry` records the owner's path from the root; an aliased counted array states its owner's count and capacity. A save writes the alias, a pointer at wherever the owner is laid out, while the copy still equals the owner, judged on the trees the two build, and a separate allocation once an edit to either makes them differ or the owner is gone; the copy is built with keys of its own, so a keyed target inside it answers only references inside it. An untouched value saves byte for byte as its tree does, and an alias no pointer owns, or one at a record around it, is refused at open. A copy, and everything read inside it, carries no address a `Ref` can follow, so a reference whose target was removed is refused rather than moved onto a surviving copy (`a_reference_to_a_removed_owner_is_refused_rather_than_moved_onto_its_copy`, #307). `Ref<P>` stays where a reference is the natural owned API, as in a `.ypt`'s lists of run elements. A string pointer into a member of another record, as a physics level's group names point into its groups, is no alias: the tree reads it as a string of its own and so does the owned form. `declared_records` holds each shape on synthetic types (`by_value`).

Where Gen8 and Gen9 lay a type out differently the two are separate types. `texture_dict::gen8::GrcTexture` and `texture_dict::gen9::GrcTextureG9` are both owned. A Gen9-only word of the Gen9 texture or its `ShaderResourceViewG9` takes a measured name where the corpus measures one, such as `block_stride`, and the view's `view_dimension` takes Direct3D's; every other such word is carried on its own at its own width, with its reason in its note and its value held by `the_carried_enhanced_texture_words_hold_what_their_notes_state` (OD-10.9). A drawable is one generic declaration per class whose typed open names its target (OD-10.8, [the target](#the-target-d16)). A root opened as the wrong target says which target the file holds.

### The raw form and the layout proof

Every declaration is the owned form: `#[record(raw, ...)]` is retired, and `or-io-macros` refuses it (a `compile_fail` doctest in `decl::refusals`, P57, #319). The raw layout an owned declaration generates is written in on-disk types (`Ptr`, `Array`, `Str`, `Trailing`) and reached as `Raw<T>`; the owned form refuses a field named `unknown*` or `unk*`. The Gen8 texture, owned and named from the source:

```rust
#[record(size = 0x90, target = "gen8")]
pub struct GrcTexture {
    pub base: PgBase,
    pub texture: GrcCellGcmTextureWrapper,
    pub name: String,
    #[init(1)]
    ref_count: u16,
    pub resource_type_and_conversion_flags: u8,
    #[pad_after(4)]
    pub layer_count: u8,
    cached_texture_ptr: Ptr<Opaque>,
    physical_size_and_template_type: u32,
    // …
    #[phys]
    #[len(masked_or_mip_chain = "physical_size_and_template_type", mask = 0x07FF_FF80)]
    pub backing_store: Vec<u8>,
    // …
}
```

The proof is the macro's, at compile time. Offsets are the ones `repr(C)` gives from field order and natural alignment. Each field must end where the next begins and the last where `size` says, so padding is always declared, by the engine's name for it or by `#[pad_after(N)]`, never inserted silently. An `#[at(0x..)]` is an optional assertion against `offset_of!`, and `#[note("...")]` carries a note into the description. Every field type must be `Plain`: valid for any bit pattern, with no padding of its own, which a `bool` or a Rust `enum` is not. A gap, a size that is not the engine's, an `#[at]` the computed layout disagrees with, a `bool`, a type parameter held by value, an `unknown*` or `unk*` field, a run with no `#[len]` and a pointer built by hand are each a compile error, held by a `compile_fail` doctest in `decl::refusals`, pinned to its error code where rustc gives one. A public padding field has no named compile-time refusal; `no_owned_record_shows_padding_a_count_a_reference_count_a_vft_or_a_pointer` refuses it on every owned record a format reaches.

`#[record]` implements `decl::Plain` on the strength of that proof and uses no zerocopy derive: zerocopy refuses `IntoBytes` on a generic `repr(C)` struct whose fields are not `Unaligned`, and the target-generic drawable spine is exactly that. The casts from bytes to `&T` and `&[T]`, in `crates/io/or-io/src/decl/view.rs`, are the `unsafe` code this rests on, sound by the proof plus a bounds and alignment check.

### Field types

The vocabulary a declaration is written in comes from `or_io::decl`, or is another declared record:

| Kind | Raw type | Owned type | Notes |
|---|---|---|---|
| Numbers | `u8`…`u64`, `i8`…`i64`, `f32`, `f64` | the same | little-endian |
| Boolean byte | `Bool8` | `Bool8` | any byte value is valid, so a file holding 2 still reads |
| Open enum | `Open<E>` | `Open<E>` | a value no variant names stays a number |
| Hash | `Hash32` | `Hash32` | the joaat hash, computed rather than transcribed |
| Vectors | `Vec2`, `Vec3`, `Vec4`; the engine's `Vec3V`, `Vec4V` in `gta5::shared` | the same | a 16-byte vector keeps its fourth lane, NaN included |
| Pointer | `Ptr<T>`, `PhysPtr<T>` | `Box<T>`, `Option<Box<T>>` | virtual or physical space; no public constructor |
| Pointer into the owner | `MemberPtr<T>` | `Member<T>` | [member pointers](#member-pointers-and-generic-layouts) |
| Unfollowed pointer | `Ptr<Opaque>` | `Ptr<Opaque>` | a pointer the description leaves unfollowed |
| String | `Str` | `String` | |
| Counted array | `Array<T>` | `Vec<T>` | the 16-byte header: pointer, count, capacity |
| Trailing run | `Trailing<T>` | `#[trailing] #[len(...)] Vec<T>` | a run the allocation holds past `size` |
| Count | `Count<W>` | none: private | the writer derives it from the child it counts |
| Bit ranges | an integer field with `#[bits(name, low, width[, "note"])]`, repeatable | the integer | described as the field's `bits`; a patch refuses the field whole; a range past its width or over another fails to compile |
| Inline union | `#[record(union, size = N)]` on an enum of `Arm(Record)` | the enum | described as N opaque bytes; `#[select(name = value => Arm, …, _ => Arm)]` on the holding field chooses the arm |
| Computed allocation | the pointer named in `#[codec(RawType)]` | a `Codec` type | its codec reads and lays out the regions ([codecs](#codecs-for-computed-regions)) |

`#[len(...)]` says where a length comes from, with the vocabulary the pack format has: `field`, `count` with `capacity`, `masked` with `mask`, `product`, `sum`, `fixed`, `bytes`, and `prefixed_above_one` for a run one entry longer than its count once the count passes one; and, in the built-in descriptions only (OD-PACKVER), `masked_or_mip_chain` with `mask`: the masked word or, where it leaves zero, the bytes of the mip chain the texture's `format`, `width`, `height`, `depth`, `mip_count`, `cut_mip_levels` and `layer_count` imply (#308). `sum` is `CountSource::Sum`: a `.yed` program is a header and three runs back to back, each counted by its own field, and every summed field is structural, so a patch cannot move where the next run begins and an edit that would resize the run is refused, because which part grows is not stated.

Planned and not built: `F16`, `Quat`, `Mat44`, `Aabb` and `Color32` wire types; offsets relative to the file or the owner, for RPF7's name heap and GXT2's strings; a fixed table of pointer slots; a dictionary type with hash lookup; field kinds in the description. A hashed map is declared as records today (`clip_dict::AtMap<T>`, `AtMapEntry<T>`).

### Alignment

A view is a `&T` into the decompressed image, so alignment is checked on every view and never assumed: `View::at_offset` refuses a misaligned instance by name. The image is the crate's own 16-byte-aligned buffer, because an RSC7 payload is DEFLATE and is always decompressed into it, and records are allocated the same way. A type the corpus holds off its natural alignment is declared `packed(N)`: its fields still read as fields, and its accessors copy. `.ynv`'s sector quadtree node, whose pointers sit at four modulo eight, is `packed(4)`.

`or_io_verify::census` counts, per type, the instances that sit off their natural alignment, over shipped files and over saved ones. The canonical writer aligns a pointer table to eight, because placed after a run of four-byte values it would sit at four modulo eight, where no typed view of the saved file opens, and each record run to its element type's natural alignment, so a save needs no `packed` declaration a shipped file does not ([OD-ALIGN](Roadmap.md#r66d-declare-gta-v-format-by-format), `a_canonical_save_places_every_record_at_its_natural_alignment`).

### Gen8 and Gen9

A type both targets lay out the same way is declared once. The four whose field set Gen9 changed — the vertex buffer, the index buffer, the shader (a `Material` in the declarations) and the texture — are declared per target, and a type whose only difference is what it points at takes a target parameter: a pointer is eight bytes whatever it points at, so no offset depends on it. `DrawableTarget` names the four per target, and `GtaDrawable<Gen8>` and `GtaDrawable<Gen9>` share one declaration, as do the drawable dictionary, the fragment and the particle list.

⚠ Never declare a union with per-target field presence: that design was built and reverted, because it let a Gen8 document carry Gen9 data with nothing but discipline to stop a rebuild dropping it. A record carries the target it was read with, and the writer refuses it under the other.

A file's target is its RSC7 version (`Format::target_of_version`), never the install it came from: an Enhanced install ships `.yfd` at the Gen8 version, three `.ytd` at Gen8's 13 among 93,076 at Gen9's 5, and one `.ytd` at a version 2 nothing declares (`FORMATS` in `crates/io/or-io/src/gta5/formats.rs` gives the census).

### Member pointers and generic layouts

A pointer into its own record is `MemberPtr<T>` (owned: `Member<T>`). A particle behaviour's keyframe table points at keyframe properties embedded in the same behaviour: 28,113 of 28,113 entries land inside their own record (`what_a_behaviour_record_looks_like`, #197). The owner is the record's concrete type, not the struct that declares the field, so a variant's table resolves against the variant's layout. A view resolves each entry to the member it lands on and refuses an address that is not a member's start; a record stores the member's path; the writer patches the pointer after placing its owner. In a description these are `Pointee::Member` and `Pointee::Members`, and the tree holds a `Value::Member`. Read as a plain pointer, a rebuild allocates a copy of every property and points at the copy, which reads back plausibly and is not the file.

The same mechanism places a Gen9 texture's view pointer: 28 of 28 point at their own view after a save (`an_enhanced_texture_view_pointer_rebuilds_at_its_own_view`, #197), at 0x58, where CodeWalker's `TextureG9` also puts it. The testimony recorded at 0x50 is answered through a `FieldMoved` in the `MOVED` of each testimony suite that reads a texture.

A layout may be generic over what it holds when every use of the parameter is behind a pointer, so no offset depends on it; `#[record]` refuses a parameter held by value. One map entry type serves both of a clip dictionary's maps: over 60 Gen8 `.ycd` the tree reaches the compiled reader's 17,775 clips and 8,301 animations, and naming the clip map's element wrongly reads no clips and 26,076 wrong animations (`what_the_permanent_write_rows_rest_on`, #197). In `or-io` these are `clip_dict::AtMap<T>` and `AtMapEntry<T>`, and `.ynv`'s `ChunkedList<T>`. In a pack the exporter monomorphises each instantiation into a type named after it (`AtMapEntry<CrClip>`), keeps the bare stem beside it with the parameter's pointers left undescribed, so a consumer asking for the stem by name reads what it always read, and declares a root no field names with `or_io::instantiate!`.

A particle rule's five derived behaviour slots are subsets of its behaviour list: 0 of 31,343 derived pointers name a record outside it (`what_a_behaviour_record_looks_like`, #197), so the tree carries them as aliases of the list's records.

### Tagged bases and enums

A tagged base names its variants on itself: `#[record(size = 0x18, tag = "type", variant(4 => CrFrameFilterMultiWeight))]`. The base's description carries the selector and reaches the variants, so the closure from a root finds them with no registration. `View::variant` is the typed dispatch (`V: VariantOf<Base>`), and the owned form generates one enum per base, `CrFrameFilterAny`, with a variant per declared layout. Each variant is one call from the pointer that holds it: the enum has `as_<variant>()` and `as_<variant>_mut()`, and a trait, `CrFrameFilterAnyExt`, gives the `Option<Box<_>>` holding it the same calls, so `dict.entries[0].as_multi_weight_mut()` reaches the `CrFrameFilterMultiWeight` an entry holds, or is `None` (`a_variant_is_one_call_from_the_pointer_that_holds_it`, #240). A tagged base may be generic over its target (#313): `Tagged`, each `VariantOf`, the enum, its trait and its `Owned` impl take the base's parameters, so `BrushAny<T>` holds a `Brush<T>` or a variant such as `BrushSwatch<T>`. A parameter no field names, as a base's target is named only by its variants, is used by a hidden zero-sized `__params` field in the raw and the owned struct, which `Default` and `from_raw` fill (`a_generic_tagged_base_opens_owned_and_saves_as_the_tree_for_legacy`, `a_generic_tagged_base_opens_owned_and_saves_as_the_tree_for_enhanced`).

- A tag may be a bit range of its field, `tag_bits(low, width)`: a collision polygon's kind is the low three bits of its `area` float.
- One layout may answer several tags, because `VariantOf` takes a list: a particle shader variable's six vector tags share one layout.
- Only variants a file can check are declared. Every frame filter the corpus and both installs ship is a `MultiWeight`, so it is the only variant; an undeclared tag is refused by name rather than read as a declared one.

An enum takes the engine's name and values (`EFrameFilterType`) and is declared with `#[record(enum, width = …)]`. The layout holds `Open<E>`, so a value no variant names stays a number, and the first line of a variant's doc comment that is not blank is its display note, so an inspector shows `Geometry (4)` rather than `4`; `every_declared_enum_variant_carries_its_note` fails on a declared variant without one. The width is required, because the enum does not say how many bytes the field holding it reads. Every variant states its value, negative allowed, because the numbers come from files and letting Rust assign them renumbers the variants below an insertion. ⚠ A wrong variant number passes every round trip, since the field is an integer carried verbatim: a primitive type numbered as Direct3D's enum would show every shipped geometry as a line strip, so only a check against the bytes' meaning catches it; two variants with one value, and a variant named `Other`, which holds the values no variant names, are compile errors. Where the values are somebody else's — Direct3D formats, FourCCs — the variant carries their name.

### Codecs for computed regions

Where an allocation's regions are computed rather than placed, the format carries a codec per site, in `gta5::drawable_codecs`: a Gen8 material's parameter blob (entries, rows and name hashes; carried whole, interior pointers rebased), a skeleton's bone-tag table (derived from the bones; refused where the file's is not the derivation), and a Gen9 material's parameter block (three pointers into one allocation, one copy per worker thread; carried whole and rebased). `Format::covered` names those sites, so a tree comparison does not count them as lost. A codec is for regions that are computed, never for regions merely undescribed; the test is whether there is a constant to write down.

An owned record holds a site in a `#[codec(RawType)]` field whose type implements `decl::Codec` (#246). `Codec::open` builds the value from the blob the codec's reader made, reading the owner's fields and children through `Site`, and refuses a blob that is not its own layout, so a save never drops what the value could not state. `Codec::save` runs after every other field, emits the blob the canonical writer lays out, and stamps the owner fields it implies through `SiteMut`. A private codec field is derived: opening runs `open` and drops the value, and a save lays out `C::default()`. The three sites' owned forms, in `gta5::drawable_codecs`:

- A Legacy material's entries, a `Vec` of `gen8::GrcInstanceDataEntry<T>`: a register, a name hash, and a texture or rows of four floats. The save lays out entries, rows and hashes and stamps the counts, the hash offset and the blob size.
- A skeleton's `BoneTags`, which holds nothing, so a skeleton declares it private and shows only its bones; opening refuses a table that is not the derivation of the bones.
- An Enhanced material's `gen9::Params<T>`: each constant buffer, texture slot and sampler byte of one worker thread's copy. The save writes the buffer, texture, other and sampler counts into `param_infos`, writes the buffers once per copy the infos state, and nulls the texture slots of every copy but the first. Its other slots (`num_unknowns`) follow each copy's texture slots, null in every copy; opening refuses a non-null one (P54, #322).

The texture slot type `T` is a parameter, chosen by the drawable's owned migration (OD-REFS). `synthetic::codecs` in `declared_records` holds each on invented owners without game files, and `every_material_and_skeleton_opens_owned_and_saves_as_its_tree_saves` holds them on the corpus. The owned drawables' materials and skeletons hold these codec fields.

### What a declaration generates

- **Accessors on generated types.** `View<'a, T>` derefs to `T::Access<'a>`, which derefs to `T`, and `RecordMut<'r, T>` derefs to `T::Edit<'r>`. Inherent methods on `View` would compile only inside `or-io`, so a record a plugin or a test declares would get no accessors and the tested path would not be the shipped one. `View::get`, `View::offset` and `View::image` are associated functions, so a field named `offset` cannot shadow them. A `Ptr<Opaque>` field takes no accessor, because there is nothing described to follow.
- **Edits through a copy.** `RecordMut` holds the record's `T` by value and writes it back when dropped, so a field assignment needs no aligned record storage; a structural edit goes through `write::edit`, which keeps a count in step with its run.
- **Padding.** `#[pad_after(N)]` generates a private field described as padding; a `_pad*` field a declaration names itself is private and left out of the description; a pad the engine names is an ordinary field. The canonical writer writes zero in padding either way. ⚠ A `#[pad_after]` is a claim that the file holds nothing there, and every save enforces it: measure what the bytes hold first, and declare a run that is not zero as a private field under the source's name, which `__carry` keeps and a save writes back.
- **The built-in description** is the closure of what a root's fields name (`decl::describe`), built once on first ask with no registration. `gta5::formats` is the table of each extension's roots, targets, RSC7 versions, codecs and validation, and `Format::description`, `Format::read` and `Format::save` are the read and canonical save every declared format shares.

### How `#[rage_struct]` retired

Its attribute vocabulary carried over under `#[record]`'s spelling: `#[len]` for the count attributes, `#[pad_after]`, an optional `#[at]`, `packed`, tags, member pointers, notes, and `#[record(enum)]` for `#[rage_enum]`. Its four code generators — classic `BinRead`/`BinWrite`, domain-first `FromResource`/`ToResource`, `pack_bound` and `pack_only` — went in R66f with the compiled readers: a view or an owned value replaces the reader, the canonical writer replaces every generated writer, and there is no second layout to compare against because there is one declaration. The testimony capture it registered went with `prim` (P65, #336), and `or-macros`, which held both macros, was deleted (P66), so `or-io-macros` is the one proc-macro crate.

### What proves a layout

There is no provenance attribute (D15). A layout is proved by the compile-time checks above and by its format's suites ([verification](#verification)); a plain comment explains a field only where something about it is unusual. The RSC7 version table, `crates/io/or-io/src/container/resource/versions.rs`, states the versions a census of both installs observes, never a sum of engine-named constants.

### Names come from the source

Every declaration migrated under D15 carries the names the GTA V source gives it, in the casing `#[rage_struct]` always used: a type's C++ name in `PascalCase` with its namespace dropped (`GrcTexture`, `RmcLodGroup`), and a member's name in `snake_case` with its `m_` and type prefix dropped (`vertices_count`). Field order, field types, embedded bases, padding and enums follow the source's declarations; CodeWalker's binary layout, the corpus and the recorded testimony are the cross-check. A word the source names carries that name, and a pad the source declares carries the source's name for it. The migrated formats are the Owned column of [R66d's table](Roadmap.md#r66d-declare-gta-v-format-by-format), and no declaration keeps OpenRage's earlier names since the raw form was retired (P57, #319).

Names the retail game carries (metadata field names, shader names) and third-party vocabularies (Direct3D formats, glTF) are used for their own things. What stays out of committed text is a citation of the source tree — a path, a file and a line — which lives in the gitignored tier, and code or comments translated from the source.

⚠ Two designs kept engine names out of the executable before D15 — a vocabulary of roles bound by the pack, and public functional names with a private overlay — and both were deleted: each was a complete second vocabulary kept in step by hand, and neither could name an enum value such as `D3DFMT_DXT1`. Reaching for either again reopens D15.

## The public API

The target API is an owned value per file; the view, the record and the generic tree stay underneath it for tools that want them.

<a id="50-the-target-d16"></a>

### The target (D16)

This is what an external developer writes, approved by the owner on 2026-09-25, and the backend is built to make exactly this work:

```rust
use or_io::gta5::*;

let mut car = GtaDrawable::open("police.ydr")?;     // typed open; target from the RSC7 header
println!("{}", car.name);                            // String
println!("{:?}", car.lod_group.lod_distances);       // [f32; 4]
if let Some(skel) = &car.skeleton {                  // pointers are plain fields; Option only where the game allows null
    for bone in &skel.bones { println!("{} {:?}", bone.name, bone.translation); }
}
for model in &car.lod_group.lods[0].models { for geom in &model.geometries {
    println!("{} verts", geom.vertex_buffer.vertices_count);
}}
car.lod_group.lod_distances[0] = 45.0;               // plain assignment
car.name = "police2".into();
car.lod_group.lods[0].models.remove(1);              // structural edits are Vec ops; counts and pointers kept in step on save
car.save()?;                                         // validated, canonical form; or save_as(path)

let frag = GtaFragType::open("police.yft")?;         // the wrong kind of file says so
match or_io::open(path)? {                           // unknown type: an Asset, resolved from header and extension
    Asset::Drawable(d) => {}, Asset::Fragment(f) => {}, Asset::TextureDict(t) => {}, Asset::Clips(c) => {}, other => {},
}

let gta = Gta5::find()?;                             // finds the install, gets keys itself (or-title)
let mut car: GtaFragType = gta.open("police.yft")?;  // searches archives and DLC in load order
gta.save_to_mod(&car, "mods/police.yft")?;           // never writes into the install

for tex in &ytd.textures { tex.to_png(format!("out/{}.png", tex.name))?; }
ytd.textures.push(Texture::from_png("decal.png")?);  // mips and compression handled (authoring feature)
let mesh = geom.mesh();                              // positions, normals, uvs, indices

let mut map = Ymap::open("hills.ymap")?;             // metadata reads the same way (meta feature)
for e in &mut map.entities { if e.archetype_name == "prop_bench_01a" { e.position.z += 1.0; } }
map.save()?;
```

Amended on 2026-09-26 ([OD-10.8](Project_Context.md#drawables-are-one-generic-declaration-per-class-2026-09-26), taken under the owner's standing instruction for R66): a drawable is one generic declaration per engine class, `GtaDrawable` generic over `DrawableTarget` with `Gen8` as the default, so the sample's first line opens a Gen8 drawable, from either edition's install, and no field holds a target. Field access is unchanged; how a Gen9 drawable opens is:

```rust
let car = GtaDrawable::open("police.ydr")?;          // Gen8; a Gen9 file is refused by name, pointing at or_io::open
let car = GtaDrawable::<Gen9>::open("police.ydr")?;  // the typed Gen9 call
match or_io::open(path)? {                           // a caller that does not know the target
    Asset::Drawable(d) => {}, Asset::DrawableEnhanced(d) => {}, other => {},
}
```

Only the refusal a user reads says Enhanced. The second variant's name is the recommendation's spelling and is not ruled: code names a target Gen8 or Gen9, and the package that adds the drawable variants to `Asset`, which has none yet (#240), names it under that rule.

The opened file is an ordinary owned Rust value (`Vec`, `Option<Box<_>>`, `String`, arrays). There is no view, record or tree for the user to choose between; errors happen at open and at save and nowhere else; counts, pointers, alignment and padding are invisible, and no field holds the target. Engine type names are the API, with no `Ydr` alias and no wrapper type, so where the engine's names differ from the sample's they win: a `.ytd` is `PgDictionary<GrcTexture>`, whose textures are `entries` keyed by `codes`.

Built so far: each root in the Owned column of [R66d's table](Roadmap.md#r66d-declare-gta-v-format-by-format) opens, edits and saves owned — `CrFrameFilterDictionary::open`, field assignment, `Vec` edits, `save`, `save_as`, `peek` and the wrong-kind error — held by `gta5_frame_filter_dict` without game files and by each format's corpus suite (`every_yfd_opens_owned_and_saves_as_its_tree_saves`, `every_legacy_ytd_opens_owned_and_saves_as_its_tree_saves`). A `Vec` edit saves whatever `#[len]` kind counts it: the count is written from the new length, or the save is refused naming the run where the count cannot state it (#230, held by `declared_records`). An owned value shows no padding, count, reference count, vft word or runtime pointer, a save recomputes each derived field, and a pointer at what another pointer owns is derived, keyed or held once ([the owned form](#the-owned-form-d16), #236). `or_io::open` and `or_io::from_bytes` return an `Asset`, `#[non_exhaustive]`, with one variant per owned root, among them `FrameFilterDict`, `ExpressionDict`, `PathRegion`, `TextureDict` for a Legacy `.ytd` and `TextureDictGen9` for an Enhanced one (`or_io_opens_an_enhanced_ytd_as_its_gen9_variant`), and, since the drawable-bearing formats' migration, `Drawable`/`DrawableGen9` for `.ydr`, `DrawableDict`/`DrawableDictGen9` for `.ydd` and `Fragment`/`FragmentGen9` for `.yft` (P36, P42, P43, #297, #298, #309). A variant for a Gen9 root is spelled with its target, `Gen9`, because code names a target Gen8 or Gen9 and only text a user reads says Enhanced; the Gen8 variants keep their unsuffixed names. A path's extension picks the format and the file's RSC7 version its target; bytes alone are refused at a version two formats share; and a declared format or target with no owned form yet is refused naming both (#240); a `.ypt` opens owned at the target its version names (`a_ypt_opens_owned_at_the_target_its_version_names`). A Legacy `.ytd`'s textures are found, edited and inserted by name, and an insert keeps the codes ascending (`a_texture_is_found_and_inserted_by_its_name`, #240). An owned record holds an allocation whose regions are computed in a `#[codec]` field ([codecs](#codecs-for-computed-regions), #246); the drawable's material parameter blob and `.yft`'s vehicle-glass block are both built this way, the latter through `fragment::readers()`/`writers()` rather than a `#[codec]` field (OD-GLASS). `to_png`, `from_png` and `mesh()` are built, both editions: the texture codec (#268) and the mesh codec in `or_io::codec::mesh` (#300, P41), the latter over any `VertexSource` so the owned Legacy and Enhanced geometries both mesh themselves. [`Gta5`](#the-install) is built in `or-title`: `find`, `at`, `open`, `bytes`, `save_to_mod` (#285, P30); `GtaFragType::open` is built too (P43, #309). A material is edited through the owned drawable: `GrcInstanceData::insert_entry`, `GrmShaderGroup<Gen8>::embed` and `unembed`, and `RmcDrawable::move_shader` (P51, #318). A drawable-bearing save refuses a mesh defect and a buffer two geometries point at, and a vertex buffer's stride is derived at save (P50, #323). The app opens, edits and saves the Legacy and Enhanced `.ytd`, `.ydr`, `.ydd` and `.yft` as owned values with no pack (P56, #324; P58, #325), and reads no pack at all: every target of every declared format authors a blank, opens, saves and reopens with none (`every_declared_format_opens_with_no_pack`, P59, #327).

The sample in the engine's names is the acceptance check ([OD-SAMPLE](Project_Context.md#d16s-sample-stays-as-approved-and-an-engine-named-twin-compiles-2026-09-26)). It is the sample line for line, with each match arm on its own line, the imports a crate outside `or-io` needs, the texture dictionary opened from the same `path` and saved, and a vertex buffer's count read through an accessor ([OD-TWIN-VCOUNT](Project_Context.md#a-vertex-buffers-count-is-read-through-an-accessor-2026-09-28)). The file half is compiled by the `or-io` example in `crates/io/or-io/examples/the_target/`, and the install half by the `or-title` example in `crates/title/or-title/examples/install/`. Each holds only the lines built today, and `the_target_sample_runs_on_a_staged_file` runs the file half on a copy of each staged Legacy `.ytd`. `every_line_of_the_target_is_built_or_owed` holds every line below either in an example or in its list of owed lines, each owed line naming the package that builds it, and never in both:

```rust
use or_io::gta5::*;
use or_io::gta5::gen8::GrcTexture;
use or_io::Asset;

let mut car = GtaDrawable::open("police.ydr")?;
println!("{}", car.name);
println!("{:?}", car.lod_group.lod_distances);
if let Some(skel) = &car.skeleton { for bone in &skel.bones { println!("{} {:?}", bone.name, bone.translation); } }
for model in &car.lod_group.lods[0].models { for geom in &model.geometries { println!("{} verts", geom.vertex_buffer.vertex_count()); } }
car.lod_group.lod_distances[0] = 45.0;
car.name = "police2".into();
car.lod_group.lods[0].models.remove(1);
car.save()?;

let frag = GtaFragType::open("police.yft")?;
match or_io::open(path)? {
    Asset::Drawable(d) => println!("{}", d.name),
    Asset::Fragment(f) => println!("{}", f.name),
    Asset::TextureDict(t) => println!("{} textures", t.entries.len()),
    Asset::Clips(c) => println!("{} clips", c.entries.len()),
    other => println!("{:?}", other.origin().path()),
}

let gta = Gta5::find()?;
let mut car: GtaFragType = gta.open("police.yft")?;
gta.save_to_mod(&car, "mods/police.yft")?;

let mut ytd = PgDictionary::<GrcTexture>::open(path)?;
for tex in ytd.entries.iter().flatten() { tex.to_png(format!("out/{}.png", tex.name))?; }
ytd.insert(GrcTexture::from_png("decal.png")?);
ytd.save()?;
let mesh = car.lod_group.lods[0].models[0].geometries[0].mesh();

let mut map = CMapData::open("hills.ymap")?;
for e in &mut map.entities { if e.archetype_name == "prop_bench_01a" { e.position.z += 1.0; } }
map.save()?;
```

### The peek path and the generic tree

`Resource::peek` opens the decompressed image and returns the root's `View`: a field is a load, and a pointer is followed on request, checked for bounds and alignment once per view. The generic record tree, driven by a description rather than a type, serves tools that want every field without naming one — the inspector, the plugin boundary, `openrage-cli inspect` — and its names are the description's.

### The install

Finding an install, mounting it and searching it is a question about where bytes live, so `Gta5` belongs in `or-title`, not in `or-io`. Built in `crates/title/or-title/src/gta5.rs` (#285, P30):

```rust
impl Gta5 {
    pub fn find() -> Result<Gta5, InstallError>;                 // Steam, Rockstar, Epic; OPENRAGE_GAME_ROOT overrides
    pub fn at(root: impl AsRef<Path>) -> Result<Gta5, InstallError>;
    pub fn open<T: File>(&self, name: &str) -> Result<T, InstallError>;  // base archives, then DLC in load order
    pub fn bytes(&self, name: &str) -> Result<Vec<u8>, InstallError>;
    pub fn names(&self) -> Result<Vec<&str>, InstallError>;      // every file name the install resolves, lowercase and sorted
    pub fn save_to_mod<T: File>(&self, value: &T, path: impl AsRef<Path>) -> Result<(), InstallError>;
}
```

`names` took the place of the planned `locate`/`Located`: it does not yet say what overrides a name, only what resolves. `InstallError` is `#[non_exhaustive]` and carries `or_io::Error` through `File(#[from] or_io::Error)`. Saving to a mod folder refuses a path inside the install root, and `open` gives the same wrong-kind message as `T::open` when a name's extension is not `T`'s. `find` extracts the keys itself ([keys](#keys-d4)), taking the NG keys from the user's key snapshot or else the bundled one (D18); with `bundled-keys` off and no snapshot of the user's, it succeeds and every archived `open` fails naming what is missing.

### Saving: the canonical writer

Every save goes through the canonical writer, `or_io::write::build`, and its contract is:

- **Value equivalence on the first save.** Reading the saved file gives the same values as reading the original: every field, every alias, every member pointer on its member.
- **A fixed point after it.** Saving an unchanged document again is byte-identical, and two runs produce the same bytes.
- **One canonical form.** One allocation order, one alignment rule, one choice wherever the format admits several. The writer copies only the byte ranges a field describes, writes every pointer slot itself and leaves a byte no field covers zero, so a codec zeroes the pointer slots it carries inside a blob.
- **Capacities carried.** A counted array's capacity is an allocation the game sized (`PhOptimizedBvh::num_nodes` is one), so a save keeps the larger of it and the length rather than shrinking it. An `atArray` whose pointer aliases another's run states that run's count and capacity (`an_aliased_counted_array_states_its_owners_count_and_capacity`, #307).
- **Validation before bytes.** A format's structural invariants — two geometries sharing a buffer, counts that contradict their runs, an unsorted binary-searched table — run over the built image before a byte is returned, through each `gta5` module's `validate`. An unchecked save stays for repair tools.

Byte-identity with the shipped file is reported where it falls out and never required. On the pack path `a_record_saves_to_a_fixed_point` holds the contract on 233 of 234 files across the ten first written rows, 4 of 4 Gen8 and 2 of 2 Gen9 `.ypt`, and 60 of 60 `.ycd` (#197); the 234th, `server_assets.ytd`, is refused because a texture states a zero payload beside a live pointer, which the owned path reads at its mip chain's length (#308).

⚠ A save that writes null where the file holds a pointer, or copies an address carried as a number, is a correctness defect by the owner's ruling of 2026-09-24, however cleanly the file reloads. `what_a_save_drops_is_the_list` names each site a save still loses, per format and target, and fails when one starts or stops losing data; they are [R66d](Roadmap.md#r66d-declare-gta-v-format-by-format)'s lossless-save item.

### Errors

Reading, viewing, opening and saving a file return one `#[non_exhaustive]` error, `or_io::Error` (#240); the description and writer layers beneath keep their own (`BuildError`, `PlanError` and the rest). The container and pack path takes an `ErrorMode`: `Lenient` skips a bad child with a diagnostic and goes on, `Strict` stops at the first. A view's error names the type and the offset or field of an out-of-bounds, misaligned, bad-pointer or not-a-member failure. A save the format's validation refuses is `Error::Refused`, which carries the format's own error whole, reached through `source()`, so a caller can downcast it to that format's `Broken` (`a_refused_save_carries_its_typed_reason`). `IoError`, `OwnedError` and `ViewError` remain as aliases of it until R66f, because the `gta5` modules and the verify suites still name them. Diagnostics are collected rather than logged, so a library user decides what to print.

### Features

`or-io` declares `gta5`, `gen9`, `archives`, `crypto`, `bundled-keys`, `dds`, `fxc`, `awc`, `gfx`, `rel`, `mmap`, `rayon`, `meta`, `authoring` and `bc7-encode`, and `crates/io/or-io/Cargo.toml` says what each turns on; the rest of the table is planned:

| Feature | Default | Adds |
|---|---|---|
| `std` | yes | files and `std::error::Error` |
| `gta5`, `gen9` | yes | GTA V's declarations; `gen9` off builds Gen8 only |
| `archives` | yes | RPF7 |
| `crypto` | yes | key extraction from the user's executable and decryption; ships no key material |
| `bundled-keys` | yes | CodeWalker's key snapshot, embedded (D18); decodes only with the AES key from the user's executable; implies `crypto` |
| `dds` | yes | DDS read and write |
| `meta` | no | PSO, RBM, RBF and their XML canonical form (D5), no schema bundled |
| `fxc`, `awc` | no | shader containers; audio |
| `gfx` | no | `.gfx` Scaleform movies, each tag kept as its bytes, and the SWF they export to |
| `rel` | no | `.rel` audio metadata containers, each item kept as its bytes |
| `authoring` | no | BCn encode, mip chains, PNG, texture import (D11) |
| `bc7-encode` | no | BC7 through `intel_tex_2`, the one native link; implies `authoring` |
| `mmap`, `rayon` | no | the executable mapped for key extraction; the key scan and the NG encrypt derivation in parallel |
| `serde`, `glam`, `tracing` | no | the generic tree through serde; math conversions; diagnostics also as events |
### Other targets and languages

- **WASM.** Hosted CI builds `or-io` for `wasm32-unknown-unknown` with every dependency it has (`.github/workflows/ci.yml`). Two users want it: a web viewer, and in-app plugins, which can link `or-io` compiled to WASM and get typed access to bytes the host passes instead of only the named-field records the plugin ABI carries.
- **no_std in stages**, a goal for 1.0 and not a promise for 0.x: the declarations and views need only `core`; records, the tree and the writer need `alloc`; files need `std`.
- **A C ABI** as a separate crate, `or-io-capi`, generated with cbindgen: opaque handles, open, detect, the generic tree walk, the neutral mesh and texture decoders, save. Typed structs do not cross a C boundary; the tree does, and Python and C# bindings sit on it.
- **Other languages without linking** read an exported description, as JSON.

### Versioning

`or-io` stays 0.x while declarations settle: a minor version may rename a field or type, a patch version may not. A struct is built through `Default` or a constructor, never a literal, so a declaration that gains a field breaks nobody. After 1.0, renaming a public field or type is a major version, and adding a declared type or variant is minor. The minimum Rust is the workspace's `rust-version`, 1.95, and raising it is a minor version. `or-io-macros` is published at the same version, pinned exactly.

### Performance

| Operation | Target | Measured |
|---|---|---|
| A field read through a view | a plain struct's field load | 0.079 ns a field, typed and plain, 1.00×; a view checked per record 1.17× (`what_a_typed_read_costs`, release, 1,006 records × 41 fields, best of 3 blocks of 4,000 rounds, three runs on a quiet machine, #201) |
| Following a pointer | one mask, one range check, one alignment check | — |
| Opening an owned file, saving it, peeking | against the tree | `what_an_owned_file_costs` prints each |
| A whole-file record read | at or under the compiled reader | 0.57–1.00× per format and target (`what_a_whole_file_record_read_costs_against_the_compiled_reader`, release, median of five, #196) |

## What carries over from the pack-native design

The design that answered the 2026-09-23 ruling (superseded by D1) built machinery `or-io` keeps as its generic path, in `or_io::desc` and `or_io::write`. The owned form reads and saves through it, and the inspector, plugins, runtime descriptions and the testimony run on it; only a typed view needs none of it.

- **Type plans.** `Pack::plans` builds one `TypePlan` per described type on first ask: the type's size and a table from each field's path to its offset and scalar kind, every field checked at build time to fit inside its type. That check is what makes one bounds check per record (`TypePlan::record`) enough for every field read after it. `at!` memoises `TypePlan::slot` per call site against the plan's epoch.
- **Records.** `Value::Struct(Record)`: a plan, so its pack and target; the struct's own bytes; and one child per pointer, pointer table and trailing run, keyed by offset. A codec's container is `Value::Blob`. The builder refuses a record read with another pack (`BuildError::WrongPack`).
- **Packed runs and the shared image.** A run of pointer-free structs is one block, `Value::Records(RecordRun)`, packed or not by the type (`is_packable`) and never by the instance, so an element whose pointers are all null still has somewhere to put one. A read shares one copy of the image's system-memory half among its records, and a plan carries the steps a read takes for a record's children, so a read neither scans scalars nor looks a type up by name per pointer.
- **Member pointers and generic layouts**, [above](#member-pointers-and-generic-layouts).
- **The testimony asked of the tree.** `DeclaredReading` finds each recorded instance where the tree filed it — through a pointer's address, a packed run's stride, an embedded struct's offset — so a reader that files a struct under the wrong element is named even when the image agrees.
- **Synthetic test packs.** A test pack with invented type and field names at invented offsets is allowed in the tree; one placing a real type at its real offsets is not.

⚠ A checked read per field costs twice the read — 0.31 against 0.16 ns on the synthetic pack (#195) — so a hot loop binds its slots through `TypePlan::pass`, whose lifetime brand lets a bound slot read a checked record with no compare. A hashed lookup on every read costs 16 ns, because `TypePlan::slot` hashes the path at run time, which is why `at!` memoises it. Measured in #195 on `84bca67e`, release, best of three blocks of 4,000 rounds, three runs, 41 scalars a record over 1,006 records; the typed struct is a floor, because summing contiguous fields auto-vectorises where scattered reads in real code do not:

| | typed struct | plan pass |
|---|---:|---:|
| synthetic pack, `what_a_plan_read_costs` | 0.078–0.080 ns | 0.126–0.130 ns |
| corpus `PtxEffectRule`, `what_a_pack_read_costs` | 0.084–0.085 ns | 0.143–0.145 ns |

Packing runs alone did not speed up `.ypt`: its 40,231 keyframe-entry runs hold 48,371 entries, 1.2 a run, and the cost was allocating and copying each of its records over 48 bytes (97,000 of its 132,000), which sharing the image removed (#196).

## Verification

Every claim the crate makes is held by a check that can fail. The per-format gates below need a staged corpus, and run locally under `just test`; the self-hosted `layouts` job runs them and the testimony gate in `or-io-verify`, then R66e's behaviour baselines, on the same corpus. The synthetic checks run on hosted CI.

### The gates per format

A format is declared once it has `#[record]` declarations and a row in `FORMATS`, and a format and target is finished in R66d when these hold, each in the format's suite in `crates/io/or-io-verify/tests/`, whose shared half is `crates/io/or-io-verify/src/gates.rs`, written as tests by `declared_gates!`:

1. Typed reads equal the tree's on every corpus file (`every_yfd_reads_typed_as_the_tree_reads` is the shape).
2. Typed reads equal the compiled reader's while it existed, where a comparison suite existed; since R66f deleted the compiled readers, the testimony, their own recorded answers, stands in for every format.
3. The recorded testimony, re-keyed by offset, shows 0 differences with reach asserted.
4. The canonical save is a fixed point and refuses what the game would misread; an owned format also passes `declared::owned_saves_as_tree`.
5. The alignment census finds no misaligned instance of an aligned type, in shipped files and in saved ones (`every_yfd_instance_is_aligned_for_its_view`).
6. The structural diff against the private description is empty, or every difference is a recorded correction with its reason. Retired by D17 (2026-09-26) format by format, in the pull request that migrates the format to the owned form, after its last output is logged; `.yfd`, `.ytd`, `.yed` and `.ynd`, owned before D17, retired it in #237, and every other format's was gone by P44 (#315), so no gate-6 suite remains.
7. Every name is the GTA source's, no offset is hand-written and no field is `unknown_*` (D15).

[R66d](Roadmap.md#r66d-declare-gta-v-format-by-format) records each format's figures against these.

### The recorded testimony

The testimony is the compiled readers' answers for every field of every struct they read, over the corpus and both retail installs: about 3.8 billion values, 0 differences through the pack at #192. It lives in `openrage-corpus/testimony` (79 files, 13.5 GiB), the owner's exception to that repository holding only game files, and stays there; `the_recorded_testimony_holds` finds it by default, and `OPENRAGE_TESTIMONY` names another copy for a one-off check. The exception covers the testimony's frozen key too (OD-TESTIMONY, 2026-09-26): the offsets of its pack-witnessed fields, taken once from the version 12 pack and never retaken (OD-TESTIMONY-KEY-FROZEN), so the check outlives the pack. The suite moved into `or-io-verify` under its name (P62), and nothing new is recorded since the compiled readers went. [Oracle_Testimony.md](../guides/Oracle_Testimony.md) owns the record and why it is the check that survives the compiled readers.

Re-keyed by address and offset (`DeclaredReading` in `or_io_verify::testimony`), the check asks which declared type sits at a recorded address and compares each recorded field by offset and width, so no name crosses and the check is public. Its rules:

- **A recorded field is compared only where declared fields tile it exactly**, because an enclosing embedded field reads the same bytes whatever its members' offsets and would agree with any layout. A span inside one scalar counts at any depth, a counted array's pointer, count and capacity tile it, and a byte array or declared padding tiles byte by byte; tiling at the finer boundary is stricter, and `.yed` answers 718,581 of 718,581 recorded fields under it (`crates/io/or-io-verify/tests/expression_dict.rs`, #220). Taken without their relocation set-asides, the Gen9 drawable falsifiers catch 17 and 4, because the view fields the older recorder read eight bytes early land inside a scalar of the view; with the set-asides they catch 183 and 17 (#220).
- **It asserts reach.** A declaration that misreads self-consistently still round-trips, and only the count of unreached instances says so.
- **Zero bytes after a record, up to its allocation's 16-byte alignment, are padding.** A recorded field that starts at or past the end of the longest record filed at its address, ends by the next 16-byte boundary, lies inside no filed record and reads all zero is counted as `padding`, apart from agreement, so the figure stays visible; one non-zero byte, or a field past the boundary, leaves it unanswered (`zero_bytes_past_a_record_up_to_its_sixteen_byte_alignment_are_padding`). A compiled reader that read a fixed-size struct where a declaration files a shorter record, such as a page map with one page slot, read that padding.
- **A record read through a pack** whose layout table held a Rust struct that no longer mirrored the file takes its offsets from the frozen key (`or_io_verify::testimony::Key`, #237), keyed by the recorded names, which stay private, so the check reads no pack; a key row whose width is not the recorded width is a discrepancy. A name-free `FieldMoved` answers a field the pack has since moved, and a record-scoped `Relocated` excuses an instance only at a stated offset into a declared record; each is still held to the file's bytes.

### The padding and variant censuses

Both run over every `FORMATS` row and target on the corpus, in `or-io-verify` (#242):

- **Padding.** `census::regions` lists every region a description does not claim as data: declared padding, opaque bytes, pointers at nothing described, and bytes no field covers. `census::liveness` measures each in shipped files and in their canonical saves, and `what_the_padding_holds` holds every live region to a recorded reason and its fate on the save: a runtime slot the writer rebuilds, a payload the save carries, a word written null by decision (`census::RULED`), which must lead to no non-zero bytes in a shipped file, or data a save loses (`census::STILL_LOST`). A reason for a region that is not live fails, a region written null that neither list names for its format and target fails, and a lost region fails once a save carries it. Each format's census also fails on a saved `pgBase` with a zero vtable (`a_saved_pg_base_never_has_a_zero_vtable`).
- **Variants and reach.** `which_variants_occur` fails on a tag value no declared variant answers, unless a row records it as the base's own layout, as a collision polygon's triangle is, and prints each declared variant and type no file reaches.
- **What the private description added.** Until P62 deleted pack discovery, a census paired the built-in description with the staged private one by address, never by name, and printed in the built-in names the bit runs, enum values and notes the private one carried and the built-in one lacked (`diff::annotations`): what the declarations have yet to say. It went with the last reader of the frozen pack.

### The structural diff

`or_io_verify::diff::structural` walks two descriptions from a root in parallel and compares each type's size and each field's offset, width, kind and pointer target, pairing by position or by address from the testimony and never by name, so the tool is public. Where a declaration was drafted from another description, this diff is a regression check rather than a second opinion; the independent evidence is the testimony, the corpus censuses and CodeWalker. As a gate it is retired for every format under D17 ([gate 6](#the-gates-per-format)), and `census_and_diff` holds the tool on synthetic types.

### Without game files

Hosted CI has no corpus and never will. `crates/io/or-io/tests/synthetic/mod.rs` declares invented types exercising every feature, and `declared_records` reads a hand-built resource typed and through the tree, saves it to a fixed point and holds each type's description to `offset_of!`. `decl::refusals` holds the compile-time refusals as doctests, `census_and_diff` holds the census and the diff, and `gta5_frame_filter_dict` holds the owned form. The lints run on every push to a non-draft pull request, and the `wasm32` build on every such push that changes code.

### Lints

`scripts/lint_provenance.ps1` fails on tracked game content and on leaked-source path shapes and source-file-and-line citations, in every tracked file, because the whole repository is published with the crate (D8). Engine names are not a pattern (D15): the lint has no engine-name rule and there is no denylist. How figures in these documents are written, and the gates that read them, is [Documentation_Standard.md](../guides/Documentation_Standard.md).

<a id="84-keys-d4"></a>

## Keys (D4)

Keys come from extraction from the user's own executable (D4), and NG material from a key snapshot: the user's own, or else CodeWalker's, which `or-io` bundles behind the default-on `bundled-keys` feature (D18).

`container::rpf::crypto` scans the executable the user names — a sliding SHA-1 match of the file against fingerprints of the key material — and caches the result beside the user's configuration. Nothing is extracted without the user naming their executable, and nothing leaves their machine. What the scan finds in every shipped executable (`what_the_extractor_recovers_per_executable`, over two Legacy `GTA5.exe` builds, two `GTA5_Enhanced.exe` builds and the Enhanced copy a FiveM cache holds, #202):

| Class | Found per executable |
|---|---|
| AES key | 1 of 1 |
| hash lookup table | 1 of 1 |
| NG keys | 0 of 101 |
| NG decrypt tables | 0 of 272 |
| AWC key | 1 of 1 |

The unpacked dumps beside the Legacy installs carry every NG decrypt table and still no NG key. AES-encrypted archives and scrambled `.awc` therefore open from extraction alone.

⚠ Every on-disk `.rpf` in both retail installs, 179 of 179, is NG-encrypted at the top level (#202), so mounting a retail install needs a key snapshot. A snapshot of the user's comes first and the bundled one after it (`SnapshotSource::or_bundled`): the app and `openrage-cli` look for `magic.dat` beside the keys cache (`openrage-cli rpf extract-keys --magic` names one), and the library's default path (`extract_keys_with`), which the install-gated suites and the shader tool use, reads `OPENRAGE_KEY_SNAPSHOT`. A snapshot found beside the keys cache or named by `OPENRAGE_KEY_SNAPSHOT` that does not decode or describes another build falls back to the bundled one; one named with `--magic` or picked in the app is used alone. With `bundled-keys` off and no snapshot of the user's, each NG archive is refused with a message saying what is missing and where to put it. No install-gated suite therefore needs `OPENRAGE_KEY_SNAPSHOT`. A keys cache an earlier build wrote without NG keys (schema 3) is extracted again once, when its executable is present, and serves its AES keys until then.

The bundled snapshot is CodeWalker's 154,069-byte `magic.dat`, masked with a stream seeded from the game's AES key and encrypted under it, so it decodes only with the key extracted from the user's own executable. It ships once, as `crates/io/or-io/data/magic.dat`, embedded by `container::rpf::crypto::magic` behind `bundled-keys` (D18); `the_key_snapshot_is_in_the_tree_once` fails on a second copy under any name or a second embed. From R66b to D18 no snapshot was in the tree. The fingerprint table extraction matches against ships too: digests of key material, CodeWalker-derived and ported under D7. Both are lawyer question 4. The key extraction and key-cache code is partly ported from CodeWalker and attributed in `LICENSE-third-party.md`.

## Legal and provenance

> Not legal advice: an engineer's reading of public sources, written so the owner can brief a lawyer. The questions for one are parked (D14) and gate only P7.

Layouts are facts and can be published with a record of how they were measured. Code or comments taken from the leak, citations of it, keys other than the key snapshot D18 ships, and game content cannot. Names derived from the source are in the public crate by the owner's decision (D15), and stay listed as a risk. The file-level inventory, which has to name the leaked tree, is [OR_IO_Leak_Cleanup.md](../extra/OR_IO_Leak_Cleanup.md).

### Where the risk is

Take-Two has acted against decompiled or leaked source code and documents taken from it (including documentation repositories made mostly of names), online-affecting tools, and ports of its content. Format tools publishing layouts — CodeWalker, Sollumz, OpenIV, community parser dumps — have stood for years.

| Kind | Risk | Why |
|---|---|---|
| Layouts: offsets, sizes, strides, enum values, bit positions | low | facts; interoperability; strongest with a measurement per fact |
| Names | low for generic or retail-visible names; medium for names only the leak carries | one name is not protected; a large set taken from stolen source is evidence of access, arguably a compilation, and what draws takedown notices |
| Code, comments, structure translated from the leak | high | a translation is a derivative work, and a citation of the tree proves access |
| Keys and key material | high to ship; medium to extract from the user's copy | the clearest circumvention-trafficking pattern |

D15 accepts the names risk for the public crate, as D9 and D13 accept the particle simulation's, the fallback shaders', the VFS device code's and the particle reset values' — each mirrors behaviour the leaked source documents and ships at full fidelity. D18 accepts the key snapshot's the same way, and it stays lawyer question 4.

### What never enters the crate

- Code, comments or structure translated or paraphrased from the leaked source, and any constant whose only source is the leak.
- Citations of the leaked source by path, file name or line.
- Formulas that derive values from engine-named constants; the RSC7 version table is measured.
- Code from a source whose authors have not permitted it. CodeWalker and CodeX ports stay under D7, attributed in `LICENSE-third-party.md`; RAGE-Tools and PyMateria code never enters. Facts measured with any of them as an oracle are fine.
- Keys and key material, with two exceptions: the fingerprints extraction matches against, and CodeWalker's key snapshot, bundled behind `bundled-keys` by the owner's D18.
- Game content: files, excerpts, transcribed tables. Parsers read the user's install; nothing is bundled as a fallback.
- The metadata schema snapshot.
- Anything that talks to a network.

### Cleanup before publishing

| # | Item | Where it stands |
|---|---|---|
| 1 | Behaviour mirrored from the leaked source: the particle simulation, the fallback shaders, the VFS device code, the particle reset values; three documents with line-level citations | kept at full fidelity (D9, D13); the citations removed from the tree in R66b, from history in P6 |
| 2 | A tracked test that walked the leaked source tree | deleted from the tree in R66b; from every commit in P6 |
| 3 | History: content citing leaked paths, commit messages naming engine source files | rewritten in place in P6 (D3) |
| 4 | The embedded key snapshot | removed from the tree in R66b; shipped again in `or-io` behind `bundled-keys` by D18 (2026-09-28), so neither the tree nor history drops it; lawyer question 4 |
| 5, 6 | Crypto ported from CodeWalker, metadata readers ported from CodeX | kept (D7); `LICENSE-third-party.md` names the ported modules since R66b |
| 7 | Engine names in public code | the declarations take the source's names (D15); `or-format-api`'s `names` and `or-asset`'s compiled structs went in R66f (P62, #332; P63, #335) |
| 8 | Leak-derived constant tables and version formulas | re-measured in R66b |
| 9 | A bundled 212-entry game-derived material table and CodeWalker's fallback colour | dropped in R66b; the palette is read from the user's install |
| 10 | The schema snapshot | never committed and never in a release; nothing compiles one in, and the editor and the CLI install the one they find at run time (`or_io::meta::install_snapshot`, [metadata](#metadata)) |
| 11 | Trademark: the name, the wording, the app logo | before P7: a not-affiliated statement, no Rockstar artwork, a trademark search on the name |
| 12 | The gitignored layout tier, the pack sources, the corpus, the testimony | private; never in a release |
| 13 | Dependency licences | `cargo-deny` in CI before P7 |

### Questions for a lawyer

Parked (D14); they gate P7 only.

1. Are measured layouts of GTA V's formats free to publish, in the US and in the owner's EU jurisdiction — including facts first seen in the leak and then independently confirmed against retail files?
2. Is a set of engine identifiers taken from the leaked source protectable (compilation, trade secret after a public leak), and does using them as the public crate's names (D15) create "reason to know" exposure?
3. The particle simulation, the renderer's fallback shaders, the VFS device code and the particle reset values mirror behaviour documented in the leaked engine source, and the owner has decided to publish them at full fidelity rather than re-derive them (D9, D13). What is the exposure, and does removing the citations to that source from the tree and from its history change it?
4. Keys are extracted from the user's own executable (D4), using a table of SHA-1 fingerprints of the key material that ships in the crate, and the crate also ships CodeWalker's key snapshot (D18), which carries the NG keys encrypted under the AES key that only the user's executable supplies. Is that circumvention "trafficking" (17 U.S.C. §1201; EU TPM rules)? Does the interoperability exemption cover it? Does publishing it on crates.io, where code cannot be withdrawn, change the answer?
5. Are the game's EULA anti-reverse-engineering terms enforceable against the developers where they live?
6. Trademark: may the project's name contain "RAGE"? What disclaimer and wording? The crates are `or-io` and `or-io-macros` (D2).
7. Does a general editor that can author maps and DLC packs fall inside the exclusions of Rockstar's single-player mod policy?
8. The repository's history is rewritten in place to remove citations, documents and a test about the leaked source, and the same repository is then made public (D3). Is a rewritten history sufficient? Old commits stay reachable on GitHub through pull-request references until GitHub removes them, and both old revisions and the current one carry engine-named code (D15). Does either create exposure, and does keeping private pre-rewrite copies create any obligation?
9. Code ported from CodeWalker and CodeX, which carry no public licence, is kept on the strength of permission the owner holds from their authors (D7), with an attribution entry and no written grant. Is that enough to publish the ported code under the crate's MIT licence, or does redistribution or sublicensing need something more?
10. May names Rockstar itself publishes — in PyMateria's API, whose licence forbids reverse engineering — be used as identifiers?
11. Is the testimony — values read from game files, recorded privately — safe to rely on as evidence without ever publishing it?

## Rewriting the history

This repository is published as it is (D3), so its history is rewritten first: a small, named set of paths and text leaves every commit, everything else stays, and the result is trusted only after a scan of it has been shown failing on a planted leak.

### What the history holds

| | |
|---|---|
| Commit messages naming engine source files or the particle engine's source directory | 8, on 2026-09-24 |
| Commits whose changes contain the leaked tree's root directory name / its source-path prefix | 21 / 16, on 2026-09-24 |
| Paths `magic.dat` has had | 5, by the `crates/` layout of each era; 7 commits on `origin/main` touch it (at `0c091de4`) |
| The schema snapshot and a `.ydr` fixture | `schema_snapshot.bin` and `cubic_meter_orig.ydr`, retracted from a branch and still reachable from `main` through the merge of the unrewritten old `main` in #1 |
| Commits touching the gitignored layout tier | none |

The patterns behind the first two rows name the leaked tree, so they are in [the private note](../extra/OR_IO_Leak_Cleanup.md). A crate called `or-io` in an old revision's flat `crates` directory is a pre-V2 crate, not this one; paths are matched by basename, so it is covered.

### What is scrubbed

1. **Removed from every commit**, matched by basename so a renamed copy is caught: the coverage test that walked the leaked tree (both paths it has had), `schema_snapshot.bin` and `cubic_meter_orig.ydr`. `magic.dat` is no longer removed: D18 ships it.
2. **Redacted in every blob**: citations of the leaked source — paths, file-and-line references, header names — in comments and documents, through an expressions file kept in the private note.
3. **Reworded**: the commit messages that name engine source files, by the same expressions. Every other byte of every message is kept, so each `(#N)` marker survives.
4. **Landed first**: the same cleanup lands on `main` as ordinary commits (R66b, R66f), so the rewrite only carries the tip's state backwards. It removes; it never adds code to an old revision.
5. **Not scrubbed**: code. The particle simulation stays (D9), and engine-named types in old revisions keep the engine's names (D12, taken on 2026-10-01). Layouts are facts.

### Procedure

`git filter-repo` runs in a fresh `git clone --mirror`, never in the checkout: filter-repo refuses a non-fresh clone, and the checkout's object store is shared by every lane worktree, whose branches, reflogs and stashes keep every old object reachable. It takes the basename path list (`--invert-paths`), `--replace-text` and `--replace-message` with the private expressions file, and writes a map from old to new commits, kept privately, which is what moves unlanded work.

1. **Freeze.** No pull request open, no lane pushing, and `just janitor` run so every landed lane is gone.
2. **Land the tip's cleanup** on `main` through a normal pull request.
3. **Mirror and rewrite.**
4. **Verify** (below), and stop on any failure.
5. **Push** every branch and tag from the mirror with `--force`, then ask GitHub Support to purge cached views and pull-request references: a user cannot rewrite `refs/pull/*`, so an old commit stays reachable through the pull request that introduced it until Support removes it.
6. **Re-clone** the checkout and recreate lanes from the new history. The old checkout holds everything the rewrite removed; it is kept offline as a private backup or deleted, and never pushed.

| Around the rewrite | Effect | Handling |
|---|---|---|
| Open pull requests | their base is replaced, so every rewritten commit shows as a change | none may be open |
| Merged and closed pull requests | GitHub keeps their old head and merge commits under `refs/pull/*` | the Support purge |
| Forks | a fork keeps the old history | the repository becomes public only after the purge |
| Lanes and worktrees | they share the old object store and sit on old commits | landed lanes removed first; an unlanded branch moves with `git rebase --onto` through the commit map into a new lane |
| Janitor landing detection | a lane at an exact old head still matches the pull request that recorded it, and the squash-and-close test still works because `(#N)` subjects are preserved | run the janitor before the rewrite; a moved lane has a new head no pull request records, so it is held as unlanded |
| Self-hosted runner | its checkout and persistent target directory hold old objects and build output | wipe both; the first build after is cold |
| Actions caches, sccache, lane target directories | build output from old sources | purge the Actions caches; the local ones go with the lanes |
| Tags and releases | tags move to new commits | a release artefact built before the rewrite is not republished |

### Verification

`scripts/lint_provenance.ps1` has a history mode that reads every commit message and every blob reachable from every ref of a given repository, and runs with the private expressions file.

1. **Falsify first.** In a throwaway copy of the rewritten mirror, add one commit carrying a planted leak citation in a comment and one in its commit message. The scan must fail and name each plant by commit, path and line; a scan that has not failed on this copy is not trusted.
2. **Scan the rewritten mirror.** The pattern lint and the private expressions find nothing in any blob or message.
3. **Structural checks.** No commit contains a removed path, and no commit touches the gitignored layout tier.
4. **The product did not change.** The rewritten `main`'s tip tree is identical to the pre-rewrite tip, because the cleanup had already landed, and `main` has the same number of commits with every `(#N)` subject in the same order.

## Migration phases

Phases P0–P7 are [R66a–R66h](Roadmap.md#r66-the-or-io-crate); the roadmap holds each one's done-when and where it stands, and lands every phase under the ordinary lane, push and merge rules.

- **P0, [R66a](Roadmap.md#r66a-decide-and-record-the-direction): decide.** Done: D1–D14 recorded, the standing documents pointed here, R66 on the roadmap.
- **P1, [R66b](Roadmap.md#r66b-clean-what-the-crate-will-contain): clean what the crate will contain.** Done: the key snapshot out of the tree, the extractor measured per executable, the version table measured, the bundled material table dropped, the attribution corrected, citations out of the tree, and the pattern lint fatal on every crate that moves in. The engine-name rule and the denylist it also built were withdrawn by D15, and D18 bundles the key snapshot again in `or-io`.
- **P2, [R66c](Roadmap.md#r66c-the-crate-and-the-macro-on-synthetic-types): the crate and the macro on synthetic types.** Done: `or-io` and `or-io-macros` exist, the six crates merged with no facade, and one declaration makes a struct, a view, a record and a description.
- **P3, [R66d](Roadmap.md#r66d-declare-gta-v-format-by-format): declare GTA V format by format.** Done: every paged format `FORMATS` lists and the RSC7 header are declared, and every root in the Owned column of [R66d's table](Roadmap.md#r66d-declare-gta-v-format-by-format) is migrated to the owned form under D15's names; RPF7's header and entry table are declared (#262); the pose database moved in as ordinary code (OD-YPDB, #254); GXT2 is declared and owned as `Gxt2` (#265); `or-meta` moved in whole as `or_io::meta` (#267); AWC and DDS moved in as ordinary code behind features (#266), and the texture codec and authoring path behind them (#268). Carried past it: `.ysc` stays ordinary code under a time-limited exception, declared once a corpus exists (OD-YSC); one site a save still loses (`census::STILL_LOST`); and `#[bits]` exists in `#[record]` but no GTA V resource declaration names its bit ranges with it yet (OD-BITS). What `or-asset` still held went with it in R66f (P62, #332). Every declaration is owned, the drawables one generic declaration per class (OD-10.8) and Gen9-only words under measured names (OD-10.9), and `#[record(raw, ...)]` is retired (P57, #319). Done when [the gates per format](#the-gates-per-format) hold for every format and target, and for metadata when every metadata suite is green in its new home and every document still reaches its fixed point.
- **P4, [R66e](Roadmap.md#r66e-move-the-consumers): move the consumers.** Done (P39–P60): the viewport mesh walk (`src/render/meshes.rs`), `or-interchange` for both targets, `or-anim`, `or-simulation` and `or-particle` with behaviour unchanged (D9), the editors, `or-cli`, and the app's one resource payload, onto typed values and views; `src/state/payload_handlers.rs` stays the one table of each format's open, save and empty, and plugins keep their ABI. Done when no crate outside `or-io` names `or-asset`'s types or `or-format-api`'s `names`, `just self-check` paints every panel, and every corpus drawable exports the same `.glb` content, every clip the same poses frame for frame, every effect the same deterministic simulation snapshot, and every documented CLI command the same values.
- **P5, [R66f](Roadmap.md#r66f-delete-the-old-crates): delete the old.** Done (P61–P67): `or-asset` (D10; `or-meta` left already, whole, in R66d, #267, D5), `or_asset::rage_types` and the domain glue, the compiled readers and writers once the testimony runs green against the built-in description, `pack_required`, `pack_walk`, `#[rage_struct]`'s code generators, the `inventory` registration and `or-format-api`'s `names`. `openrage-packs` is no longer read by the app and stays the second description the structural diff runs against, authored in what [the open question](#open-questions-for-the-owner) settles. That sentence superseded on 2026-09-26 by D17: nothing reads the pack after R66f, and [R66i](Roadmap.md#r66i-retire-openrage-packs) retires it. Done when those crates are gone, the pattern lint is clean on the tip, the workspace opens every declared format with no pack, and the standing documents describe the result.
- **P6, [R66g](Roadmap.md#r66g-rewrite-the-history): rewrite the history** as [above](#rewriting-the-history). D12, taken on 2026-10-01, keeps the engine's names in old revisions, so the rewrite renames nothing; since D15 the tip carries engine names as well.
- **P7, [R66h](Roadmap.md#r66h-publish-the-crate-and-the-editor-together): publish the crate and the editor together** (D8): a README with the not-affiliated statement, docs.rs configuration, this repository public, and `or-io` and `or-io-macros` 0.1.0 on crates.io the same day, with `publish` flipped for those two. Done when the lawyer questions are answered, the pattern lint is clean on the tip, a clean clone passes hosted CI with the synthetic round trips, `cargo-deny` is clean, and `cargo publish --dry-run` succeeds for both crates from the published package alone.

## Open questions for the owner

The lawyer questions gate P7; these block work before it.

- **10.8, one type per engine class, or one per edition?** D16's sample opens drawables of both editions with `GtaDrawable::open`. Where the engine's layouts differ (textures, vertex and index buffers, shaders), one Rust type cannot hold both without the per-target field presence [forbidden above](#gen8-and-gen9). As built so far: the two textures are separate types, the Gen8 one owned and the Gen9 one raw (10.9), and `or_io::open`, which would dispatch on the header, does not exist yet. The alternative is an owned enum per root (`GtaDrawable::Legacy(..)` and `::Enhanced(..)`), which breaks the sample's field access. Answered on 2026-09-26 by [OD-10.8](Project_Context.md#drawables-are-one-generic-declaration-per-class-2026-09-26), under the owner's standing instruction for R66: one generic declaration per class, which amends D16 as [the target](#the-target-d16) shows.
- **10.9, names for Gen9-only fields.** D15 names every field from the GTA source, and the source predates Enhanced: its texture header, shader resource view and the Gen9 buffer words have no declaration there, and CodeWalker calls them `Unknown_*` too. The Gen9 texture stays in the raw form until a naming rule for them is chosen. Answered on 2026-09-26 by [OD-10.9](Project_Context.md#gen9-only-words-take-measured-names-2026-09-26), under the same instruction: measured names, and Direct3D's names for an embedded Direct3D structure; bytes are carried only case by case with a recorded reason, never as padding.
- **The pose database's scope.** Whether YPDB is declared, as a format derived from the engine's own deserializer, or stays ordinary code in the crate. Answered on 2026-09-26 by [OD-YPDB](Project_Context.md#the-pose-database-is-ordinary-code-2026-09-26), under the same instruction: ordinary code in the crate.
- **What the private pack is written in after R66f.** Its descriptions are `#[rage_struct(layout_only)]` structs and `#[rage_enum]` enums from `or-macros`, collected by the `inventory` registration its exporter reads, and R66f deletes the code generators and that registration while the pack stays the structural diff's second description. Answered on 2026-09-26 by D17: nothing, because the pack is retired ([R66i](Roadmap.md#r66i-retire-openrage-packs)).

## Alternatives considered

| Alternative | Why not |
|---|---|
| Pack-only reads with roles (the 2026-09-24 design) | names still reach public source as strings; a typo fails at run time; a field read costs 1.6–2.0× a struct (#195); a clean checkout opens almost nothing |
| A private typed SDK generated from the pack, loaded by a public host as a module | its prototype read a real drawable through a module built against a generated SDK, but every feature that names a field lives in the private repository, so the public editor is a shell and external developers get nothing |
| A macro rewriting `d.name` into memoised pack reads | real field syntax, but the names are still in source, untyped positions read a run-time-typed value, and file-level use needs a nightly feature |
| Code generation from a schema file (Kaitai-style) | loses `offset_of!`, IDE support and the existing macro; worth it only for non-Rust readers, which a description export serves |
| A runtime pack with typed accessors generated from a public schema | methods rather than fields, about twice a struct per read, and still needs a pack to open anything |
