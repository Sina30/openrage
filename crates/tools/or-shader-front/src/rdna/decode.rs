//! The gfx10 instruction encodings: one instruction decoded from a stream of little-endian words,
//! its operands typed, and its disassembly.

use std::fmt;

use super::opcodes;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("word {pc} is past the code's {len} words")]
    OutOfRange { pc: usize, len: usize },
    #[error("the {family:?} instruction at word {pc} needs {need} words and the code ends first")]
    Truncated {
        pc: usize,
        family: Family,
        need: usize,
    },
}

/// The encoding families, told apart by an instruction's leading bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    Sop2,
    Sopk,
    Sop1,
    Sopc,
    Sopp,
    Smem,
    Vop2,
    Vop1,
    Vopc,
    Vop3,
    Vop3p,
    Vintrp,
    Exp,
    Ds,
    Mubuf,
    Mtbuf,
    Mimg,
    Flat,
    Global,
    Scratch,
}

/// An opcode number within its family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Op {
    pub family: Family,
    pub number: u16,
}

fn find(table: &'static [(u16, &'static str)], number: u16) -> Option<&'static str> {
    table.iter().find(|row| row.0 == number).map(|row| row.1)
}

fn find3(table: &'static [(u16, &'static str, u8)], number: u16) -> Option<(&'static str, u8)> {
    (table.iter().find(|row| row.0 == number)).map(|row| (row.1, row.2))
}

impl Op {
    /// The mnemonic and how many sources the three-source form reads; `None` for a number the family does not define.
    fn entry(self) -> Option<(&'static str, u8)> {
        let n = self.number;
        let one = |table| find(table, n).map(|name| (name, 0));
        match self.family {
            Family::Sop2 => one(opcodes::SOP2),
            Family::Sopk => one(opcodes::SOPK),
            Family::Sop1 => one(opcodes::SOP1),
            Family::Sopc => one(opcodes::SOPC),
            Family::Sopp => one(opcodes::SOPP),
            Family::Smem => one(opcodes::SMEM),
            Family::Vop2 => one(opcodes::VOP2),
            Family::Vop1 => one(opcodes::VOP1),
            Family::Vopc => one(opcodes::VOPC),
            Family::Vintrp => one(opcodes::VINTRP),
            Family::Ds => one(opcodes::DS),
            Family::Mubuf => one(opcodes::MUBUF),
            Family::Mtbuf => one(opcodes::MTBUF),
            Family::Mimg => one(opcodes::MIMG),
            Family::Flat => one(opcodes::FLAT),
            Family::Global => one(opcodes::GLOBAL),
            Family::Scratch => one(opcodes::SCRATCH),
            Family::Exp => (n == 0).then_some(("exp", 4)),
            Family::Vop3p => find3(opcodes::VOP3P, n),
            // The three-source family also carries the compares, then the two-source and the
            // one-source instructions, each at a base added to its own number.
            Family::Vop3 => find3(opcodes::VOP3, n).or_else(|| match n {
                0..=0xFF => find(opcodes::VOPC, n).map(|name| (name, 2)),
                0x100..=0x17F => find(opcodes::VOP2, n - 0x100).map(|name| {
                    let third = matches!(
                        name,
                        "v_cndmask_b32" | "v_addc_co_u32" | "v_subb_co_u32" | "v_subbrev_co_u32"
                    );
                    (name, if third { 3 } else { 2 })
                }),
                _ => find(opcodes::VOP1, n - 0x180).map(|name| (name, 1)),
            }),
        }
    }

    /// How many sources a three-source instruction reads, 0 where the family or the number does not say.
    pub fn sources(self) -> u8 {
        self.entry().map_or(0, |entry| entry.1)
    }

    /// The mnemonic, `None` for a number the family does not define.
    pub fn name(self) -> Option<&'static str> {
        self.entry().map(|entry| entry.0)
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => f.write_str(name),
            None => write!(
                f,
                "{}_{:#x}",
                format!("{:?}", self.family).to_lowercase(),
                self.number
            ),
        }
    }
}

