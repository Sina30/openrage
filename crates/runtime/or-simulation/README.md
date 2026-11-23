# or-simulation

The particle simulation behind the `.ypt` preview.

It follows the engine's solver and behaviours, so an effect in the editor moves as its rules say, and it draws nothing: `or-renderer` draws what it simulates.

- Layer: runtime.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-simulation`.
- How the preview works and what it leaves out: [Particle_Preview.md](../../../docs/features/Particle_Preview.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#runtime-crates).
