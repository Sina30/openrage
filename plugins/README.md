# Plugins

This directory is for extensions that ship with the editor, and holds the
guest library a WebAssembly plugin is built against, a sample guest and the cloth
painter, the first plugin ([The cloth painter](#the-cloth-painter)). This
file says how to build a guest, and describes the registries every extension
goes through, built in or loaded.

Nothing is loaded from here at run time: the app loads installed plugins from
the `plugins` folder of the configuration directory ([Installing a plugin](#installing-a-plugin)). The
in-app plugin host is `crates/domain/or-plugin`, whose contract
is [Plugin_ABI.md](../docs/reference/Plugin_ABI.md); the app embeds it in
`src/state/plugins.rs`, and what a plugin adds to the editor beyond running on
the open document (panels, commands, settings and viewport overlays,
[R45](../docs/direction/Roadmap.md#r45-the-plugin-sdk-and-registration-surface)) is in
`src/state/plugins/surface.rs`, described in
[Adding to the editor](#adding-to-the-editor). A plugin that extends a build, a validator
or an emitter, is run over each plan by `src/state/plugins/extensions.rs`
([Extending a build](#extending-a-build)).

## Writing a guest

`plugins/guest`, the crate `or-plugin-guest`, is the guest half of
[Plugin_ABI.md](../docs/reference/Plugin_ABI.md) at ABI version 1: each host function
as a call answering `Result<_, Status>`, which retries a short buffer once at
the length the host reported, and `Value`, the scalars, enums and runs of
scalars `field_value` answers and `patch_field` takes, and the file's bytes:
`file_size`, `read_file`, `file_bytes` and `write_file`, whose refusal carries
the host's sentence, and what a plugin that stays loaded adds to the editor:
`register_panel`, `register_command`, `declare_setting`, `setting_value`,
`set_overlay`, `overlay_picking` and the `ui` widgets
([Adding to the editor](#adding-to-the-editor)), and what a plugin that extends a build calls:
`plan`, `report` and `add_file` ([Extending a build](#extending-a-build)), and what version 1 added: `write_files`, `pack` and `report_with_fix` ([Version 1](#version-1)). `plugins/sample` is a guest
built from it that doubles every `f32` field of the document's root, and, built
with `--features bytes`, one that parses the open `.ydd` with `or-io` linked into
the module and writes it back with the first level-of-detail distance of every
drawable doubled, and, built with `--features surface`, one that registers a
panel, a command and a setting, draws an overlay and takes a pick, and, built with
`--features extend`, a validator and an emitter, and, built with `--features fixes`, a validator that reads the clothing packs of a build and suggests a fix with a finding, and, built with `--features writes`, one that writes two files of the project together.

They are a cargo workspace of their own, outside the editor's, built for
wasm32:

```text
rustup target add wasm32-unknown-unknown
cd plugins
cargo build --release -p or-plugin-sample --target wasm32-unknown-unknown
```

The module lands in `target/wasm32-unknown-unknown/release/` under
`plugins/`. A guest is a `cdylib` that depends on `or-plugin-guest` by path and
exports an `extern "C"` function taking and returning nothing, the entry the
embedder names; a panic traps and ends the run, never the host.

- ⚠ Only a function the guest calls is imported, and an import the grants do
  not pay for fails instantiation: a guest that calls `patch_field` does not
  run without *change the open document*, even when it would not patch.
- A guest that links `or-io` parses a file it reads with `file_bytes`, as
  `PgDictionary::<GtaDrawable>::from_bytes` does for a Legacy `.ydd`, and
  hands the file it saves to `write_file`; the sample builds that way with
  `cargo build --release -p or-plugin-sample --features bytes --target
  wasm32-unknown-unknown`, and `or-io` builds for wasm32 with
  `default-features = false, features = ["gta5"]`.
- ⚠ Parsing in a guest spends fuel before it reads a byte, and the app runs a
  plugin on the default limit, which an authored one-drawable `.ydd` already
  exceeds ([R44f](../docs/direction/Roadmap.md#r44f-payload-and-gpu-access) has the
  measurements).
- `crates/domain/or-plugin/tests/the_sample_guest_runs_under_the_host.rs`
  builds the sample, runs it through the host, and holds the library's
  statuses and encoding to the host's
  (`the_guest_library_encodes_what_the_host_encodes`), so a change to either
  side of the ABI fails there.
- `crates/domain/or-plugin/tests/a_guest_reads_and_replaces_a_files_bytes.rs`
  builds the sample with `bytes` and runs it on a `.ydd` through the host.
- `crates/domain/or-plugin/tests/what_a_plugin_adds_to_the_editor.rs` builds the
  sample with `surface` and holds each registration, widget, event, command,
  setting, overlay and pick to the host's session, and the grants to the
  functions they buy.

## Installing a plugin

A plugin is a directory under `plugins` in the editor's configuration
directory (`%APPDATA%/openrage/plugins` on Windows, `~/.config/openrage/plugins` on Linux,
`~/Library/Application Support/openrage/plugins` on macOS), holding the module as
`plugin.wasm` and its manifest as `plugin.json`:

```json
{
  "id": "sample",
  "name": "Sample",
  "abi": 0,
  "wants": ["cap.read_open_document", "cap.edit_open_document"]
}
```

A plugin that reads and replaces the file's bytes wants `cap.read_file_bytes`
and `cap.write_file_bytes` instead.

`wants` names capabilities by their stable key (`Capability::key`); a key this
build does not define, a field it does not read, another `abi`, no id or no
wants refuse the plugin, which stays listed in Settings → Plugins with the
reason. The app runs the export `go` on the open document's described root,
from Tools → Run plugin; a plugin that wants to
[add to the editor](#adding-to-the-editor) is loaded there instead.

- **Consent comes before the first run.** The prompt lists what the manifest
  declares, and the answer is kept in the preferences by plugin id. A grant
  covers the keys it was given, so a manifest that wants one more is asked
  again; Settings → Plugins revokes a grant or a refusal, and the next run
  asks again. A declined plugin does not run.
- **A grant is not the edit mode.** A patch, and a file a plugin writes, also
  pass the editor's edit mode for the document's path, and a shipped game file
  is refused rather than offered the confirmation a save gets. The format's own
  read-back and canonical save is the host's `SaveCheck`, for a written file at
  the document's own target, and a patch or a written file is in memory until
  the document is saved.
- **A refusal is shown, never swallowed.** Each run's report, in the toast and
  under the plugin in Settings → Plugins, carries the refusal's key and
  capability (`plugin.refused`, `cap.edit_open_document`) and the sentence the
  prompt used. A guest that stops after a refusal is reported with the refusal,
  and a failed run puts every byte back.
- `src/state/a_plugin_asks_before_it_runs.rs` holds this with the sample guest,
  both builds.

## Adding to the editor

A plugin whose manifest wants `cap.add_panels`, `cap.add_commands`,
`cap.add_settings` or `cap.draw_overlays` stays loaded, and exports `register`,
`draw_panel`, `run_command` and `picked` in place of `go`
([Plugin_ABI.md](../docs/reference/Plugin_ABI.md#a-guest-that-stays-loaded)):

```json
{
  "id": "sample",
  "name": "Sample",
  "abi": 0,
  "wants": ["cap.add_panels", "cap.add_commands", "cap.add_settings", "cap.draw_overlays"]
}
```

Each capability buys one kind of registration, and the guest registers what it
wants once, from `register`:

- **A panel** is in View → Panels and a saved layout as `plugin.<id>.<panel>`.
  The guest describes its widgets (`ui::label`, `button`, `slider`, `checkbox`,
  `text_field` and `list`) each frame it shows, and each call answers what the
  user did to that widget since the last one. egui never crosses the boundary.
- **A command** is in the command palette and Tools → Plugin commands, with the
  binding it asked for where nothing else holds it.
- **A setting** has a page of its own in Settings, under the plugin's name, and
  the preferences keep what the user chose; the guest reads it with
  `setting_value`.
- **An overlay** is points and lines in the open document's model space, drawn
  in front of the meshes by `set_overlay`. A guest that calls `overlay_picking`
  is sent the viewport's clicks as the geometry, triangle and vertex under the
  cursor, and they select nothing.

- **When it loads.** At startup for a plugin the user has already answered for,
  and from Tools → Run plugin otherwise, which asks first and loads it again if
  it is loaded. Settings → Plugins → Revoke takes it out of the editor, and Read
  again reloads what is installed.
- **What it can reach of the document.** The file's bytes, through
  `cap.read_file_bytes` and `cap.write_file_bytes`, for the call that is handed
  them, and nothing described: a write passes the edit mode and the format's own
  check as it does for a plugin run once.
- `plugins/sample` built with `--features surface` registers a command, a setting
  and two panels, whose buttons share an id because an event belongs to its panel, and
  `src/state/plugins/a_plugin_adds_to_the_editor.rs` holds each with it: a click on a
  panel's button in a real frame reaches the guest, a command runs it, a setting
  is written to the preferences and read back, and an overlay's primitives are the
  line vertices the viewport draws. `just self-check` draws the host's side of a
  guest panel, which has no wasm32 module to run there.

## Extending a build

A validator reports what is wrong with the plan of a build, and an emitter adds files to it. Both get the
plan in as the JSON `openrage-cli wardrobe plan` prints (without where on the disk each file is copied
from) and give data out, one finding or one file per
call ([Plugin_ABI.md](../docs/reference/Plugin_ABI.md#a-plugin-that-extends-a-build)):

```json
{
  "id": "catalogue",
  "name": "Catalogue",
  "abi": 0,
  "wants": ["cap.read_plan", "cap.report_findings", "cap.add_files"]
}
```

The guest exports `validate` (reads the plan with `plan()`, calls `report(code, level, subject, message)`)
and `emit` (calls `add_file(path, text)`), and has no `register`: nothing is added to the editor.

- **When it runs.** Over each plan the Project workspace's scan makes and over each plan a build
  writes, in the order of the plugins' ids, after the user has answered yes for what the manifest
  declares (Tools → Run plugin asks, Settings → Plugins revokes). It runs on the worker that makes
  the plan, never on a frame.
- **What the user sees.** An error is a problem of the build, so the build is refused; a warning is
  in the Problems list under the build; a note is kept and not listed. A plugin that stops or is
  refused is a warning of the build, and the plan is made without it.
- **What the plan does with a file.** A FiveM resource takes it, as a document the build writes, reads
  back and keeps with the rest; any other kind of output reports it as a problem naming the plugin.
- `plugins/sample` built with `--features extend` is both: it errors on a path with a space, warns
  on a file over a mebibyte, and adds `config/catalogue.json` listing the plan's items.
  `src/state/workbench/scan/a_plugin_extends_what_a_project_builds.rs` installs it beside the app and
  holds each effect: the finding in the Problems list, the file in the plan, the refusal of the build
  and the file in the resource it writes.
- The command line does not embed the host: a script reads the same plan from
  `openrage-cli wardrobe plan` ([Runtime loading](#runtime-loading)).

## Version 1

The ABI has a version number, `abi` in `plugin.json`, and an older plugin still loads: a plugin that
declares `"abi": 0` runs as it was built, the Plugins panel says it is built for an older interface, and
a plugin that declares a version newer than the editor knows is listed with the sentence that says to update
OpenRage ([Versions](../docs/reference/Plugin_ABI.md#versions-an-older-plugin-still-loads)). A plugin that
calls nothing newer than version 0 keeps declaring 0, as the cloth painter does; one that calls what version
1 added declares 1, and a manifest that declares 0 and wants one of the three capabilities below is refused
with a sentence that says to rebuild it.

| Capability | Calls | What it buys |
|---|---|---|
| `cap.write_files` | `write_files(&[(path, bytes)])` | Several files of the project written together, all or none, as one step of the project's History: Undo takes every file back and Redo puts them all back |
| `cap.read_pack` | `pack()` | The clothing packs the build takes, as the JSON document the ABI describes, for a validator to read |
| `cap.suggest_fixes` | `report_with_fix(code, level, subject, message, label, &[(path, bytes)])` | A finding with a fix: the files are written only if the person accepts it with Fix… in Problems |

A path is inside the project folder, with `/`; the editor refuses a game file (a project holds sources),
the project's own files and a `.json` that does not read, and the whole set with them. The sample's `fixes` and
`writes` features are a validator and an entry that use them, and
`src/state/plugins/a_plugin_writes_files_together.rs` and
`src/state/plugins/a_validator_suggests_a_fix_and_reads_the_pack.rs` install them beside the app and hold
each effect.

## The cloth painter

`plugins/cloth-painter` is [R46](../docs/direction/Roadmap.md#r46-first-plugin-a-clothing-tool-on-openrage)'s
per-vertex cloth tool, built as a plugin and needing no change inside the editor. Its manifest
(`plugin.json` beside the source) wants the file's bytes to read and replace, a panel, a command, two
settings and an overlay:

- **The panel** shows a `.yld` open in the editor: its cloths, the level to paint, the vertex count
  and the range of each stored value. Vertices are chosen by region (the fraction of the level's
  extent along each axis), by index (`0-20, 25`), all, inverted or none. A stroke sets the weight,
  the inflation scale, the pin radius or whether a vertex is pinnable on the selection, or paints
  the weight as a gradient along an axis; the numbers are typed in, since no range is known for
  them but what the staged cloths hold. The cloth's force and its negation are set the same way.
- **The overlay** is a mark at each vertex of the level, coloured by weight, with the selection
  apart. The settings say whether it is drawn and how large a mark is.
- **Writing** is the button or the command *Cloth painter: write the painted cloth to the open
  document*. Every stroke runs a trial export (the owned dictionary saved, which refuses what the
  game would misread) and says the result, and the host reads the written file back and saves it in
  canonical form before it keeps it; the document is then changed and unsaved, and Undo takes strokes
  back.
- It edits what a level stores. A run a cloth leaves empty is not created, since the engine's default
  for an absent run was not measured.
- ⚠ **The viewport cannot be clicked to choose a vertex.** A click is delivered to a plugin only for a
  drawable, and a `.yld`'s cloth is not one, so the painter selects in the panel. That is what R46's
  acceptance test found insufficient in the surface, and it is the one host change the painter would
  want.
- Build and install it as any plugin: `cargo build --release -p or-plugin-cloth-painter --target
  wasm32-unknown-unknown` in `plugins/`, then `plugin.wasm` and `plugin.json` into a folder of the
  plugins directory. `crates/domain/or-plugin/tests/the_cloth_painter_is_a_plugin.rs` holds the painter
  to the host, over a cloth built in the test and over staged cloths, and
  `src/state/plugins/the_cloth_painter_installs_as_a_plugin.rs` installs it in the app.

## The registries

Each is a set of builtins plus a process-global table behind a lock, with a
free `register` (and `register_at` for a tier) that adds to it after startup.

| Registry | Keyed by | Holds | A second registration of a key |
|---|---|---|---|
| `src/state/format_handlers.rs` | `FormatId`: resource type and target | `FormatHandler`, which extensions name a format | replaces within its tier |
| `src/state/payload_handlers.rs` | `FormatId` | `PayloadHandler`: parse, serialize, empty | replaces within its tier |
| `src/state/capabilities.rs` | `DocumentKind` | `FormatCapabilities`: what the editor can do with a document | replaces within its tier |
| `src/state/kinds.rs` | `DocumentKind` | `KindInfo`: noun, glyph, colour, workspace; optional | replaces within its tier |
| `crates/title/or-title/src/game/registry.rs` | title id | `&'static dyn GameProfile` | refused; builtins are asked first |
| `src/ui/panel_registry.rs` | panel id | `&'static dyn Panel` | refused |

The format family shares one `format_handlers::Tier`, looked up
preferred, then standard, then fallback: a handler is a choice, and a tier says
which wins without depending on load order. A title claims an install and a
panel an id, where two claimants would be ambiguous, so the first registration
wins and nothing displaces a builtin.

## Registering safely

⚠ A registration replaces the whole entry and never merges. A `FormatHandler`
registered at `Tier::Preferred` shadows the builtin's extension list too, and a
capability row built from `..FormatCapabilities::none(kind)` clears every cell
it does not set, including cells added to the table later. Start from the
current entry, change the cell you mean and register that
(`a_later_registration_replaces_an_earlier_one_in_the_same_tier`).

- **The live tables are process-global**, and tests in one binary share them
  in parallel. Assert against a table the test builds, `FormatRegistry::builtin()`
  or `CapabilityRegistry::builtin()`; only a test about runtime registration
  reads the live one. The first-wins registries cannot lose an entry, which is
  why `crates/title/or-title/tests/adding_a_title.rs` reads its live registry.
- **A test that registers a format handler** claims a resource type nothing
  else in the binary reads, which today is only `Ypm` (no named kind, no payload
  handler, no retail file), and keeps the builtin extensions it replaces:
  registering `Ydr` at `Tier::Preferred` with only a probe extension leaves
  `.ydr` unopenable for the rest of the process. `Yed` has its own kind, handler
  and panels, so registering over it collides with a builtin.
- **The `register` seams look dead and are not.** `capabilities::register`,
  `kinds::register` and `payload_handlers::register` are the plugin API and have
  no caller in the tree by definition; each carries an item-level
  `#[allow(dead_code)]` giving the reason. Do not delete one on a dead-code
  report. `panel_registry::register` has a caller, which registers the panels a
  plugin that stays loaded adds.

## Why they are shaped this way

- **Object-safe traits and plain `fn` pointers**, the shape a runtime-loaded
  implementation has to take, kept now because it costs nothing.
- **One explicit `register` call**, not `inventory` or `linkme`, which run
  before `main` in an unspecified order: title detection depends on order, and
  a registration may depend on configuration or on the install found.
- **Stable string ids**, because dock layouts are persisted and name panels by
  id. A saved id nothing provides renders a `MissingPanel` placeholder rather
  than being dropped.

## Adding a format, a panel or a title

In the repository, a panel is an `impl Panel` and a line in
`src/ui/panels/registry.rs`, and a title an `impl GameProfile` and a line in
`crates/title/or-title/src/game/registry.rs`
([Adding a title](../docs/reference/Crate_Reference.md#adding-a-title)). A paged format
is a `ResourceType` variant, for which `FormatRegistry::builtin` makes a handler
per target, plus its payload handler, capability row and kind row
([What a format costs the app](../docs/reference/Crate_Reference.md#what-a-format-costs-the-app)).

At run time a format is three registrations and no lines in `src/`: a
`FormatHandler`, a `PayloadHandler` and a `FormatCapabilities` row. A
`KindInfo` is optional: without one the format gets `KindInfo::provisional`,
its resource's own description with a neutral glyph. Its reader returns `DocPayload::Registered { format, data }`,
where `data` is an `Arc<dyn Any + Send + Sync>` the app never looks inside: the
generic panels read the document's own decompressed bytes, and `data` goes back
only to your `serialize` and your panels. `src/state/adding_a_format.rs`
declares such a format inside itself and walks detected, named, opens and
described (`a_synthetic_format_is_fully_usable`). What it gets is the generic
editor over its bytes, not one written for its semantics.

## Runtime loading

- **Data.** None: GTA V's description is built into `or-io` from its
  declarations, and no runtime `.orpack` is loaded, so a title gets the
  generic tree from its own declarations
  ([OD-OTHER-TITLES](../docs/direction/Project_Context.md#no-runtime-pack-for-another-title-2026-09-26)).
- **Titles.** A title is one method short of a data file: a detection marker
  distinct from the executable list, since Enhanced names `GTA5.exe` for key
  extraction and refuses it for detection
  (`one_shipped_title_needs_behaviour_and_the_rest_are_data`). It is not added,
  because nobody has asked for a title plugin and a second real title is a new
  container ([R56](../docs/direction/Roadmap.md#r56-rdr2-the-second-container)).
- **Behaviour.** Decided on 2026-09-21
  ([Project_Context.md](../docs/direction/Project_Context.md#a-plugin-is-wasm-in-the-app-and-a-subprocess-outside-it-2026-09-21)):
  inside the app a plugin is WebAssembly, untrusted by default and
  capability-controlled, through `crates/domain/or-plugin`; outside it, a
  subprocess speaking [the NDJSON protocol](../docs/reference/CLI_Server_Protocol.md). A
  native in-process ABI is a non-goal.
- **Unloading.** The title and panel registries hold `&'static dyn` values,
  which a loaded object satisfies only by being leaked: fine for a plugin
  loaded once at startup, and it rules out unloading one. A plugin's panel is
  one small leaked object per distinct panel id, and revoking the plugin's
  consent takes its guest, commands, settings page and overlay out of the editor
  while its panels stay registered and say it is not loaded.
