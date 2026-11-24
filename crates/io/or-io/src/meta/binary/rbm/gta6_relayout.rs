//! A GTA VI headerless RBM resource laid out anew from a value tree: its data blocks by the table
//! form's walk, the stored block order kept and a new block after it, the regions of a member
//! whose byte 9 sets bit 2 in blocks of their own (physical where bit 5 is set), and every
//! allocation of both sections packed as the measured bodies pack theirs.
//!
//! ⚠ The class, enum, string and id tables are carried as read. New 0x4D and 0x4F text needs a
//! string table entry, whose blobs are not read, so the walk refuses it.

use std::cmp::Reverse;

use crate::meta::binary::layout::{ContainerLayout, RbmLayout};
use crate::meta::binary::pso::table_form;
use crate::meta::binary::pso::table_layout::{free_namespace, lay_out_headerless, Laid};
use crate::meta::error::{MetaError, Result};
use crate::meta::instance::value::Value;
use crate::meta::instance::MetaInstance;

use super::gta6::{described_namespaces, open};

const VIRTUAL: u64 = 0x5000_0000;
const PHYSICAL: u64 = 0x6000_0000;
const HEADER: usize = 0x60;
/// A page is a power of two times this many bytes, and a section is cut into pieces of it.
const PAGE: usize = 0x1_0000;
/// The bytes of a section a pointer's low 28 bits reach.
const SECTION_LIMIT: usize = 1 << 28;
/// How many times a body laid out anew is read and laid out again before it is refused.
const ROUNDS: usize = 3;

fn other(why: impl std::fmt::Display) -> MetaError {
    MetaError::Other(format!("GTA VI RBM: {why}"))
}

/// A headerless resource's sections with `instance` laid out anew, kept once a read lays out alike.
// A hash table is rebuilt in the order its pairs are given, and a read gives them in slot order,
// so a pair whose probe wraps can take another slot when what was read is laid out again.
pub(super) fn write(layout: &RbmLayout, instance: &MetaInstance) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut written = laid_anew(layout, instance)?;
    for round in 0..ROUNDS {
        let (decoded, again) = open(&written.0, &written.1)?;
        let ContainerLayout::Rbm(again) = again.container else {
            return Err(other(
                "the body laid out anew no longer reads as a headerless resource",
            ));
        };
        // A fixed point holds a lost edit as well as a kept one.
        if round == 0 && !same_tree(&decoded, instance) {
            return Err(other(
                "the body laid out anew reads back as another tree, so a value the classes do not place is lost",
            ));
        }
        let next = laid_anew(&again, &decoded)?;
        if next == written {
            return Ok(written);
        }
        written = next;
    }
    Err(other(format!(
        "the body laid out anew, read and laid out again {ROUNDS} times, still moves"
    )))
}

/// Whether `a` and `b` hold the same values, a map's pairs in any order: a read gives a rebuilt
/// table's pairs in slot order.
fn same_tree(a: &MetaInstance, b: &MetaInstance) -> bool {
    a.type_name == b.type_name
        && a.extras == b.extras
        && a.fields.len() == b.fields.len()
        && (a.fields.iter().zip(&b.fields)).all(|(x, y)| x.0 == y.0 && same_value(&x.1, &y.1))
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Struct(x), Value::Struct(y)) => same_tree(x, y),
        (Value::Pointer(Some(x)), Value::Pointer(Some(y))) => same_tree(x, y),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same_value(x, y))
        }
        (Value::Map(x), Value::Map(y)) => {
            // ponytail: quadratic in a map's pairs; a table holds tens of them.
            let mut taken = vec![false; y.len()];
            x.len() == y.len()
                && x.iter().all(|(k, v)| {
                    let found = (y.iter().enumerate())
                        .position(|(i, (l, w))| !taken[i] && same_value(k, l) && same_value(v, w));
                    found.map(|i| taken[i] = true).is_some()
                })
        }
        _ => a == b,
    }
}

