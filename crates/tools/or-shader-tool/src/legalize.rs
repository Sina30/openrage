//! Rewrites the SPIR-V a DXBC translator emits into the subset naga reads, and reports what it changed.

use std::collections::{HashMap, HashSet};

use thiserror::Error;

mod inline;
mod interface;
mod repairs;

const MAGIC: u32 = 0x0723_0203;
const HEADER_WORDS: usize = 5;

const OP_EXECUTION_MODE: u32 = 16;
const OP_EXECUTION_MODE_ID: u32 = 331;
const OP_MEMORY_MODEL: u32 = 14;
const OP_CAPABILITY: u32 = 17;
const OP_CONSTANT: u32 = 43;
const OP_PHI: u32 = 245;
const OP_LABEL: u32 = 248;
const OP_BRANCH: u32 = 249;
const OP_KILL: u32 = 252;
const OP_RETURN: u32 = 253;
const OP_DEMOTE_TO_HELPER_INVOCATION: u32 = 5380;

const MODE_LOCAL_SIZE: u32 = 17;
const MODE_LOCAL_SIZE_ID: u32 = 38;
/// Denorm and rounding modes: the float-controls execution modes a GPU applies on its own.
const FLOAT_MODES: [u32; 5] = [4459, 4460, 4461, 4462, 4463];

/// The capabilities naga does not read and a translated D3D shader does not need: the Vulkan memory model, physical pointers and the float-control modes.
const DROPPED_CAPABILITIES: [u32; 8] = [5345, 5346, 5347, 4464, 4465, 4466, 4467, 4468];
const CAPABILITY_DEMOTE_TO_HELPER_INVOCATION: u32 = 5379;

const ADDRESSING_LOGICAL: u32 = 0;
const MEMORY_MODEL_GLSL450: u32 = 1;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LegalizeError {
    #[error("not a SPIR-V module: {0}")]
    NotSpirv(&'static str),
    #[error("an instruction at word {0} runs past the module")]
    Overrun(usize),
}

/// What a rewrite changed, so a shader can be labelled as exact or approximated.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Lowerings {
    /// The addressing and memory model were not Logical and GLSL450.
    pub memory_model: bool,
    pub capabilities_dropped: usize,
    pub float_modes_dropped: usize,
    pub local_size_ids: usize,
    /// A demote ended its block with `OpKill`: the invocation stops instead of continuing as a helper, which changes only what later derivatives see.
    pub demotes_to_kill: usize,
    /// A demote that did not end its block, left for naga to refuse.
    pub demotes_kept: usize,
    /// Locations whose variables were merged into one vector variable.
    pub interface_groups_merged: usize,
    /// Locations of a vertex stage's outputs or a fragment stage's inputs held by one variable of fewer than four components, which became a four-component variable so both stages of a pipeline type the location alike.
    pub interface_locations_widened: usize,
    /// Locations shared by variables that could not be merged: different types or decorations, or overlapping components.
    pub interface_groups_kept: usize,
    /// Clip and cull distances made private, so they no longer clip: an approximation.
    pub clip_distances_dropped: usize,
    /// Base vertex, base instance and draw index inputs made private variables of nought, which is what a draw that starts at nought reads.
    pub base_parameters_zeroed: usize,
    /// Negations of an unsigned integer written as a subtraction from nought, which is exact.
    pub unsigned_negations_subtracted: usize,
    /// Storage buffers the shader only writes, which can be read as well now.
    pub written_buffers_made_readable: usize,
    /// Initial values taken off workgroup variables, whose contents start undefined.
    pub workgroup_initializers_dropped: usize,
    /// Coverage-mask outputs declared as an array of one, written as the integer naga has them as.
    pub sample_masks_scalarized: usize,
    /// Reads of an input at a sample, a centroid or an offset made reads at the pixel's centre: an approximation for a draw of more than one sample.
    pub interpolations_loaded: usize,
    /// Calls of a helper function replaced by its instructions, which is exact.
    pub helper_calls_inlined: usize,
    /// Searches for a set bit whose result SPIR-V types as the other signedness than its operand, done as the operand's and bitcast: exact, since the bits are the same.
    pub bit_searches_retyped: usize,
}

