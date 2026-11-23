# YCD channel encoding

How OpenRage writes `crBlockStream` channel data for an edited `.ycd` animation: what it re-encodes, which formats it emits, the layout rules it has to reproduce, and what a re-encode loses.

The encoder is `crates/io/or-io/src/codec/anim/encode.rs`. The runtime and the edit layer that call it are [Animation_Preview.md](Animation_Preview.md); the on-disk layout and the decode side are [YCD_File_Structure_Specification.md](../extra/YCD_File_Structure_Specification.md), in the local-only layout tier.

## What is re-encoded

The channel encoding is not injective. The exporter chooses each channel's format, a `kQuantize` channel's width, scale and offset, a `kLinear` channel's Rice divisor and segment size, what a `kIndirect` table deduplicates, and whether a quaternion's fourth component is stored or reconstructed, so many byte sequences decode to the same floats. OpenRage does not try to reproduce a shipped exporter's bytes. It re-encodes an animation only when asked to, and holds the result to decoding back to the same floats.

- The unit is the animation. `reencode_animation` replaces every block of the animation it is given (`reencoding_an_animation_replaces_every_block`).
- Two callers reach it: `or_anim::bake_into`, run by `AppState::bake_anim_edits` when a clip with edits is saved, and `openrage-cli convert reencode`, which re-encodes the animation of every single-animation clip in a file and writes a multi-animation clip unchanged.
- Every other animation's blocks travel through a plain save as the payload bytes the owned value carries, and `saving_without_re_encoding_is_byte_identical` asserts that saving OpenRage's own output again gives the same bytes.

## What is emitted

`EncodePolicy` chooses per channel. `Lossless` is the default; `Quantize { bits }` clamps its width to 2..=31.

| Situation | Format emitted |
|---|---|
| every frame equal within `CONSTANT_EPSILON` (1e-6) | `kConstant`, `kConstantVec` or `kConstantQuat` |
| otherwise, under `Lossless` | `kRaw`, a full `f32` per frame |
| otherwise, under `Quantize { bits }` | `kQuantize` at that width |
| a quaternion whose `w` is recoverable | three channels and a `kReconstruct` post-pass |

`kIndirect` and `kLinear` decode and are never written. They carry the most exporter judgement (a shared value table with arbitrary deduplication, and a segmented Rice stream whose `numSegments + 1` initials are reconciled by an offset folded into every segment address), and a correct file needs neither. `kNormalize` is not written either: it renormalises a quaternion after lossy components, and the lossless default has nothing to correct. `BitWriter::push_rice` stays, as the primitive a `kLinear` encoder would need (`rice_round_trips_through_packed_sequence`).

## Layout rules

A block's payload has three sections:

```text
[0 .. constant_size)                          constant section, in section order
[constant_size .. + frame_size * num_frames)  variable section, one stride per frame
[constant_size + frame_size * num_frames ..)  channel directory
```

`ChannelWalk::walk` advances two accumulators across all nine format sections without resetting them: a word offset into the constant section and a bit address within the frame. The encoder writes in the same order, so the write order is the addressing.

