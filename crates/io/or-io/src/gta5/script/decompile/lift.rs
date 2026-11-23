//! Lifting one basic block's stack code into statements over expressions.
//!
//! ⚠ **A value is a run of slots, not one.** A vector is three slots and a
//! call can return several, so the symbolic stack holds an expression with
//! its width, splits it into components when an instruction takes fewer
//! slots than it spans, and gathers single slots back into one value when an
//! instruction takes several at once.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

use super::ast::{BinOp, Expr, Kind, Stmt, Var};
use super::Function;
use crate::gta5::script::disasm::{Instruction, Operand};
use crate::gta5::script::natives::NativeDb;
use crate::gta5::script::opcode::{InstructionSet, Op};
use crate::gta5::script::ScrProgram;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Access {
    Addr,
    Load,
    Store,
}

/// What an instruction does to the stack, decided once per opcode from its mnemonic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Sem {
    Nop,
    Bin(BinOp, Kind),
    Not,
    Neg(Kind),
    Cast(&'static str, u32),
    PushInt(i64),
    PushFloat(f32),
    PushOperand,
    Dup,
    Drop,
    Native,
    Enter,
    Leave,
    Load,
    Store,
    StoreRev,
    LoadN,
    StoreN,
    Array(Access),
    Local(Access),
    Static(Access),
    Global(Access),
    Imm(BinOp),
    IOffset,
    Offset(Access),
    J,
    Jz,
    CmpJz(BinOp),
    Call,
    Switch,
    String,
    StringHash,
    TextLabel(&'static str),
    TextLabelCopy,
    Catch,
    Throw,
    CallIndirect,
    IsBitSet,
    Unknown,
}

fn sem_of(m: &'static str) -> Sem {
    use BinOp::*;
    let access = if m.ends_with("_LOAD") {
        Access::Load
    } else if m.ends_with("_STORE") {
        Access::Store
    } else {
        Access::Addr
    };
    let int = |op| Sem::Bin(op, Kind::Int);
    let float = |op| Sem::Bin(op, Kind::Float);
    let vector = |op| Sem::Bin(op, Kind::Vector);
    match m {
        "NOP" => Sem::Nop,
        "IADD" => int(Add),
        "ISUB" => int(Sub),
        "IMUL" => int(Mul),
        "IDIV" => int(Div),
        "IMOD" => int(Mod),
        "INOT" => Sem::Not,
        "INEG" => Sem::Neg(Kind::Int),
        "IEQ" => int(Eq),
        "INE" => int(Ne),
        "IGT" => int(Gt),
        "IGE" => int(Ge),
        "ILT" => int(Lt),
        "ILE" => int(Le),
        "FADD" => float(Add),
        "FSUB" => float(Sub),
        "FMUL" => float(Mul),
        "FDIV" => float(Div),
        "FMOD" => float(Mod),
        "FNEG" => Sem::Neg(Kind::Float),
        "FEQ" => float(Eq),
        "FNE" => float(Ne),
        "FGT" => float(Gt),
        "FGE" => float(Ge),
        "FLT" => float(Lt),
        "FLE" => float(Le),
        "VADD" => vector(Add),
        "VSUB" => vector(Sub),
        "VMUL" => vector(Mul),
        "VDIV" => vector(Div),
        "VNEG" => Sem::Neg(Kind::Vector),
        "IAND" => int(And),
        "IOR" => int(Or),
        "IXOR" => int(Xor),
        "I2F" => Sem::Cast("float", 1),
        "F2I" => Sem::Cast("int", 1),
        "F2V" => Sem::Cast("vector", 3),
        "DUP" => Sem::Dup,
        "DROP" => Sem::Drop,
        "NATIVE" => Sem::Native,
        "ENTER" => Sem::Enter,
        "LEAVE" => Sem::Leave,
        "LOAD" => Sem::Load,
        "STORE" => Sem::Store,
        "STORE_REV" => Sem::StoreRev,
        "LOAD_N" => Sem::LoadN,
        "STORE_N" => Sem::StoreN,
        "IADD_U8" | "IADD_S16" => Sem::Imm(Add),
        "IMUL_U8" | "IMUL_S16" => Sem::Imm(Mul),
        "IOFFSET" => Sem::IOffset,
        "J" => Sem::J,
        "JZ" => Sem::Jz,
        "IEQ_JZ" => Sem::CmpJz(Eq),
        "INE_JZ" => Sem::CmpJz(Ne),
        "IGT_JZ" => Sem::CmpJz(Gt),
        "IGE_JZ" => Sem::CmpJz(Ge),
        "ILT_JZ" => Sem::CmpJz(Lt),
        "ILE_JZ" => Sem::CmpJz(Le),
        "CALL" => Sem::Call,
        "SWITCH" => Sem::Switch,
        "STRING" => Sem::String,
        "STRINGHASH" => Sem::StringHash,
        "TEXT_LABEL_COPY" => Sem::TextLabelCopy,
        "CATCH" => Sem::Catch,
        "THROW" => Sem::Throw,
        "CALLINDIRECT" => Sem::CallIndirect,
        "IS_BIT_SET" => Sem::IsBitSet,
        "PUSH_CONST_F" => Sem::PushOperand,
        _ if m.starts_with("PUSH_CONST_F") => match &m["PUSH_CONST_F".len()..] {
            "M1" => Sem::PushFloat(-1.0),
            digit => digit
                .parse::<u8>()
                .map_or(Sem::Unknown, |d| Sem::PushFloat(d as f32)),
        },
        _ if m.starts_with("PUSH_CONST_") => match &m["PUSH_CONST_".len()..] {
            "M1" => Sem::PushInt(-1),
            rest => rest
                .parse::<u8>()
                .map_or(Sem::PushOperand, |d| Sem::PushInt(d as i64)),
        },
        _ if m.starts_with("ARRAY_") => Sem::Array(access),
        _ if m.starts_with("LOCAL_") => Sem::Local(access),
        _ if m.starts_with("STATIC_") => Sem::Static(access),
        _ if m.starts_with("GLOBAL_") => Sem::Global(access),
        _ if m.starts_with("IOFFSET_") => Sem::Offset(access),
        _ if m.starts_with("TEXT_LABEL_") => Sem::TextLabel(m),
        _ => Sem::Unknown,
    }
}

/// The stack behaviour of `op`, in the 131-opcode numbering.
pub(super) fn sem(op: Op) -> Sem {
    static TABLE: OnceLock<Vec<Sem>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        InstructionSet::U24Locals
            .table()
            .iter()
            .map(|info| sem_of(info.mnemonic))
            .collect()
    });
    table.get(op as usize).copied().unwrap_or(Sem::Unknown)
}

