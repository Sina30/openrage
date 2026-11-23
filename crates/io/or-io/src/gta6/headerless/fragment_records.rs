//! The records a fragment's root may point at besides its physics level: vehicle glass at 0xF0,
//! glass panes at 0x98, group pairs at 0x58 and 0x50, a lattice of cells at 0xA0 with its vehicle
//! record at 0xE8; and its wheels, which are groups.

use super::bytes::{f32_at, u32_at, u64_at, Bytes};
use super::fragment::{apply, bind_worlds, Fragment, FragmentGroup, M3};
use super::skeleton::Bone;
use super::{HeaderlessError, Result};

/// A window's header, before its shattermap.
const WINDOW: usize = 0x70;
/// The part of a glass pane record every measured one holds.
const PANE: usize = 0x1A0;
/// A lattice record up to the end of its last array slot.
const LATTICE: usize = 0x138;
/// A shattermap row's second start where it has no second run, and a row of no run at all.
const NO_RUN: u8 = 0xFF;
/// No cell across a face.
const NONE: u16 = 0xFFFF;
/// The first word of each unused entry of the group links at 0x50.
const UNUSED_LINK: f32 = 5.434_822;

/// A vehicle's breakable glass: the `HWGV` block and its windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VehicleGlass {
    pub stride: u16,
    /// The word between the window table and the first window.
    pub after_offsets: u32,
    pub windows: Vec<GlassWindow>,
}

/// One window: its header, carried whole, and its shattermap's rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlassWindow {
    /// Only the item id at +0x44 and the map's counts at +0x48..+0x4D are known.
    pub header: [u8; WINDOW],
    pub rows: Vec<GlassRow>,
}

/// One row of a shattermap as the file spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlassRow {
    /// The one byte 0xFF: no run.
    Blank,
    /// No run, spelled as a last column before the first.
    Empty { start: u8 },
    /// One run of health bytes, or two, each at its first column; a `start2` of 0xFF is none.
    Runs {
        start1: u8,
        data1: Vec<u8>,
        start2: u8,
        data2: Vec<u8>,
    },
    /// A row of a map wider than 256 columns, whose u8 columns wrap, kept as its bytes.
    Raw(Vec<u8>),
}

/// A glass pane: GTA V's `bgPaneModelInfoBase`, its outlines' counts and the group it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct GlassPane {
    /// A corner and the two edges from it, in the owner group's bone frame.
    pub base: [f32; 3],
    pub width: [f32; 3],
    pub height: [f32; 3],
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    /// Each outline's vertex count; where the vertices are stored is not known.
    pub outlines: Vec<u32>,
    /// `None` where several outlines' data may reach over the word that names it.
    pub owner: Option<u16>,
    /// The four corners in the drawable's frame, the owner bone's bind world applied.
    pub corners: Option<[[f32; 3]; 4]>,
}

/// The group pairs at 0x50, each with a value.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupLinks {
    pub links: Vec<GroupLink>,
    /// The entries the array has room for, those past the links holding the unused fill.
    pub capacity: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroupLink {
    pub groups: [u16; 2],
    /// 1/128 in most; what it means is not known.
    pub value: f32,
}

/// The lattice of hexahedral cells at 0xA0; its other arrays are not laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct Lattice {
    pub vertices: Vec<[f32; 3]>,
    pub cells: Vec<LatticeCell>,
    /// Whether the vehicle record at 0xE8 is set, whose head restates the lattice's counts.
    pub vehicle: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatticeCell {
    /// Eight distinct vertices.
    pub corners: [u16; 8],
    /// The cell across each face, faces paired 0/1, 2/3, 4/5, each pair's axis not known.
    pub neighbours: [Option<u16>; 6],
}

/// A group on a wheel bone; its geometry is the drawable's, skinned to that bone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wheel {
    pub group: u16,
    /// Its first child the composite lists, whose bound is the wheel's collider.
    pub child: u16,
}

