//! Turning a function's control-flow graph into if/else, loops and switches.
//!
//! ⚠ **Block order is source order.** The compiler lays a function out in the
//! order its source reads, so a region is a run of consecutive blocks: an
//! `if` is the blocks up to its jump target, a loop the blocks from its
//! header to the last block jumping back to it, a switch case the blocks up
//! to the next case. Emission walks each region once from its first block to
//! its last, which is what emits every block exactly once; a jump no
//! enclosing construct explains becomes a `goto` to a label instead.

use std::collections::{BTreeSet, HashMap, HashSet};

use super::ast::{own_continues, BinOp, Case, Expr, Kind, Stmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Block(usize),
    /// A code offset outside this function.
    Outside(i64),
}

#[derive(Clone, Debug)]
pub(super) enum Exit {
    Fall,
    Jump(Target),
    Branch {
        cond: Expr,
        target: Target,
    },
    Switch {
        value: Expr,
        cases: Vec<(i64, Target)>,
    },
    Return(Option<Expr>),
}

impl Exit {
    pub fn successors(&self, b: usize, n: usize) -> Vec<usize> {
        let block = |t: &Target| match t {
            Target::Block(t) => Some(*t),
            Target::Outside(_) => None,
        };
        let next = (b + 1 < n).then_some(b + 1);
        match self {
            Exit::Fall => next.into_iter().collect(),
            Exit::Jump(t) => block(t).into_iter().collect(),
            Exit::Branch { target, .. } => next.into_iter().chain(block(target)).collect(),
            Exit::Switch { cases, .. } => next
                .into_iter()
                .chain(cases.iter().filter_map(|(_, t)| block(t)))
                .collect(),
            Exit::Return(_) => Vec::new(),
        }
    }
}

pub(super) struct Block {
    /// Code offset of the first instruction.
    pub at: u32,
    pub stmts: Vec<Stmt>,
    pub exit: Exit,
    pub instructions: usize,
    /// Unreachable `NOP`s and jumps the compiler laid down to pad a code page.
    pub padding: bool,
}

/// Where control goes from the region being emitted.
#[derive(Clone, Copy, Default)]
struct Ctx {
    /// Where falling off the end of the region goes.
    follow: Option<usize>,
    /// What `break` leaves to: the innermost loop's or switch's exit.
    brk: Option<usize>,
    /// What `continue` goes back to: the innermost loop's header.
    cont: Option<usize>,
}

pub(super) struct Structurer<'a> {
    blocks: &'a [Block],
    preds: Vec<Vec<usize>>,
    in_loop: Vec<bool>,
    /// Times each block was emitted; every entry ends at one.
    pub emitted: Vec<u32>,
    pub gotos: usize,
    /// Gotos naming each label, by code offset.
    labels: HashMap<u32, usize>,
}

impl<'a> Structurer<'a> {
    pub fn new(blocks: &'a [Block]) -> Self {
        let n = blocks.len();
        let mut preds = vec![Vec::new(); n];
        for (b, block) in blocks.iter().enumerate() {
            // Dead code is structured too: it holds loops of its own.
            if !block.padding {
                for s in block.exit.successors(b, n) {
                    preds[s].push(b);
                }
            }
        }
        Structurer {
            blocks,
            preds,
            in_loop: vec![false; n],
            emitted: vec![0; n],
            gotos: 0,
            labels: HashMap::new(),
        }
    }

    pub fn run(&mut self) -> Vec<Stmt> {
        self.seq(0, self.blocks.len(), Ctx::default())
    }

    /// The labels some goto still names.
    pub fn used_labels(&self) -> HashSet<u32> {
        self.labels
            .iter()
            .filter(|(_, n)| **n > 0)
            .map(|(at, _)| *at)
            .collect()
    }

    /// The step of a `for` whose `continue`s jump to its latch rather than its
    /// header, taken off the end of `body` with those gotos made `continue`.
    ///
    /// ⚠ A `continue` in a `for` runs the step, so a jump to a latch that only
    /// steps and goes back is one, but only in a loop printed as that `for`.
    fn step_continues(
        &mut self,
        body: &mut Vec<Stmt>,
        header: usize,
        latch: usize,
    ) -> Option<Stmt> {
        let block = &self.blocks[latch];
        let back = matches!(block.exit, Exit::Jump(Target::Block(h)) if h == header);
        let [step @ Stmt::Assign(..)] = block.stmts.as_slice() else {
            return None;
        };
        let at = block.at;
        if !back || latch == header || own_continues(body) > 0 {
            return None;
        }
        let (last, steps_last) = non_label(body).next_back().map(|(i, s)| (i, s == step))?;
        let jumps = level_gotos(body, at, false);
        if !steps_last || jumps == 0 {
            return None;
        }
        let step = body.remove(last);
        level_gotos(body, at, true);
        self.gotos -= jumps;
        if let Some(n) = self.labels.get_mut(&at) {
            *n -= jumps;
        }
        Some(step)
    }