/// The integer an operand carries, whichever width it was read at.
fn imm(i: &Instruction) -> i64 {
    match i.operand {
        Operand::U8(v) => v as i64,
        Operand::U16(v) => v as i64,
        Operand::S16(v) => v as i64,
        Operand::U24(v) => v as i64,
        Operand::U32(v) => v as i32 as i64,
        _ => 0,
    }
}

/// How a block hands control on, with jump targets as code offsets.
#[derive(Clone, Debug)]
pub(super) enum Term {
    Fall,
    Jump(i64),
    /// To `target` when `cond` is false, else to the next block.
    Branch {
        cond: Expr,
        target: i64,
    },
    /// To the next block when no case matches.
    Switch {
        value: Expr,
        cases: Vec<(i64, i64)>,
    },
    Return(Option<Expr>),
}

pub(super) struct Lifted {
    pub stmts: Vec<Stmt>,
    pub term: Term,
    /// Slots left on the stack for the next block.
    pub exit_depth: usize,
    /// Pops past the whole frame, values left behind at a return, and counts
    /// that are not constants: each one a place the stack model failed.
    pub mismatches: usize,
    /// Pops past the bottom of the operand stack, read as the frame's last slots.
    pub below: usize,
    pub native_calls: usize,
    /// Native calls the list named.
    pub natives_named: usize,
}

/// What lifting a function's blocks needs to know about the whole script.
pub(super) struct FnCtx<'a> {
    pub program: &'a ScrProgram,
    /// Every instruction of the script; blocks index into it.
    pub instructions: &'a [Instruction],
    pub names: Option<&'a NativeDb>,
    pub functions: &'a [Function],
    pub params: u32,
    pub frame: u32,
    /// Indices of the `JZ`s a short-circuit condition folds away.
    pub folded: &'a HashSet<usize>,
    /// The `IAND` or `IOR` closing each short-circuit condition.
    pub logical: &'a HashMap<usize, BinOp>,
    /// `(parameters, return slots)` of the functions whose address the script
    /// pushes, commonest first: what an indirect call can reach.
    pub pointer_signatures: &'a [(u32, u32)],
}

