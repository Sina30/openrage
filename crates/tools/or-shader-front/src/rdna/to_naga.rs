//! A pixel or vertex program as a naga module: one lane, every register a 32-bit local, the graph
//! structured into ifs and loops, tables followed to the resources they name, and everything this
//! front end does not model refused by name.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::num::NonZeroU32;

use naga::{
    BinaryOperator as Bo, Block, DerivativeAxis, DerivativeControl, Expression as E, Handle,
    ImageClass, ImageDimension, Literal, MathFunction as Mf, RelationalFunction, SampleLevel,
    Scalar, ScalarKind, Span, Statement, TypeInner, UnaryOperator as Uo, VectorSize,
};

use super::cfg::{Cfg, Terminator};
use super::decode::{Inst, Operand};
use super::sym::{self, Loc, State, Sym, EXEC, VCC};
use super::Program;
use crate::dxil::ir::{BasicBlock, Body, Inst as IrInst, Op as IrOp};
use crate::dxil::structure::{structure, Node};
use crate::dxil::ShaderKind;

mod buffers;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotTranslated {
    #[error("{name} at word {pc} is not translated")]
    Opcode { pc: u32, name: String },
    #[error("{name} at word {pc} is not translated with {what}")]
    Form {
        pc: u32,
        name: String,
        what: &'static str,
    },
    #[error("{register} is read at word {pc} before the program writes it: a system value or a user datum this front end does not model")]
    LiveIn { pc: u32, register: String },
    #[error("the operand {operand} at word {pc} is not translated")]
    Operand { pc: u32, operand: String },
    #[error("the instruction at word {pc} reads {what}")]
    Descriptor { pc: u32, what: &'static str },
    #[error("the control flow is not structured: {0}")]
    Structure(String),
    #[error("the jump through a register at word {pc} is not followed")]
    Indirect { pc: u32 },
    #[error("the {0} stage is not translated")]
    Stage(String),
    #[error("a vertex program that exports no position")]
    NoPosition,
}

impl NotTranslated {
    /// The short stable word a census counts a refusal by: the opcode or the construct, without its place.
    pub fn name(&self) -> String {
        match self {
            Self::Opcode { name, .. } => name.clone(),
            Self::Form { name, what, .. } => format!("{name}: {what}"),
            Self::LiveIn { register, .. } => {
                let class: String = register
                    .chars()
                    .take_while(|c| !c.is_ascii_digit())
                    .collect();
                format!("live-in {class}")
            }
            Self::Operand { operand, .. } => format!("operand {operand}"),
            Self::Descriptor { what, .. } => (*what).to_owned(),
            Self::Structure(_) => "control flow".to_owned(),
            Self::Indirect { .. } => "indirect jump".to_owned(),
            Self::Stage(stage) => format!("{stage} stage"),
            Self::NoPosition => "no position".to_owned(),
        }
    }
}

/// What a resource of a translated module is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    /// The 32 dwords the pipeline loads into `s0`..`s31`, as eight rows of four.
    UserData,
    /// `rows` rows of four dwords.
    ConstantBuffer {
        rows: u32,
    },
    /// `dim` is the instruction's dimension field: 1 two-dimensional, 2 three-dimensional, 5 a two-dimensional array.
    Texture {
        dim: u8,
        compared: bool,
    },
    Sampler {
        comparison: bool,
    },
    /// Dwords read by address. With no `stride` the first dword is the element size an indexed read
    /// multiplies by and the elements start at the fifth; with one, the code stated it and they start at the first.
    Buffer {
        stride: Option<u32>,
    },
}

/// One binding of a translated module and the table word its descriptor starts at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub kind: ResourceKind,
    /// `None` for the user data, which no table holds, and for a descriptor the code wrote itself. A
    /// buffer with a stride is the memory the user registers `root` and `root + 1` point at.
    pub at: Option<Loc>,
    /// The four words of a sampler the code builds from constants.
    pub words: Option<[u32; 4]>,
    pub group: u32,
    pub binding: u32,
}

/// A program translated: the module, what it binds, and each way it departs from the hardware, counted.
#[derive(Debug, Clone)]
pub struct Lowered {
    pub module: naga::Module,
    pub resources: Vec<Resource>,
    pub approximations: BTreeMap<&'static str, u32>,
    /// The parameter slots a pixel module reads as 16-bit halves: the stage before it has to export those as halves.
    pub half_slots: BTreeSet<u8>,
}

/// What a lowering is told of the program's neighbours.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The parameter slots the pixel stage reads as 16-bit halves ([`Lowered::half_slots`]).
    pub half_slots: BTreeSet<u8>,
}

/// The first export target of each kind: the colour targets start at 0.
const MRT_COUNT: u8 = 8;
const MRTZ: u8 = 8;
const NULL_TARGET: u8 = 9;
const POS0: u8 = 12;
const PARAM0: u8 = 32;
/// The user registers the pipeline loads, and the rows of four they make.
const USER_REGISTERS: u8 = 32;
/// The rows a constant buffer read at a computed offset is declared with: 64 KiB, the most a uniform block is sure to hold.
const DYNAMIC_ROWS: u32 = 4096;
/// The fetch routine leaves component `c` of the input in register `r` here.
const FETCH_BASE: u32 = 4;
/// The location of a parameter's high halves, past every parameter's own.
// ponytail: a device that links fewer than 64 locations refuses the pipeline; pack the halves densely if one matters.
const HIGH_HALVES: u32 = 32;
/// The export target of a mesh program's primitive: three vertex numbers and a cull bit.
const PRIM: u8 = 20;
/// The threads of a mesh program's group, each of which may place one vertex and one primitive.
// ponytail: every mesh program of the build was reported to state 128 threads; a program's own count belongs in `Program`.
const MESH_THREADS: u32 = 128;
/// The lanes of the wave a mesh program was compiled for.
const WAVE: u32 = 64;
/// The first vector register that holds a pixel program's system values, after the barycentric pair.
const SYSTEM_BASE: u8 = 2;
const TAU: f32 = std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Reg {
    V(u8),
    S(u8),
    Scc,
    /// The condition block `0` ends on.
    Cond(u32),
    /// The block a dispatch loop runs next.
    Next,
}

impl Reg {
    fn name(self) -> String {
        match self {
            Self::V(n) => format!("v{n}"),
            Self::S(n) => match n {
                106 => "vcc_lo".to_owned(),
                107 => "vcc_hi".to_owned(),
                108..=123 => format!("ttmp{}", n - 108),
                124 => "m0".to_owned(),
                126 => "exec_lo".to_owned(),
                127 => "exec_hi".to_owned(),
                _ => format!("s{n}"),
            },
            Self::Scc => "scc".to_owned(),
            Self::Cond(block) => format!("cond{block}"),
            Self::Next => "next".to_owned(),
        }
    }
}

/// A function's expressions and statements as they are built, with the `Emit` of those not yet covered.
#[derive(Default)]
struct Builder {
    fun: naga::Function,
    blocks: Vec<Block>,
    pending: Option<(Handle<E>, Handle<E>)>,
}

impl Builder {
    fn new() -> Self {
        Self {
            blocks: vec![Block::new()],
            ..Self::default()
        }
    }

    fn emit(&mut self, expression: E) -> Handle<E> {
        // naga refuses an `Emit` of an expression in scope from the start.
        let covered = !expression.needs_pre_emit();
        if !covered {
            self.flush();
        }
        let handle = self.fun.expressions.append(expression, Span::UNDEFINED);
        if covered {
            self.pending = Some((self.pending.map_or(handle, |(first, _)| first), handle));
        }
        handle
    }

    fn flush(&mut self) {
        if let Some((first, last)) = self.pending.take() {
            if let Some(block) = self.blocks.last_mut() {
                block.push(
                    Statement::Emit(naga::Range::new_from_bounds(first, last)),
                    Span::UNDEFINED,
                );
            }
        }
    }

    fn push(&mut self, statement: Statement) {
        self.flush();
        if let Some(block) = self.blocks.last_mut() {
            block.push(statement, Span::UNDEFINED);
        }
    }

    fn open(&mut self) {
        self.flush();
        self.blocks.push(Block::new());
    }

    fn close(&mut self) -> Block {
        self.flush();
        self.blocks.pop().unwrap_or_default()
    }

    fn finish(mut self) -> naga::Function {
        self.flush();
        self.fun.body = self.blocks.drain(..).next().unwrap_or_default();
        self.fun
    }

    fn u32(&mut self, word: u32) -> Handle<E> {
        self.emit(E::Literal(Literal::U32(word)))
    }

    fn f32(&mut self, value: f32) -> Handle<E> {
        self.emit(E::Literal(Literal::F32(value)))
    }

    fn cast(&mut self, expr: Handle<E>, kind: ScalarKind) -> Handle<E> {
        self.emit(E::As {
            expr,
            kind,
            convert: None,
        })
    }

    /// A value converted, not reread: an integer to the nearest float, a float truncated and clamped.
    fn convert(&mut self, expr: Handle<E>, kind: ScalarKind) -> Handle<E> {
        self.emit(E::As {
            expr,
            kind,
            convert: Some(4),
        })
    }

    fn float(&mut self, bits: Handle<E>) -> Handle<E> {
        self.cast(bits, ScalarKind::Float)
    }

    fn bits(&mut self, value: Handle<E>) -> Handle<E> {
        self.cast(value, ScalarKind::Uint)
    }

    fn sint(&mut self, bits: Handle<E>) -> Handle<E> {
        self.cast(bits, ScalarKind::Sint)
    }

    fn bin(&mut self, op: Bo, left: Handle<E>, right: Handle<E>) -> Handle<E> {
        self.emit(E::Binary { op, left, right })
    }

    fn un(&mut self, op: Uo, expr: Handle<E>) -> Handle<E> {
        self.emit(E::Unary { op, expr })
    }

    fn math(&mut self, fun: Mf, args: &[Handle<E>]) -> Handle<E> {
        self.emit(E::Math {
            fun,
            arg: args[0],
            arg1: args.get(1).copied(),
            arg2: args.get(2).copied(),
            arg3: None,
        })
    }

    fn select(&mut self, condition: Handle<E>, accept: Handle<E>, reject: Handle<E>) -> Handle<E> {
        self.emit(E::Select {
            condition,
            accept,
            reject,
        })
    }

    fn index(&mut self, base: Handle<E>, index: u32) -> Handle<E> {
        self.emit(E::AccessIndex { base, index })
    }

    fn load(&mut self, pointer: Handle<E>) -> Handle<E> {
        self.emit(E::Load { pointer })
    }

    fn ne0(&mut self, value: Handle<E>) -> Handle<E> {
        let zero = self.u32(0);
        self.bin(Bo::NotEqual, value, zero)
    }

    /// A boolean as the dword 1 or 0.
    fn flag(&mut self, condition: Handle<E>) -> Handle<E> {
        let (one, zero) = (self.u32(1), self.u32(0));
        self.select(condition, one, zero)
    }

    fn not(&mut self, value: Handle<E>) -> Handle<E> {
        self.un(Uo::BitwiseNot, value)
    }

    fn lnot(&mut self, condition: Handle<E>) -> Handle<E> {
        self.un(Uo::LogicalNot, condition)
    }

    /// `left` shifted by the low five bits of `by`; `left` is read signed for an arithmetic shift.
    fn shift(&mut self, op: Bo, signed: bool, left: Handle<E>, by: Handle<E>) -> Handle<E> {
        let mask = self.u32(31);
        let by = self.bin(Bo::And, by, mask);
        if !signed {
            return self.bin(op, left, by);
        }
        let left = self.sint(left);
        let value = self.bin(op, left, by);
        self.bits(value)
    }

    /// The low `width` bits set, every bit for a width of 32 or more.
    fn low_bits(&mut self, width: Handle<E>) -> Handle<E> {
        let (one, limit, all) = (self.u32(1), self.u32(32), self.u32(u32::MAX));
        let shifted = self.shift(Bo::ShiftLeft, false, one, width);
        let mask = self.bin(Bo::Subtract, shifted, one);
        let whole = self.bin(Bo::GreaterEqual, width, limit);
        self.select(whole, all, mask)
    }

    /// `op` over the float readings of two patterns, as a pattern.
    fn fbin(&mut self, op: Bo, a: Handle<E>, b: Handle<E>) -> Handle<E> {
        let (a, b) = (self.float(a), self.float(b));
        let value = self.bin(op, a, b);
        self.bits(value)
    }

    /// `fun` over the float readings of `args`, as a pattern.
    fn fmath(&mut self, fun: Mf, args: &[Handle<E>]) -> Handle<E> {
        let args: Vec<_> = args.iter().map(|&arg| self.float(arg)).collect();
        let value = self.math(fun, &args);
        self.bits(value)
    }

    /// `a * b` over floats, with the legacy rule that a zero operand gives zero whatever the other is.
    fn mul_legacy(&mut self, a: Handle<E>, b: Handle<E>) -> Handle<E> {
        let zero = self.f32(0.0);
        let (za, zb) = (self.bin(Bo::Equal, a, zero), self.bin(Bo::Equal, b, zero));
        let either = self.bin(Bo::LogicalOr, za, zb);
        let product = self.bin(Bo::Multiply, a, b);
        self.select(either, zero, product)
    }

    /// `a * b + c` over float readings, the product rounded before the sum.
    fn mad(&mut self, legacy: bool, a: Handle<E>, b: Handle<E>, c: Handle<E>) -> Handle<E> {
        let (a, b, c) = (self.float(a), self.float(b), self.float(c));
        let product = match legacy {
            true => self.mul_legacy(a, b),
            false => self.bin(Bo::Multiply, a, b),
        };
        let value = self.bin(Bo::Add, product, c);
        self.bits(value)
    }

    /// The high dword of the 64-bit product of two dwords, by 16-bit limbs.
    fn mul_hi(&mut self, signed: bool, a: Handle<E>, b: Handle<E>) -> Handle<E> {
        let (sixteen, low) = (self.u32(16), self.u32(0xFFFF));
        let (al, ah) = (
            self.bin(Bo::And, a, low),
            self.bin(Bo::ShiftRight, a, sixteen),
        );
        let (bl, bh) = (
            self.bin(Bo::And, b, low),
            self.bin(Bo::ShiftRight, b, sixteen),
        );
        let (ll, lh) = (
            self.bin(Bo::Multiply, al, bl),
            self.bin(Bo::Multiply, al, bh),
        );
        let (hl, hh) = (
            self.bin(Bo::Multiply, ah, bl),
            self.bin(Bo::Multiply, ah, bh),
        );
        let carried = self.bin(Bo::ShiftRight, ll, sixteen);
        let (lh_low, hl_low) = (self.bin(Bo::And, lh, low), self.bin(Bo::And, hl, low));
        let cross = self.bin(Bo::Add, carried, lh_low);
        let cross = self.bin(Bo::Add, cross, hl_low);
        let carry = self.bin(Bo::ShiftRight, cross, sixteen);
        let (lh_high, hl_high) = (
            self.bin(Bo::ShiftRight, lh, sixteen),
            self.bin(Bo::ShiftRight, hl, sixteen),
        );
        let mut high = self.bin(Bo::Add, hh, lh_high);
        high = self.bin(Bo::Add, high, hl_high);
        high = self.bin(Bo::Add, high, carry);
        if signed {
            // Read unsigned, a negative operand counts the other one 2^32 times too many.
            let zero = self.u32(0);
            for (x, other) in [(a, b), (b, a)] {
                let top = self.u32(31);
                let sign = self.bin(Bo::ShiftRight, x, top);
                let negative = self.ne0(sign);
                let excess = self.select(negative, other, zero);
                high = self.bin(Bo::Subtract, high, excess);
            }
        }
        high
    }

    /// One of the ten bitwise functions the scalar group names, `None` for another word.
    fn logic(&mut self, kind: &str, a: Handle<E>, b: Handle<E>) -> Option<Handle<E>> {
        let (op, invert_a, invert_b, invert) = match kind {
            "and" => (Bo::And, false, false, false),
            "or" => (Bo::InclusiveOr, false, false, false),
            "xor" => (Bo::ExclusiveOr, false, false, false),
            "andn1" => (Bo::And, true, false, false),
            "andn2" => (Bo::And, false, true, false),
            "orn1" => (Bo::InclusiveOr, true, false, false),
            "orn2" => (Bo::InclusiveOr, false, true, false),
            "nand" => (Bo::And, false, false, true),
            "nor" => (Bo::InclusiveOr, false, false, true),
            "xnor" => (Bo::ExclusiveOr, false, false, true),
            _ => return None,
        };
        let a = if invert_a { self.not(a) } else { a };
        let b = if invert_b { self.not(b) } else { b };
        let value = self.bin(op, a, b);
        Some(if invert { self.not(value) } else { value })
    }

    /// An integer comparison by its word in a mnemonic.
    fn icmp(&mut self, rel: &str, signed: bool, a: Handle<E>, b: Handle<E>) -> Option<Handle<E>> {
        let op = match rel {
            "eq" => Bo::Equal,
            "lg" | "ne" => Bo::NotEqual,
            "gt" => Bo::Greater,
            "ge" => Bo::GreaterEqual,
            "lt" => Bo::Less,
            "le" => Bo::LessEqual,
            "f" => return Some(self.emit(E::Literal(Literal::Bool(false)))),
            "tru" | "t" => return Some(self.emit(E::Literal(Literal::Bool(true)))),
            _ => return None,
        };
        let (a, b) = match signed {
            true => (self.sint(a), self.sint(b)),
            false => (a, b),
        };
        Some(self.bin(op, a, b))
    }

