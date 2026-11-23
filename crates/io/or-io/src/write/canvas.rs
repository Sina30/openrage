//! Which bytes of a rebuilt resource did nobody write?
//!
//! ⚠ A reservation is zero-filled, and zero is a plausible value for most of
//! what a resource holds, so a writer that misses a field produces output that
//! parses, reloads and passes a census that compares lengths. The same value
//! tree is therefore built twice under two different fills: a written byte is
//! the same in both and an unwritten one is the fill, so **untouched is exactly
//! "the two runs disagree here"**, even for a byte written to the poison value.
//! It measures the writer from outside rather than trusting a coverage map the
//! writer keeps, because a writer that forgets to write a field is exactly the
//! writer that forgets to record it.
//!
//! ⚠ **Not `meta`'s `write_with_coverage`, and not to be merged with it.** A
//! PSO or RBM writer encodes over the document it opened, so an unwritten field
//! inherits the old file's value; an RSC7 writer's canvas is a fresh
//! reservation, so an unwritten field inherits zero. Most untouched bytes are legitimate gaps and alignment tails:
//! a run is a finding only when it lands inside a described field, which is
//! what [`Canvas::inherited`] answers.

use std::collections::BTreeMap;

use crate::container::paging::{ResourceHeap, DEFAULT_CANVAS_FILL, POISON_CANVAS_FILL};
use crate::desc::{FieldRepr, Pack, PackType, Value};

use crate::write::build::{build_into_mapping_with, BlobWriters, BuildError, StructMap};

/// A run of bytes nobody wrote, as an offset into a heap and a length.
pub type Untouched = (usize, usize);

#[derive(Debug)]
pub struct Canvas {
    pub root: u64,
    pub virtual_data: Vec<u8>,
    pub physical_data: Vec<u8>,
    pub virtual_untouched: Vec<Untouched>,
    pub physical_untouched: Vec<Untouched>,
    pub structs: StructMap,
}

impl Canvas {
    pub fn untouched_bytes(&self) -> usize {
        self.virtual_untouched
            .iter()
            .chain(&self.physical_untouched)
            .map(|(_, len)| len)
            .sum()
    }

    pub fn total_bytes(&self) -> usize {
        self.virtual_data.len() + self.physical_data.len()
    }

    /// Every described field the build left holding its canvas.
    ///
    /// Reported once per `(type, field)` with the number of instances, because
    /// a missed field on a type a file holds four hundred of is one defect.
    pub fn inherited(&self, pack: &Pack) -> Vec<Inherited> {
        let mut found: BTreeMap<(&str, &str), Inherited> = BTreeMap::new();
        for placed in &self.structs.structs {
            let Some(ty) = pack.type_by_name(&placed.type_name) else {
                continue;
            };
            self.attribute(pack, ty, placed.at, &mut found);
        }
        found.into_values().collect()
    }

    fn attribute<'p>(
        &self,
        pack: &'p Pack,
        ty: &'p PackType,
        base: usize,
        found: &mut BTreeMap<(&'p str, &'p str), Inherited>,
    ) {
        for field in &ty.fields {
            let at = base + field.offset;
            match &field.repr {
                FieldRepr::Inline { type_name } => {
                    if let Some(inner) = pack.type_by_name(type_name) {
                        self.attribute(pack, inner, at, found);
                    }
                }
                FieldRepr::InlineArray { type_name, count } => {
                    if let Some(inner) = pack.type_by_name(type_name) {
                        for i in 0..*count {
                            self.attribute(pack, inner, at + i * inner.size, found);
                        }
                    }
                }
                // Padding is the one repr whose whole job is to be unwritten.
                FieldRepr::Padding { .. } => {}
                repr => {
                    let Some(width) = repr.inline_size() else {
                        continue;
                    };
                    if width == 0 {
                        continue;
                    }
                    let untouched = self.untouched_in(at, width);
                    if untouched == 0 {
                        continue;
                    }
                    let entry = found
                        .entry((ty.name.as_str(), field.name.as_str()))
                        .or_insert_with(|| Inherited {
                            type_name: ty.name.clone(),
                            field: field.name.clone(),
                            width,
                            instances: 0,
                            first_at: at,
                            bytes: 0,
                        });
                    entry.instances += 1;
                    entry.bytes += untouched;
                    entry.first_at = entry.first_at.min(at);
                }
            }
        }
    }

    /// How many bytes of `at..at + width` fall in a run nobody wrote.
    ///
    /// ⚠ The runs come in order and do not overlap (`differing_runs` walks the heap once), so the
    /// first that matters is found by a search: a scan of every run for each field of a file that
    /// holds millions of them took longer than fifteen minutes on `core.ypt`.
    fn untouched_in(&self, at: usize, width: usize) -> usize {
        let end = at + width;
        let runs = &self.virtual_untouched;
        let first = runs.partition_point(|(start, len)| start + len <= at);
        (runs[first..].iter())
            .take_while(|(start, _)| *start < end)
            .map(|(start, len)| end.min(start + len).saturating_sub(at.max(*start)))
            .sum()
    }
}

/// A described field whose bytes came from the reservation rather than a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inherited {
    pub type_name: String,
    pub field: String,
    pub width: usize,
    /// How many instances of this `(type, field)` hold unwritten bytes.
    pub instances: usize,
    pub first_at: usize,
    pub bytes: usize,
}

impl std::fmt::Display for Inherited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}::{} ({} byte(s) wide): {} instance(s), {} byte(s) never written, first at {:#x}",
            self.type_name, self.field, self.width, self.instances, self.bytes, self.first_at
        )
    }
}

pub fn coverage(pack: &Pack, value: &Value) -> Result<Canvas, BuildError> {
    coverage_with(pack, value, &BlobWriters::new())
}

