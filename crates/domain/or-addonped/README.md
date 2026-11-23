# or-addonped

The addon ped engine: ped models a person streams into the game, and the data that defines them, as content of a project, with no UI.

`addonped.json` is the peds as a person edits them. The crate says where each file lands in a resource, what is wrong as rules with codes, severities and waivers, what the peds contribute to a build's plan, and reads a build back to verify it. `openrage-cli addonped` runs it with no window.

⚠ Nothing here says a ped loads in game: a build says every file it wrote reloads through the readers.

- Layer: domain; a kind of content of [or-project](../or-project).
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-addonped`.
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#domain-crates).