    fn is_nan(&mut self, argument: Handle<E>) -> Handle<E> {
        self.emit(E::Relational {
            fun: RelationalFunction::IsNan,
            argument,
        })
    }

    /// A float comparison by its word in a mnemonic, over two patterns; the `n` forms are true of a NaN.
    fn fcmp(&mut self, rel: &str, a: Handle<E>, b: Handle<E>) -> Option<Handle<E>> {
        let (a, b) = (self.float(a), self.float(b));
        let (base, negate) = match rel.strip_prefix('n') {
            Some(base @ ("ge" | "lg" | "gt" | "le" | "eq" | "lt")) => (base, true),
            _ => (rel, false),
        };
        let value = match base {
            "lt" => self.bin(Bo::Less, a, b),
            "eq" => self.bin(Bo::Equal, a, b),
            "le" => self.bin(Bo::LessEqual, a, b),
            "gt" => self.bin(Bo::Greater, a, b),
            "ge" => self.bin(Bo::GreaterEqual, a, b),
            "lg" => {
                let (less, greater) = (self.bin(Bo::Less, a, b), self.bin(Bo::Greater, a, b));
                self.bin(Bo::LogicalOr, less, greater)
            }
            "o" | "u" => {
                let (na, nb) = (self.is_nan(a), self.is_nan(b));
                let unordered = self.bin(Bo::LogicalOr, na, nb);
                match base {
                    "u" => unordered,
                    _ => self.lnot(unordered),
                }
            }
            "f" => self.emit(E::Literal(Literal::Bool(false))),
            "tru" | "t" => self.emit(E::Literal(Literal::Bool(true))),
            _ => return None,
        };
        Some(if negate { self.lnot(value) } else { value })
    }

    /// Whether the float pattern `x` is of a class `mask` selects: bit 0 a signalling NaN, 1 a quiet
    /// one, 2 and 9 the infinities, 3 and 8 the normals, 4 and 7 the denormals, 5 and 6 the zeros.
    fn class(&mut self, x: Handle<E>, mask: Handle<E>) -> Handle<E> {
        let (s31, s23, s22) = (self.u32(31), self.u32(23), self.u32(22));
        let (one, nil, byte) = (self.u32(1), self.u32(0), self.u32(0xFF));
        let sign = self.bin(Bo::ShiftRight, x, s31);
        let negative = self.ne0(sign);
        let shifted = self.bin(Bo::ShiftRight, x, s23);
        let exponent = self.bin(Bo::And, shifted, byte);
        let low = self.u32(0x7F_FFFF);
        let mantissa = self.bin(Bo::And, x, low);
        // The class by sign, the negative one first.
        let mut classes = [nil; 4];
        for (class, pair) in classes.iter_mut().zip([(2, 9), (3, 8), (4, 7), (5, 6)]) {
            let (n, p) = (self.u32(pair.0), self.u32(pair.1));
            *class = self.select(negative, n, p);
        }
        let [infinity, normal, denormal, zero] = classes;
        let quiet = self.bin(Bo::ShiftRight, mantissa, s22);
        let nan = self.bin(Bo::And, quiet, one);
        let no_mantissa = self.bin(Bo::Equal, mantissa, nil);
        let top = self.bin(Bo::Equal, exponent, byte);
        let bottom = self.bin(Bo::Equal, exponent, nil);
        let special = self.select(no_mantissa, infinity, nan);
        let small = self.select(no_mantissa, zero, denormal);
        let finite = self.select(bottom, small, normal);
        let class = self.select(top, special, finite);
        let bit = self.bin(Bo::ShiftRight, mask, class);
        let bit = self.bin(Bo::And, bit, one);
        self.ne0(bit)
    }

    /// The pattern an arithmetic instruction writes from its sources' patterns, `None` for one not lowered.
    fn alu(&mut self, name: &str, s: &[Handle<E>]) -> Option<Handle<E>> {
        Some(match (name, s) {
            ("v_mov_b32", &[a]) => a,
            ("v_add_f32", &[a, b]) => self.fbin(Bo::Add, a, b),
            ("v_sub_f32", &[a, b]) => self.fbin(Bo::Subtract, a, b),
            ("v_subrev_f32", &[a, b]) => self.fbin(Bo::Subtract, b, a),
            ("v_mul_f32", &[a, b]) => self.fbin(Bo::Multiply, a, b),
            ("v_mul_legacy_f32", &[a, b]) => {
                let (a, b) = (self.float(a), self.float(b));
                let value = self.mul_legacy(a, b);
                self.bits(value)
            }
            // ponytail: naga's min and max leave a NaN operand's result open; the hardware returns the other operand.
            ("v_min_f32", &[a, b]) => self.fmath(Mf::Min, &[a, b]),
            ("v_max_f32", &[a, b]) => self.fmath(Mf::Max, &[a, b]),
            ("v_min3_f32", &[a, b, c]) => {
                let first = self.fmath(Mf::Min, &[a, b]);
                self.fmath(Mf::Min, &[first, c])
            }
            ("v_max3_f32", &[a, b, c]) => {
                let first = self.fmath(Mf::Max, &[a, b]);
                self.fmath(Mf::Max, &[first, c])
            }
            ("v_med3_f32", &[a, b, c]) => {
                let (low, high) = (self.fmath(Mf::Min, &[a, b]), self.fmath(Mf::Max, &[a, b]));
                let capped = self.fmath(Mf::Min, &[high, c]);
                self.fmath(Mf::Max, &[low, capped])
            }
            ("v_and_b32", &[a, b]) => self.bin(Bo::And, a, b),
            ("v_or_b32", &[a, b]) => self.bin(Bo::InclusiveOr, a, b),
            ("v_xor_b32", &[a, b]) => self.bin(Bo::ExclusiveOr, a, b),
            ("v_xnor_b32", &[a, b]) => {
                let value = self.bin(Bo::ExclusiveOr, a, b);
                self.not(value)
            }
            ("v_add_u32", &[a, b]) => self.bin(Bo::Add, a, b),
            ("v_sub_u32", &[a, b]) => self.bin(Bo::Subtract, a, b),
            ("v_subrev_u32", &[a, b]) => self.bin(Bo::Subtract, b, a),
            ("v_mul_lo_u32" | "v_mul_lo_i32", &[a, b]) => self.bin(Bo::Multiply, a, b),
            ("v_mad_u32_u24", &[a, b, c]) => {
                let low = self.u32(0xFF_FFFF);
                let (a, b) = (self.bin(Bo::And, a, low), self.bin(Bo::And, b, low));
                let product = self.bin(Bo::Multiply, a, b);
                self.bin(Bo::Add, product, c)
            }
            ("v_mul_hi_u32", &[a, b]) => self.mul_hi(false, a, b),
            ("v_mul_hi_i32", &[a, b]) => self.mul_hi(true, a, b),
            ("v_mul_u32_u24", &[a, b]) => {
                let low = self.u32(0xFF_FFFF);
                let (a, b) = (self.bin(Bo::And, a, low), self.bin(Bo::And, b, low));
                self.bin(Bo::Multiply, a, b)
            }
            ("v_mul_i32_i24", &[a, b]) => {
                let eight = self.u32(8);
                let mut wide = [a, b];
                for x in &mut wide {
                    let up = self.bin(Bo::ShiftLeft, *x, eight);
                    *x = self.shift(Bo::ShiftRight, true, up, eight);
                }
                self.bin(Bo::Multiply, wide[0], wide[1])
            }
            ("v_min_u32", &[a, b]) => self.math(Mf::Min, &[a, b]),
            ("v_max_u32", &[a, b]) => self.math(Mf::Max, &[a, b]),
            ("v_min_i32" | "v_max_i32", &[a, b]) => {
                let (a, b) = (self.sint(a), self.sint(b));
                let fun = if name == "v_min_i32" {
                    Mf::Min
                } else {
                    Mf::Max
                };
                let value = self.math(fun, &[a, b]);
                self.bits(value)
            }
            ("v_min3_u32", &[a, b, c]) => {
                let first = self.math(Mf::Min, &[a, b]);
                self.math(Mf::Min, &[first, c])
            }
            ("v_max3_u32", &[a, b, c]) => {
                let first = self.math(Mf::Max, &[a, b]);
                self.math(Mf::Max, &[first, c])
            }
            ("v_med3_u32", &[a, b, c]) => {
                let (low, high) = (self.math(Mf::Min, &[a, b]), self.math(Mf::Max, &[a, b]));
                let capped = self.math(Mf::Min, &[high, c]);
                self.math(Mf::Max, &[low, capped])
            }
            ("v_min3_i32" | "v_max3_i32" | "v_med3_i32", &[a, b, c]) => {
                let (a, b, c) = (self.sint(a), self.sint(b), self.sint(c));
                let value = match name {
                    "v_min3_i32" => {
                        let first = self.math(Mf::Min, &[a, b]);
                        self.math(Mf::Min, &[first, c])
                    }
                    "v_max3_i32" => {
                        let first = self.math(Mf::Max, &[a, b]);
                        self.math(Mf::Max, &[first, c])
                    }
                    _ => {
                        let (low, high) =
                            (self.math(Mf::Min, &[a, b]), self.math(Mf::Max, &[a, b]));
                        let capped = self.math(Mf::Min, &[high, c]);
                        self.math(Mf::Max, &[low, capped])
                    }
                };
                self.bits(value)
            }
            ("v_lshlrev_b32", &[a, b]) => self.shift(Bo::ShiftLeft, false, b, a),
            ("v_lshrrev_b32", &[a, b]) => self.shift(Bo::ShiftRight, false, b, a),
            ("v_ashrrev_i32", &[a, b]) => self.shift(Bo::ShiftRight, true, b, a),
            ("v_not_b32", &[a]) => self.not(a),
            ("v_bfrev_b32", &[a]) => self.math(Mf::ReverseBits, &[a]),
            ("v_ffbl_b32", &[a]) => self.math(Mf::FirstTrailingBit, &[a]),
            ("v_bfe_u32", &[a, offset, width]) => {
                let low = self.u32(31);
                let width = self.bin(Bo::And, width, low);
                let mask = self.low_bits(width);
                let shifted = self.shift(Bo::ShiftRight, false, a, offset);
                self.bin(Bo::And, shifted, mask)
            }
            ("v_bfi_b32", &[mask, a, b]) => {
                let (kept, inverse) = (self.bin(Bo::And, mask, a), self.not(mask));
                let rest = self.bin(Bo::And, inverse, b);
                self.bin(Bo::InclusiveOr, kept, rest)
            }
            ("v_lshl_add_u32", &[a, b, c]) => {
                let shifted = self.shift(Bo::ShiftLeft, false, a, b);
                self.bin(Bo::Add, shifted, c)
            }
            ("v_lshl_or_b32", &[a, b, c]) => {
                let shifted = self.shift(Bo::ShiftLeft, false, a, b);
                self.bin(Bo::InclusiveOr, shifted, c)
            }
            ("v_add3_u32", &[a, b, c]) => {
                let sum = self.bin(Bo::Add, a, b);
                self.bin(Bo::Add, sum, c)
            }
            ("v_and_or_b32", &[a, b, c]) => {
                let both = self.bin(Bo::And, a, b);
                self.bin(Bo::InclusiveOr, both, c)
            }
            ("v_or3_b32", &[a, b, c]) => {
                let either = self.bin(Bo::InclusiveOr, a, b);
                self.bin(Bo::InclusiveOr, either, c)
            }
            ("v_xor3_b32", &[a, b, c]) => {
                let either = self.bin(Bo::ExclusiveOr, a, b);
                self.bin(Bo::ExclusiveOr, either, c)
            }
            ("v_xad_u32", &[a, b, c]) => {
                let either = self.bin(Bo::ExclusiveOr, a, b);
                self.bin(Bo::Add, either, c)
            }
            ("v_fract_f32", &[a]) => {
                // The hardware returns the largest float below one where the subtraction rounds to one.
                let (x, below_one) = (self.float(a), self.f32(f32::from_bits(0x3F7F_FFFF)));
                let fraction = self.math(Mf::Fract, &[x]);
                let value = self.math(Mf::Min, &[fraction, below_one]);
                self.bits(value)
            }
            ("v_trunc_f32", &[a]) => self.fmath(Mf::Trunc, &[a]),
            ("v_ceil_f32", &[a]) => self.fmath(Mf::Ceil, &[a]),
            ("v_floor_f32", &[a]) => self.fmath(Mf::Floor, &[a]),
            ("v_rndne_f32", &[a]) => self.fmath(Mf::Round, &[a]),
            ("v_sqrt_f32", &[a]) => self.fmath(Mf::Sqrt, &[a]),
            ("v_rsq_f32", &[a]) => self.fmath(Mf::InverseSqrt, &[a]),
            ("v_exp_f32", &[a]) => self.fmath(Mf::Exp2, &[a]),
            ("v_log_f32", &[a]) => self.fmath(Mf::Log2, &[a]),
            ("v_sin_f32" | "v_cos_f32", &[a]) => {
                // The operand is in turns: reduced to half a turn either way, then scaled.
                let x = self.float(a);
                let whole = self.math(Mf::Round, &[x]);
                let part = self.bin(Bo::Subtract, x, whole);
                let tau = self.f32(TAU);
                let angle = self.bin(Bo::Multiply, part, tau);
                let fun = if name == "v_sin_f32" {
                    Mf::Sin
                } else {
                    Mf::Cos
                };
                let value = self.math(fun, &[angle]);
                self.bits(value)
            }
            ("v_rcp_f32" | "v_rcp_iflag_f32", &[a]) => {
                let one = self.u32(1f32.to_bits());
                self.fbin(Bo::Divide, one, a)
            }
            ("v_cvt_f32_i32", &[a]) => {
                let a = self.sint(a);
                let value = self.convert(a, ScalarKind::Float);
                self.bits(value)
            }
            ("v_cvt_f32_u32", &[a]) => {
                let value = self.convert(a, ScalarKind::Float);
                self.bits(value)
            }
            (
                "v_cvt_f32_ubyte0" | "v_cvt_f32_ubyte1" | "v_cvt_f32_ubyte2" | "v_cvt_f32_ubyte3",
                &[a],
            ) => {
                let byte = u32::from(name.as_bytes()[name.len() - 1] - b'0');
                let (by, low) = (self.u32(8 * byte), self.u32(0xFF));
                let shifted = self.bin(Bo::ShiftRight, a, by);
                let byte = self.bin(Bo::And, shifted, low);
                let value = self.convert(byte, ScalarKind::Float);
                self.bits(value)
            }
            // naga's conversion truncates, clamps to the integer's range and reads a NaN as zero, as the hardware does.
            ("v_cvt_u32_f32", &[a]) => {
                let x = self.float(a);
                self.convert(x, ScalarKind::Uint)
            }
            ("v_cvt_i32_f32" | "v_cvt_flr_i32_f32" | "v_cvt_rpi_i32_f32", &[a]) => {
                let mut x = self.float(a);
                if name == "v_cvt_rpi_i32_f32" {
                    let half = self.f32(0.5);
                    x = self.bin(Bo::Add, x, half);
                }
                if name != "v_cvt_i32_f32" {
                    x = self.math(Mf::Floor, &[x]);
                }
                let value = self.convert(x, ScalarKind::Sint);
                self.bits(value)
            }
            ("v_mad_f32", &[a, b, c]) => self.mad(false, a, b, c),
            ("v_mad_legacy_f32", &[a, b, c]) => self.mad(true, a, b, c),
            ("v_fma_f32", &[a, b, c]) => self.fmath(Mf::Fma, &[a, b, c]),
            _ => return None,
        })
    }
}

fn refused(pc: u32, inst: &Inst) -> NotTranslated {
    NotTranslated::Opcode {
        pc,
        name: inst.op_word(),
    }
}

fn form(pc: u32, inst: &Inst, what: &'static str) -> NotTranslated {
    NotTranslated::Form {
        pc,
        name: inst.op_word(),
        what,
    }
}

fn insert(module: &mut naga::Module, inner: TypeInner) -> Handle<naga::Type> {
    module
        .types
        .insert(naga::Type { name: None, inner }, Span::UNDEFINED)
}

fn vector(module: &mut naga::Module, size: VectorSize, scalar: Scalar) -> Handle<naga::Type> {
    insert(module, TypeInner::Vector { size, scalar })
}

fn size_of(count: usize) -> VectorSize {
    match count {
        2 => VectorSize::Bi,
        3 => VectorSize::Tri,
        _ => VectorSize::Quad,
    }
}

/// What a mesh program run as a vertex program keeps between the two runs of its body: the thread
/// it stands for, its group, and what that thread exported.
#[derive(Debug, Clone, Copy)]
struct Mesh {
    thread: Handle<naga::GlobalVariable>,
    group: Handle<naga::GlobalVariable>,
    prim: Handle<naga::GlobalVariable>,
    prim_set: Handle<naga::GlobalVariable>,
    pos_set: Handle<naga::GlobalVariable>,
}

/// The lanes of a quad a permutation reads, one selector a lane.
type Quad = [u8; 4];