/// A source or scalar destination, as the 9-bit operand field states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operand {
    Sgpr(u8),
    VccLo,
    VccHi,
    Ttmp(u8),
    M0,
    Null,
    ExecLo,
    ExecHi,
    Int(i32),
    /// An inline float constant, as its bits.
    Float(u32),
    /// The trailing word holds a per-operand selection (`sdwa`).
    Sdwa,
    /// The trailing word holds a lane permutation (`dpp`, `dpp8`, `dpp8` with fetch-inactive).
    Dpp,
    Dpp8,
    Dpp8Fi,
    /// The bounds of the shared and private address ranges, and the wave id an ordered pixel program releases.
    SharedBase,
    SharedLimit,
    PrivateBase,
    PrivateLimit,
    PopsExitingWaveId,
    Vccz,
    Execz,
    Scc,
    /// The 32-bit word that follows the instruction.
    Literal(u32),
    Vgpr(u8),
    /// A field value gfx10 gives no meaning.
    Reserved(u16),
}

impl Operand {
    fn of(code: u32) -> Self {
        match code {
            0..=105 => Self::Sgpr(code as u8),
            106 => Self::VccLo,
            107 => Self::VccHi,
            108..=123 => Self::Ttmp((code - 108) as u8),
            124 => Self::M0,
            125 => Self::Null,
            126 => Self::ExecLo,
            127 => Self::ExecHi,
            128..=192 => Self::Int(code as i32 - 128),
            193..=208 => Self::Int(192 - code as i32),
            233 => Self::Dpp8,
            234 => Self::Dpp8Fi,
            235 => Self::SharedBase,
            236 => Self::SharedLimit,
            237 => Self::PrivateBase,
            238 => Self::PrivateLimit,
            239 => Self::PopsExitingWaveId,
            240..=248 => Self::Float(
                [
                    0.5f32,
                    -0.5,
                    1.0,
                    -1.0,
                    2.0,
                    -2.0,
                    4.0,
                    -4.0,
                    0.5 * std::f32::consts::FRAC_1_PI,
                ][code as usize - 240]
                    .to_bits(),
            ),
            249 => Self::Sdwa,
            250 => Self::Dpp,
            251 => Self::Vccz,
            252 => Self::Execz,
            253 => Self::Scc,
            255 => Self::Literal(0),
            256..=511 => Self::Vgpr((code - 256) as u8),
            _ => Self::Reserved(code as u16),
        }
    }

    /// An 8-bit scalar field, where the `sdwa` and `dpp` codes mean nothing and take no word.
    fn scalar(code: u32) -> Self {
        match Self::of(code) {
            Self::Sdwa | Self::Dpp | Self::Dpp8 | Self::Dpp8Fi => Self::Reserved(code as u16),
            operand => operand,
        }
    }

    /// Whether the operand's value or controls are the word after the instruction.
    pub(super) fn takes_word(self) -> bool {
        matches!(
            self,
            Self::Literal(_) | Self::Sdwa | Self::Dpp | Self::Dpp8 | Self::Dpp8Fi
        )
    }

    fn with(self, word: Option<u32>) -> Self {
        match (self, word) {
            (Self::Literal(_), Some(word)) => Self::Literal(word),
            _ => self,
        }
    }
}

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Sgpr(n) => write!(f, "s{n}"),
            Self::VccLo => f.write_str("vcc_lo"),
            Self::VccHi => f.write_str("vcc_hi"),
            Self::Ttmp(n) => write!(f, "ttmp{n}"),
            Self::M0 => f.write_str("m0"),
            Self::Null => f.write_str("null"),
            Self::ExecLo => f.write_str("exec_lo"),
            Self::ExecHi => f.write_str("exec_hi"),
            Self::Int(n) => write!(f, "{n}"),
            Self::Float(bits) => write!(f, "{:?}", f32::from_bits(bits)),
            Self::Sdwa => f.write_str("sdwa"),
            Self::Dpp => f.write_str("dpp"),
            Self::Dpp8 => f.write_str("dpp8"),
            Self::Dpp8Fi => f.write_str("dpp8_fi"),
            Self::SharedBase => f.write_str("src_shared_base"),
            Self::SharedLimit => f.write_str("src_shared_limit"),
            Self::PrivateBase => f.write_str("src_private_base"),
            Self::PrivateLimit => f.write_str("src_private_limit"),
            Self::PopsExitingWaveId => f.write_str("src_pops_exiting_wave_id"),
            Self::Vccz => f.write_str("vccz"),
            Self::Execz => f.write_str("execz"),
            Self::Scc => f.write_str("scc"),
            Self::Literal(word) => write!(f, "{word:#x}"),
            Self::Vgpr(n) => write!(f, "v{n}"),
            Self::Reserved(code) => write!(f, "src_{code}"),
        }
    }
}