fn refuse<T>(why: &'static str) -> Result<T> {
    Err(HeaderlessError::Fragment(why))
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

/// Each record the root of `v` points at, read against `f`'s groups and children and `bones`;
/// only a wheel group with no child refuses the fragment.
pub(super) fn read_records(v: Bytes, f: &mut Fragment, bones: &[Bone]) -> Result<()> {
    let worlds = bind_worlds(bones);
    let (groups, children) = (f.groups.len(), f.children.len());
    let slot = |at: usize| v.ptr("root record", at);
    f.vehicle_glass = slot(0xF0)?.map(|at| vehicle_glass(v, at, children));
    f.panes = slot(0x98)?.map(|at| panes(v, at, &f.groups, &worlds));
    f.self_collision = slot(0x58)?.map(|at| self_collision(v, at, groups));
    f.group_links = slot(0x50)?.map(|at| group_links(v, at, groups));
    let vehicle = slot(0xE8)?;
    f.lattice = match slot(0xA0)? {
        Some(at) => Some(lattice(v, at, bones.len(), vehicle)),
        None => vehicle.map(|_| refuse("it has a vehicle record at 0xE8 and no lattice")),
    };
    f.wheels = wheels(&f.groups, bones)?;
    Ok(())
}

impl GlassWindow {
    /// The fragment child the window is, by index.
    pub fn child(&self) -> u16 {
        le16(&self.header, 0x44)
    }

    pub fn width(&self) -> u16 {
        le16(&self.header, 0x48)
    }

    /// The map's length in bytes, its row table included; 0 for no map.
    pub fn map_len(&self) -> u16 {
        le16(&self.header, 0x4C)
    }
}

impl GlassRow {
    /// The row at `at` of a map no wider than 256 columns, and its length; `None` past `b`.
    fn read(b: &[u8], at: usize) -> Option<(Self, usize)> {
        let byte = |i: usize| b.get(at + i).copied();
        let run = |from: usize, len: usize| Some(b.get(at + from..at + from + len)?.to_vec());
        let start1 = byte(0)?;
        if start1 == NO_RUN {
            return Some((Self::Blank, 1));
        }
        let end1 = byte(1)?;
        if end1 < start1 {
            return Some((Self::Empty { start: start1 }, 2));
        }
        let n1 = usize::from(end1 - start1) + 1;
        let data1 = run(2, n1)?;
        let mut len = 2 + n1;
        let start2 = byte(len)?;
        len += 1;
        let mut data2 = Vec::new();
        if start2 != NO_RUN {
            let end2 = byte(len)?;
            len += 1;
            if end2 >= start2 {
                let n2 = usize::from(end2 - start2) + 1;
                data2 = run(len, n2)?;
                len += n2;
            }
        }
        let row = Self::Runs {
            start1,
            data1,
            start2,
            data2,
        };
        Some((row, len))
    }

    fn write(&self, out: &mut Vec<u8>) {
        let last = |start: u8, len: usize| start.wrapping_add((len as u8).wrapping_sub(1));
        match self {
            Self::Blank => out.push(NO_RUN),
            Self::Empty { start } => {
                let start = (*start).max(1);
                out.extend([start, start - 1]);
            }
            Self::Runs {
                start1,
                data1,
                start2,
                data2,
            } => {
                out.extend([*start1, last(*start1, data1.len())]);
                out.extend_from_slice(data1);
                out.push(*start2);
                if *start2 != NO_RUN {
                    out.push(last(*start2, data2.len()));
                    out.extend_from_slice(data2);
                }
            }
            Self::Raw(bytes) => out.extend_from_slice(bytes),
        }
    }
}

impl VehicleGlass {
    /// The block as the file holds it, its table, lengths, row offsets and padding recomputed.
    pub fn encode(&self) -> Vec<u8> {
        let count = self.windows.len();
        let slots = count + (count & 1);
        let prologue = 12 + 8 * slots + 4;
        let (mut table, mut body) = (Vec::with_capacity(8 * slots), Vec::new());
        for w in &self.windows {
            table.extend_from_slice(&u32::from(w.child()).to_le_bytes());
            table.extend_from_slice(&((prologue + body.len()) as u32).to_le_bytes());
            body.extend_from_slice(&w.header);
            if w.map_len() != 0 {
                let mut rows = Vec::new();
                for row in &w.rows {
                    body.extend_from_slice(&(rows.len() as u16).to_le_bytes());
                    row.write(&mut rows);
                }
                body.extend_from_slice(&rows);
            }
            let padded = (prologue + body.len()).next_multiple_of(16);
            body.resize(padded - prologue, 0);
        }
        table.resize(8 * slots, 0);
        let mut out = Vec::with_capacity(prologue + body.len());
        out.extend_from_slice(b"HWGV");
        out.extend_from_slice(&self.stride.to_le_bytes());
        out.extend_from_slice(&(count as u16).to_le_bytes());
        out.extend_from_slice(&((prologue + body.len()) as u32).to_le_bytes());
        out.extend_from_slice(&table);
        out.extend_from_slice(&self.after_offsets.to_le_bytes());
        out.extend_from_slice(&body);
        out
    }
}

/// A map's rows and where it ends.
type Rows = (Vec<GlassRow>, usize);

/// `len` bytes of the vehicle glass block `b` from `from`.
fn take(b: &[u8], from: usize, len: usize) -> Result<&[u8]> {
    let end = from.checked_add(len);
    match end.and_then(|end| b.get(from..end)) {
        Some(bytes) => Ok(bytes),
        None => refuse("its vehicle glass block ends before a window or row it states"),
    }
}

/// The rows of a window's map of `width` columns and `map` bytes at `c` in `b`, the table of
/// `count` offsets first, and where the map ends.
fn map_rows(b: &[u8], c: usize, width: u16, count: usize, map: usize) -> Result<Rows> {
    let offsets: Vec<usize> = (take(b, c, 2 * count)?.as_chunks::<2>().0.iter())
        .map(|p| usize::from(u16::from_le_bytes(*p)))
        .collect();
    let (base, mut c) = (c + 2 * count, c + 2 * count);
    let mut rows = Vec::with_capacity(count);
    if width <= 256 {
        for from in offsets {
            if from != c - base {
                return refuse("a shattermap row is not where the rows before it end");
            }
            let Some((row, len)) = GlassRow::read(b, c) else {
                return refuse("a shattermap row reaches past its vehicle glass block");
            };
            rows.push(row);
            c += len;
        }
        return Ok((rows, c));
    }
    // A wider map's u8 columns wrap, so the row table alone delimits its rows.
    let Some(end) = map.checked_sub(2 * count) else {
        return refuse("a vehicle glass window's map is shorter than its row table");
    };
    if offsets.first().is_some_and(|o| *o != 0) {
        return refuse("a wide vehicle glass window's first row is not at its start");
    }
    for (i, from) in offsets.iter().enumerate() {
        let to = offsets.get(i + 1).copied().unwrap_or(end);
        if *from >= to {
            return refuse("a wide vehicle glass window's row offsets do not ascend");
        }
        rows.push(GlassRow::Raw(take(b, base + from, to - from)?.to_vec()));
    }
    Ok((rows, if count == 0 { c } else { base + end }))
}

/// The vehicle glass block at `at`, its windows each a child below `children`.
fn vehicle_glass(v: Bytes, at: usize, children: usize) -> Result<VehicleGlass> {
    let head = v.get("vehicle glass", at, 12)?;
    let (stride, count) = (le16(head, 4), usize::from(le16(head, 6)));
    let b = v.array("vehicle glass", at, u32_at(head, 8) as usize, 1)?;
    let slots = count + (count & 1);
    let after_offsets = u32_at(take(b, 12 + 8 * slots, 4)?, 0);
    let mut c = 12 + 8 * slots + 4;
    let mut windows = Vec::with_capacity(count.min(b.len() / WINDOW));
    for _ in 0..count {
        let mut header = [0u8; WINDOW];
        header.copy_from_slice(take(b, c, WINDOW)?);
        if header[0x40..0x44] != *b"CWGV" {
            return refuse("a vehicle glass window does not hold CWGV at +0x40");
        }
        c += WINDOW;
        let mut window = GlassWindow {
            header,
            rows: Vec::new(),
        };
        let map = usize::from(window.map_len());
        if map != 0 {
            let start = c;
            let rows = usize::from(le16(&header, 0x4A));
            (window.rows, c) = map_rows(b, c, window.width(), rows, map)?;
            if c - start != map {
                return refuse("a vehicle glass window's map is not the length it states");
            }
        }
        c = c.next_multiple_of(16);
        windows.push(window);
    }
    if c != b.len() {
        return refuse("its vehicle glass windows do not end where the block states");
    }
    let glass = VehicleGlass {
        stride,
        after_offsets,
        windows,
    };
    if glass.encode().as_slice() != b {
        return refuse("its vehicle glass block is not what its windows encode to");
    }
    let ids: Vec<u16> = glass.windows.iter().map(GlassWindow::child).collect();
    let past = ids.iter().any(|i| usize::from(*i) >= children);
    if past || ids.windows(2).any(|w| w[0] >= w[1]) {
        return refuse("its vehicle glass windows' children do not ascend below its count");
    }
    Ok(glass)
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| a[k] + b[k])
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| a[k] * b[k]).sum()
}

