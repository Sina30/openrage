//! Decompiling a script's bytecode to C-like source.
//!
//! Each function is cut into basic blocks, each block's stack code is lifted
//! into statements, and the blocks are structured into if/else, loops and
//! switches, with a labelled goto only where no construct explains a jump.
//!
//! ⚠ **A name is a slot, not a meaning.** Parameters, locals, statics and
//! globals are named by slot index, so a name is stable from run to run and
//! says nothing the bytecode does not.

mod ast;
mod lift;
mod structure;

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::Write;
use std::ops::Range;

use ast::{BinOp, Kind, Printer, Stmt, Var};
use lift::{FnCtx, Sem, Term};
use structure::{Block, Exit, Structurer, Target};

use super::disasm::{self, DecodeError, Instruction, Operand};
use super::natives::NativeDb;
use super::{ScrProgram, CODE_PAGE_SIZE};

/// A script's decompiled source, and what producing it found.
#[derive(Clone, Debug)]
pub struct Decompiled {
    pub text: String,
    pub stats: DecompileStats,
}

/// What decompiling one script found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DecompileStats {
    pub functions: usize,
    /// Functions in which structuring fell back to at least one `goto`.
    pub functions_with_goto: usize,
    pub gotos: usize,
    pub instructions: usize,
    /// Instructions in blocks the source emits, each counted once.
    pub instructions_consumed: usize,
    /// Instructions emitted again after their first emission.
    pub instructions_repeated: usize,
    /// Places the stack model failed: a pop past the whole frame, a count that
    /// is not a constant, a value left behind at a return, or two
    /// predecessors handing one block different depths.
    pub stack_mismatches: usize,
    /// Pops past the bottom of the operand stack, which read the frame's last slots.
    pub reads_below_stack: usize,
    pub native_calls: usize,
    /// Native calls a natives list named.
    pub natives_named: usize,
}

impl DecompileStats {
    pub fn add(&mut self, other: &DecompileStats) {
        self.functions += other.functions;
        self.functions_with_goto += other.functions_with_goto;
        self.gotos += other.gotos;
        self.instructions += other.instructions;
        self.instructions_consumed += other.instructions_consumed;
        self.instructions_repeated += other.instructions_repeated;
        self.stack_mismatches += other.stack_mismatches;
        self.reads_below_stack += other.reads_below_stack;
        self.native_calls += other.native_calls;
        self.natives_named += other.natives_named;
    }
}

/// A function: an `ENTER` and the instructions up to the next one.
struct Function {
    at: u32,
    name: String,
    params: u32,
    /// Slots the first `LEAVE` returns; zero when the function never returns.
    returns: u32,
    frame: u32,
    range: Range<usize>,
}

/// The function a call to `target` reaches.
///
/// ⚠ A function the compiler pushed onto the next code page keeps the address
/// its callers were given, which is the padding in front of it.
fn callee(functions: &[Function], target: u32) -> Option<&Function> {
    let f = functions.get(functions.partition_point(|f| f.at < target))?;
    let through_padding = (f.at as usize).is_multiple_of(CODE_PAGE_SIZE)
        && ((f.at - target) as usize) < CODE_PAGE_SIZE;
    (f.at == target || through_padding).then_some(f)
}

/// Decode `program` and decompile it, naming natives from `names` where it can.
pub fn decompile(
    program: &ScrProgram,
    names: Option<&NativeDb>,
) -> Result<Decompiled, DecodeError> {
    let (instructions, _) = disasm::check(program)?;
    Ok(decompile_decoded(program, &instructions, names))
}