fn laid_anew(layout: &RbmLayout, instance: &MetaInstance) -> Result<(Vec<u8>, Vec<u8>)> {
    let model = &layout.model;
    let split = layout.virtual_page_size.min(layout.decompressed.len());
    let (v, p) = layout.decompressed.split_at(split);
    let (_, mut held) = lay_out_headerless(model, &table_form::decode(model)?, None)?;
    let described = described_namespaces(v)?;
    held.extend(
        (0..=u8::MAX)
            .filter(|ns| !described.contains(ns))
            .map(free_namespace),
    );
    let (laid, _) = lay_out_headerless(model, instance, Some(&held))?;
    place(&Tables::read(v, p)?, &laid)
}

fn word(bytes: &[u8], at: usize) -> u64 {
    (bytes.get(at..at + 8))
        .and_then(|b| b.try_into().ok())
        .map_or(0, u64::from_le_bytes)
}

fn quad(bytes: &[u8], at: usize) -> usize {
    (bytes.get(at..at + 4)).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

fn half(bytes: &[u8], at: usize) -> usize {
    (bytes.get(at..at + 2)).map_or(0, |b| usize::from(u16::from_le_bytes([b[0], b[1]])))
}

/// `len` bytes at the resource address `ptr`: 0x5 is the virtual section, 0x6 the physical.
fn at<'b>(v: &'b [u8], p: &'b [u8], ptr: u64, len: usize, what: &str) -> Result<&'b [u8]> {
    let section = match ptr >> 28 {
        5 => v,
        6 => p,
        _ => return Err(other(format!("{what} is at {ptr:#X}, in neither section"))),
    };
    let from = (ptr & 0x0FFF_FFFF) as usize;
    (from.checked_add(len))
        .and_then(|end| section.get(from..end))
        .ok_or_else(|| {
            other(format!(
                "{what} ({len} bytes at {ptr:#X}) runs past its section"
            ))
        })
}

fn records<'b>(
    v: &'b [u8],
    p: &'b [u8],
    ptr: u64,
    count: usize,
    size: usize,
    what: &str,
) -> Result<&'b [u8]> {
    if count == 0 {
        return Ok(&[]);
    }
    let len =
        (count.checked_mul(size)).ok_or_else(|| other(format!("{what} states {count} records")))?;
    at(v, p, ptr, len, what)
}

/// What a body states besides its data blocks and their table, carried as read.
struct Tables<'b> {
    header: &'b [u8],
    classes: &'b [u8],
    /// Each class's member records, in class table order.
    members: Vec<&'b [u8]>,
    enums: &'b [u8],
    /// Each enum's value records, in enum table order.
    values: Vec<&'b [u8]>,
    /// The string table at 0x40 and the table the word at 0x50 points at, where they are stated.
    strings: Option<&'b [u8]>,
    ids: Option<&'b [u8]>,
}

impl<'b> Tables<'b> {
    fn read(v: &'b [u8], p: &'b [u8]) -> Result<Self> {
        let header = v
            .get(..HEADER)
            .ok_or_else(|| other("the virtual section is shorter than its header"))?;
        let classes = records(
            v,
            p,
            word(header, 0x28),
            half(header, 0x48),
            32,
            "the class table",
        )?;
        let members = (classes.as_chunks::<32>().0.iter())
            .map(|c| records(v, p, word(c, 16), half(c, 30), 16, "a class's members"))
            .collect::<Result<Vec<_>>>()?;
        let enums = records(
            v,
            p,
            word(header, 0x30),
            half(header, 0x4A),
            24,
            "the enum table",
        )?;
        let values = (enums.as_chunks::<24>().0.iter())
            .map(|e| records(v, p, word(e, 8), quad(e, 0x10), 16, "an enum's values"))
            .collect::<Result<Vec<_>>>()?;
        // `{u32 count; count × {u8 namespace, u8 kind, u16, u32 blob length}; blobs}`.
        let strings = match word(header, 0x40) {
            0 => None,
            ptr => {
                let count = quad(at(v, p, ptr, 4, "the string table")?, 0);
                let entries = records(
                    v,
                    p,
                    ptr.wrapping_add(4),
                    count,
                    8,
                    "the string table's entries",
                )?;
                let blobs: u64 = entries
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|e| quad(e, 4) as u64)
                    .sum();
                let len = (usize::try_from(blobs).ok())
                    .and_then(|blobs| blobs.checked_add(4 + entries.len()))
                    .ok_or_else(|| other("the string table's blobs pass the address space"))?;
                Some(at(v, p, ptr, len, "the string table")?)
            }
        };
        // `{u32 count; count × 16 bytes}`.
        let ids = match word(header, 0x50) {
            0 => None,
            ptr => {
                let count = quad(at(v, p, ptr, 4, "the id table")?, 0);
                let len = (count.checked_mul(16))
                    .and_then(|n| n.checked_add(4))
                    .ok_or_else(|| other(format!("the id table states {count} entries")))?;
                Some(at(v, p, ptr, len, "the id table")?)
            }
        };
        Ok(Self {
            header,
            classes,
            members,
            enums,
            values,
            strings,
            ids,
        })
    }
}