/// A register that holds another's value as a neighbouring lane computed it.
#[derive(Debug, Clone, Copy)]
struct Permuted {
    source: u8,
    /// How many times the source had been written in this block when it was read.
    version: u32,
    quad: Quad,
}

/// What lowering one block left: its statements, what it reads before writing, and what it writes.
struct Lowering {
    block: Block,
    used: BTreeMap<Reg, u32>,
    defined: BTreeSet<Reg>,
    /// The registers still holding a neighbouring lane's value when the block ends.
    permuted: BTreeSet<u8>,
}

struct Cx<'a> {
    program: &'a Program,
    module: naga::Module,
    b: Builder,
    fragment: bool,
    /// Whether any instruction changes the lane mask: then every vector write is kept to an active lane.
    masked: bool,
    locals: HashMap<Reg, Handle<E>>,
    /// The block being lowered: its index, where each scalar register's value came from, and its sets.
    index: u32,
    state: State,
    used: BTreeMap<Reg, u32>,
    defined: BTreeSet<Reg>,
    versions: HashMap<u8, u32>,
    permuted: HashMap<u8, Permuted>,
    mesh: Option<Mesh>,
    half_slots: &'a BTreeSet<u8>,
    /// A pixel input by its parameter slot: the private variable, and whether anything interpolates it.
    attributes: BTreeMap<u8, (Handle<naga::GlobalVariable>, bool)>,
    /// A pixel input read as 16-bit halves, by its parameter slot and whether the halves are the high ones.
    halves: BTreeMap<(u8, bool), Handle<naga::GlobalVariable>>,
    /// The pixel's position and whether its triangle faces front, where the program reads them.
    frag_coord: Option<Handle<naga::GlobalVariable>>,
    front_face: Option<Handle<naga::GlobalVariable>>,
    /// A vertex input by its register, with the scalar the signature gives it.
    fetched: BTreeMap<u32, (Handle<naga::GlobalVariable>, Scalar)>,
    vertex_index: Option<Handle<naga::GlobalVariable>>,
    /// An export target's private `vec4`, and the depth.
    exports: BTreeMap<u8, Handle<naga::GlobalVariable>>,
    depth: Option<Handle<naga::GlobalVariable>>,
    user_data: Option<Handle<naga::GlobalVariable>>,
    /// A constant buffer by its descriptor's place, with the rows it is read to.
    buffers: BTreeMap<Loc, (Handle<naga::GlobalVariable>, u32)>,
    textures: BTreeMap<(Loc, u8, bool), Handle<naga::GlobalVariable>>,
    /// A sampler by its descriptor's place or its constant words, and whether it compares.
    samplers: BTreeMap<SamplerKey, Handle<naga::GlobalVariable>>,
    /// A buffer read by address, by its descriptor's place and the stride the code states.
    storage: BTreeMap<(Loc, Option<u32>), Handle<naga::GlobalVariable>>,
    resources: Vec<Resource>,
    notes: BTreeMap<&'static str, u32>,
}

/// A sampler's descriptor place or its constant words, and whether it compares.
type SamplerKey = (Option<Loc>, Option<[u32; 4]>, bool);
type Done = Result<(), NotTranslated>;
type Value = Result<Handle<E>, NotTranslated>;

impl<'a> Cx<'a> {
    fn note(&mut self, what: &'static str) {
        *self.notes.entry(what).or_default() += 1;
    }

    fn ty(&mut self, inner: TypeInner) -> Handle<naga::Type> {
        insert(&mut self.module, inner)
    }

    fn private(&mut self, name: String, ty: Handle<naga::Type>) -> Handle<naga::GlobalVariable> {
        let global = naga::GlobalVariable {
            name: Some(name),
            space: naga::AddressSpace::Private,
            binding: None,
            ty,
            init: None,
            memory_decorations: naga::MemoryDecorations::empty(),
        };
        self.module.global_variables.append(global, Span::UNDEFINED)
    }

    /// A bound global at the next binding of group 0, listed among the resources.
    fn bound(
        &mut self,
        name: String,
        inner: TypeInner,
        space: naga::AddressSpace,
        kind: ResourceKind,
        at: Option<Loc>,
    ) -> Handle<naga::GlobalVariable> {
        let ty = self.ty(inner);
        let binding = self.resources.len() as u32;
        self.resources.push(Resource {
            kind,
            at,
            words: None,
            group: 0,
            binding,
        });
        let global = naga::GlobalVariable {
            name: Some(name),
            space,
            binding: Some(naga::ResourceBinding { group: 0, binding }),
            ty,
            init: None,
            memory_decorations: naga::MemoryDecorations::empty(),
        };
        self.module.global_variables.append(global, Span::UNDEFINED)
    }

    fn rows(&mut self, rows: u32) -> TypeInner {
        let base = vector(&mut self.module, VectorSize::Quad, Scalar::U32);
        TypeInner::Array {
            base,
            size: naga::ArraySize::Constant(NonZeroU32::new(rows).unwrap_or(NonZeroU32::MIN)),
            stride: 16,
        }
    }

    fn global(&mut self, global: Handle<naga::GlobalVariable>) -> Handle<E> {
        self.b.emit(E::GlobalVariable(global))
    }

    /// The pointer to a register's local, made on its first use.
    fn local(&mut self, reg: Reg) -> Handle<E> {
        if let Some(&pointer) = self.locals.get(&reg) {
            return pointer;
        }
        let scalar = match reg {
            Reg::Cond(_) => Scalar::BOOL,
            _ => Scalar::U32,
        };
        let ty = self.ty(TypeInner::Scalar(scalar));
        let variable = naga::LocalVariable {
            name: Some(reg.name()),
            ty,
            init: None,
        };
        let variable = (self.b.fun.local_variables).append(variable, Span::UNDEFINED);
        let pointer = self.b.emit(E::LocalVariable(variable));
        self.locals.insert(reg, pointer);
        pointer
    }

    /// A register's value, with no word on whether the program wrote it.
    fn raw(&mut self, reg: Reg) -> Handle<E> {
        let pointer = self.local(reg);
        self.b.load(pointer)
    }

    fn get(&mut self, pc: u32, reg: Reg) -> Handle<E> {
        if !self.defined.contains(&reg) {
            self.used.entry(reg).or_insert(pc);
        }
        self.raw(reg)
    }

    fn put(&mut self, reg: Reg, value: Handle<E>) {
        let pointer = self.local(reg);
        self.b.push(Statement::Store { pointer, value });
        self.defined.insert(reg);
    }

    fn vreg(&mut self, pc: u32, n: u8) -> Value {
        if self.permuted.contains_key(&n) {
            return Err(NotTranslated::Form {
                pc,
                name: "ds_swizzle_b32".to_owned(),
                what:
                    "a neighbouring lane's value read other than by the subtraction of a derivative",
            });
        }
        Ok(self.get(pc, Reg::V(n)))
    }

    fn sreg(&mut self, pc: u32, r: u8) -> Value {
        match self.state.get(usize::from(r)) {
            Some(Sym::Word(_) | Sym::Mixed) => Err(NotTranslated::Descriptor {
                pc,
                what: "a table's word as a number",
            }),
            _ => Ok(self.get(pc, Reg::S(r))),
        }
    }

    /// Whether this lane runs: bit 0 of the lane mask.
    fn active(&mut self) -> Handle<E> {
        let exec = self.raw(Reg::S(EXEC));
        let one = self.b.u32(1);
        let bit = self.b.bin(Bo::And, exec, one);
        self.b.ne0(bit)
    }

    /// Writes a vector register, in an active lane only where the program masks lanes.
    fn vput(&mut self, n: u8, value: Handle<E>) {
        let value = match self.masked {
            true => {
                let (active, old) = (self.active(), self.raw(Reg::V(n)));
                self.b.select(active, value, old)
            }
            false => value,
        };
        self.put(Reg::V(n), value);
        *self.versions.entry(n).or_default() += 1;
        self.permuted.remove(&n);
    }

    /// A scalar source's dword.
    fn sv(&mut self, pc: u32, operand: Operand) -> Value {
        if let Some(r) = sym::register(operand) {
            return self.sreg(pc, r);
        }
        Ok(match operand {
            Operand::Null => self.b.u32(0),
            Operand::Int(n) => self.b.u32(n as u32),
            Operand::Float(word) | Operand::Literal(word) => self.b.u32(word),
            Operand::Scc => self.get(pc, Reg::Scc),
            Operand::Vccz | Operand::Execz => {
                let base = if operand == Operand::Vccz { VCC } else { EXEC };
                let any = self.mask_any(pc, base)?;
                let none = self.b.lnot(any);
                self.b.flag(none)
            }
            Operand::Vgpr(n) => self.vreg(pc, n)?,
            _ => {
                return Err(NotTranslated::Operand {
                    pc,
                    operand: operand.to_string(),
                })
            }
        })
    }

    /// A 64-bit scalar source as its two dwords: a register pair, or a constant extended.
    fn sv64(&mut self, pc: u32, operand: Operand) -> Result<[Handle<E>; 2], NotTranslated> {
        if let Some(r) = sym::register(operand) {
            return Ok([self.sreg(pc, r)?, self.sreg(pc, r + 1)?]);
        }
        Ok(match operand {
            Operand::Null => [self.b.u32(0), self.b.u32(0)],
            Operand::Int(n) => [self.b.u32(n as u32), self.b.u32((n >> 31) as u32)],
            Operand::Literal(word) => [self.b.u32(word), self.b.u32(0)],
            _ => {
                return Err(NotTranslated::Operand {
                    pc,
                    operand: operand.to_string(),
                })
            }
        })
    }

    fn sput(&mut self, pc: u32, dst: Operand, value: Handle<E>) -> Done {
        match (sym::register(dst), dst) {
            (Some(r), _) => self.put(Reg::S(r), value),
            (None, Operand::Null) => {}
            (None, _) => {
                return Err(NotTranslated::Operand {
                    pc,
                    operand: dst.to_string(),
                })
            }
        }
        Ok(())
    }

    fn sput64(&mut self, pc: u32, dst: Operand, value: [Handle<E>; 2]) -> Done {
        match (sym::register(dst), dst) {
            (Some(r), _) => {
                self.put(Reg::S(r), value[0]);
                self.put(Reg::S(r + 1), value[1]);
            }
            (None, Operand::Null) => {}
            (None, _) => {
                return Err(NotTranslated::Operand {
                    pc,
                    operand: dst.to_string(),
                })
            }
        }
        Ok(())
    }

    fn scc(&mut self, condition: Handle<E>) {
        let flag = self.b.flag(condition);
        self.put(Reg::Scc, flag);
    }

    fn scc_set(&mut self, pc: u32) -> Handle<E> {
        let scc = self.get(pc, Reg::Scc);
        self.b.ne0(scc)
    }

    /// Whether a lane mask has any lane set: both halves of a 64-lane mask, the low one of a 32-lane.
    // ponytail: this lane stands for the wave, so a vote over the wave is this lane's own bit.
    fn mask_any(&mut self, pc: u32, base: u8) -> Value {
        let mut bits = self.sreg(pc, base)?;
        if !self.program.wave32 {
            let high = self.sreg(pc, base + 1)?;
            bits = self.b.bin(Bo::InclusiveOr, bits, high);
        }
        Ok(self.b.ne0(bits))
    }

    /// This lane's bit of the mask in register `base`.
    fn mask_bit(&mut self, pc: u32, base: u8) -> Value {
        let mask = self.sreg(pc, base)?;
        let one = self.b.u32(1);
        let bit = self.b.bin(Bo::And, mask, one);
        Ok(self.b.ne0(bit))
    }

    /// Writes a comparison's result as a lane mask: this lane's bit where it is active, nothing of the other lanes.
    fn put_mask(&mut self, base: u8, condition: Handle<E>) {
        let active = self.active();
        let bit = self.b.bin(Bo::LogicalAnd, condition, active);
        let low = self.b.flag(bit);
        self.put(Reg::S(base), low);
        if !self.program.wave32 {
            let zero = self.b.u32(0);
            self.put(Reg::S(base + 1), zero);
        }
    }

    /// Clears this lane's bit of the lane mask where `condition` fails.
    fn narrow_exec(&mut self, condition: Handle<E>) {
        let exec = self.raw(Reg::S(EXEC));
        let (all, others) = (self.b.u32(u32::MAX), self.b.u32(!1));
        let keep = self.b.select(condition, all, others);
        let value = self.b.bin(Bo::And, exec, keep);
        self.put(Reg::S(EXEC), value);
    }

    fn sopp(&mut self, pc: u32, inst: &Inst, name: &str) -> Done {
        let condition = match name {
            "s_nop" | "s_inst_prefetch" | "s_waitcnt" | "s_waitcnt_depctr" | "s_endpgm"
            | "s_code_end" | "s_branch" => return Ok(()),
            // The counts a mesh program announces and the wait for its other threads: each thread
            // here runs alone and says by its own exports what it placed.
            "s_sendmsg" | "s_barrier" if self.mesh.is_some() => return Ok(()),
            "s_cbranch_scc0" | "s_cbranch_scc1" => {
                let set = self.scc_set(pc);
                match name {
                    "s_cbranch_scc0" => self.b.lnot(set),
                    _ => set,
                }
            }
            "s_cbranch_vccz" | "s_cbranch_vccnz" | "s_cbranch_execz" | "s_cbranch_execnz" => {
                let base = if name.contains("vcc") { VCC } else { EXEC };
                let any = self.mask_any(pc, base)?;
                match name.ends_with("nz") {
                    true => any,
                    false => self.b.lnot(any),
                }
            }
            _ => return Err(refused(pc, inst)),
        };
        self.put(Reg::Cond(self.index), condition);
        Ok(())
    }

    /// Whether a scalar source holds a table's word, which a move carries without reading.
    fn carries_table(&self, operand: Operand, words: u8) -> bool {
        sym::register(operand).is_some_and(|r| {
            (0..words).any(|k| {
                matches!(
                    self.state.get(usize::from(r) + usize::from(k)),
                    Some(Sym::Word(_) | Sym::Mixed)
                )
            })
        })
    }

    fn define_scalars(&mut self, dst: Operand, words: u8) {
        if let Some(r) = sym::register(dst) {
            for k in 0..words {
                self.defined.insert(Reg::S(r + k));
            }
        }
    }

    fn sop1(&mut self, pc: u32, inst: &Inst, name: &str, dst: Operand, src: Operand) -> Done {
        match name {
            "s_mov_b32" | "s_mov_b64" => {
                let words = if name == "s_mov_b64" { 2 } else { 1 };
                if self.carries_table(src, words) {
                    self.define_scalars(dst, words);
                } else if words == 2 {
                    let value = self.sv64(pc, src)?;
                    self.sput64(pc, dst, value)?;
                } else {
                    let value = self.sv(pc, src)?;
                    self.sput(pc, dst, value)?;
                }
            }
            "s_not_b32" | "s_wqm_b32" => {
                let mut value = self.sv(pc, src)?;
                if name == "s_not_b32" {
                    value = self.b.not(value);
                } else {
                    self.note("whole-quad mode read as this lane alone");
                }
                self.sput(pc, dst, value)?;
                let any = self.b.ne0(value);
                self.scc(any);
            }
            "s_not_b64" | "s_wqm_b64" => {
                let mut value = self.sv64(pc, src)?;
                if name == "s_not_b64" {
                    value = [self.b.not(value[0]), self.b.not(value[1])];
                } else {
                    self.note("whole-quad mode read as this lane alone");
                }
                self.sput64(pc, dst, value)?;
                let either = self.b.bin(Bo::InclusiveOr, value[0], value[1]);
                let any = self.b.ne0(either);
                self.scc(any);
            }
            "s_brev_b32" | "s_bcnt1_i32_b32" | "s_ff1_i32_b32" => {
                let value = self.sv(pc, src)?;
                let fun = match name {
                    "s_brev_b32" => Mf::ReverseBits,
                    "s_bcnt1_i32_b32" => Mf::CountOneBits,
                    _ => Mf::FirstTrailingBit,
                };
                let value = self.b.math(fun, &[value]);
                self.sput(pc, dst, value)?;
                if name == "s_bcnt1_i32_b32" {
                    let any = self.b.ne0(value);
                    self.scc(any);
                }
            }
            "s_abs_i32" => {
                let value = self.sv(pc, src)?;
                let signed = self.b.sint(value);
                let magnitude = self.b.math(Mf::Abs, &[signed]);
                let value = self.b.bits(magnitude);
                self.sput(pc, dst, value)?;
                let any = self.b.ne0(value);
                self.scc(any);
            }
            "s_sext_i32_i8" | "s_sext_i32_i16" => {
                let value = self.sv(pc, src)?;
                let by = self.b.u32(if name == "s_sext_i32_i8" { 24 } else { 16 });
                let up = self.b.bin(Bo::ShiftLeft, value, by);
                let value = self.b.shift(Bo::ShiftRight, true, up, by);
                self.sput(pc, dst, value)?;
            }
            "s_swappc_b64" if !self.fragment && self.fetched.is_empty() => self.fetch(pc, inst)?,
            _ if name.contains("_saveexec_") || name.contains("_wrexec_") => {
                self.save_exec(pc, inst, name, dst, src)?
            }
            _ => return Err(refused(pc, inst)),
        }
        Ok(())
    }

