# Particle shader contract

What the game's own particle passes read, and what the renderer gives them: the
vertex streams of a sprite, a trail and a model, the constants and textures
they bind, and what they are given nothing for. The passes are the SPIR-V
pack's (`ptfx_sprite`, `ptfx_trail`, `ptfx_model`), built by `game_shaders` from
what each module declares; nothing here is a table the renderer owns, and a
particle draws nothing without a pack ([Particle_Preview.md](../features/Particle_Preview.md#drawing)).
The measurements behind the layouts are in
[Shader_Pipeline_Design.md](../direction/Shader_Pipeline_Design.md#particle-sprites-trails-and-models).

## One pass for a batch

A batch names a technique of its effect, and its blend, cull and depth flags
make the pipeline's state. `EffectPass` builds it once for the technique, the
state and the target, binds each resource the modules declare, and keeps the
vertex, instance and index buffers between frames, growing them when a frame
needs more. A frame writes the constants that changed, the vertices and a draw.
A batch whose technique the pack lacks, or whose vertex stage reads what a
particle has none of, is refused by name and said once.

## Sprites

A sprite draws from two streams (`vertex_streams`). Per vertex it reads only
which corner it is, as two indices in `TEXCOORD3`. Per sprite it reads, from a
second buffer, its centre in `POSITION0`, its colour in `COLOR0`, the frame
blend and glow in `TEXCOORD0`, the corners as the centre plus or minus the
right and up directions through the xyz and w of `TEXCOORD6` to `TEXCOORD8`, the
table of corner positions in `TEXCOORD9`, and the cell of the atlas of each of
the two frames as `[u_min, u_max, v_min, v_max]` in `TEXCOORD10` and
`TEXCOORD11`, with `v_min` the top. `particle_sprites::basis` gives the
directions the width and height run along from the batch's alignment, and
`particle_sprites::frame_cell` the cell of a frame.

## Trails

A trail is one triangle strip read once per vertex: `POSITION0`, `COLOR0` and
`TEXCOORD0` as the texture coordinate, and for a lit technique `TEXCOORD1` as
the direction to the eye (`particle_trails::point_of`). A strip of fewer than
four vertices is not drawn.

## Models

A model is one indexed triangle list, read once per vertex: `POSITION0`,
`COLOR0`, `TEXCOORD0` (and `TEXCOORD1` where the unlit technique reads it twice),
`NORMAL0`, and a tangent that a model has none of and is given as the x axis.
Each instance of a batch is laid out in the world by the CPU
(`particle_meshes::lay_out`), so a batch is one draw. A model is not scaled by
the particle's dimensions but to them: the mesh is divided by its drawable's
bounding box (`bounds_box` of the lod group), so a particle of 0.1 metres is a
chunk 0.1 metres across whatever size the mesh was made at, and past the rule's
camera shrink distance both the size and the alpha grow by the distance over it
(`camera_shrink_factor`). The effect is the game's
ordinary mesh effect, whose texture is named `DiffuseSampler` where the
particle effects name it `DiffuseTexSampler`. A technique that reads bone
weights is refused.

## Constants and textures

- **Constants** are filled in four layers: what the effect declares as its
  default, then the batch's own values by the name of the variable, then the
  rule's shader variables, then the engine's (`EngineConstants`: the world, view
  and projection matrices and the sun and ambient light). A constant nothing sets
  holds the effect's default, and one with no default holds zero. A rule's
  vector or keyframe variable is named by the hash of its constant's name with
  the leading g left off (`SoftnessShadowMult` for `gSoftnessShadowMult`), or of
  the whole name where there is none (`Softness`), and every rule of a sprite or
  trail carries the same block of them, which the keyframes vary over the
  effect's progress (`rule_constants`, `EffectPass::set_rule_values`).
- **The diffuse texture** is the one the rule's first texture variable names,
  decoded once from the effect list's dictionary; a batch without one is given
  a white texel.
- **Everything else a module binds** is given a stand-in of the right kind, so
  a pass builds and draws. A stand-in for a depth texture holds the far value,
  so the shadow map a lit technique compares against finds nothing in the way
  (`far_depth_view`); one holding nought shadowed every lit sprite black.

## Known gaps

- Where the install holds no timecycle the light is the renderer's own choice of a clear day, as dark or as bright as `EngineConstants` says; with one it is the time of
  day's at the hour and the weather the drivers say, mapped to the screen by the game's filmic curve at the exposure the moment suits.
- A soft technique fades against the depth of the picture, which the ground gives it something to meet; the
  frame a refracting technique samples is a stand-in, and the viewport no
  longer copies the scene colour for it each frame.
- The decals an effect's rules describe, and the shadows of its particles,
  are not drawn: they were the renderer's own deferred passes, which went
  when Shaded took the game's shaders
  ([Viewport_Shading_Modes.md](../features/Viewport_Shading_Modes.md)). Its
  lights are drawn, as spheres or as cones, and its fog volumes as the game's fog
  volume pass draws them.
- Of the sprite techniques, the screen-space and thermal ones place a sprite in
  another space than the world and draw somewhere else, and the two that read
  the vertex number are refused.
- Most of the technique names a trail rule can carry are placeholders in the
  game's own shaders, which write one flat colour.
- A model is drawn on both faces because the winding of the game's meshes has
  not been measured.