struct Slot {
    e: Expr,
    w: u32,
}

struct Lifter<'a, 'b> {
    ctx: &'b FnCtx<'a>,
    stack: Vec<Slot>,
    stmts: Vec<Stmt>,
    mismatches: usize,
    below: usize,
    native_calls: usize,
    natives_named: usize,
    vars: &'b mut BTreeSet<Var>,
    /// The `(parameters, return slots)` decided for each `CALLINDIRECT`, by instruction index.
    indirect: &'b HashMap<usize, (u32, u32)>,
    watch: Watch,
}

/// One `CALLINDIRECT` followed through a lift: the first undecided one unless
/// an index is given.
#[derive(Clone, Copy, Default)]
struct Watch {
    index: Option<usize>,
    reached: bool,
    /// Slots on the stack under the function pointer.
    depth: u32,
    /// Pops from an empty stack after it.
    pops_after: u32,
}

/// Lift `instructions`, which start at index `first` of the script, from a stack `depth` slots deep.
pub(super) fn lift(
    ctx: &FnCtx<'_>,
    instructions: &[Instruction],
    first: usize,
    depth: usize,
    vars: &mut BTreeSet<Var>,
) -> Lifted {
    // ⚠ An indirect call states neither its argument nor its return count.
    // Each one takes, in turn, the signature of an address-taken function
    // under which the block lifts with the fewest failures, the later ones
    // lifted meanwhile under the commonest. When every such signature fails,
    // it takes as many arguments as the stack under it holds, at most, and as
    // many return slots as the rest of the block pops past the bottom.
    let fallback = [(0, 0)];
    let candidates = match ctx.pointer_signatures {
        [] => &fallback[..],
        sigs => sigs,
    };
    let mut decided = HashMap::new();
    let mut once = |decided: &HashMap<usize, (u32, u32)>, watch: Option<usize>| {
        let (lifted, w) = lift_once(ctx, instructions, first, depth, vars, decided, watch);
        (
            lifted.mismatches + lifted.below + lifted.exit_depth,
            lifted,
            w,
        )
    };
    loop {
        let (_, lifted, watch) = once(&decided, None);
        let Some(index) = watch.index.filter(|_| watch.reached) else {
            return lifted;
        };
        let mut best = (usize::MAX, candidates[0]);
        for &sig in candidates {
            decided.insert(index, sig);
            let (score, ..) = once(&decided, Some(index));
            if score < best.0 {
                best = (score, sig);
            }
        }
        for params in 0..=watch.depth.min(MAX_INFERRED_PARAMS) {
            if best.0 == 0 {
                break;
            }
            decided.insert(index, (params, 0));
            let (_, _, w) = once(&decided, Some(index));
            let sig = (params, w.pops_after);
            decided.insert(index, sig);
            let (score, ..) = once(&decided, Some(index));
            if score < best.0 {
                best = (score, sig);
            }
        }
        decided.insert(index, best.1);
    }
}

/// The most arguments an indirect call is tried with when no address-taken signature fits.
const MAX_INFERRED_PARAMS: u32 = 16;

fn lift_once(
    ctx: &FnCtx<'_>,
    instructions: &[Instruction],
    first: usize,
    depth: usize,
    vars: &mut BTreeSet<Var>,
    indirect: &HashMap<usize, (u32, u32)>,
    watch: Option<usize>,
) -> (Lifted, Watch) {
    let mut l = Lifter {
        ctx,
        stack: Vec::new(),
        stmts: Vec::new(),
        mismatches: 0,
        below: 0,
        native_calls: 0,
        natives_named: 0,
        vars,
        indirect,
        watch: Watch {
            index: watch,
            ..Watch::default()
        },
    };
    for d in 0..depth {
        let temp = l.var(Var::Temp(d as u32));
        l.push(temp, 1);
    }
    let mut term = Term::Fall;
    for (k, i) in instructions.iter().enumerate() {
        if let Some(t) = l.step(i, first + k) {
            term = t;
            break;
        }
    }
    if matches!(term, Term::Return(_)) {
        l.mismatches += usize::from(!l.stack.is_empty());
        l.stack.clear();
    }
    let exit_depth = l.spill();
    let lifted = Lifted {
        stmts: l.stmts,
        term,
        exit_depth,
        mismatches: l.mismatches,
        below: l.below,
        native_calls: l.native_calls,
        natives_named: l.natives_named,
    };
    (lifted, l.watch)
}

