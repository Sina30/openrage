//! Buffer reads by address: raw dwords and typed elements, of a buffer a table names or one the
//! code describes itself around an address in its user registers.

use naga::{BinaryOperator as Bo, Expression as E, Handle, MathFunction as Mf, Scalar, TypeInner};

use super::{form, refused, Cx, Done, NotTranslated, ResourceKind, Value};
use crate::rdna::decode::{Inst, Operand};
use crate::rdna::sym::{self, Loc};

/// The dwords before the first element of a buffer that states its own stride.
const HEADER_WORDS: u32 = 4;
/// A descriptor's component selection that leaves the four components where they are.
const IDENTITY_SELECTION: u32 = 0xFAC;

/// How the bits of an element's component are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    Unorm,
    Snorm,
    Uscaled,
    Sscaled,
    Uint,
    Sint,
    Float,
}

/// The six integer readings a format family comes in, in the order of their codes.
const FAMILY: [Channel; 6] = [
    Channel::Unorm,
    Channel::Snorm,
    Channel::Uscaled,
    Channel::Sscaled,
    Channel::Uint,
    Channel::Sint,
];

/// An element format by its gfx10 code: each component's first bit and width, and how its bits are read.
/// The packed 10- and 11-bit float formats are not read.
fn element_format(code: u8) -> Option<(&'static [(u32, u32)], Channel)> {
    const B8: &[(u32, u32)] = &[(0, 8), (8, 8), (16, 8), (24, 8)];
    const B16: &[(u32, u32)] = &[(0, 16), (16, 16), (32, 16), (48, 16)];
    const B32: &[(u32, u32)] = &[(0, 32), (32, 32), (64, 32), (96, 32)];
    // A packed format's name lists its widths from the top bit down.
    const B10_2: &[(u32, u32)] = &[(0, 2), (2, 10), (12, 10), (22, 10)];
    const B2_10: &[(u32, u32)] = &[(0, 10), (10, 10), (20, 10), (30, 2)];
    let family = |first: u8| FAMILY[usize::from(code - first)];
    Some(match code {
        1..=6 => (&B8[..1], family(1)),
        7..=12 => (&B16[..1], family(7)),
        13 => (&B16[..1], Channel::Float),
        14..=19 => (&B8[..2], family(14)),
        20 => (&B32[..1], Channel::Uint),
        21 => (&B32[..1], Channel::Sint),
        22 => (&B32[..1], Channel::Float),
        23..=28 => (&B16[..2], family(23)),
        29 => (&B16[..2], Channel::Float),
        44..=49 => (B10_2, family(44)),
        50..=55 => (B2_10, family(50)),
        56..=61 => (B8, family(56)),
        62 => (&B32[..2], Channel::Uint),
        63 => (&B32[..2], Channel::Sint),
        64 => (&B32[..2], Channel::Float),
        65..=70 => (B16, family(65)),
        71 => (B16, Channel::Float),
        72 => (&B32[..3], Channel::Uint),
        73 => (&B32[..3], Channel::Sint),
        74 => (&B32[..3], Channel::Float),
        75 => (B32, Channel::Uint),
        76 => (B32, Channel::Sint),
        77 => (B32, Channel::Float),
        _ => return None,
    })
}

/// A buffer an instruction reads: its global, its element stride, the dwords before its first
/// element, and the fourth descriptor word where the code wrote it.
struct Bound {
    global: Handle<naga::GlobalVariable>,
    stride: Handle<E>,
    first: u32,
    word3: Option<u32>,
}

