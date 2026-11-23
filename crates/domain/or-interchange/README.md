# or-interchange

A drawable as an external modelling tool sees it, and the interchange files that carry it.

`scene` is the model, with no RAGE concern and no glTF concern. `extract` fills it from a drawable, `gltf` writes and reads it, `fbx` reads one more format, `import` puts edited geometry back into a drawable that already exists, and `doctor` holds the mesh repairs. `openrage-cli mesh` runs it with no window.

⚠ Import is not a mirror of export: it has to satisfy invariants a reader tolerates, which is where the hazards are.

- Layer: domain.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-interchange`.
- What it does and refuses: [Mesh_Interchange.md](../../../docs/features/Mesh_Interchange.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#domain-crates).
