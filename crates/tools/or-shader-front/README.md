# or-shader-front

Turns a DXBC shader container into a SPIR-V module through the vendored dxbc-spirv translator.

One call translates one shader, through a thin C shim. The C++ is compiled by the crate's build script only with the `native` feature, so a build without it needs no C++ compiler and translates nothing.

- Layer: tools; `or-shader-tool` is its caller.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-shader-front`.
- The translator's licence: [LICENSE-third-party.md](../../../LICENSE-third-party.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#tool-crates).