    /// The last block in `[header, to)` that jumps back to `header`.
    fn latch(&self, header: usize, to: usize) -> Option<usize> {
        self.preds[header]
            .iter()
            .copied()
            .filter(|p| *p >= header && *p < to)
            .max()
    }

    fn goto(&mut self, target: usize) -> Stmt {
        let at = self.blocks[target].at;
        *self.labels.entry(at).or_default() += 1;
        self.gotos += 1;
        Stmt::Goto(at)
    }

    /// The statement that sends control to `target`, if one is needed.
    fn jump(&mut self, target: usize, at_end: bool, ctx: Ctx) -> Option<Stmt> {
        if at_end && Some(target) == ctx.follow {
            return None;
        }
        if Some(target) == ctx.brk {
            return Some(Stmt::Break);
        }
        if Some(target) == ctx.cont {
            return Some(Stmt::Continue);
        }
        if target == self.blocks.len() {
            return None;
        }
        Some(self.goto(target))
    }

    fn jump_to(&mut self, target: Target, at_end: bool, ctx: Ctx) -> Option<Stmt> {
        match target {
            Target::Block(t) => self.jump(t, at_end, ctx),
            Target::Outside(at) => {
                self.gotos += 1;
                Some(Stmt::Goto(at as u32))
            }
        }
    }

    /// After a construct ends at `b`: falling into the region's end needs a jump
    /// unless that is where the region goes anyway.
    fn reach_end(&mut self, out: &mut Vec<Stmt>, b: usize, to: usize, ctx: Ctx) {
        if b == to && Some(to) != ctx.follow {
            out.extend(self.jump(to, true, ctx));
        }
    }

    fn seq(&mut self, from: usize, to: usize, ctx: Ctx) -> Vec<Stmt> {
        let mut out = Vec::new();
        let mut b = from;
        while b < to {
            if !self.in_loop[b] {
                if let Some(latch) = self.latch(b, to) {
                    let exit = latch + 1;
                    self.in_loop[b] = true;
                    let mut body = self.seq(
                        b,
                        exit,
                        Ctx {
                            follow: Some(b),
                            brk: Some(exit),
                            cont: Some(b),
                        },
                    );
                    self.in_loop[b] = false;
                    let step = self.step_continues(&mut body, b, latch);
                    let lp = make_loop(body, &mut out, step);
                    out.push(lp);
                    b = exit;
                    self.reach_end(&mut out, b, to, ctx);
                    continue;
                }
            }

            self.emitted[b] += 1;
            let block = &self.blocks[b];
            out.push(Stmt::Label(block.at));
            if block.padding {
                b += 1;
                continue;
            }
            out.extend(block.stmts.iter().cloned());
            let at_end = b + 1 == to;
            match block.exit.clone() {
                Exit::Fall => {
                    if at_end {
                        out.extend(self.jump(b + 1, true, ctx));
                    }
                    b += 1;
                }
                Exit::Jump(target) => {
                    if target != Target::Block(b + 1) || at_end {
                        out.extend(self.jump_to(target, at_end, ctx));
                    }
                    b += 1;
                }
                Exit::Return(value) => {
                    out.push(Stmt::Return(value));
                    b += 1;
                }
                Exit::Branch { cond, target } => {
                    b = self.branch(&mut out, b, cond, target, to, ctx);
                }
                Exit::Switch { value, cases } => {
                    b = self.switch(&mut out, b, value, cases, to, ctx);
                }
            }
        }
        out
    }