/// One instruction. A VGPR-only field is its register number; an SGPR base is the number of its first register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inst {
    Sop2 {
        op: Op,
        dst: Operand,
        src0: Operand,
        src1: Operand,
    },
    Sopk {
        op: Op,
        dst: Operand,
        imm: i16,
        /// The second word of `s_setreg_imm32_b32`.
        imm32: Option<u32>,
    },
    Sop1 {
        op: Op,
        dst: Operand,
        src: Operand,
    },
    Sopc {
        op: Op,
        src0: Operand,
        src1: Operand,
    },
    Sopp {
        op: Op,
        imm: u16,
    },
    Smem {
        op: Op,
        base: u8,
        data: Operand,
        offset: u32,
        soffset: Operand,
        glc: bool,
        dlc: bool,
    },
    Vop1 {
        op: Op,
        dst: u8,
        src: Operand,
        /// The trailing word: the literal, or the `sdwa` or `dpp` controls.
        extra: Option<u32>,
    },
    Vop2 {
        op: Op,
        dst: u8,
        src0: Operand,
        src1: u8,
        /// The trailing word: the literal, the constant of `v_madmk_f32` and `v_madak_f32`, or the `sdwa` or `dpp` controls.
        extra: Option<u32>,
    },
    Vopc {
        op: Op,
        src0: Operand,
        src1: u8,
        extra: Option<u32>,
    },
    // `dst` is a VGPR number, or an operand code where the instruction writes a mask; `abs` and
    // `opsel` are a scalar carry destination's bits in the forms that write one.
    Vop3 {
        op: Op,
        dst: u8,
        srcs: [Operand; 3],
        abs: u8,
        neg: u8,
        opsel: u8,
        clamp: bool,
        omod: u8,
    },
    Vop3p {
        op: Op,
        dst: u8,
        srcs: [Operand; 3],
        opsel_lo: u8,
        opsel_hi: u8,
        neg_lo: u8,
        neg_hi: u8,
        clamp: bool,
    },
    Vintrp {
        op: Op,
        dst: u8,
        src: u8,
        attr: u8,
        chan: u8,
    },
    Exp {
        target: u8,
        /// Bit `i` set: component `i` is written from `srcs[i]`.
        en: u8,
        srcs: [u8; 4],
        compr: bool,
        done: bool,
        vm: bool,
    },
    Ds {
        op: Op,
        addr: u8,
        data0: u8,
        data1: u8,
        dst: u8,
        offset0: u8,
        offset1: u8,
        gds: bool,
    },
    Mubuf {
        op: Op,
        addr: u8,
        data: u8,
        rsrc: u8,
        soffset: Operand,
        offset: u16,
        offen: bool,
        idxen: bool,
        glc: bool,
        dlc: bool,
        slc: bool,
        tfe: bool,
        lds: bool,
    },
    Mtbuf {
        op: Op,
        format: u8,
        addr: u8,
        data: u8,
        rsrc: u8,
        soffset: Operand,
        offset: u16,
        offen: bool,
        idxen: bool,
        glc: bool,
        dlc: bool,
        slc: bool,
        tfe: bool,
    },
    Mimg {
        op: Op,
        /// The address VGPR, then the registers of each extra address word, four a word.
        addrs: Vec<u8>,
        data: u8,
        rsrc: u8,
        samp: u8,
        dmask: u8,
        dim: u8,
        unorm: bool,
        glc: bool,
        dlc: bool,
        slc: bool,
        r128: bool,
        tfe: bool,
        lwe: bool,
        a16: bool,
        d16: bool,
    },
    /// The flat, global and scratch memory instructions, which share one layout.
    Flat {
        op: Op,
        addr: u8,
        data: u8,
        saddr: Operand,
        dst: u8,
        /// In bytes: 0 to 4095 for a flat address, signed for a global or a scratch one.
        offset: i16,
        glc: bool,
        slc: bool,
        dlc: bool,
        lds: bool,
        nv: bool,
    },
    /// A word whose leading bits are no gfx10 family: data among the code, or a decode that lost its place.
    Unknown {
        word: u32,
    },
}

fn bits(word: u32, low: u32, len: u32) -> u32 {
    (word >> low) & ((1 << len) - 1)
}

fn bit(word: u32, at: u32) -> bool {
    word >> at & 1 != 0
}

