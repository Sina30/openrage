# or-weapons

The weapons engine: weapons a person streams into the game, and the metadata that defines them, as content of a project, with no UI.

`weapons.json` is the weapons as a person edits them. The crate reads and edits their metadata through the typed weapon family of `or-io`, says which parts a weapon wears and at which bone, says what is wrong as rules with codes, severities and waivers, and reads a build back to verify it. `openrage-cli weapons` runs it with no window.

⚠ Nothing here says a weapon loads in game: a build says every file it wrote reloads through the readers.

- Layer: domain; a kind of content of [or-project](../or-project).
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-weapons`.
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#domain-crates).