impl Cx<'_> {
    /// One dword of a buffer.
    fn buffer_word(&mut self, global: Handle<naga::GlobalVariable>, index: Handle<E>) -> Handle<E> {
        let pointer = self.global(global);
        let cell = self.b.emit(E::Access {
            base: pointer,
            index,
        });
        self.b.load(cell)
    }

    /// The buffer the four descriptor registers from `rsrc` name.
    fn buffer(&mut self, pc: u32, rsrc: u8) -> Result<Bound, NotTranslated> {
        let memory = sym::memory(&self.state, rsrc);
        let key = match (sym::descriptor(&self.state, rsrc, 4), memory) {
            (Some(at), _) => (at, None),
            (None, Some(memory)) => (
                Loc {
                    root: memory.root,
                    path: Vec::new(),
                    offset: 0,
                    index: None,
                },
                Some(memory.stride),
            ),
            (None, None) => {
                return Err(NotTranslated::Descriptor {
                    pc,
                    what: "a buffer whose descriptor it did not load from a table",
                })
            }
        };
        let global = match self.storage.get(&key) {
            Some(&global) => global,
            None => {
                let base = self.ty(TypeInner::Scalar(Scalar::U32));
                let inner = TypeInner::Array {
                    base,
                    size: naga::ArraySize::Dynamic,
                    stride: 4,
                };
                let space = naga::AddressSpace::Storage {
                    access: naga::StorageAccess::LOAD,
                };
                let kind = ResourceKind::Buffer { stride: key.1 };
                let name = format!("buf{}", self.storage.len());
                let global = self.bound(name, inner, space, kind, Some(key.0.clone()));
                self.storage.insert(key.clone(), global);
                global
            }
        };
        let (stride, first) = match key.1 {
            Some(stride) => (self.b.u32(stride), 0),
            None => {
                let zero = self.b.u32(0);
                (self.buffer_word(global, zero), HEADER_WORDS)
            }
        };
        Ok(Bound {
            global,
            stride,
            first,
            word3: memory.map(|memory| memory.word3),
        })
    }

    /// The number of the dword a buffer instruction reads first: the index times the stride, the
    /// offset register, the scalar offset and the immediate, in bytes, then as a dword past the header.
    // ponytail: an address that is no multiple of four reads the dword it falls in; no program was seen to pass one.
    fn buffer_address(
        &mut self,
        pc: u32,
        bound: &Bound,
        (addr, idxen, offen): (u8, bool, bool),
        soffset: Operand,
        offset: u16,
    ) -> Value {
        let mut bytes = self.b.u32(u32::from(offset));
        let mut register = addr;
        if idxen {
            let index = self.vreg(pc, register)?;
            let scaled = self.b.bin(Bo::Multiply, index, bound.stride);
            bytes = self.b.bin(Bo::Add, bytes, scaled);
            register = register.wrapping_add(1);
        }
        if offen {
            let by = self.vreg(pc, register)?;
            bytes = self.b.bin(Bo::Add, bytes, by);
        }
        if !matches!(soffset, Operand::Null | Operand::Int(0)) {
            let by = self.sv(pc, soffset)?;
            bytes = self.b.bin(Bo::Add, bytes, by);
        }
        let (two, first) = (self.b.u32(2), self.b.u32(bound.first));
        let word = self.b.bin(Bo::ShiftRight, bytes, two);
        Ok(self.b.bin(Bo::Add, word, first))
    }

    /// `width` bits from bit `first` of `word`, sign-extended where `signed`.
    pub(super) fn field(
        &mut self,
        word: Handle<E>,
        first: u32,
        width: u32,
        signed: bool,
    ) -> Handle<E> {
        if width == 32 {
            return word;
        }
        let up = self.b.u32(32 - first - width);
        let raised = self.b.bin(Bo::ShiftLeft, word, up);
        let down = self.b.u32(32 - width);
        self.b.shift(Bo::ShiftRight, signed, raised, down)
    }

    /// Reads `count` components of the element at dword `at` in the format `code` into the registers from `data`.
    fn typed(
        &mut self,
        pc: u32,
        inst: &Inst,
        bound: &Bound,
        at: Handle<E>,
        code: u8,
        (count, data): (usize, u8),
    ) -> Done {
        let Some((fields, channel)) = element_format(code) else {
            return Err(form(
                pc,
                inst,
                "an element format this front end does not read",
            ));
        };
        let Some(fields) = fields.get(..count) else {
            return Err(form(pc, inst, "more components than its format holds"));
        };
        let words = fields.iter().map(|(first, _)| first / 32 + 1).max();
        let mut loaded = Vec::with_capacity(4);
        for k in 0..words.unwrap_or(0) {
            let by = self.b.u32(k);
            let index = self.b.bin(Bo::Add, at, by);
            loaded.push(self.buffer_word(bound.global, index));
        }
        for (i, &(first, width)) in fields.iter().enumerate() {
            let word = loaded[(first / 32) as usize];
            let signed = matches!(channel, Channel::Snorm | Channel::Sscaled | Channel::Sint);
            let raw = self.field(word, first % 32, width, signed);
            let value = match channel {
                Channel::Uint | Channel::Sint => raw,
                Channel::Float if width == 32 => raw,
                Channel::Float => {
                    let half = self.half(raw, false);
                    self.b.bits(half)
                }
                Channel::Uscaled | Channel::Unorm => {
                    let mut x = self.b.convert(raw, naga::ScalarKind::Float);
                    if channel == Channel::Unorm {
                        let most = self.b.f32(((1u64 << width) - 1) as f32);
                        x = self.b.bin(Bo::Divide, x, most);
                    }
                    self.b.bits(x)
                }
                Channel::Sscaled | Channel::Snorm => {
                    let integer = self.b.sint(raw);
                    let mut x = self.b.convert(integer, naga::ScalarKind::Float);
                    if channel == Channel::Snorm {
                        let (most, least) = (
                            self.b.f32(((1u64 << (width - 1)) - 1) as f32),
                            self.b.f32(-1.0),
                        );
                        x = self.b.bin(Bo::Divide, x, most);
                        x = self.b.math(Mf::Max, &[x, least]);
                    }
                    self.b.bits(x)
                }
            };
            self.vput(data.wrapping_add(i as u8), value);
        }
        Ok(())
    }

    /// `buffer_load_dword*` and `buffer_load_format_*`; a store from a stage that cannot make one is dropped.
    pub(super) fn mubuf(&mut self, pc: u32, inst: &Inst, name: &str) -> Done {
        let Inst::Mubuf {
            addr,
            data,
            rsrc,
            soffset,
            offset,
            offen,
            idxen,
            ..
        } = inst
        else {
            return Err(refused(pc, inst));
        };
        if name.starts_with("buffer_store_") && !self.fragment {
            // ponytail: a vertex stage has no writable buffer on every backend, and what it stores
            // is read by another pass, never by this draw.
            self.note("a buffer store before the pixel stage, dropped");
            return Ok(());
        }
        let (raw, count) = match name {
            "buffer_load_dword" => (true, 1),
            "buffer_load_dwordx2" => (true, 2),
            "buffer_load_dwordx3" => (true, 3),
            "buffer_load_dwordx4" => (true, 4),
            "buffer_load_format_x" => (false, 1),
            "buffer_load_format_xy" => (false, 2),
            "buffer_load_format_xyz" => (false, 3),
            "buffer_load_format_xyzw" => (false, 4),
            _ => return Err(refused(pc, inst)),
        };
        let bound = self.buffer(pc, *rsrc)?;
        let at = self.buffer_address(pc, &bound, (*addr, *idxen, *offen), *soffset, *offset)?;
        if !raw {
            // The element's format is the descriptor's, which only a descriptor the code wrote states here.
            let Some(word3) = bound.word3 else {
                return Err(form(pc, inst, "a format only its descriptor states"));
            };
            if word3 & 0xFFF != IDENTITY_SELECTION {
                return Err(form(pc, inst, "a descriptor that reorders the components"));
            }
            let code = ((word3 >> 12) & 0x7F) as u8;
            return self.typed(pc, inst, &bound, at, code, (count, *data));
        }
        for k in 0..count {
            let by = self.b.u32(k as u32);
            let index = self.b.bin(Bo::Add, at, by);
            let value = self.buffer_word(bound.global, index);
            self.vput(data.wrapping_add(k as u8), value);
        }
        Ok(())
    }

    /// `tbuffer_load_format_*`: an element in the format the instruction states.
    pub(super) fn mtbuf(&mut self, pc: u32, inst: &Inst, name: &str) -> Done {
        let Inst::Mtbuf {
            format,
            addr,
            data,
            rsrc,
            soffset,
            offset,
            offen,
            idxen,
            ..
        } = inst
        else {
            return Err(refused(pc, inst));
        };
        let count = match name {
            "tbuffer_load_format_x" => 1,
            "tbuffer_load_format_xy" => 2,
            "tbuffer_load_format_xyz" => 3,
            "tbuffer_load_format_xyzw" => 4,
            _ => return Err(refused(pc, inst)),
        };
        let bound = self.buffer(pc, *rsrc)?;
        let at = self.buffer_address(pc, &bound, (*addr, *idxen, *offen), *soffset, *offset)?;
        self.typed(pc, inst, &bound, at, *format, (count, *data))
    }
}