    fn branch(
        &mut self,
        out: &mut Vec<Stmt>,
        mut b: usize,
        mut cond: Expr,
        target: Target,
        to: usize,
        ctx: Ctx,
    ) -> usize {
        let t = match target {
            Target::Block(t) if t > b && t <= to && Some(t) != ctx.brk && Some(t) != ctx.cont => t,
            _ => {
                let then: Vec<Stmt> = self.jump_to(target, false, ctx).into_iter().collect();
                out.push(Stmt::If {
                    cond: cond.negate(),
                    then,
                    els: Vec::new(),
                });
                if b + 1 == to {
                    out.extend(self.jump(to, true, ctx));
                }
                return b + 1;
            }
        };

        // `if (a && b)` is a branch to the else for each operand: a block that only
        // tests and leaves for the same place, reached from this one alone, joins
        // the condition.
        while b + 1 < t {
            let next = &self.blocks[b + 1];
            let joins = next.stmts.is_empty() && !next.padding && self.preds[b + 1] == [b];
            match &next.exit {
                Exit::Branch {
                    cond: more,
                    target: Target::Block(same),
                } if joins && *same == t => {
                    cond = Expr::bin(BinOp::LogicalAnd, Kind::Int, cond, more.clone());
                    b += 1;
                    self.emitted[b] += 1;
                }
                _ => break,
            }
        }

        let mut follow = t;
        if t > b + 1 && !self.blocks[t - 1].padding {
            if let Exit::Jump(Target::Block(e)) = self.blocks[t - 1].exit {
                let inside = e < to && Some(e) != ctx.brk && Some(e) != ctx.cont;
                if e > t && (inside || (e == to && ctx.follow == Some(to))) {
                    follow = e;
                }
            }
        }
        let inner = Ctx {
            follow: Some(follow),
            ..ctx
        };
        let then = self.seq(b + 1, t, inner);
        let els = if follow > t {
            self.seq(t, follow, inner)
        } else {
            Vec::new()
        };
        out.push(Stmt::If { cond, then, els });
        self.reach_end(out, follow, to, ctx);
        follow
    }

    fn switch(
        &mut self,
        out: &mut Vec<Stmt>,
        b: usize,
        value: Expr,
        cases: Vec<(i64, Target)>,
        to: usize,
        ctx: Ctx,
    ) -> usize {
        let targets: Option<Vec<(i64, usize)>> = cases
            .iter()
            .map(|(v, t)| match t {
                Target::Block(t) if *t > b && *t <= to => Some((*v, *t)),
                _ => None,
            })
            .collect();
        let default_block = b + 1;
        let Some(targets) = targets.filter(|_| default_block <= to) else {
            // No region holds the cases: each one is a goto.
            let mut gone = Vec::new();
            for (v, t) in cases {
                let body = self.jump_to(t, false, ctx).into_iter().collect();
                gone.push(Case {
                    labels: vec![Some(v)],
                    body,
                });
            }
            out.push(Stmt::Switch { value, cases: gone });
            if b + 1 == to {
                out.extend(self.jump(to, true, ctx));
            }
            return b + 1;
        };

        let trampoline = match self.blocks.get(default_block) {
            Some(d) if default_block < to && d.stmts.is_empty() => match d.exit {
                Exit::Jump(Target::Block(x)) if x > b && x <= to => Some(x),
                _ => None,
            },
            _ => None,
        };
        let default = trampoline.unwrap_or(default_block);
        let max_case = targets.iter().map(|(_, t)| *t).max().unwrap_or(default);

        let starts: BTreeSet<usize> = targets
            .iter()
            .map(|(_, t)| *t)
            .chain(std::iter::once(default))
            .collect();
        let mut exits = Vec::new();
        if trampoline.is_some() && default > max_case {
            exits.push(default);
        }
        for s in starts.iter().skip(1) {
            if let Exit::Jump(Target::Block(x)) = self.blocks[s - 1].exit {
                if s - 1 > b && x > max_case && x <= to {
                    exits.push(x);
                }
            }
        }
        let exit = exits.into_iter().max().unwrap_or(to).max(max_case);
        let body_starts: Vec<usize> = starts.iter().copied().filter(|s| *s < exit).collect();
        let first = body_starts.first().copied().unwrap_or(exit);

        let mut out_cases = Vec::new();
        let mut pre = Vec::new();
        let mut from = default_block;
        if trampoline.is_some() {
            self.emitted[default_block] += 1;
            pre.push(Stmt::Label(self.blocks[default_block].at));
            from += 1;
        }
        let inner = |follow: usize| Ctx {
            follow: Some(follow),
            brk: Some(exit),
            cont: ctx.cont,
        };
        if from < first {
            pre.extend(self.seq(from, first, inner(first)));
        }
        out_cases.push(Case {
            labels: Vec::new(),
            body: pre,
        });
        for (n, s) in body_starts.iter().enumerate() {
            let end = body_starts.get(n + 1).copied().unwrap_or(exit);
            let mut labels: Vec<Option<i64>> = targets
                .iter()
                .filter(|(_, t)| t == s)
                .map(|(v, _)| Some(*v))
                .collect();
            if default == *s {
                labels.push(None);
            }
            let body = self.seq(*s, end, inner(end));
            out_cases.push(Case { labels, body });
        }
        let empty: Vec<Option<i64>> = targets
            .iter()
            .filter(|(_, t)| *t == exit)
            .map(|(v, _)| Some(*v))
            .collect();
        if !empty.is_empty() {
            out_cases.push(Case {
                labels: empty,
                body: vec![Stmt::Break],
            });
        }
        out.push(Stmt::Switch {
            value,
            cases: out_cases,
        });
        self.reach_end(out, exit, to, ctx);
        exit
    }
}

