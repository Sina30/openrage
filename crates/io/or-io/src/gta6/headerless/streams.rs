//! A geometry's vertex data in the physical section: the slot table that states each channel's
//! stream again, the quantised position, and the other channels decoded by the form their type
//! states; and the buffer of quantised deltas a morph target holds with no declaration.

use std::f32::consts::TAU;

use super::bytes::{f32_at, sections, u32_at, Bytes};
use super::drawable::Channel;
use super::{HeaderlessError, Result};
use crate::gta6::hash::name_hash32;

/// Stride, kind and flags of every measured vertex buffer object.
pub(super) const VERTEX_BUFFER: (u16, u16, u32) = (4, 9, 0x1601);
/// Stride, kind and flags of every measured index buffer object.
pub(super) const INDEX_BUFFER: (u16, u16, u32) = (4, 8, 0x1380);

/// The declaration channel each of the 23 slots states; `None` for a slot no measured body sets.
pub(super) const SLOT_CHANNELS: [Option<u8>; 23] = [
    Some(0),
    Some(24),
    Some(4),
    Some(28),
    Some(29),
    Some(8),
    Some(16),
    Some(17),
    Some(20),
    Some(21),
    Some(12),
    Some(9),
    Some(25),
    Some(26),
    Some(30),
    Some(31),
    Some(32),
    Some(33),
    Some(10),
    Some(11),
    None,
    None,
    None,
];

/// The declaration types a slot's code byte is seen with: its low five bits name the type.
fn code_types(code: u8) -> &'static [u8] {
    match code {
        0x00 => &[0x39, 0x3E],
        0x21 => &[0x29],
        0x25 => &[0x1E],
        0x26 => &[0x1C],
        0x27 => &[0x18],
        0x28 => &[0x26],
        0x42 => &[0x10],
        0x4A => &[0x0C],
        0x4B => &[0x0A],
        0x63 => &[0x06],
        0x84 => &[0x02],
        0x8C => &[0x03],
        _ => &[],
    }
}

/// The data of the 0x50-byte buffer object at `at`: its count of elements at its physical
/// pointer, once its stride, kind and flags are the `expected` every measured buffer of its use
/// holds.
pub(super) fn buffer<'a>(
    v: Bytes<'a>,
    p: Bytes<'a>,
    what: &'static str,
    at: usize,
    expected: (u16, u16, u32),
) -> Result<&'a [u8]> {
    v.get("buffer", at, 0x50)?;
    let fields = [
        (
            "buffer's stride",
            0x0C,
            v.u16("buffer's stride", at + 0x0C)?.into(),
        ),
        (
            "buffer's kind",
            0x0E,
            v.u16("buffer's kind", at + 0x0E)?.into(),
        ),
        ("buffer's flags", 0x10, v.u32("buffer's flags", at + 0x10)?),
    ];
    let wanted = [expected.0.into(), expected.1.into(), expected.2];
    for ((what, offset, found), wanted) in fields.into_iter().zip(wanted) {
        if found != wanted {
            return Err(HeaderlessError::Constant {
                what,
                at: (at + offset) as u64,
                found: found.into(),
            });
        }
    }
    let count = v.u32("buffer's count", at + 0x08)? as usize;
    p.array(what, v.phys(what, at + 0x18)?, count, expected.0.into())
}

