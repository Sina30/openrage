# or-particle

The particle editor's model: the effect, emitter and particle rule hierarchy of an open `.ypt`.

It sits over the owned records `or-io` declares for a `.ypt` and shows them as the editor does: behaviours, domains, keyframes and evolutions. `or_particle::list` finds rules by name, and adds, renames and removes them.

⚠ Not [or-particles](../or-particles), which is particle effects as content of a project.

- Layer: domain.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-particle`.
- How the preview plays one: [Particle_Preview.md](../../../docs/features/Particle_Preview.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#domain-crates).
