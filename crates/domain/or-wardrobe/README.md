# or-wardrobe

The Wardrobe engine: a clothing pack for the freemode peds as a model, and everything a build of it needs, with no UI.

It reads garments from the names of their files, gives each its place among the drawables of its slot, says what is wrong as rules with codes, severities and waivers, mends a whole pack in one undoable change, writes the `.ymt` and shop metadata the places imply, and reads a build back to verify it. `openrage-cli wardrobe` runs it with no window.

⚠ Nothing here says a pack loads in game: a build says every file it wrote reloads through the readers.

- Layer: domain; a kind of content of [or-project](../or-project).
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-wardrobe`.
- For a clothing creator: [Workbench_Guide.md](../../../docs/guides/Workbench_Guide.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#domain-crates).