/// Refuses vertex data whose slot table does not state each channel of the declaration once, at
/// the channel's offset and in a form of the channel's type.
pub(super) fn slot_table(data: &[u8], channels: &[Channel]) -> Result<()> {
    let refuse = |why| HeaderlessError::Streams { why };
    let head = data
        .get(..0x60)
        .ok_or(refuse("the vertex data is shorter than its slot table"))?;
    if u32_at(head, 0x5C) != 0x200 {
        return Err(refuse("the word after its slots is not 0x200"));
    }
    let mut stated = 0;
    for (slot, channel) in SLOT_CHANNELS.iter().enumerate() {
        let word = u32_at(head, 4 * slot);
        if word == 0 {
            continue;
        }
        let index = channel.ok_or(refuse("it sets a slot no measured body sets"))?;
        let c = channels
            .iter()
            .find(|c| c.index == index)
            .ok_or(refuse("a slot states a channel the declaration lacks"))?;
        if (word >> 8) * 2 != c.offset {
            return Err(refuse("a slot's offset is not its channel's"));
        }
        let code = (word & 0xFF) as u8;
        let size_fits = match code >> 5 {
            0 => c.size < 4,
            words => u32::from(c.size) == 4 * u32::from(words),
        };
        if !size_fits || !code_types(code).contains(&c.code) {
            return Err(refuse("a slot's code is not a form of its channel's type"));
        }
        stated += 1;
    }
    // Each slot names a distinct channel, so the counts agree only for a bijection.
    if stated != channels.len() {
        return Err(refuse("a channel of the declaration has no slot"));
    }
    Ok(())
}

/// The three lanes of an 8-byte quantised element: bytes 3, 2, 1; bytes 5, 4, 0; bytes 7, 6.
pub(super) fn lanes(e: &[u8]) -> [u32; 3] {
    let (lo, hi) = (u32_at(e, 0), u32_at(e, 4));
    [lo >> 8, ((hi & 0xFFFF) << 8) | (lo & 0xFF), hi >> 16]
}