/// Build twice under two canvas fills and report what the two disagree about.
///
/// ⚠ **The two builds must differ in nothing but the fill.** They do: the fill
/// changes no allocation's size and no address, so the placements are
/// identical and a differing byte can only be an unwritten one. If that ever
/// stops being true the two heaps come out different lengths, which this
/// refuses rather than diffing to the shorter.
pub fn coverage_with(
    pack: &Pack,
    value: &Value,
    blobs: &BlobWriters,
) -> Result<Canvas, BuildError> {
    let (root, structs, plain_virtual, plain_physical) =
        build_under(pack, value, blobs, DEFAULT_CANVAS_FILL)?;
    let (poisoned_root, _, poisoned_virtual, poisoned_physical) =
        build_under(pack, value, blobs, POISON_CANVAS_FILL)?;

    if root != poisoned_root
        || plain_virtual.len() != poisoned_virtual.len()
        || plain_physical.len() != poisoned_physical.len()
    {
        return Err(BuildError::Builder(
            "a build whose placement depends on what its canvas held: the two fills \
             produced different shapes, so no byte can be attributed to a field"
                .to_string(),
        ));
    }

    Ok(Canvas {
        root,
        virtual_untouched: differing_runs(&plain_virtual, &poisoned_virtual),
        physical_untouched: differing_runs(&plain_physical, &poisoned_physical),
        virtual_data: plain_virtual,
        physical_data: plain_physical,
        structs,
    })
}

fn build_under(
    pack: &Pack,
    value: &Value,
    blobs: &BlobWriters,
    fill: u8,
) -> Result<(u64, StructMap, Vec<u8>, Vec<u8>), BuildError> {
    let mut heap = ResourceHeap::new();
    heap.set_canvas_fill(fill);
    let (root, _, map) = build_into_mapping_with(&mut heap, pack, value, blobs)?;
    let (virtual_data, physical_data) = heap.finish();
    Ok((root, map, virtual_data, physical_data))
}

fn differing_runs(a: &[u8], b: &[u8]) -> Vec<Untouched> {
    let mut out = Vec::new();
    let mut run: Option<usize> = None;
    for i in 0..a.len() {
        match (a[i] != b[i], run) {
            (true, None) => run = Some(i),
            (false, Some(start)) => {
                out.push((start, i - start));
                run = None;
            }
            _ => {}
        }
    }
    if let Some(start) = run {
        out.push((start, a.len() - start));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field is charged for the bytes of the runs it overlaps, counted as a scan of every run counts them.
    #[test]
    fn the_bytes_a_field_holds_unwritten_are_those_of_the_runs_it_overlaps() {
        let runs: Vec<Untouched> = vec![(2, 3), (8, 1), (10, 6), (20, 2)];
        let canvas = Canvas {
            root: 0,
            virtual_data: Vec::new(),
            physical_data: Vec::new(),
            virtual_untouched: runs.clone(),
            physical_untouched: Vec::new(),
            structs: StructMap::default(),
        };
        let scan = |at: usize, width: usize| -> usize {
            (at..at + width)
                .filter(|byte| runs.iter().any(|(s, l)| (*s..s + l).contains(byte)))
                .count()
        };
        for at in 0..26 {
            for width in 1..12 {
                assert_eq!(
                    canvas.untouched_in(at, width),
                    scan(at, width),
                    "{width} byte(s) at {at}"
                );
            }
        }
    }

    /// The instrument, on a heap rather than on a resource.
    ///
    /// Reserve sixteen bytes, write eight of them, and the other eight have to
    /// come back as unwritten. Without this the whole module is only ever
    /// exercised through a corpus, and a corpus-gated instrument reports
    /// "nothing inherited" on a machine with no corpus.
    #[test]
    fn a_reservation_nobody_filled_is_what_the_two_fills_disagree_about() {
        let write = |fill: u8| {
            let mut heap = ResourceHeap::new();
            heap.set_canvas_fill(fill);
            heap.alloc_virtual(16);
            heap.write_bytes_at(0, &[1, 2, 3, 4, 5, 6, 7, 8])
                .expect("in bounds");
            heap.finish().0
        };

        let plain = write(DEFAULT_CANVAS_FILL);
        let poisoned = write(POISON_CANVAS_FILL);

        assert_eq!(differing_runs(&plain, &poisoned), vec![(8, 8)]);
        assert_eq!(
            &plain[..8],
            &[1, 2, 3, 4, 5, 6, 7, 8],
            "the written half must not move with the fill"
        );
    }

    /// ⚠ A byte written *to the poison value* is still a written byte.
    ///
    /// This is the case a single-poison comparison — is this byte `0xCD`? —
    /// cannot decide, and it is why the instrument builds twice instead.
    #[test]
    fn a_byte_written_to_the_poison_value_is_not_reported_as_unwritten() {
        let write = |fill: u8| {
            let mut heap = ResourceHeap::new();
            heap.set_canvas_fill(fill);
            heap.alloc_virtual(4);
            heap.write_bytes_at(0, &[POISON_CANVAS_FILL, POISON_CANVAS_FILL])
                .expect("in bounds");
            heap.finish().0
        };

        assert_eq!(
            differing_runs(&write(DEFAULT_CANVAS_FILL), &write(POISON_CANVAS_FILL)),
            vec![(2, 2)],
            "the two poison bytes were written and must not be reported"
        );
    }

    #[test]
    fn nothing_differs_when_everything_is_written() {
        let write = |fill: u8| {
            let mut heap = ResourceHeap::new();
            heap.set_canvas_fill(fill);
            heap.write_bytes(&[9, 9, 9, 9]);
            heap.finish().0
        };
        assert!(differing_runs(&write(DEFAULT_CANVAS_FILL), &write(POISON_CANVAS_FILL)).is_empty());
    }
}
