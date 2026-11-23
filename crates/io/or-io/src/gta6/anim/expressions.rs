//! An expression set, the body of signature `0PhhPhPh`: the tracks it reads (facial controls,
//! bones and tracks of one unnamed word) and the tracks it writes (facial controls and face-rig
//! bones), each group in 16-byte slots of its root. The set holds nothing else, so how what it
//! reads becomes what it writes is not in it.

use std::collections::HashSet;

use super::{AnimError, Section};
use crate::decl::{Aligned, Image};

/// The signature of the headerless body an expression set is.
pub const EXPRESSIONS_SIGNATURE: &str = "0PhhPhPh";
/// Each group's record, 8 bytes before its first slot: slots lie at 0x18 + 16 k.
const INPUTS: usize = 0x10;
const INPUT_COUNTS: usize = 0x48;
const BONES: usize = 0x50;
const BONE_MASKS: usize = 0x88;
const CONTROLS: usize = 0x90;
const CONTROL_SLOTS: usize = 0xC8;
const CONTROL_WORDS: usize = 0xD8;
const FACIAL_OUTPUTS: usize = 0xE0;
const OUTPUT_COUNTS: usize = 0x118;
const OUTPUTS: usize = 0x120;
const OUTPUT_MASKS: usize = 0x158;
const TRACK_LEN: usize = 8;
/// The track word of group C, whose name is not known.
const CONTROL_TRACK: u32 = 0x8332_88FB;

/// An expression set: its key, the tracks it reads in one slot space, those it writes in another.
#[derive(Debug, Clone, PartialEq)]
pub struct Expressions {
    /// The word at 0x10, stated twice, whose name is not known.
    pub key: u32,
    /// Group A: the facial controls it reads, 1 to 3 components each.
    pub inputs: ExpressionTracks,
    /// Group B: the bones it reads, a rotation and a translation of each, starting 3 slots apart.
    pub bones: ExpressionTracks,
    /// One for each of [`Expressions::bones`]: 7 or 0x87.
    pub bone_masks: Vec<u8>,
    /// Group C: the tracks of one unnamed word it reads, one slot and one byte each.
    pub controls: ExpressionTracks,
    /// Slot 11: further input slots of group C.
    pub control_slots: Vec<u16>,
    /// Slot 12: one word for each of [`Expressions::control_slots`].
    pub control_words: Vec<u32>,
    /// Group D: the facial controls it writes, in the first output slots.
    pub facial_outputs: ExpressionTracks,
    /// Group O: the face-rig bone tracks it drives, in the output slots after group D's.
    pub outputs: Option<ExpressionTracks>,
    /// One for each output track: 0x80 and the x, y and z bits it drives.
    pub output_masks: Vec<u8>,
}

/// One group's tracks: each a 32-bit id and its track's word, and what it states for each.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpressionTracks {
    /// `(id, track word)`: a facial control's or a bone's hash, and its track's hash.
    pub tracks: Vec<(u32, u32)>,
    /// The group's slots, one for each component it reads or writes.
    pub slots: Vec<u16>,
    /// A facial group's components a track, group C's byte, or a bone group's first slot a track.
    pub components: Vec<u16>,
}

/// A group of an expression set's tracks, in the order its root states them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpressionGroup {
    /// Group A: facial controls it reads.
    Inputs,
    /// Group B: bones it reads.
    Bones,
    /// Group C: tracks of the unnamed word it reads.
    Controls,
    /// Group D: facial controls it writes.
    FacialOutputs,
    /// Group O: face-rig bones it drives.
    Outputs,
}

impl ExpressionGroup {
    pub const ALL: [Self; 5] = [
        Self::Inputs,
        Self::Bones,
        Self::Controls,
        Self::FacialOutputs,
        Self::Outputs,
    ];

    /// Whether the set reads this group's tracks; it writes the others'.
    pub fn reads(self) -> bool {
        matches!(self, Self::Inputs | Self::Bones | Self::Controls)
    }

    /// This group's tracks in `set`; none for group O where the set drives no bone.
    pub fn of(self, set: &Expressions) -> Option<&ExpressionTracks> {
        match self {
            Self::Inputs => Some(&set.inputs),
            Self::Bones => Some(&set.bones),
            Self::Controls => Some(&set.controls),
            Self::FacialOutputs => Some(&set.facial_outputs),
            Self::Outputs => set.outputs.as_ref(),
        }
    }