- ⚠ The directory's position is derived, `constant_size + frame_size * num_frames` (`BlockSource::channel_directory_offset`), and nothing may sit between the variable section and it. A guard word there displaces the whole directory: the nine counts are read from the tail of the last frame and every channel header addresses garbage. None is needed, because `PackedArray::get` reads the second word only when a value straddles it. The header's `channel_offset` (the engine's `m_ChannelOffset`) is not the directory; it locates the root-motion reference table, counted from the start of the block including its 0x20 header (`every_animations_blocks_agree_with_its_frames`). The encoder writes the payload length plus 0x20, which is what a shipped block with no root motion states.
- Each directory index array is padded to `(count + 3) & !3` slots of `u16`, so the engine's SIMD loops can over-read (`directory_index_arrays_are_padded_to_four_entries`). An index packs `dof << 2 | component` (`a_directory_index_packs_the_dof_and_component`).
- A `kQuantize` width is read masked to five bits, so the encoder clamps to 31 (`quantize_bits_are_clamped_to_what_the_format_can_store`).
- A Rice value is its remainder in `divisor` bits LSB-first, then its quotient written as that number of zero bits and a terminating one, then a sign bit, except that a magnitude of zero has no sign bit. Emitting one for zero desynchronises every later value (`rice_encoding_of_zero_carries_no_sign_bit`).

## The kReconstruct singularity

`kReconstruct` computes `w = +sqrt(1 - x² - y² - z²)`, whose derivative `1/(2w)` is unbounded as `w` approaches zero, so any error in `x`, `y` or `z` is amplified near the half-turn. Measured on `dlc_mpAirRaces__bunk_int_p3_t01.ycd` (clip `pack:/mp_m_freemode_01.clip`, at `0fca1935`): a bone whose `w` reaches 1.2e-6 reconstructs to 2.2e-3 from losslessly stored `x`, `y` and `z`, because the decoder's sub-frame interpolation perturbs them.

So a quaternion is reconstructed only when `w` is at least `MIN_RECONSTRUCTIBLE_W`, 0.05 (about 174 degrees), at every frame, which also excludes a negative `w` since `kReconstruct` takes the positive root; and only when the reconstruction agrees with the stored `w` within 1e-4, which rejects a non-unit quaternion. Otherwise `w` is stored explicitly (`a_near_zero_w_quaternion_is_not_reconstructible`, `a_negative_w_quaternion_is_not_reconstructible`, `a_non_unit_quaternion_is_not_reconstructible`).

## What a re-encode loses

- **Block headers.** Each block's `hash` is dropped, and cannot be carried because it identifies the bytes rather than the animation (`a_block_hash_is_not_an_animation_identity`). `num_mover_channels` and the root-motion reference table are dropped too, so an edited clip loses its root motion; the reference layout is unconfirmed, and carrying refs that may name renumbered channels would substitute wrong motion for none (`what_a_reencoded_clip_drops`). `quantize_size` stays zero even under `Quantize` (`a_quantizing_policy_still_leaves_the_header_width_at_zero`).
- **Size.** `kRaw` spends 32 bits per component per frame where a shipped channel may be quantised to far fewer, so a re-encoded animation is larger. `convert reencode` prints the block bytes before and after; `--policy quantize:N` is the smaller option.
- **Compact blocks.** `crAnimCache` blocks cannot be decoded, so they cannot be re-encoded, and `reencode_animation` clears the compact flag on what it writes, because the flag would no longer be true. ⚠ `convert reencode` writes only the clips it rebuilt, so a clip whose animation is compact, or that has no animation, is missing from the file it writes.
- **Undecodable channels.** One `kLinear` channel in the retail corpus fails to decode its last frame. A bake writes such a DOF as the bind pose and `convert reencode` writes it as zeros; both report the count. The corpus sweep skips it.
- **Idempotence.** A second bake is not byte-identical to the first: it decodes at computed times that can land between frames and blend them, so the samples agree within 1e-4 and the bytes move (`re_baking_preserves_the_samples_even_though_the_bytes_move`). That is why `AppState::bake_anim_edits` takes the edit layer, and why a save of an unedited clip never bakes.

## Verification

- The unit tests in `crates/io/or-io/src/codec/anim/encode.rs` cover the format choice, the layout rules, Rice and the reconstruction limits.
- `crates/io/or-io/tests/ycd_encode.rs`: `kRaw` and the constant formats decode exactly and `kQuantize` within one step (`a_quantized_channel_round_trips_within_its_step` in the unit tests holds the same bound); the same bounds through an owned clip dictionary saved and opened again; a byte-identical resave, with `re_encoding_changes_the_bytes` so that check cannot pass vacuously; and corpus fidelity, `fixture_animations_reencode_to_the_same_samples`, bounded by default and exhaustive with `OPENRAGE_YCD_CORPUS=1`. A bounded run held 16 animations and 561 DOFs within 2.0e-4 (at `d4838113`).
- `crates/io/or-io/tests/what_a_reencoded_clip_drops.rs`: the header losses above, measured against the corpus.
- `crates/runtime/or-anim/tests/edit_roundtrip.rs`: edit, bake, save and reload.

⚠ The corpus checks skip without a staged corpus. A green run on a clean checkout has checked the unit tests and the synthetic round trips through a file, and nothing measured against retail clips.

## CLI

```bash
# Re-encode every channel and report the worst drift; an output path writes the result.
openrage-cli convert reencode in.ycd out.ycd --policy raw
openrage-cli convert reencode in.ycd --policy quantize:12
# The exhaustive corpus sweep.
OPENRAGE_YCD_CORPUS=1 cargo test -p or-io --test ycd_encode
```