/// Channel 0's positions, accepted only where the vertices the triangles use reach the
/// geometry's box; a geometry whose every triangle is flat is not tested against its box.
pub(super) fn positions(
    data: &[u8],
    channels: &[Channel],
    count: u32,
    triangles: &[[u32; 3]],
    (box_min, box_max): ([f32; 3], [f32; 3]),
) -> Result<Vec<[f32; 3]>> {
    let refuse = |why| HeaderlessError::Positions { why };
    let channel = channels
        .iter()
        .find(|c| c.index == 0)
        .ok_or(refuse("the declaration has no channel 0"))?;
    let start = channel.offset as usize;
    // The declaration check placed every channel inside `data`.
    let at = |i: usize, size: usize| &data[start + size * i..start + size * (i + 1)];
    let (positions, quanta): (Vec<[f64; 3]>, [f64; 3]) = match (channel.code, channel.size) {
        (6, 12) => (
            (0..count as usize)
                .map(|i| {
                    let v = at(i, 12);
                    std::array::from_fn(|a| f64::from(f32_at(v, 4 * a)))
                })
                .collect(),
            [0.0; 3],
        ),
        (0x10, 8) => {
            let header = data.get(..0x80).ok_or(refuse(
                "the vertex data is shorter than its 0x80-byte header",
            ))?;
            if f32_at(header, 0x7C) != 4.0 {
                return Err(refuse("the quantisation block's word at 0x7C is not 4.0"));
            }
            let origin: [f64; 3] = std::array::from_fn(|l| f64::from(f32_at(header, 0x60 + 4 * l)));
            let step: [f64; 3] = std::array::from_fn(|l| f64::from(f32_at(header, 0x70 + 4 * l)));
            // The axis each lane gives, by the block's word at 0x6C.
            let axes = match u32_at(header, 0x6C) {
                0 => [2, 1, 0],
                1 => [2, 0, 1],
                2 => [0, 1, 2],
                _ => {
                    return Err(refuse(
                        "the quantisation block's axis word is not 0, 1 or 2",
                    ))
                }
            };
            let mut quanta = [0.0; 3];
            for (l, axis) in axes.iter().enumerate() {
                quanta[*axis] = step[l].abs();
            }
            let positions = (0..count as usize)
                .map(|i| {
                    let q = lanes(at(i, 8));
                    let mut p = [0.0; 3];
                    for (l, axis) in axes.iter().enumerate() {
                        p[*axis] = origin[l] + f64::from(q[l]) * step[l];
                    }
                    p
                })
                .collect();
            (positions, quanta)
        }
        _ => {
            return Err(refuse(
                "channel 0 is neither the 8-byte quantised position nor three floats",
            ))
        }
    };
    if triangles.is_empty() {
        return Err(refuse("the geometry has no triangle to place its vertices"));
    }
    let flat = triangles.iter().all(|t| {
        let [a, b, c] = t.map(|i| positions[i as usize]);
        let (u, w) = (sub(b, a), sub(c, a));
        u[1] * w[2] - u[2] * w[1] == 0.0
            && u[2] * w[0] - u[0] * w[2] == 0.0
            && u[0] * w[1] - u[1] * w[0] == 0.0
    });
    if !flat {
        let (mut low, mut high) = ([f64::MAX; 3], [f64::MIN; 3]);
        for &i in triangles.iter().flatten() {
            let p = positions[i as usize];
            for a in 0..3 {
                low[a] = low[a].min(p[a]);
                high[a] = high[a].max(p[a]);
            }
        }
        let reaches = (0..3).all(|a| {
            let (min, max) = (f64::from(box_min[a]), f64::from(box_max[a]));
            let tolerance = 2.0 * quanta[a] + min.abs().max(max.abs()) / 1024.0 + 1e-4;
            (low[a] - min).abs() <= tolerance && (high[a] - max).abs() <= tolerance
        });
        if !reaches {
            return Err(refuse(
                "the decoded vertices do not reach the geometry's box",
            ));
        }
    }
    Ok(positions.into_iter().map(|p| p.map(|x| x as f32)).collect())
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// A geometry's channels beyond position, each decoded by the form its type states.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Vertices {
    /// Channel 4.
    pub normals: Vec<[f32; 3]>,
    /// Texcoord 0's tangent, which channel 4's frame word holds, or channel 8 where channel 4's
    /// 10:10:10 normal holds none; `w` is 1 where the cross product of normal and tangent runs
    /// along the texture's v, else -1.
    pub tangents: Vec<[f32; 4]>,
    /// Channels 9 to 11, the tangents of texcoords 1 to 3, `w` as in [`Self::tangents`].
    pub uv_tangents: Vec<(u8, Vec<[f32; 4]>)>,
    /// Channels 28 to 33, texcoords 0 to 5.
    pub uvs: Vec<(u8, Vec<[f32; 2]>)>,
    /// Channels 24 to 26, each byte over 255 in the order stored; what each colour means is not known.
    pub colours: Vec<(u8, Vec<[f32; 4]>)>,
    /// Channels 16 and 17, each byte over 255, in no weight order; eight lanes sum to one, four to at most one.
    pub weights: Vec<(u8, Vec<[f32; 4]>)>,
    /// Channels 20 and 21: weight lane k's index through the palette; a one-index form fills lane 0 of 20.
    pub joints: Vec<(u8, Vec<[u16; 4]>)>,
    /// Channel 33's u16 form, per vertex: where lane 0's bit 15 is set, lanes 1 to 3 as a reference
    /// position, `(q - 32768) / 8192` m less a per-buffer offset whose source is not known; `None`
    /// where it is clear, as lanes whose meaning is not known.
    pub reference: Vec<Option<[f32; 3]>>,
    /// Channels, or parts of them, whose values are not read, with why.
    pub skipped: Vec<(u8, &'static str)>,
}

/// Every channel of `channels` but the position, decoded from `data` for `count` vertices.
pub(super) fn vertices(data: &[u8], channels: &[Channel], count: u32) -> Vertices {
    let mut out = Vertices::default();
    for c in channels {
        let (index, size) = (c.index, usize::from(c.size));
        // The declaration check placed every channel inside `data`.
        let elements = || {
            data[c.offset as usize..c.offset as usize + size * count as usize].chunks_exact(size)
        };
        let words = || elements().map(|e| u32_at(e, 0));
        match (index, c.code, c.size) {
            (0, ..) => {}
            (4, 0x29, 4) => {
                (out.normals, out.tangents) = words().map(frame).unzip();
            }
            (4, 0x18, 4) => out.normals = words().map(unorm10).collect(),
            (4, 0x06, 12) => out.normals = elements().map(float3).collect(),
            // Texcoord 0's tangent in the tangent group's form, its sign 0 or 3.
            (8, 0x18, 4) if words().all(|w| matches!(w >> 30, 0 | 3)) => {
                out.tangents = words().map(uv_tangent).collect();
            }
            (8, ..) => out.skipped.push((
                index,
                "channel 8 holds a sign other than 0 or 3, or a form not measured",
            )),
            (9..=11, 0x18, 4) => out
                .uv_tangents
                .push((index, words().map(uv_tangent).collect())),
            (16 | 17, 0x1C, 4) => out
                .weights
                .push((index, elements().map(bytes_unorm).collect())),
            (24..=26, 0x1C, 4) => out
                .colours
                .push((index, elements().map(bytes_unorm).collect())),
            (20 | 21, 0x1E, 4) => out.joints.push((
                index,
                elements()
                    .map(|e| std::array::from_fn(|k| u16::from(e[k])))
                    .collect(),
            )),
            (20 | 21, 0x03, 16) => {
                out.joints.push((
                    index,
                    elements()
                        .map(|e| std::array::from_fn(|k| (u32_at(e, 4 * k) & 0xFFF) as u16))
                        .collect(),
                ));
                out.skipped.push((
                    index,
                    "bits 12 to 31 of each lane hold a per-vertex field whose meaning is not known",
                ));
            }
            (20 | 21, 0x0C, 8) => out.joints.push((
                index,
                elements()
                    .map(|e| std::array::from_fn(|k| u16::from_le_bytes([e[2 * k], e[2 * k + 1]])))
                    .collect(),
            )),
            // One index a vertex, which takes all of its weight.
            (20 | 21, 0x3E, 1) => out.joints.push((
                index,
                elements().map(|e| [u16::from(e[0]), 0, 0, 0]).collect(),
            )),
            (20 | 21, 0x39, 2) => out.joints.push((
                index,
                elements()
                    .map(|e| [u16::from_le_bytes([e[0], e[1]]), 0, 0, 0])
                    .collect(),
            )),
            (20 | 21, ..) => out.skipped.push((
                index,
                "no body's blend indices of this form were checked against its bone count",
            )),
            (28..=32, 0x10, 8) => out.uvs.push((
                index,
                elements().map(|e| [f32_at(e, 0), f32_at(e, 4)]).collect(),
            )),
            (28..=33, 0x26, 4) => out.uvs.push((
                index,
                elements()
                    .map(|e| {
                        let s16 = |at: usize| f32::from(i16::from_le_bytes([e[at], e[at + 1]]));
                        [s16(0) / 8192.0, s16(2) / 8192.0]
                    })
                    .collect(),
            )),
            (33, 0x10, 8) => {
                out.reference = elements()
                    .map(|e| {
                        let lane = |k: usize| u16::from_le_bytes([e[2 * k], e[2 * k + 1]]);
                        let metres = |k: usize| (f32::from(lane(k)) - 32768.0) / 8192.0;
                        (lane(0) & 0x8000 != 0).then(|| [metres(1), metres(2), metres(3)])
                    })
                    .collect();
                if out.reference.iter().any(Option::is_none) {
                    out.skipped.push((
                        index,
                        "channel 33's lanes where lane 0's bit 15 is clear hold values whose meaning is not known",
                    ));
                }
            }
            (33, ..) => out.skipped.push((
                index,
                "channel 33 in this form holds values whose meaning is not known",
            )),
            _ => out
                .skipped
                .push((index, "this channel's form was not measured")),
        }
    }
    out
}

fn unit(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 0.0 {
        v.map(|x| x / len)
    } else {
        v
    }
}

/// `bits` bits of `w` from bit `at` as an unsigned normalised lane over [-1, 1].
fn lane(w: u32, at: u32, bits: u32) -> f32 {
    let max = (1u32 << bits) - 1;
    ((w >> at) & max) as f32 / (max as f32 / 2.0) - 1.0
}

/// The unit vector an octahedral pair encodes.
fn octahedral(x: f32, y: f32) -> [f32; 3] {
    let z = 1.0 - x.abs() - y.abs();
    if z < 0.0 {
        unit([
            (1.0 - y.abs()) * 1f32.copysign(x),
            (1.0 - x.abs()) * 1f32.copysign(y),
            z,
        ])
    } else {
        unit([x, y, z])
    }
}

fn unorm10(w: u32) -> [f32; 3] {
    unit([lane(w, 0, 10), lane(w, 10, 10), lane(w, 20, 10)])
}

/// A tangent group word: a 10:10:10 tangent, `w` 1 where bits 30 and 31 are clear, the opposite
/// convention to the frame word's bit 30.
fn uv_tangent(w: u32) -> [f32; 4] {
    let t = unorm10(w);
    [t[0], t[1], t[2], if w >> 30 == 0 { 1.0 } else { -1.0 }]
}

fn float3(e: &[u8]) -> [f32; 3] {
    std::array::from_fn(|a| f32_at(e, 4 * a))
}

fn bytes_unorm(e: &[u8]) -> [f32; 4] {
    std::array::from_fn(|k| f32::from(e[k]) / 255.0)
}

/// A normal and tangent frame word: with bit 31 set, an octahedral 10:10 normal and the tangent's
/// angle about it in bits 20 to 29; with it clear, an octahedral 8:8 normal and a 7:7 tangent.
/// Bit 30 is set where the cross product of normal and tangent runs along the texture's v.
pub(super) fn frame(w: u32) -> ([f32; 3], [f32; 4]) {
    let sign = if (w >> 30) & 1 == 1 { 1.0 } else { -1.0 };
    let (n, t) = if w >> 31 == 1 {
        let n = octahedral(lane(w, 0, 10), lane(w, 10, 10));
        let theta = ((w >> 20) & 0x3FF) as f32 * TAU / 1024.0;
        // Duff's branchless orthonormal basis about n.
        let s = 1f32.copysign(n[2]);
        let a = -1.0 / (s + n[2]);
        let b = n[0] * n[1] * a;
        let b1 = [1.0 + s * n[0] * n[0] * a, s * b, -s * n[0]];
        let b2 = [b, s + n[1] * n[1] * a, -n[1]];
        let t = std::array::from_fn(|k| theta.cos() * b1[k] - theta.sin() * b2[k]);
        (n, t)
    } else {
        let n = octahedral(lane(w, 0, 8), lane(w, 8, 8));
        (n, octahedral(lane(w, 16, 7), lane(w, 23, 7)))
    };
    (n, [t[0], t[1], t[2], sign])
}

/// A morph target: quantised three-lane deltas for the head level of detail its key names.
// They sit in a vertex buffer with no declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct MorphTarget {
    /// The name hash of the mesh the deltas belong to.
    pub mesh_key: u32,
    /// The value of a zero quantum, per lane.
    pub origin: [f32; 3],
    /// The size of one quantum, per lane; lanes 0 and 1 hold 24 bits and lane 2 16.
    pub step: [f32; 3],
    /// 8-byte elements, applied to no mesh: their vertex and each lane's axis are not known.
    pub elements: Vec<[u32; 3]>,
}