impl Lifter<'_, '_> {
    fn var(&mut self, v: Var) -> Expr {
        if matches!(v, Var::Local(_) | Var::Temp(_)) {
            self.vars.insert(v);
        }
        Expr::Var(v)
    }

    fn local(&mut self, slot: i64) -> Expr {
        let slot = slot as u32;
        let params = self.ctx.params;
        let v = if slot < params {
            Var::Param(slot)
        } else if slot < params + 2 {
            Var::Frame(slot - params)
        } else {
            Var::Local(slot - params - 2)
        };
        self.var(v)
    }

    fn push(&mut self, e: Expr, w: u32) {
        match e {
            Expr::Tuple(items) if items.len() == w as usize => {
                for item in items {
                    self.stack.push(Slot { e: item, w: 1 });
                }
            }
            e if w > 0 => self.stack.push(Slot { e, w }),
            _ => {}
        }
    }

    /// What a pop from an empty operand stack reads.
    ///
    /// ⚠ The operand stack starts right after the frame, so a pop past its
    /// bottom reads the frame's last slot, then the one before. Retail scripts
    /// do it: a native given one argument fewer than it takes reads a local.
    fn missing(&mut self) -> Expr {
        if self.watch.reached {
            self.watch.pops_after += 1;
        }
        let slot = self.ctx.frame.checked_sub(1 + self.below as u32);
        self.below += 1;
        match slot {
            Some(slot) => self.local(slot as i64),
            None => {
                self.mismatches += 1;
                Expr::Missing
            }
        }
    }

    /// One slot, splitting a wider value into its components.
    fn pop_slot(&mut self) -> Expr {
        loop {
            match self.stack.pop() {
                None => return self.missing(),
                Some(Slot { e, w: 1 }) => return e,
                Some(Slot { e, w }) => {
                    for i in 0..w {
                        let c = Expr::Component {
                            e: Box::new(e.clone()),
                            i,
                            of: w,
                        };
                        self.stack.push(Slot { e: c, w: 1 });
                    }
                }
            }
        }
    }

    /// `n` slots taken as one value.
    fn pop_value(&mut self, n: u32) -> Expr {
        if self.stack.last().is_some_and(|s| s.w == n) {
            return self.stack.pop().map_or(Expr::Missing, |s| s.e);
        }
        if n == 1 {
            return self.pop_slot();
        }
        let mut items: Vec<Expr> = (0..n).map(|_| self.pop_slot()).collect();
        items.reverse();
        let whole = match items.first() {
            Some(Expr::Component { e, i: 0, of }) if *of == n => Some(e.clone()),
            _ => None,
        };
        if let Some(whole) = whole {
            let gathered = items.iter().enumerate().all(|(k, item)| {
                matches!(item, Expr::Component { e, i, .. } if *i == k as u32 && **e == *whole)
            });
            if gathered {
                return *whole;
            }
        }
        Expr::Tuple(items)
    }

    /// `n` slots taken as arguments, keeping each whole value whole.
    fn pop_args(&mut self, n: u32) -> Vec<Expr> {
        let mut args = Vec::new();
        let mut left = n;
        while left > 0 {
            match self.stack.last() {
                Some(s) if s.w <= left => {
                    left -= s.w;
                    args.extend(self.stack.pop().map(|s| s.e));
                }
                _ => {
                    left -= 1;
                    let arg = self.pop_slot();
                    args.push(arg);
                }
            }
        }
        args.reverse();
        args
    }

    fn assign(&mut self, lhs: Expr, rhs: Expr) {
        self.stmts.push(Stmt::Assign(lhs, rhs));
    }

    /// A call's result: a statement when it returns nothing, else a value.
    fn result(&mut self, call: Expr, returns: u32) {
        if returns == 0 {
            self.stmts.push(Stmt::Expr(call));
        } else {
            self.push(call, returns);
        }
    }