/// What an allocation holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Block(usize),
    BlockTable,
    BlockMap,
    EnumTable,
    Values(usize),
    ClassTable,
    Members(usize),
    Strings,
    Ids,
}

/// An allocation: what it holds, the room it takes, where it may start, and the alignment that
/// orders it among allocations of the same room.
#[derive(Debug, Clone, Copy)]
struct Alloc {
    part: Part,
    room: usize,
    align: usize,
    rank: usize,
}

impl Alloc {
    /// A data block takes its size to 16 bytes.
    fn block(i: usize, len: usize) -> Self {
        Self {
            part: Part::Block(i),
            room: len.next_multiple_of(16),
            align: 16,
            rank: 16,
        }
    }

    fn table(part: Part, room: usize, align: usize) -> Self {
        Self {
            part,
            room,
            align,
            rank: 0,
        }
    }
}

fn is_physical(laid: &Laid, i: usize) -> bool {
    laid.physical.get(i).copied().unwrap_or(false)
}

/// The virtual section's allocations after the header, in the order they are placed: the data
/// blocks, the block table and the block map by room to 16 bytes, largest first, then the enum,
/// class, string and id tables the same way, each list's ties in the order it is given.
fn virtual_list(t: &Tables, laid: &Laid, map_len: usize) -> Vec<Alloc> {
    let mut a: Vec<Alloc> = (0..laid.blocks.len())
        .filter(|&i| !is_physical(laid, i))
        .map(|i| Alloc::block(i, laid.blocks[i].1.len()))
        .collect();
    a.push(Alloc {
        part: Part::BlockTable,
        room: 16 * laid.blocks.len(),
        align: 8,
        rank: 8,
    });
    // ⚠ The loose form's writer finds the block map only at a 16-byte boundary.
    a.push(Alloc {
        part: Part::BlockMap,
        room: map_len,
        align: 16,
        rank: 8,
    });
    a.sort_by_key(|x| (Reverse(x.room.next_multiple_of(16)), Reverse(x.rank)));
    let mut b = vec![Alloc::table(Part::EnumTable, t.enums.len(), 8)];
    b.extend((t.values.iter().enumerate()).map(|(e, x)| Alloc::table(Part::Values(e), x.len(), 8)));
    b.push(Alloc::table(Part::ClassTable, t.classes.len(), 8));
    b.extend(
        (t.members.iter().enumerate()).map(|(c, x)| Alloc::table(Part::Members(c), x.len(), 1)),
    );
    b.extend(t.strings.map(|x| Alloc::table(Part::Strings, x.len(), 1)));
    b.extend(t.ids.map(|x| Alloc::table(Part::Ids, x.len(), 1)));
    b.sort_by_key(|x| Reverse(x.room.next_multiple_of(16)));
    a.extend(b);
    a
}

/// Where each allocation starts, and the section's length.
#[derive(Debug, PartialEq, Eq)]
struct Packed {
    at: Vec<usize>,
    len: usize,
}

/// The page an allocation of `len` bytes opens: the least power of two times 64 KiB that holds it.
fn page_for(len: usize) -> usize {
    (len.max(1).div_ceil(PAGE).next_power_of_two()).saturating_mul(PAGE)
}

