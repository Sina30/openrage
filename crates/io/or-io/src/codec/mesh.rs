//! Vertices addressed by channel rather than by byte offset, decoded to a
//! neutral [`Mesh`], and the boxes a drawable ships recomputed over them,
//! for any vertex buffer that implements [`VertexSource`].
//!
//! ⚠ **The recomputed boxes are not equally derivable.** `openrage-cli
//! bounds` over a retail Legacy install — 158,951 files, 993,907 geometries —
//! puts a recomputed geometry box bit-identical to the shipped one **95.42%**
//! of the time and inside it **100.00%** of the time, with the model prefix at
//! 90.11% and 100.00%. The drawable's own box is neither: only 56.82% of
//! shipped boxes are within 1% of the union of their own vertices and
//! **30.36% do not contain their own mesh**. The control — the same
//! comparison against the neighbouring slot — agrees 0.49% of the time.
//!
//! ⚠ **Enhanced is exact where Legacy is not, and the gap is the encoder
//! rather than the rule.** The same run over a retail Enhanced install —
//! 989,349 geometries — is **100.00%** bit-identical at both levels. Legacy's
//! 4.58% are boxes whose fourth lanes still hold the `±FLT_MAX` a min/max
//! accumulator starts from, so they were computed somewhere other than over
//! the packed positions. The drawable level disagrees identically on both
//! (56.20% within 1%, 69.11% containing), which says it is authored.

use crate::{f32_to_half, half_to_f32};

/// One of the sixteen channels a Legacy vertex can carry, numbered as its FVF bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GrcFvfChannel {
    Position = 0,
    Weight = 1,
    Binding = 2,
    Normal = 3,
    Diffuse = 4,
    Specular = 5,
    Tex0 = 6,
    Tex1 = 7,
    Tex2 = 8,
    Tex3 = 9,
    Tex4 = 10,
    Tex5 = 11,
    Tex6 = 12,
    Tex7 = 13,
    Tangent0 = 14,
    Tangent1 = 15,
}

impl GrcFvfChannel {
    /// Every channel, in the order a vertex lays them out, which is the
    /// discriminant order: a slot number needs no table of its own.
    pub const ALL: [Self; Self::COUNT] = [
        Self::Position,
        Self::Weight,
        Self::Binding,
        Self::Normal,
        Self::Diffuse,
        Self::Specular,
        Self::Tex0,
        Self::Tex1,
        Self::Tex2,
        Self::Tex3,
        Self::Tex4,
        Self::Tex5,
        Self::Tex6,
        Self::Tex7,
        Self::Tangent0,
        Self::Tangent1,
    ];

    pub const COUNT: usize = 16;

    /// The eight texture-coordinate channels, lowest first.
    pub const TEX: [Self; 8] = [
        Self::Tex0,
        Self::Tex1,
        Self::Tex2,
        Self::Tex3,
        Self::Tex4,
        Self::Tex5,
        Self::Tex6,
        Self::Tex7,
    ];

    #[inline]
    pub const fn mask(self) -> u32 {
        1 << (self as u8)
    }

    pub const fn from_slot(slot: u8) -> Option<Self> {
        if (slot as usize) < Self::COUNT {
            Some(Self::ALL[slot as usize])
        } else {
            None
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Position => "position",
            Self::Weight => "weight",
            Self::Binding => "binding",
            Self::Normal => "normal",
            Self::Diffuse => "diffuse",
            Self::Specular => "specular",
            Self::Tex0 => "tex0",
            Self::Tex1 => "tex1",
            Self::Tex2 => "tex2",
            Self::Tex3 => "tex3",
            Self::Tex4 => "tex4",
            Self::Tex5 => "tex5",
            Self::Tex6 => "tex6",
            Self::Tex7 => "tex7",
            Self::Tangent0 => "tangent0",
            Self::Tangent1 => "tangent1",
        }
    }
}

/// How one channel is encoded, numbered as the FVF nibble that names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GrcDataSize {
    Half = 0,
    Half2 = 1,
    Half3 = 2,
    Half4 = 3,
    Float = 4,
    Float2 = 5,
    Float3 = 6,
    Float4 = 7,
    UByte4 = 8,
    Color = 9,
    PackedNormal = 10,
    ShortUnorm = 11,
    Short2Unorm = 12,
    Byte2Unorm = 13,
    Short2 = 14,
    Short4 = 15,
}

impl GrcDataSize {
    /// Every encoding, indexed by the nibble that names it; exhaustive over
    /// the four bits, so every nibble a file holds names one.
    pub const ALL: [Self; 16] = [
        Self::Half,
        Self::Half2,
        Self::Half3,
        Self::Half4,
        Self::Float,
        Self::Float2,
        Self::Float3,
        Self::Float4,
        Self::UByte4,
        Self::Color,
        Self::PackedNormal,
        Self::ShortUnorm,
        Self::Short2Unorm,
        Self::Byte2Unorm,
        Self::Short2,
        Self::Short4,
    ];

    /// The encoding the low four bits of `nibble` name.
    pub const fn from_nibble(nibble: u8) -> Self {
        Self::ALL[(nibble & 0x0F) as usize]
    }

    /// How many components the encoding actually carries.
    pub const fn components(self) -> usize {
        match self {
            Self::Half | Self::Float | Self::ShortUnorm => 1,
            Self::Half2 | Self::Float2 | Self::Short2Unorm | Self::Byte2Unorm | Self::Short2 => 2,
            Self::Half3 | Self::Float3 | Self::PackedNormal => 3,
            Self::Half4 | Self::Float4 | Self::UByte4 | Self::Color | Self::Short4 => 4,
        }
    }