/// The glass panes the list at `list` points at, each on a group of `groups`.
fn panes(
    v: Bytes,
    list: usize,
    groups: &[FragmentGroup],
    worlds: &[(M3, [f32; 3])],
) -> Result<Vec<GlassPane>> {
    let count = usize::from(v.u8("glass pane count", 0xDA)?);
    if count == 0 {
        return refuse("it points at glass panes and counts none");
    }
    v.array("glass panes", list, count, 8)?;
    let mut out = Vec::with_capacity(count);
    for k in 0..count {
        let r = v.get("glass pane", v.to("glass pane", list + 8 * k)?, PANE)?;
        let vec3 = |o: usize| -> [f32; 3] { std::array::from_fn(|a| f32_at(r, o + 4 * a)) };
        let (base, width, height) = (vec3(0), vec3(0x10), vec3(0x20));
        let (w, h) = (dot(width, width).sqrt(), dot(height, height).sqrt());
        let square = w > 1e-5 && h > 1e-5 && dot(width, height).abs() < 1e-2 * w * h;
        if !square {
            return refuse("a glass pane's width and height are not perpendicular edges");
        }
        let lines = u32_at(r, 0x40) as usize;
        if !(1..=15).contains(&lines) {
            return refuse("a glass pane does not have 1 to 15 outlines");
        }
        let outlines: Vec<u32> = (0..lines).map(|j| u32_at(r, 0x44 + 4 * j)).collect();
        if outlines.iter().any(|n| !(3..=64).contains(n)) {
            return refuse("a glass pane's outline has fewer than 3 or over 64 vertices");
        }
        let owner = match u16::try_from(u32_at(r, 0x170)) {
            Ok(g) if usize::from(g) < groups.len() => Some(g),
            _ if lines == 1 => return refuse("a glass pane's owner is not a group of it"),
            _ => None,
        };
        let bone = owner.map(|g| usize::from(groups[usize::from(g)].bone));
        let corners = bone.and_then(|b| worlds.get(b)).map(|(rotation, t)| {
            let far = add(add(base, width), height);
            let local = [base, add(base, width), far, add(base, height)];
            local.map(|c| add(apply(rotation, c), *t))
        });
        out.push(GlassPane {
            base,
            width,
            height,
            uv_min: [f32_at(r, 0x30), f32_at(r, 0x34)],
            uv_max: [f32_at(r, 0x38), f32_at(r, 0x3C)],
            outlines,
            owner,
            corners,
        });
    }
    Ok(out)
}

