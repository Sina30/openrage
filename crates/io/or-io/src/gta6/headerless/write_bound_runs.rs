//! A geometry's +0x148 runs, (key index, primitive count) pairs of u16 that give its primitives in
//! order an entry each of its +0x138 table of 8-byte keys, followed through a primitive added or
//! deleted; a vertex edit leaves them. And a geometry child's record copied with every array it
//! names laid out anew.

use crate::container::rpf::Rpf9Sections;

use super::bound::{child_bound, BOUND_CHILD, GEOMETRY, PLAIN_GEOMETRY};
use super::bytes::{sections, u64_at};
use super::write_bound_grow::{room, BoundGrowError};
use super::write_bvh::NODE;
use super::write_physics::{put, refuse};
use super::{BoundGeometry, ChildBound, HeaderlessError, Result};

type Grown<T> = std::result::Result<T, BoundGrowError>;

/// The word that addresses virtual offset 0.
const VIRTUAL: u64 = 0x5000_0000;
/// No neighbour across an edge.
const OPEN: u16 = 0xFFFF;
/// The most primitives one pair counts.
const LONGEST: usize = 0xFFFF;
/// A BVH record's bytes.
const BVH_RECORD: usize = 0x70;
/// The words of a geometry child's record that name an array a copy lays out anew.
const NAMED: [usize; 7] = [0x88, 0xB0, 0xF0, 0x118, 0x130, 0x138, 0x148];

/// Why a child is not copied.
pub(super) const NOT_COPIED: &str = "only a primitive child or a geometry with a BVH is copied: what a geometry with none holds at +0xF8 and +0x108 is not known";
const EXCESS: &str = "the geometry's runs give keys to more primitives than it holds, and which primitive the extra runs belong to is not known";
const SHARED: &str =
    "a word of the geometry child aims at bytes no reader lays out, so a copy would share them";

/// A geometry's key table and runs where it holds them, each primitive's key index expanded.
struct Runs {
    table: usize,
    entries: usize,
    runs: usize,
    words: usize,
    index: Vec<u16>,
}

/// The table and runs of the geometry child at `at`, expanded over its `primitives`; `None` where
/// both are null.
fn runs_of(v: &[u8], at: usize, primitives: usize) -> Result<Option<Runs>> {
    sections(v, Some(&[]), |b, _| {
        let table = b.ptr("key table", at + 0x138)?;
        let entries = usize::from(b.u16("key count", at + 0x140)?);
        let words = b.u32("run word count", at + 0x144)? as usize;
        let runs = b.ptr("key runs", at + 0x148)?;
        let (table, runs) = match (table, runs) {
            (None, None) if entries == 0 && words == 0 => return Ok(None),
            (Some(t), Some(r)) if entries != 0 && words != 0 => (t, r),
            _ => {
                return Err(refuse(
                    "a geometry's key table and runs are both null with no counts, or both set with counts, in every measured one",
                ))
            }
        };
        if !words.is_multiple_of(2) {
            return Err(refuse(
                "a geometry states its runs in an odd number of words, where a run is two",
            ));
        }
        b.array("key table", table, entries, 8)?;
        let pairs = b.array("key runs", runs, words, 2)?;
        let mut index = Vec::with_capacity(primitives);
        for pair in pairs.as_chunks::<4>().0 {
            let key = u16::from_le_bytes([pair[0], pair[1]]);
            let run = usize::from(u16::from_le_bytes([pair[2], pair[3]]));
            if run == 0 {
                return Err(refuse("a geometry's run gives its key to no primitive"));
            }
            if usize::from(key) >= entries {
                return Err(HeaderlessError::IndexPastCount {
                    what: "run's key index",
                    index: key.into(),
                    count: entries as u64,
                });
            }
            if index.len() + run > primitives {
                return Err(refuse(EXCESS));
            }
            index.extend(std::iter::repeat_n(key, run));
        }
        if index.len() != primitives {
            return Err(refuse(
                "the geometry's runs give keys to fewer primitives than it holds",
            ));
        }
        Ok(Some(Runs {
            table,
            entries,
            runs,
            words,
            index,
        }))
    })
}

/// `index` as maximal runs in primitive order, each an (index, length) pair of u16.
// A run longer than a pair counts is split.
pub(super) fn runs_encoded(index: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    for run in index.chunk_by(|a, b| a == b) {
        for part in run.chunks(LONGEST) {
            out.extend(part[0].to_le_bytes());
            out.extend((part.len() as u16).to_le_bytes());
        }
    }
    out
}

fn word_of(to: usize) -> [u8; 8] {
    (VIRTUAL + to as u64).to_le_bytes()
}

fn primitive_count(v: &[u8], at: usize) -> Result<usize> {
    sections(v, Some(&[]), |b, _| {
        Ok(b.u32("primitive count", at + 0xD4)? as usize)
    })
}