    #[inline]
    pub const fn byte_size(self) -> usize {
        match self {
            Self::Half => 2,
            Self::Half2 => 4,
            Self::Half3 => 6,
            Self::Half4 => 8,
            Self::Float => 4,
            Self::Float2 => 8,
            Self::Float3 => 12,
            Self::Float4 => 16,
            Self::UByte4 => 4,
            Self::Color => 4,
            Self::PackedNormal => 4,
            Self::ShortUnorm => 2,
            Self::Short2Unorm => 4,
            Self::Byte2Unorm => 2,
            Self::Short2 => 4,
            Self::Short4 => 8,
        }
    }
}

/// Where one channel lives, and how it is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelSlot {
    /// Byte offset of vertex 0's value: within the vertex when interleaved,
    /// from the start of the payload when the channel is its own run.
    pub offset: usize,
    /// The step between vertices; zero means the buffer's own stride.
    pub stride: usize,
    pub size: GrcDataSize,
}

/// Every channel's position within a vertex, computed once.
///
/// ⚠ A Legacy FVF lays channels out in **discriminant order**, not in the
/// order a caller asks about them, so the walk visits all 16 slots in order
/// and accumulates: skipping a channel you do not want still costs its bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexLayout {
    slots: [Option<ChannelSlot>; GrcFvfChannel::COUNT],
    stride: usize,
}

impl VertexLayout {
    /// A Legacy FVF's layout: bit N of `fvf` present, nibble N of `channel_sizes` its encoding.
    pub fn of(fvf: u32, channel_sizes: u64) -> Self {
        let mut slots = [None; GrcFvfChannel::COUNT];
        let mut offset = 0usize;
        for channel in GrcFvfChannel::ALL {
            if fvf & channel.mask() == 0 {
                continue;
            }
            let size = GrcDataSize::from_nibble((channel_sizes >> (channel as u64 * 4)) as u8);
            slots[channel as usize] = Some(ChannelSlot {
                offset,
                stride: 0,
                size,
            });
            offset += size.byte_size();
        }
        Self {
            slots,
            stride: offset,
        }
    }

    /// An Enhanced declaration's layout, over the elements a Legacy channel names.
    ///
    /// ⚠ Each element keeps its own base and stride: a structure-of-arrays
    /// buffer keeps its positions outside any one vertex, so addressing them as
    /// `vertex * stride + offset_in_vertex` reads colour.
    pub fn of_g9(elements: &[VertexElementG9]) -> Self {
        let mut slots = [None; GrcFvfChannel::COUNT];
        let mut stride = 0usize;
        for element in elements {
            let (Some(channel), Some(size)) = (
                element.gen8_semantic().and_then(GrcFvfChannel::from_slot),
                element.format.gen8_data_size(),
            ) else {
                continue;
            };
            slots[channel as usize] = Some(ChannelSlot {
                offset: element.offset as usize,
                stride: usize::from(element.stride),
                size,
            });
            stride += size.byte_size();
        }
        Self { slots, stride }
    }

    pub fn slot(&self, channel: GrcFvfChannel) -> Option<ChannelSlot> {
        self.slots[channel as usize]
    }

    pub fn has(&self, channel: GrcFvfChannel) -> bool {
        self.slots[channel as usize].is_some()
    }

    /// The stride the channels themselves imply.
    ///
    /// ⚠ Not necessarily the buffer's declared stride: a buffer may pad. Use
    /// [`VertexReader::stride`] to address vertices and this to check the
    /// declaration against the channels.
    pub fn packed_stride(&self) -> usize {
        self.stride
    }

    pub fn present(&self) -> impl Iterator<Item = (GrcFvfChannel, ChannelSlot)> + '_ {
        GrcFvfChannel::ALL
            .into_iter()
            .filter_map(|c| self.slot(c).map(|s| (c, s)))
    }

    pub fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

/// What the codec reads of one vertex buffer.
pub trait VertexSource {
    fn vertex_data(&self) -> &[u8];
    /// `None` when the buffer declares no layout.
    fn vertex_layout(&self) -> Option<VertexLayout>;
    /// The step between vertices the buffer states; zero when it states none.
    fn vertex_stride(&self) -> usize;
    /// How many vertices the buffer states; zero when it states none.
    fn vertex_declared(&self) -> usize;
}

/// Reads any channel of any vertex out of a buffer.
#[derive(Debug, Clone, Copy)]
pub struct VertexReader<'a> {
    data: &'a [u8],
    layout: VertexLayout,
    stride: usize,
    declared: usize,
}

impl<'a> VertexReader<'a> {
    /// ⚠ `None` when the buffer declares no layout. A buffer without a
    /// declaration has bytes but no channels, and guessing a layout for it
    /// would read whatever happened to be at the offset.
    pub fn new<S: VertexSource + ?Sized>(source: &'a S) -> Option<Self> {
        let layout = source.vertex_layout()?;
        // The declared stride wins, because it is what the file says the
        // spacing is; the two differ when a buffer pads.
        let stride = match source.vertex_stride() {
            0 => layout.packed_stride(),
            declared => declared,
        };
        Some(Self {
            data: source.vertex_data(),
            layout,
            stride,
            declared: source.vertex_declared(),
        })
    }

    pub fn layout(&self) -> &VertexLayout {
        &self.layout
    }

    pub fn stride(&self) -> usize {
        self.stride
    }

    /// How many vertices are actually addressable.
    ///
    /// ⚠ The lesser of the declared count and what the bytes hold. A
    /// truncated buffer declares more than it carries, and trusting the
    /// declaration reads past the end.
    pub fn count(&self) -> usize {
        if self.stride == 0 {
            return 0;
        }
        let held = self.data.len() / self.stride;
        if self.declared == 0 {
            held
        } else {
            self.declared.min(held)
        }
    }