fn family_of(word: u32) -> Option<Family> {
    if word >> 31 == 0 {
        return Some(match bits(word, 25, 6) {
            0x3F => Family::Vop1,
            0x3E => Family::Vopc,
            _ => Family::Vop2,
        });
    }
    if word >> 30 == 0b10 {
        return Some(match word >> 23 {
            0x17D => Family::Sop1,
            0x17E => Family::Sopc,
            0x17F => Family::Sopp,
            _ if word >> 28 == 0xB => Family::Sopk,
            _ => Family::Sop2,
        });
    }
    Some(match word >> 26 {
        0x32 => Family::Vintrp,
        0x33 => Family::Vop3p,
        0x35 => Family::Vop3,
        0x36 => Family::Ds,
        0x37 => match bits(word, 14, 2) {
            1 => Family::Scratch,
            2 => Family::Global,
            _ => Family::Flat,
        },
        0x38 => Family::Mubuf,
        0x3A => Family::Mtbuf,
        0x3C => Family::Mimg,
        0x3D => Family::Smem,
        0x3E => Family::Exp,
        _ => return None,
    })
}

/// The instruction at word `pc` and how many words it takes.
pub fn decode_one(words: &[u32], pc: usize) -> Result<(Inst, usize), DecodeError> {
    let len = words.len();
    let w = *(words.get(pc)).ok_or(DecodeError::OutOfRange { pc, len })?;
    let Some(family) = family_of(w) else {
        return Ok((Inst::Unknown { word: w }, 1));
    };
    let at = |i: usize| {
        (pc.checked_add(i).and_then(|at| words.get(at)).copied()).ok_or(DecodeError::Truncated {
            pc,
            family,
            need: i + 1,
        })
    };
    let op = |number: u32| Op {
        family,
        number: number as u16,
    };
    // The word after a one-word instruction, where an operand calls for one.
    let trailing = |wanted: bool| wanted.then(|| at(1)).transpose();
    let sized = |inst: Inst, extra: Option<u32>| (inst, 1 + usize::from(extra.is_some()));
    Ok(match family {
        Family::Sop2 => {
            let (src0, src1) = (
                Operand::scalar(bits(w, 0, 8)),
                Operand::scalar(bits(w, 8, 8)),
            );
            let literal = trailing(src0.takes_word() || src1.takes_word())?;
            let inst = Inst::Sop2 {
                op: op(bits(w, 23, 7)),
                dst: Operand::of(bits(w, 16, 7)),
                src0: src0.with(literal),
                src1: src1.with(literal),
            };
            sized(inst, literal)
        }
        Family::Sopk => {
            let op = op(bits(w, 23, 5));
            let imm32 = trailing(op.name() == Some("s_setreg_imm32_b32"))?;
            let inst = Inst::Sopk {
                op,
                dst: Operand::of(bits(w, 16, 7)),
                imm: w as u16 as i16,
                imm32,
            };
            sized(inst, imm32)
        }
        Family::Sop1 => {
            let src = Operand::scalar(bits(w, 0, 8));
            let literal = trailing(src.takes_word())?;
            let inst = Inst::Sop1 {
                op: op(bits(w, 8, 8)),
                dst: Operand::of(bits(w, 16, 7)),
                src: src.with(literal),
            };
            sized(inst, literal)
        }
        Family::Sopc => {
            let (src0, src1) = (
                Operand::scalar(bits(w, 0, 8)),
                Operand::scalar(bits(w, 8, 8)),
            );
            let literal = trailing(src0.takes_word() || src1.takes_word())?;
            let inst = Inst::Sopc {
                op: op(bits(w, 16, 7)),
                src0: src0.with(literal),
                src1: src1.with(literal),
            };
            sized(inst, literal)
        }
        Family::Sopp => {
            let inst = Inst::Sopp {
                op: op(bits(w, 16, 7)),
                imm: w as u16,
            };
            (inst, 1)
        }
        Family::Vop2 => {
            let op = op(bits(w, 25, 6));
            let src0 = Operand::of(bits(w, 0, 9));
            let constant = matches!(
                op.name(),
                Some(
                    "v_madmk_f32"
                        | "v_madak_f32"
                        | "v_fmamk_f32"
                        | "v_fmaak_f32"
                        | "v_fmamk_f16"
                        | "v_fmaak_f16"
                )
            );
            let extra = trailing(src0.takes_word() || constant)?;
            let inst = Inst::Vop2 {
                op,
                dst: bits(w, 17, 8) as u8,
                src0: src0.with(extra),
                src1: bits(w, 9, 8) as u8,
                extra,
            };
            sized(inst, extra)
        }
        Family::Vop1 => {
            let src = Operand::of(bits(w, 0, 9));
            let extra = trailing(src.takes_word())?;
            let inst = Inst::Vop1 {
                op: op(bits(w, 9, 8)),
                dst: bits(w, 17, 8) as u8,
                src: src.with(extra),
                extra,
            };
            sized(inst, extra)
        }
        Family::Vopc => {
            let src0 = Operand::of(bits(w, 0, 9));
            let extra = trailing(src0.takes_word())?;
            let inst = Inst::Vopc {
                op: op(bits(w, 17, 8)),
                src0: src0.with(extra),
                src1: bits(w, 9, 8) as u8,
                extra,
            };
            sized(inst, extra)
        }
        Family::Vintrp => {
            let inst = Inst::Vintrp {
                op: op(bits(w, 16, 2)),
                dst: bits(w, 18, 8) as u8,
                src: w as u8,
                attr: bits(w, 10, 6) as u8,
                chan: bits(w, 8, 2) as u8,
            };
            (inst, 1)
        }
        Family::Vop3 | Family::Vop3p => {
            let x = at(1)?;
            let srcs = [0, 9, 18].map(|low| Operand::of(bits(x, low, 9)));
            let literal = match srcs.iter().any(|src| src.takes_word()) {
                true => Some(at(2)?),
                false => None,
            };
            let srcs = srcs.map(|src| src.with(literal));
            let inst = if family == Family::Vop3 {
                Inst::Vop3 {
                    op: op(bits(w, 16, 10)),
                    dst: w as u8,
                    srcs,
                    abs: bits(w, 8, 3) as u8,
                    neg: bits(x, 29, 3) as u8,
                    opsel: bits(w, 11, 4) as u8,
                    clamp: bit(w, 15),
                    omod: bits(x, 27, 2) as u8,
                }
            } else {
                Inst::Vop3p {
                    op: op(bits(w, 16, 7)),
                    dst: w as u8,
                    srcs,
                    opsel_lo: bits(w, 11, 3) as u8,
                    opsel_hi: (bits(x, 27, 2) | bits(w, 14, 1) << 2) as u8,
                    neg_lo: bits(x, 29, 3) as u8,
                    neg_hi: bits(w, 8, 3) as u8,
                    clamp: bit(w, 15),
                }
            };
            (inst, 2 + usize::from(literal.is_some()))
        }
        Family::Exp => {
            let x = at(1)?;
            let inst = Inst::Exp {
                target: bits(w, 4, 6) as u8,
                en: bits(w, 0, 4) as u8,
                srcs: x.to_le_bytes(),
                compr: bit(w, 10),
                done: bit(w, 11),
                vm: bit(w, 12),
            };
            (inst, 2)
        }
        Family::Ds => {
            let [addr, data0, data1, dst] = at(1)?.to_le_bytes();
            let inst = Inst::Ds {
                op: op(bits(w, 18, 8)),
                addr,
                data0,
                data1,
                dst,
                offset0: w as u8,
                offset1: (w >> 8) as u8,
                gds: bit(w, 17),
            };
            (inst, 2)
        }
        Family::Smem => {
            let x = at(1)?;
            let inst = Inst::Smem {
                op: op(bits(w, 18, 8)),
                base: (bits(w, 0, 6) * 2) as u8,
                data: Operand::of(bits(w, 6, 7)),
                offset: bits(x, 0, 21),
                soffset: Operand::of(bits(x, 25, 7)),
                glc: bit(w, 16),
                dlc: bit(w, 14),
            };
            (inst, 2)
        }
        Family::Mubuf => {
            let x = at(1)?;
            let inst = Inst::Mubuf {
                op: op(bits(w, 18, 7) | bits(w, 25, 1) << 7),
                addr: x as u8,
                data: (x >> 8) as u8,
                rsrc: (bits(x, 16, 5) * 4) as u8,
                soffset: Operand::scalar(bits(x, 24, 8)),
                offset: bits(w, 0, 12) as u16,
                offen: bit(w, 12),
                idxen: bit(w, 13),
                glc: bit(w, 14),
                dlc: bit(w, 15),
                slc: bit(x, 22),
                tfe: bit(x, 23),
                lds: bit(w, 16),
            };
            (inst, 2)
        }
        Family::Mtbuf => {
            let x = at(1)?;
            let inst = Inst::Mtbuf {
                op: op(bits(w, 16, 3) | bits(x, 21, 1) << 3),
                format: bits(w, 19, 7) as u8,
                addr: x as u8,
                data: (x >> 8) as u8,
                rsrc: (bits(x, 16, 5) * 4) as u8,
                soffset: Operand::scalar(bits(x, 24, 8)),
                offset: bits(w, 0, 12) as u16,
                offen: bit(w, 12),
                idxen: bit(w, 13),
                glc: bit(w, 14),
                dlc: bit(w, 15),
                slc: bit(x, 22),
                tfe: bit(x, 23),
            };
            (inst, 2)
        }
        Family::Mimg => {
            let x = at(1)?;
            let extra = bits(w, 1, 2) as usize;
            let mut addrs = vec![x as u8];
            for i in 0..extra {
                addrs.extend(at(2 + i)?.to_le_bytes());
            }
            let inst = Inst::Mimg {
                op: op(bits(w, 18, 7) | bits(w, 0, 1) << 7),
                addrs,
                data: (x >> 8) as u8,
                rsrc: (bits(x, 16, 5) * 4) as u8,
                samp: (bits(x, 21, 5) * 4) as u8,
                dmask: bits(w, 8, 4) as u8,
                dim: bits(w, 3, 3) as u8,
                unorm: bit(w, 12),
                glc: bit(w, 13),
                dlc: bit(w, 7),
                slc: bit(w, 25),
                r128: bit(w, 15),
                tfe: bit(w, 16),
                lwe: bit(w, 17),
                a16: bit(x, 30),
                d16: bit(x, 31),
            };
            (inst, 2 + extra)
        }
        Family::Flat | Family::Global | Family::Scratch => {
            let x = at(1)?;
            let inst = Inst::Flat {
                op: op(bits(w, 18, 7)),
                addr: x as u8,
                data: (x >> 8) as u8,
                saddr: Operand::of(bits(x, 16, 7)),
                dst: (x >> 24) as u8,
                offset: match family {
                    Family::Flat => bits(w, 0, 12) as i16,
                    _ => ((w << 20) as i32 >> 20) as i16,
                },
                glc: bit(w, 16),
                slc: bit(w, 17),
                dlc: bit(w, 12),
                lds: bit(w, 13),
                nv: bit(x, 23),
            };
            (inst, 2)
        }
    })
}

