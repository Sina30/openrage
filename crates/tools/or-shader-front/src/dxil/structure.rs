//! A function's control flow as naga's structured statements: ifs, loops and switches found from
//! the dominator tree, each branch made a fall into the next construct, a `Break`, a `Continue` or
//! a refusal by name.

use std::collections::{BTreeSet, HashMap};

use super::ir::{BlockId, Body, Op, ValueId};

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// The instructions of a basic block, its terminator left out.
    Code(BlockId),
    /// Before leaving `from` for `to`: store each of `to`'s phis' incoming value from `from`.
    Edge {
        from: BlockId,
        to: BlockId,
    },
    If {
        cond: ValueId,
        then: Vec<Node>,
        otherwise: Vec<Node>,
    },
    Loop {
        body: Vec<Node>,
    },
    /// Cases grouped by target block; a case that falls through into another is refused.
    Switch {
        value: ValueId,
        cases: Vec<(Vec<i64>, Vec<Node>)>,
        default: Vec<Node>,
    },
    Break,
    Continue,
    Return(Option<ValueId>),
    Unreachable,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StructureError {
    #[error("block {0} is entered other than through its loop's header: irreducible control flow")]
    Irreducible(BlockId),
    #[error("block {from} jumps to block {to}, which is neither the next construct, its loop's exit nor its header")]
    Exit { from: BlockId, to: BlockId },
    #[error("a case of the switch at block {0} falls through into another")]
    Fallthrough(BlockId),
    #[error("block {0} is not reached by a terminator the structurizer reads")]
    Shape(BlockId),
}

/// Constructs nested deeper than this are refused as `Shape`: it bounds the walk's recursion.
const MAX_NESTING: usize = 256;

/// The structured form of `body`'s control flow, entered at block 0.
pub fn structure(body: &Body) -> Result<Vec<Node>, StructureError> {
    let mut walk = Walk::new(body)?;
    let mut out = Vec::new();
    walk.tree(0, &mut out)?;
    Ok(out)
}

/// Every phi and every instruction result read outside its own block: the values kept in locals.
// A naga expression is in scope only in the statement block that emitted it and those nested in
// it. A phi's incoming value counts as read in its incoming block, where its `Edge` stores it.
pub fn crossing(body: &Body) -> BTreeSet<ValueId> {
    let mut home = HashMap::new();
    for (block, basic) in body.blocks.iter().enumerate() {
        for inst in &basic.insts {
            if let Some(result) = inst.result {
                home.insert(result, block);
            }
        }
    }
    let elsewhere = |value: ValueId, block: usize| home.get(&value).is_some_and(|&h| h != block);
    let mut out = BTreeSet::new();
    for (block, basic) in body.blocks.iter().enumerate() {
        for inst in &basic.insts {
            if let Op::Phi { incoming } = &inst.op {
                out.extend(inst.result);
                for &(value, from) in incoming {
                    if elsewhere(value, from as usize) {
                        out.insert(value);
                    }
                }
            } else {
                for value in operands(&inst.op) {
                    if elsewhere(value, block) {
                        out.insert(value);
                    }
                }
            }
        }
    }
    out
}

/// The values an instruction reads.
fn operands(op: &Op) -> Vec<ValueId> {
    match op {
        Op::Binary { lhs, rhs, .. } | Op::Cmp { lhs, rhs, .. } => vec![*lhs, *rhs],
        Op::Cast { value, .. } => vec![*value],
        Op::Select {
            cond,
            then,
            otherwise,
        } => vec![*cond, *then, *otherwise],
        Op::ExtractValue { aggregate, .. } => vec![*aggregate],
        Op::InsertValue {
            aggregate, value, ..
        } => vec![*aggregate, *value],
        Op::ExtractElement { vector, index } => vec![*vector, *index],
        Op::InsertElement {
            vector,
            value,
            index,
        } => vec![*vector, *value, *index],
        Op::ShuffleVector { a, b, mask } => vec![*a, *b, *mask],
        Op::Phi { incoming } => incoming.iter().map(|&(value, _)| value).collect(),
        Op::Alloca { count, .. } => vec![*count],
        Op::Load { ptr } => vec![*ptr],
        Op::Store { ptr, value } | Op::AtomicRmw { ptr, value, .. } => vec![*ptr, *value],
        Op::Gep { base, indices, .. } => std::iter::once(*base)
            .chain(indices.iter().copied())
            .collect(),
        Op::CmpXchg { ptr, expected, new } => vec![*ptr, *expected, *new],
        Op::Dx { args, .. } => args.clone(),
        Op::Call { callee, args } => std::iter::once(*callee)
            .chain(args.iter().copied())
            .collect(),
        Op::CondBr { cond, .. } => vec![*cond],
        Op::Switch { value, .. } => vec![*value],
        Op::Ret { value } => value.iter().copied().collect(),
        Op::Br { .. } | Op::Unreachable => Vec::new(),
    }
}