/// [`decompile`] over instructions already decoded from `program`.
pub fn decompile_decoded(
    program: &ScrProgram,
    instructions: &[Instruction],
    names: Option<&NativeDb>,
) -> Decompiled {
    let functions = split(instructions);
    let mut index_at = vec![u32::MAX; program.code.len() + 1];
    for (n, i) in instructions.iter().enumerate() {
        if let Some(slot) = index_at.get_mut(i.at as usize) {
            *slot = n as u32;
        }
    }
    let index_of = |target: i64| -> Option<usize> {
        let n = *index_at.get(usize::try_from(target).ok()?)?;
        (n != u32::MAX).then_some(n as usize)
    };

    let mut stats = DecompileStats {
        functions: functions.len(),
        instructions: instructions.len(),
        ..DecompileStats::default()
    };
    let pointer_signatures = pointer_signatures(instructions, &functions, index_of);
    let mut text = String::new();
    write_statics(&mut text, program);
    for f in &functions {
        let script = Script {
            program,
            instructions,
            functions: &functions,
            names,
            index_of: &index_of,
            pointer_signatures: &pointer_signatures,
        };
        let (source, found) = script.function(f);
        text.push('\n');
        text.push_str(&source);
        stats.add(&found);
    }
    Decompiled { text, stats }
}

fn split(instructions: &[Instruction]) -> Vec<Function> {
    let mut starts: Vec<usize> = instructions
        .iter()
        .enumerate()
        .filter(|(_, i)| i.op == super::opcode::ENTER)
        .map(|(n, _)| n)
        .collect();
    if starts.first() != Some(&0) && !instructions.is_empty() {
        starts.insert(0, 0);
    }
    let mut ordinal = 0;
    let mut out = Vec::new();
    for (k, &s) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(instructions.len());
        let returns = instructions[s..end]
            .iter()
            .find_map(|i| match i.operand {
                Operand::Leave { returns, .. } => Some(returns as u32),
                _ => None,
            })
            .unwrap_or(0);
        let (params, frame, name) = match &instructions[s].operand {
            Operand::Enter {
                params,
                frame,
                name,
            } => {
                let name = if name.is_empty() {
                    format!("func_{ordinal}")
                } else {
                    String::from_utf8_lossy(name).into_owned()
                };
                ordinal += 1;
                (*params as u32, *frame as u32, name)
            }
            _ => (0, 0, "__preamble".to_owned()),
        };
        out.push(Function {
            at: instructions[s].at,
            name,
            params,
            returns,
            frame,
            range: s..end,
        });
    }
    out
}

/// `(parameters, return slots)` of each function whose address a `PUSH_CONST_U24`
/// pushes, commonest first.
///
/// ⚠ An address may be the page padding in front of the function, as a call's
/// may, so one that lands on a `NOP` or a `J` counts through the padding.
fn pointer_signatures(
    instructions: &[Instruction],
    functions: &[Function],
    index_of: impl Fn(i64) -> Option<usize>,
) -> Vec<(u32, u32)> {
    let mut counts: HashMap<(u32, u32), usize> = HashMap::new();
    for i in instructions {
        if let (super::opcode::PUSH_CONST_U24, Operand::U24(at)) = (i.op, &i.operand) {
            let pads = index_of(*at as i64)
                .is_some_and(|n| matches!(lift::sem(instructions[n].op), Sem::Nop | Sem::J));
            let found = callee(functions, *at).filter(|f| f.at == *at || pads);
            if let Some(f) = found {
                *counts.entry((f.params, f.returns)).or_default() += 1;
            }
        }
    }
    let mut sigs: Vec<((u32, u32), usize)> = counts.into_iter().collect();
    sigs.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    sigs.into_iter().map(|(sig, _)| sig).collect()
}

fn write_statics(out: &mut String, program: &ScrProgram) {
    let _ = writeln!(
        out,
        "// {} static(s), the first {} of them the script's arguments; one not declared here starts at 0",
        program.statics.len(),
        program.parameter_count
    );
    for (n, value) in program.statics.iter().enumerate() {
        let argument = (n as u32) < program.parameter_count;
        if *value == 0 && !argument {
            continue;
        }
        let shown = match u32::try_from(*value) {
            Ok(v) => (v as i32).to_string(),
            Err(_) => format!("0x{value:X}"),
        };
        let note = if argument { " // argument" } else { "" };
        let _ = writeln!(out, "var static_{n} = {shown};{note}");
    }
}

