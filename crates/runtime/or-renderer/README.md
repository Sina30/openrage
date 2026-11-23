# or-renderer

The wgpu viewport: meshes, lines, particles and the game's own shader passes.

`RenderContext` and `Renderer` draw what the editor's viewport shows, in the Wireframe, Solid, Material, Channel and Shaded modes. Shaded runs the passes of the game's shaders from a pack of SPIR-V modules that `or-shader-tool` bakes from an install.

It has no egui dependency: the app owns the integration, and nothing here opens a window.

- Layer: runtime.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-renderer`; a GPU test needs an adapter.
- What each mode draws: [Viewport_Shading_Modes.md](../../../docs/features/Viewport_Shading_Modes.md). The particle shader contract: [Particle_Shader_ABI.md](../../../docs/reference/Particle_Shader_ABI.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#runtime-crates).
