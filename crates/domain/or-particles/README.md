# or-particles

The particles engine: the particle effects a person streams into the game, as content of a project, with no UI.

`effects.json` is the assets of a pack, each the source of a `.ypt`: its rules as one document, its textures as exact DDS files and its models kept whole. The crate says what is wrong as rules, what the effects contribute to a build's plan, and reads a build back to verify it. `openrage-cli particles` runs it with no window.

⚠ Not [or-particle](../or-particle), which is the editor's model of one open effects list. This crate is the kind of content a project holds.

- Layer: domain; a kind of content of [or-project](../or-project).
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-particles`.
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#domain-crates).