/// `index` written as the runs of the geometry child at `at`, which held `held`: over the array
/// where they fit its 16-byte slot, else laid out past the section's end; the word count with
/// them, and read back.
fn runs_written(s: &mut Rpf9Sections, at: usize, held: &Runs, index: &[u16]) -> Grown<()> {
    let mut bytes = runs_encoded(index);
    let words = bytes.len() / 2;
    // Runs that grow stay only where the bytes up to the next 16 are zero, as measured in every one.
    let (old, end) = (held.runs + 2 * held.words, held.runs + bytes.len());
    let free =
        (s.virtual_section.get(old..end.max(old))).is_some_and(|b| b.iter().all(|x| *x == 0));
    let fits = end <= old || (free && end <= old.next_multiple_of(16));
    let to = match fits {
        true => {
            bytes.resize(bytes.len().max(2 * held.words), 0);
            held.runs
        }
        false => room(s, &[bytes.len()])?[0],
    };
    let v = &mut s.virtual_section;
    put(v, to, &bytes)?;
    put(v, at + 0x148, &word_of(to))?;
    put(v, at + 0x144, &(words as u32).to_le_bytes())?;
    let back = runs_of(&s.virtual_section, at, index.len())?.map(|r| r.index);
    if back.as_deref() != Some(index) {
        return Err(refuse("the geometry's runs as written do not read back").into());
    }
    Ok(())
}

/// The runs of the geometry child at `at` once its primitive `p` is deleted, as +0xD4 states.
// `p`'s key index goes, and the table entry no primitive names then goes with every index past it
// one less, which keeps the table ascending.
pub(super) fn runs_after_delete(s: &mut Rpf9Sections, at: usize, p: usize) -> Grown<()> {
    let now = primitive_count(&s.virtual_section, at)?;
    let Some(held) = runs_of(&s.virtual_section, at, now + 1)? else {
        return Ok(());
    };
    let mut index = held.index.clone();
    if p >= index.len() {
        return Err(HeaderlessError::IndexPastCount {
            what: "primitive to delete",
            index: p as u64,
            count: index.len() as u64,
        }
        .into());
    }
    let gone = index.remove(p);
    if !index.contains(&gone) {
        for i in &mut index {
            if *i > gone {
                *i -= 1;
            }
        }
        let v = &mut s.virtual_section;
        let len = 8 * held.entries;
        let mut table = sections(v, Some(&[]), |b, _| {
            Ok(b.array("key table", held.table, held.entries, 8)?.to_vec())
        })?;
        let from = 8 * usize::from(gone);
        table.drain(from..from + 8);
        table.resize(len, 0);
        put(v, held.table, &table)?;
        put(v, at + 0x140, &((held.entries - 1) as u16).to_le_bytes())?;
    }
    runs_written(s, at, &held, &index)
}

/// The runs of the geometry child at `at` once a primitive is added last, as +0xD4 states.
// It takes the key index of the first triangle it links to across an edge, else the last
// primitive's; no table entry is added, since what an entry means is not known.
pub(super) fn runs_after_add(s: &mut Rpf9Sections, at: usize) -> Grown<()> {
    let now = primitive_count(&s.virtual_section, at)?;
    let n = now
        .checked_sub(1)
        .ok_or(refuse("the geometry holds no primitive added"))?;
    let Some(held) = runs_of(&s.virtual_section, at, n)? else {
        return Ok(());
    };
    let record = sections(&s.virtual_section, Some(&[]), |b, _| {
        let primitives = b.to("primitives", at + 0x88)?;
        Ok(b.get("added primitive", primitives + 16 * n, 16)?.to_vec())
    })?;
    let linked = match record[3] {
        0 => [0xA_usize, 0xC, 0xE]
            .map(|o| u16::from_le_bytes([record[o], record[o + 1]]))
            .into_iter()
            .find(|k| *k != OPEN && usize::from(*k) < n)
            .map(usize::from),
        _ => None,
    };
    let key = (linked.or(n.checked_sub(1)))
        .and_then(|k| held.index.get(k).copied())
        .ok_or(refuse(
            "the added primitive has no primitive before it to take a key from",
        ))?;
    let mut index = held.index.clone();
    index.push(key);
    runs_written(s, at, &held, &index)
}

/// What a geometry child's copy lays out: each array its record names, by the word that names it,
/// and its BVH record with the nodes at the count it allocates.
struct Parts {
    record: Vec<u8>,
    arrays: Vec<(usize, Vec<u8>)>,
    bvh: Option<(Vec<u8>, Vec<u8>)>,
}

