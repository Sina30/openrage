# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

The schema has no desktop value. OpenRage is a native desktop application
(egui and egui_dock on winit and wgpu), not a website: read `web` here as
"no iOS or Android design language applies".

## Users

The primary user is a content modder: someone making or changing GTA V assets
(clothing, vehicles, maps, textures, effects) for single-player mods or FiveM
servers, who today chains OpenIV, CodeWalker and other tools to do one job.
When a trade-off in the editor has to be decided, this user's job decides it.

Server studio teams and tool developers also use the CLI, the NDJSON server
and the `or-io` crate. They were not confirmed as audiences the editor is
designed for.

## Product Purpose

OpenRage is an asset editor for Rockstar's RAGE engine. It reads, edits and
writes the files GTA V Legacy and Enhanced ship: drawables, fragments,
textures, collision bounds, cloth, animation clips, particle effects, audio,
archives and metadata. Success is a modder doing the whole job, from opening a
shipped archive to saving a changed asset into a mods overlay or a DLC pack,
without leaving the app.

## Positioning

Four claims, all confirmed by the owner, that a neighbouring tool could not
truthfully copy together:

- **One tool for the whole job.** Every shipped format opens, edits and saves
  in one app, in place of a chain of separate tools.
- **Both editions natively.** Legacy and Enhanced are read and written, and a
  file's layout comes from the file itself, never from the install.
- **The game's own picture.** The viewport draws with the game's shaders, and
  previews particles, animation and time of day.
- **Scriptable and open.** The same work is available in `openrage-cli`, its
  NDJSON `serve` mode, WebAssembly plugins and a public Rust crate.

## Operating Context

- A desktop editor with a dockable layout: tabs and a rail, workspaces, an
  inspector, a GPU viewport, a particle simulator and an animation timeline.
- Windows is the primary platform and the only one a release is built for;
  Linux and macOS are supported.
- The user works on their own game install. Shipped archives are read in
  place, and edits go to the install's `mods` overlay, a project build, a DLC
  pack or an `.oiv`.
- Sessions are long and file-heavy: browsing archives, opening many documents
  in tabs, comparing a change against the original.
- `openrage-cli` does the same work in scripts and for other programs.

## Capabilities and Constraints

- The read and write format list is the table in [README.md](README.md);
  `.ysc`, `.fxc` and a few others are read-only.
- A save writes OpenRage's canonical form; byte-identity with a shipped file
  is not a goal.
- User-facing text says Legacy and Enhanced. Gen8 and Gen9 are code terms and
  never reach a screen.
- Every sentence a person reads is translated: English plus nine built-in
  languages (fr, de, es, it, pt, ru, zh, ja, ko).
- OpenRage downloads and installs nothing on its own, ships no game asset and
  takes its keys from the user's own executable.
- A write the app cannot make safely is refused by name, with the reason.
- Pre-1.0 and unpublished: nothing is released yet, and public surfaces
  (a website, a landing page, store listings) do not exist.
- Undecided: whether studio teams and tool developers are audiences the editor
  itself is designed for.

## Brand Commitments

- **Name and logo are settled.** The OpenRage name and the assets in
  `assets/brand` and `assets/logos` are identity, not open for redesign.
- **Plain factual voice.** Copy states what the app does and names what is
  untested or refused. No marketing tone, in the app or in the documents.
- **Every language is equal.** A screen must work in all ten languages, so a
  layout takes long strings and CJK text without breaking.
- OpenRage is unofficial and independent of Rockstar Games and Take-Two; no
  surface may suggest otherwise.

## Evidence on Hand

- The format table and feature claims in [README.md](README.md), each backed
  by tests.
- Screens of the real app, produced without a window by `just shots SCRIPT DIR`.
- The roadmap and dated owner decisions under `docs/direction`.
- Absent, and not to be invented: users, testimonials, download counts,
  benchmarks against other tools, pricing, a release date.

## Product Principles

1. The modder's whole job happens here; a step that sends them to another tool
   is a gap to close.
2. Say what is true: untested, refused and read-only are stated where the user
   meets them.
3. The file decides, not the install: edition and layout come from the bytes.
4. What the editor can do, the CLI can do.
5. Nothing happens to the user's install without them asking for it.