    /// This group's tracks in `set`, to edit.
    pub fn of_mut(self, set: &mut Expressions) -> Option<&mut ExpressionTracks> {
        match self {
            Self::Inputs => Some(&mut set.inputs),
            Self::Bones => Some(&mut set.bones),
            Self::Controls => Some(&mut set.controls),
            Self::FacialOutputs => Some(&mut set.facial_outputs),
            Self::Outputs => set.outputs.as_mut(),
        }
    }

    /// The mask bone track `track` of group B or O states; none for any other group.
    pub fn mask(self, set: &Expressions, track: usize) -> Option<u8> {
        match self {
            Self::Bones => set.bone_masks.get(track).copied(),
            Self::Outputs => set.output_masks.get(track).copied(),
            _ => None,
        }
    }

    /// The slots track `track` of this group reads or writes.
    pub fn slots(self, set: &Expressions, track: usize) -> Option<&[u16]> {
        let group = self.of(set)?;
        let component = usize::from(*group.components.get(track)?);
        let (start, len) = match self {
            // A facial track's slots follow the components of the tracks before it.
            Self::Inputs | Self::FacialOutputs => {
                let before = group.components.get(..track)?;
                (before.iter().map(|c| usize::from(*c)).sum(), component)
            }
            Self::Bones => (component, 3),
            Self::Controls => (track, 1),
            Self::Outputs => {
                let mask = self.mask(set, track)?;
                (component, (mask & 7).count_ones() as usize)
            }
        };
        group.slots.get(start..start.checked_add(len)?)
    }
}

/// The expression set whose virtual section, addressed from 0x5000_0000, is `section`.
pub fn read_expressions(section: &[u8]) -> Result<Expressions, AnimError> {
    let aligned = Aligned::from_bytes(section);
    let s = Section {
        image: Image::new(aligned.bytes(), section.len()),
        left: std::cell::Cell::new(section.len().saturating_mul(2)),
    };
    let bad = |why: String| AnimError::Expressions(why);
    if s.u64("expression set", 0)? != 0 || s.pointer("block map", 8)?.is_none() {
        return Err(bad(
            "its first word is not zero or its block map pointer is null".into(),
        ));
    }
    let key = s.u32("expression set", INPUTS)?;
    if s.u32("expression set", INPUTS + 4)? != key {
        return Err(bad("its key at 0x10 is not stated twice".into()));
    }
    let inputs = s.tracks(INPUTS, 1)?;
    s.facial(&inputs, INPUT_COUNTS, "inputs")?;
    let bones = s.tracks(BONES, 2)?;
    let bone_masks = s.slot(BONE_MASKS, 1)?.to_vec();
    let thirds =
        (bones.components.iter().enumerate()).all(|(k, start)| usize::from(*start) == 3 * k);
    if bones.slots.len() != 3 * bones.tracks.len()
        || !thirds
        || bone_masks.len() != bones.tracks.len()
        || bone_masks.iter().any(|mask| mask & 0x7F != 7)
    {
        return Err(bad(
            "its bone inputs are not three slots, a start 3k and a mask 7 or 0x87 a track".into(),
        ));
    }
    let controls = s.tracks(CONTROLS, 1)?;
    let control_slots: Vec<u16> = (s.slot(CONTROL_SLOTS, 2)?.as_chunks::<2>().0.iter())
        .map(|w| u16::from_le_bytes(*w))
        .collect();
    let control_words: Vec<u32> = (s.slot(CONTROL_WORDS, 4)?.as_chunks::<4>().0.iter())
        .map(|w| u32::from_le_bytes(*w))
        .collect();
    if controls.slots.len() != controls.tracks.len()
        || controls
            .tracks
            .iter()
            .any(|(_, track)| *track != CONTROL_TRACK)
    {
        return Err(bad(format!(
            "its inputs of track {CONTROL_TRACK:#010X} are not one slot a track of that word"
        )));
    }
    let facial_outputs = s.tracks(FACIAL_OUTPUTS, 1)?;
    s.facial(&facial_outputs, OUTPUT_COUNTS, "facial outputs")?;
    if (facial_outputs.slots.iter().enumerate()).any(|(i, slot)| usize::from(*slot) != i) {
        return Err(bad("its facial outputs' slots are not 0 onward".into()));
    }
    // The output record is there where its track pointer is a virtual address in the section.
    let pointer = s.u64("expression set", OUTPUTS + 0x10).unwrap_or(0);
    let present = pointer >> 28 == 5 && ((pointer & 0x0FFF_FFFF) as usize) < section.len();
    let (outputs, output_masks) = match present {
        false => (None, Vec::new()),
        true => {
            let outputs = s.tracks(OUTPUTS, 2)?;
            let masks = s.slot(OUTPUT_MASKS, 1)?.to_vec();
            // A track's first slot follows the x, y and z bits of the masks before it.
            let mut run = 0;
            let mut holds = masks.len() == outputs.tracks.len();
            for (start, mask) in outputs.components.iter().zip(&masks) {
                holds &= (mask & 0xF8) == 0x80 && mask & 7 != 0 && usize::from(*start) == run;
                run += (mask & 7).count_ones() as usize;
            }
            let first = facial_outputs.slots.len();
            let follows =
                (outputs.slots.iter().enumerate()).all(|(i, slot)| usize::from(*slot) == first + i);
            if !holds || run != outputs.slots.len() || !follows {
                return Err(bad("its bone outputs are not a mask 0x80 and x, y and z bits a track, starting where the bits before it end, in the slots after its facial outputs'".into()));
            }
            (Some(outputs), masks)
        }
    };
    if outputs.is_none() && !bones.tracks.is_empty() {
        return Err(bad("it reads bones and drives none".into()));
    }
    Ok(Expressions {
        key,
        inputs,
        bones,
        bone_masks,
        controls,
        control_slots,
        control_words,
        facial_outputs,
        outputs,
        output_masks,
    })
}