    /// `s_<f>_saveexec` keeps the lane mask in its destination and sets it to `f` of the source and
    /// itself; `s_<f>_wrexec` sets it and copies the new one.
    fn save_exec(&mut self, pc: u32, inst: &Inst, name: &str, dst: Operand, src: Operand) -> Done {
        let wide = name.ends_with("_b64");
        let save = name.contains("_saveexec_");
        let kind = (name.strip_prefix("s_"))
            .and_then(|rest| rest.split('_').next())
            .unwrap_or_default();
        let source = match wide {
            true => self.sv64(pc, src)?.to_vec(),
            false => vec![self.sv(pc, src)?],
        };
        let mut old = Vec::with_capacity(2);
        let mut new = Vec::with_capacity(2);
        for (half, &s) in source.iter().enumerate() {
            let exec = self.raw(Reg::S(EXEC + half as u8));
            let value = (self.b.logic(kind, s, exec)).ok_or_else(|| refused(pc, inst))?;
            old.push(exec);
            new.push(value);
        }
        let kept = if save { &old } else { &new };
        match wide {
            true => self.sput64(pc, dst, [kept[0], kept[1]])?,
            false => self.sput(pc, dst, kept[0])?,
        }
        for (half, &value) in new.iter().enumerate() {
            self.put(Reg::S(EXEC + half as u8), value);
        }
        let mut bits = new[0];
        if let Some(&high) = new.get(1) {
            bits = self.b.bin(Bo::InclusiveOr, bits, high);
        }
        let any = self.b.ne0(bits);
        self.scc(any);
        Ok(())
    }