/// Whether `stmts` ends in a `return;` that the closing brace already says.
fn trailing_return(stmts: &[Stmt]) -> Option<usize> {
    let (i, last) = stmts
        .iter()
        .enumerate()
        .rev()
        .find(|(_, s)| !matches!(s, Stmt::Label(_)))?;
    (*last == Stmt::Return(None)).then_some(i)
}

/// The type each variable takes from the natives whose results it stores, `None` where two disagree.
///
/// ⚠ An integer constant stored as well does not untype it, since a handle
/// is reset to 0 or -1; anything else stored does. A vector result spans
/// three slots and types nothing.
fn declared_types(blocks: &[Block]) -> HashMap<Var, Option<&str>> {
    let mut types: HashMap<Var, Option<&str>> = HashMap::new();
    for stmt in blocks.iter().flat_map(|b| &b.stmts) {
        let Stmt::Assign(ast::Expr::Var(v), value) = stmt else {
            continue;
        };
        let ty = match value {
            ast::Expr::Int(_) => continue,
            ast::Expr::Call {
                returns: Some(ty), ..
            } if !matches!(ty.as_str(), "Any" | "Vector3") => Some(ty.as_str()),
            _ => None,
        };
        let entry = types.entry(*v).or_insert(ty);
        if *entry != ty {
            *entry = None;
        }
    }
    types
}

struct Script<'a, F: Fn(i64) -> Option<usize>> {
    program: &'a ScrProgram,
    instructions: &'a [Instruction],
    functions: &'a [Function],
    names: Option<&'a NativeDb>,
    index_of: &'a F,
    pointer_signatures: &'a [(u32, u32)],
}