    /// What the buffer says it holds, whatever the bytes say.
    pub fn declared_count(&self) -> usize {
        self.declared
    }

    pub fn is_truncated(&self) -> bool {
        self.stride != 0 && self.declared > self.data.len() / self.stride
    }

    /// The raw bytes of one channel of one vertex.
    pub fn raw(&self, channel: GrcFvfChannel, vertex: usize) -> Option<&'a [u8]> {
        let slot = self.layout.slot(channel)?;
        let step = if slot.stride == 0 {
            self.stride
        } else {
            slot.stride
        };
        let at = vertex.checked_mul(step)?.checked_add(slot.offset)?;
        self.data.get(at..at.checked_add(slot.size.byte_size())?)
    }

    /// One channel of one vertex, decoded.
    ///
    /// ⚠ Padding follows the D3D convention: components the encoding does
    /// not carry read `0.0`, except the fourth, which reads `1.0`. A caller
    /// that needs to know how many were real asks
    /// [`GrcDataSize::components`].
    pub fn get(&self, channel: GrcFvfChannel, vertex: usize) -> Option<[f32; 4]> {
        let slot = self.layout.slot(channel)?;
        decode(slot.size, self.raw(channel, vertex)?)
    }

    /// Every addressable vertex's position.
    pub fn positions(&self) -> impl Iterator<Item = [f32; 3]> + '_ {
        (0..self.count())
            .filter_map(|v| self.get(GrcFvfChannel::Position, v))
            .map(|p| [p[0], p[1], p[2]])
    }

    /// The box over every position; `None` when there is none.
    pub fn aabb(&self) -> Option<Aabb> {
        let mut builder = AabbBuilder::new();
        self.positions().for_each(|p| builder.push(p));
        builder.finish()
    }
}

/// A geometry's vertices and triangles, whatever target laid them out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    /// Empty when the geometry carries no normal.
    pub normals: Vec<[f32; 3]>,
    /// The lowest texture-coordinate channel the geometry carries; empty when it carries none.
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl Mesh {
    /// The mesh `vertices` and `indices` hold; a buffer with no layout gives no vertices.
    ///
    /// ⚠ The vertices are those [`VertexReader::positions`] reads, as the box
    /// covers; a channel that cannot be read for every one of them is left
    /// empty rather than padded.
    pub fn decode(vertices: Option<VertexReader<'_>>, indices: &[u16]) -> Self {
        let indices = indices.iter().map(|&i| u32::from(i)).collect();
        let Some(reader) = vertices else {
            return Self {
                indices,
                ..Self::default()
            };
        };
        let positions: Vec<[f32; 3]> = reader.positions().collect();
        let channel = |c: GrcFvfChannel| -> Vec<[f32; 4]> {
            if !reader.layout().has(c) {
                return Vec::new();
            }
            (0..positions.len())
                .map(|v| reader.get(c, v))
                .collect::<Option<_>>()
                .unwrap_or_default()
        };
        let xyz = |v: [f32; 4]| [v[0], v[1], v[2]];
        let uv = GrcFvfChannel::TEX
            .into_iter()
            .find(|&c| reader.layout().has(c));
        Self {
            normals: channel(GrcFvfChannel::Normal)
                .into_iter()
                .map(xyz)
                .collect(),
            uvs: uv
                .map(|c| channel(c).into_iter().map(|v| [v[0], v[1]]).collect())
                .unwrap_or_default(),
            positions,
            indices,
        }
    }
}

/// Encode `[f32; 4]` into one channel's bytes.
pub fn encode(size: GrcDataSize, value: [f32; 4], out: &mut [u8]) {
    debug_assert_eq!(out.len(), size.byte_size());
    let put_f32 = |out: &mut [u8], i: usize, v: f32| {
        out[i..i + 4].copy_from_slice(&v.to_le_bytes());
    };
    let put_half = |out: &mut [u8], i: usize, v: f32| {
        out[i..i + 2].copy_from_slice(&f32_to_half(v).to_le_bytes());
    };
    let unorm8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let put_u16 = |out: &mut [u8], i: usize, v: u16| {
        out[i..i + 2].copy_from_slice(&v.to_le_bytes());
    };

    match size {
        GrcDataSize::Half => put_half(out, 0, value[0]),
        GrcDataSize::Half2 => {
            put_half(out, 0, value[0]);
            put_half(out, 2, value[1]);
        }
        GrcDataSize::Half3 => {
            for (c, v) in value.iter().take(3).enumerate() {
                put_half(out, c * 2, *v);
            }
        }
        GrcDataSize::Half4 => {
            for (c, v) in value.iter().enumerate() {
                put_half(out, c * 2, *v);
            }
        }
        GrcDataSize::Float => put_f32(out, 0, value[0]),
        GrcDataSize::Float2 => {
            put_f32(out, 0, value[0]);
            put_f32(out, 4, value[1]);
        }
        GrcDataSize::Float3 => {
            for (c, v) in value.iter().take(3).enumerate() {
                put_f32(out, c * 4, *v);
            }
        }
        GrcDataSize::Float4 => {
            for (c, v) in value.iter().enumerate() {
                put_f32(out, c * 4, *v);
            }
        }
        GrcDataSize::UByte4 => {
            for (slot, v) in out.iter_mut().zip(value) {
                *slot = v.clamp(0.0, 255.0).round() as u8;
            }
        }
        GrcDataSize::Color => {
            for (slot, v) in out.iter_mut().zip(value) {
                *slot = unorm8(v);
            }
        }
        GrcDataSize::PackedNormal => {
            let axis = |v: f32| ((v.clamp(-1.0, 1.0) * 511.0).round() as i32 & 0x3FF) as u32;
            let raw = axis(value[0]) | (axis(value[1]) << 10) | (axis(value[2]) << 20);
            out.copy_from_slice(&raw.to_le_bytes());
        }
        GrcDataSize::ShortUnorm => {
            put_u16(out, 0, (value[0].clamp(0.0, 1.0) * 65535.0).round() as u16)
        }
        GrcDataSize::Short2Unorm => {
            for (c, v) in value.iter().take(2).enumerate() {
                put_u16(out, c * 2, (v.clamp(0.0, 1.0) * 65535.0).round() as u16);
            }
        }
        GrcDataSize::Byte2Unorm => {
            for (slot, v) in out.iter_mut().zip(value) {
                *slot = unorm8(v);
            }
        }
        GrcDataSize::Short2 => {
            for (c, v) in value.iter().take(2).enumerate() {
                let v = v.clamp(-32768.0, 32767.0).round() as i16;
                out[c * 2..c * 2 + 2].copy_from_slice(&v.to_le_bytes());
            }
        }
        GrcDataSize::Short4 => {
            for (c, v) in value.iter().enumerate() {
                let v = v.clamp(-32768.0, 32767.0).round() as i16;
                out[c * 2..c * 2 + 2].copy_from_slice(&v.to_le_bytes());
            }
        }
    }
}