/// `allocs` packed first fit into pages, the first holding `head` bytes before them: each goes to
/// the first page whose cursor leaves it room, else opens the next page; the section ends at the
/// last page's cursor, to 16 bytes.
fn pack(head: usize, allocs: &[Alloc]) -> Packed {
    // `[start, size, cursor]`.
    let mut pages: Vec<[usize; 3]> = Vec::new();
    let mut at = Vec::with_capacity(allocs.len());
    for a in allocs {
        let align = a.align.max(1);
        let fit = pages.iter_mut().find_map(|[start, size, cursor]| {
            let from = (*cursor).next_multiple_of(align);
            (from.saturating_add(a.room) <= *size).then(|| {
                *cursor = from + a.room;
                *start + from
            })
        });
        let placed = match fit {
            Some(placed) => placed,
            None => {
                let (start, used) = match pages.last() {
                    Some([start, size, _]) => (start + size, 0),
                    None => (0, head),
                };
                pages.push([start, page_for(used + a.room), used + a.room]);
                start + used
            }
        };
        at.push(placed);
    }
    let end = pages
        .last()
        .map_or(head, |[start, _, cursor]| start + cursor);
    Packed {
        at,
        len: end.next_multiple_of(16),
    }
}

/// How many pieces the loose form cuts a section of `len` bytes into, as its block map counts them.
fn pieces(len: usize) -> usize {
    usize::from(len >= PAGE) + usize::from(!len.is_multiple_of(PAGE))
}

/// Both sections of `laid` with the tables of `t`, the block map's size settled against the
/// virtual section's length.
fn place(t: &Tables, laid: &Laid) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut physical: Vec<Alloc> = (0..laid.blocks.len())
        .filter(|&i| is_physical(laid, i))
        .map(|i| Alloc::block(i, laid.blocks[i].1.len()))
        .collect();
    physical.sort_by_key(|x| Reverse(x.room));
    let in_physical = pack(0, &physical);
    let pc = pieces(in_physical.len);
    let mut vc = 1;
    for _ in 0..4 {
        let in_order = virtual_list(t, laid, 16 + 8 * (vc + pc));
        let in_virtual = pack(HEADER, &in_order);
        if pieces(in_virtual.len) == vc {
            let counts = [vc as u8, pc as u8];
            return build(
                t,
                laid,
                (in_order.as_slice(), &in_virtual),
                (physical.as_slice(), &in_physical),
                counts,
            );
        }
        vc = pieces(in_virtual.len);
    }
    Err(other(
        "the block map's size and the virtual section's length do not settle",
    ))
}

fn put(section: &mut [u8], at: usize, bytes: &[u8]) -> Result<()> {
    let held = section.len();
    (at.checked_add(bytes.len()))
        .and_then(|end| section.get_mut(at..end))
        .ok_or_else(|| {
            other(format!(
                "{} bytes at {at:#X} pass the {held}-byte section",
                bytes.len()
            ))
        })?
        .copy_from_slice(bytes);
    Ok(())
}

