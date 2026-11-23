# or-anim

Skeletal animation for RAGE clips: forward kinematics, pose evaluation and CPU skinning.

It takes a decoded `.ycd` to a moving figure with no GPU, so it is tested anywhere, and `resolve` answers which skeleton a document offers.

⚠ Skinning is `world * inverse_bind`, in that order; the reverse explodes the mesh.

- Layer: runtime. It does not depend on `or-particle`: the previews share the clock in `or-transport`.
- Start at [src/lib.rs](src/lib.rs).
- Tests: `just test-crate or-anim`.
- How the preview plays a clip: [Animation_Preview.md](../../../docs/features/Animation_Preview.md).
- Reference: [Crate_Reference.md](../../../docs/reference/Crate_Reference.md#runtime-crates).