/// The base heads a morph target's key names, each with one of [`MORPH_LOD_SUFFIXES`].
const MORPH_HEADS: [&str; 8] = [
    "head_fr1_base_000_m_full",
    "head_mr1_base_000_m_full",
    "head_mo1_base_000_m_full",
    "head_fo1_base_000_m_full",
    "head_mb1_base_000_m_full",
    "head_fb1_base_000_m_full",
    "head_mi1_base_000_i_full",
    "head_mk1_base_000_k_full",
];
/// A key's suffix, by the level of detail it names.
const MORPH_LOD_SUFFIXES: [&str; 4] = ["", "_lod", "_lod2", "_lod3"];
/// Bits of each lane of an element.
const MORPH_LANE_BITS: [u32; 3] = [24, 24, 16];
const MORPH_VIRTUAL_LEN: usize = 240;
const MORPH_BUFFER_AT: usize = 0x70;

impl MorphTarget {
    /// The base head mesh its key names, with its suffix, and the level of detail the suffix gives.
    pub fn mesh(&self) -> Option<(String, usize)> {
        for head in MORPH_HEADS {
            for (lod, suffix) in MORPH_LOD_SUFFIXES.into_iter().enumerate() {
                let name = format!("{head}{suffix}");
                if name_hash32(name.as_bytes()) == self.mesh_key {
                    return Some((name, lod));
                }
            }
        }
        None
    }

