# or-io-macros

The `#[record]` attribute `or-io` declares a structure with.

One declaration makes the owned struct a user edits, its raw `repr(C)` layout, its description and its view, and proves the layout at compile time. Offsets come from field order, padding is a declared field or `#[pad_after(N)]`, and a field whose name says it is unknown is refused.

- Layer: io; a procedural macro crate that only `or-io` depends on.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-io-macros`.
- How a structure is declared: [OR_IO_Specification.md](../../../docs/direction/OR_IO_Specification.md#declaring-a-structure).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#io-crates).
