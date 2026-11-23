//! What survives a `.ycd` re-encode, probed on both sides of the operation.
//!
//! [`reencode_animation`] is the save path for an edited clip (`or_anim::bake_into`,
//! the app's `AnimationSession` and `openrage-cli convert reencode` all reach it).
//! It replaces **every block of the animation**, not only the blocks whose samples
//! changed, so every `crBlockStream` field [`encode_block`] does not reproduce is
//! dropped for the whole animation. `ycd_roundtrip` checks a plain save, where the
//! header is carried verbatim, and `ycd_encode` the decoded samples; neither looks
//! at the header a re-encode writes.
//!
//! ⚠ **This suite compares content, not counts.** The block count and the payload
//! length agree across a re-encode by construction.
//!
//! Measured over the 60-file `.ycd` corpus at a cap of 400 animations / 1,335
//! blocks; the shipped cap is lower, and the suite prints its own denominators:
//!
//! | Field | Present before | Survives | Verdict |
//! |---|---:|---:|---|
//! | `hash` | 1,335 | 0 | **dropped** — see below |
//! | `num_mover_channels` | 1,124 | 0 | **dropped** |
//! | root-motion reference table | 1,124 | 0 | **dropped**, 34,548 bytes |
//! | `indirect_size` | 748 | 0 | correct — no `kIndirect` is emitted |
//! | `quantize_size` | 746 | 0 | correct under `Lossless`; see the note |
//! | `slop_size` | 0 | — | degenerate: zero in every block reached |
//!
//! A field whose correct new value is zero is not a drop, so the last three rows
//! are reported apart from the losses. `quantize_size` is zero under every
//! policy, quantizing ones included, because its units are unknown.
//!
//! **`hash` and the root-motion reference table are real losses, left open.**
//! `hash` covers the bytes rather than the animation
//! (`a_block_hash_is_not_an_animation_identity`), so carrying it forward is wrong.
//! `MoverChannelRef` is six bytes whose layout `channel.rs` records as
//! unconfirmed, and nothing in the workspace constructs one; carrying refs that
//! may name renumbered channels would substitute wrong root motion for absent
//! root motion. The encoder's directory index is `dof << 2 | component` and a
//! re-encode keeps the DOF list exactly, so if a ref's six bytes name a DOF and
//! component the table is carryable verbatim; if they name a section-relative
//! slot, it is not.
//!
//! `decode_dof` answering `Ok(None)` becomes `[0.0; 4]` in every caller here,
//! which for a rotation track is not the identity quaternion; it happens 0 times
//! in 1,687,069 samples.
//!
//! ⚠ `channel_offset` is the root-motion table's offset in block coordinates.
//! Written as the channel directory's start in payload coordinates, 0x20 short,
//! 1,335 of 1,335 re-encoded blocks claimed a root-motion table that was really
//! the tail of the last frame plus the directory; with no refs, a block states
//! "no root motion" the way the 7,040 real blocks that carry none do.
//!
//! Every tally is paired with controls, because a field is also absent when the
//! walk never reached a block: the DOF list and the samples must survive, and a
//! field zero in the file is reported as unable to falsify rather than counted as
//! preserved.

use std::path::PathBuf;

use or_io::codec::anim::{
    decode_dof, encode_block, format_type, reencode_animation, track, AnimationSource,
    DofSampleTable, EncodePolicy, BLOCK_HEADER_SIZE, COMPACT,
};
use or_io::decl::File;
use or_io::gta5::{CrAnimation, CrBlockStream, CrClipAny, CrClipDictionary};

fn ycd_fixtures() -> Vec<PathBuf> {
    or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "ycd").find()
}

/// How many animations go through a re-encode.
///
/// A re-encode decodes every DOF at every frame and writes them all back, so the
/// whole 17,775-animation corpus is ten minutes of work; the suite prints how many
/// it reached against how many exist. The table in the module doc was taken at 400.
const MAX_REENCODED: usize = 60;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BlockFacts {
    hash: u32,
    slop_size: u16,
    indirect_size: u16,
    quantize_size: u16,
    num_mover_channels: u8,
    mover_bytes: usize,
    channel_offset: u32,
    payload_len: usize,
}