/// `set`'s track ids written in place over the expression set `section` holds; nothing else of it.
// Any other change is refused: it would move or break a slot.
pub fn write_expression_ids(section: &mut [u8], set: &Expressions) -> Result<(), AnimError> {
    let stored = read_expressions(section)?;
    let unwritten = |why: String| AnimError::Unwritten(why);
    if without_ids(set) != without_ids(&stored) {
        return Err(unwritten(
            "only a track's id is written in place: any other change to an expression set moves or breaks its slots".into(),
        ));
    }
    let aligned = Aligned::from_bytes(section);
    let s = Section {
        image: Image::new(aligned.bytes(), section.len()),
        left: std::cell::Cell::new(section.len().saturating_mul(2)),
    };
    let mut places = Vec::new();
    for ((at, edited), (_, held)) in groups(set).into_iter().zip(groups(&stored)) {
        let (Some(edited), Some(held)) = (edited, held) else {
            continue;
        };
        if edited.tracks == held.tracks {
            continue;
        }
        let mut seen = HashSet::new();
        if !edited.tracks.iter().all(|track| seen.insert(*track)) {
            return Err(unwritten(format!(
                "the group at {at:#X} would state one id on one track word twice"
            )));
        }
        places.push((s.target("expression record", at + 0x10)?, &edited.tracks));
    }
    let mut out = section.to_vec();
    for (from, tracks) in places {
        for (k, (id, _)) in tracks.iter().enumerate() {
            let at = from + TRACK_LEN * k;
            let word = out.get_mut(at..at + 4).ok_or_else(|| {
                unwritten(format!(
                    "track {k} of the array at {from:#X} lies past the section"
                ))
            })?;
            word.copy_from_slice(&id.to_le_bytes());
        }
    }
    if read_expressions(&out)? != *set {
        return Err(unwritten(
            "the ids written do not read back as edited: two groups share one array".into(),
        ));
    }
    section.copy_from_slice(&out);
    Ok(())
}

/// Each group's record offset and its tracks, group O's none where the set drives no bone.
fn groups(set: &Expressions) -> [(usize, Option<&ExpressionTracks>); 5] {
    [
        (INPUTS, Some(&set.inputs)),
        (BONES, Some(&set.bones)),
        (CONTROLS, Some(&set.controls)),
        (FACIAL_OUTPUTS, Some(&set.facial_outputs)),
        (OUTPUTS, set.outputs.as_ref()),
    ]
}