/// The pairs at 0x58: a count, a word, and the pairs of u16 group indices, none repeated.
fn self_collision(v: Bytes, at: usize, groups: usize) -> Result<Vec<[u16; 2]>> {
    let count = v.u32("self-collision pairs", at)? as usize;
    let pairs_at = v.to("self-collision pairs", at + 8)?;
    if count == 0 {
        return refuse("its self-collision record points at pairs and counts none");
    }
    let bytes = v.array("self-collision pairs", pairs_at, count, 4)?;
    let pairs: Vec<[u16; 2]> = (bytes.as_chunks::<4>().0.iter())
        .map(|p| [le16(p, 0), le16(p, 2)])
        .collect();
    let mut distinct = pairs.clone();
    distinct.sort_unstable();
    distinct.dedup();
    let past = pairs.iter().flatten().any(|g| usize::from(*g) >= groups);
    if past || distinct.len() != count {
        return refuse("a self-collision pair repeats or names a group it lacks");
    }
    Ok(pairs)
}

/// The links at 0x50: entries of 16 zero bytes, a pair of groups and a value, then the fill.
fn group_links(v: Bytes, at: usize, groups: usize) -> Result<GroupLinks> {
    let entries = v.to("group links", at)?;
    let used = v.u16("group links", at + 8)?;
    let capacity = v.u16("group links", at + 0xA)?;
    if used > capacity || v.u32("group links", at + 0xC)? != 0 {
        return refuse("its group links count past their room, or their +0x0C is set");
    }
    let bytes = v.array("group links", entries, usize::from(capacity), 0x18)?;
    let mut links = Vec::with_capacity(usize::from(used));
    for (k, e) in bytes.as_chunks::<0x18>().0.iter().enumerate() {
        if k >= usize::from(used) {
            let fill = u32_at(e, 0) == UNUSED_LINK.to_bits()
                && u32_at(e, 4) == 1
                && e[8..].iter().all(|b| *b == 0);
            if !fill {
                return refuse("an unused group link is not the fill every one holds");
            }
            continue;
        }
        let pair = [le16(e, 0x10), le16(e, 0x12)];
        let past = pair.iter().any(|g| usize::from(*g) >= groups);
        if past || e[..0x10].iter().any(|b| *b != 0) {
            return refuse("a group link names a group it lacks, or its head is set");
        }
        links.push(GroupLink {
            groups: pair,
            value: f32_at(e, 0x14),
        });
    }
    Ok(GroupLinks { links, capacity })
}

