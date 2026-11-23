# Workbench plan

The order in which OpenRage gets projects, sources, the clothing engine and the screens around them, and the briefing each piece of work starts from.

Status: the plan of 2026-10-07. It supersedes the plan tables of [Clothing_Tool_Design.md](Clothing_Tool_Design.md), [Project_Model_Design.md](Project_Model_Design.md) and [Source_Format_Design.md](Source_Format_Design.md), whose studies it keeps as the reasons. [Workbench_UX_Design.md](Workbench_UX_Design.md) says what is drawn. [R39](Roadmap.md#r39-textures), [R46](Roadmap.md#r46-first-plugin-a-clothing-tool-on-openrage) and [R48](Roadmap.md#r48-projects) stand until a wave that changes one rewrites it.

## What this plan is for

The owner's goal, stated on 2026-10-07: implement projects properly in OpenRage, with the advantages of the incumbents and none of their costs, from the general modder to the clothing creator who comes from Durty Cloth Tool. The product is a project system that any kind of content plugs into, a source format that is the same for every kind, a headless clothing engine, and screens that serve both people without two modes.

## Rulings in force

Decided by the owner in conversation on 2026-10-07. A wave builds these and does not reopen them.

| Ruling | Applies to |
|---|---|
| A project has no type. It holds content (a module in the documents) and declares builds (an output). Templates replace types. Links are by path, later | Waves C, D, K |
| A model is stored as glTF plus a JSON file and is imported from glTF, GLB or FBX | Wave I |
| A texture is stored as its exact DDS and compiled from DDS or PNG, TGA, TIFF, BMP or JPG, with rules in a sidecar, channel packing and re-encoding of only the changed blocks. There is no convert button | Waves A, G |
| A folder is `<name>.<kind>.src` with a word for the kind and no platform letter; a source is recognised by its folder suffix, its fixed file names and the header of `asset.json`; sidecars are plain JSON | Wave A and every later source |
| A saved source is recooked and its preview refreshed without a manual build | Waves A, E1 |
| The screen words are Content and Builds | Waves E1, E2, H |
| Ctrl+B builds; Ctrl+1 to Ctrl+9 switch workspace | Wave E1 |
| A launch reopens the last project by default, and the start page stays one action away | Wave E2 |
| One project per window; a second project opens in a second window for now | Wave E2 |
| Open folder offers to make the folder a project and moves nothing | Wave E2 |
| A project holds no compiled game file: a game file added by any door is decompiled at once into its platform-free source, and the build compiles it for the platform or platforms it states. A source comes into a project by being created (New, then filled) or imported (a game file, an `asset.json`, a source folder as it is, or a folder scanned recursively). The only thing a content may name that is not a source of the project is a reference: a file of this project, a file of another project (the user chooses to import it or only to reference it) or an id typed by hand, chosen in a custom picker; the rule is for every kind (owner, 2026-10-09) | Wave P33 and every later view |
| Answered on the decision sheet of 2026-10-20: the new kinds come in the order weapons, particles, maps, then vehicles, props and interiors, audio, animations, graphics packs and scripts, each starting with its data before its screen; a metadata source stays a `.metadata.src` folder with a text `.meta` row; Windows only; Cancel stops the build and keeps the last good one; a singleplayer DLC and an `.oiv` leave out an unchanged game source with an option to ship it; no pre-push hook; interface languages start now (the mechanism, then French and English); one window for each project; the plugin ABI gains a multi-file transaction, a validator that reads the pack and a version number; the excluded rows of the parity matrix are the owner's rulings; the task-screen table comes before a second task screen; a data source ships at the path its content's table states; build cells stay derived; the Weapons view covers metadata, models, textures, components and attachment points with effects, sounds and animations as references | Waves P39 to P44 |
| Particles get a source form and a content kind, not only a panel and a template (owner, 2026-10-20, overruling the recommendation of the views draft) | Wave P43 |
| The Map view includes placement of objects with a manipulator in OpenRage, not only as a plugin (owner, 2026-10-20, overruling the recommendation of the views draft) | Wave P44 |
| Answered on the second part of the decision sheet of 2026-10-21: the French vocabulary is validated as it stands; a typed entity position is refused beyond 100,000 m of the world's centre; the weather's clear state reads Clear sky; the particle source is to be split by rule like the game's particle tool keeps its files (the owner pointed at its file kinds), not kept as one JSON; the interior records, the nested map archive of a DLC and the translation of library sentences were not answered | Wave P45 |
| The workflow's agents run Sonnet 5.5 at the highest effort | The run |

## Defaults taken on the owner's go

The owner said go to the roadmap built from the three studies, without ruling on each question those studies asked. Each wave below takes the study's recommendation, and a single sentence from the owner reverses it.

| Question of the studies | Default taken | Waves it shapes |
|---|---|---|
| Typed ped metadata in or-io, as the world family was | Yes | B, F |
| The clothing tool is first-party, a headless crate with a workspace on top | Yes. R44f's text and R46's text are not edited by a wave until the owner approves; R46 narrows to cloth physics when wave L lands | F, H, L |
| The first target is a FiveM resource, then singleplayer | FiveM first | C, J |
| A stable `id` in every `asset.json` | Yes | A |
| Durty-compatible shop hashes | An option, off by default, built after wave H | H |

## Held back

Nothing in this plan builds these, because each needs a ruling that has not been given.

- Importing the CodeWalker XML of a drawable, which would reopen the ruling of 2026-09-22 on XML.
- Author mode for drawables, which reverses a documented non-goal in [Mesh_Interchange.md](../features/Mesh_Interchange.md).
- The shader and vertex-layout presets that ship, drawn from [Shader_Definition_Database.md](../features/Shader_Definition_Database.md).
- Who runs GTA V to verify a build in game. A build never reports that it loads in game.
- Tattoo and overlay editing, alt:V and RageMP output, and an in-app texture painter, which the clothing study excludes.

## The waves

A wave is one lane and, usually, one pull request. Each ends on a check that has failed once before it passes ([Documentation_Standard.md](../guides/Documentation_Standard.md#evidence)). The stages are the order the workflow launches them; a wave starts only when every wave it depends on has merged.

| Wave | Content | Depends on | Stage | Lane |
|---|---|---|---|---|
| A | Texture sources: `asset.json`, the textures folder, exact DDS, image inputs, mips, live recook | none | 1 | `wb-a-texture-sources` |
| B | Typed ped metadata in or-io | none | 1 | `wb-b-ped-metadata` |
| C | The project container: content, builds, the plan as data, a FiveM resource executor, a verifier, the CLI | none | 1 | `wb-c-project-container` |
| D | The build plan: contributions, merge policy, collisions, budgets, split, build diff | C | 2 | `wb-d-build-plan` |
| E1 | The Project workspace, its panels, menu, status bar and Build button | C | 2 | `wb-e1-project-workspace` |
| G | Texture rules: channel packing, changed-block re-encoding, BC7, the normal map convention | A | 2 | `wb-g-texture-rules` |
| I | Models as sources: glb and JSON extraction, glTF, GLB and FBX import | A | 2 | `wb-i-model-sources` |
| E2 | The front door: start page, New project, launch, recents, adoption, second window | E1 | 3 | `wb-e2-front-door` |
| F | The Wardrobe engine and its CLI verbs | B, C, D | 3 | `wb-f-wardrobe-engine` |
| J | Singleplayer DLC from a manifest, `.oiv` authoring, Enhanced output | C, D | 3 | `wb-j-targets` |
| H | The Wardrobe workspace and the Clothing pack template | E1, E2, F | 4 | `wb-h-wardrobe-workspace` |
| K | Links by path, adopting an existing resource, templates as shareable files | E2 | 4 | `wb-k-links-adoption` |
| L | Extension: validator and emitter kinds for plugins, the cloth painter, the addon ped | F, H | 5 | `wb-l-extension` |
| H3 | A shop's own metadata imported and resolved, a garment's cloth linked rather than declared, a file watcher for the Wardrobe | H, L | 6 | `wb-h3-shop-and-cloth` |
| H2 | The dressed-ped preview of the Wardrobe workspace, texture variants by dropping an image, named flags | H3 | 7 | `wb-h2-dressed-ped` |

Two waves are never run at the same time when both edit the menu bar, the dock or the project file, which is why E2 waits for E1 and why D and E1 only share the plan's data type.

Status on 2026-10-08. Merged: A (#621, #633), B (#632, #638), C (#620, #626), D (#643, #650), E1 (#651, #653), G (#649, #656), I (#652, #654), E2 (#658, #660), F (#661, #665), J (#657, #659), K (#672, #675), H (#678, #682) and L (#686, #691). H3 and H2 stay planned and now follow the front waves of [the second plan](#the-second-plan-from-the-audit-of-2026-10-08), because they edit files those waves edit.

## The second plan, from the audit of 2026-10-08

On 2026-10-08 the owner asked to push the project system to the maximum, to polish it, and to miss nothing said in the conversation. An audit read every message of the owner about projects, checked each requirement against the tree, drew the real screens with the screenshot recipe, compared the Wardrobe with the three competing tools, and had a critic look for what its synthesis lost. Its result: the engine is sound and the front door is not. Nothing in the app or the command line calls the extractors, so a game file cannot yet be imported and decompiled to disk; Open and Import are on no screen as two actions; a build does not cook a source; and the project and Wardrobe screens carry major defects.

The waves that close the gaps are P1 onwards. Each wave's brief, the files it edits, its checks and what it does not do are in `docs/audit/workbench-waves-2026-10-08.json`, beside the rulings the waves need, each with a recommendation, and the risks; a wave reads its own entry. The plan takes every recommended ruling as a default until the owner says otherwise, except the ones it holds back: CodeWalker XML import, author mode and shipped shader or vertex presets, a claim that a build loads in game, and a change to R44f or R46. The waves keep the rules above, and two waves that may run together never edit one file, which the file lists of the entries were checked for.

| Wave | Content | Depends on | Size | Needs a ruling |
|---|---|---|---|---|
| P1 | Front door: start page, New project, open and close, and shots that can see them | L | L | no |
| P2 | Project screens: the five major defects and the dialogs | L | M | no |
| P3 | Import engine: a game file becomes a source on disk | L | M | no |
| P4 | Open and Import in the app: one word, commands, report, Undo | P2, P3 | L | no |
| P5 | Drops and bulk import: folders, progress, cancel | P1, P4 | M | no |
| P6 | A build cooks sources | P3, L | L | no |
| P7 | Verification as data, plain build sentences and CLI verbs | P6 | M | no |
| P8 | Live reload: a recook refreshes every open preview | H3 | S | no |
| P9 | Source edit loop: replace geometry, check against the game, mount the cooked model | P4, P7, P8 | M | no |
| P10 | Fragment source kind and flat metadata sources | P9 | L | no |
| P11 | Texture rules, schemas and the recorded format comparison | P10 | M | no |
| P12 | Model JSON says what glTF cannot (edit mode) | P11, P9 | L | yes |
| P13 | Project panels: counts, quick look, filter, missing members, ids, narrow windows | P4, H3, P9 | M | no |
| P14 | Build tab, status bar, edit-mode banner, Jobs and the Project menu | P7, P13, P1 | M | no |
| P15 | Builds editor and links | P14, P6 | L | no |
| P16 | Project History, rollback and a Save that does not churn Git | P15, P1 | L | no |
| P17 | Sharing safely, and help for a Durty user | P16, P3, P1 | M | no |
| P18 | Wardrobe screen: the majors and the polish | H2, P13, P1 | L | no |
| P19 | Wardrobe edits are recoverable: redo, backups, rollback, lock, originals | P18, P16 | L | no |
| P20 | Wardrobe edit operations: replace, duplicate, reorder, variations, bulk | P19 | L | no |
| P21 | Wardrobe list ergonomics: select, keys, search, tags, thumbnails | P20 | M | no |
| P22 | Wardrobe import breadth: images, name resolution, duplicates, mass add | P21, P11 | M | no |
| P23 | Wardrobe rules and limits: texture, mesh, game limits, configuration | P22 | L | no |
| P33 | Only sources in a project: create or import, and references for what stays outside | P23 | XL | no |
| P34 | Wardrobe screen gaps: props worn on the ped, light, wind, shop fields, clear Open and Import | P28 | L | no |
| P35 | Texture and import fidelity: alpha rule, skin-tone and 16-bit rules, normal and specular generation, budgeting, material auto-fix | P33, P24 | L | no |
| P36 | Project polish: plain words, missing files in Problems, one next action, rebuild check, ids, plugins, onboarding | P33, P28 | L | no |
| P37 | Power features: query bar, saved queries, dry-run bulk edits, review mode, credits, pack package | P34 | L | no |
| P24 | Wardrobe Fix all safe and the texture optimiser | P23, P33 | L | no |
| P25 | Server outputs: clothes dump, singleplayer DLC and .oiv, build options | P23, P7, P15 | L | no |
| P26 | Reopen a built pack, exchange formats, headless parity | P25, P1, P22 | M | no |
| P27 | Wardrobe metadata depth: alternates, flags, audio, first person | P26, P24 | L | no |
| P28 | Wardrobe preview tools: LODs, wireframe, skeleton, animation, thumbnails | P27 | M | no |
| P29 | Capacity: cap default, automatic split, freeze, dummies, replace modes | P28 | L | yes |
| P30 | Mesh doctor: LOD generation, rig validation and repair, UV crop, mirror | P28, P12 | L | yes |
| P31 | Re-verification against the owner's words | P17, P24, P26, P27, P28, P29, P30, P34, P35, P36, P37 | M | no |
| P32 | Draft the weapon, particle, map and other project views | P31 | M | no |
| P38 | Preview parity and texture places: alternates, heels floor, first person, shadows, reserved texture places, sampler auto-fix | P31 | M | no |
| P39 | DLC and .oiv leave out unchanged game sources, and the plugin ABI grows a transaction | P31 | M | no |
| P40 | Task screens as a table: the Wardrobe moves onto it | P31 | L | no |
| P41 | Interface languages: the translation mechanism, French and English | P40, P39 | XL | no |
| P42 | Weapons: the typed weapon family, a Weapons content kind and its task screen | P40, P41 | XL | no |
| P43 | Particles: a source form, a Particles content kind and its task screen | P42 | XL | no |
| P44 | Maps: a Map content kind, its task screen and placement with a manipulator | P43 | XL | no |
| P45 | Particles source form by rule, like the particle tool keeps its files, and a bound on a typed position | P43, P44 | L | no |

What a user meets first comes first: P1 to P3 start together, H3 and P4 follow them, the Wardrobe line is serial because its waves share files, and the last two waves re-verify the whole against the owner's words and then draft the weapon, particle, map and other project views.

## Briefs

Every brief names what to read first, what to build, the checks that must fail before they pass, and what the wave does not do. A wave that finds its brief contradicted by the tree reports it and stops, as the working principles of the repository say.

### Wave A: texture sources

Read [Source_Format_Design.md](Source_Format_Design.md) in full, [BCn_Encoding_And_Mipmaps.md](../features/BCn_Encoding_And_Mipmaps.md) and the texture dictionary code in or-io.

Build, in a new domain crate (the working name is or-source; follow the placement rules of the repository):

- `asset.json` as the design states it, with a stable id, written canonically: keys sorted, two-space indent, LF.
- A textures source folder, `<name>.textures.src`, holding the exact DDS of every texture and a `textures.json` of per-texture rules with defaults.
- Extraction of a `.ytd` into that folder and compilation of the folder back, with the source choice in this order: the `source` named in `textures.json`, then an image of the same name beside the DDS, then the DDS.
- Image inputs through the `image` crate that the tree already uses, mips down to the 4 by 4 level, and a DDS that is copied exactly when nothing changed. The current DDS import decodes mip 0 and recompresses it, which loses data at every round trip; the exact path must not.
- Live recook as a function with no UI: given a changed file inside a source folder, recook that one asset into a shadow output, ignoring what OpenRage wrote itself, and report a failure as data while leaving the previous output in place. Use a watcher crate only if one is already in the tree or the dependency is justified in the pull request.

Checks, each seen failing first: extracting and compiling every corpus `.ytd` returns the same pixel bytes per mip; replacing one image changes only that texture in the compiled file; compiling an unchanged folder is byte-identical twice; `asset.json` round-trips canonically; the recook touches one asset and ignores its own writes.

Does not do: channel packing, changed-block re-encoding, BC7 (wave G), any panel (wave E1).

### Wave B: typed ped metadata

Read [OR_IO_Specification.md](OR_IO_Specification.md) (D15 and D16), [New_Format_Implementation_Guide.md](../guides/New_Format_Implementation_Guide.md) and the world family's typed roots in or-io's metadata, which are the precedent.

Build the typed root or roots of the ped variation family, named by the engine's names as D15 requires and cross-checked against the corpus and the testimony, with a writer that can grow an array. Re-measure why the ped component `.ydd` files in the corpus fail to open or lose skinning, and record the result in the pull request.

Checks: typed reads of the vanilla and any addon ped metadata in the corpus hold the canonical fixed point; a reference pack's metadata re-encodes equal in meaning. If no addon ped metadata is staged, the wave says so and builds on the vanilla files alone.

Does not do: the clothing model (wave F), anything in the app.

### Wave C: the project container

Read [Project_Model_Design.md](Project_Model_Design.md) in full, the project file code in `src/state/project.rs` and the project cases in `src/state/the_front_door.rs`.

Build, in a new domain crate (the working name is or-project), so that the app and the CLI share it:

- Content and builds declared in the `.openrage-proj` file as added keys, with a schema bump and a migration on the next save. A key this build does not know is still carried, and an older build still opens a newer file.
- A Files content kind that takes over what the project's documents are today: the project of today reads unchanged and migrates when saved.
- The plan as data: a pure function from the manifest and the files to the list of files, copies and documents that a build will write, with its limits.
- A FiveM resource executor over files only: write to a fresh folder, reload every written file through the readers, swap it in and keep the previous build. The output is deterministic.
- `openrage-cli` verbs to create a project, print the plan, build and verify, following [CLI_Server_Protocol.md](../reference/CLI_Server_Protocol.md) where a verb enters the protocol.

Checks: a project of loose files builds a resource that reads back the same; an older build carries the new keys and a newer file opens in it; two builds of one project are byte-identical; a build that fails midway leaves the previous output untouched.

Does not do: merge policy, collisions and budgets (wave D), any panel (wave E1), any kind of content other than Files.

### Wave D: the build plan

Read the plan and contribution sections of [Project_Model_Design.md](Project_Model_Design.md) and the code wave C merged.

Build the contract through which content contributes to a plan (files, documents, limits), the merge policy and collision rules (collection names unique across the project), budgets read from limits stored as data, the split of one build into several at content boundaries, and the diff since the last build as data: added, changed, moved and removed, by stable id, with an exportable redirect map.

Checks: two content items claiming one collection name are refused at planning; a split lands on content boundaries; the diff reports a moved id and a removed one; each seen failing first.

Does not do: any screen, any clothing rule.

### Wave E1: the Project workspace

Read [Workbench_UX_Design.md](Workbench_UX_Design.md) in full, the shell code it names, and the app notes in [Crate_Reference.md](../reference/Crate_Reference.md).

Build the Project workspace and its panels (the Content and Builds panel, the virtualised Assets table with its filter and quick look, Properties, Problems, the project's History, with the Jobs and Log panels that exist), the top-level Project menu, the status bar's project name and Build button with its state dot, the edit mode banner that follows the document, the pinned workspace buttons with More, Ctrl+B and Ctrl+1 to Ctrl+9, and the Installed mods title. Every new command is an action in the command table. Content and builds are read from the container of wave C; the Build tab shows the plan data. Saved-image live recook is wired to the Assets table's state.

Checks: the self-check draws every panel and workspace including the new ones, and every workspace's empty state offers an action; a project file with shown buttons round-trips; the menu items are all in the palette; `just shots` before and after screens of the Project workspace are in the pull request, with what they show.

Does not do: the start page and New project (wave E2), any Wardrobe screen. Never opens a window outside `just self-check` and `just shots`.

### Wave G: texture rules

Read the textures chapter of [Source_Format_Design.md](Source_Format_Design.md), the code wave A merged and [BCn_Encoding_And_Mipmaps.md](../features/BCn_Encoding_And_Mipmaps.md).

Build channel packing (`pack` entries naming the image of each channel), re-encoding of only the compressed blocks that changed so that the original mips survive outside an edit, and the BC7 encoder in the compile path. Measure the normal map convention on shipped normal textures in the corpus and record the result in the design document before a default is set.

Checks: a packed specular map equals the one built by hand from the same three images; a save with no change changes no byte; an edit changes only the blocks it covers.

### Wave I: models as sources

Read the models chapter of [Source_Format_Design.md](Source_Format_Design.md), [Mesh_Interchange.md](../features/Mesh_Interchange.md) and the code wave A merged.

Build edit mode only: a drawable or a drawable dictionary extracted to a glb and a `drawable.json`, compiled back by replacing the geometry of its base; import of glTF, GLB and FBX with a report of what was dropped; the reference skeleton embedded for ped components. If no Rust FBX reader with an acceptable licence exists, ship glTF and GLB first and report the options for FBX in the pull request.

Checks: extracting and compiling every corpus drawable returns the same meshes; a ped component keeps its skinning through the round trip.

Does not do: author mode, presets, CodeWalker XML.

### Wave E2: the front door

Read [Workbench_UX_Design.md](Workbench_UX_Design.md) (the flows and the start page) and the code wave E1 merged.

Build the start page's four buttons and its recent-project cards with their content badges and build state, the first-launch goal cards for the content that exists, the New project dialog with the templates that exist (Empty project, Files, and Adopt a folder, which is named Project from a folder on screen), reopening the last project at launch with a setting and a way to the start page, the recent-project menu in the status bar, the palette entries, the Not found handling with Locate and Remove, the Make this a project banner on Open folder, and Open in new window as a second process.

Checks: a launch with a remembered project reopens it on its workspace; a launch without one opens the remembered install as the cases in `src/state/the_front_door.rs` require; closing a project shows the start page; a missing project shows Not found; before and after screens in the pull request.

### Wave F: the Wardrobe engine

Read [Clothing_Tool_Design.md](Clothing_Tool_Design.md) in full and the code of waves B, C and D.

Build, in a new domain crate (the working name is or-wardrobe) that depends on or-io, or-title and or-interchange and never on a renderer or on egui: the model of garments, slots and variations; the resolver, which reads gender and slot from names and marks what it cannot read; the allocator, with positions pinned by stable id; the rules as data with codes, severities and waivers; the planner contributing to the project plan; the verifier; and `openrage-cli` verbs to validate, build, diff and import a pack, with a plan emitter.

Checks: the planner's output for a reference pack matches the reference's structure, written first and seen failing; a headless build over staged files; a renumbering is reported with a migration map.

Does not do: any screen (wave H), the plugin kinds (wave L), shop hashes.

### Wave J: targets

Read [R51](Roadmap.md#r51-package-authoring-uninstall-and-dlc-creation) and [Dlc_Pack_Creation.md](../features/Dlc_Pack_Creation.md). Build a singleplayer DLC output and `.oiv` authoring from a build's plan, and the Enhanced target of an output. A rebuilt output reopens through the same readers.

### Wave H: the Wardrobe workspace

Read [Workbench_UX_Design.md](Workbench_UX_Design.md) (the Wardrobe workspace) and the code of waves E1, E2 and F. Build the Wardrobe workspace, the Clothing pack template and the Clothes card, and then the Durty-compatible shop hash option. The screen is thin: every rule and every write is the engine's. Checks: the self-check draws the workspace; dropping files adds garments and Undo removes them; before and after screens in the pull request.

### Wave K: links, adoption and shareable templates

Read the links and templates parts of [Project_Model_Design.md](Project_Model_Design.md). Build links by path with one writer and a banner, opening an existing resource as content with what could not be placed listed, and templates as files that another project can use.

### Wave L: extension

Read [Plugin_ABI.md](../reference/Plugin_ABI.md) and the clothing study's extension principle: a plugin extends and never hosts. Build validator and emitter kinds as data in and data out, the cloth painter of R46 as the first plugin, and the addon ped as the second kind of content, extracting the shared substrate only then. The clothing tests pass unchanged after the extraction.

### Wave H2: the dressed-ped preview

Read [Workbench_UX_Design.md](Workbench_UX_Design.md) (the Wardrobe workspace), [Viewport_Shading_Modes.md](../features/Viewport_Shading_Modes.md), [Mesh_Interchange.md](../features/Mesh_Interchange.md), the viewport and gallery code in the app, the skeleton handling in or-anim, and the Wardrobe panels of wave H.

The engine decides what is worn and the scene only draws it. Build:

- In or-wardrobe, a pure query from a pack and a selection to the drawable and the texture each slot selects, with the ped's base outfit for the slots nothing selects, so that the answer is tested without a renderer.
- The dressed ped in the centre of the Wardrobe workspace: orbit and zoom, the selected garment or the chosen outfit worn on the ped of its gender, drawn through the existing viewport and skeleton. The base ped's files come from the mounted install; with no install the preview says so and offers to choose one.
- Outfit (several garments at once), Tint (a palette choice), Skin, Heels (the ped raised by the garment's heel height) and A/B (the original against the edit).
- Replacing a texture by dropping an image on its variant, or by saving one beside the file, through the recook of or-source, with the preview refreshed. There is no convert button.
- Named flags in the Garment panel (hide hair, hide ears, heels with a height) as checkboxes, written by the engine.

Checks, each seen failing first: for a reference pack the query selects the drawables and textures its slots state; wearing a garment changes exactly its slot's drawable; the heel flag raises the ped by its stated height; A/B shows the original and the edit; the self-check with the viewport draws the scene; before and after screens in the pull request.

Does not do: animation playback, cloth simulation, tattoos and overlays, catalogue thumbnails, anything a held-back item names.

### Wave H3: a shop's own metadata, linked cloth, a file watcher

Read [Clothing_Tool_Design.md](Clothing_Tool_Design.md) (the pack model and the compatibility question), the or-wardrobe import, model, metadata and rules code, wave K's adoption code and the Wardrobe panels of wave H. It records the owner's ruling of 2026-10-08: servers that left Durty Cloth Tool, or never used it, have their own shop metadata and their own entry hashes, so compatibility cannot be Durty's alone; and a garment does not declare that it has cloth, it is linked to one.

Build:

- **Shop metadata import.** Adding a file, several files or a folder searched recursively to a Clothing content recognises each shop apparel metadata file (one a resource manifest registers as shop apparel data, or any file whose root is the shop apparel element) and parses it. Each entry is resolved to a garment and a texture by the ped, the component or prop type, the local drawable index and the texture index it states, against the pack's variation metadata. The hash an entry states, a name the game hashes or a literal value, is kept exactly as written and never assumed to fit 32 bits; it becomes the default shop id of that garment's texture, kept in the pack file with where it came from, together with the cost and any other field the entry states. An entry that points at no garment, a garment with no entry, and two entries claiming one hash are listed and never dropped or invented. A build writes the stated ids back unchanged; for a garment with none it writes OpenRage's names, or Durty's when the pack asks. Other files the manifest registers with the shop, such as a server's own extension, are carried unchanged and listed.
- **Cloth as a linked child.** A cloth file belongs to one drawable, because the game finds it by the model's name, so it is stored as a child of its garment. A garment has cloth when one is linked, and the flag that says so is written from that: neither the model nor the Garment panel carries an editable ownsCloth. Importing a pack whose metadata states the flag with no cloth file lists the garment as missing its cloth, with a way to link one; a cloth file whose flag is off is linked and the flag is written on, after the wave has checked on a real pack what the game does with a cloth shipped that way. Link and unlink are edits with Undo. "Cloth missing" (an Error a reason can waive) replaces wave H's cloth-not-shipped rule, and "cloth ignored" names the opposite case.
- **A file watcher** for the Wardrobe: a change to a pack's files rescans it, in place of the fixed three-second cadence, which stays as the fallback when the watcher cannot start.
- The Garment panel gets a Cloth section (the linked file, Link and Unlink) and a Shop section (the id of each texture, where it came from, an override).

A real pack is the check: the wave runs the reference-pack suite against the owner's server pack, which holds shop metadata with its own hashes, and reports the result.

Checks, each seen failing first: a folder holding shop metadata resolves every entry the pack's variations allow and lists the rest; a build writes the imported ids back unchanged and a second import of the output gives the same ids; linking a cloth turns the flag on and unlinking turns it off; a flagged garment with no cloth is listed; an external edit to a pack is seen by the watcher; the self-check draws the new sections with names a screen reader can speak.

Does not do: the dressed ped (wave H2), a bulk editor of hashes, anything on a server.

## What every wave does

The repository's own rules apply and every agent already loads them. These are the ones a long autonomous run breaks first.

- **Make the lane from the checkout** with `just agent-worktree NAME`, using the lane name of the table, and work only inside it. Create nothing outside the lane root, and touch no other lane or `.claude/worktrees` row.
- **Write the check first and see it fail.** A check that never failed is not evidence.
- **Never open the editor.** Use `just self-check` and `just shots`.
- **Read the owner's rulings above and the decisions in [Project_Context.md](Project_Context.md) before coding.** A contradiction is reported, not resolved in code.
- **Check before merging:** `just ready`, and `just ready-full` for a wave that changes how a format is read or written (A, B, G, I). Do not run the checks the pre-commit hook already ran. Never pass `--no-verify`.
- **Push once,** open the pull request and merge it with `gh pr merge N --repo openragee/openrage --squash --delete-branch` from outside the lane, so the remote branch is deleted and no local step fails. Rebase locally before the push, and land a conflict by a local squash onto `main` with `(#N)` at the end of the subject.
- **Docs travel with the code.** A new crate is added to [Crate_Reference.md](../reference/Crate_Reference.md); a wave that changes what [R39](Roadmap.md#r39-textures) or [R48](Roadmap.md#r48-projects) says rewrites that row; the documentation gates run under `just ready`. A wave does not edit the wave table of this plan: the orchestrator records status.
- **The shared stash stack is not yours.** Use a work-in-progress commit, or a uniquely tagged stash and `git stash apply`.
- **Report honestly.** The result of a wave says what was merged, which checks failed first and passed, what was skipped and why, the public names it added, and any question for the owner.

## Running it

The workflow runs a stage at a time. Each wave is one implementing agent, then one reviewing agent that reads the merged diff against the brief and the rules above and either accepts, asks for a fix, or rejects. A fix is one more agent in a new lane. Every agent is Sonnet 5.5 at the highest effort. A wave whose dependency did not merge does not start, and the run says which. When a stage ends, the orchestrator records the result here and in memory, reads the reports, and launches the next stage with the names the previous one actually produced.

Stage 1 is waves A, B and C, which touch separate code and can run together.

## Risks

| Risk | Why | Answer |
|---|---|---|
| Parallel lanes conflict | Two waves add a crate, which edits the workspace manifest, the lock file and the crate reference | Rebase before the push; a conflict lands by a local squash; waves that edit the menu, the dock or the project file are never parallel |
| A wave invents names | A hand-declared structure with guessed fields is the failure D15 forbids | Names come from the engine through the corpus and the testimony; an unproven field is reported |
| Autonomous work drifts from the design | Long runs with no owner in the loop | The reviewing agent, the checks that fail first, and the stage boundary where the orchestrator reads before continuing |
| The corpus is absent on some machines | A skipped suite has not passed | A wave states which suites ran and which skipped |
| No one can run GTA V | A written pack may still crash a client | The verification line stays unticked; see the held-back list |

## Sources

[Clothing_Tool_Design.md](Clothing_Tool_Design.md), [Project_Model_Design.md](Project_Model_Design.md), [Source_Format_Design.md](Source_Format_Design.md) and [Workbench_UX_Design.md](Workbench_UX_Design.md) for the reasons; the owner's messages of 2026-10-07 for the rulings; the tree at `a9b7cb96` for what exists.