    fn variable(&mut self, access: Access, v: Expr) {
        match access {
            Access::Addr => self.push(Expr::addr_of(v), 1),
            Access::Load => self.push(v, 1),
            Access::Store => {
                let value = self.pop_slot();
                self.assign(v, value);
            }
        }
    }

    /// A constant count off the stack, or `None` when it is not one.
    fn count(&mut self) -> Option<u32> {
        match self.pop_slot() {
            Expr::Int(n) if (0..=u16::MAX as i64).contains(&n) => Some(n as u32),
            _ => {
                self.mismatches += 1;
                None
            }
        }
    }

    fn step(&mut self, i: &Instruction, index: usize) -> Option<Term> {
        match sem(i.op) {
            Sem::Nop | Sem::Enter => {}
            Sem::Bin(op, Kind::Vector) => {
                let r = self.pop_value(3);
                let l = self.pop_value(3);
                self.push(Expr::bin(op, Kind::Vector, l, r), 3);
            }
            Sem::Bin(op, kind) => {
                let r = self.pop_slot();
                let l = self.pop_slot();
                let op = self.ctx.logical.get(&index).copied().unwrap_or(op);
                self.push(Expr::bin(op, kind, l, r), 1);
            }
            Sem::Not => {
                let e = self.pop_slot();
                self.push(e.negate(), 1);
            }
            Sem::Neg(Kind::Vector) => {
                let e = self.pop_value(3);
                self.push(Expr::Neg(Box::new(e)), 3);
            }
            Sem::Neg(_) => {
                let e = self.pop_slot();
                self.push(Expr::Neg(Box::new(e)), 1);
            }
            Sem::Cast(to, w) => {
                let e = self.pop_slot();
                self.push(Expr::Cast { to, e: Box::new(e) }, w);
            }
            Sem::PushInt(v) => self.push(Expr::Int(v), 1),
            Sem::PushFloat(v) => self.push(Expr::Float(v), 1),
            Sem::PushOperand => match i.operand {
                Operand::U8U8(a, b) => {
                    self.push(Expr::Int(a as i64), 1);
                    self.push(Expr::Int(b as i64), 1);
                }
                Operand::U8U8U8(a, b, c) => {
                    self.push(Expr::Int(a as i64), 1);
                    self.push(Expr::Int(b as i64), 1);
                    self.push(Expr::Int(c as i64), 1);
                }
                Operand::F32(v) => self.push(Expr::Float(v), 1),
                _ => self.push(Expr::Int(imm(i)), 1),
            },
            Sem::Dup => {
                let e = self.pop_slot();
                self.push(e.clone(), 1);
                self.push(e, 1);
            }
            Sem::Drop => match self.pop_slot() {
                Expr::Component { e, i: 0, .. } if e.has_call() => self.stmts.push(Stmt::Expr(*e)),
                Expr::Component { .. } => {}
                e if e.has_call() => self.stmts.push(Stmt::Expr(e)),
                _ => {}
            },
            Sem::Native => {
                if let Operand::Native {
                    args,
                    returns,
                    slot,
                } = i.operand
                {
                    let args = self.pop_args(args as u32);
                    let call = self.native_call(slot, args);
                    self.result(call, returns as u32);
                }
            }
            Sem::Leave => {
                let returns = match i.operand {
                    Operand::Leave { returns, .. } => returns as u32,
                    _ => 0,
                };
                let value = (returns > 0).then(|| self.pop_value(returns));
                return Some(Term::Return(value));
            }
            Sem::Load => {
                let p = self.pop_slot();
                self.push(Expr::deref(p), 1);
            }
            Sem::Store => {
                let p = self.pop_slot();
                let v = self.pop_slot();
                self.assign(Expr::deref(p), v);
            }
            Sem::StoreRev => {
                let v = self.pop_slot();
                let p = self.pop_slot();
                self.push(p.clone(), 1);
                self.assign(Expr::deref(p), v);
            }
            Sem::LoadN => {
                let p = self.pop_slot();
                if let Some(n) = self.count() {
                    self.push(Expr::deref(p), n);
                }
            }
            Sem::StoreN => {
                let p = self.pop_slot();
                if let Some(n) = self.count() {
                    let v = self.pop_value(n);
                    self.assign(Expr::deref(p), v);
                }
            }
            Sem::Array(access) => {
                let a = self.pop_slot();
                let index_e = self.pop_slot();
                let element = Expr::Index {
                    base: Box::new(Expr::deref(a)),
                    index: Box::new(index_e),
                    size: imm(i) as u32,
                };
                self.variable(access, element);
            }
            Sem::Local(access) => {
                let v = self.local(imm(i));
                self.variable(access, v);
            }
            Sem::Static(access) => {
                let v = self.var(Var::Static(imm(i) as u32));
                self.variable(access, v);
            }
            Sem::Global(access) => {
                let v = self.var(Var::Global(imm(i) as u32));
                self.variable(access, v);
            }
            Sem::Imm(op) => {
                let e = self.pop_slot();
                self.push(Expr::bin(op, Kind::Int, e, Expr::Int(imm(i))), 1);
            }
            Sem::IOffset => {
                let offset = self.pop_slot();
                let a = self.pop_slot();
                let field = Expr::DynField {
                    base: Box::new(Expr::deref(a)),
                    offset: Box::new(offset),
                };
                self.push(Expr::addr_of(field), 1);
            }
            Sem::Offset(access) => {
                let a = self.pop_slot();
                let field = Expr::Field {
                    base: Box::new(Expr::deref(a)),
                    offset: imm(i),
                };
                self.variable(access, field);
            }
            Sem::J if falls_through(self.ctx.instructions, index) => {}
            Sem::J => return Some(Term::Jump(jump_target(i))),
            Sem::Jz => {
                let cond = self.pop_slot();
                if !self.ctx.folded.contains(&index) {
                    return Some(Term::Branch {
                        cond,
                        target: jump_target(i),
                    });
                }
            }
            Sem::CmpJz(op) => {
                let r = self.pop_slot();
                let l = self.pop_slot();
                return Some(Term::Branch {
                    cond: Expr::bin(op, Kind::Int, l, r),
                    target: jump_target(i),
                });
            }
            Sem::Call => {
                let target = match i.operand {
                    Operand::Call { target } => target,
                    _ => 0,
                };
                match super::callee(self.ctx.functions, target) {
                    Some(f) => {
                        let (name, params, returns) = (f.name.clone(), f.params, f.returns);
                        let args = self.pop_args(params);
                        self.result(Expr::call(name, args), returns);
                    }
                    None => {
                        self.mismatches += 1;
                        self.stmts
                            .push(Stmt::Expr(Expr::call(format!("sub_{target:06X}"), vec![])));
                    }
                }
            }
            Sem::Switch => {
                let value = self.pop_slot();
                let cases = match &i.operand {
                    Operand::Switch(cases) => cases
                        .iter()
                        .map(|c| (c.value as i32 as i64, c.target))
                        .collect(),
                    _ => Vec::new(),
                };
                return Some(Term::Switch { value, cases });
            }
            Sem::String => {
                let e = match self.pop_slot() {
                    Expr::Int(at) => match usize::try_from(at)
                        .ok()
                        .and_then(|at| self.ctx.program.string_at(at))
                    {
                        Some(text) => Expr::Str(String::from_utf8_lossy(text).into_owned()),
                        None => Expr::call("STRING", vec![Expr::Int(at)]),
                    },
                    other => Expr::call("STRING", vec![other]),
                };
                self.push(e, 1);
            }
            Sem::StringHash => {
                let e = self.pop_slot();
                self.push(Expr::call("STRINGHASH", vec![e]), 1);
            }
            Sem::TextLabel(name) => {
                let dest = self.pop_slot();
                let source = self.pop_slot();
                self.stmts.push(Stmt::Expr(Expr::call(
                    name,
                    vec![dest, source, Expr::Int(imm(i))],
                )));
            }
            Sem::TextLabelCopy => {
                let dest = self.pop_slot();
                let size = self.pop_slot();
                if let Some(n) = self.count() {
                    let source = self.pop_value(n);
                    self.stmts.push(Stmt::Expr(Expr::call(
                        "TEXT_LABEL_COPY",
                        vec![dest, size, source],
                    )));
                }
            }
            Sem::Catch => self.push(Expr::call("CATCH", vec![]), 1),
            Sem::Throw => {
                let e = self.pop_slot();
                self.stmts.push(Stmt::Expr(Expr::call("THROW", vec![e])));
            }
            Sem::CallIndirect => {
                let f = self.pop_slot();
                let decided = self.indirect.get(&index).copied();
                if self.watch.index.is_none() && decided.is_none() {
                    self.watch.index = Some(index);
                }
                if self.watch.index == Some(index) {
                    self.watch.reached = true;
                    self.watch.depth = self.stack.iter().map(|s| s.w).sum();
                }
                let commonest = self.ctx.pointer_signatures.first().copied();
                let (params, returns) = decided.or(commonest).unwrap_or((0, 0));
                let args = self.pop_args(params);
                let name = format!("(*{})", f.text());
                self.result(Expr::call(name, args), returns);
            }
            Sem::IsBitSet => {
                let r = self.pop_slot();
                let l = self.pop_slot();
                self.push(Expr::call("IS_BIT_SET", vec![l, r]), 1);
            }
            Sem::Unknown => self.mismatches += 1,
        }
        None
    }