/// The sections written: every allocation's bytes where it was placed, and every pointer to one.
fn build(
    t: &Tables,
    laid: &Laid,
    (virt, vp): (&[Alloc], &Packed),
    (phys, pp): (&[Alloc], &Packed),
    counts: [u8; 2],
) -> Result<(Vec<u8>, Vec<u8>)> {
    for (section, len) in [("virtual", vp.len), ("physical", pp.len)] {
        if len > SECTION_LIMIT {
            return Err(other(format!(
                "the {section} section laid out anew is {len} bytes, past the {SECTION_LIMIT} a pointer reaches"
            )));
        }
    }
    let (mut v, mut p) = (vec![0u8; vp.len], vec![0u8; pp.len]);
    let mut addr = vec![0u64; laid.blocks.len()];
    for (a, &at) in phys.iter().zip(&pp.at) {
        if let Part::Block(i) = a.part {
            addr[i] = PHYSICAL + at as u64;
            put(&mut p, at, &laid.blocks[i].1)?;
        }
    }
    put(&mut v, 0, t.header)?;
    let mut values_at = vec![0usize; t.values.len()];
    let mut members_at = vec![0usize; t.members.len()];
    let (mut table_at, mut map_at, mut enums_at) = (0, 0, 0);
    let (mut classes_at, mut strings_at, mut ids_at) = (0, 0, 0);
    for (a, &at) in virt.iter().zip(&vp.at) {
        let bytes: &[u8] = match a.part {
            Part::Block(i) => {
                addr[i] = VIRTUAL + at as u64;
                laid.blocks[i].1.as_slice()
            }
            Part::BlockTable => {
                table_at = at;
                &[]
            }
            Part::BlockMap => {
                map_at = at;
                &[]
            }
            Part::EnumTable => {
                enums_at = at;
                t.enums
            }
            Part::Values(e) => {
                values_at[e] = at;
                t.values[e]
            }
            Part::ClassTable => {
                classes_at = at;
                t.classes
            }
            Part::Members(c) => {
                members_at[c] = at;
                t.members[c]
            }
            Part::Strings => {
                strings_at = at;
                t.strings.unwrap_or_default()
            }
            Part::Ids => {
                ids_at = at;
                t.ids.unwrap_or_default()
            }
        };
        put(&mut v, at, bytes)?;
    }
    let pointer = |at: usize| (VIRTUAL + at as u64).to_le_bytes();
    // A pointer the body stated as null stays null.
    for (c, &at) in members_at.iter().enumerate() {
        if word(t.classes, 32 * c + 16) != 0 {
            put(&mut v, classes_at + 32 * c + 16, &pointer(at))?;
        }
    }
    for (e, &at) in values_at.iter().enumerate() {
        if word(t.enums, 24 * e + 8) != 0 {
            put(&mut v, enums_at + 24 * e + 8, &pointer(at))?;
        }
    }
    for (i, ((ty, data), a)) in laid.blocks.iter().zip(&addr).enumerate() {
        let record = table_at + 16 * i;
        put(&mut v, record, &ty.to_le_bytes())?;
        put(&mut v, record + 4, &(data.len() as u32).to_le_bytes())?;
        put(&mut v, record + 8, &a.to_le_bytes())?;
    }
    put(&mut v, map_at + 8, &counts)?;
    for (field, at) in [
        (0x08, map_at),
        (0x28, classes_at),
        (0x30, enums_at),
        (0x38, table_at),
        (0x40, strings_at),
        (0x50, ids_at),
    ] {
        if word(t.header, field) != 0 {
            put(&mut v, field, &pointer(at))?;
        }
    }
    put(&mut v, 0x20, &laid.root.to_le_bytes())?;
    put(&mut v, 0x4C, &(laid.blocks.len() as u16).to_le_bytes())?;
    Ok((v, p))
}

#[cfg(test)]
mod tests {
    use super::super::gta6::read_model;
    use super::super::gta6_grow::tests::{body, member, pointer};
    use super::super::gta6_grow_tree::write_grown;
    use super::*;
    use crate::container::rpf::{Rpf9Form, Rpf9Sections};
    use crate::meta::binary::hash_index::default_hash_index;
    use crate::meta::binary::layout::MetaLayout;
    use crate::meta::instance::value::Value;

    const ROOT: u32 = 0x4747_0001;
    const ELEMENT: u32 = 0x4747_0002;
    const OTHER: u32 = 0x4747_0003;
    const FIRST: u32 = 0x4747_0010;
    const ARRAY_INFO: u32 = 0x100;
    /// The type of a block of pointers.
    const POINTERS: u32 = 7;

    fn name(hash: u32) -> String {
        default_hash_index().name_or_literal(hash)
    }

    fn element(i: i32, f: f32) -> Vec<u8> {
        [i.to_le_bytes(), f.to_le_bytes()].concat()
    }

    fn pair(i: i64, f: f64) -> MetaInstance {
        let mut e = MetaInstance::new(name(ELEMENT));
        e.fields.push((name(0x4747_0020), Value::Int(i)));
        e.fields.push((name(0x4747_0021), Value::Float(f)));
        e
    }