impl Lowerings {
    /// The rewrites that changed the module, by name and count.
    pub fn counts(&self) -> std::collections::BTreeMap<&'static str, usize> {
        [
            ("memory_model", usize::from(self.memory_model)),
            ("capabilities_dropped", self.capabilities_dropped),
            ("float_modes_dropped", self.float_modes_dropped),
            ("local_size_ids", self.local_size_ids),
            ("demotes_to_kill", self.demotes_to_kill),
            ("demotes_kept", self.demotes_kept),
            ("interface_groups_merged", self.interface_groups_merged),
            (
                "interface_locations_widened",
                self.interface_locations_widened,
            ),
            ("interface_groups_kept", self.interface_groups_kept),
            ("clip_distances_dropped", self.clip_distances_dropped),
            ("base_parameters_zeroed", self.base_parameters_zeroed),
            (
                "unsigned_negations_subtracted",
                self.unsigned_negations_subtracted,
            ),
            (
                "written_buffers_made_readable",
                self.written_buffers_made_readable,
            ),
            (
                "workgroup_initializers_dropped",
                self.workgroup_initializers_dropped,
            ),
            ("sample_masks_scalarized", self.sample_masks_scalarized),
            ("interpolations_loaded", self.interpolations_loaded),
            ("helper_calls_inlined", self.helper_calls_inlined),
            ("bit_searches_retyped", self.bit_searches_retyped),
        ]
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .collect()
    }

    /// Whether a rewrite changed what the module does and not only how it says it.
    pub fn approximates(&self) -> bool {
        self.demotes_to_kill > 0
            || self.clip_distances_dropped > 0
            || self.interpolations_loaded > 0
    }

    /// Add another module's lowerings to this total.
    pub fn add(&mut self, other: &Lowerings) {
        self.memory_model |= other.memory_model;
        self.capabilities_dropped += other.capabilities_dropped;
        self.float_modes_dropped += other.float_modes_dropped;
        self.local_size_ids += other.local_size_ids;
        self.demotes_to_kill += other.demotes_to_kill;
        self.demotes_kept += other.demotes_kept;
        self.interface_groups_merged += other.interface_groups_merged;
        self.interface_locations_widened += other.interface_locations_widened;
        self.interface_groups_kept += other.interface_groups_kept;
        self.clip_distances_dropped += other.clip_distances_dropped;
        self.base_parameters_zeroed += other.base_parameters_zeroed;
        self.unsigned_negations_subtracted += other.unsigned_negations_subtracted;
        self.written_buffers_made_readable += other.written_buffers_made_readable;
        self.workgroup_initializers_dropped += other.workgroup_initializers_dropped;
        self.sample_masks_scalarized += other.sample_masks_scalarized;
        self.interpolations_loaded += other.interpolations_loaded;
        self.helper_calls_inlined += other.helper_calls_inlined;
        self.bit_searches_retyped += other.bit_searches_retyped;
    }
}

/// The instructions after the five-word header, each as its words.
fn instructions(words: &[u32]) -> Result<Vec<&[u32]>, LegalizeError> {
    let mut out = Vec::new();
    let mut at = HEADER_WORDS;
    while at < words.len() {
        let count = (words[at] >> 16) as usize;
        if count == 0 || at + count > words.len() {
            return Err(LegalizeError::Overrun(at));
        }
        out.push(&words[at..at + count]);
        at += count;
    }
    Ok(out)
}

fn opcode(inst: &[u32]) -> u32 {
    inst[0] & 0xffff
}

fn header(opcode: u32, words: usize) -> u32 {
    ((words as u32) << 16) | opcode
}