/// Decode one channel's bytes into `[f32; 4]`.
pub fn decode(size: GrcDataSize, bytes: &[u8]) -> Option<[f32; 4]> {
    if bytes.len() < size.byte_size() {
        return None;
    }
    let f32_at =
        |i: usize| f32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let half_at = |i: usize| half_to_f32(u16::from_le_bytes([bytes[i], bytes[i + 1]]));
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let i16_at = |i: usize| i16::from_le_bytes([bytes[i], bytes[i + 1]]) as f32;

    let mut out = [0.0, 0.0, 0.0, 1.0];
    match size {
        GrcDataSize::Half => out[0] = half_at(0),
        GrcDataSize::Half2 => {
            out[0] = half_at(0);
            out[1] = half_at(2);
        }
        GrcDataSize::Half3 => {
            out[0] = half_at(0);
            out[1] = half_at(2);
            out[2] = half_at(4);
        }
        GrcDataSize::Half4 => {
            out = [half_at(0), half_at(2), half_at(4), half_at(6)];
        }
        GrcDataSize::Float => out[0] = f32_at(0),
        GrcDataSize::Float2 => {
            out[0] = f32_at(0);
            out[1] = f32_at(4);
        }
        GrcDataSize::Float3 => {
            out[0] = f32_at(0);
            out[1] = f32_at(4);
            out[2] = f32_at(8);
        }
        GrcDataSize::Float4 => out = [f32_at(0), f32_at(4), f32_at(8), f32_at(12)],
        // ⚠ `UByte4` is an index payload — bone indices in `Binding` — so it
        // keeps its integer value. `Color` is a normalised channel and does
        // not. Dividing the first by 255 silently renumbers every bone.
        GrcDataSize::UByte4 => {
            out = [
                bytes[0] as f32,
                bytes[1] as f32,
                bytes[2] as f32,
                bytes[3] as f32,
            ]
        }
        GrcDataSize::Color => {
            out = [
                bytes[0] as f32 / 255.0,
                bytes[1] as f32 / 255.0,
                bytes[2] as f32 / 255.0,
                bytes[3] as f32 / 255.0,
            ]
        }
        GrcDataSize::PackedNormal => {
            let raw = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            out[0] = dec3n(raw, 0);
            out[1] = dec3n(raw, 10);
            out[2] = dec3n(raw, 20);
        }
        GrcDataSize::ShortUnorm => out[0] = u16_at(0) as f32 / 65535.0,
        GrcDataSize::Short2Unorm => {
            out[0] = u16_at(0) as f32 / 65535.0;
            out[1] = u16_at(2) as f32 / 65535.0;
        }
        GrcDataSize::Byte2Unorm => {
            out[0] = bytes[0] as f32 / 255.0;
            out[1] = bytes[1] as f32 / 255.0;
        }
        GrcDataSize::Short2 => {
            out[0] = i16_at(0);
            out[1] = i16_at(2);
        }
        GrcDataSize::Short4 => out = [i16_at(0), i16_at(2), i16_at(4), i16_at(6)],
    }
    Some(out)
}

/// One 10-bit signed axis of a `PackedNormal`, sign-extended and normalised.
fn dec3n(raw: u32, shift: u32) -> f32 {
    let value = ((raw >> shift) & 0x3FF) as i32;
    let value = if value & 0x200 != 0 {
        value | !0x3FF
    } else {
        value
    };
    (value as f32 / 511.0).clamp(-1.0, 1.0)
}

/// How many element slots an Enhanced vertex declaration has.
pub const G9_ELEMENT_SLOTS: usize = 52;

/// The Legacy channel each Enhanced element slot carries, where one does.
pub const G9_SLOT_TO_GEN8_SEMANTIC: [Option<u8>; G9_ELEMENT_SLOTS] = {
    let mut map = [None; G9_ELEMENT_SLOTS];
    map[0] = Some(0);
    map[4] = Some(3);
    map[8] = Some(14);
    map[16] = Some(1);
    map[20] = Some(2);
    map[24] = Some(4);
    map[25] = Some(5);
    map[28] = Some(6);
    map[29] = Some(7);
    map[30] = Some(8);
    map[31] = Some(9);
    map[32] = Some(10);
    map[33] = Some(11);
    map
};