    fn pair_class() -> (u32, u32, Vec<[u8; 16]>) {
        let members = vec![
            member(0x4747_0020, 0, 0x14, 0, 0),
            member(0x4747_0021, 4, 0x21, 0, 0),
        ];
        (ELEMENT, 8, members)
    }

    /// A root whose one member is an array of structures of class `element`, of pointers where it
    /// is 0, its head `head`, and the blocks after the root's.
    fn rooted(element: u32, head: Vec<u8>, blocks: Vec<(u32, Vec<u8>)>) -> Rpf9Sections {
        let code = if element == 0 { 0x07 } else { 0x05 };
        let root = (
            ROOT,
            0x10,
            vec![
                member(FIRST, 0x00, 0x52, 1, 0),
                member(ARRAY_INFO, 0, code, 0, element),
            ],
        );
        let other = (OTHER, 4, vec![member(0x4747_0030, 0, 0x15, 0, 0)]);
        let all = [vec![(ROOT, head)], blocks].concat();
        body(&[root, pair_class(), other], &all, 1)
    }

    fn opened(v: &[u8], p: &[u8]) -> (MetaInstance, RbmLayout) {
        let (instance, MetaLayout { container, .. }) = open(v, p).expect("it opens");
        let ContainerLayout::Rbm(layout) = container else {
            panic!("a headerless RBM layout");
        };
        (instance, *layout)
    }

    fn items(tree: &mut MetaInstance) -> &mut Vec<Value> {
        match tree.get_mut(&name(FIRST)) {
            Some(Value::Array(items)) => items,
            _ => panic!("an array"),
        }
    }

    /// Opens `written`, checks that a save of what it reads gives it back, and returns that tree.
    fn read_back(written: &(Vec<u8>, Vec<u8>)) -> MetaInstance {
        let (tree, layout) = opened(&written.0, &written.1);
        assert_eq!(
            &write_grown(&layout, &tree).expect("saved again"),
            written,
            "an unchanged save of the body laid out anew"
        );
        let loose = crate::container::rpf::write_rpf9_resource(&Rpf9Sections {
            form: Rpf9Form::BlockMapped,
            virtual_section: written.0.clone(),
            physical: written.1.clone(),
        });
        assert!(loose.is_ok(), "{loose:?}");
        tree
    }

    #[test]
    fn an_empty_array_behind_a_null_pointer_takes_its_first_element_in_a_new_block() {
        let s = rooted(ELEMENT, vec![0; 0x10], Vec::new());
        let (mut tree, layout) = opened(&s.virtual_section, &s.physical);
        assert_eq!(*items(&mut tree), Vec::<Value>::new());
        items(&mut tree).push(Value::Struct(Box::new(pair(5, 2.0))));
        let written = write_grown(&layout, &tree).expect("laid out anew");
        let mut again = read_back(&written);
        assert_eq!(
            *items(&mut again),
            vec![Value::Struct(Box::new(pair(5, 2.0)))]
        );
        let model = read_model(&written.0, &written.1).expect("it reads");
        let types: Vec<u32> = model.data_blocks.iter().map(|b| b.data_type).collect();
        assert_eq!(types, [ROOT, ELEMENT]);
        assert_eq!(model.data_blocks[0].data[..8], pointer(2, 0).to_le_bytes());
        assert_eq!(model.data_blocks[0].data[8..12], [1, 0, 1, 0]);
    }

    #[test]
    fn a_value_under_a_name_the_class_does_not_declare_is_refused_rather_than_dropped() {
        let s = rooted(ELEMENT, vec![0; 0x10], Vec::new());
        let (mut tree, layout) = opened(&s.virtual_section, &s.physical);
        let mut extra = pair(5, 2.0);
        extra.fields.push((name(0x4747_0099), Value::Int(7)));
        items(&mut tree).push(Value::Struct(Box::new(extra)));
        assert!(write(&layout, &tree).is_err());
        assert!(write_grown(&layout, &tree).is_err());
    }