/// Rewrite `bytes` into a module naga's reader accepts where the cause is one of the lowerings in [`Lowerings`].
pub fn legalize(bytes: &[u8]) -> Result<(Vec<u8>, Lowerings), LegalizeError> {
    if bytes.len() < HEADER_WORDS * 4 || !bytes.len().is_multiple_of(4) {
        return Err(LegalizeError::NotSpirv(
            "its length is not a whole header and words",
        ));
    }
    let words: Vec<u32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if words[0] != MAGIC {
        return Err(LegalizeError::NotSpirv("the magic number is missing"));
    }
    let insts = instructions(&words)?;

    let constants: HashMap<u32, u32> = insts
        .iter()
        .filter(|i| opcode(i) == OP_CONSTANT && i.len() >= 4)
        .map(|i| (i[2], i[3]))
        .collect();

    let mut lowerings = Lowerings::default();
    let mut out: Vec<Vec<u32>> = Vec::with_capacity(insts.len());
    let mut block = 0;
    let mut removed_edges: HashSet<(u32, u32)> = HashSet::new();
    let mut at = 0;
    while at < insts.len() {
        let inst = insts[at];
        at += 1;
        match opcode(inst) {
            OP_CAPABILITY if inst.len() == 2 && DROPPED_CAPABILITIES.contains(&inst[1]) => {
                lowerings.capabilities_dropped += 1;
            }
            OP_EXECUTION_MODE if inst.len() >= 3 && FLOAT_MODES.contains(&inst[2]) => {
                lowerings.float_modes_dropped += 1;
            }
            OP_MEMORY_MODEL if inst.len() == 3 => {
                lowerings.memory_model |=
                    (inst[1], inst[2]) != (ADDRESSING_LOGICAL, MEMORY_MODEL_GLSL450);
                out.push(vec![
                    header(OP_MEMORY_MODEL, 3),
                    ADDRESSING_LOGICAL,
                    MEMORY_MODEL_GLSL450,
                ]);
            }
            OP_EXECUTION_MODE_ID if inst.len() == 6 && inst[2] == MODE_LOCAL_SIZE_ID => {
                let size = |id: u32| constants.get(&id).copied().unwrap_or(1);
                out.push(vec![
                    header(OP_EXECUTION_MODE, 6),
                    inst[1],
                    MODE_LOCAL_SIZE,
                    size(inst[3]),
                    size(inst[4]),
                    size(inst[5]),
                ]);
                lowerings.local_size_ids += 1;
            }
            OP_LABEL if inst.len() == 2 => {
                block = inst[1];
                out.push(inst.to_vec());
            }
            OP_DEMOTE_TO_HELPER_INVOCATION => match insts.get(at) {
                Some(next) if opcode(next) == OP_BRANCH && next.len() == 2 => {
                    removed_edges.insert((block, next[1]));
                    out.push(vec![header(OP_KILL, 1)]);
                    lowerings.demotes_to_kill += 1;
                    at += 1;
                }
                Some(next) if opcode(next) == OP_RETURN => {
                    out.push(vec![header(OP_KILL, 1)]);
                    lowerings.demotes_to_kill += 1;
                    at += 1;
                }
                _ => {
                    lowerings.demotes_kept += 1;
                    out.push(inst.to_vec());
                }
            },
            _ => out.push(inst.to_vec()),
        }
    }

    if lowerings.demotes_kept == 0 {
        out.retain(|i| {
            let drop = opcode(i) == OP_CAPABILITY
                && i.len() == 2
                && i[1] == CAPABILITY_DEMOTE_TO_HELPER_INVOCATION;
            lowerings.capabilities_dropped += usize::from(drop);
            !drop
        });
    }
    drop_phi_operands(&mut out, &removed_edges);
    let mut bound = words[3];
    let (out, merged) = interface::merge_components(out, &mut bound);
    lowerings.interface_groups_merged = merged.groups;
    lowerings.interface_locations_widened = merged.widened;
    lowerings.interface_groups_kept = merged.kept;
    let (out, clipped) = interface::drop_clip_distances(out, &mut bound);
    lowerings.clip_distances_dropped = clipped;
    let (out, zeroed) = interface::zero_base_parameters(out, &mut bound);
    lowerings.base_parameters_zeroed = zeroed;
    let (out, subtracted) = repairs::subtract_unsigned_negations(out, &mut bound);
    lowerings.unsigned_negations_subtracted = subtracted;
    let (out, readable) = repairs::make_written_buffers_readable(out);
    lowerings.written_buffers_made_readable = readable;
    let (out, dropped) = repairs::drop_workgroup_initializers(out);
    lowerings.workgroup_initializers_dropped = dropped;
    let (out, masks) = repairs::scalarize_sample_masks(out, &mut bound);
    lowerings.sample_masks_scalarized = masks;
    let (out, interpolated) = repairs::load_interpolations(out);
    lowerings.interpolations_loaded = interpolated;
    let (out, inlined) = inline::inline_helpers(out, &mut bound);
    lowerings.helper_calls_inlined = inlined;
    let (out, retyped) = repairs::retype_bit_searches(out, &mut bound);
    lowerings.bit_searches_retyped = retyped;

    let mut module: Vec<u32> = words[..HEADER_WORDS].to_vec();
    module[3] = bound;
    module.extend(out.into_iter().flatten());
    Ok((
        module.iter().flat_map(|w| w.to_le_bytes()).collect(),
        lowerings,
    ))
}

/// A block that no longer branches to its successor is no longer one of that successor's phi parents.
fn drop_phi_operands(out: &mut [Vec<u32>], removed: &HashSet<(u32, u32)>) {
    if removed.is_empty() {
        return;
    }
    let mut block = 0;
    for inst in out.iter_mut() {
        match opcode(inst) {
            OP_LABEL if inst.len() == 2 => block = inst[1],
            OP_PHI if inst.len() >= 3 => {
                let mut kept = inst[..3].to_vec();
                for pair in inst[3..].as_chunks::<2>().0 {
                    if !removed.contains(&(pair[1], block)) {
                        kept.extend_from_slice(pair);
                    }
                }
                kept[0] = header(OP_PHI, kept.len());
                *inst = kept;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
