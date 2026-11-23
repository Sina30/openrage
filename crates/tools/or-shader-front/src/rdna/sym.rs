//! Where each scalar register's value came from, at every instruction: a user datum the pipeline
//! loads, or a word of a table a user pointer leads to. A program names its resources no other way.

use std::collections::BTreeMap;

use super::cfg::Cfg;
use super::decode::{Inst, Operand};

/// The scalar registers an operand field can name: `s0`..`s105`, then `vcc`, `ttmp`, `m0`, `exec`.
pub const SCALARS: usize = 128;
pub const VCC: u8 = 106;
pub const M0: u8 = 124;
pub const EXEC: u8 = 126;

/// A dword of a table: follow the pointer in user registers `root` and `root + 1`, then for each
/// offset of `path` the pointer read there, and read at `offset`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Loc {
    pub root: u8,
    pub path: Vec<u32>,
    pub offset: u32,
    /// Added to `offset` where the program indexes the table: a slot the pipeline or a constant buffer names.
    pub index: Option<Index>,
}

/// A table index: `source` shifted left by `shift`, the size of the table's entries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Index {
    pub source: IndexSource,
    pub shift: u8,
}

/// Where a table index comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IndexSource {
    /// The value the pipeline put in a user register.
    User(u8),
    /// A dword of the constant buffer whose descriptor is at `buffer`, at `offset` plus the user register `plus`.
    Data {
        buffer: Box<Loc>,
        offset: u32,
        plus: Option<u8>,
    },
}

/// A buffer descriptor the code builds around an address in user registers `root` and `root + 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory {
    pub root: u8,
    /// The element size an indexed read multiplies by, in bytes.
    pub stride: u32,
    /// The descriptor's fourth word: the format in bits 18:12, the component selection in bits 11:0.
    pub word3: u32,
}

impl Loc {
    pub fn plus(&self, bytes: u32) -> Self {
        Self {
            offset: self.offset.wrapping_add(bytes),
            ..self.clone()
        }
    }
}