    /// A native call, named, its literal arguments spelled and its result typed as the list states.
    ///
    /// ⚠ Arguments are typed only when there is one per parameter: a vector
    /// handed to three `float` parameters arrives as one argument.
    fn native_call(&mut self, slot: u16, args: Vec<Expr>) -> Expr {
        self.native_calls += 1;
        let hash = self.ctx.program.native_hash(slot as usize);
        let Some(native) = hash.and_then(|h| self.ctx.names.and_then(|db| db.name(h))) else {
            let name = match hash {
                None => format!("NATIVE_SLOT_{slot}"),
                Some(hash) => format!("_0x{hash:016X}"),
            };
            return Expr::call(name, args);
        };
        self.natives_named += 1;
        let args = if args.len() == native.params.len() {
            args.into_iter()
                .zip(&native.params)
                .map(|(arg, param)| typed_literal(arg, &param.ty))
                .collect()
        } else {
            args
        };
        Expr::Call {
            name: native.to_string(),
            args,
            returns: native.returns.clone().filter(|t| t != "void"),
        }
    }

    /// Hand what is left on the stack to the next block through `stackN` temporaries.
    fn spill(&mut self) -> usize {
        let mut slots = Vec::new();
        while !self.stack.is_empty() {
            slots.push(self.pop_slot());
        }
        slots.reverse();
        for (d, e) in slots.iter().enumerate() {
            let temp = self.var(Var::Temp(d as u32));
            if *e != temp {
                self.assign(temp, e.clone());
            }
        }
        slots.len()
    }
}