/// `set` with every track's id zero: what an id edit leaves as it was.
fn without_ids(set: &Expressions) -> Expressions {
    let mut set = set.clone();
    let each = [
        &mut set.inputs,
        &mut set.bones,
        &mut set.controls,
        &mut set.facial_outputs,
    ];
    for group in each.into_iter().chain(set.outputs.as_mut()) {
        group.tracks.iter_mut().for_each(|(id, _)| *id = 0);
    }
    set
}

impl Section<'_> {
    /// The slot at `at`, `{u32 n, u32 n, pointer}`: the bytes of its n elements of `width` bytes.
    fn slot(&self, at: usize, width: usize) -> Result<&[u8], AnimError> {
        let (n, again) = (
            self.u32("expression slot", at)?,
            self.u32("expression slot", at + 4)?,
        );
        if n != again || n as usize > self.image.len() {
            return Err(AnimError::Expressions(format!(
                "the slot at {at:#X} states {n} and {again}, where both are one count the section can hold"
            )));
        }
        let len = n as usize * width;
        if len == 0 {
            return Ok(&[]);
        }
        let from = self.target("expression slot", at + 8)?;
        self.spend(len)?;
        self.bytes("expression slot", from, len)
    }

    /// Whether a facial group's components are 1 to 3 a track, sum to its slots and are counted
    /// by width in the slot `{0, n1, n2, n3}` at `counts`.
    fn facial(&self, group: &ExpressionTracks, counts: usize, what: &str) -> Result<(), AnimError> {
        let mut stated = [0u32; 4];
        for (i, word) in stated.iter_mut().enumerate() {
            *word = self.u32("expression counts", counts + 4 * i)?;
        }
        let by = |width: u16| group.components.iter().filter(|c| **c == width).count() as u32;
        let sum: usize = group.components.iter().map(|c| usize::from(*c)).sum();
        let holds = group.components.iter().all(|c| (1..=3).contains(c))
            && sum == group.slots.len()
            && stated == [0, by(1), by(2), by(3)];
        if !holds {
            return Err(AnimError::Expressions(format!(
                "its {what}' components are not 1 to 3 a track, summing to its slots and counted at {counts:#X}"
            )));
        }
        Ok(())
    }

    /// The record at `at`: `{n, n}` and its tracks at 0x08, `{m, m}` and its u16 slots at 0x18,
    /// `{n, n}` and its `width`-byte components at 0x28.
    fn tracks(&self, at: usize, width: usize) -> Result<ExpressionTracks, AnimError> {
        let count = |field: usize| -> Result<usize, AnimError> {
            let (n, again) = (
                self.u32("expression record", at + field)?,
                self.u32("expression record", at + field + 4)?,
            );
            if n != again || n as usize > self.image.len() {
                return Err(AnimError::Expressions(format!(
                    "the record at {at:#X} states {n} and {again} at {field:#X}, where both counts are one count the section can hold"
                )));
            }
            Ok(n as usize)
        };
        let (n, m) = (count(0x08)?, count(0x18)?);
        if count(0x28)? != n {
            return Err(AnimError::Expressions(format!(
                "the record at {at:#X} states two track counts"
            )));
        }
        let array = |field: usize, len: usize| -> Result<&[u8], AnimError> {
            if len == 0 {
                return Ok(&[]);
            }
            let from = self.target("expression record", at + field)?;
            self.spend(len)?;
            self.bytes("expression record", from, len)
        };
        let tracks = (array(0x10, TRACK_LEN * n)?.as_chunks::<8>().0.iter())
            .map(|t| {
                let word = |i: usize| u32::from_le_bytes([t[i], t[i + 1], t[i + 2], t[i + 3]]);
                (word(0), word(4))
            })
            .collect();
        let slots = (array(0x20, 2 * m)?.as_chunks::<2>().0.iter())
            .map(|w| u16::from_le_bytes(*w))
            .collect();
        let components = array(0x30, width * n)?;
        let components = match width {
            1 => components.iter().map(|c| u16::from(*c)).collect(),
            _ => (components.as_chunks::<2>().0.iter())
                .map(|w| u16::from_le_bytes(*w))
                .collect(),
        };
        Ok(ExpressionTracks {
            tracks,
            slots,
            components,
        })
    }
}

#[cfg(test)]
#[path = "expressions_tests.rs"]
mod tests;
