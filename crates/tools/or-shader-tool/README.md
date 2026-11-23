# or-shader-tool

Translates GTA V's compiled `.fxc` shaders into a pack of SPIR-V modules, which the viewport's Shaded mode draws with.

It is a library and a binary of its own. The editor runs the binary as a helper to bake a pack in the background once a game is set, and `bake-pack` does the same from a terminal. `inventory` counts what an install's shaders are.

The translator is C++, so the helper needs a C++ compiler:

```bash
just build-helper
```

- Layer: tools, beside the chain; only the app depends on it.
- Start at [src/main.rs](src/main.rs) and [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-shader-tool`.
- Design: [Shader_Pipeline_Design.md](../../../docs/direction/Shader_Pipeline_Design.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#tool-crates).