fn non_label(stmts: &[Stmt]) -> impl DoubleEndedIterator<Item = (usize, &Stmt)> {
    stmts
        .iter()
        .enumerate()
        .filter(|(_, s)| !matches!(s, Stmt::Label(_)))
}

/// Count the gotos to `at` that belong to this loop and not a nested one,
/// making each a `continue` when `replace` is set.
fn level_gotos(stmts: &mut [Stmt], at: u32, replace: bool) -> usize {
    let mut n = 0;
    for s in stmts {
        match s {
            Stmt::Goto(to) if *to == at => {
                n += 1;
                if replace {
                    *s = Stmt::Continue;
                }
            }
            Stmt::If { then, els, .. } => {
                n += level_gotos(then, at, replace) + level_gotos(els, at, replace);
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    n += level_gotos(&mut c.body, at, replace);
                }
            }
            _ => {}
        }
    }
    n
}

/// A `while (true)` around `body`, made a `while`, `do`/`while` or `for`
/// where its shape says so; `before` loses the `for`'s initialiser.
fn make_loop(mut body: Vec<Stmt>, before: &mut Vec<Stmt>, step: Option<Stmt>) -> Stmt {
    let head = non_label(&body).next().and_then(|(i, s)| match s {
        Stmt::If { then, els, .. } if then.as_slice() == [Stmt::Break] && els.is_empty() => Some(i),
        _ => None,
    });
    let cond = head.and_then(|i| match body.remove(i) {
        Stmt::If { cond, .. } => Some(cond.negate()),
        _ => None,
    });
    if let Some(step) = step {
        let init = match (before.last(), &step) {
            (Some(Stmt::Assign(v, _)), Stmt::Assign(stepped, _)) if v == stepped => before.pop(),
            _ => None,
        };
        return Stmt::For {
            init: init.map(Box::new),
            cond,
            step: Box::new(step),
            body,
        };
    }
    if let Some(cond) = cond {
        return make_for(cond, body, before);
    }

    let tail: Vec<usize> = non_label(&body).rev().take(2).map(|(i, _)| i).collect();
    if let [last, prev] = tail[..] {
        let repeats = matches!(
            &body[prev],
            Stmt::If { then, els, .. } if then.as_slice() == [Stmt::Continue] && els.is_empty()
        );
        if body[last] == Stmt::Break && repeats && own_continues(&body) == 1 {
            body.remove(last);
            if let Stmt::If { cond, .. } = body.remove(prev) {
                return Stmt::DoWhile { body, cond };
            }
        }
    }
    Stmt::While { cond: None, body }
}

fn make_for(cond: Expr, mut body: Vec<Stmt>, before: &mut Vec<Stmt>) -> Stmt {
    let counter = match before.last() {
        Some(Stmt::Assign(Expr::Var(v), _)) if cond.mentions(*v) => Some(*v),
        _ => None,
    };
    let step_at = non_label(&body)
        .next_back()
        .and_then(|(i, s)| match (s, counter) {
            (Stmt::Assign(Expr::Var(v), rhs), Some(c)) if *v == c && rhs.mentions(c) => Some(i),
            _ => None,
        });
    if let (Some(step_at), Some(init)) = (step_at, before.last().cloned()) {
        if own_continues(&body) == 0 {
            before.pop();
            let step = body.remove(step_at);
            return Stmt::For {
                init: Some(Box::new(init)),
                cond: Some(cond),
                step: Box::new(step),
                body,
            };
        }
    }
    Stmt::While {
        cond: Some(cond),
        body,
    }
}