/// An Enhanced element's format code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VertexElementFormatG9 {
    #[default]
    None,
    R32G32B32A32Float,
    R32G32B32Float,
    R16G16B16A16Float,
    R32G32Float,
    R10G10B10A2,
    R8G8B8A8Unorm,
    R8G8B8A8Uint,
    R16G16Float,
    Unknown(u8),
}

impl VertexElementFormatG9 {
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::None,
            2 => Self::R32G32B32A32Float,
            6 => Self::R32G32B32Float,
            10 => Self::R16G16B16A16Float,
            16 => Self::R32G32Float,
            24 => Self::R10G10B10A2,
            28 => Self::R8G8B8A8Unorm,
            30 => Self::R8G8B8A8Uint,
            34 => Self::R16G16Float,
            other => Self::Unknown(other),
        }
    }

    pub fn to_raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::R32G32B32A32Float => 2,
            Self::R32G32B32Float => 6,
            Self::R16G16B16A16Float => 10,
            Self::R32G32Float => 16,
            Self::R10G10B10A2 => 24,
            Self::R8G8B8A8Unorm => 28,
            Self::R8G8B8A8Uint => 30,
            Self::R16G16Float => 34,
            Self::Unknown(raw) => raw,
        }
    }

    pub fn byte_size(self) -> Option<usize> {
        Some(match self {
            Self::None => 0,
            Self::R32G32B32A32Float => 16,
            Self::R32G32B32Float => 12,
            Self::R16G16B16A16Float => 8,
            Self::R32G32Float => 8,
            Self::R10G10B10A2 => 4,
            Self::R8G8B8A8Unorm => 4,
            Self::R8G8B8A8Uint => 4,
            Self::R16G16Float => 4,
            Self::Unknown(_) => return None,
        })
    }

    /// The Legacy encoding that reads the same bytes; `None` for a format with none.
    pub fn gen8_data_size(self) -> Option<GrcDataSize> {
        Some(match self {
            Self::R32G32B32A32Float => GrcDataSize::Float4,
            Self::R32G32B32Float => GrcDataSize::Float3,
            Self::R32G32Float => GrcDataSize::Float2,
            Self::R16G16Float => GrcDataSize::Half2,
            Self::R16G16B16A16Float => GrcDataSize::Half4,
            Self::R8G8B8A8Unorm => GrcDataSize::Color,
            Self::R8G8B8A8Uint => GrcDataSize::UByte4,
            Self::R10G10B10A2 => GrcDataSize::PackedNormal,
            Self::None | Self::Unknown(_) => return None,
        })
    }
}

/// One populated slot of an Enhanced vertex declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexElementG9 {
    pub slot: usize,
    pub offset: u32,
    /// The step between vertices; zero means the buffer's own stride.
    pub stride: u8,
    pub format: VertexElementFormatG9,
}

impl VertexElementG9 {
    pub fn gen8_semantic(self) -> Option<u8> {
        G9_SLOT_TO_GEN8_SEMANTIC.get(self.slot).copied().flatten()
    }
}

/// The populated slots of an Enhanced declaration's three tables, lowest slot first.
pub fn elements_g9(
    offsets: &[u32; G9_ELEMENT_SLOTS],
    sizes: &[u8; G9_ELEMENT_SLOTS],
    types: &[u8; G9_ELEMENT_SLOTS],
) -> Vec<VertexElementG9> {
    (0..G9_ELEMENT_SLOTS)
        .filter(|&slot| types[slot] != 0)
        .map(|slot| VertexElementG9 {
            slot,
            offset: offsets[slot],
            stride: sizes[slot],
            format: VertexElementFormatG9::from_raw(types[slot]),
        })
        .collect()
}

/// An axis-aligned box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Aabb {
    /// The sphere the engine writes beside a box: the box's centre, and the
    /// distance from it to a corner.
    ///
    /// A sphere over the vertices would be tighter, and is not what ships.
    /// `openrage-cli bounds` over a retail Legacy install: of 239,761
    /// drawables, 239,553 shipped centres and 239,590 shipped radii are this
    /// function of the drawable's own shipped box.
    pub fn bounding_sphere(&self) -> ([f32; 3], f32) {
        let centre = [0, 1, 2].map(|a| (self.min[a] + self.max[a]) * 0.5);
        let radius = (0..3)
            .map(|a| {
                let half = (self.max[a] - self.min[a]) * 0.5;
                half * half
            })
            .sum::<f32>()
            .sqrt();
        (centre, radius)
    }
}

/// An axis-aligned box grown a point at a time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AabbBuilder {
    min: [f32; 3],
    max: [f32; 3],
    any: bool,
}

impl Default for AabbBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AabbBuilder {
    pub const fn new() -> Self {
        Self {
            min: [f32::INFINITY; 3],
            max: [f32::NEG_INFINITY; 3],
            any: false,
        }
    }

    /// ⚠ A NaN coordinate is dropped rather than propagated. `min`/`max`
    /// against NaN keeps the incumbent on one operand order and takes the
    /// NaN on the other, so a single bad vertex otherwise decides the whole
    /// box by which way round the comparison was written.
    pub fn push(&mut self, point: [f32; 3]) {
        if point.iter().any(|v| v.is_nan()) {
            return;
        }
        for ((low, high), axis) in self.min.iter_mut().zip(self.max.iter_mut()).zip(point) {
            *low = low.min(axis);
            *high = high.max(axis);
        }
        self.any = true;
    }

    pub fn absorb(&mut self, box_: &Aabb) {
        self.push(box_.min);
        self.push(box_.max);
    }

    pub fn is_empty(&self) -> bool {
        !self.any
    }

    pub fn finish(&self) -> Option<Aabb> {
        self.any.then_some(Aabb {
            min: self.min,
            max: self.max,
        })
    }
}