/// An array slot `{pointer, u16 count, u16 capacity, u32 0}`: its target and count.
fn counted(v: Bytes, at: usize) -> Result<(Option<usize>, usize)> {
    let target = v.ptr("array slot", at)?;
    let n = v.u16("array slot", at + 8)?;
    let capacity = v.u16("array slot", at + 0xA)?;
    let zero = v.u32("array slot", at + 0xC)? == 0;
    if n != capacity || !zero || target.is_none() != (n == 0) {
        return refuse("an array slot is not a full array, null exactly when empty");
    }
    Ok((target, usize::from(n)))
}

/// The bytes of the array a slot read by [`counted`] holds, `stride` an element.
fn slice<'a>(v: Bytes<'a>, slot: (Option<usize>, usize), stride: usize) -> Result<&'a [u8]> {
    match slot.0 {
        Some(at) => v.array("lattice array", at, slot.1, stride),
        None => Ok(&[]),
    }
}

/// The lattice at `at` over `bones` bones, and the vehicle record at `vehicle` checked against it.
fn lattice(v: Bytes, at: usize, bones: usize, vehicle: Option<usize>) -> Result<Lattice> {
    let head = v.get("lattice", at, LATTICE)?;
    let form = if vehicle.is_some() { 0 } else { 2 };
    if (le16(head, 0), le16(head, 2)) != (0x2A3, form) {
        return refuse("its lattice's head is not 0x2A3, then 0 with 0xE8 set or 2");
    }
    let stated = u32_at(head, 8) as usize;
    if u32_at(head, 4) as usize != bones || u64_at(head, 0x18) != 0 {
        return refuse("its lattice does not count the bones, or its +0x18 is set");
    }
    let mut slots = Vec::with_capacity(15);
    for o in (0x20..=0xF0).step_by(0x10).chain([0x128]) {
        slots.push((o, counted(v, at + o)?));
    }
    let slot = |o: usize| (slots.iter().find(|(s, _)| *s == o)).map_or((None, 0), |(_, c)| *c);
    let (vertices, cells) = (slot(0x40).1, slot(0x60).1);
    if slot(0x20).1 != bones || slot(0x30).1 != stated {
        return refuse("its lattice's +0x20 does not count bones, or +0x30 its +0x08");
    }
    if slot(0xD0).1 != cells || slot(0x50).1 != cells {
        return refuse("its lattice's cells, boxes and +0x50 do not share a count");
    }
    let points: Vec<[f32; 3]> = (slice(v, slot(0x40), 12)?.as_chunks::<12>().0.iter())
        .map(|p| std::array::from_fn(|a| f32_at(p, 4 * a)))
        .collect();
    let records = slice(v, slot(0x60), 0x20)?.as_chunks::<0x20>().0;
    let boxes = slice(v, slot(0xD0), 0x20)?.as_chunks::<0x20>().0;
    let raw: Vec<[u16; 6]> = (records.iter())
        .map(|e| std::array::from_fn(|f| le16(e, 0x10 + 2 * f)))
        .collect();
    let mut out = Vec::with_capacity(cells);
    for (c, (e, b)) in records.iter().zip(boxes).enumerate() {
        let corners: [u16; 8] = std::array::from_fn(|k| le16(e, 2 * k));
        let mut sorted = corners;
        sorted.sort_unstable();
        let distinct = sorted.windows(2).all(|w| w[0] != w[1]);
        if !distinct || corners.iter().any(|i| usize::from(*i) >= vertices) {
            return refuse("a lattice cell's corners are not 8 distinct vertices");
        }
        let across = raw[c];
        let set = across.iter().filter(|x| **x != NONE).count();
        if u32_at(e, 0x1C) as usize != set {
            return refuse("a lattice cell's +0x1C does not count its neighbours");
        }
        for (f, x) in across.iter().enumerate() {
            let back = raw.get(usize::from(*x)).map(|n| n[f ^ 1]);
            if *x != NONE && back != Some(c as u16) {
                return refuse("a lattice cell's neighbour does not name it back");
            }
        }
        for k in [0, 1, 2] {
            let lane = corners.map(|i| points[usize::from(i)][k]);
            let lo = lane.iter().copied().fold(f32::INFINITY, f32::min);
            let hi = lane.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let (min, max) = (f32_at(b, 4 * k), f32_at(b, 0x10 + 4 * k));
            if (lo - min).abs() > 1e-4 || (hi - max).abs() > 1e-4 {
                return refuse("a lattice cell's box is not the extent of its corners");
            }
        }
        if f32_at(b, 0xC) != f32_at(b, 0) || f32_at(b, 0x1C) != f32_at(b, 0x10) {
            return refuse("a lattice cell's box does not repeat x in its w lanes");
        }
        out.push(LatticeCell {
            corners,
            neighbours: across.map(|x| (x != NONE).then_some(x)),
        });
    }
    if let Some(e) = vehicle {
        vehicle_record(v, e, [bones, stated, vertices, cells])?;
    }
    Ok(Lattice {
        vertices: points,
        cells: out,
        vehicle: vehicle.is_some(),
    })
}