    /// Whether every step is zero, which the reader holds only with every element zero.
    pub fn is_empty(&self) -> bool {
        self.step.iter().all(|s| *s == 0.0)
    }

    /// An element's three lanes as values: the origin plus its quanta times the step.
    pub fn value(&self, element: [u32; 3]) -> [f32; 3] {
        std::array::from_fn(|l| self.origin[l] + element[l] as f32 * self.step[l])
    }
}

pub fn read_morph_target(virtual_section: &[u8], physical: Option<&[u8]>) -> Result<MorphTarget> {
    let refuse = |why| HeaderlessError::Streams { why };
    if virtual_section.len() != MORPH_VIRTUAL_LEN {
        return Err(refuse("a morph target's virtual section is not 240 bytes"));
    }
    let (first, zeros) = (&virtual_section[..8], &virtual_section[0x18..0x58]);
    if first.iter().chain(zeros).any(|b| *b != 0) {
        return Err(refuse(
            "a morph target's root is not zero at 0 and from 0x18 to 0x58",
        ));
    }
    sections(virtual_section, physical, |v, p| {
        let vb = v.to("vertex buffer", 0x10)?;
        if vb != MORPH_BUFFER_AT {
            return Err(HeaderlessError::Disagree {
                what: "morph target's buffer address and the end of its root",
                first: vb as u64,
                second: MORPH_BUFFER_AT as u64,
            });
        }
        let start = v.phys("deltas", vb + 0x18)?;
        if start != 0 {
            return Err(HeaderlessError::Constant {
                what: "morph target's deltas address",
                at: (vb + 0x18) as u64,
                found: start as u64,
            });
        }
        let declaration = v.u64("vertex declaration", vb + 0x40)?;
        if declaration != 0 {
            return Err(HeaderlessError::Constant {
                what: "morph target's vertex declaration",
                at: (vb + 0x40) as u64,
                found: declaration,
            });
        }
        let data = buffer(v, p, "deltas", vb, VERTEX_BUFFER)?;
        let head = data
            .get(..0x20)
            .ok_or(refuse("the deltas are shorter than their 0x20-byte head"))?;
        if u32_at(head, 0) != 1 || u32_at(head, 0x1C) != 0 {
            return Err(refuse(
                "the deltas' head does not hold 1 at 0 and 0 at 0x1C",
            ));
        }
        let body = &data[0x20..];
        if body.len() % 8 != 0 {
            return Err(refuse("the deltas are not whole 8-byte elements"));
        }
        let target = MorphTarget {
            mesh_key: v.u32("mesh key", 0x58)?,
            origin: std::array::from_fn(|l| f32_at(head, 4 + 4 * l)),
            step: std::array::from_fn(|l| f32_at(head, 0x10 + 4 * l)),
            elements: body.as_chunks::<8>().0.iter().map(|e| lanes(e)).collect(),
        };
        morph_holds(&target)?;
        Ok(target)
    })
}