/// Which slot of a model's boxes holds geometry `index`'s box.
///
/// ⚠ **A model with one geometry ships one box and no prefix.** Above one,
/// the engine prefixes the union of them all, so geometry `i` is at `i + 1`
/// and a recompute that writes one-per-geometry puts the first geometry's
/// box where the model's belongs.
pub fn aabb_slot_of(geometry_count: usize, index: usize) -> Option<usize> {
    if index >= geometry_count {
        return None;
    }
    Some(if geometry_count > 1 { index + 1 } else { 0 })
}

/// How many boxes a model of `geometry_count` geometries ships.
///
/// ⚠ Delegates to the pack's rule,
/// [`CountSource::PrefixedAboveOne`](crate::desc::CountSource::PrefixedAboveOne):
/// a second spelling of it is how the pack-driven rebuild came to write this
/// array one entry short for every multi-geometry model.
pub fn aabb_slots(geometry_count: usize) -> usize {
    crate::desc::CountSource::prefixed_above_one(geometry_count)
}

/// The whole array a model ships, in file order, from one box per geometry.
pub fn model_aabbs(per_geometry: Vec<Aabb>) -> Vec<Aabb> {
    if per_geometry.len() < 2 {
        return per_geometry;
    }
    let mut union = AabbBuilder::new();
    per_geometry.iter().for_each(|b| union.absorb(b));
    let Some(prefix) = union.finish() else {
        return per_geometry;
    };
    std::iter::once(prefix).chain(per_geometry).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Buffer {
        data: Vec<u8>,
        layout: Option<VertexLayout>,
        stride: usize,
        declared: usize,
    }

    impl VertexSource for Buffer {
        fn vertex_data(&self) -> &[u8] {
            &self.data
        }
        fn vertex_layout(&self) -> Option<VertexLayout> {
            self.layout
        }
        fn vertex_stride(&self) -> usize {
            self.stride
        }
        fn vertex_declared(&self) -> usize {
            self.declared
        }
    }

    fn layout(channels: &[(GrcFvfChannel, GrcDataSize)]) -> VertexLayout {
        let (mut fvf, mut sizes) = (0u32, 0u64);
        for (channel, size) in channels {
            fvf |= channel.mask();
            sizes |= (*size as u64) << (*channel as u64 * 4);
        }
        VertexLayout::of(fvf, sizes)
    }

    fn buffer(data: Vec<u8>, stride: usize, channels: &[(GrcFvfChannel, GrcDataSize)]) -> Buffer {
        Buffer {
            data,
            layout: Some(layout(channels)),
            stride,
            declared: 0,
        }
    }

    fn floats(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn every_nibble_names_an_encoding() {
        for (nibble, size) in GrcDataSize::ALL.into_iter().enumerate() {
            assert_eq!(size as usize, nibble, "{size:?} is not at its nibble");
            assert_eq!(GrcDataSize::from_nibble(nibble as u8), size);
            assert!(size.byte_size() > 0);
            assert!((1..=4).contains(&size.components()));
        }
    }

    #[test]
    fn a_channel_slot_is_its_own_discriminant() {
        for (slot, channel) in GrcFvfChannel::ALL.into_iter().enumerate() {
            assert_eq!(channel as usize, slot);
            assert_eq!(GrcFvfChannel::from_slot(slot as u8), Some(channel));
            assert_eq!(channel.mask(), 1 << slot);
        }
        assert_eq!(GrcFvfChannel::from_slot(16), None);
    }

    /// ⚠ The FVF lays channels out in discriminant order, not in the order
    /// they were declared. Declaring `Tex0` before `Normal` must not move
    /// either of them.
    #[test]
    fn a_layout_follows_discriminant_order_not_declaration_order() {
        let layout = layout(&[
            (GrcFvfChannel::Tex0, GrcDataSize::Float2),
            (GrcFvfChannel::Normal, GrcDataSize::Float3),
            (GrcFvfChannel::Position, GrcDataSize::Float3),
        ]);
        assert_eq!(layout.slot(GrcFvfChannel::Position).unwrap().offset, 0);
        assert_eq!(layout.slot(GrcFvfChannel::Normal).unwrap().offset, 12);
        assert_eq!(layout.slot(GrcFvfChannel::Tex0).unwrap().offset, 24);
        assert_eq!(layout.packed_stride(), 32);
        assert_eq!(layout.count(), 3);
        assert!(!layout.has(GrcFvfChannel::Tex1));
    }

    /// ⚠ A channel nobody asked about still costs its bytes.
    #[test]
    fn an_unwanted_channel_still_shifts_everything_after_it() {
        let with = layout(&[
            (GrcFvfChannel::Position, GrcDataSize::Float3),
            (GrcFvfChannel::Diffuse, GrcDataSize::Color),
            (GrcFvfChannel::Tex0, GrcDataSize::Float2),
        ]);
        let without = layout(&[
            (GrcFvfChannel::Position, GrcDataSize::Float3),
            (GrcFvfChannel::Tex0, GrcDataSize::Float2),
        ]);
        assert_eq!(with.slot(GrcFvfChannel::Tex0).unwrap().offset, 16);
        assert_eq!(without.slot(GrcFvfChannel::Tex0).unwrap().offset, 12);
    }

    #[test]
    fn every_encoding_decodes_and_pads_the_d3d_way() {
        for size in GrcDataSize::ALL {
            let bytes = vec![0u8; size.byte_size()];
            let decoded = decode(size, &bytes).unwrap_or_else(|| panic!("{size:?}"));
            assert_eq!(
                decoded[3],
                if size.components() == 4 { 0.0 } else { 1.0 },
                "{size:?} padded its fourth component wrongly"
            );
            assert!(decode(size, &bytes[..size.byte_size() - 1]).is_none());
        }
    }

    /// ⚠ `UByte4` carries bone indices and `Color` a normalised colour, in the same four bytes.
    #[test]
    fn ubyte4_keeps_its_integers_and_color_does_not() {
        let bytes = [0u8, 64, 128, 255];
        assert_eq!(
            decode(GrcDataSize::UByte4, &bytes).unwrap(),
            [0.0, 64.0, 128.0, 255.0]
        );
        let colour = decode(GrcDataSize::Color, &bytes).unwrap();
        assert!((colour[3] - 1.0).abs() < 1e-6);
        assert!((colour[1] - 64.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn a_packed_normal_recovers_the_axes_it_was_built_from() {
        let raw = 511u32 | ((-511i32 as u32 & 0x3FF) << 20);
        let out = decode(GrcDataSize::PackedNormal, &raw.to_le_bytes()).unwrap();
        assert!((out[0] - 1.0).abs() < 1e-6);
        assert!(out[1].abs() < 1e-6);
        assert!((out[2] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_reader_addresses_every_vertex_and_refuses_past_the_end() {
        let data = (0..3)
            .flat_map(|i| floats(&[i as f32, 0.0, 0.0, i as f32 * 0.5, 0.0]))
            .collect();
        let source = buffer(
            data,
            20,
            &[
                (GrcFvfChannel::Position, GrcDataSize::Float3),
                (GrcFvfChannel::Tex0, GrcDataSize::Float2),
            ],
        );
        let reader = VertexReader::new(&source).unwrap();
        assert_eq!(reader.count(), 3);
        assert_eq!(reader.stride(), 20);
        assert_eq!(reader.get(GrcFvfChannel::Position, 2).unwrap()[0], 2.0);
        assert_eq!(reader.get(GrcFvfChannel::Tex0, 2).unwrap()[0], 1.0);
        assert!(reader.get(GrcFvfChannel::Position, 3).is_none());
        assert!(reader.get(GrcFvfChannel::Tex7, 0).is_none());
    }

    /// ⚠ A declared stride above the packed one is padding, and addressing by
    /// the packed stride walks off alignment after vertex 0.
    #[test]
    fn a_padded_buffer_is_addressed_by_its_declared_stride() {
        let data = (0..2)
            .flat_map(|i| floats(&[i as f32, 0.0, 0.0, 0.0]))
            .collect();
        let source = buffer(data, 16, &[(GrcFvfChannel::Position, GrcDataSize::Float3)]);
        let reader = VertexReader::new(&source).unwrap();
        assert_eq!(reader.layout().packed_stride(), 12);
        assert_eq!(reader.stride(), 16);
        assert_eq!(reader.count(), 2);
        assert_eq!(reader.get(GrcFvfChannel::Position, 1).unwrap()[0], 1.0);
    }

    /// ⚠ A buffer that declares more vertices than it carries is bounded by its bytes.
    #[test]
    fn a_truncated_buffer_is_bounded_by_what_it_actually_holds() {
        let mut source = buffer(
            vec![0u8; 24],
            12,
            &[(GrcFvfChannel::Position, GrcDataSize::Float3)],
        );
        source.declared = 900;
        let reader = VertexReader::new(&source).unwrap();
        assert_eq!(reader.declared_count(), 900);
        assert_eq!(reader.count(), 2);
        assert!(reader.is_truncated());
        assert!(reader.get(GrcFvfChannel::Position, 2).is_none());
    }

    /// ⚠ A stride below the packed one leaves the last vertex's uv past the
    /// end while its position still reads.
    #[test]
    fn a_mesh_never_pads_a_vertex_the_bytes_do_not_hold() {
        let mut source = buffer(
            floats(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
            12,
            &[
                (GrcFvfChannel::Position, GrcDataSize::Float3),
                (GrcFvfChannel::Tex0, GrcDataSize::Float2),
            ],
        );
        source.declared = 900;
        let reader = VertexReader::new(&source).unwrap();
        let mesh = Mesh::decode(Some(reader), &[]);
        assert_eq!(mesh.positions.len(), reader.positions().count());
        assert_eq!(mesh.positions, [[0.0, 1.0, 2.0], [3.0, 4.0, 5.0]]);
        assert!(mesh.uvs.is_empty(), "{:?}", mesh.uvs);
    }

    #[test]
    fn a_buffer_without_a_layout_has_no_channels_to_read() {
        let mut source = buffer(vec![0u8; 64], 16, &[]);
        source.layout = None;
        assert!(VertexReader::new(&source).is_none());
        assert_eq!(
            Mesh::decode(VertexReader::new(&source), &[0, 1, 2]).indices,
            [0, 1, 2]
        );
    }

    #[test]
    fn every_encoding_writes_what_its_reader_returns() {
        for size in GrcDataSize::ALL {
            let value = match size {
                GrcDataSize::UByte4 | GrcDataSize::Short2 | GrcDataSize::Short4 => {
                    [1.0, 2.0, 3.0, 4.0]
                }
                GrcDataSize::Color
                | GrcDataSize::ShortUnorm
                | GrcDataSize::Short2Unorm
                | GrcDataSize::Byte2Unorm => [0.0, 1.0, 1.0, 0.0],
                GrcDataSize::PackedNormal => [1.0, 0.0, -1.0, 1.0],
                _ => [0.5, -0.25, 2.0, -4.0],
            };
            let mut bytes = vec![0u8; size.byte_size()];
            encode(size, value, &mut bytes);
            let back = decode(size, &bytes).unwrap();
            for c in 0..size.components() {
                assert!(
                    (back[c] - value[c]).abs() < 1e-5,
                    "{size:?} component {c}: wrote {} and read {}",
                    value[c],
                    back[c]
                );
            }
        }
    }

    /// ⚠ An Enhanced structure-of-arrays buffer keeps each channel in its own
    /// run: positions at 0 with stride 12, normals after all of them.
    #[test]
    fn an_enhanced_run_is_read_at_its_own_base_and_stride() {
        let (mut offsets, mut sizes, mut types) = ([0u32; 52], [0u8; 52], [0u8; 52]);
        types[0] = VertexElementFormatG9::R32G32B32Float.to_raw();
        sizes[0] = 12;
        types[4] = VertexElementFormatG9::R32G32B32Float.to_raw();
        offsets[4] = 24;
        sizes[4] = 12;
        let elements = elements_g9(&offsets, &sizes, &types);
        assert_eq!(elements.len(), 2);
        let source = Buffer {
            data: floats(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0]),
            layout: Some(VertexLayout::of_g9(&elements)),
            stride: 24,
            declared: 2,
        };
        let mesh = Mesh::decode(VertexReader::new(&source), &[0, 1, 1]);
        assert_eq!(mesh.positions, [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
        assert_eq!(mesh.normals, [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0]]);
        assert!(mesh.uvs.is_empty());
    }

    #[test]
    fn slot_map_matches_the_rage_semantic_order() {
        assert_eq!(G9_SLOT_TO_GEN8_SEMANTIC[0], Some(0));
        assert_eq!(G9_SLOT_TO_GEN8_SEMANTIC[4], Some(3));
        assert_eq!(G9_SLOT_TO_GEN8_SEMANTIC[8], Some(14));
        assert_eq!(G9_SLOT_TO_GEN8_SEMANTIC[16], Some(1));
        assert_eq!(G9_SLOT_TO_GEN8_SEMANTIC[20], Some(2));
        assert_eq!(G9_SLOT_TO_GEN8_SEMANTIC[33], Some(11));
        assert_eq!(G9_SLOT_TO_GEN8_SEMANTIC[1], None);
        assert_eq!(G9_SLOT_TO_GEN8_SEMANTIC[51], None);
    }

    #[test]
    fn every_known_format_reports_a_size_and_unknown_reports_none() {
        for raw in [2u8, 6, 10, 16, 24, 28, 30, 34] {
            let f = VertexElementFormatG9::from_raw(raw);
            assert!(f.byte_size().is_some(), "{f:?} needs a size");
            assert!(
                f.gen8_data_size().is_some(),
                "{f:?} needs a Legacy encoding"
            );
            assert_eq!(f.to_raw(), raw);
        }
        assert_eq!(VertexElementFormatG9::from_raw(99).byte_size(), None);
        assert_eq!(VertexElementFormatG9::from_raw(99).gen8_data_size(), None);
    }

    #[test]
    fn gen8_data_size_codes_match_grc_data_size() {
        use VertexElementFormatG9 as F;
        for (format, size) in [
            (F::R32G32B32Float, GrcDataSize::Float3),
            (F::R32G32B32A32Float, GrcDataSize::Float4),
            (F::R32G32Float, GrcDataSize::Float2),
            (F::R8G8B8A8Unorm, GrcDataSize::Color),
            // Bone indices: Gen8 stores them as a packed byte quad.
            (F::R8G8B8A8Uint, GrcDataSize::UByte4),
        ] {
            assert_eq!(format.gen8_data_size(), Some(size), "{format:?}");
        }
        assert_eq!(GrcDataSize::Float3 as u8, 6);
        assert_eq!(GrcDataSize::Color as u8, 9);
    }

    #[test]
    fn a_box_covers_every_point_and_drops_a_nan() {
        let mut builder = AabbBuilder::new();
        assert!(builder.finish().is_none());
        for p in [[1.0, -2.0, 3.0], [f32::NAN, 0.0, 0.0], [-4.0, 5.0, -6.0]] {
            builder.push(p);
        }
        let found = builder.finish().unwrap();
        assert_eq!(found.min, [-4.0, -2.0, -6.0]);
        assert_eq!(found.max, [1.0, 5.0, 3.0]);
    }

    /// ⚠ The single-geometry model is the whole reason this indexing needs a
    /// function: one geometry ships one box with no model prefix.
    #[test]
    fn one_geometry_ships_one_box_and_more_than_one_ships_a_prefix() {
        assert_eq!(aabb_slots(0), 0);
        assert_eq!(aabb_slots(1), 1);
        assert_eq!(aabb_slots(3), 4);
        assert_eq!(aabb_slot_of(1, 0), Some(0));
        assert_eq!(aabb_slot_of(1, 1), None);
        assert_eq!(aabb_slot_of(3, 0), Some(1));
        assert_eq!(aabb_slot_of(3, 2), Some(3));
        assert_eq!(aabb_slot_of(3, 3), None);

        let one = Aabb {
            min: [0.0; 3],
            max: [1.0; 3],
        };
        let two = Aabb {
            min: [-5.0, 0.0, 0.0],
            max: [0.0, 2.0, 0.0],
        };
        assert_eq!(model_aabbs(vec![one]), [one]);
        let both = model_aabbs(vec![one, two]);
        assert_eq!(both.len(), 3);
        assert_eq!((both[0].min[0], both[0].max[1]), (-5.0, 2.0));
        assert_eq!(&both[1..], [one, two]);
    }

    #[test]
    fn a_sphere_reaches_the_corner_of_its_box() {
        let (centre, radius) = Aabb {
            min: [-1.0, -2.0, -2.0],
            max: [1.0, 2.0, 2.0],
        }
        .bounding_sphere();
        assert_eq!(centre, [0.0, 0.0, 0.0]);
        assert!((radius - 3.0).abs() < 1e-6);
    }
}