/// The vehicle record at `at`, whose head restates `counts`: bones, the lattice's +0x08, its
/// vertices and its cells.
fn vehicle_record(v: Bytes, at: usize, counts: [usize; 4]) -> Result<()> {
    let r = v.get("vehicle lattice record", at, 0xD0)?;
    if u32_at(r, 0) != 0x3E9 || u64_at(r, 8) != 0 || r[0x40..].iter().any(|b| *b != 0) {
        return refuse("its vehicle record at 0xE8 lacks the head and zeros it has");
    }
    let (units, bones, third) = (
        counted(v, at + 0x10)?,
        counted(v, at + 0x20)?,
        counted(v, at + 0x30)?,
    );
    let (Some(units_at), Some(bones_at), Some(_)) = (units.0, bones.0, third.0) else {
        return refuse("an array of its vehicle record at 0xE8 is null");
    };
    if bones.1 != counts[0] || third.1 != counts[1] {
        return refuse("its vehicle record does not count the bones and lattice's +0x08");
    }
    let head = v.array("vehicle lattice units", units_at, units.1, 16)?;
    v.array("vehicle lattice bones", bones_at, bones.1, 8)?;
    if [0, 4, 8, 12].map(|o| u32_at(head, o) as usize) != counts {
        return refuse("its vehicle record's first unit does not restate the lattice");
    }
    Ok(())
}

/// The groups of `groups` whose bone of `bones` is named `wheel_` and more, in any case.
fn wheels(groups: &[FragmentGroup], bones: &[Bone]) -> Result<Vec<Wheel>> {
    let mut out = Vec::new();
    for (g, group) in groups.iter().enumerate() {
        let bone = bones.get(usize::from(group.bone));
        let name = bone.and_then(|b| b.name.get(..6));
        if !name.is_some_and(|n| n.eq_ignore_ascii_case("wheel_")) {
            continue;
        }
        if group.children.is_empty() {
            return refuse("a group on a wheel bone holds no child the composite lists");
        }
        out.push(Wheel {
            group: g as u16,
            child: group.children.start,
        });
    }
    Ok(out)
}

#[cfg(test)]
#[path = "fragment_records_tests.rs"]
mod tests;