impl BlockFacts {
    fn of(block: &CrBlockStream) -> Self {
        // The table runs from `channel_offset`, counted from the header, to the
        // end of the payload; zero bytes means the block states no root motion.
        let mover_bytes = (block.channel_offset as usize)
            .checked_sub(BLOCK_HEADER_SIZE)
            .and_then(|start| block.data.get(start..))
            .map_or(0, |s| s.len());
        Self {
            hash: block.hash,
            slop_size: block.slop_size,
            indirect_size: block.indirect_size,
            quantize_size: block.quantize_size,
            num_mover_channels: block.num_mover_channels,
            mover_bytes,
            channel_offset: block.channel_offset,
            payload_len: block.data.len(),
        }
    }

    fn offset_is_block_relative(&self) -> bool {
        self.channel_offset as usize >= BLOCK_HEADER_SIZE
    }

    fn says_no_root_motion(&self) -> bool {
        self.offset_is_block_relative()
            && self.channel_offset as usize - BLOCK_HEADER_SIZE == self.payload_len
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Tally {
    present: usize,
    survived: usize,
    degenerate: usize,
}

impl Tally {
    fn observe(&mut self, before: bool, after: bool) {
        if before {
            self.present += 1;
            if after {
                self.survived += 1;
            }
        } else {
            self.degenerate += 1;
        }
    }

    fn line(&self, what: &str) -> String {
        format!(
            "  {what:<32} {:>6} of {:>6} survive   ({} could not falsify)",
            self.survived, self.present, self.degenerate
        )
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct SilentSamples {
    total: usize,
    absent: usize,
    absent_rotation: usize,
}

fn try_decode_all(
    animation: &CrAnimation,
    silent: &mut SilentSamples,
) -> Vec<Option<Vec<[f32; 4]>>> {
    (0..animation.dofs.len())
        .map(|dof| {
            let is_rotation = animation.dofs[dof].track == track::BONE_ROTATION;
            let mut frames = Vec::with_capacity(usize::from(animation.num_frames));
            for frame in 0..animation.num_frames {
                let time = animation.internal_frame_to_time(frame as f32);
                match decode_dof(animation, dof, time) {
                    Ok(Some(s)) => {
                        silent.total += 1;
                        frames.push(s.components);
                    }
                    Ok(None) => {
                        silent.total += 1;
                        silent.absent += 1;
                        if is_rotation {
                            silent.absent_rotation += 1;
                        }
                        frames.push([0.0; 4]);
                    }
                    Err(_) => return None,
                }
            }
            Some(frames)
        })
        .collect()
}

#[derive(Default)]
struct Report {
    files: usize,
    animations_total: usize,
    animations_reencoded: usize,
    blocks: usize,

    hash: Tally,
    slop: Tally,
    indirect: Tally,
    quantize: Tally,
    mover_flag: Tally,
    mover_table: Tally,
    mover_table_bytes: usize,

    dof_lists_intact: usize,
    dof_lists_changed: usize,
    worst_drift: f32,
    dofs_compared: usize,

    offset_below_header: usize,
    offset_claims_a_table_it_lacks: usize,
    offset_well_formed_after: usize,
    offset_well_formed_before: usize,

    silent: SilentSamples,
}

fn measure() -> Report {
    let mut r = Report::default();

    for path in ycd_fixtures() {
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let Ok(dict) = CrClipDictionary::from_bytes(&data) else {
            continue;
        };
        r.files += 1;

        for animation in primary_animations(&dict) {
            if is_compact(animation) || animation.num_frames == 0 || animation.dofs.is_empty() {
                continue;
            }
            r.animations_total += 1;

            let before: Vec<BlockFacts> = animation
                .blocks
                .iter()
                .flatten()
                .map(|b| BlockFacts::of(b))
                .collect();
            for facts in &before {
                if facts.says_no_root_motion() {
                    r.offset_well_formed_before += 1;
                }
            }

            if r.animations_reencoded >= MAX_REENCODED {
                continue;
            }

            let samples = try_decode_all(animation, &mut r.silent);
            let mut copy = animation.clone();
            let sample_ref = samples.clone();
            if reencode_animation(&mut copy, EncodePolicy::Lossless, |dof, frame| {
                sample_ref
                    .get(dof)
                    .and_then(|f| f.as_ref())
                    .and_then(|f| f.get(frame))
                    .copied()
                    .unwrap_or([0.0; 4])
            })
            .is_err()
            {
                continue;
            }
            r.animations_reencoded += 1;

            if copy.dofs == animation.dofs {
                r.dof_lists_intact += 1;
            } else {
                r.dof_lists_changed += 1;
            }
            for (dof, want) in samples.iter().enumerate() {
                let Some(want) = want else { continue };
                let components = format_type::component_count(animation.dofs[dof].r#type).min(4);
                r.dofs_compared += 1;
                for (frame, expected) in want.iter().enumerate() {
                    let time = copy.internal_frame_to_time(frame as f32);
                    if let Ok(Some(got)) = decode_dof(&copy, dof, time) {
                        for (a, b) in expected.iter().zip(got.components.iter()).take(components) {
                            r.worst_drift = r.worst_drift.max((a - b).abs());
                        }
                    }
                }
            }

            let after: Vec<BlockFacts> = copy
                .blocks
                .iter()
                .flatten()
                .map(|b| BlockFacts::of(b))
                .collect();
            for (b, a) in before.iter().zip(after.iter()) {
                r.blocks += 1;
                r.hash.observe(b.hash != 0, a.hash != 0);
                r.slop.observe(b.slop_size != 0, a.slop_size != 0);
                r.indirect
                    .observe(b.indirect_size != 0, a.indirect_size != 0);
                r.quantize
                    .observe(b.quantize_size != 0, a.quantize_size != 0);
                r.mover_flag
                    .observe(b.num_mover_channels != 0, a.num_mover_channels != 0);
                r.mover_table.observe(
                    b.num_mover_channels != 0 && b.mover_bytes > 0,
                    a.num_mover_channels != 0 && a.mover_bytes > 0,
                );
                if b.num_mover_channels != 0 {
                    r.mover_table_bytes += b.mover_bytes;
                }

                if !a.offset_is_block_relative() {
                    r.offset_below_header += 1;
                } else if a.says_no_root_motion() {
                    r.offset_well_formed_after += 1;
                } else if a.num_mover_channels == 0 {
                    r.offset_claims_a_table_it_lacks += 1;
                }
            }
        }
    }

    r
}

/// Each clip's first animation.
fn primary_animations(dict: &CrClipDictionary) -> Vec<&CrAnimation> {
    let Some(anims) = dict.anim_dictionary.as_deref() else {
        return Vec::new();
    };
    dict.clips
        .entries()
        .filter_map(|entry| {
            let key = match entry.data.as_deref()? {
                CrClipAny::CrClipAnimation(c) => c.animation.key?,
                CrClipAny::CrClipAnimations(c) => c.animations.first()?.animation.key?,
                CrClipAny::Base(_) => return None,
            };
            anims
                .animations
                .entries()
                .find(|e| e.key == key)?
                .data
                .as_deref()
        })
        .collect()
}

fn is_compact(animation: &CrAnimation) -> bool {
    animation.flags & COMPACT != 0
        || animation
            .blocks
            .iter()
            .flatten()
            .any(|b| b.compact_size != 0)
}

#[test]
fn what_a_reencoded_clip_drops() {
    let r = measure();
    if r.files == 0 {
        or_test_support::skip_corpus(
            "what_a_reencoded_clip_drops",
            "no .ycd fixtures staged - skipping",
        );
        return;
    }

    eprintln!(
        "\n=== what a .ycd re-encode drops ===\n\
         {} file(s), {} animation(s) present, {} re-encoded (cap {}), {} block(s) compared\n",
        r.files, r.animations_total, r.animations_reencoded, MAX_REENCODED, r.blocks
    );
    eprintln!("controls - the operation is otherwise faithful:");
    eprintln!(
        "  DOF list identical             {:>6} of {:>6} animation(s)",
        r.dof_lists_intact,
        r.dof_lists_intact + r.dof_lists_changed
    );
    eprintln!(
        "  worst sample drift             {:.3e} over {} decodable DOF(s)",
        r.worst_drift, r.dofs_compared
    );
    eprintln!("\ndropped - carried by the file, absent from the re-encode:");
    eprintln!("{}", r.hash.line("hash"));
    eprintln!("{}", r.mover_flag.line("num_mover_channels"));
    eprintln!("{}", r.mover_table.line("root-motion reference table"));
    eprintln!(
        "  root-motion table bytes shipped {:>6}",
        r.mover_table_bytes
    );
    eprintln!("\nzero because of what was written, not because it was lost:");
    eprintln!("{}", r.indirect.line("indirect_size"));
    eprintln!("{}", r.quantize.line("quantize_size"));
    eprintln!("{}", r.slop.line("slop_size"));
    eprintln!("\nm_ChannelOffset, which is block-relative in every real file:");
    eprintln!(
        "  well-formed before / after     {:>6} / {:>6} block(s)",
        r.offset_well_formed_before, r.offset_well_formed_after
    );
    eprintln!(
        "  below the 0x20 header after    {:>6} block(s)",
        r.offset_below_header
    );
    eprintln!(
        "  claims a table it lacks after  {:>6} block(s)",
        r.offset_claims_a_table_it_lacks
    );
    eprintln!("\na DOF the block says nothing about, written back as saying zero:");
    eprintln!(
        "  Ok(None) samples               {:>6} of {:>6}   ({} on a rotation track)",
        r.silent.absent, r.silent.total, r.silent.absent_rotation
    );

    assert!(
        r.animations_reencoded > 0,
        "no animation was re-encoded, so nothing above measured anything"
    );
    assert_eq!(
        r.dof_lists_changed, 0,
        "the DOF list changed, so the re-encode is not otherwise faithful and the \
         header tallies say nothing"
    );
    assert!(
        r.worst_drift < 1e-3,
        "samples drifted by {:.3e}; the re-encode is not otherwise faithful",
        r.worst_drift
    );
    assert!(
        r.hash.present > 0 && r.mover_flag.present > 0,
        "no block reached carries an hash ({}) or mover channels ({}), so the \
         drops below are unfalsifiable on this corpus",
        r.hash.present,
        r.mover_flag.present
    );

    assert_eq!(
        r.offset_below_header, 0,
        "{} re-encoded block(s) wrote an channel_offset below the 0x20 header, \
         which is not a block-relative offset at all and makes \
         CrBlockStream::mover_section return None",
        r.offset_below_header
    );
    assert_eq!(
        r.offset_claims_a_table_it_lacks, 0,
        "{} re-encoded block(s) declare no mover channels and point channel_offset \
         somewhere other than the end of the payload, so a reader following the \
         documented rule finds a root-motion table that is really the tail of the \
         last frame",
        r.offset_claims_a_table_it_lacks
    );
    assert_eq!(
        r.offset_well_formed_after, r.blocks,
        "every re-encoded block emits no mover refs, so every one should state that \
         the way real blocks do"
    );

    assert_eq!(
        r.hash.survived, 0,
        "hash now survives a re-encode on {} of {} block(s). It could not before: \
         the hash covers a payload the re-encode has replaced, and the algorithm \
         that produces it is not known here. If this is now derived, delete this \
         assertion and say where the derivation came from.",
        r.hash.survived, r.hash.present
    );
    assert_eq!(
        r.silent.absent, 0,
        "{} of {} (dof, frame) samples decode to Ok(None) and are written back as \
         [0.0; 4] — {} of them on a rotation track, where zero is not the identity \
         quaternion. This was 0 of 1,687,069 when the substitution was measured, \
         which is what made it harmless. It is not harmless now.",
        r.silent.absent, r.silent.total, r.silent.absent_rotation
    );
    assert_eq!(
        r.mover_table.survived, 0,
        "the root-motion reference table now survives a re-encode on {} of {} \
         block(s). It could not before: MoverChannelRef is six bytes whose layout \
         channel.rs records as unconfirmed, so carrying refs that may name channel \
         indices the re-encode renumbered would substitute wrong root motion for \
         absent root motion. If the layout is confirmed now, delete this assertion \
         and cite the measurement.",
        r.mover_table.survived, r.mover_table.present
    );
}

/// `hash` varies per block, so it is not an animation identity a re-encode
/// could carry forward; `crAnimation` carries a `signature` for that.
///
/// Over the whole corpus: 48,411 blocks, none with a zero hash; 10,124
/// multi-block animations, none where every block agrees; none where any block
/// hash equals `signature`. Read-only, so the denominator is the corpus rather
/// than a cap.
#[test]
fn a_block_hash_is_not_an_animation_identity() {
    let fixtures = ycd_fixtures();
    if fixtures.is_empty() {
        or_test_support::skip_corpus(
            "a_block_hash_is_not_an_animation_identity",
            "no .ycd fixtures staged - skipping",
        );
        return;
    }

    let mut multi_block = 0usize;
    let mut all_blocks_agree = 0usize;
    let mut equals_signature = 0usize;
    let mut animations = 0usize;
    let mut blocks = 0usize;
    let mut zero_hashes = 0usize;

    for path in &fixtures {
        let Ok(data) = std::fs::read(path) else {
            continue;
        };
        let Ok(dict) = CrClipDictionary::from_bytes(&data) else {
            continue;
        };
        for animation in primary_animations(&dict) {
            let hashes: Vec<u32> = animation.blocks.iter().flatten().map(|b| b.hash).collect();
            if hashes.is_empty() {
                continue;
            }
            animations += 1;
            blocks += hashes.len();
            zero_hashes += hashes.iter().filter(|&&h| h == 0).count();
            if hashes.contains(&animation.signature) {
                equals_signature += 1;
            }
            if hashes.len() > 1 {
                multi_block += 1;
                if hashes.iter().all(|&h| h == hashes[0]) {
                    all_blocks_agree += 1;
                }
            }
        }
    }

    eprintln!(
        "\n=== is hash an animation identity? ===\n\
         {} animation(s), {} block(s), {} with a zero hash\n\
         {} multi-block animation(s), {} of them with every block agreeing\n\
         {} animation(s) where some block hash equals signature",
        animations, blocks, zero_hashes, multi_block, all_blocks_agree, equals_signature
    );

    assert!(
        multi_block > 0,
        "no multi-block animation reached, so per-block variation is unfalsifiable"
    );
    assert_eq!(
        all_blocks_agree, 0,
        "{all_blocks_agree} of {multi_block} multi-block animation(s) give every block \
         the same hash. If that is now the rule, the field is an animation identity \
         and `reencode_animation` should carry it rather than zeroing it — which would \
         close one of the two open losses in this suite."
    );
    assert_eq!(
        equals_signature, 0,
        "{equals_signature} animation(s) have a block hash equal to signature, so the \
         two may be the same quantity and the block field may be derivable after all"
    );
}

#[test]
fn a_quantizing_policy_still_leaves_the_header_width_at_zero() {
    let frames: Vec<[f32; 4]> = (0..8)
        .map(|i| [i as f32, i as f32 * 2.0, i as f32 * 0.5, 0.0])
        .collect();
    let table = DofSampleTable::from_frames(format_type::VECTOR3, &frames);

    let quantized: CrBlockStream = encode_block(
        std::slice::from_ref(&table),
        8,
        EncodePolicy::Quantize { bits: 12 },
    )
    .expect("encode")
    .into();
    let lossless =
        encode_block(std::slice::from_ref(&table), 8, EncodePolicy::Lossless).expect("encode");

    assert_ne!(
        quantized.data, lossless.data,
        "the quantizing policy produced the same payload as the lossless one, so \
         this test is not exercising kQuantize at all"
    );
    assert!(
        quantized.data.len() < lossless.data.len(),
        "a 12-bit quantized channel should be smaller than a 32-bit raw one; got \
         {} against {}",
        quantized.data.len(),
        lossless.data.len()
    );

    assert_eq!(
        quantized.quantize_size, 0,
        "quantize_size is stated now. Update the module doc: it was 0 for every \
         policy, and the reason it stayed 0 was that its units were unknown."
    );
}
