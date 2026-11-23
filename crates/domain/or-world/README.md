# or-world

The world editor's model: what a `.ymap` places and what a `.ytyp` declares.

`or-io` reads both into a value tree that says nothing about entities, archetypes, interiors or extents. This crate is that layer, and it copies nothing: every type borrows the tree the reader produced, so no second model can disagree with the file, and an edit changes the tree itself.

⚠ Nothing here keys on a field's spelling, which depends on whether a schema snapshot is staged. Fields are read by the hash both spellings stand for.

- Layer: domain.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-world`.
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#domain-crates).