/// Refuses a morph target whose scale is not finite, an empty target with an element that is not
/// zero, or a lane that does not reach both faces of its box or whose box leaves out zero.
fn morph_holds(t: &MorphTarget) -> Result<()> {
    let refuse = |why| HeaderlessError::Streams { why };
    if !t.origin.iter().chain(&t.step).all(|x| x.is_finite()) {
        return Err(refuse("a morph target's origin or step is not finite"));
    }
    if t.is_empty() {
        return match t.elements.iter().all(|e| *e == [0; 3]) {
            true => Ok(()),
            false => Err(refuse(
                "a morph target whose every step is zero holds an element that is not zero",
            )),
        };
    }
    if t.elements.is_empty() {
        return Ok(());
    }
    for (l, bits) in MORPH_LANE_BITS.into_iter().enumerate() {
        let lane = || t.elements.iter().map(|e| e[l]);
        let (lo, hi) = (lane().min().unwrap_or(0), lane().max().unwrap_or(0));
        let face = 1u32 << (bits - 8);
        if lo > face || hi < (1u32 << bits) - face {
            return Err(refuse(
                "a lane of the deltas does not reach both faces of its box",
            ));
        }
        let (origin, step) = (f64::from(t.origin[l]), f64::from(t.step[l]));
        let top = origin + f64::from((1u32 << bits) - 1) * step;
        if origin > 1e-9 || top < -1e-9 {
            return Err(refuse("a lane's box of the deltas leaves out zero"));
        }
    }
    Ok(())
}

