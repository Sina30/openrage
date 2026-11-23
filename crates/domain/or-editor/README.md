# or-editor

View models: what a panel shows, without knowing what read the file.

`Summary` is a property grid for any described type, and `Outline` is the generic tree the Outline panel falls back to. Both are built from a layout's description, so a format `or-io` declares gets an inspector and an outline with no code of its own in the app.

- Layer: domain; it has no UI dependency, and the app draws what it returns.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-editor`.
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#domain-crates).