    /// The call a vertex program opens with runs a fetch routine the program does not hold: each
    /// element of the input signature arrives in the four registers from `4 + 4 * register`.
    fn fetch(&mut self, pc: u32, inst: &Inst) -> Done {
        let program = self.program;
        for input in &program.vertex_inputs {
            let first = FETCH_BASE + 4 * input.register;
            if first + 3 > u32::from(u8::MAX) {
                return Err(form(
                    pc,
                    inst,
                    "an input register past the vector registers",
                ));
            }
            let scalar = match input.component_type {
                1 => Scalar::U32,
                2 => Scalar::I32,
                _ => Scalar::F32,
            };
            let ty = vector(&mut self.module, VectorSize::Quad, scalar);
            let global = match self.fetched.get(&input.register) {
                Some(&(global, _)) => global,
                None => {
                    let global = self.private(format!("in{}", input.register), ty);
                    self.fetched.insert(input.register, (global, scalar));
                    global
                }
            };
            let pointer = self.global(global);
            let value = self.b.load(pointer);
            for c in 0..4u32 {
                // A component the element does not hold reads as the format's default: 0, 0, 0, 1.
                let part = match (input.mask >> c & 1 != 0, c, scalar.kind) {
                    (true, _, _) => {
                        let part = self.b.index(value, c);
                        self.b.bits(part)
                    }
                    (false, 3, ScalarKind::Float) => self.b.u32(1f32.to_bits()),
                    (false, 3, _) => self.b.u32(1),
                    (false, _, _) => self.b.u32(0),
                };
                self.vput((first + c) as u8, part);
            }
        }
        // The call leaves its return address in the destination pair, which nothing reads back.
        if let Inst::Sop1 { dst, .. } = inst {
            self.define_scalars(*dst, 2);
        }
        if program.vertex_inputs.is_empty() {
            self.note("a fetch call with no input signature");
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn sop2(
        &mut self,
        pc: u32,
        inst: &Inst,
        name: &str,
        dst: Operand,
        src0: Operand,
        src1: Operand,
    ) -> Done {
        let bare = name.strip_prefix("s_").unwrap_or(name);
        if let Some(kind) = bare.strip_suffix("_b64") {
            if matches!(
                kind,
                "and" | "or" | "xor" | "andn2" | "orn2" | "nand" | "nor" | "xnor"
            ) {
                let (a, b) = (self.sv64(pc, src0)?, self.sv64(pc, src1)?);
                let low = self.b.logic(kind, a[0], b[0]);
                let high = self.b.logic(kind, a[1], b[1]);
                let (Some(low), Some(high)) = (low, high) else {
                    return Err(refused(pc, inst));
                };
                let value = [low, high];
                self.sput64(pc, dst, value)?;
                let either = self.b.bin(Bo::InclusiveOr, value[0], value[1]);
                let any = self.b.ne0(either);
                self.scc(any);
                return Ok(());
            }
            if kind == "cselect" {
                let (a, b) = (self.sv64(pc, src0)?, self.sv64(pc, src1)?);
                let set = self.scc_set(pc);
                let value = [
                    self.b.select(set, a[0], b[0]),
                    self.b.select(set, a[1], b[1]),
                ];
                return self.sput64(pc, dst, value);
            }
            return Err(refused(pc, inst));
        }
        let (a, b) = (self.sv(pc, src0)?, self.sv(pc, src1)?);
        if let Some(value) = (bare.strip_suffix("_b32")).and_then(|kind| self.b.logic(kind, a, b)) {
            self.sput(pc, dst, value)?;
            let any = self.b.ne0(value);
            self.scc(any);
            return Ok(());
        }
        let top = |b: &mut Builder, bits: Handle<E>| {
            let by = b.u32(31);
            let sign = b.bin(Bo::ShiftRight, bits, by);
            b.ne0(sign)
        };
        let (value, scc) = match name {
            "s_add_u32" => {
                let sum = self.b.bin(Bo::Add, a, b);
                (sum, Some(self.b.bin(Bo::Less, sum, a)))
            }
            "s_sub_u32" => {
                let difference = self.b.bin(Bo::Subtract, a, b);
                (difference, Some(self.b.bin(Bo::Greater, b, a)))
            }
            "s_add_i32" => {
                let sum = self.b.bin(Bo::Add, a, b);
                let (x, y) = (
                    self.b.bin(Bo::ExclusiveOr, a, sum),
                    self.b.bin(Bo::ExclusiveOr, b, sum),
                );
                let both = self.b.bin(Bo::And, x, y);
                (sum, Some(top(&mut self.b, both)))
            }
            "s_sub_i32" => {
                let difference = self.b.bin(Bo::Subtract, a, b);
                let (x, y) = (
                    self.b.bin(Bo::ExclusiveOr, a, b),
                    self.b.bin(Bo::ExclusiveOr, a, difference),
                );
                let both = self.b.bin(Bo::And, x, y);
                (difference, Some(top(&mut self.b, both)))
            }
            "s_addc_u32" => {
                let carry = self.get(pc, Reg::Scc);
                let partial = self.b.bin(Bo::Add, a, b);
                let sum = self.b.bin(Bo::Add, partial, carry);
                let (first, second) = (
                    self.b.bin(Bo::Less, partial, a),
                    self.b.bin(Bo::Less, sum, partial),
                );
                (sum, Some(self.b.bin(Bo::LogicalOr, first, second)))
            }
            "s_subb_u32" => {
                let borrow = self.get(pc, Reg::Scc);
                let partial = self.b.bin(Bo::Subtract, a, b);
                let difference = self.b.bin(Bo::Subtract, partial, borrow);
                let (first, second) = (
                    self.b.bin(Bo::Greater, b, a),
                    self.b.bin(Bo::Greater, borrow, partial),
                );
                (difference, Some(self.b.bin(Bo::LogicalOr, first, second)))
            }
            "s_min_i32" | "s_max_i32" | "s_min_u32" | "s_max_u32" => {
                let rel = if name.contains("min") { "lt" } else { "gt" };
                let chosen = (self.b.icmp(rel, name.ends_with("i32"), a, b))
                    .ok_or_else(|| refused(pc, inst))?;
                (self.b.select(chosen, a, b), Some(chosen))
            }
            "s_cselect_b32" => {
                let set = self.scc_set(pc);
                (self.b.select(set, a, b), None)
            }
            "s_lshl_b32" | "s_lshr_b32" | "s_ashr_i32" => {
                let value = match name {
                    "s_lshl_b32" => self.b.shift(Bo::ShiftLeft, false, a, b),
                    "s_lshr_b32" => self.b.shift(Bo::ShiftRight, false, a, b),
                    _ => self.b.shift(Bo::ShiftRight, true, a, b),
                };
                (value, Some(self.b.ne0(value)))
            }
            "s_bfm_b32" => {
                let low = self.b.u32(31);
                let width = self.b.bin(Bo::And, a, low);
                let mask = self.b.low_bits(width);
                (self.b.shift(Bo::ShiftLeft, false, mask, b), None)
            }
            "s_mul_i32" => (self.b.bin(Bo::Multiply, a, b), None),
            "s_mul_hi_u32" => (self.b.mul_hi(false, a, b), None),
            "s_mul_hi_i32" => (self.b.mul_hi(true, a, b), None),
            "s_bfe_u32" | "s_bfe_i32" => {
                // The second source holds the offset in its low five bits and the width in bits 22:16.
                let (sixteen, low7) = (self.b.u32(16), self.b.u32(0x7F));
                let width = self.b.bin(Bo::ShiftRight, b, sixteen);
                let width = self.b.bin(Bo::And, width, low7);
                let mask = self.b.low_bits(width);
                let shifted = self.b.shift(Bo::ShiftRight, false, a, b);
                let mut value = self.b.bin(Bo::And, shifted, mask);
                if name == "s_bfe_i32" {
                    // Sign-extended from the field's top bit; a width of 32 or more leaves it as it is.
                    let (limit, zero) = (self.b.u32(32), self.b.u32(0));
                    let spare = self.b.bin(Bo::Subtract, limit, width);
                    let narrow = self.b.bin(Bo::Less, width, limit);
                    let spare = self.b.select(narrow, spare, zero);
                    let up = self.b.shift(Bo::ShiftLeft, false, value, spare);
                    value = self.b.shift(Bo::ShiftRight, true, up, spare);
                }
                (value, Some(self.b.ne0(value)))
            }
            "s_absdiff_i32" => {
                let difference = self.b.bin(Bo::Subtract, a, b);
                let signed = self.b.sint(difference);
                let magnitude = self.b.math(Mf::Abs, &[signed]);
                let value = self.b.bits(magnitude);
                (value, Some(self.b.ne0(value)))
            }
            "s_lshl1_add_u32" | "s_lshl2_add_u32" | "s_lshl3_add_u32" | "s_lshl4_add_u32" => {
                let by = self.b.u32(u32::from(name.as_bytes()[6] - b'0'));
                let shifted = self.b.bin(Bo::ShiftLeft, a, by);
                let sum = self.b.bin(Bo::Add, shifted, b);
                (sum, Some(self.b.bin(Bo::Less, sum, b)))
            }
            _ => return Err(refused(pc, inst)),
        };
        self.sput(pc, dst, value)?;
        if let Some(scc) = scc {
            self.scc(scc);
        }
        Ok(())
    }

    fn sopk(
        &mut self,
        pc: u32,
        inst: &Inst,
        name: &str,
        dst: Operand,
        imm: i16,
        imm32: Option<u32>,
    ) -> Done {
        let signed = self.b.u32(i32::from(imm) as u32);
        match name {
            "s_movk_i32" => self.sput(pc, dst, signed)?,
            "s_cmovk_i32" => {
                let (set, old) = (self.scc_set(pc), self.sv(pc, dst)?);
                let value = self.b.select(set, signed, old);
                self.sput(pc, dst, value)?;
            }
            "s_addk_i32" => {
                let a = self.sv(pc, dst)?;
                let sum = self.b.bin(Bo::Add, a, signed);
                let (x, y) = (
                    self.b.bin(Bo::ExclusiveOr, a, sum),
                    self.b.bin(Bo::ExclusiveOr, signed, sum),
                );
                let both = self.b.bin(Bo::And, x, y);
                let by = self.b.u32(31);
                let sign = self.b.bin(Bo::ShiftRight, both, by);
                let overflow = self.b.ne0(sign);
                self.sput(pc, dst, sum)?;
                self.scc(overflow);
            }
            "s_mulk_i32" => {
                let a = self.sv(pc, dst)?;
                let value = self.b.bin(Bo::Multiply, a, signed);
                self.sput(pc, dst, value)?;
            }
            "s_setreg_b32" | "s_setreg_imm32_b32" => {
                // The immediate names a register, a first bit and a width. Only the float mode is
                // written here, and only its single-precision rounding changes what this module computes.
                let word = imm as u16;
                let (id, first, width) = (
                    word & 0x3F,
                    u32::from(word >> 6 & 0x1F),
                    u32::from(word >> 11) + 1,
                );
                let rounding = first < 2;
                let nearest = match imm32 {
                    Some(value) => {
                        value & (((1u64 << width.min(2 - first.min(2))) - 1) as u32) == 0
                    }
                    None => false,
                };
                // Registers 20 and 21 are the scratch memory's base, which only scratch instructions read.
                let scratch = matches!(id, 20 | 21);
                if !scratch && (id != 1 || (rounding && !nearest) || width + first > 32) {
                    return Err(form(pc, inst, "a mode other than round-to-nearest"));
                }
                if !scratch {
                    self.note("a float mode write that is not modelled");
                }
            }
            _ => {
                let Some(rest) = name.strip_prefix("s_cmpk_") else {
                    return Err(refused(pc, inst));
                };
                let (rel, kind) = rest.split_once('_').unwrap_or((rest, ""));
                let a = self.sv(pc, dst)?;
                let b = match kind {
                    "i32" => signed,
                    _ => self.b.u32(u32::from(imm as u16)),
                };
                let condition =
                    (self.b.icmp(rel, kind == "i32", a, b)).ok_or_else(|| refused(pc, inst))?;
                self.scc(condition);
            }
        }
        Ok(())
    }

    fn sopc(&mut self, pc: u32, inst: &Inst, name: &str, src0: Operand, src1: Operand) -> Done {
        let condition = match name {
            "s_cmp_eq_u64" | "s_cmp_lg_u64" => {
                let (a, b) = (self.sv64(pc, src0)?, self.sv64(pc, src1)?);
                let (low, high) = (
                    self.b.bin(Bo::Equal, a[0], b[0]),
                    self.b.bin(Bo::Equal, a[1], b[1]),
                );
                let equal = self.b.bin(Bo::LogicalAnd, low, high);
                match name {
                    "s_cmp_eq_u64" => equal,
                    _ => self.b.lnot(equal),
                }
            }
            "s_bitcmp0_b32" | "s_bitcmp1_b32" => {
                let (a, b) = (self.sv(pc, src0)?, self.sv(pc, src1)?);
                let shifted = self.b.shift(Bo::ShiftRight, false, a, b);
                let one = self.b.u32(1);
                let bit = self.b.bin(Bo::And, shifted, one);
                let set = self.b.ne0(bit);
                match name {
                    "s_bitcmp1_b32" => set,
                    _ => self.b.lnot(set),
                }
            }
            _ => {
                let Some(rest) = name.strip_prefix("s_cmp_") else {
                    return Err(refused(pc, inst));
                };
                let (rel, kind) = rest.split_once('_').unwrap_or((rest, ""));
                let (a, b) = (self.sv(pc, src0)?, self.sv(pc, src1)?);
                (self.b.icmp(rel, kind == "i32", a, b)).ok_or_else(|| refused(pc, inst))?
            }
        };
        self.scc(condition);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn smem(
        &mut self,
        pc: u32,
        inst: &Inst,
        name: &str,
        base: u8,
        data: Operand,
        offset: u32,
        soffset: Operand,
    ) -> Done {
        let (Some(words), Some(to)) = (sym::load_words(name), sym::register(data)) else {
            return Err(refused(pc, inst));
        };
        if name.starts_with("s_load_dword") {
            // A table read computes nothing here: `sym` carries which words the registers hold.
            if sym::loaded(&self.state, base, offset, soffset).is_none() {
                return Err(NotTranslated::Descriptor {
                    pc,
                    what: "a table through a pointer or at an offset the program computed",
                });
            }
            for k in 0..words {
                self.defined.insert(Reg::S(to + k));
            }
            return Ok(());
        }
        let Some(at) = sym::descriptor(&self.state, base, 4) else {
            return Err(NotTranslated::Descriptor {
                pc,
                what: "a buffer whose descriptor it did not load from a table",
            });
        };
        let dynamic = match soffset {
            Operand::Null => None,
            _ => {
                let by = self.sv(pc, soffset)?;
                let mask = self.b.u32(!3);
                Some(self.b.bin(Bo::And, by, mask))
            }
        };
        let last = (offset & !3) / 4 + u32::from(words) - 1;
        let rows = match dynamic {
            Some(_) => DYNAMIC_ROWS,
            None => last / 4 + 1,
        };
        let global = match self.buffers.get_mut(&at) {
            Some((global, held)) => {
                *held = (*held).max(rows);
                *global
            }
            None => {
                let inner = self.rows(1);
                let global = self.bound(
                    format!("cb{}", self.buffers.len()),
                    inner,
                    naga::AddressSpace::Uniform,
                    ResourceKind::ConstantBuffer { rows: 1 },
                    Some(at.clone()),
                );
                self.buffers.insert(at, (global, rows));
                global
            }
        };
        for k in 0..u32::from(words) {
            let word = (offset & !3) / 4 + k;
            let pointer = self.global(global);
            let cell = match dynamic {
                None => {
                    let row = self.b.index(pointer, word / 4);
                    self.b.index(row, word % 4)
                }
                Some(by) => {
                    let (two, three, fixed) = (self.b.u32(2), self.b.u32(3), self.b.u32(word));
                    let moved = self.b.bin(Bo::ShiftRight, by, two);
                    let index = self.b.bin(Bo::Add, moved, fixed);
                    let (row, column) = (
                        self.b.bin(Bo::ShiftRight, index, two),
                        self.b.bin(Bo::And, index, three),
                    );
                    let row = self.b.emit(E::Access {
                        base: pointer,
                        index: row,
                    });
                    self.b.emit(E::Access {
                        base: row,
                        index: column,
                    })
                }
            };
            let value = self.b.load(cell);
            self.put(Reg::S(to + k as u8), value);
        }
        Ok(())
    }

    /// `bits` with the float modifiers a three-source encoding gives a source.
    fn modified(&mut self, bits: Handle<E>, abs: bool, neg: bool) -> Handle<E> {
        if !abs && !neg {
            return bits;
        }
        let mut x = self.b.float(bits);
        if abs {
            x = self.b.math(Mf::Abs, &[x]);
        }
        if neg {
            x = self.b.un(Uo::Negate, x);
        }
        self.b.bits(x)
    }

    /// A vector comparison's verdict by its mnemonic, `None` for one not lowered.
    fn compare(&mut self, name: &str, a: Handle<E>, b: Handle<E>) -> Option<Handle<E>> {
        let rest = (name.strip_prefix("v_cmpx_")).or_else(|| name.strip_prefix("v_cmp_"))?;
        let (rel, kind) = rest.rsplit_once('_')?;
        match (rel, kind) {
            ("class", "f32") => Some(self.b.class(a, b)),
            (_, "f32") => self.b.fcmp(rel, a, b),
            (_, "i32" | "u32") => self.b.icmp(rel, kind == "i32", a, b),
            _ => None,
        }
    }

    /// Writes a comparison where its encoding says: the lane mask for a `cmpx`, else `dst`.
    fn put_compare(&mut self, name: &str, dst: u8, condition: Handle<E>) {
        match name.starts_with("v_cmpx") {
            true => self.narrow_exec(condition),
            false => self.put_mask(dst, condition),
        }
    }

    /// The derivative a subtraction of two neighbouring lanes' values is, `None` where neither operand is one.
    fn derivative(
        &mut self,
        pc: u32,
        inst: &Inst,
        left: Operand,
        right: Operand,
    ) -> Result<Option<Handle<E>>, NotTranslated> {
        let tag = |cx: &Self, operand: Operand| match operand {
            Operand::Vgpr(n) => cx.permuted.get(&n).copied(),
            _ => None,
        };
        let (a, b) = match (tag(self, left), tag(self, right)) {
            (None, None) => return Ok(None),
            (Some(a), Some(b)) => (a, b),
            _ => {
                return Err(form(
                    pc,
                    inst,
                    "one neighbouring lane's value and one of this lane",
                ))
            }
        };
        let current = self.versions.get(&a.source).copied().unwrap_or(0);
        if a.source != b.source || a.version != b.version || a.version != current {
            return Err(form(
                pc,
                inst,
                "neighbouring lanes' values of two different registers",
            ));
        }
        let (axis, ctrl, negate) = match (a.quad, b.quad) {
            ([1, 1, 1, 1], [0, 0, 0, 0]) => (DerivativeAxis::X, DerivativeControl::Coarse, false),
            ([0, 0, 0, 0], [1, 1, 1, 1]) => (DerivativeAxis::X, DerivativeControl::Coarse, true),
            ([2, 2, 2, 2], [0, 0, 0, 0]) => (DerivativeAxis::Y, DerivativeControl::Coarse, false),
            ([0, 0, 0, 0], [2, 2, 2, 2]) => (DerivativeAxis::Y, DerivativeControl::Coarse, true),
            ([1, 1, 3, 3], [0, 0, 2, 2]) => (DerivativeAxis::X, DerivativeControl::Fine, false),
            ([0, 0, 2, 2], [1, 1, 3, 3]) => (DerivativeAxis::X, DerivativeControl::Fine, true),
            ([2, 3, 2, 3], [0, 1, 0, 1]) => (DerivativeAxis::Y, DerivativeControl::Fine, false),
            ([0, 1, 0, 1], [2, 3, 2, 3]) => (DerivativeAxis::Y, DerivativeControl::Fine, true),
            _ => {
                return Err(form(
                    pc,
                    inst,
                    "a pair of lane permutations that is no derivative",
                ))
            }
        };
        if !self.fragment {
            return Err(form(pc, inst, "a derivative outside a pixel program"));
        }
        let source = self.get(pc, Reg::V(a.source));
        let expr = self.b.float(source);
        let mut value = self.b.emit(E::Derivative { axis, ctrl, expr });
        if negate {
            value = self.b.un(Uo::Negate, value);
        }
        Ok(Some(self.b.bits(value)))
    }

    /// Marks `dst` as holding `source` as the quad's lanes `quad` computed it.
    fn permute(&mut self, dst: u8, source: u8, quad: Quad) {
        let version = self.versions.get(&source).copied().unwrap_or(0);
        *self.versions.entry(dst).or_default() += 1;
        self.defined.insert(Reg::V(dst));
        self.permuted.insert(
            dst,
            Permuted {
                source,
                version,
                quad,
            },
        );
    }

    fn vop1(
        &mut self,
        pc: u32,
        inst: &Inst,
        name: &str,
        dst: u8,
        src: Operand,
        extra: Option<u32>,
    ) -> Done {
        match (name, src, extra) {
            ("v_nop", _, _) => {}
            ("v_readfirstlane_b32", _, _) => {
                // ponytail: this lane stands for the first active one, which is exact where the value is the same in every lane.
                let value = self.sv(pc, src)?;
                self.put(Reg::S(dst), value);
                self.note("the first lane read as this lane");
            }
            ("v_mov_b32", Operand::Dpp, Some(word)) => {
                // The control word: the source register, a nine-bit control, then the bank and row masks.
                let (source, control, masks) = (word as u8, word >> 8 & 0x1FF, word >> 24);
                if control > 0xFF || masks != 0xFF {
                    return Err(form(
                        pc,
                        inst,
                        "a lane permutation that is not a whole quad's",
                    ));
                }
                let quad = [0, 2, 4, 6].map(|low| (control >> low & 3) as u8);
                self.permute(dst, source, quad);
            }
            (_, Operand::Sdwa, Some(word)) => {
                let (a, _) = self.sdwa_sources(pc, inst, word, None)?;
                let value = self.b.alu(name, &[a]).ok_or_else(|| refused(pc, inst))?;
                self.sdwa_put(pc, inst, word, dst, value)?;
            }
            _ => {
                let a = self.sv(pc, src)?;
                let value = match name {
                    "v_cvt_f16_f32" => {
                        let (x, zero) = (self.b.float(a), self.b.f32(0.0));
                        let packed = self.pack_halves(x, zero);
                        let low = self.b.u32(0xFFFF);
                        self.b.bin(Bo::And, packed, low)
                    }
                    "v_cvt_f32_f16" => {
                        let value = self.half(a, false);
                        self.b.bits(value)
                    }
                    _ => self.b.alu(name, &[a]).ok_or_else(|| refused(pc, inst))?,
                };
                self.vput(dst, value);
            }
        }
        Ok(())
    }

    /// Two floats as the two 16-bit halves of a dword, the first in the low half.
    // ponytail: naga packs to the nearest half; `v_cvt_pkrtz_f16_f32` rounds toward zero, one step of a half away at most.
    fn pack_halves(&mut self, low: Handle<E>, high: Handle<E>) -> Handle<E> {
        let ty = vector(&mut self.module, VectorSize::Bi, Scalar::F32);
        let pair = self.b.emit(E::Compose {
            ty,
            components: vec![low, high],
        });
        self.note("a 16-bit float rounded to nearest");
        self.b.math(Mf::Pack2x16float, &[pair])
    }

    /// One 16-bit half of a dword as a float.
    fn half(&mut self, bits: Handle<E>, high: bool) -> Handle<E> {
        let pair = self.b.math(Mf::Unpack2x16float, &[bits]);
        self.b.index(pair, u32::from(high))
    }

    #[allow(clippy::too_many_arguments)]
    fn vop2(
        &mut self,
        pc: u32,
        inst: &Inst,
        name: &str,
        dst: u8,
        src0: Operand,
        src1: u8,
        extra: Option<u32>,
    ) -> Done {
        if matches!(name, "v_sub_f32" | "v_subrev_f32") {
            let (left, right) = match name {
                "v_sub_f32" => (src0, Operand::Vgpr(src1)),
                _ => (Operand::Vgpr(src1), src0),
            };
            if let Some(value) = self.derivative(pc, inst, left, right)? {
                self.vput(dst, value);
                return Ok(());
            }
        }
        if let (Operand::Sdwa, Some(word)) = (src0, extra) {
            let (a, b) = self.sdwa_sources(pc, inst, word, Some(src1))?;
            let value =
                (b.and_then(|b| self.b.alu(name, &[a, b]))).ok_or_else(|| refused(pc, inst))?;
            return self.sdwa_put(pc, inst, word, dst, value);
        }
        let a = self.sv(pc, src0)?;
        let b = self.vreg(pc, src1)?;
        let constant = match (src0.takes_word(), extra) {
            (false, Some(word)) => Some(self.b.u32(word)),
            _ => None,
        };
        let value = match (name, constant) {
            ("v_cndmask_b32", _) => {
                let chosen = self.mask_bit(pc, VCC)?;
                self.b.select(chosen, b, a)
            }
            ("v_mac_f32" | "v_mac_legacy_f32", _) => {
                let c = self.vreg(pc, dst)?;
                self.b.mad(name == "v_mac_legacy_f32", a, b, c)
            }
            ("v_fmac_f32", _) => {
                let c = self.vreg(pc, dst)?;
                self.b.fmath(Mf::Fma, &[a, b, c])
            }
            ("v_madmk_f32", Some(k)) => self.b.mad(false, a, k, b),
            ("v_madak_f32", Some(k)) => self.b.mad(false, a, b, k),
            ("v_fmamk_f32", Some(k)) => self.b.fmath(Mf::Fma, &[a, k, b]),
            ("v_fmaak_f32", Some(k)) => self.b.fmath(Mf::Fma, &[a, b, k]),
            ("v_cvt_pkrtz_f16_f32", _) => {
                let (low, high) = (self.b.float(a), self.b.float(b));
                self.pack_halves(low, high)
            }
            _ => (self.b.alu(name, &[a, b])).ok_or_else(|| refused(pc, inst))?,
        };
        self.vput(dst, value);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn vop3(
        &mut self,
        pc: u32,
        inst: &Inst,
        name: &str,
        dst: u8,
        srcs: [Operand; 3],
        (abs, neg, opsel): (u8, u8, u8),
        (clamp, omod): (bool, u8),
        count: usize,
    ) -> Done {
        // A select takes the float modifiers too: it is how a negated or absolute value is chosen.
        let float_in = name.ends_with("_f32") || name == "v_cndmask_b32";
        let float_out =
            (name.ends_with("_f32") && !name.starts_with("v_cvt_") && !name.starts_with("v_cmp"))
                || name.starts_with("v_cvt_f32_");
        let compare = name.starts_with("v_cmp");
        if name.contains("_co_") || matches!(name, "v_readlane_b32" | "v_writelane_b32") {
            return Err(refused(pc, inst));
        }
        if matches!(name, "v_mbcnt_lo_u32_b32" | "v_mbcnt_hi_u32_b32") {
            return self.mbcnt(pc, inst, name, dst, srcs);
        }
        if name == "v_pack_b32_f16" {
            if clamp || omod != 0 {
                return Err(form(pc, inst, "an output modifier on packed halves"));
            }
            return self.pack_b32(pc, inst, dst, srcs, (abs, neg, opsel));
        }
        if !compare && opsel != 0 {
            return Err(form(pc, inst, "a half selection"));
        }
        if (omod != 0 || clamp) && !float_out {
            return Err(form(pc, inst, "an output modifier on an integer result"));
        }
        if (abs != 0 || neg != 0) && !float_in {
            return Err(form(pc, inst, "a float modifier on an integer operand"));
        }
        if matches!(name, "v_sub_f32" | "v_subrev_f32") && abs == 0 && neg == 0 {
            let (left, right) = match name {
                "v_sub_f32" => (srcs[0], srcs[1]),
                _ => (srcs[1], srcs[0]),
            };
            if let Some(value) = self.derivative(pc, inst, left, right)? {
                self.vput(dst, value);
                return Ok(());
            }
        }
        let mut values = Vec::with_capacity(count);
        for (i, src) in srcs.iter().take(count).enumerate() {
            let bits = self.sv(pc, *src)?;
            values.push(self.modified(bits, abs >> i & 1 != 0, neg >> i & 1 != 0));
        }
        if compare {
            let condition = match values.as_slice() {
                &[a, b] => self.compare(name, a, b),
                _ => None,
            };
            let condition = condition.ok_or_else(|| refused(pc, inst))?;
            self.put_compare(name, dst & 0x7F, condition);
            return Ok(());
        }
        let mut value = match (name, values.as_slice()) {
            ("v_cndmask_b32", &[a, b, _]) => {
                let mask = sym::register(srcs[2]).ok_or_else(|| refused(pc, inst))?;
                let chosen = self.mask_bit(pc, mask)?;
                self.b.select(chosen, b, a)
            }
            ("v_mac_f32" | "v_mac_legacy_f32", &[a, b]) => {
                let c = self.vreg(pc, dst)?;
                self.b.mad(name == "v_mac_legacy_f32", a, b, c)
            }
            ("v_fmac_f32", &[a, b]) => {
                let c = self.vreg(pc, dst)?;
                self.b.fmath(Mf::Fma, &[a, b, c])
            }
            ("v_cvt_pkrtz_f16_f32", &[a, b]) => {
                let (low, high) = (self.b.float(a), self.b.float(b));
                self.pack_halves(low, high)
            }
            (_, values) => (self.b.alu(name, values)).ok_or_else(|| refused(pc, inst))?,
        };
        if omod != 0 {
            let by = self.b.f32([2.0, 4.0, 0.5][usize::from(omod - 1) % 3]);
            let x = self.b.float(value);
            let scaled = self.b.bin(Bo::Multiply, x, by);
            value = self.b.bits(scaled);
        }
        if clamp {
            value = self.b.fmath(Mf::Saturate, &[value]);
        }
        self.vput(dst, value);
        Ok(())
    }

    /// `v_fma_mix_f32`: a fused multiply-add whose every source is a whole float or a 16-bit half.
    #[allow(clippy::too_many_arguments)]
    fn fma_mix(
        &mut self,
        pc: u32,
        name: &str,
        dst: u8,
        srcs: [Operand; 3],
        (half_high, halved): (u8, u8),
        (neg, abs): (u8, u8),
        clamp: bool,
    ) -> Done {
        let mut values = Vec::with_capacity(3);
        for (i, src) in srcs.iter().enumerate() {
            let bits = self.sv(pc, *src)?;
            let mut x = match halved >> i & 1 != 0 {
                true => self.half(bits, half_high >> i & 1 != 0),
                false => self.b.float(bits),
            };
            if abs >> i & 1 != 0 {
                x = self.b.math(Mf::Abs, &[x]);
            }
            if neg >> i & 1 != 0 {
                x = self.b.un(Uo::Negate, x);
            }
            values.push(x);
        }
        let mut value = self.b.math(Mf::Fma, &values);
        if clamp {
            value = self.b.math(Mf::Saturate, &[value]);
        }
        let value = match name {
            "v_fma_mix_f32" => self.b.bits(value),
            // The 16-bit forms write one half of the destination and keep the other.
            _ => {
                let zero = self.b.f32(0.0);
                let packed = self.pack_halves(value, zero);
                let (low, old) = (self.b.u32(0xFFFF), self.raw(Reg::V(dst)));
                let half = self.b.bin(Bo::And, packed, low);
                match name {
                    "v_fma_mixlo_f16" => {
                        let high = self.b.u32(0xFFFF_0000);
                        let kept = self.b.bin(Bo::And, old, high);
                        self.b.bin(Bo::InclusiveOr, kept, half)
                    }
                    _ => {
                        let sixteen = self.b.u32(16);
                        let raised = self.b.bin(Bo::ShiftLeft, half, sixteen);
                        let kept = self.b.bin(Bo::And, old, low);
                        self.b.bin(Bo::InclusiveOr, kept, raised)
                    }
                }
            }
        };
        self.vput(dst, value);
        Ok(())
    }

    /// `v_pack_b32_f16`: the chosen 16-bit half of each source, the first in the low half of the result.
    fn pack_b32(
        &mut self,
        pc: u32,
        inst: &Inst,
        dst: u8,
        srcs: [Operand; 3],
        (abs, neg, opsel): (u8, u8, u8),
    ) -> Done {
        let mut halves = Vec::with_capacity(2);
        for (i, src) in srcs.iter().take(2).enumerate() {
            // An inline float constant is a 16-bit one here, which the operand does not spell.
            if matches!(src, Operand::Float(_)) {
                return Err(form(
                    pc,
                    inst,
                    "an inline float constant as a 16-bit operand",
                ));
            }
            let mut half = self.sv(pc, *src)?;
            if (opsel >> i) & 1 != 0 {
                let sixteen = self.b.u32(16);
                half = self.b.bin(Bo::ShiftRight, half, sixteen);
            }
            let low = self.b.u32(0xFFFF);
            half = self.b.bin(Bo::And, half, low);
            if (abs >> i) & 1 != 0 {
                let magnitude = self.b.u32(0x7FFF);
                half = self.b.bin(Bo::And, half, magnitude);
            }
            if (neg >> i) & 1 != 0 {
                let sign = self.b.u32(0x8000);
                half = self.b.bin(Bo::ExclusiveOr, half, sign);
            }
            halves.push(half);
        }
        let sixteen = self.b.u32(16);
        let raised = self.b.bin(Bo::ShiftLeft, halves[1], sixteen);
        let value = self.b.bin(Bo::InclusiveOr, halves[0], raised);
        self.vput(dst, value);
        Ok(())
    }

    /// The 16-bit interpolation: the second step reads one half of a parameter's component, interpolated,
    /// and leaves it as a 16-bit float; the first step's partial sum is only ever the second's input.
    fn interp_half(&mut self, pc: u32, inst: &Inst, name: &str, dst: u8, place: Operand) -> Done {
        if !self.fragment {
            return Err(refused(pc, inst));
        }
        if name != "v_interp_p2_f16" {
            self.defined.insert(Reg::V(dst));
            return Ok(());
        }
        // The first source field is no operand here: the attribute in bits 5:0, the component in 7:6, the half in 8.
        let code = match place {
            Operand::Sgpr(n) => u32::from(n),
            Operand::Int(n) if n >= 0 => 128 + n as u32,
            Operand::Int(n) => (192 - n) as u32,
            Operand::Reserved(code) => u32::from(code),
            Operand::Vgpr(n) => 256 + u32::from(n),
            _ => {
                return Err(form(
                    pc,
                    inst,
                    "an attribute field that reads as a status operand",
                ))
            }
        };
        let (attr, chan, high) = ((code & 0x3F) as u8, (code >> 6) & 3, (code >> 8) & 1 != 0);
        let Some(input) = self.program.inputs.get(usize::from(attr)) else {
            return Err(form(
                pc,
                inst,
                "an attribute its package's input table does not list",
            ));
        };
        let slot = input.slot;
        if self.attributes.contains_key(&slot) {
            return Err(form(pc, inst, "an input read both whole and as halves"));
        }
        let global = match self.halves.get(&(slot, high)) {
            Some(&global) => global,
            None => {
                let ty = vector(&mut self.module, VectorSize::Quad, Scalar::F32);
                let half = if high { "high" } else { "low" };
                let global = self.private(format!("attr{slot}_{half}"), ty);
                self.halves.insert((slot, high), global);
                global
            }
        };
        let pointer = self.global(global);
        let cell = self.b.index(pointer, chan);
        let (value, zero) = (self.b.load(cell), self.b.f32(0.0));
        // ponytail: interpolated as a float and rounded once; the hardware rounds each step to 16 bits.
        let packed = self.pack_halves(value, zero);
        self.vput(dst, packed);
        Ok(())
    }

    /// The system value a pixel program finds in its `k`th vector register after the barycentric pair:
    /// the position components its signature reads, in order, then the front face as plus or minus one.
    fn system_input(&mut self, k: u8) -> Option<Handle<E>> {
        let program = self.program;
        let read: Vec<u32> = (0..4)
            .filter(|c| (program.position_read >> c) & 1 != 0)
            .collect();
        if let Some(&component) = read.get(usize::from(k)) {
            let global = match self.frag_coord {
                Some(global) => global,
                None => {
                    let ty = vector(&mut self.module, VectorSize::Quad, Scalar::F32);
                    let global = self.private("frag_coord".to_owned(), ty);
                    self.frag_coord = Some(global);
                    global
                }
            };
            let pointer = self.global(global);
            let cell = self.b.index(pointer, component);
            let value = self.b.load(cell);
            return Some(self.b.bits(value));
        }
        if !program.front_face || usize::from(k) != read.len() {
            return None;
        }
        let global = match self.front_face {
            Some(global) => global,
            None => {
                let ty = self.ty(TypeInner::Scalar(Scalar::BOOL));
                let global = self.private("front_facing".to_owned(), ty);
                self.front_face = Some(global);
                global
            }
        };
        let pointer = self.global(global);
        let front = self.b.load(pointer);
        let (yes, no) = (self.b.u32(1f32.to_bits()), self.b.u32((-1f32).to_bits()));
        Some(self.b.select(front, yes, no))
    }

    /// A sampler's global, by the table word its descriptor starts at or by the constant words the code wrote.
    fn sampler(
        &mut self,
        at: Option<Loc>,
        words: Option<[u32; 4]>,
        comparison: bool,
    ) -> Handle<naga::GlobalVariable> {
        let key = (at, words, comparison);
        if let Some(&global) = self.samplers.get(&key) {
            return global;
        }
        let global = self.bound(
            format!("samp{}", self.samplers.len()),
            TypeInner::Sampler { comparison },
            naga::AddressSpace::Handle,
            ResourceKind::Sampler { comparison },
            key.0.clone(),
        );
        if let Some(resource) = self.resources.last_mut() {
            resource.words = words;
        }
        self.samplers.insert(key, global);
        global
    }

    /// A mesh thread's number in its group, and its group's number.
    fn mesh_ids(&mut self, mesh: Mesh) -> (Handle<E>, Handle<E>) {
        let pointer = self.global(mesh.thread);
        let thread = self.b.load(pointer);
        let pointer = self.global(mesh.group);
        (thread, self.b.load(pointer))
    }

    /// `v_mbcnt_*` over a mask of every lane: how many lanes of the wave come before this one, in
    /// the mask's low half or in its high half, added to the second source.
    fn mbcnt(&mut self, pc: u32, inst: &Inst, name: &str, dst: u8, srcs: [Operand; 3]) -> Done {
        let Some(mesh) = self.mesh else {
            return Err(form(pc, inst, "a lane's number outside a mesh program"));
        };
        if srcs[0] != Operand::Int(-1) {
            return Err(form(pc, inst, "a mask that is not every lane"));
        }
        let (thread, _) = self.mesh_ids(mesh);
        let (last, half) = (self.b.u32(WAVE - 1), self.b.u32(WAVE / 2));
        let lane = self.b.bin(Bo::And, thread, last);
        let before = match name {
            "v_mbcnt_lo_u32_b32" => self.b.math(Mf::Min, &[lane, half]),
            _ => {
                let past = self.b.math(Mf::Max, &[lane, half]);
                self.b.bin(Bo::Subtract, past, half)
            }
        };
        let base = self.sv(pc, srcs[1])?;
        let value = self.b.bin(Bo::Add, before, base);
        self.vput(dst, value);
        Ok(())
    }

    /// A mesh thread's primitive: the dword of three vertex numbers and a cull bit, kept for the entry point.
    fn export_primitive(&mut self, pc: u32, en: u8, source: u8) -> Done {
        let Some(mesh) = self.mesh else {
            return Ok(());
        };
        if en & 1 == 0 {
            return Ok(());
        }
        let value = self.vreg(pc, source)?;
        let active = self.masked.then(|| self.active());
        if active.is_some() {
            self.b.open();
        }
        let pointer = self.global(mesh.prim);
        self.b.push(Statement::Store { pointer, value });
        let pointer = self.global(mesh.prim_set);
        let value = self.b.emit(E::Literal(Literal::Bool(true)));
        self.b.push(Statement::Store { pointer, value });
        if let Some(condition) = active {
            let accept = self.b.close();
            self.b.push(Statement::If {
                condition,
                accept,
                reject: Block::new(),
            });
        }
        Ok(())
    }

    /// The part of a dword an `sdwa` selector names: a byte, a 16-bit half or the whole.
    fn sdwa_part(
        &mut self,
        pc: u32,
        inst: &Inst,
        value: Handle<E>,
        sel: u32,
        signed: bool,
    ) -> Value {
        Ok(match sel {
            0..=3 => self.field(value, 8 * sel, 8, signed),
            4 | 5 => self.field(value, 16 * (sel - 4), 16, signed),
            6 => value,
            _ => return Err(form(pc, inst, "a part selector that names nothing")),
        })
    }

    /// The sources of an instruction whose trailing word selects a part of each: the word names the
    /// first, `second` is the instruction's own register field.
    fn sdwa_sources(
        &mut self,
        pc: u32,
        inst: &Inst,
        word: u32,
        second: Option<u8>,
    ) -> Result<(Handle<E>, Option<Handle<E>>), NotTranslated> {
        // Bits 13 to 15 clamp and scale the result; 20, 21, 28 and 29 negate or take the magnitude of a source.
        if word & 0x3030_E000 != 0 {
            return Err(form(pc, inst, "a float modifier in its part selection"));
        }
        let register = |cx: &mut Self, number: u8, scalar: bool| match scalar {
            true => cx.sreg(pc, number),
            false => cx.vreg(pc, number),
        };
        let first = register(self, word as u8, (word >> 23) & 1 != 0)?;
        let first = self.sdwa_part(pc, inst, first, (word >> 16) & 7, (word >> 19) & 1 != 0)?;
        let second = match second {
            Some(number) => {
                let value = register(self, number, word >> 31 != 0)?;
                Some(self.sdwa_part(pc, inst, value, (word >> 24) & 7, (word >> 27) & 1 != 0)?)
            }
            None => None,
        };
        Ok((first, second))
    }

    /// Writes a result to the part of the destination an `sdwa` word selects, the rest of it zero or kept.
    fn sdwa_put(&mut self, pc: u32, inst: &Inst, word: u32, dst: u8, value: Handle<E>) -> Done {
        let (sel, unused) = ((word >> 8) & 7, (word >> 11) & 3);
        let (first, width) = match sel {
            0..=3 => (8 * sel, 8),
            4 | 5 => (16 * (sel - 4), 16),
            6 => {
                self.vput(dst, value);
                return Ok(());
            }
            _ => return Err(form(pc, inst, "a part selector that names nothing")),
        };
        let mask = ((1u32 << width) - 1) << first;
        let (by, bits) = (self.b.u32(first), self.b.u32(mask));
        let raised = self.b.bin(Bo::ShiftLeft, value, by);
        let placed = self.b.bin(Bo::And, raised, bits);
        let value = match unused {
            0 => placed,
            2 => {
                let (old, rest) = (self.raw(Reg::V(dst)), self.b.u32(!mask));
                let kept = self.b.bin(Bo::And, old, rest);
                self.b.bin(Bo::InclusiveOr, kept, placed)
            }
            _ => return Err(form(pc, inst, "a sign-extended part of the destination")),
        };
        self.vput(dst, value);
        Ok(())
    }

    /// Ends this lane: a pixel lane the program switched off writes nothing.
    fn finish_lane(&mut self) {
        if self.fragment && self.masked {
            let active = self.active();
            let condition = self.b.lnot(active);
            let mut accept = Block::new();
            accept.push(Statement::Kill, Span::UNDEFINED);
            self.b.push(Statement::If {
                condition,
                accept,
                reject: Block::new(),
            });
        }
        self.b.push(Statement::Return { value: None });
    }

    /// The blocks as one loop over a switch on the number of the block to run next, for a graph that
    /// no nest of ifs and loops reproduces.
    fn dispatch(&mut self, ends: &[End], blocks: Vec<Block>) {
        let entry = self.b.u32(0);
        self.put(Reg::Next, entry);
        self.b.open();
        let selector = self.raw(Reg::Next);
        let mut cases = Vec::with_capacity(blocks.len() + 1);
        for (index, (statements, end)) in blocks.into_iter().zip(ends).enumerate() {
            self.b.open();
            self.b.push(Statement::Block(statements));
            match *end {
                End::Return => self.finish_lane(),
                End::Jump(to) => {
                    let to = self.b.u32(to);
                    self.put(Reg::Next, to);
                }
                End::Branch { then, otherwise } => {
                    let condition = self.raw(Reg::Cond(index as u32));
                    let (then, otherwise) = (self.b.u32(then), self.b.u32(otherwise));
                    let to = self.b.select(condition, then, otherwise);
                    self.put(Reg::Next, to);
                }
            }
            cases.push(naga::SwitchCase {
                value: naga::SwitchValue::U32(index as u32),
                body: self.b.close(),
                fall_through: false,
            });
        }
        self.b.open();
        self.b.push(Statement::Return { value: None });
        cases.push(naga::SwitchCase {
            value: naga::SwitchValue::Default,
            body: self.b.close(),
            fall_through: false,
        });
        self.b.push(Statement::Switch { selector, cases });
        let body = self.b.close();
        self.b.push(Statement::Loop {
            body,
            continuing: Block::new(),
            break_if: None,
        });
    }

    /// The private `vec4` a pixel input arrives in, by the attribute number the code names.
    fn attribute(
        &mut self,
        pc: u32,
        inst: &Inst,
        attr: u8,
        interpolated: bool,
    ) -> Result<Handle<naga::GlobalVariable>, NotTranslated> {
        let Some(input) = self.program.inputs.get(usize::from(attr)) else {
            return Err(form(
                pc,
                inst,
                "an attribute its package's input table does not list",
            ));
        };
        let slot = input.slot;
        if self.halves.keys().any(|(held, _)| *held == slot) {
            return Err(form(pc, inst, "an input read both whole and as halves"));
        }
        if let Some((global, any)) = self.attributes.get_mut(&slot) {
            *any |= interpolated;
            return Ok(*global);
        }
        let ty = vector(&mut self.module, VectorSize::Quad, Scalar::F32);
        let global = self.private(format!("attr{slot}"), ty);
        self.attributes.insert(slot, (global, interpolated));
        Ok(global)
    }

    fn vintrp(&mut self, pc: u32, inst: &Inst, name: &str, dst: u8, attr: u8, chan: u8) -> Done {
        if !self.fragment {
            return Err(refused(pc, inst));
        }
        // The first step's partial sum is only ever the second step's input, which reads the
        // whole interpolated value from the stage instead.
        // ponytail: the stage's own interpolation stands in for the barycentric pair the
        // instruction reads, so a centroid or sample pair reads as the declared one.
        if name == "v_interp_p1_f32" {
            self.defined.insert(Reg::V(dst));
            return Ok(());
        }
        let global = self.attribute(pc, inst, attr, name != "v_interp_mov_f32")?;
        let pointer = self.global(global);
        let cell = self.b.index(pointer, u32::from(chan & 3));
        let value = self.b.load(cell);
        let bits = self.b.bits(value);
        self.vput(dst, bits);
        Ok(())
    }

    fn export_global(&mut self, target: u8) -> Handle<naga::GlobalVariable> {
        if let Some(&global) = self.exports.get(&target) {
            return global;
        }
        let ty = vector(&mut self.module, VectorSize::Quad, Scalar::F32);
        let global = self.private(format!("out{target}"), ty);
        self.exports.insert(target, global);
        global
    }

    fn exp(
        &mut self,
        pc: u32,
        inst: &Inst,
        target: u8,
        en: u8,
        srcs: [u8; 4],
        compr: bool,
    ) -> Done {
        if self.mesh.is_some() && target == PRIM {
            return self.export_primitive(pc, en, srcs[0]);
        }
        // An export of nothing is the one a wave with no vertex still has to make.
        if !self.fragment && en == 0 {
            return Ok(());
        }
        let colour = target < MRT_COUNT;
        let known = match self.fragment {
            true => colour || target == MRTZ || target == NULL_TARGET,
            false => target == POS0 || target >= PARAM0,
        };
        if !known {
            return Err(form(pc, inst, "that target"));
        }
        if target == NULL_TARGET {
            return Ok(());
        }
        if compr && !colour {
            return Err(form(
                pc,
                inst,
                "packed halves to a target that is not a colour",
            ));
        }
        if target == MRTZ && en != 1 {
            return Err(form(pc, inst, "a stencil, a sample mask or an alpha"));
        }
        let mut parts = Vec::with_capacity(4);
        for i in 0..4u8 {
            // Packed, each source holds two components as halves, and its enable bit is the pair's first.
            let (source, enabled) = match compr {
                true => (srcs[usize::from(i / 2)], en >> (i & !1) & 1 != 0),
                false => (srcs[usize::from(i)], en >> i & 1 != 0),
            };
            parts.push(match (enabled, compr) {
                (false, _) => self.b.f32(0.0),
                (true, false) => {
                    let bits = self.vreg(pc, source)?;
                    self.b.float(bits)
                }
                (true, true) => {
                    let bits = self.vreg(pc, source)?;
                    self.half(bits, i % 2 == 1)
                }
            });
        }
        let masked = self.masked;
        let active = masked.then(|| self.active());
        if masked {
            self.b.open();
        }
        if target == MRTZ {
            let global = match self.depth {
                Some(global) => global,
                None => {
                    let ty = self.ty(TypeInner::Scalar(Scalar::F32));
                    let global = self.private("depth".to_owned(), ty);
                    self.depth = Some(global);
                    global
                }
            };
            let pointer = self.global(global);
            self.b.push(Statement::Store {
                pointer,
                value: parts[0],
            });
        } else {
            let ty = vector(&mut self.module, VectorSize::Quad, Scalar::F32);
            let global = self.export_global(target);
            let pointer = self.global(global);
            let value = self.b.emit(E::Compose {
                ty,
                components: parts,
            });
            self.b.push(Statement::Store { pointer, value });
            if let (Some(mesh), POS0) = (self.mesh, target) {
                let pointer = self.global(mesh.pos_set);
                let value = self.b.emit(E::Literal(Literal::Bool(true)));
                self.b.push(Statement::Store { pointer, value });
            }
        }
        if let Some(condition) = active {
            let accept = self.b.close();
            self.b.push(Statement::If {
                condition,
                accept,
                reject: Block::new(),
            });
        }
        Ok(())
    }

    /// The register of an image instruction's `i`th address operand: listed one by one, or the `i`th from the first.
    fn address(&mut self, pc: u32, inst: &Inst, addrs: &[u8], i: usize) -> Value {
        let register = match addrs {
            [first] => u8::try_from(usize::from(*first) + i).ok(),
            listed => listed.get(i).copied(),
        };
        let register = register.ok_or_else(|| form(pc, inst, "an address past its registers"))?;
        self.vreg(pc, register)
    }

    #[allow(clippy::too_many_lines)]
    fn image(&mut self, pc: u32, inst: &Inst, name: &str) -> Done {
        let Inst::Mimg {
            addrs,
            data,
            rsrc,
            samp,
            dmask,
            dim,
            a16,
            d16,
            ..
        } = inst
        else {
            return Err(refused(pc, inst));
        };
        if *a16 || *d16 {
            return Err(form(pc, inst, "16-bit addresses or data"));
        }
        let (dimension, arrayed, coords) = match dim {
            1 => (ImageDimension::D2, false, 2usize),
            5 => (ImageDimension::D2, true, 2),
            2 => (ImageDimension::D3, false, 3),
            _ => return Err(form(pc, inst, "that image dimension")),
        };
        let load = matches!(name, "image_load" | "image_load_mip");
        let tokens: Vec<&str> = match name.strip_prefix("image_sample") {
            Some(rest) => rest.split('_').filter(|t| !t.is_empty()).collect(),
            None if load => Vec::new(),
            None => return Err(refused(pc, inst)),
        };
        let (compared, level) = match tokens.as_slice() {
            [] => (false, "auto"),
            ["lz"] => (false, "zero"),
            ["l"] => (false, "exact"),
            ["b"] => (false, "bias"),
            ["d"] => (false, "gradient"),
            ["c"] => (true, "auto"),
            ["c", "lz"] => (true, "zero"),
            _ => return Err(refused(pc, inst)),
        };
        if !load && !self.fragment && matches!(level, "auto" | "bias") {
            return Err(form(pc, inst, "an implicit level outside a pixel program"));
        }
        let Some(at) = sym::descriptor(&self.state, *rsrc, 8) else {
            return Err(NotTranslated::Descriptor {
                pc,
                what: "an image whose descriptor it did not load from a table",
            });
        };
        let texture = match self.textures.get(&(at.clone(), *dim, compared)) {
            Some(&global) => global,
            None => {
                let class = match compared {
                    true => ImageClass::Depth { multi: false },
                    false => ImageClass::Sampled {
                        kind: ScalarKind::Float,
                        multi: false,
                    },
                };
                let global = self.bound(
                    format!("tex{}", self.textures.len()),
                    TypeInner::Image {
                        dim: dimension,
                        arrayed,
                        class,
                    },
                    naga::AddressSpace::Handle,
                    ResourceKind::Texture {
                        dim: *dim,
                        compared,
                    },
                    Some(at.clone()),
                );
                self.textures.insert((at, *dim, compared), global);
                global
            }
        };
        let image = self.global(texture);
        let mut next = 0usize;
        let texel = if load {
            // Integer coordinates, the slice of an array after them, then the level where the mnemonic takes one.
            let mut parts = Vec::with_capacity(coords);
            for _ in 0..coords {
                let bits = self.address(pc, inst, addrs, next)?;
                parts.push(self.b.sint(bits));
                next += 1;
            }
            let array_index = match arrayed {
                true => {
                    let bits = self.address(pc, inst, addrs, next)?;
                    next += 1;
                    Some(self.b.sint(bits))
                }
                false => None,
            };
            let level = match name {
                "image_load_mip" => {
                    let bits = self.address(pc, inst, addrs, next)?;
                    self.b.sint(bits)
                }
                _ => self.b.emit(E::Literal(Literal::I32(0))),
            };
            let ty = vector(&mut self.module, size_of(coords), Scalar::I32);
            let coordinate = self.b.emit(E::Compose {
                ty,
                components: parts,
            });
            // ponytail: the texel is read as floats; an integer format needs the descriptor's own format, known only at run time.
            self.note("an image loaded as floats");
            self.b.emit(E::ImageLoad {
                image,
                coordinate,
                array_index,
                sample: None,
                level: Some(level),
            })
        } else {
            let sampler = match (
                sym::descriptor(&self.state, *samp, 4),
                sym::literal(&self.state, *samp),
            ) {
                (Some(at), _) => self.sampler(Some(at), None, compared),
                (None, Some(words)) => self.sampler(None, Some(words), compared),
                (None, None) => {
                    return Err(NotTranslated::Descriptor {
                        pc,
                        what: "a sampler whose descriptor it did not load from a table",
                    })
                }
            };
            let sampler = self.global(sampler);
            // The address operands come in this order: bias, reference, gradients, coordinates, level.
            let float = |cx: &mut Self, next: &mut usize| -> Value {
                let bits = cx.address(pc, inst, addrs, *next)?;
                *next += 1;
                Ok(cx.b.float(bits))
            };
            let bias = match level {
                "bias" => Some(float(self, &mut next)?),
                _ => None,
            };
            let depth_ref = match compared {
                true => Some(float(self, &mut next)?),
                false => None,
            };
            let ty = vector(&mut self.module, size_of(coords), Scalar::F32);
            let mut vectors = Vec::with_capacity(3);
            let groups = if level == "gradient" { 3 } else { 1 };
            for _ in 0..groups {
                let mut parts = Vec::with_capacity(coords);
                for _ in 0..coords {
                    parts.push(float(self, &mut next)?);
                }
                vectors.push(self.b.emit(E::Compose {
                    ty,
                    components: parts,
                }));
            }
            let coordinate = vectors[groups - 1];
            let array_index = match arrayed {
                true => {
                    let slice = float(self, &mut next)?;
                    let nearest = self.b.math(Mf::Round, &[slice]);
                    Some(self.b.convert(nearest, ScalarKind::Sint))
                }
                false => None,
            };
            let level = match level {
                "zero" => SampleLevel::Zero,
                "exact" => SampleLevel::Exact(float(self, &mut next)?),
                "bias" => SampleLevel::Bias(bias.unwrap_or(coordinate)),
                "gradient" => SampleLevel::Gradient {
                    x: vectors[0],
                    y: vectors[1],
                },
                _ => SampleLevel::Auto,
            };
            self.b.emit(E::ImageSample {
                image,
                sampler,
                gather: None,
                coordinate,
                array_index,
                offset: None,
                level,
                depth_ref,
                clamp_to_edge: false,
            })
        };
        // The enabled components land in consecutive registers from `data`; a comparison gives one.
        let mut to = *data;
        for c in (0..4u32).filter(|c| dmask >> c & 1 != 0) {
            let part = match compared {
                true if c == 0 => texel,
                true => self.b.f32(0.0),
                false => self.b.index(texel, c),
            };
            let bits = self.b.bits(part);
            self.vput(to, bits);
            to = to.wrapping_add(1);
        }
        Ok(())
    }

    fn inst(&mut self, pc: u32, inst: &Inst) -> Done {
        let name = inst.name();
        match inst {
            Inst::Sopp { .. } => self.sopp(pc, inst, name),
            Inst::Sop1 { dst, src, .. } => self.sop1(pc, inst, name, *dst, *src),
            Inst::Sop2 {
                dst, src0, src1, ..
            } => self.sop2(pc, inst, name, *dst, *src0, *src1),
            Inst::Sopk {
                dst, imm, imm32, ..
            } => self.sopk(pc, inst, name, *dst, *imm, *imm32),
            Inst::Sopc { src0, src1, .. } => self.sopc(pc, inst, name, *src0, *src1),
            Inst::Smem {
                base,
                data,
                offset,
                soffset,
                ..
            } => self.smem(pc, inst, name, *base, *data, *offset, *soffset),
            Inst::Vop1 {
                dst, src, extra, ..
            } => self.vop1(pc, inst, name, *dst, *src, *extra),
            Inst::Vop2 {
                dst,
                src0,
                src1,
                extra,
                ..
            } => self.vop2(pc, inst, name, *dst, *src0, *src1, *extra),
            Inst::Vopc { src0, src1, .. } => {
                let a = self.sv(pc, *src0)?;
                let b = self.vreg(pc, *src1)?;
                let condition = self.compare(name, a, b).ok_or_else(|| refused(pc, inst))?;
                self.put_compare(name, VCC, condition);
                Ok(())
            }
            Inst::Vop3 { dst, srcs, .. }
                if name.starts_with("v_interp_p") && name.ends_with("_f16") =>
            {
                self.interp_half(pc, inst, name, *dst, srcs[0])
            }
            Inst::Vop3 {
                op,
                dst,
                srcs,
                abs,
                neg,
                opsel,
                clamp,
                omod,
            } => self.vop3(
                pc,
                inst,
                name,
                *dst,
                *srcs,
                (*abs, *neg, *opsel),
                (*clamp, *omod),
                usize::from(op.sources()),
            ),
            Inst::Vop3p {
                dst,
                srcs,
                opsel_lo,
                opsel_hi,
                neg_lo,
                neg_hi,
                clamp,
                ..
            } if matches!(
                name,
                "v_fma_mix_f32" | "v_fma_mixlo_f16" | "v_fma_mixhi_f16"
            ) =>
            {
                self.fma_mix(
                    pc,
                    name,
                    *dst,
                    *srcs,
                    (*opsel_lo, *opsel_hi),
                    (*neg_lo, *neg_hi),
                    *clamp,
                )
            }
            Inst::Vintrp {
                dst, attr, chan, ..
            } => self.vintrp(pc, inst, name, *dst, *attr, *chan),
            Inst::Exp {
                target,
                en,
                srcs,
                compr,
                ..
            } => self.exp(pc, inst, *target, *en, *srcs, *compr),
            Inst::Ds {
                addr,
                dst,
                offset0,
                offset1,
                ..
            } if name == "ds_swizzle_b32" => {
                // The 16-bit pattern's top bit says it permutes each quad's four lanes, two bits a lane.
                if offset1 & 0x80 == 0 {
                    return Err(form(pc, inst, "a pattern that is not a quad's permutation"));
                }
                let quad = [0, 2, 4, 6].map(|low| offset0 >> low & 3);
                self.permute(*dst, *addr, quad);
                Ok(())
            }
            Inst::Mimg { .. } => self.image(pc, inst, name),
            Inst::Mubuf { .. } => self.mubuf(pc, inst, name),
            Inst::Mtbuf { .. } => self.mtbuf(pc, inst, name),
            _ => Err(refused(pc, inst)),
        }
    }

    /// Lowers one block alone: every value crosses blocks in a local, so its statements stand by themselves.
    fn block(
        &mut self,
        index: u32,
        block: &super::cfg::Block,
        state: State,
    ) -> Result<Lowering, NotTranslated> {
        self.index = index;
        self.state = state;
        self.used.clear();
        self.defined.clear();
        self.versions.clear();
        self.permuted.clear();
        self.b.open();
        for (pc, inst) in &block.insts {
            self.inst(*pc, inst)?;
            sym::step(&mut self.state, inst);
        }
        Ok(Lowering {
            block: self.b.close(),
            used: std::mem::take(&mut self.used),
            defined: std::mem::take(&mut self.defined),
            permuted: self.permuted.keys().copied().collect(),
        })
    }

    /// The structured statements of `nodes`, each block's own statements taken from `blocks`.
    fn nodes(&mut self, nodes: &[Node], blocks: &mut [Option<Block>]) -> Done {
        for node in nodes {
            match node {
                Node::Code(block) => {
                    let statements = (blocks.get_mut(*block as usize))
                        .and_then(Option::take)
                        .ok_or_else(|| {
                            NotTranslated::Structure(format!("block {block} is emitted twice"))
                        })?;
                    self.b.push(Statement::Block(statements));
                }
                Node::Edge { .. } => {}
                Node::If {
                    cond,
                    then,
                    otherwise,
                } => {
                    let condition = self.raw(Reg::Cond(*cond));
                    let accept = self.nested(then, blocks)?;
                    let reject = self.nested(otherwise, blocks)?;
                    self.b.push(Statement::If {
                        condition,
                        accept,
                        reject,
                    });
                }
                Node::Loop { body } => {
                    let body = self.nested(body, blocks)?;
                    self.b.push(Statement::Loop {
                        body,
                        continuing: Block::new(),
                        break_if: None,
                    });
                }
                Node::Break => self.b.push(Statement::Break),
                Node::Continue => self.b.push(Statement::Continue),
                Node::Return(_) | Node::Unreachable => self.finish_lane(),
                Node::Switch { .. } => {
                    return Err(NotTranslated::Structure("a switch".to_owned()));
                }
            }
        }
        Ok(())
    }

    fn nested(
        &mut self,
        nodes: &[Node],
        blocks: &mut [Option<Block>],
    ) -> Result<Block, NotTranslated> {
        self.b.open();
        let lowered = self.nodes(nodes, blocks);
        let block = self.b.close();
        lowered.map(|()| block)
    }

    /// Gives each register the entry reads before any write its value, or refuses the read.
    fn live_ins(&mut self, live: &BTreeMap<Reg, u32>) -> Done {
        let (one, zero) = (self.b.u32(1), self.b.u32(0));
        self.put(Reg::S(EXEC), one);
        self.put(Reg::S(EXEC + 1), zero);
        for (&reg, &pc) in live {
            match reg {
                Reg::S(r) if r == EXEC || r == EXEC + 1 => {}
                // A mesh thread's launch number is its group's first plus its own; the wave it is in
                // sits in the top byte of the third scalar register.
                Reg::V(5) | Reg::S(3) if self.mesh.is_some() => {
                    let Some(mesh) = self.mesh else { continue };
                    let (thread, group) = self.mesh_ids(mesh);
                    let value = match reg {
                        Reg::V(_) => {
                            let threads = self.b.u32(MESH_THREADS);
                            let first = self.b.bin(Bo::Multiply, group, threads);
                            self.b.bin(Bo::Add, first, thread)
                        }
                        _ => {
                            let (lanes, top) = (self.b.u32(WAVE.trailing_zeros()), self.b.u32(24));
                            let wave = self.b.bin(Bo::ShiftRight, thread, lanes);
                            self.b.bin(Bo::ShiftLeft, wave, top)
                        }
                    };
                    self.put(reg, value);
                }
                Reg::S(r) if r < USER_REGISTERS => {
                    let global = match self.user_data {
                        Some(global) => global,
                        None => {
                            let inner = self.rows(u32::from(USER_REGISTERS) / 4);
                            let global = self.bound(
                                "user_data".to_owned(),
                                inner,
                                naga::AddressSpace::Uniform,
                                ResourceKind::UserData,
                                None,
                            );
                            self.user_data = Some(global);
                            global
                        }
                    };
                    let pointer = self.global(global);
                    let row = self.b.index(pointer, u32::from(r) / 4);
                    let cell = self.b.index(row, u32::from(r) % 4);
                    let value = self.b.load(cell);
                    self.put(reg, value);
                }
                Reg::V(n) if self.fragment && n >= SYSTEM_BASE => {
                    let value = (self.system_input(n - SYSTEM_BASE)).ok_or_else(|| {
                        NotTranslated::LiveIn {
                            pc,
                            register: reg.name(),
                        }
                    })?;
                    self.put(reg, value);
                }
                Reg::V(0) if !self.fragment && self.mesh.is_none() => {
                    let global = match self.vertex_index {
                        Some(global) => global,
                        None => {
                            let ty = self.ty(TypeInner::Scalar(Scalar::U32));
                            let global = self.private("vertex_index".to_owned(), ty);
                            self.vertex_index = Some(global);
                            global
                        }
                    };
                    let pointer = self.global(global);
                    let value = self.b.load(pointer);
                    self.put(reg, value);
                }
                _ => {
                    return Err(NotTranslated::LiveIn {
                        pc,
                        register: reg.name(),
                    })
                }
            }
        }
        Ok(())
    }
}

/// What each block's entry may read before anything writes it, by the first word that reads it.
fn liveness(lowered: &[Lowering], successors: &[Vec<usize>]) -> Vec<BTreeMap<Reg, u32>> {
    let mut live: Vec<BTreeMap<Reg, u32>> = lowered.iter().map(|l| l.used.clone()).collect();
    let mut changed = true;
    while changed {
        changed = false;
        for block in (0..lowered.len()).rev() {
            for &next in &successors[block] {
                let incoming: Vec<(Reg, u32)> = (live[next].iter())
                    .filter(|(reg, _)| !lowered[block].defined.contains(*reg))
                    .map(|(&reg, &pc)| (reg, pc))
                    .collect();
                for (reg, pc) in incoming {
                    if let std::collections::btree_map::Entry::Vacant(slot) = live[block].entry(reg)
                    {
                        slot.insert(pc);
                        changed = true;
                    }
                }
            }
        }
    }
    live
}

/// The first read of a register that may still hold a neighbouring lane's value from another block.
fn stale_permutation(lowered: &[Lowering], successors: &[Vec<usize>]) -> Option<u32> {
    let mut entering: Vec<BTreeSet<u8>> = vec![BTreeSet::new(); lowered.len()];
    let mut changed = true;
    while changed {
        changed = false;
        for (block, l) in lowered.iter().enumerate() {
            let mut leaving: BTreeSet<u8> = (entering[block].iter())
                .filter(|&&n| !l.defined.contains(&Reg::V(n)))
                .copied()
                .collect();
            leaving.extend(&l.permuted);
            for &next in &successors[block] {
                let before = entering[next].len();
                entering[next].extend(&leaving);
                changed |= entering[next].len() != before;
            }
        }
    }
    (lowered.iter().zip(&entering))
        .find_map(|(l, entering)| (entering.iter()).find_map(|&n| l.used.get(&Reg::V(n)).copied()))
}

/// Whether any instruction of the graph changes the lane mask.
fn masks_lanes(cfg: &Cfg) -> bool {
    cfg.blocks.values().flat_map(|b| &b.insts).any(|(_, inst)| {
        let name = inst.name();
        let writes_exec = match inst {
            Inst::Sop1 { dst, .. } | Inst::Sop2 { dst, .. } | Inst::Sopk { dst, .. } => {
                matches!(dst, Operand::ExecLo | Operand::ExecHi)
            }
            _ => false,
        };
        writes_exec || name.starts_with("v_cmpx") || name.contains("exec")
    })
}

/// How a block hands control on, by block number.
#[derive(Debug, Clone, Copy)]
enum End {
    Return,
    Jump(u32),
    Branch { then: u32, otherwise: u32 },
}

/// The naga module of a pixel or vertex program that stands alone.
pub fn lower(program: &Program, cfg: &Cfg) -> Result<Lowered, NotTranslated> {
    lower_with(program, cfg, &Options::default())
}

/// The naga module of a pixel or vertex program, told what its neighbours need of it.
#[allow(clippy::too_many_lines)]
pub fn lower_with(
    program: &Program,
    cfg: &Cfg,
    options: &Options,
) -> Result<Lowered, NotTranslated> {
    let stage = match program.kind {
        ShaderKind::Pixel => naga::ShaderStage::Fragment,
        // A mesh program runs as a vertex program: its body once for a primitive and once for the
        // vertex at this corner of it, since a thread places one of each.
        ShaderKind::Vertex | ShaderKind::Mesh => naga::ShaderStage::Vertex,
        other => return Err(NotTranslated::Stage(format!("{other:?}").to_lowercase())),
    };
    let fragment = stage == naga::ShaderStage::Fragment;

    // The graph as the structurizer reads one: a terminator a block, a branch's condition named by its block.
    let starts: Vec<u32> = cfg.blocks.keys().copied().collect();
    let place = |start: u32| starts.binary_search(&start).unwrap_or(0) as u32;
    let mut body = Body {
        blocks: Vec::with_capacity(starts.len()),
        values: Vec::new(),
    };
    let mut successors = Vec::with_capacity(starts.len());
    let mut ends = Vec::with_capacity(starts.len());
    for (index, block) in cfg.blocks.values().enumerate() {
        let op = match block.terminator {
            Terminator::End => IrOp::Ret { value: None },
            Terminator::Jump(to) | Terminator::Fall(to) => IrOp::Br { target: place(to) },
            Terminator::Branch { taken, fallthrough } => IrOp::CondBr {
                cond: index as u32,
                then: place(taken),
                otherwise: place(fallthrough),
            },
            Terminator::Indirect => {
                let pc = block.insts.last().map_or(block.start, |(pc, _)| *pc);
                return Err(NotTranslated::Indirect { pc });
            }
        };
        let end = match &op {
            IrOp::Br { target } => End::Jump(*target),
            IrOp::CondBr {
                then, otherwise, ..
            } => End::Branch {
                then: *then,
                otherwise: *otherwise,
            },
            _ => End::Return,
        };
        successors.push(match end {
            End::Jump(to) => vec![to as usize],
            End::Branch { then, otherwise } => vec![then as usize, otherwise as usize],
            End::Return => Vec::new(),
        });
        ends.push(end);
        body.blocks.push(BasicBlock {
            insts: vec![IrInst {
                result: None,
                ty: 0,
                op,
            }],
        });
    }
    // A graph the structurizer does not nest still runs, block by block, in a dispatch loop.
    let nodes = structure(&body).ok();

    let mut cx = Cx {
        program,
        module: naga::Module::default(),
        b: Builder::new(),
        fragment,
        masked: masks_lanes(cfg),
        locals: HashMap::new(),
        index: 0,
        state: sym::entry(),
        used: BTreeMap::new(),
        defined: BTreeSet::new(),
        versions: HashMap::new(),
        permuted: HashMap::new(),
        mesh: None,
        half_slots: &options.half_slots,
        attributes: BTreeMap::new(),
        halves: BTreeMap::new(),
        frag_coord: None,
        front_face: None,
        fetched: BTreeMap::new(),
        vertex_index: None,
        exports: BTreeMap::new(),
        depth: None,
        user_data: None,
        buffers: BTreeMap::new(),
        textures: BTreeMap::new(),
        samplers: BTreeMap::new(),
        storage: BTreeMap::new(),
        resources: Vec::new(),
        notes: BTreeMap::new(),
    };
    if program.kind == ShaderKind::Mesh {
        let word = cx.ty(TypeInner::Scalar(Scalar::U32));
        let flag = cx.ty(TypeInner::Scalar(Scalar::BOOL));
        cx.mesh = Some(Mesh {
            thread: cx.private("thread".to_owned(), word),
            group: cx.private("group".to_owned(), word),
            prim: cx.private("prim".to_owned(), word),
            prim_set: cx.private("prim_set".to_owned(), flag),
            pos_set: cx.private("pos_set".to_owned(), flag),
        });
    }
    let states = sym::solve(cfg);
    let mut lowered = Vec::with_capacity(starts.len());
    for (index, (start, block)) in cfg.blocks.iter().enumerate() {
        let state = states.get(start).cloned().unwrap_or_else(sym::entry);
        lowered.push(cx.block(index as u32, block, state)?);
    }
    if let Some(pc) = stale_permutation(&lowered, &successors) {
        return Err(NotTranslated::Form {
            pc,
            name: "ds_swizzle_b32".to_owned(),
            what: "a neighbouring lane's value read in another block",
        });
    }
    let live = liveness(&lowered, &successors);
    // Nothing in the entry block is written yet, so the stores land before every block's statements.
    cx.used.clear();
    cx.defined.clear();
    cx.live_ins(live.first().unwrap_or(&BTreeMap::new()))?;
    match nodes {
        Some(nodes) => {
            let mut blocks: Vec<Option<Block>> =
                lowered.into_iter().map(|l| Some(l.block)).collect();
            cx.nodes(&nodes, &mut blocks)?;
        }
        None => {
            cx.note("a dispatch loop for a graph that does not nest");
            cx.dispatch(&ends, lowered.into_iter().map(|l| l.block).collect());
        }
    }
    if !fragment && !cx.exports.contains_key(&POS0) {
        return Err(NotTranslated::NoPosition);
    }

    let Cx {
        mut module,
        b,
        mesh,
        half_slots,
        attributes,
        halves,
        frag_coord,
        front_face,
        fetched,
        vertex_index,
        exports,
        depth,
        buffers,
        mut resources,
        notes,
        ..
    } = cx;
    // A constant buffer is declared as long as the furthest row any read of it reaches.
    for (at, (global, rows)) in buffers {
        let base = vector(&mut module, VectorSize::Quad, Scalar::U32);
        let ty = insert(
            &mut module,
            TypeInner::Array {
                base,
                size: naga::ArraySize::Constant(NonZeroU32::new(rows).unwrap_or(NonZeroU32::MIN)),
                stride: 16,
            },
        );
        module.global_variables[global].ty = ty;
        for resource in &mut resources {
            if resource.at.as_ref() == Some(&at)
                && matches!(resource.kind, ResourceKind::ConstantBuffer { .. })
            {
                resource.kind = ResourceKind::ConstantBuffer { rows };
            }
        }
    }
    let mut function = b.finish();
    function.name = Some("main_body".into());
    let function = module.functions.append(function, Span::UNDEFINED);

    // The entry point: the stage's inputs into the private variables, the body, then the exports out of theirs.
    let vec4 = vector(&mut module, VectorSize::Quad, Scalar::F32);
    let mut w = Builder::new();
    let mut arguments = Vec::new();
    let mut argument = |w: &mut Builder, name: String, ty, binding| {
        let index = arguments.len() as u32;
        arguments.push(naga::FunctionArgument {
            name: Some(name),
            ty,
            binding: Some(binding),
        });
        w.emit(E::FunctionArgument(index))
    };
    let location = |location: u32, flat: bool| naga::Binding::Location {
        location,
        interpolation: Some(match flat {
            true => naga::Interpolation::Flat,
            false => naga::Interpolation::Perspective,
        }),
        sampling: (!flat).then_some(naga::Sampling::Center),
        blend_src: None,
        per_primitive: false,
    };
    let store = |w: &mut Builder, global, value| {
        let pointer = w.emit(E::GlobalVariable(global));
        w.push(Statement::Store { pointer, value });
    };
    for (&slot, &(global, interpolated)) in &attributes {
        let binding = location(u32::from(slot), !interpolated);
        let value = argument(&mut w, format!("attr{slot}"), vec4, binding);
        store(&mut w, global, value);
    }
    for (&(slot, high), &global) in &halves {
        let at = u32::from(slot) + if high { HIGH_HALVES } else { 0 };
        let half = if high { "high" } else { "low" };
        let value = argument(
            &mut w,
            format!("attr{slot}_{half}"),
            vec4,
            location(at, false),
        );
        store(&mut w, global, value);
    }
    if let Some(global) = frag_coord {
        let binding = naga::Binding::BuiltIn(naga::BuiltIn::Position { invariant: false });
        let value = argument(&mut w, "frag_coord".to_owned(), vec4, binding);
        store(&mut w, global, value);
    }
    if let Some(global) = front_face {
        let ty = insert(&mut module, TypeInner::Scalar(Scalar::BOOL));
        let binding = naga::Binding::BuiltIn(naga::BuiltIn::FrontFacing);
        let value = argument(&mut w, "front_facing".to_owned(), ty, binding);
        store(&mut w, global, value);
    }
    for (&register, &(global, scalar)) in &fetched {
        let ty = vector(&mut module, VectorSize::Quad, scalar);
        let binding = naga::Binding::Location {
            location: register,
            interpolation: None,
            sampling: None,
            blend_src: None,
            per_primitive: false,
        };
        let value = argument(&mut w, format!("in{register}"), ty, binding);
        store(&mut w, global, value);
    }
    if let Some(global) = vertex_index {
        let ty = insert(&mut module, TypeInner::Scalar(Scalar::U32));
        let binding = naga::Binding::BuiltIn(naga::BuiltIn::VertexIndex);
        let value = argument(&mut w, "vertex_index".to_owned(), ty, binding);
        store(&mut w, global, value);
    }
    let call = || Statement::Call {
        function,
        arguments: Vec::new(),
        result: None,
    };
    match mesh {
        None => w.push(call()),
        // Instance `g` is group `g`; its vertices come three to a primitive, in the order of the
        // threads that place them.
        Some(mesh) => {
            let word = insert(&mut module, TypeInner::Scalar(Scalar::U32));
            let binding = naga::Binding::BuiltIn(naga::BuiltIn::VertexIndex);
            let vertex = argument(&mut w, "vertex_index".to_owned(), word, binding);
            let binding = naga::Binding::BuiltIn(naga::BuiltIn::InstanceIndex);
            let instance = argument(&mut w, "instance_index".to_owned(), word, binding);
            store(&mut w, mesh.group, instance);
            let three = w.u32(3);
            let (primitive, corner) = (
                w.bin(Bo::Divide, vertex, three),
                w.bin(Bo::Modulo, vertex, three),
            );
            let no = w.emit(E::Literal(Literal::Bool(false)));
            store(&mut w, mesh.thread, primitive);
            store(&mut w, mesh.prim_set, no);
            w.push(call());
            // The primitive's dword holds its three vertex numbers ten bits apart and a cull bit on top.
            let pointer = w.emit(E::GlobalVariable(mesh.prim));
            let prim = w.load(pointer);
            let pointer = w.emit(E::GlobalVariable(mesh.prim_set));
            let made = w.load(pointer);
            let (ten, nine_bits, top) = (w.u32(10), w.u32(0x1FF), w.u32(31));
            let by = w.bin(Bo::Multiply, corner, ten);
            let shifted = w.bin(Bo::ShiftRight, prim, by);
            let index = w.bin(Bo::And, shifted, nine_bits);
            let sign = w.bin(Bo::ShiftRight, prim, top);
            let culled = w.ne0(sign);
            store(&mut w, mesh.thread, index);
            store(&mut w, mesh.pos_set, no);
            w.push(call());
            // A primitive no thread made, a culled one, or a corner no thread placed: the same point
            // outside the clip volume, so the triangle has no area.
            let pointer = w.emit(E::GlobalVariable(mesh.pos_set));
            let placed = w.load(pointer);
            let (unmade, unplaced) = (w.lnot(made), w.lnot(placed));
            let gone = w.bin(Bo::LogicalOr, unmade, culled);
            let gone = w.bin(Bo::LogicalOr, gone, unplaced);
            if let Some(&position) = exports.get(&POS0) {
                let (two, one) = (w.f32(2.0), w.f32(1.0));
                let outside = w.emit(E::Compose {
                    ty: vec4,
                    components: vec![two, two, two, one],
                });
                w.open();
                store(&mut w, position, outside);
                let accept = w.close();
                w.push(Statement::If {
                    condition: gone,
                    accept,
                    reject: Block::new(),
                });
            }
        }
    }

    let mut members = Vec::new();
    let mut components = Vec::new();
    let mut offset = 0u32;
    for (&target, &global) in &exports {
        let (name, binding) = match target {
            POS0 => (
                "position".to_owned(),
                naga::Binding::BuiltIn(naga::BuiltIn::Position { invariant: false }),
            ),
            _ if fragment => (
                format!("mrt{target}"),
                naga::Binding::Location {
                    location: u32::from(target),
                    interpolation: None,
                    sampling: None,
                    blend_src: None,
                    per_primitive: false,
                },
            ),
            _ => (
                format!("param{}", target - PARAM0),
                location(u32::from(target - PARAM0), false),
            ),
        };
        let pointer = w.emit(E::GlobalVariable(global));
        let whole = w.load(pointer);
        let slot = target.wrapping_sub(PARAM0);
        if fragment || target < PARAM0 || !half_slots.contains(&slot) {
            members.push(naga::StructMember {
                name: Some(name),
                ty: vec4,
                binding: Some(binding),
                offset,
            });
            offset += 16;
            components.push(whole);
            continue;
        }
        // Each component holds two 16-bit floats, which the rasterizer has to interpolate apart.
        let mut sides = [Vec::with_capacity(4), Vec::with_capacity(4)];
        for c in 0..4 {
            let part = w.index(whole, c);
            let bits = w.bits(part);
            let pair = w.math(Mf::Unpack2x16float, &[bits]);
            for (side, parts) in sides.iter_mut().enumerate() {
                parts.push(w.index(pair, side as u32));
            }
        }
        for (side, parts) in sides.into_iter().enumerate() {
            let (half, base) = [("low", 0), ("high", HIGH_HALVES)][side];
            members.push(naga::StructMember {
                name: Some(format!("{name}_{half}")),
                ty: vec4,
                binding: Some(location(base + u32::from(slot), false)),
                offset,
            });
            offset += 16;
            components.push(w.emit(E::Compose {
                ty: vec4,
                components: parts,
            }));
        }
    }
    if let Some(global) = depth {
        let ty = insert(&mut module, TypeInner::Scalar(Scalar::F32));
        members.push(naga::StructMember {
            name: Some("depth".to_owned()),
            ty,
            binding: Some(naga::Binding::BuiltIn(naga::BuiltIn::FragDepth)),
            offset,
        });
        offset += 4;
        let pointer = w.emit(E::GlobalVariable(global));
        components.push(w.load(pointer));
    }
    let result = if members.is_empty() {
        w.push(Statement::Return { value: None });
        None
    } else {
        let ty = module.types.insert(
            naga::Type {
                name: Some("Output".into()),
                inner: TypeInner::Struct {
                    members,
                    span: offset.next_multiple_of(16),
                },
            },
            Span::UNDEFINED,
        );
        let value = w.emit(E::Compose { ty, components });
        w.push(Statement::Return { value: Some(value) });
        Some(naga::FunctionResult { ty, binding: None })
    };
    let mut function = w.finish();
    function.name = Some("main".into());
    function.arguments = arguments;
    function.result = result;
    module.entry_points.push(naga::EntryPoint {
        name: "main".into(),
        stage,
        early_depth_test: None,
        workgroup_size: [0; 3],
        workgroup_size_overrides: None,
        function,
        mesh_info: None,
        task_payload: None,
        incoming_ray_payload: None,
    });
    Ok(Lowered {
        module,
        resources,
        approximations: notes,
        half_slots: halves.keys().map(|(slot, _)| *slot).collect(),
    })
}