/// Every instruction of `words` in order, each with the word it starts at.
// ponytail: a linear sweep, so data among the code can throw it off; `Cfg::build` follows the
// control flow instead and is what a translation reads.
pub fn decode(words: &[u32]) -> Result<Vec<(u32, Inst)>, DecodeError> {
    let (mut pc, mut out) = (0, Vec::new());
    while pc < words.len() {
        let (inst, len) = decode_one(words, pc)?;
        out.push((pc as u32, inst));
        pc += len;
    }
    Ok(out)
}

impl Inst {
    /// The opcode, `None` for a word of no family.
    pub fn op(&self) -> Option<Op> {
        Some(match self {
            Self::Sop2 { op, .. }
            | Self::Sopk { op, .. }
            | Self::Sop1 { op, .. }
            | Self::Sopc { op, .. }
            | Self::Sopp { op, .. }
            | Self::Smem { op, .. }
            | Self::Vop1 { op, .. }
            | Self::Vop2 { op, .. }
            | Self::Vopc { op, .. }
            | Self::Vop3 { op, .. }
            | Self::Vop3p { op, .. }
            | Self::Vintrp { op, .. }
            | Self::Ds { op, .. }
            | Self::Mubuf { op, .. }
            | Self::Mtbuf { op, .. }
            | Self::Mimg { op, .. }
            | Self::Flat { op, .. } => *op,
            Self::Exp { .. } => Op {
                family: Family::Exp,
                number: 0,
            },
            Self::Unknown { .. } => return None,
        })
    }