    #[test]
    fn an_added_pointer_whose_class_no_stored_element_has_opens_a_block_of_that_class() {
        let mut head = vec![0u8; 0x10];
        head[..8].copy_from_slice(&pointer(2, 0).to_le_bytes());
        head[8..12].copy_from_slice(&[2, 0, 2, 0]);
        let pointers = [pointer(3, 0), pointer(3, 8)]
            .map(u64::to_le_bytes)
            .concat();
        let records = [element(10, 0.25), element(-20, 0.5)].concat();
        let s = rooted(0, head, vec![(POINTERS, pointers), (ELEMENT, records)]);
        let (mut tree, layout) = opened(&s.virtual_section, &s.physical);
        let mut added = MetaInstance::new(name(OTHER));
        added.fields.push((name(0x4747_0030), Value::UInt(9)));
        items(&mut tree).push(Value::Pointer(Some(Box::new(added.clone()))));
        let written = write_grown(&layout, &tree).expect("laid out anew");
        let mut again = read_back(&written);
        let want = vec![
            Value::Pointer(Some(Box::new(pair(10, 0.25)))),
            Value::Pointer(Some(Box::new(pair(-20, 0.5)))),
            Value::Pointer(Some(Box::new(added))),
        ];
        assert_eq!(*items(&mut again), want);
        let model = read_model(&written.0, &written.1).expect("it reads");
        let types: Vec<u32> = model.data_blocks.iter().map(|b| b.data_type).collect();
        assert_eq!(types, [ROOT, POINTERS, ELEMENT, OTHER]);
    }

    #[test]
    fn a_member_whose_byte_9_sets_bits_2_and_5_has_its_regions_in_the_physical_section() {
        let mut head = vec![0u8; 0x10];
        head[..8].copy_from_slice(&pointer(2, 0).to_le_bytes());
        head[8..12].copy_from_slice(&[2, 0, 2, 0]);
        let records = [element(10, 0.25), element(-20, 0.5)].concat();
        let mut s = rooted(ELEMENT, head, vec![(ELEMENT, records)]);
        // The root's first member record follows the three class records.
        let at = 0x60 + 32 * 3 + 9;
        s.virtual_section[at] = 0x24;
        let (tree, layout) = opened(&s.virtual_section, &s.physical);
        let written = write(&layout, &tree).expect("laid out anew");
        let model = read_model(&written.0, &written.1).expect("it reads");
        assert_eq!(model.data_blocks[0].data_addr >> 28, 5);
        assert_eq!(model.data_blocks[1].data_addr, PHYSICAL);
        assert_eq!(written.1.len(), 16);
        let facts = super::super::gta6::header_facts(&written.0).expect("it reads");
        assert_eq!((facts.virtual_blocks, facts.physical_blocks), (1, 1));
        assert_eq!(read_back(&written), tree);
    }

    #[test]
    fn allocations_pack_first_fit_into_pages_of_a_power_of_two_times_64_kib() {
        let alloc = |room, align| Alloc::table(Part::Strings, room, align);
        // The first opens page 0 after the header; the second does not fit its rest and opens
        // the next page; the third goes back to page 0's cursor.
        let rooms = [alloc(0x8000, 16), alloc(0x9000, 16), alloc(0x100, 8)];
        let packed = pack(HEADER, &rooms);
        let first_end = HEADER + 0x8000;
        assert_eq!(packed.at, [HEADER, PAGE, first_end]);
        assert_eq!(packed.len, PAGE + 0x9000);
        // One larger than a page opens a page of the next power of two, and a small one follows it.
        let packed = pack(0, &[alloc(3 * PAGE, 16), alloc(0x10, 16)]);
        assert_eq!(packed.at, [0, 3 * PAGE]);
        assert_eq!(packed.len, 3 * PAGE + 0x10);
        assert_eq!(page_for(3 * PAGE), 4 * PAGE);
        // An allocation that does not fit what is left starts the next page, whatever gap that leaves.
        let packed = pack(0, &[alloc(PAGE - 8, 8), alloc(16, 8)]);
        assert_eq!(packed.at, [0, PAGE]);
        assert_eq!(
            [pieces(0), pieces(0x40), pieces(PAGE), pieces(PAGE + 1)],
            [0, 1, 1, 2]
        );
    }
}
