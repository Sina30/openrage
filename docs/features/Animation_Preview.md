# Animation preview

How the Animation workspace plays and edits `.ycd` clips: a clip is bound to a skeleton from another document, evaluated on the CPU, and drawn as a skinned mesh with a bone overlay in the viewport.

Writing edited channel data back is [YCD_Channel_Encoding.md](YCD_Channel_Encoding.md); the particle preview is [Particle_Preview.md](Particle_Preview.md); the on-disk format is [YCD_File_Structure_Specification.md](../extra/YCD_File_Structure_Specification.md), in the local-only layout tier.

## Where it lives

| Path | Holds |
|---|---|
| `crates/foundation/or-transport/src/lib.rs` | `Transport`, the playback clock type; `or-simulation`'s `SimClock` is an alias for it |
| `crates/runtime/or-anim/src/skeleton.rs` | `Skeleton` (the engine's `crSkeletonData`): bones in topological order, a tag-to-index map, bind and inverse-bind matrices |
| `crates/runtime/or-anim/src/pose.rs` | `Pose` (the engine's `crFrame`): local transforms, forward kinematics, skinning matrices |
| `crates/runtime/or-anim/src/binding.rs`, `crates/runtime/or-anim/src/resolve.rs` | `ClipBinding`, which resolves DOF bone tags once, and `BindingReport`; `bind`, `rank_skeletons` and `BoundClip` |
| `crates/runtime/or-anim/src/player.rs` | `ClipPlayer`: a clip evaluated at a time |
| `crates/runtime/or-anim/src/skin.rs` | `Weight` and `Binding` decode, CPU skinning, `SkinnedMesh` |
| `crates/runtime/or-anim/src/retarget.rs` | a geometry's matrix palette moved onto another skeleton by bone tag |
| `crates/runtime/or-anim/src/edit.rs` | `PoseEdits`, the edit layer, and `bake_into` |
| `crates/io/or-io/src/codec/anim/channel.rs`, `crates/io/or-io/src/codec/anim/encode.rs` | decoding the nine channel encodings, and encoding them per [YCD_Channel_Encoding.md](YCD_Channel_Encoding.md) |
| `src/state/preview_scene.rs` | `PreviewScene` (clip, skeleton source, mesh sources, display switches) and `AnimSession` (player, pose, matrices) |
| `src/state/animation_session.rs` | the per-frame tick, the skeleton choice, the rebuild, the bake, and pose edits as steps of the clip document's history |
| `src/render/skeleton_lines.rs` | the bone overlay, the highlight of the bone selected and the bind-pose ghost |
| `src/render/viewport/bones.rs` | which skeletons draw bones, posed or at rest, and their lines |
| `src/render/viewport/retarget.rs` | pairing mesh sources with the skeleton, and the palette retarget |
| `src/render/viewport/posing.rs` | skinning the held meshes again whenever the pose is a new one (`AnimSession::skin_revision`), not whenever the time moves |
| `src/ui/timeline/` | the timeline widget shared with the particle workspace, behind the `TimelineSource` trait |
| `src/ui/panels/timeline_ycd.rs` | `YcdTimelineSource` and key editing |
| `src/ui/panels/preview_settings.rs`, `src/ui/panels/ycd_clips.rs` | skeleton and mesh choice, clip fields and the edit summary; the clip table |

`or-anim` is GPU-free, so none of its suites needs a graphics device.

## Per-frame flow

```text
AppState::tick_previews                        when the Animation workspace is active
└─ tick_animation
   ├─ forget a clip, skeleton source or mesh source whose document closed
   ├─ resolve_skeleton_automatically           before the rebuild
   ├─ rebuild_anim_session if the scene or the skeleton's contents changed   or_anim::bind → ClipPlayer + BoundClip
   ├─ Transport::tick(real dt)
   └─ AnimSession::evaluate(time, apply root motion, edits)
      ├─ ClipPlayer::evaluate_into             local transforms
      ├─ PoseEdits::apply_to_pose              edits on top
      └─ Pose::pose_into                       world and skin matrices
ui::draw          RenderBridge::refresh_meshes / refresh_skinning, YcdTimelineSource, preview panel
RenderBridge::end_frame   skinned vertex buffers written in place; line pass for bones and ghost
```

## A clip is a pure function of time

Sampling a clip at `t` never depends on having sampled `t - dt`, whereas particle state is integrated and needs a snapshot cache to seek backwards. Scrubbing backwards therefore costs what playing forwards costs, and the only state carried between frames is each bone's quaternion hemisphere history, which affects sign only and is safe to reset. That is why animation is its own crate rather than part of `or-simulation`, and why the Animation workspace owns its own `Transport` (`AppState::anim_transport`) apart from the particle manager's clock.

## Binding a clip to a skeleton

A `.ycd` holds no skeleton. Its DOFs name bones by tag (`crAnimation::Dof::m_Id`, the same hash as `crBoneData::m_BoneId`), so playback needs a skeleton from a `.yft`, `.ydr` or `.ydd` and a tag-to-index resolution between them.

⚠ A wrong skeleton gives a T-pose or motion that is slightly off, which looks exactly like a decoder bug. `BindingReport` says which it is:

| Field | Meaning |
|---|---|
| `resolved` | clip tags the skeleton has |
| `unresolved_tags` | clip tags the skeleton lacks; a long list means the wrong rig |
| `untouched_bones` | skeleton bones the clip never drives, normal for a partial clip |
| `compact` | the blocks are `crAnimCache`-compressed and cannot be decoded |
| `has_root_motion` | the animation carries `MOVER_*` tracks |

`match_ratio` is resolved tags over clip tags. A binding with no resolved tag, or a ratio below 0.5, does not play and the preview says why, unless the user sets `allow_mismatched_skeleton`, which keeps "any dictionary on any rig" available.

The skeleton is chosen automatically. `resolve_skeleton_automatically` ranks the skeletons that open documents carry with `or_anim::rank_skeletons` (best tag coverage first, the smaller skeleton on a tie) and adopts the best, unless the user has pinned a choice. `skeleton_pinned` is what tells "nothing chosen yet" from "the user chose none". It runs before the rebuild, because a session built with no skeleton reports a binding failure for one frame on an asset that was never wrong. `rank_skeletons` identifies candidates by index into the slice it is given, because `or-anim` has no document model. A skeleton that is in the install but in no open document is not found.

Mesh sources and the skeleton source are chosen separately, and a geometry's matrix palette holds indices into the skeleton it was authored against. `or_anim::retarget` resolves each palette slot through that skeleton's bone tag to the bound skeleton's index, and the viewport applies it to every skinned upload whose drawable carries the skeleton its palette indexes. A mesh without one, as most clothing components are, keeps its file's indices, because they are the only statement there is. `crates/runtime/or-anim/tests/what_a_matrix_palette_addresses.rs` measures why an index read against another skeleton is in range and wrong.

The preview draws what its sources hold now, whichever document is active. The session is bound from the skeleton source's bones, so it is built again when that document's contents change (`skeleton_epoch`, its `payload_epoch`: once a grid edit is read back, not for each frame of a drag), as it is when a clip or skeleton is chosen, or when a key, a bake, an undo or a toggle asks for it. The transport belongs to the clip and skeleton source it was set from (`holds_the_transport_for`): a rebuild for them keeps the working range and Loop the user set and follows only the clip's length and frame rate, except that a change to the clip's own Looped flag (the Loop box in Preview settings, its undo or redo) sets Loop, and choosing another clip or skeleton source hands all of it back to the clip's own. The flag is compared with what the transport last took from it (`looped_changed`), so one edited while the clip is skipped as a mismatch wins when it is allowed again. The viewport's mesh key holds the skeleton the session was built from (`skeleton_built_from`), so meshes retargeted onto the old bones for a frame, as after an undo in the menu, are built again once the session is. Each mesh carries the mesh source it was built for (`MeshUpload::source`), and its texture is resolved from that source's own embedded textures (`texture_embedded_in`) before the open dictionaries', which are ranked against that source and not the active document: its stem, its folder, its archive and the dictionaries pinned for it. The badge's Pin, Back to Auto and its checkboxes act on the documents the preview draws (`textured_documents`), and the level-of-detail switch lists the levels any of them has models at. The viewport skins its meshes again when `AnimSession::skin_revision` moves, which another clip chosen or an edit made while the preview is paused also does.

⚠ With no mesh added the preview draws the skeleton source's own drawable, and a ped's `.yft` holds a 25 metre square plane as its only geometry, so it draws that (`a_peds_fragment_chosen_alone_draws_the_one_plane_its_own_bounds_state`). The box "Draw the skeleton file's own model" in the skeleton section of Preview settings hides it (`PreviewScene::draw_skeleton_model`, ticked by default): the skeleton, its bones and every mesh source are drawn as before, and a restart keeps the choice (`Prefs::draw_skeleton_model`). A mesh source added is drawn with the box either way, and with it unticked and no mesh source the viewport draws only the bones (`unticking_the_box_draws_no_mesh_but_the_skeleton_and_a_mesh_source_added_is_still_drawn`).

The bones are drawn from the skeleton source whether or not a clip drives it (`src/render/viewport/bones.rs`, which the lines, the click and the viewport's corner box all ask). Posed by the clip where one binds, they are at rest where none is chosen and where the clip does not fit the skeleton (the session is cleared for it, so it holds no pose), built from the bones of the skeleton source's own document. Show bones (`PreviewScene::show_bones`) starts ticked, and the Bones box in the viewport's row is the same flag here. Outside this workspace the viewport's own Bones box (`PreviewScene::show_rest_bones`, off by default) draws the skeleton of the drawable it draws at rest, the bone selected in the Outline highlighted and a click on a line selecting its row (`src/state/a_skeleton_is_seen_without_an_animation.rs`).

## Coverage

| Clip type or track | Status |
|---|---|
| `kClipTypeAnimation` (1) | played |
| `kClipTypeAnimations` (2), parallel | equal-weight blend (`parallel_clips_average_their_tracks`) |
| `kClipTypeAnimations` (2), sequential | the track covering the time (`sequential_clips_pick_the_track_covering_the_time`) |
| `kClipTypeAnimationExpression` (3) | refused on read: its `crExpressions` subtree is not modelled |
| `BONE_TRANSLATION`, `BONE_ROTATION`, `BONE_SCALE` | evaluated and editable |
| `MOVER_TRANSLATION`, `MOVER_ROTATION` | read into `Pose::root_motion`, applied when root motion is on |
| `BONE_CONSTRAINT` | read, not evaluated: it needs `crJointData` IK |
| `VISIBILITY` | read, not evaluated |

All nine channel encodings have a decoder; what still fails to decode is under [Known gaps](#known-gaps). Which ones are written, and why the others are not, is [YCD_Channel_Encoding.md](YCD_Channel_Encoding.md#what-is-emitted).

| Rendering | Status |
|---|---|
| CPU skinning | `Weight` and `Binding` decode through the geometry's `matrix_palette` |
| Several meshes on one skeleton | every mesh source is skinned to the bound skeleton |
| Bone overlay and bind-pose ghost | drawn in the line pass, together when both are on |
| GPU skinning | not implemented; `SkinnedMesh` is the seam, and a GPU path needs a variant of each pipeline family `MeshVertex` feeds plus its WGSL |
| Cloth during playback | not simulated; a `.yld` is drawn statically |

## Editing

Edits are a sparse additive layer, `or_anim::PoseEdits`, keyed by bone, track and frame over the decoded clip. The source animation is not modified until save, so the original stays recoverable and opening a clip and trying an edit cannot degrade it.

Saving a clip with edits runs `AppState::bake_anim_edits`, which bakes the layer into the animation through `or_anim::bake_into`, re-encodes it, clears the layer and clears the clip document's undo history; undoing a pose step after the bake would apply its edits twice. A bake that cannot run refuses the save with its reason: no bound skeleton, or a clip that plays several animations, which is edited one animation at a time. A DOF that does not decode is baked as the bind pose, and the save's message counts it.

A pose step in the clip document's history holds the whole layer the edit replaced (`Change::Pose`), a few hundred bytes for a sparse layer, so there is no inverse to derive: undoing swaps it back. Clip fields (name, start and end time, rate, loop flag) are edited directly, a step holding the fields they replaced (`Change::ClipFields`), and need no encoder.

## Engine rules the code keeps

- Frame conversion divides by `numFrames - 1`, the interval count (`frame_rate_uses_frames_minus_one`). Dividing by the frame count stretches every animation by a frame.
- A quaternion blend takes the destination on the source's hemisphere, as the engine's `PrepareSlerp` does, and the player keeps each bone's samples on one hemisphere from frame to frame. Without it a bone snaps through half a turn.
- Skinning is `world * inverse_bind`, in that order (`reversed_multiplication_order_would_be_wrong`). Reversed, the mesh explodes as soon as a bone leaves its bind orientation.

## Verification

- `crates/runtime/or-anim/tests/fk_golden.rs`: forward kinematics reproduces `crSkeletonData::m_InverseTransforms`, worst deviation 9.5e-6 over 62 retail skeletons (at `0fca1935`). `m_DefaultTransforms` holds each bone's local bind matrix, not its world one (`shipped_default_transforms_are_not_the_world_pose`): compared against composed world matrices it fails on 99 of a ped's 100 bones by exactly 2.0, the root being the one bone where local and world coincide (at `0fca1935`). The two checks separate a parent-walk regression from a per-bone composition one.
- `crates/runtime/or-anim/tests/skinning.rs`: weights sum to one, every influence names a real bone, and bind-pose skinning returns the authored vertices, over 316,575 vertices in 49 skinned drawables (at `d4838113`).
- `crates/runtime/or-anim/tests/what_a_matrix_palette_addresses.rs`: palettes hold indices, and the retarget moves them where an index would not.
- `crates/runtime/or-anim/tests/edit_roundtrip.rs`: edit, bake, save, reload and play, over a clip dictionary the test builds, so it needs no corpus.
- `src/state/editing_a_clip.rs`: undo, redo, clip fields, and the bake refused with no skeleton bound.
- `src/state/the_animation_preview_draws_what_its_sources_hold.rs`: a bone edit poses the skeleton again, a mesh source binds its own embedded textures while a clip dictionary is active, and what a ped's fragment draws alone.
- `src/state/the_animation_preview_follows_its_sources.rs`: another clip chosen while paused is posed, a mesh source's textures are ranked and pinned for it and its levels of detail listed while a clip dictionary is active, a closed mesh source leaves the list, and a Looped edit made while a clip is skipped wins when it is allowed again.

## Known gaps

- `crAnimCache` compact blocks cannot be decoded, so they cannot be played or edited. The clip table, the timeline's `compact` badge and the binding report say so; the player poses the bind pose and reports the failure (`a_compact_animation_reports_the_decode_failure_and_poses_bind`).
- One `kLinear` channel in the retail corpus fails to decode its last frame ("Rice quotient … has no terminating bit within 32 bits").
- Animation retargeting, moving motion between rigs by chain mapping or IK, does not exist; only the palette retarget above does.

## CLI

```bash
# Bind a clip to a skeleton, report what matched, and optionally dump a pose.
openrage-cli inspect anim move.ycd ped.yft --clip walk --at 0.5
# The structural report of every clip.
openrage-cli inspect clips move.ycd
```
