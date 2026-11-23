# Particle preview

How the Particle workspace plays a `.ypt` effect: a CPU simulation in
`or-simulation` that follows the engine's particle system, drawn by
`or-renderer` through the game's own baked shaders.

Only a Gen8 `.ypt` is simulated. `tick_sim` clears the preview unless the active
document is an owned Legacy particle list, so a Gen9 `.ypt` opens, inspects and
saves, and its effects play in neither the viewport nor the timeline. By the
owner's decision D9 the simulation follows the engine's solver and its
game-side behaviours at full fidelity, and is neither re-derived nor cut
([owner decisions](../direction/OR_IO_Specification.md#owner-decisions)); every entry under
[Known gaps](#known-gaps) is a divergence to close, not a design choice.

## Where the code is

| Path | Holds |
|---|---|
| `crates/runtime/or-simulation/src/manager.rs` | `PtfxManager`: the clock, the root effect and its spawned children, the snapshot cache, the world hooks and the output sinks |
| `crates/runtime/or-simulation/src/effect_inst.rs` | `EffectInst`: culling, LOD evolutions, loops, zoom, then the emitters |
| `crates/runtime/or-simulation/src/emitter_inst.rs` | `EmitterInst`: domains, the event window, the spawn loop, spawner triggers |
| `crates/runtime/or-simulation/src/behaviours/` | One file per behaviour; `crates/runtime/or-simulation/src/behaviour.rs` holds the trait, `BehaviourParams` and the `order` constants |
| `crates/runtime/or-simulation/src/kfp.rs` | Keyframe queries, evolution blending, bias links |
| `crates/runtime/or-simulation/src/point.rs` | `PointBuffer`, the structure-of-arrays particle store |
| `crates/runtime/or-simulation/src/random_table.rs` | `PtxRandomTable`, the per-particle random table; `crates/runtime/or-simulation/src/random.rs` is the LCG `RandomTable` the preview seeds its own draws from |
| `crates/foundation/or-transport/src/lib.rs` | `Transport`, the clock animation playback shares; `SimClock` is an alias of it |
| `src/state/sim.rs` | `SimSession`: the manager on `AppState`, rebuilds, pins |
| `src/state/drivers/` | Time of day, weather, exposure, world and LOD drivers that write evolutions and the environment |
| `src/render/particles/` | Sprite batches (`build_batches`) and the particle texture cache; trails and models are `src/render/trails.rs` and `src/render/particle_models.rs` |

## A frame

```text
App::render_editor                      src/app/editor_window.rs
└─ AppState::tick_previews              polls the file watcher, then per workspace
   └─ tick_sim                          Particle workspace only
      ├─ rebuild the EffectInst when the document, the selection or pending_rebuild changed
      ├─ drivers write evolutions, wind, the LOD override; collision BVH and sinks installed
      └─ PtfxManager::tick(real dt)
         ├─ a pending seek: restore the latest snapshot at or before it, replay
         ├─ clock.tick → sim dt, capped at max_dt
         ├─ EffectInst::update_with_hooks
         │   culling → LOD evolutions → loop and duration → zoom → emitters
         │   EmitterInst::update per emitter, on the CPU (parallel.rs: rayon from PARALLEL_THRESHOLD up)
         │     refresh domains → outside the event window: stop → spawn →
         │     per particle, behaviours in order (init_point when new, then update_point) →
         │     spawner triggers → update_finalize → liquid and decal-pool emits → sweep dead
         ├─ capture a snapshot every snapshot_interval
         └─ spawn queued child effects, re-attach them, tick them, drop finished ones
viewport panel → refresh_particles      builds sprite, trail and model batches
```

[Viewport_Shading_Modes.md](Viewport_Shading_Modes.md) has the passes that draw them.

## Simulation

**Order.** `build_dispatch` sorts a rule's behaviours by their `order` constant
with a stable sort, so a tie keeps the rule's own order: `ptxu_Gravity` and
`ptxu_Acceleration` share a slot. The event window gates the whole emitter
update, spawning and updating alike.

**Behaviours.** The ones whose behaviour is not obvious from the name:

| Behaviour | What the preview does |
|---|---|
| `ptxu_Wind` | Low and high LOD bands by camera distance (`wind_outside_low_lod_is_no_op`); samples the `WindField` the weather driver installs |
| `ptxu_Collision` | Ray casts against a host-set `BvhCollider` (Möller–Trumbore); without one, against a ground plane at z = 0. A hit sets `HAS_COLLIDED` |
| `ptxu_Noise` | A fresh signed random vector per frame, scaled by each keyframe's bias |
| `ptxu_ZCull` | Culls or clamps above or below a reference height: the effect's, the emitter's, or the water height at the camera or at the particle (the host's `WaterField`) |
| `ptxu_Dampening` | Air resistance from `WorldEnvironment::air_resistance` |
| `ptxu_River` | Moves particles with the host's `RiverField`; a no-op without one |
| `ptxu_Liquid`, `ptxu_DecalPool` | Aggregated into `liquid_batches` and `decal_pool_batches`, drawn as wire gizmos by `append_gizmo_lines` |
| `ptxu_Light` | Kept as the lights of the last update and drawn as spheres of the game's `point` pass, or as cones of its `spot` pass where the rule's light type is not nought ([Viewport_Shading_Modes.md](Viewport_Shading_Modes.md)) |
| `ptxu_FogVolume` | Kept as the volumes of the last update and drawn over the lit frame by the game's `vfx_fogvolume` effect ([Viewport_Shading_Modes.md](Viewport_Shading_Modes.md)) |
| `ptxu_Decal` | Not drawn: the decal splats the deferred path drew went with it, and the game's own pass for them is not wired |
| `ptxu_Fire` | Not a `Behaviour` variant; a stub dispatched by type hash reports collisions to the fire sink |
| `ptxu_DecalTrail` | A no-op stub, as the engine's is empty |
| Any other unknown | Dropped from dispatch, with one warning per type hash per process |

`ptxu_FadeIn`, `ptxu_FadeOut` and `ptxu_Gravity` are editor behaviours with no
engine counterpart. A rule saved with a fade behaviour is not a shipping rule;
a gravity entry round-trips only where the source file already carried one,
because the engine applies gravity inside `ptxu_Acceleration`.

**Domains.** Box, sphere and cylinder sample their volume, hollowed by the
inner size; an attractor domain collapses to its centre and pulls during
`ptxu_Attractor`. Each domain's position, rotation and outer and inner size
keyframes are re-evaluated every frame at the emitter's life ratio, and its
world-space, creation-relative and target-relative flags choose the parent
matrix.

**Evolutions.** `query_kpd_with_evo` blends each evolved keyframe into the base
sample with the property's blend mode (`AVERAGE`, `ADD`, `MAX`,
`FULL_AVERAGE`). A slot starts at its `override_value` only when
`is_overridden`. LOD evolutions (keyframes tagged `is_lod_evolution`, or a slot
named `lod` or `distance`) are rewritten every frame from the camera distance
between `lod_evo_dist_min` and its maximum, or from
`WorldEnvironment::lod_distance_override` when that is set, and an overridden
slot is left alone; a slot named `hq` follows `high_quality_evo` when that is
above zero. Each particle captures the evolution values at spawn
(`set_evo_snapshot`) and its behaviours query with those, so moving a slider
changes new particles and leaves live ones as they were. The drivers write the
slots their names match (`write_named_evolutions`) and skip overridden ones. An emitter whose
event carries its own list (`emitter_evolution_lists`) keeps its own values,
seeded from its overrides; the Inspector's sliders and the drivers write the
effect's. A bias link overwrites the random index of every keyframe property it
lists, so linked curves draw the same random sample.

**Spawning.** The spawn count is the rate times the frame time plus the
distance travelled times the rate over distance, both scaled by the part of the
frame inside the event window; the fraction carries to the next frame. An
emitter that jumps more than 25 units in a frame while it has a rate over
distance spawns nothing that frame and resets its accumulator, so a teleport
does not fill the gap with particles. The N particles a frame spawns are spread
along the emitter's path at `(i + 1) / (N + 1)`: by re-evaluating the domains at
the interpolated effect matrix when several spawn and the matrix moved, and
otherwise by offsetting between the previous and current emitter positions.
One-shot emitters and screen-space rules skip the spread.

**Effect scalars.** An effect samples its playback rate once from
`playback_rate_scalar_min` and its maximum and scales every behaviour's time
step by it. `EmitterRules::from_event` samples an event's playback-rate, zoom and
colour-tint ranges once per emitter. `zoom_scalar_kfp` is re-sampled every
frame. The effect colour tint (`colour_tint_min_kfp`, or a random point towards
the maximum when `colour_tint_max_enable` is set) multiplies in the Colour
behaviour, or in the sprite batch builder for a rule without one.

**Loops, duration and culling.** `num_loops` of -1 loops forever, 0 plays once
and N finishes after N loops; the manager drops finished spawned children. Each
loop re-rolls the duration between its minimum and maximum and adds a fresh
`rand_offset_pos_*` jitter to the effect matrix; `set_effect` applies one too,
then pre-warms the effect by `pre_update_time` in steps of
`pre_update_time_interval`. Past `distance_culling_cull_dist`, or
`viewport_culling_radius` measured as a camera distance, an effect stops
updating unless its `update_when_*_culled` flag says otherwise, and its alpha
fades out over the last quarter of `distance_culling_cull_dist`.

**Effect spawners.** `effect_spawner_at_ratio` queues a child effect at a
particle's position when its life ratio crosses the trigger;
`effect_spawner_on_coln` does so on a collision whose speed reaches the trigger,
oriented by the hit normal or, with `tracks_point_dir` or `tracks_point_neg_dir`,
by the particle's direction. Every spawn walk clears `HAS_COLLIDED`, so the next
hit fires again. `MAX_SPAWNED_EFFECTS` caps the children, so a self-spawning
effect cannot recurse without bound. A child samples its spawner's scalars once
from the triggering particle's random index and applies them with
`apply_spawned_scalars`; its colour tint folds into the effect tint
(`effect_colour_tint_override`), not the event tint. `inherits_point_life`
shortens the child to the parent particle's remaining life, and
`tracks_point_pos` re-finds the parent every frame by its random index across
the root and every spawned effect, so chains of spawners stay attached.

**Seeking.** The manager snapshots every emitter's particles and spawn state,
and every live spawned child, each `snapshot_interval`, up to a cap
(`the_snapshot_cache_is_capped_at_80`). A seek restores the latest snapshot at
or before the target and replays the rest in `max_dt` steps, or replays from
the start when none is early enough (`snapshot_cache_speeds_up_seek`).
An edit clears the cache, so a stale state never replays into an edited
effect.

## Drawing

⚠ **The particle draws need the pack of the game's shaders.** Sprite, trail and
model pipelines are built only from the game's own passes in the SPIR-V pack
the app bakes once a game is set. With no pack, or a technique the pack lacks,
the batch is skipped and nothing else draws the effect.

**Sprites** draw through the SPIR-V pack (`src/render/viewport/game_shaders.rs`
hands it to the renderer): `crates/runtime/or-renderer/src/particle_sprites.rs`
lays each batch out as sprites and draws it with the technique of `ptfx_sprite`
the rule names (`shader_technique`, the rule's `shader_template_technique_name`),
by `game_shaders::sprite` ([Shader_Pipeline_Design.md](../direction/Shader_Pipeline_Design.md#particle-sprites-trails-and-models)).
A particle's four dimensions are its top, bottom, left and right, a sprite's
width and height their sums; its alignment gives the directions they run, its
spin turns them, and its atlas frame is a cell of the texture. The batch's blend,
cull and depth flags are the pipeline's. Where a
batch's technique is refused (the two `_proj` ones read the vertex number) it is
said once and not drawn.

**Trails** draw through the pack the same way (`crates/runtime/or-renderer/src/particle_trails.rs`,
`game_shaders::trail`): a batch's strip of vertices is read once per vertex as a position, a
colour and a texture coordinate, with the direction to the eye as the normal a lit technique
asks for, and is drawn as a triangle strip with the technique of `ptfx_trail` its rule names;
a strip of fewer than four vertices is not drawn. Most of the technique names a trail rule can
carry are placeholders in the game's own shaders, which write one flat colour in all four
passes, so a trail of one draws as that colour.

**Models** draw through the pack too (`crates/runtime/or-renderer/src/particle_meshes.rs`,
`game_shaders::model`): the mesh of a drawable is kept on the CPU, each instance of a batch is
scaled, turned and moved into the world there (`lay_out`), and the batch is drawn as one indexed
mesh with a technique of `ptfx_model` (the rule names one of the sprite effect's, which the mesh effect lacks, so its lighting flag picks `lightweight0_draw` or `unlit_draw`), the diffuse texture of its geometry and
the instance's colour on every vertex. Models are drawn double-sided, since the winding of the
game's meshes has not been measured, and a skinned technique, which reads bone weights a
particle model has none of, is refused by name. What the passes read, and what they
are given for the rest, is [Particle_Shader_ABI.md](../reference/Particle_Shader_ABI.md).

What the batch builders decide on the CPU:

- **Skips.** An effect whose `draw_list_id` exceeds `MAX_DRAW_LIST`, an emitter
  with no particles, a sprite rule with `disable_draw`, and a rule whose draw
  type belongs to the trail or model builder.
- **Blend.** `ParticleBlendMode::from_engine_byte` maps the rule's blend set one
  to one and anything past the engine's range to `Alpha`; `Screen` is an editor
  mode no file selects (`every_blend_set_round_trips_through_renderer`). In
  Shaded mode `collapse_to_retail` folds every mode
  `retail_falls_back_to_normal` names to `Alpha`, and HeatHaze, NormalSpec and
  Screen templates to Refract, Lit and Default, matching the retail game.
- **Template.** `ParticleShader` is chosen from the technique flags in this order:
  a projection mode → Proj, normal-spec and lit → NormalSpec, refract →
  Refract or HeatHaze by name, lit → Lit, screen-space → Screen, else the
  template name, with a warning once for a name nothing maps. Only the refract
  bucket is read from it now: Refract and HeatHaze are drawn after the rest
  (the batch's `refract` flag).
- **Textures.** Each texture shader variable goes to a slot by its name hash
  (diffuse, normal, dirt, projection; an unknown hash is
  the diffuse). Atlas rows and columns come from the install's
  `ptxclipregions.dat` and are inferred, with a warning, when it is not loaded.
  The diffuse decodes once through `decode_level` with the preview channel swizzle
  into an `Image` the pass binds; the decoding rules are
  [BCn_Encoding_And_Mipmaps.md](BCn_Encoding_And_Mipmaps.md).
- **Sort.** Instances sort by the rule's sort type unless the blend
  is commutative (Additive, AlphaAdd, Min, Max). There are no particle shadows:
  the cascade that `NO_SHADOWS` and a sprite's shadow intensity steered went
  with the deferred path.

## The editor around it

| Outline selection | Previewed |
|---|---|
| Effect | The whole effect |
| Emitter event | That event alone, its window widened to the whole effect |
| Particle under an event | Its owning event, so the particle keeps its context |
| Particle rule in the library | A synthetic 5 s effect with one emitter at the origin (`PreviewTarget::Particle`) |
| Anything else | Nothing |

- **Timeline.** `src/ui/panels/timeline.rs` runs the shared timeline widget on a
  copy of `state.sim.manager.clock` and writes it back, so there is one clock.
  An evaluating seek goes through `SimSession::seek` to the manager's pending
  seek; a plain scrub moves the time only. One track per emitter event shows its
  event window; the curves pane drags behaviour keyframe handles, and each drag
  is one step of the document's history. Rails above hold the drivers and the pins.
  A Gen9 `.ypt` has the same tracks and curves, scaled to the effect selected, else the longest it
  authors. Nothing simulates its clock, so Play, Rate, Loop and the Drivers rail are greyed and say so.
- **Pins.** Ctrl- or Cmd-clicking an effect in the Outline pins it. Each
  rebuild adds pinned effects to `spawned_effects` with an optional start
  offset; the pin rail unpins them.
- **Inspector.** Edits effect, event, emitter, particle, behaviour and domain
  fields of the open asset. Every edit records the rule it changed as a step of
  the document's history, which flags a rebuild and clears the snapshot cache. With an effect selected, one slider per
  evolution writes the running effect's values without a rebuild.
- **In a project.** While a project holds particles, the workspace shows the Effects
  panel (`src/ui/panels/particles/`) beside the Outline: every particle file of the
  pack with its effects, and a click opens the file the source cooks to and previews
  the effect. A save writes the rules into the source and not the cooked file
  (`or_source::particles::write_back`). The screen is described under
  [what the Particle screen builds](../direction/Workbench_UX_Design.md#what-the-particle-screen-builds).
- **Live reload.** `src/state/file_watcher.rs` watches every open `.ypt` with
  `notify`, debounced by `DEFAULT_DEBOUNCE`; `reload_ypt_from_disk` re-reads
  the file as an owned `PtxFxList` and swaps only the payload, so the
  selection, the camera and the transport survive.

## Known gaps

- Culling reads a viewport cull radius as a camera distance, with no frustum
  test; `viewport_culling_mode`, the viewport cull offset,
  `distance_culling_fade_dist`, `render_when_*_culled` and
  `sort_events_by_distance` are read from the file and unused, and
  `is_emit_culled` is computed and never consulted.
- The particles draw once, as the normal stage: the mirror, refraction and
  shadow bucket passes are not drawn.
- A trail's anchor point takes its colour and size from the head particle.
- `ptxu_AnimateTexture` counts its frames as `frame_max - frame_min + 1` even
  when the first frame is randomised, and `ptxu_Age` loops a looped particle
  whether or not the effect is playing; both differ from the engine.
- A Gen9 `.ypt` is watched like any other, but its reload goes through the Gen8
  reader, which refuses it, so the document keeps its old payload and the log
  says the reload failed.
- The app sets no collision BVH, no river field and no high-quality evolution
  value, so collision meets the ground plane, `ptxu_River` does nothing and an
  `hq` slot keeps its own value. `LodDistanceDriver` can override the LOD ramp,
  and no panel enables it.
- `use_data_volume` is read and not consumed.
- Rendering fidelity is bounded by the shader contract's own gaps: see
  [Particle_Shader_ABI.md](../reference/Particle_Shader_ABI.md#known-gaps).