/// What encloses the point being emitted, innermost last.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Frame {
    /// A branch of an `If`, which is the last statement before whatever encloses it.
    If,
    /// A case of the switch ending block `0`; a `Break` here would leave the switch.
    Switch(BlockId),
    Loop(BlockId),
    /// The code before merge node `0`, which falling off its end runs.
    Follow(BlockId),
}

struct Walk<'a> {
    body: &'a Body,
    /// Each block's place in reverse post-order; `usize::MAX` for one the entry does not reach.
    rpo: Vec<usize>,
    header: Vec<bool>,
    /// Two or more forward predecessors.
    merge: Vec<bool>,
    /// The merge nodes each block immediately dominates, in reverse post-order.
    merges: Vec<Vec<BlockId>>,
    phis: Vec<bool>,
    frames: Vec<Frame>,
    depth: usize,
}

impl<'a> Walk<'a> {
    fn new(body: &'a Body) -> Result<Self, StructureError> {
        let count = body.blocks.len();
        if count == 0 {
            return Err(StructureError::Shape(0));
        }
        let successors: Vec<Vec<BlockId>> =
            (0..count).map(|b| body.successors(b as BlockId)).collect();

        let mut post = Vec::with_capacity(count);
        let mut seen = vec![false; count];
        seen[0] = true;
        let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
        while let Some(top) = stack.last_mut() {
            let (block, next) = *top;
            match successors[block].get(next) {
                Some(&to) => {
                    top.1 += 1;
                    let target = to as usize;
                    if target >= count {
                        return Err(StructureError::Shape(to));
                    }
                    if !seen[target] {
                        seen[target] = true;
                        stack.push((target, 0));
                    }
                }
                None => {
                    post.push(block);
                    stack.pop();
                }
            }
        }
        let order: Vec<usize> = post.into_iter().rev().collect();
        let mut rpo = vec![usize::MAX; count];
        for (place, &block) in order.iter().enumerate() {
            rpo[block] = place;
        }
        let mut preds = vec![Vec::new(); count];
        for &block in &order {
            for &to in &successors[block] {
                preds[to as usize].push(rpo[block]);
            }
        }

        // Cooper, Harvey and Kennedy's iteration, over reverse post-order places.
        let mut idom = vec![usize::MAX; order.len()];
        idom[0] = 0;
        let mut changed = true;
        while changed {
            changed = false;
            for (place, &block) in order.iter().enumerate().skip(1) {
                let mut new = usize::MAX;
                for &pred in &preds[block] {
                    if idom[pred] != usize::MAX {
                        new = if new == usize::MAX {
                            pred
                        } else {
                            intersect(&idom, pred, new)
                        };
                    }
                }
                if idom[place] != new {
                    idom[place] = new;
                    changed = true;
                }
            }
        }

        let mut header = vec![false; count];
        let mut forward = vec![0u32; count];
        for &block in &order {
            for &to in &successors[block] {
                let (from, target) = (rpo[block], rpo[to as usize]);
                if target <= from {
                    if !dominates(&idom, target, from) {
                        return Err(StructureError::Irreducible(to));
                    }
                    header[to as usize] = true;
                } else {
                    forward[to as usize] += 1;
                }
            }
        }
        let merge: Vec<bool> = forward.iter().map(|&n| n >= 2).collect();
        let mut merges = vec![Vec::new(); count];
        for (place, &block) in order.iter().enumerate().skip(1) {
            if merge[block] {
                merges[order[idom[place]]].push(block as BlockId);
            }
        }
        let phis = body
            .blocks
            .iter()
            .map(|b| b.insts.iter().any(|inst| matches!(inst.op, Op::Phi { .. })))
            .collect();

        Ok(Self {
            body,
            rpo,
            header,
            merge,
            merges,
            phis,
            frames: Vec::new(),
            depth: 0,
        })
    }