/// Vertex buffers with no index buffer and no drawable: each buffer's vertex count and channels.
#[derive(Debug, Clone, PartialEq)]
pub struct VertexBuffers {
    pub buffers: Vec<(u32, Vec<Channel>)>,
}

pub fn read_vertex_buffers(
    virtual_section: &[u8],
    physical: Option<&[u8]>,
) -> Result<VertexBuffers> {
    sections(virtual_section, physical, |v, p| {
        let (count, capacity) = (
            v.u16("array's count", 0x18)?,
            v.u16("array's capacity", 0x1A)?,
        );
        if count != capacity {
            return Err(HeaderlessError::Disagree {
                what: "array's count and capacity",
                first: count.into(),
                second: capacity.into(),
            });
        }
        let array = v.to("buffer array", 0x10)?;
        let pointers = v.array("buffer array", array, count.into(), 8)?;
        let mut buffers = Vec::with_capacity(count.into());
        for i in 0..pointers.len() / 8 {
            let vb = v.to("vertex buffer", array + 8 * i)?;
            let data = buffer(v, p, "vertex data", vb, VERTEX_BUFFER)?;
            let (vertices, channels) = super::drawable::declaration(
                v,
                v.to("vertex declaration", vb + 0x40)?,
                data.len(),
            )?;
            slot_table(data, &channels)?;
            buffers.push((vertices, channels));
        }
        Ok(VertexBuffers { buffers })
    })
}

/// A blob under a 64-bit key, whose hash and bytes are not known, in either section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyedBlob {
    pub key: u64,
    pub len: u64,
    pub physical: bool,
}

pub fn read_keyed_blob(virtual_section: &[u8], physical: Option<&[u8]>) -> Result<KeyedBlob> {
    sections(virtual_section, physical, |v, p| {
        let (key, stated) = (v.u64("blob's key", 0x20)?, v.u64("blob's length", 0x28)?);
        let (len, physical) = match v.ptr("raw buffer", 0x10)? {
            Some(raw) => (buffer(v, p, "blob", raw, INDEX_BUFFER)?.len() as u64, true),
            // The blob runs from its pointer to the block map.
            None => {
                let at = v.to("blob", 0x18)?;
                let map = v.to("block map", 0x08)?;
                (map.saturating_sub(at) as u64, false)
            }
        };
        if len != stated {
            return Err(HeaderlessError::Disagree {
                what: "blob's stated length and the bytes it spans",
                first: stated,
                second: len,
            });
        }
        Ok(KeyedBlob { key, len, physical })
    })
}
