//! `.yed`: the programs that drive bones an animation does not — a wheel that
//! turns with speed, a spring that trails a limb — keyed by name hash.

use super::shared::{AtString, PgBase, PgDictionary, Vec3V, Vec4V};
use crate::decl::*;
use crate::Generation;

/// A `.yed`: expression sets keyed by the hash of their name's stem.
pub type CrExpressionsDictionary = PgDictionary<CrExpressions>;

impl Resource for CrExpressionsDictionary {
    const EXT: &'static str = "yed";
    const TARGET: Generation = Generation::Gen8;
}

/// A named set of expression programs, the bone tracks they write, the motions
/// they simulate and the variables they read.
#[record(size = 0x90)]
pub struct CrExpressions {
    pub base: PgBase,
    /// Empty in every shipped expression set.
    expressions: Vec<Ptr<Opaque>>,
    pub expression_streams: Vec<Option<Box<CrExpressionStream>>>,
    pub tracks: Vec<Track>,
    /// Every motion the programs define, gathered where the runtime wants them contiguous.
    pub motions: Vec<CrMotionDescription>,
    /// A program's variable instruction indexes this array rather than naming the hash.
    pub variables: Vec<Variable>,
    /// ⚠ An authoring path, `pack:/<stem>.expr`. The dictionary key is the
    /// hash of the lowercased stem, not of this: see [`key_of`].
    pub name: AtString,
    #[init(1)]
    ref_count: u32,
    #[note("checked against a skeleton before binding; carried, never recomputed")]
    pub signature: u32,
    /// ⚠ One scratch buffer of this size serves every program, so a program
    /// longer than it overruns the buffer: [`validate`] refuses one.
    pub max_packed_size: u32,
    pub flags: u16,
    #[pad_after(8)]
    padding: [u8; 10],
}

/// One compiled program: a header, then its three runs back to back.
///
/// ⚠ The runs are one program in three encodings — an operation byte per
/// instruction, and two parameter runs walked in lockstep with it — so `data`
/// is carried whole: editing one part without the others makes a program the
/// runtime walks off the end of.
#[record(size = 0x10)]
pub struct CrExpressionStream {
    pub hash: u32,
    #[note("bytes of the aligned parameter run, which begins at 0x10")]
    align_param_size: u32,
    #[note("bytes of the parameter run, which follows the aligned one")]
    param_size: u32,
    #[note("one operation byte per instruction; the run follows the parameters")]
    num_operations: u16,
    #[note("evaluation stack the runtime reserves")]
    pub max_stack_depth: u16,
    #[trailing]
    #[len(sum = "align_param_size + param_size + num_operations")]
    pub data: Vec<u8>,
}

impl CrExpressionStream {
    /// A program from its three runs, whose lengths a save states as its three counts.
    pub fn new(
        hash: u32,
        max_stack_depth: u16,
        align_params: &[u8],
        params: &[u8],
        operations: &[u8],
    ) -> Self {
        let mut stream = Self {
            hash,
            max_stack_depth,
            data: [align_params, params, operations].concat(),
            ..Default::default()
        };
        let carry = &mut stream.__carry;
        // ⚠ A run longer than its count can state keeps the count's maximum,
        // so the sum falls short of `data` and the save refuses the program.
        carry.hide(
            std::mem::offset_of!(Raw<Self>, align_param_size),
            u32::try_from(align_params.len()).unwrap_or(u32::MAX),
        );
        carry.hide(
            std::mem::offset_of!(Raw<Self>, param_size),
            u32::try_from(params.len()).unwrap_or(u32::MAX),
        );
        carry.hide(
            std::mem::offset_of!(Raw<Self>, num_operations),
            u16::try_from(operations.len()).unwrap_or(u16::MAX),
        );
        stream
    }

    /// The header and the runs: what `max_packed_size` bounds.
    pub fn size(&self) -> usize {
        0x10 + self.data.len()
    }
}

fn stream_size(align_param_size: u32, param_size: u32, num_operations: u16) -> usize {
    0x10 + align_param_size as usize + param_size as usize + usize::from(num_operations)
}

/// One bone and track a program writes.
#[record(size = 0x04)]
pub struct Track {
    /// A bone's tag, not its index in a skeleton.
    pub id: u16,
    pub track: u8,
    /// ⚠ Bit 7 is not part of the format: it marks the entry as continuing
    /// the one above it, so masking the whole byte reads 128 for format 0.
    pub flags: u8,
}