    /// Emit `block` and what it dominates; a loop header as a `Loop`.
    fn tree(&mut self, mut block: BlockId, out: &mut Vec<Node>) -> Result<(), StructureError> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            return Err(StructureError::Shape(block));
        }
        loop {
            if self.header[block as usize] {
                self.frames.push(Frame::Loop(block));
                let mut body = Vec::new();
                if let Some(last) = self.within(block, &mut body)? {
                    self.tree(last, &mut body)?;
                }
                self.frames.pop();
                out.push(Node::Loop { body });
                break;
            }
            // The last merge node follows everything else here: emitted in this loop, not nested.
            match self.within(block, out)? {
                Some(last) => block = last,
                None => break,
            }
        }
        self.depth -= 1;
        Ok(())
    }

    /// Emit `block`, then each merge node it immediately dominates in reverse post-order, each
    /// after the code that reaches it; the last is returned for the caller to emit next.
    fn within(
        &mut self,
        block: BlockId,
        out: &mut Vec<Node>,
    ) -> Result<Option<BlockId>, StructureError> {
        let merges = std::mem::take(&mut self.merges[block as usize]);
        for &merge in merges.iter().rev() {
            self.frames.push(Frame::Follow(merge));
        }
        self.leaf(block, out)?;
        for (n, &merge) in merges.iter().enumerate() {
            self.frames.pop();
            if n + 1 == merges.len() {
                return Ok(Some(merge));
            }
            self.tree(merge, out)?;
        }
        Ok(None)
    }

    /// Emit `block`'s code and its terminator.
    fn leaf(&mut self, block: BlockId, out: &mut Vec<Node>) -> Result<(), StructureError> {
        out.push(Node::Code(block));
        let body = self.body;
        let Some(last) = body.blocks[block as usize].insts.last() else {
            return Err(StructureError::Shape(block));
        };
        match &last.op {
            Op::Br { target } => self.branch(block, *target, out),
            Op::CondBr {
                then, otherwise, ..
            } if then == otherwise => self.branch(block, *then, out),
            Op::CondBr {
                cond,
                then,
                otherwise,
            } => {
                self.frames.push(Frame::If);
                let mut then_nodes = Vec::new();
                self.branch(block, *then, &mut then_nodes)?;
                let mut otherwise_nodes = Vec::new();
                self.branch(block, *otherwise, &mut otherwise_nodes)?;
                self.frames.pop();
                out.push(Node::If {
                    cond: *cond,
                    then: then_nodes,
                    otherwise: otherwise_nodes,
                });
                Ok(())
            }
            Op::Switch {
                value,
                default,
                cases,
            } => {
                // A case that goes where the default goes is the default's.
                let mut groups: Vec<(BlockId, Vec<i64>)> = Vec::new();
                for &(case, to) in cases {
                    if to == *default {
                        continue;
                    }
                    match groups.iter_mut().find(|(target, _)| *target == to) {
                        Some((_, values)) => values.push(case),
                        None => groups.push((to, vec![case])),
                    }
                }
                if groups.is_empty() {
                    return self.branch(block, *default, out);
                }
                self.frames.push(Frame::Switch(block));
                let mut arms = Vec::with_capacity(groups.len());
                for (to, values) in groups {
                    let mut nodes = Vec::new();
                    self.branch(block, to, &mut nodes)?;
                    arms.push((values, nodes));
                }
                let mut fallback = Vec::new();
                self.branch(block, *default, &mut fallback)?;
                self.frames.pop();
                out.push(Node::Switch {
                    value: *value,
                    cases: arms,
                    default: fallback,
                });
                Ok(())
            }
            Op::Ret { value } => {
                out.push(Node::Return(*value));
                Ok(())
            }
            Op::Unreachable => {
                out.push(Node::Unreachable);
                Ok(())
            }
            _ => Err(StructureError::Shape(block)),
        }
    }

    /// Emit the branch from `from` to `to`: its phi copies, then a `Continue` to the innermost
    /// loop's header, a fall or a single `Break` into the merge node that follows, or `to` itself
    /// where `from` is its only forward predecessor.
    fn branch(
        &mut self,
        from: BlockId,
        to: BlockId,
        out: &mut Vec<Node>,
    ) -> Result<(), StructureError> {
        let target = to as usize;
        if self.phis[target] {
            out.push(Node::Edge { from, to });
        }
        if self.rpo[target] <= self.rpo[from as usize] {
            let innermost = self.frames.iter().rev().find_map(|frame| match *frame {
                Frame::Loop(header) => Some(header),
                _ => None,
            });
            if innermost != Some(to) {
                return Err(StructureError::Exit { from, to });
            }
            out.push(Node::Continue);
            return Ok(());
        }
        if !self.merge[target] {
            return self.tree(to, out);
        }
        let mut left_loop = false;
        let mut switch_before_loop = false;
        let mut switches = Vec::new();
        for frame in self.frames.iter().rev() {
            match *frame {
                Frame::If => {}
                Frame::Switch(block) => {
                    switches.push(block);
                    switch_before_loop |= !left_loop;
                }
                Frame::Loop(_) => {
                    if left_loop || switch_before_loop {
                        return Err(StructureError::Exit { from, to });
                    }
                    left_loop = true;
                }
                Frame::Follow(merge) if merge == to => {
                    if left_loop {
                        out.push(Node::Break);
                    }
                    return Ok(());
                }
                Frame::Follow(merge) => {
                    let body = self.body;
                    let into_case = switches
                        .iter()
                        .find(|&&s| body.successors(s).contains(&merge));
                    return Err(match into_case {
                        Some(&switch) => StructureError::Fallthrough(switch),
                        None => StructureError::Exit { from, to },
                    });
                }
            }
        }
        Err(StructureError::Exit { from, to })
    }
}