fn parts(v: &[u8], at: usize) -> Result<Parts> {
    sections(v, Some(&[]), |b, _| {
        let record = b.get("geometry child", at, BOUND_CHILD)?.to_vec();
        let primitives = b.u32("primitive count", at + 0xD4)? as usize;
        let lists = [
            (0x88, "primitives", primitives, 16),
            (
                0xB0,
                "vertices",
                b.u32("vertex count", at + 0xD0)? as usize,
                6,
            ),
            (
                0xF0,
                "material ids",
                usize::from(b.u8("material count", at + 0x120)?),
                8,
            ),
            (0x118, "per-primitive material indices", primitives, 1),
            (
                0x138,
                "key table",
                usize::from(b.u16("key count", at + 0x140)?),
                8,
            ),
            (
                0x148,
                "key runs",
                b.u32("run word count", at + 0x144)? as usize,
                2,
            ),
        ];
        let mut arrays = Vec::new();
        for (word, what, count, stride) in lists {
            let Some(to) = b.ptr(what, at + word)? else {
                continue;
            };
            let bytes = b.array(what, to, count, stride)?;
            // An empty array names nothing a copy could share.
            if !bytes.is_empty() {
                arrays.push((word, bytes.to_vec()));
            }
        }
        let bvh = match b.ptr("BVH", at + 0x130)? {
            Some(h) => {
                let record = b.get("BVH", h, BVH_RECORD)?.to_vec();
                let count = b.u32("BVH node count", h + 0x58)? as usize;
                let held = count.max(b.u32("BVH allocated nodes", h + 0x5C)? as usize);
                let mut nodes = b
                    .array("BVH nodes", b.to("BVH nodes", h + 0x50)?, count, NODE)?
                    .to_vec();
                let len = (held.checked_mul(NODE))
                    .filter(|l| *l <= b.len())
                    .ok_or(refuse(
                        "the geometry's BVH allocates more nodes than its section holds",
                    ))?;
                nodes.resize(len, 0);
                Some((record, nodes))
            }
            None => None,
        };
        Ok(Parts {
            record,
            arrays,
            bvh,
        })
    })
}

/// Refused where a word of `bytes` other than those at `known` aims at either section.
fn named_only(bytes: &[u8], known: &[usize]) -> Result<()> {
    for o in (0..bytes.len().saturating_sub(7)).step_by(8) {
        let word = u64_at(bytes, o);
        if word >> 32 == 0 && matches!((word as u32) >> 28, 5 | 6) && !known.contains(&o) {
            return Err(refuse(SHARED));
        }
    }
    Ok(())
}

/// The record of a copy of the geometry child at `source`; `None` for a child that is no geometry.
// Every array it names, its BVH record and nodes are laid out anew past the virtual section's end
// and named by it. Its keys are copied as they are: a key may be named by several geometries of a
// body.
pub(super) fn geometry_copied(s: &mut Rpf9Sections, source: usize) -> Grown<Option<Vec<u8>>> {
    let v = &s.virtual_section;
    match sections(v, Some(&[]), |b, _| b.u8("bound type", source + 0x10))? {
        GEOMETRY => {}
        PLAIN_GEOMETRY => return Err(refuse(NOT_COPIED).into()),
        _ => return Ok(None),
    }
    sections(v, Some(&[]), |b, _| child_bound(b, source))?;
    let Parts {
        mut record,
        arrays,
        bvh,
    } = parts(v, source)?;
    named_only(&record, &NAMED)?;
    let mut lens: Vec<usize> = arrays.iter().map(|(_, bytes)| bytes.len()).collect();
    if let Some((r, nodes)) = &bvh {
        named_only(r, &[0x50])?;
        lens.extend([r.len(), nodes.len()]);
    }
    let places = room(s, &lens)?;
    let v = &mut s.virtual_section;
    for ((word, bytes), &to) in arrays.iter().zip(&places) {
        put(v, to, bytes)?;
        put(&mut record, *word, &word_of(to))?;
    }
    if let Some((mut r, nodes)) = bvh {
        let (to, nodes_to) = (places[arrays.len()], places[arrays.len() + 1]);
        put(v, nodes_to, &nodes)?;
        put(&mut r, 0x50, &word_of(nodes_to))?;
        put(v, to, &r)?;
        put(&mut record, 0x130, &word_of(to))?;
    }
    Ok(Some(record))
}

/// A copy of geometry `g`; `None` for one with no BVH, which a body does not copy.
pub(super) fn geometry_value_copied(g: &BoundGeometry) -> Option<ChildBound> {
    g.bvh.as_ref()?;
    Some(ChildBound::Geometry(BoundGeometry {
        box_min: g.box_min,
        box_max: g.box_max,
        vertices: g.vertices.clone(),
        primitives: g.primitives.clone(),
        material_ids: g.material_ids.clone(),
        primitive_materials: g.primitive_materials.clone(),
        bvh: g.bvh,
        unknown_shapes: g.unknown_shapes.clone(),
    }))
}