impl<F: Fn(i64) -> Option<usize>> Script<'_, F> {
    fn function(&self, f: &Function) -> (String, DecompileStats) {
        let range = f.range.clone();
        let (folded, logical) = self.short_circuits(range.clone());

        let mut leaders = BTreeSet::from([range.start]);
        for idx in range.clone() {
            let i = &self.instructions[idx];
            let ends = match lift::sem(i.op) {
                Sem::Switch | Sem::Leave | Sem::CmpJz(_) => true,
                Sem::J => !lift::falls_through(self.instructions, idx),
                Sem::Jz => !folded.contains(&idx),
                _ => false,
            };
            if !ends {
                continue;
            }
            if idx + 1 < range.end {
                leaders.insert(idx + 1);
            }
            for t in i.targets() {
                if let Some(k) = (self.index_of)(t).filter(|k| range.contains(k)) {
                    leaders.insert(k);
                }
            }
        }
        let starts: Vec<usize> = leaders.into_iter().collect();
        let n = starts.len();
        let bounds = |b: usize| starts[b]..starts.get(b + 1).copied().unwrap_or(range.end);
        let target = |t: i64| match (self.index_of)(t).filter(|k| range.contains(k)) {
            Some(k) => match starts.binary_search(&k) {
                Ok(b) => Target::Block(b),
                Err(_) => Target::Outside(t),
            },
            None => Target::Outside(t),
        };
        let exit_of = |term: Term| match term {
            Term::Fall => Exit::Fall,
            Term::Jump(t) => Exit::Jump(target(t)),
            Term::Branch { cond, target: t } => Exit::Branch {
                cond,
                target: target(t),
            },
            Term::Switch { value, cases } => Exit::Switch {
                value,
                cases: cases.into_iter().map(|(v, t)| (v, target(t))).collect(),
            },
            Term::Return(value) => Exit::Return(value),
        };

        let ctx = FnCtx {
            program: self.program,
            instructions: self.instructions,
            names: self.names,
            functions: self.functions,
            params: f.params,
            frame: f.frame,
            folded: &folded,
            logical: &logical,
            pointer_signatures: self.pointer_signatures,
        };
        let mut vars = BTreeSet::new();
        let mut stats = DecompileStats::default();
        let mut lift_block = |b: usize, depth: usize, vars: &mut BTreeSet<Var>| {
            let r = bounds(b);
            let lifted = lift::lift(&ctx, &self.instructions[r.clone()], r.start, depth, vars);
            stats.stack_mismatches += lifted.mismatches;
            stats.reads_below_stack += lifted.below;
            stats.native_calls += lifted.native_calls;
            stats.natives_named += lifted.natives_named;
            lifted
        };

        // Breadth first from the entry, so each block starts at the depth its
        // first predecessor hands it.
        let mut lifted: Vec<Option<(Vec<Stmt>, Exit)>> = (0..n).map(|_| None).collect();
        let mut depth: Vec<Option<usize>> = vec![None; n];
        let mut mismatched_joins = 0;
        depth[0] = Some(0);
        let mut queue = VecDeque::from([0]);
        while let Some(b) = queue.pop_front() {
            if lifted[b].is_some() {
                continue;
            }
            let l = lift_block(b, depth[b].unwrap_or(0), &mut vars);
            let exit = exit_of(l.term);
            for s in exit.successors(b, n) {
                match depth[s] {
                    None => {
                        depth[s] = Some(l.exit_depth);
                        queue.push_back(s);
                    }
                    Some(d) if d != l.exit_depth => mismatched_joins += 1,
                    Some(_) => {}
                }
            }
            lifted[b] = Some((l.stmts, exit));
        }

        let mut blocks = Vec::with_capacity(n);
        for (b, slot) in lifted.into_iter().enumerate() {
            let r = bounds(b);
            let reachable = depth[b].is_some();
            let padding = !reachable
                && self.instructions[r.clone()]
                    .iter()
                    .all(|i| matches!(lift::sem(i.op), Sem::Nop | Sem::J));
            let (stmts, exit) = match slot {
                Some(done) => done,
                None if padding => (Vec::new(), Exit::Fall),
                None => {
                    let l = lift_block(b, 0, &mut vars);
                    (l.stmts, exit_of(l.term))
                }
            };
            blocks.push(Block {
                at: self.instructions[r.start].at,
                stmts,
                exit,
                instructions: r.len(),
                padding,
            });
        }
        stats.stack_mismatches += mismatched_joins;

        let mut s = Structurer::new(&blocks);
        let body = s.run();
        for (block, count) in blocks.iter().zip(&s.emitted) {
            if *count > 0 {
                stats.instructions_consumed += block.instructions;
                stats.instructions_repeated += block.instructions * (*count as usize - 1);
            }
        }
        stats.gotos = s.gotos;
        stats.functions_with_goto = usize::from(s.gotos > 0);

        let mut out = String::new();
        let _ = writeln!(
            out,
            "// {} at 0x{:06X}: {} parameter(s), {} return slot(s), frame {}",
            f.name, f.at, f.params, f.returns, f.frame
        );
        if stats.stack_mismatches > 0 {
            let _ = writeln!(
                out,
                "// ⚠ the stack model failed {} time(s) in this function",
                stats.stack_mismatches
            );
        }
        if stats.reads_below_stack > 0 {
            let _ = writeln!(
                out,
                "// ⚠ reads {} slot(s) below its operand stack, which are its frame's last slots",
                stats.reads_below_stack
            );
        }
        let ret = match f.returns {
            0 => "void".to_owned(),
            1 => "var".to_owned(),
            r => format!("var[{r}]"),
        };
        let params: Vec<String> = (0..f.params).map(|p| format!("var param{p}")).collect();
        let _ = writeln!(out, "{ret} {}({})\n{{", f.name, params.join(", "));
        let types = declared_types(&blocks);
        let mut declared: Vec<(&str, Vec<String>)> = vec![("var", Vec::new())];
        for v in vars
            .iter()
            .filter(|v| matches!(v, Var::Local(_) | Var::Temp(_)))
        {
            let ty = types.get(v).copied().flatten().unwrap_or("var");
            match declared.iter_mut().find(|(t, _)| *t == ty) {
                Some((_, names)) => names.push(v.to_string()),
                None => declared.push((ty, vec![v.to_string()])),
            }
        }
        declared.retain(|(_, names)| !names.is_empty());
        for (ty, names) in &declared {
            let _ = writeln!(out, "    {ty} {};", names.join(", "));
        }
        if !declared.is_empty() {
            out.push('\n');
        }
        let labels = s.used_labels();
        let mut p = Printer {
            out,
            labels: &labels,
        };
        match trailing_return(&body) {
            Some(i) => {
                p.block(&body[..i], 1);
                p.block(&body[i + 1..], 1);
            }
            None => p.block(&body, 1),
        }
        let mut out = p.out;
        out.push_str("}\n");
        (out, stats)
    }

    /// The `JZ`s of `a && b` and `a || b`, and the `IAND` or `IOR` closing each.
    ///
    /// ⚠ **`a && b` is `a DUP JZ L b IAND L:`, and `a || b` the same with an
    /// `INOT` before the `JZ`.** Read as plain stack code with the jump left
    /// out, the run between `JZ` and `L` leaves `a` and `b` for the closing
    /// instruction, so the condition lifts as one expression. That holds only
    /// when nothing else jumps into or out of the run, which is checked until
    /// no candidate changes.
    fn short_circuits(&self, range: Range<usize>) -> (HashSet<usize>, HashMap<usize, BinOp>) {
        let ins = self.instructions;
        let sem = |idx: usize| lift::sem(ins[idx].op);
        // Page padding can sit anywhere in the pattern, so it is read past `NOP`s.
        let before = |idx: usize| (range.start..idx).rev().find(|n| sem(*n) != Sem::Nop);
        // `(JZ, L, operator, the IAND or IOR)`.
        let mut cands: Vec<(usize, usize, BinOp, usize)> = Vec::new();
        for j in range.clone() {
            if sem(j) != Sem::Jz {
                continue;
            }
            let p1 = before(j);
            let p2 = p1.and_then(before);
            let op = match (p1.map(sem), p2.map(sem)) {
                (Some(Sem::Not), Some(Sem::Dup)) => BinOp::LogicalOr,
                (Some(Sem::Dup), _) => BinOp::LogicalAnd,
                _ => continue,
            };
            let Some(k) = ins[j]
                .targets()
                .first()
                .and_then(|t| (self.index_of)(*t))
                .filter(|k| *k > j + 1 && *k < range.end)
            else {
                continue;
            };
            let closes = match op {
                BinOp::LogicalOr => Sem::Bin(BinOp::Or, Kind::Int),
                _ => Sem::Bin(BinOp::And, Kind::Int),
            };
            if let Some(close) = before(k).filter(|c| *c > j && sem(*c) == closes) {
                cands.push((j, k, op, close));
            }
        }

        loop {
            let jzs: HashSet<usize> = cands.iter().map(|c| c.0).collect();
            let mut control = Vec::new();
            let mut targets = Vec::new();
            for idx in range.clone() {
                let is_control = match sem(idx) {
                    Sem::Switch | Sem::Leave | Sem::CmpJz(_) => true,
                    Sem::J => !lift::falls_through(ins, idx),
                    Sem::Jz => !jzs.contains(&idx),
                    _ => false,
                };
                if is_control {
                    control.push(idx);
                    targets.extend(ins[idx].targets().into_iter().filter_map(self.index_of));
                }
            }
            targets.sort_unstable();
            let any_within = |v: &[usize], after: usize, upto: usize| {
                v.get(v.partition_point(|x| *x <= after))
                    .is_some_and(|x| *x <= upto)
            };
            let mut bad = HashSet::new();
            for &(j, k, ..) in &cands {
                if any_within(&control, j, k - 1) || any_within(&targets, j, k) {
                    bad.insert(j);
                }
            }
            for (a, &(j1, k1, ..)) in cands.iter().enumerate() {
                for &(j2, k2, ..) in &cands[a + 1..] {
                    if j2 >= k1 {
                        break;
                    }
                    if k2 > k1 {
                        bad.insert(j1);
                        bad.insert(j2);
                    }
                }
            }
            if bad.is_empty() {
                break;
            }
            cands.retain(|c| !bad.contains(&c.0));
        }

        let mut logical = HashMap::new();
        for &(_, _, op, close) in &cands {
            logical.entry(close).or_insert(op);
        }
        (cands.iter().map(|c| c.0).collect(), logical)
    }
}