fn intersect(idom: &[usize], mut a: usize, mut b: usize) -> usize {
    while a != b {
        while a > b {
            a = idom[a];
        }
        while b > a {
            b = idom[b];
        }
    }
    a
}

/// Whether place `a` dominates place `b`, both reached, after the iteration has settled.
fn dominates(idom: &[usize], a: usize, mut b: usize) -> bool {
    while b > a {
        b = idom[b];
    }
    b == a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxil::ir::{BasicBlock, BinOp, CastOp, Inst};

    fn block(ops: Vec<Op>) -> BasicBlock {
        BasicBlock {
            insts: ops
                .into_iter()
                .map(|op| Inst {
                    result: None,
                    ty: 0,
                    op,
                })
                .collect(),
        }
    }

    fn body(blocks: Vec<BasicBlock>) -> Body {
        Body {
            blocks,
            values: Vec::new(),
        }
    }

    fn phi(incoming: Vec<(ValueId, BlockId)>) -> Op {
        Op::Phi { incoming }
    }

    fn br(target: BlockId) -> Op {
        Op::Br { target }
    }

    fn cond_br(cond: ValueId, then: BlockId, otherwise: BlockId) -> Op {
        Op::CondBr {
            cond,
            then,
            otherwise,
        }
    }

    fn ret() -> Op {
        Op::Ret { value: None }
    }

    #[test]
    fn a_diamond_is_an_if_with_its_merge_after_it() {
        let diamond = body(vec![
            block(vec![cond_br(100, 1, 2)]),
            block(vec![br(3)]),
            block(vec![br(3)]),
            block(vec![
                phi(vec![(101, 1), (102, 2)]),
                Op::Ret { value: Some(103) },
            ]),
        ]);
        assert_eq!(
            structure(&diamond).unwrap(),
            vec![
                Node::Code(0),
                Node::If {
                    cond: 100,
                    then: vec![Node::Code(1), Node::Edge { from: 1, to: 3 }],
                    otherwise: vec![Node::Code(2), Node::Edge { from: 2, to: 3 }],
                },
                Node::Code(3),
                Node::Return(Some(103)),
            ]
        );
    }

    #[test]
    fn a_loop_continues_at_its_header_and_breaks_to_its_exit() {
        // 0 skips the loop or enters it; 1 is the header, 2 the latch, 3 the exit both reach.
        let looped = body(vec![
            block(vec![cond_br(100, 1, 3)]),
            block(vec![phi(vec![(104, 0), (105, 2)]), cond_br(101, 2, 3)]),
            block(vec![br(1)]),
            block(vec![ret()]),
        ]);
        assert_eq!(
            structure(&looped).unwrap(),
            vec![
                Node::Code(0),
                Node::If {
                    cond: 100,
                    then: vec![
                        Node::Edge { from: 0, to: 1 },
                        Node::Loop {
                            body: vec![
                                Node::Code(1),
                                Node::If {
                                    cond: 101,
                                    then: vec![
                                        Node::Code(2),
                                        Node::Edge { from: 2, to: 1 },
                                        Node::Continue,
                                    ],
                                    otherwise: vec![Node::Break],
                                },
                            ],
                        },
                    ],
                    otherwise: vec![],
                },
                Node::Code(3),
                Node::Return(None),
            ]
        );
    }

    #[test]
    fn a_cycle_with_two_entries_is_irreducible() {
        let cycle = body(vec![
            block(vec![cond_br(100, 1, 2)]),
            block(vec![br(2)]),
            block(vec![br(1)]),
        ]);
        assert_eq!(structure(&cycle), Err(StructureError::Irreducible(1)));
    }

    #[test]
    fn a_break_out_of_two_loops_is_refused() {
        // Outer loop 1, inner loop 2; block 3 leaves both for 5, which 0 also reaches.
        let nested = body(vec![
            block(vec![cond_br(100, 1, 5)]),
            block(vec![br(2)]),
            block(vec![cond_br(101, 3, 4)]),
            block(vec![cond_br(102, 2, 5)]),
            block(vec![br(1)]),
            block(vec![ret()]),
        ]);
        assert_eq!(
            structure(&nested),
            Err(StructureError::Exit { from: 3, to: 5 })
        );
    }

    #[test]
    fn a_switch_groups_its_cases_by_target() {
        let switch = body(vec![
            block(vec![Op::Switch {
                value: 100,
                default: 3,
                cases: vec![(1, 1), (2, 2), (7, 1), (9, 3)],
            }]),
            block(vec![br(3)]),
            block(vec![br(3)]),
            block(vec![ret()]),
        ]);
        assert_eq!(
            structure(&switch).unwrap(),
            vec![
                Node::Code(0),
                Node::Switch {
                    value: 100,
                    cases: vec![
                        (vec![1, 7], vec![Node::Code(1)]),
                        (vec![2], vec![Node::Code(2)])
                    ],
                    default: vec![],
                },
                Node::Code(3),
                Node::Return(None),
            ]
        );
    }

    #[test]
    fn a_case_that_falls_into_another_is_refused() {
        let switch = body(vec![
            block(vec![Op::Switch {
                value: 100,
                default: 3,
                cases: vec![(1, 1), (2, 2)],
            }]),
            block(vec![br(2)]),
            block(vec![br(3)]),
            block(vec![ret()]),
        ]);
        assert_eq!(structure(&switch), Err(StructureError::Fallthrough(0)));
    }

    #[test]
    fn a_block_with_no_terminator_or_a_branch_out_of_the_function_is_refused() {
        assert_eq!(structure(&body(vec![])), Err(StructureError::Shape(0)));
        let open = body(vec![block(vec![br(1)]), BasicBlock::default()]);
        assert_eq!(structure(&open), Err(StructureError::Shape(1)));
        assert_eq!(
            structure(&body(vec![block(vec![br(7)])])),
            Err(StructureError::Shape(7))
        );
    }

    #[test]
    fn nesting_past_the_bound_is_refused_and_below_it_is_not() {
        // Block i branches on into block i + 1 or returns through block depth + i.
        let nested = |depth: u32| {
            let mut blocks: Vec<BasicBlock> = (0..depth)
                .map(|i| block(vec![cond_br(100 + i, i + 1, depth + 1 + i)]))
                .collect();
            blocks.push(block(vec![ret()]));
            blocks.extend((0..depth).map(|_| block(vec![ret()])));
            body(blocks)
        };
        assert!(structure(&nested(MAX_NESTING as u32 - 2)).is_ok());
        assert!(matches!(
            structure(&nested(MAX_NESTING as u32 + 4)),
            Err(StructureError::Shape(_))
        ));
    }

    #[test]
    fn values_used_in_another_block_and_phis_cross() {
        let mut flow = body(vec![
            block(vec![
                Op::Binary {
                    op: BinOp::Add,
                    lhs: 1,
                    rhs: 2,
                },
                cond_br(10, 1, 2),
            ]),
            block(vec![
                Op::Cast {
                    op: CastOp::Bitcast,
                    value: 10,
                },
                br(2),
            ]),
            block(vec![
                phi(vec![(11, 1), (12, 0)]),
                Op::Ret { value: Some(13) },
            ]),
        ]);
        flow.blocks[0].insts[0].result = Some(10);
        flow.blocks[1].insts[0].result = Some(11);
        flow.blocks[2].insts[0].result = Some(13);
        // 10 is read in block 1, 13 is a phi; 11 reaches the phi from its own block, and 1, 2, 12
        // are no instruction's results.
        assert_eq!(crossing(&flow), BTreeSet::from([10, 13]));
    }
}