impl Track {
    pub const CONTINUES: u8 = 0x80;

    pub fn format(&self) -> u8 {
        self.flags & !Self::CONTINUES
    }

    pub fn continues(&self) -> bool {
        self.flags & Self::CONTINUES != 0
    }
}

/// A variable a program reads, by the hash of its name.
#[record(size = 0x04)]
pub struct Variable {
    pub hash: u32,
}

/// One degree-of-freedom block of a motion: stiffness, damping and its limits.
#[record(size = 0x40)]
pub struct Dof3 {
    pub strength: Vec3V,
    pub damping: Vec3V,
    pub min_constraint: Vec3V,
    pub max_constraint: Vec3V,
}

/// A spring a program simulates: linear and angular blocks, a direction and gravity.
#[record(size = 0xa0)]
pub struct CrMotionDescription {
    pub linear: Dof3,
    pub angular: Dof3,
    pub direction: Vec3V,
    /// ⚠ The fourth word holds the bone's tag in its low sixteen bits.
    pub gravity: Vec4V,
}

impl CrMotionDescription {
    /// The bone the motion trails from: a tag, not an index in a skeleton.
    pub fn bone_id(&self) -> u16 {
        self.gravity.w.to_bits() as u16
    }
}

/// The dictionary key an expression named `name` is found by: the hash of its lowercased stem.
pub fn key_of(name: &str) -> u32 {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = file.rfind('.').map_or(file, |dot| &file[..dot]);
    crate::hash::string_hash(stem.to_ascii_lowercase().as_bytes())
}

/// A dictionary the game would misread: its order, or a program too long for its buffer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Broken {
    #[error(
        "the dictionary's codes are not ascending, so a lookup would find the wrong expression"
    )]
    CodesUnsorted,
    #[error("the dictionary holds {codes} codes and {entries} expressions")]
    Lengths { codes: usize, entries: usize },
    #[error("expression {hash:#010x} states {states} bytes of scratch and holds a {longest}-byte program, which would overrun it")]
    ProgramTooLong {
        hash: u32,
        states: u32,
        longest: usize,
    },
    #[error("{0}")]
    View(#[from] ViewError),
}

/// The longest program an expression set holds, header included.
pub fn longest_program(expressions: &View<'_, Raw<CrExpressions>>) -> Result<usize, ViewError> {
    let streams = expressions.expression_streams()?;
    let mut longest = 0;
    for i in 0..streams.len() {
        if let Some(s) = streams.follow(i)? {
            longest = longest.max(stream_size(
                s.align_param_size,
                s.param_size,
                s.num_operations,
            ));
        }
    }
    Ok(longest)
}

/// The dictionary's order, and that no program outgrows the scratch its expression set states.
pub fn validate(image: Image<'_>, root: usize) -> Result<(), Broken> {
    let dict = View::<Raw<CrExpressionsDictionary>>::at_offset(image, root)?;
    let (codes, entries) = (dict.codes()?, dict.entries()?);
    if codes.len() != entries.len() {
        return Err(Broken::Lengths {
            codes: codes.len(),
            entries: entries.len(),
        });
    }
    if !CrExpressionsDictionary::codes_are_sorted(&codes) {
        return Err(Broken::CodesUnsorted);
    }
    for (i, hash) in codes.iter().enumerate() {
        let Some(expressions) = entries.follow(i)? else {
            continue;
        };
        let longest = longest_program(&expressions)?;
        if longest > expressions.max_packed_size as usize {
            return Err(Broken::ProgramTooLong {
                hash: *hash,
                states: expressions.max_packed_size,
                longest,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_the_lowercased_stem_and_not_the_path() {
        let path = "pack:/P_BarrierCrash_02_s.expr";
        assert_eq!(
            key_of(path),
            crate::hash::string_hash(b"p_barriercrash_02_s")
        );
        assert_ne!(key_of(path), crate::hash::string_hash(path.as_bytes()));
    }

    #[test]
    fn a_tracks_continuation_bit_is_not_part_of_its_format() {
        let t = Track {
            id: 0xD325,
            track: 0,
            flags: 0x80,
            ..Default::default()
        };
        assert_eq!(t.format(), 0);
        assert!(t.continues());
    }
}