    /// The mnemonic, empty for a word of no family or a number its family does not define.
    pub fn name(&self) -> &'static str {
        self.op().and_then(Op::name).unwrap_or("")
    }

    /// The opcode as a person reads it: the mnemonic, the family and number, or the raw word.
    pub fn op_word(&self) -> String {
        match (self.op(), self) {
            (Some(op), _) => op.to_string(),
            (None, Self::Unknown { word }) => format!(".long {word:#010x}"),
            (None, _) => String::new(),
        }
    }
}

/// An export target's name.
fn target(f: &mut fmt::Formatter<'_>, target: u8) -> fmt::Result {
    match target {
        0..=7 => write!(f, "mrt{target}"),
        8 => f.write_str("mrtz"),
        9 => f.write_str("null"),
        12..=16 => write!(f, "pos{}", target - 12),
        20 => f.write_str("prim"),
        32..=63 => write!(f, "param{}", target - 32),
        _ => write!(f, "target{target}"),
    }
}

fn flags(f: &mut fmt::Formatter<'_>, flags: &[(&str, bool)]) -> fmt::Result {
    for (name, _) in flags.iter().filter(|flag| flag.1) {
        write!(f, " {name}")?;
    }
    Ok(())
}

/// The disassembly, one line, in the assembler's operand order where this front end knows it.
impl fmt::Display for Inst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sop2 {
                op,
                dst,
                src0,
                src1,
            } => write!(f, "{op} {dst}, {src0}, {src1}"),
            Self::Sopk {
                op,
                dst,
                imm,
                imm32,
            } => {
                write!(f, "{op} {dst}, {imm:#x}")?;
                imm32.map_or(Ok(()), |word| write!(f, ", {word:#x}"))
            }
            Self::Sop1 { op, dst, src } => write!(f, "{op} {dst}, {src}"),
            Self::Sopc { op, src0, src1 } => write!(f, "{op} {src0}, {src1}"),
            Self::Sopp { op, imm } => match *imm != 0 || self.name().contains("branch") {
                true => write!(f, "{op} {imm:#x}"),
                false => write!(f, "{op}"),
            },
            Self::Smem {
                op,
                base,
                data,
                offset,
                soffset,
                glc,
                dlc,
            } => {
                write!(f, "{op} {data}, s{base}, {offset:#x}")?;
                if *soffset != Operand::Null {
                    write!(f, ", {soffset}")?;
                }
                flags(f, &[("glc", *glc), ("dlc", *dlc)])
            }
            Self::Vop1 {
                op,
                dst,
                src,
                extra,
            } => {
                write!(f, "{op} v{dst}, {src}")?;
                controls(f, *src, *extra)
            }
            Self::Vop2 {
                op,
                dst,
                src0,
                src1,
                extra,
            } => {
                write!(f, "{op} v{dst}, {src0}, v{src1}")?;
                match (src0.takes_word(), extra) {
                    (false, Some(word)) => write!(f, ", {word:#x}"),
                    _ => controls(f, *src0, *extra),
                }
            }
            Self::Vopc {
                op,
                src0,
                src1,
                extra,
            } => {
                write!(f, "{op} {src0}, v{src1}")?;
                controls(f, *src0, *extra)
            }
            Self::Vop3 {
                op,
                dst,
                srcs,
                abs,
                neg,
                opsel,
                clamp,
                omod,
            } => {
                let (name, count) = op.entry().unwrap_or(("", 3));
                match name.starts_with("v_cmp") {
                    true => write!(f, "{op} {}", Operand::of(u32::from(*dst)))?,
                    false => write!(f, "{op} v{dst}")?,
                }
                for (i, src) in srcs.iter().take(usize::from(count)).enumerate() {
                    let (minus, bar) = (
                        if neg >> i & 1 != 0 { "-" } else { "" },
                        if abs >> i & 1 != 0 { "|" } else { "" },
                    );
                    write!(f, ", {minus}{bar}{src}{bar}")?;
                }
                if *opsel != 0 {
                    write!(f, " op_sel:{opsel:#x}")?;
                }
                match omod {
                    0 => {}
                    1 => f.write_str(" mul:2")?,
                    2 => f.write_str(" mul:4")?,
                    _ => f.write_str(" div:2")?,
                }
                flags(f, &[("clamp", *clamp)])
            }
            Self::Vop3p {
                op,
                dst,
                srcs,
                opsel_lo,
                opsel_hi,
                neg_lo,
                neg_hi,
                clamp,
            } => {
                let count = op.entry().map_or(3, |entry| entry.1);
                write!(f, "{op} v{dst}")?;
                for src in srcs.iter().take(usize::from(count)) {
                    write!(f, ", {src}")?;
                }
                write!(
                    f,
                    " op_sel:{opsel_lo:#x} op_sel_hi:{opsel_hi:#x} neg_lo:{neg_lo:#x} neg_hi:{neg_hi:#x}"
                )?;
                flags(f, &[("clamp", *clamp)])
            }
            Self::Vintrp {
                op,
                dst,
                src,
                attr,
                chan,
            } => write!(
                f,
                "{op} v{dst}, v{src}, attr{attr}.{}",
                ["x", "y", "z", "w"][usize::from(*chan & 3)]
            ),
            Self::Exp {
                target: to,
                en,
                srcs,
                compr,
                done,
                vm,
            } => {
                f.write_str("exp ")?;
                target(f, *to)?;
                for (i, src) in srcs.iter().enumerate() {
                    match en >> i & 1 != 0 {
                        true => write!(f, ", v{src}")?,
                        false => f.write_str(", off")?,
                    }
                }
                flags(f, &[("done", *done), ("compr", *compr), ("vm", *vm)])
            }
            Self::Ds {
                op,
                addr,
                data0,
                data1,
                dst,
                offset0,
                offset1,
                gds,
            } => {
                write!(
                    f,
                    "{op} v{dst}, v{addr}, v{data0}, v{data1} offset0:{offset0} offset1:{offset1}"
                )?;
                flags(f, &[("gds", *gds)])
            }
            Self::Mubuf {
                op,
                addr,
                data,
                rsrc,
                soffset,
                offset,
                offen,
                idxen,
                glc,
                dlc,
                slc,
                tfe,
                lds,
            } => {
                write!(f, "{op} v{data}, v{addr}, s{rsrc}, {soffset}")?;
                flags(f, &[("idxen", *idxen), ("offen", *offen)])?;
                write!(f, " offset:{offset}")?;
                flags(
                    f,
                    &[
                        ("glc", *glc),
                        ("slc", *slc),
                        ("dlc", *dlc),
                        ("tfe", *tfe),
                        ("lds", *lds),
                    ],
                )
            }
            Self::Mtbuf {
                op,
                format,
                addr,
                data,
                rsrc,
                soffset,
                offset,
                offen,
                idxen,
                glc,
                dlc,
                slc,
                tfe,
            } => {
                write!(
                    f,
                    "{op} v{data}, v{addr}, s{rsrc}, {soffset} format:{format}"
                )?;
                flags(f, &[("idxen", *idxen), ("offen", *offen)])?;
                write!(f, " offset:{offset}")?;
                flags(
                    f,
                    &[("glc", *glc), ("slc", *slc), ("dlc", *dlc), ("tfe", *tfe)],
                )
            }
            Self::Mimg {
                op,
                addrs,
                data,
                rsrc,
                samp,
                dmask,
                dim,
                unorm,
                glc,
                dlc,
                slc,
                r128,
                tfe,
                lwe,
                a16,
                d16,
            } => {
                write!(f, "{op} v{data}, [")?;
                for (i, addr) in addrs.iter().enumerate() {
                    write!(f, "{}v{addr}", if i == 0 { "" } else { ", " })?;
                }
                write!(f, "], s{rsrc}, s{samp} dmask:{dmask:#x} dim:{dim}")?;
                flags(
                    f,
                    &[
                        ("unorm", *unorm),
                        ("glc", *glc),
                        ("slc", *slc),
                        ("dlc", *dlc),
                        ("r128", *r128),
                        ("tfe", *tfe),
                        ("lwe", *lwe),
                        ("a16", *a16),
                        ("d16", *d16),
                    ],
                )
            }
            Self::Flat {
                op,
                addr,
                data,
                saddr,
                dst,
                offset,
                glc,
                slc,
                dlc,
                lds,
                nv,
            } => {
                write!(f, "{op} v{dst}, v{addr}, v{data}, {saddr} offset:{offset}")?;
                flags(
                    f,
                    &[
                        ("glc", *glc),
                        ("slc", *slc),
                        ("dlc", *dlc),
                        ("lds", *lds),
                        ("nv", *nv),
                    ],
                )
            }
            Self::Unknown { word } => write!(f, ".long {word:#010x}"),
        }
    }
}

/// The trailing word of an `sdwa` or `dpp` form, raw.
fn controls(f: &mut fmt::Formatter<'_>, src: Operand, extra: Option<u32>) -> fmt::Result {
    match (src, extra) {
        (Operand::Sdwa | Operand::Dpp | Operand::Dpp8 | Operand::Dpp8Fi, Some(word)) => {
            write!(f, " {word:#010x}")
        }
        _ => Ok(()),
    }
}