impl std::fmt::Display for Loc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "s{}", self.root)?;
        for at in &self.path {
            write!(f, "->{at:#x}")?;
        }
        write!(f, "+{:#x}", self.offset)?;
        match &self.index {
            Some(Index {
                source: IndexSource::User(r),
                shift,
            }) => write!(f, "+(s{r}<<{shift})"),
            Some(Index {
                source: IndexSource::Data { buffer, offset, .. },
                shift,
            }) => write!(f, "+([{buffer}]+{offset:#x}<<{shift})"),
            None => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Sym {
    /// Computed by the program.
    Unknown,
    /// A table's word on one path here and something else on another: not a value to compute with.
    Mixed,
    /// The value the pipeline put in this register before the program ran.
    User(u8),
    Word(Loc),
    /// A constant the code wrote.
    Const(u32),
    /// A user register's value with constant bits set: a descriptor word built around an address.
    UserOr(u8, u32),
    /// A dword read from a constant buffer.
    Data {
        buffer: Loc,
        offset: u32,
        plus: Option<u8>,
    },
    /// A user datum or a constant buffer's dword shifted left: a table index.
    Scaled(Index),
}

pub type State = Vec<Sym>;

pub fn entry() -> State {
    (0..SCALARS as u8).map(Sym::User).collect()
}

/// The register an operand names, `None` for a constant or a status bit.
pub fn register(operand: Operand) -> Option<u8> {
    Some(match operand {
        Operand::Sgpr(n) => n,
        Operand::VccLo => VCC,
        Operand::VccHi => VCC + 1,
        Operand::Ttmp(n) => 108 + n,
        Operand::M0 => M0,
        Operand::ExecLo => EXEC,
        Operand::ExecHi => EXEC + 1,
        _ => return None,
    })
}

/// How many dwords a scalar or buffer load reads, from its mnemonic.
pub fn load_words(name: &str) -> Option<u8> {
    let rest = name
        .strip_prefix("s_load_dword")
        .or_else(|| name.strip_prefix("s_buffer_load_dword"))?;
    match rest {
        "" => Some(1),
        "x2" => Some(2),
        "x4" => Some(4),
        "x8" => Some(8),
        "x16" => Some(16),
        _ => None,
    }
}

/// Whether a scalar instruction's destination is a register pair.
pub fn wide(name: &str) -> bool {
    (name.ends_with("_b64") || name.ends_with("_i64") || name.ends_with("_u64"))
        && !name.contains("_i32_")
}

/// The table a pointer in registers `base` and `base + 1` leads to; its low half says which.
pub fn table(state: &State, base: u8) -> Option<Loc> {
    match state.get(usize::from(base))? {
        Sym::User(root) => Some(Loc {
            root: *root,
            path: Vec::new(),
            offset: 0,
            index: None,
        }),
        // A pointer read at an index leads to no one table.
        Sym::Word(at) if at.index.is_some() => None,
        Sym::Word(at) => {
            let mut path = at.path.clone();
            path.push(at.offset);
            Some(Loc {
                root: at.root,
                path,
                offset: 0,
                index: None,
            })
        }
        _ => None,
    }
}

/// The descriptor whose first dword register `first` holds, where the `words` registers from it
/// hold that table's consecutive dwords.
pub fn descriptor(state: &State, first: u8, words: u8) -> Option<Loc> {
    let Some(Sym::Word(at)) = state.get(usize::from(first)) else {
        return None;
    };
    (1..words)
        .all(|k| {
            state.get(usize::from(first) + usize::from(k))
                == Some(&Sym::Word(at.plus(4 * u32::from(k))))
        })
        .then(|| at.clone())
}

/// The four constant words of a descriptor the code wrote itself, such as a sampler's.
pub fn literal(state: &State, first: u8) -> Option<[u32; 4]> {
    let mut words = [0; 4];
    for (k, word) in words.iter_mut().enumerate() {
        let Some(Sym::Const(value)) = state.get(usize::from(first) + k) else {
            return None;
        };
        *word = *value;
    }
    Some(words)
}

/// The buffer descriptor in the four registers from `first` where the code built it around a user address.
pub fn memory(state: &State, first: u8) -> Option<Memory> {
    let at = usize::from(first);
    let Some(Sym::User(root)) = state.get(at) else {
        return None;
    };
    // The address's high half shares its dword with the stride, in bits 29:16.
    let high = match state.get(at + 1)? {
        Sym::User(r) if *r == root + 1 => 0,
        Sym::UserOr(r, bits) if *r == root + 1 => *bits,
        _ => return None,
    };
    let (Some(Sym::Const(_)), Some(Sym::Const(word3))) = (state.get(at + 2), state.get(at + 3))
    else {
        return None;
    };
    Some(Memory {
        root: *root,
        stride: (high >> 16) & 0x3FFF,
        word3: *word3,
    })
}

/// What a scalar source holds: a register's state, or the constant an operand is.
fn held(state: &State, operand: Operand) -> Sym {
    match (register(operand), operand) {
        (Some(r), _) => state.get(usize::from(r)).cloned().unwrap_or(Sym::Unknown),
        (None, Operand::Int(n)) => Sym::Const(n as u32),
        (None, Operand::Literal(word) | Operand::Float(word)) => Sym::Const(word),
        (None, Operand::Null) => Sym::Const(0),
        _ => Sym::Unknown,
    }
}

/// The table word a scalar load reads first: a table at a constant offset, with the index an offset register holds.
pub fn loaded(state: &State, base: u8, offset: u32, soffset: Operand) -> Option<Loc> {
    // A scalar load's immediate is 21 bits, signed, and truncated to a dword.
    if offset >> 20 != 0 {
        return None;
    }
    let index = match soffset {
        Operand::Null => None,
        _ => match held(state, soffset) {
            Sym::Scaled(index) => Some(index),
            Sym::User(r) => Some(Index {
                source: IndexSource::User(r),
                shift: 0,
            }),
            _ => return None,
        },
    };
    let at = table(state, base)?;
    Some(Loc {
        offset: offset & !3,
        index,
        ..at
    })
}

/// The dword a constant buffer read loads first, where its descriptor and its offset are known.
fn data(state: &State, base: u8, offset: u32, soffset: Operand) -> Option<Sym> {
    let buffer = descriptor(state, base, 4)?;
    let plus = match soffset {
        Operand::Null => None,
        _ => match held(state, soffset) {
            Sym::User(r) => Some(r),
            _ => return None,
        },
    };
    Some(Sym::Data {
        buffer,
        offset: offset & !3,
        plus,
    })
}

fn set(state: &mut State, register: u8, count: u8, sym: impl Fn(u8) -> Sym) {
    for k in 0..count {
        if let Some(slot) = state.get_mut(usize::from(register) + usize::from(k)) {
            *slot = sym(k);
        }
    }
}

/// `state` after `inst`.
pub fn step(state: &mut State, inst: &Inst) {
    let name = inst.name();
    let unknown = |_| Sym::Unknown;
    match inst {
        Inst::Sop1 { dst, src, .. } => {
            let Some(to) = register(*dst) else { return };
            let from = register(*src);
            match (name, from) {
                ("s_mov_b32", Some(from)) => {
                    state[usize::from(to)] = state[usize::from(from)].clone()
                }
                ("s_mov_b32", None) => state[usize::from(to)] = held(state, *src),
                ("s_mov_b64", None) => {
                    let low = held(state, *src);
                    // An inline integer is sign-extended; a literal word is not.
                    let high = match *src {
                        Operand::Int(n) if n < 0 => u32::MAX,
                        _ => 0,
                    };
                    set(state, to, 2, |k| match (&low, k) {
                        (Sym::Const(_), 0) => low.clone(),
                        (Sym::Const(_), _) => Sym::Const(high),
                        _ => Sym::Unknown,
                    });
                }
                ("s_mov_b64", Some(from)) => {
                    for k in 0..2 {
                        let value = state.get(usize::from(from) + k).cloned();
                        if let Some(slot) = state.get_mut(usize::from(to) + k) {
                            *slot = value.unwrap_or(Sym::Unknown);
                        }
                    }
                }
                _ => set(state, to, if wide(name) { 2 } else { 1 }, unknown),
            }
            if name.contains("exec") {
                set(state, EXEC, 2, unknown);
            }
        }
        Inst::Sop2 {
            dst, src0, src1, ..
        } => {
            let Some(to) = register(*dst) else { return };
            let (a, b) = (held(state, *src0), held(state, *src1));
            let value = match (name, a, b) {
                ("s_lshl_b32", Sym::User(r), Sym::Const(by)) if by < 32 => Sym::Scaled(Index {
                    source: IndexSource::User(r),
                    shift: by as u8,
                }),
                (
                    "s_lshl_b32",
                    Sym::Data {
                        buffer,
                        offset,
                        plus,
                    },
                    Sym::Const(by),
                ) if by < 32 => Sym::Scaled(Index {
                    source: IndexSource::Data {
                        buffer: Box::new(buffer),
                        offset,
                        plus,
                    },
                    shift: by as u8,
                }),
                ("s_or_b32", Sym::User(r), Sym::Const(bits))
                | ("s_or_b32", Sym::Const(bits), Sym::User(r)) => Sym::UserOr(r, bits),
                _ => Sym::Unknown,
            };
            set(state, to, if wide(name) { 2 } else { 1 }, |k| match k {
                0 => value.clone(),
                _ => Sym::Unknown,
            });
        }
        // A compare with a constant names the register it reads where the others name the one they write.
        Inst::Sopk { .. } if name.starts_with("s_cmpk_") => {}
        Inst::Sopk { dst, imm, .. } => {
            if let Some(to) = register(*dst) {
                let value = match name {
                    "s_movk_i32" => Sym::Const(i32::from(*imm) as u32),
                    _ => Sym::Unknown,
                };
                set(state, to, if wide(name) { 2 } else { 1 }, |_| value.clone());
            }
        }
        Inst::Smem {
            base,
            data: into,
            offset,
            soffset,
            ..
        } => {
            let (Some(to), Some(words)) = (register(*into), load_words(name)) else {
                return;
            };
            if name.starts_with("s_load_dword") {
                match loaded(state, *base, *offset, *soffset) {
                    Some(at) => set(state, to, words, |k| Sym::Word(at.plus(4 * u32::from(k)))),
                    None => set(state, to, words, unknown),
                }
                return;
            }
            match data(state, *base, *offset, *soffset) {
                Some(Sym::Data {
                    buffer,
                    offset,
                    plus,
                }) => set(state, to, words, |k| Sym::Data {
                    buffer: buffer.clone(),
                    offset: offset + 4 * u32::from(k),
                    plus,
                }),
                _ => set(state, to, words, unknown),
            }
        }
        // `v_readfirstlane_b32` writes a scalar register.
        Inst::Vop1 { dst, .. } if name == "v_readfirstlane_b32" => set(state, *dst, 1, unknown),
        Inst::Vopc { .. } => set(
            state,
            if name.starts_with("v_cmpx") {
                EXEC
            } else {
                VCC
            },
            2,
            unknown,
        ),
        Inst::Vop2 { .. } if name.contains("_co_") => set(state, VCC, 2, unknown),
        Inst::Vop3 {
            dst, opsel, abs, ..
        } => {
            if name.starts_with("v_cmpx") {
                set(state, EXEC, 2, unknown);
            } else if name.starts_with("v_cmp") || name == "v_readlane_b32" {
                let pair = if name == "v_readlane_b32" { 1 } else { 2 };
                set(state, *dst, pair, unknown);
            } else if name.contains("_co_")
                || name.starts_with("v_mad_u64")
                || name.starts_with("v_mad_i64")
            {
                // The carry destination sits in the modifier bits.
                set(state, (opsel << 3 | abs) & 0x7F, 2, unknown);
            }
        }
        _ => {}
    }
}

/// The state on entry to each block: what every path to it agrees on.
pub fn solve(cfg: &Cfg) -> BTreeMap<u32, State> {
    let mut at: BTreeMap<u32, State> = BTreeMap::from([(0, entry())]);
    let mut work = vec![0u32];
    while let Some(start) = work.pop() {
        let (Some(block), Some(mut state)) = (cfg.blocks.get(&start), at.get(&start).cloned())
        else {
            continue;
        };
        for (_, inst) in &block.insts {
            step(&mut state, inst);
        }
        for next in cfg.successors(start) {
            let changed = match at.get_mut(&next) {
                None => {
                    at.insert(next, state.clone());
                    true
                }
                Some(held) => {
                    let mut changed = false;
                    for (slot, new) in held.iter_mut().zip(&state) {
                        let table = |sym: &Sym| matches!(sym, Sym::Word(_) | Sym::Mixed);
                        let joined = match slot == new {
                            true => continue,
                            false if table(slot) || table(new) => Sym::Mixed,
                            false => Sym::Unknown,
                        };
                        if *slot != joined {
                            *slot = joined;
                            changed = true;
                        }
                    }
                    changed
                }
            };
            if changed {
                work.push(next);
            }
        }
    }
    at
}