/// An integer constant as a parameter of type `ty` spells it.
fn typed_literal(arg: Expr, ty: &str) -> Expr {
    match (arg, ty) {
        (Expr::Int(0), "BOOL") => Expr::Literal("false".into()),
        (Expr::Int(1), "BOOL") => Expr::Literal("true".into()),
        (Expr::Int(v), "Hash") => Expr::Literal(format!("0x{:08X}", v as u32)),
        (arg, _) => arg,
    }
}

/// Whether instruction `index` is a `J` that reaches where falling through would, past `NOP`s only.
///
/// ⚠ The compiler lays one down where a block meets a code page boundary, with
/// the stack still holding values, so splitting the block there would hand a
/// constant count to the next block as a temporary.
pub(super) fn falls_through(instructions: &[Instruction], index: usize) -> bool {
    let i = &instructions[index];
    if sem(i.op) != Sem::J {
        return false;
    }
    let target = jump_target(i);
    instructions[index + 1..]
        .iter()
        .find(|next| next.at as i64 >= target || sem(next.op) != Sem::Nop)
        .is_some_and(|next| next.at as i64 == target)
}

fn jump_target(i: &Instruction) -> i64 {
    match i.operand {
        Operand::Jump { target } => target,
        _ => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every opcode in the wider set has a stack behaviour, so none is lifted as nothing.
    #[test]
    fn every_opcode_has_a_stack_behaviour() {
        for info in InstructionSet::U24Locals.table() {
            assert_ne!(sem(info.op), Sem::Unknown, "{}", info.mnemonic);
        }
        assert_eq!(
            sem(crate::gta5::script::opcode::PUSH_CONST_M1),
            Sem::PushInt(-1)
        );
        assert_eq!(
            sem(crate::gta5::script::opcode::PUSH_CONST_7),
            Sem::PushInt(7)
        );
        assert_eq!(
            sem(crate::gta5::script::opcode::PUSH_CONST_U8),
            Sem::PushOperand
        );
    }
}
