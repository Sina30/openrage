//! A GTA VI MoVE network's node outline: machines, states, transitions, bindings and leaf nodes read
//! from the root by their offsets. A node whose layout is not known is named and not read past.

use std::collections::{BTreeMap, BTreeSet};

use super::other_bodies::MoveNetworkHead;
use crate::error::{IoError, Result};
use crate::mrf::{FCurve, Keyframe, Operation, Operator};

const WHAT: &str = "GTA VI MoVE network";
const TAIL: u16 = 2;
const STATE: u16 = 22;
/// Bytes a binding block's head takes before its records.
const BINDING_HEAD: usize = 12;
/// The smallest transition: its fixed words up to the target offset at +0x48.
const TRANSITION_MIN: u32 = 0x4C;
const TRANSITION_WORDS: usize = TRANSITION_MIN as usize / 4;

/// A network's head and every node its walk from the root reached.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveOutline {
    pub head: MoveNetworkHead,
    /// The root node's offset in the body.
    pub root: usize,
    /// Every node read, in body order.
    pub nodes: Vec<MoveNode>,
    /// Every node reached whose layout is not known, in body order; none in a body walked whole.
    pub stops: Vec<MoveStop>,
    /// 12-byte tails no offset names, which sit between the nodes of a body walked whole.
    pub loose_tails: usize,
}

/// One node: the head every kind shares, the bytes its record takes and what it leads to.
#[derive(Debug, Clone, PartialEq)]
pub struct MoveNode {
    /// Offset in the body.
    pub at: usize,
    pub kind: u16,
    /// Its index among its state's children.
    pub index: u16,
    pub id: u32,
    /// Bytes from `at` its record takes, its transitions, entries and bindings included.
    pub size: usize,
    pub body: MoveBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MoveBody {
    /// Kind 1, or kind 3 with inputs.
    Machine(Machine),
    /// Kind 22.
    State(State),
    /// Any other kind its size rule reads: the word at +0x08 (0 for an 8-byte node) and its inputs.
    Leaf { flags: u32, inputs: Vec<usize> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
    /// Where its initial-state offset leads; `None` where it leaves the body.
    pub initial: Option<usize>,
    /// Each state's id and node offset, in table order.
    pub states: Vec<(u32, usize)>,
    /// Kind 3's input nodes.
    pub inputs: Vec<usize>,
    pub transitions: Vec<MoveTransition>,
    pub entries: Vec<MachineEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct State {
    /// The output node, which its initial offset leads to.
    pub output: usize,
    /// Children, not counting tails.
    pub children: u8,
    pub transitions: Vec<MoveTransition>,
    pub bindings: Vec<Binding>,
}

/// A transition: its size word, its target and its conditions; its other words are carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveTransition {
    pub at: usize,
    pub size: u32,
    /// Where its offset at +0x48 leads.
    pub target: usize,
    /// Its fixed words from +0x00 to +0x48 as stored, its size and target offset among them.
    pub words: [u32; TRANSITION_WORDS],
    /// As many as bits 6 to 8 of its flags state, from +0x4C to its size.
    pub conditions: Vec<MoveCondition>,
}

/// An entry of a machine's second list: conditions on one of its states, a role only inferred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineEntry {
    pub size: u16,
    /// Byte +3: 0 or 1, whose meaning is not known.
    pub flag: u8,
    /// Where its offset at +4 leads: one of the machine's states.
    pub state: usize,
    /// As many as byte +2 states, from +8 to its size.
    pub conditions: Vec<MoveCondition>,
}

/// A condition: a kind, a flag half and the payload words its kind sizes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveCondition {
    pub kind: u16,
    /// 0, or 1 in kinds 2 to 4: GTA V's `set` by its place, not confirmed.
    pub flag: u16,
    /// A signal or hash, then by kind a range's bounds and halves, a value, or a word not known.
    pub words: Vec<u32>,
}

/// What a state binds to one of its children: hashed records in four groups, then operations.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub child: u8,
    /// Group A, inferred inputs: a hash, then three words.
    pub inputs_a: Vec<[u32; 4]>,
    /// Group B, inferred inputs: a hash, then three words.
    pub inputs_b: Vec<[u32; 4]>,
    /// Group C, inferred outputs: a hash, then three words.
    pub outputs: Vec<[u32; 4]>,
    /// Group D, inferred events: a hash and a word.
    pub events: Vec<[u32; 2]>,
    /// In GTA V's form, each held to its size word.
    pub operations: Vec<Operation>,
}

/// A node the walk reached and did not read, since no layout of it is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveStop {
    pub at: usize,
    pub kind: u16,
    pub index: u16,
    pub id: u32,
    /// The word at +0x08: a keyed node's kind word, else its flags or count; 0 past the body.
    pub word: u32,
    pub reason: StopReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Kinds 9, 12 and 28: a kind word picks a field schema the bytes do not state.
    Keyed,
    /// Kinds 13, 30 and 35 in a form other than the one measured of each.
    Unmeasured,
    /// Kind 32 with byte 10 or 11 set or byte 9 neither 0 nor 2: no layout of that form is measured.
    WideCount,
    /// A clip whose context is 3, a form no body holds.
    ClipContext,
}

/// GTA V's name of a kind by its place in GTA V's list less five kinds: not confirmed.
pub fn kind_name(kind: u16) -> Option<&'static str> {
    Some(match kind {
        1 => "StateMachine",
        2 => "Tail",
        3 => "InlinedStateMachine",
        4 => "Blend",
        5 => "AddSubtract",
        6 => "Filter",
        8 => "Frame",
        10 => "BlendN",
        11 => "Clip",
        15 => "Capture",
        16 => "Proxy",
        17 => "AddN",
        18 => "Identity",
        19 => "Merge",
        20 => "Pose",
        21 => "MergeN",
        22 => "State",
        23 => "Invalid",
        25 => "SubNetwork",
        26 => "Reference",
        _ => return None,
    })
}

/// GTA V's name of a condition kind by its number, not confirmed; kinds 13 and 14 have none.
pub fn condition_name(kind: u16) -> Option<&'static str> {
    Some(match kind {
        0 => "InRange",
        1 => "OutOfRange",
        2 => "OnRequest",
        3 => "OnFlag",
        4 => "AtEvent",
        5 => "GreaterThan",
        6 => "GreaterThanEqual",
        7 => "LessThan",
        8 => "LessThanEqual",
        9 => "LifetimeGreaterThan",
        10 => "LifetimeLessThan",
        12 => "BoolEquals",
        _ => return None,
    })
}

impl MoveOutline {
    /// Reads the head and every node the root leads to, refusing what the bytes do not bear out.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let head = MoveNetworkHead::open(bytes)?;
        let root = bytes.len() - head.rest.len() - 8;
        let b = Body(bytes);
        let mut nodes = BTreeMap::new();
        let mut stops = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut todo = vec![root];
        while let Some(n) = todo.pop() {
            if !seen.insert(n) {
                continue;
            }
            if n < root || n + 8 > bytes.len() {
                return Err(refuse(format!("an offset leaves the body ({n:#X})")));
            }
            let (kind, index, id) = (b.u16(n)?, b.u16(n + 2)?, b.u32(n + 4)?);
            let (end, body) = match kind {
                1 | 3 => {
                    let (end, machine) = machine(&b, n, kind)?;
                    todo.extend(machine.states.iter().map(|s| s.1));
                    todo.extend(&machine.inputs);
                    (end, MoveBody::Machine(machine))
                }
                STATE => {
                    let (end, state) = state(&b, n)?;
                    todo.push(state.output);
                    (end, MoveBody::State(state))
                }
                _ => match leaf(&b, n, kind)? {
                    Ok((size, inputs)) => {
                        todo.extend(&inputs);
                        let flags = if size >= 12 { b.u32(n + 8)? } else { 0 };
                        (n + size, MoveBody::Leaf { flags, inputs })
                    }
                    Err(reason) => {
                        let word = b.u32(n + 8).unwrap_or(0);
                        let stop = MoveStop {
                            at: n,
                            kind,
                            index,
                            id,
                            word,
                            reason,
                        };
                        stops.insert(n, stop);
                        continue;
                    }
                },
            };
            let node = MoveNode {
                at: n,
                kind,
                index,
                id,
                size: end - n,
                body,
            };
            nodes.insert(n, node);
        }
        let mut outline = Self {
            head,
            root,
            nodes: nodes.into_values().collect(),
            stops: stops.into_values().collect(),
            loose_tails: 0,
        };
        if outline.stops.is_empty() {
            outline.loose_tails = outline.check(&b)?;
        }
        Ok(outline)
    }

    /// The node read at `at`.
    pub fn node(&self, at: usize) -> Option<&MoveNode> {
        let i = self.nodes.binary_search_by_key(&at, |n| n.at).ok()?;
        self.nodes.get(i)
    }

    /// The node reached at `at` and not read.
    pub fn stop(&self, at: usize) -> Option<&MoveStop> {
        self.stops.iter().find(|s| s.at == at)
    }

    /// Holds a body walked whole to its tiling, targets and child indices; counts the loose tails.
    fn check(&self, b: &Body) -> Result<usize> {
        let mut at = self.root;
        let mut tails = 0;
        let ends = self.nodes.iter().map(|n| (n.at, n.at + n.size));
        for (start, end) in ends.chain([(b.len(), b.len())]) {
            while at < start && at + 12 <= start && b.u16(at)? == TAIL {
                at += 12;
                tails += 1;
            }
            if start != at {
                return Err(refuse(format!(
                    "a gap or overlap at {at:#X}, before the node at {start:#X}"
                )));
            }
            at = end;
        }
        for node in &self.nodes {
            let (transitions, children) = match &node.body {
                MoveBody::Machine(m) => {
                    let states: Vec<usize> = m.states.iter().map(|s| s.1).collect();
                    if let Some(e) = m.entries.iter().find(|e| !states.contains(&e.state)) {
                        return Err(refuse(format!(
                            "an entry of the machine at {:#X} leads to {:#X}, none of its states",
                            node.at, e.state
                        )));
                    }
                    (&m.transitions, None)
                }
                MoveBody::State(s) => (&s.transitions, Some((s.output, s.children))),
                MoveBody::Leaf { .. } => continue,
            };
            for t in transitions {
                let lands = self.node(t.target).map(|n| n.kind);
                if !matches!(lands, Some(1 | 3 | STATE)) {
                    return Err(refuse(format!(
                        "the transition at {:#X} lands on no state ({:#X})",
                        t.at, t.target
                    )));
                }
            }
            if let Some((output, children)) = children {
                self.check_children(node.at, output, children)?;
            }
        }
        Ok(tails)
    }

    /// No node a state's output leads to holds a child index past the state's count.
    fn check_children(&self, state: usize, output: usize, children: u8) -> Result<()> {
        let mut visited = BTreeSet::new();
        let mut stack = vec![output];
        while let Some(at) = stack.pop() {
            if !visited.insert(at) {
                continue;
            }
            let node = self
                .node(at)
                .ok_or_else(|| refuse(format!("no node at {at:#X}")))?;
            if node.kind != TAIL && node.index >= u16::from(children) {
                return Err(refuse(format!(
                    "the state at {state:#X} reaches child index {} past its {children} children",
                    node.index
                )));
            }
            if let MoveBody::Leaf { inputs, .. } = &node.body {
                stack.extend(inputs);
            }
        }
        Ok(())
    }
}

/// A machine at `n`: its end, after its state table, transitions and entries, and what it holds.
fn machine(b: &Body, n: usize, kind: u16) -> Result<(usize, Machine)> {
    let (states, count) = (b.u8(n + 0x12)?, b.u8(n + 0x13)?);
    if b.u32(n + 0x20)? != 0 {
        return Err(refuse(format!(
            "the machine at {n:#X} has a word at +0x20 that is not zero"
        )));
    }
    let (table, input_count) = if kind == 3 {
        let inputs = b.u32(n + 0x58)?;
        (b.after(n + 0x5C, inputs, 4)?, inputs as usize)
    } else {
        (n + 0x58, 0)
    };
    let mut end = b.after(table, u32::from(states), 8)?;
    let mut transitions = Vec::new();
    if count > 0 {
        if b.rel(n + 0x24)? != end {
            return Err(refuse(format!(
                "the machine at {n:#X} has its transitions elsewhere than after its state table"
            )));
        }
        (transitions, end) = read_transitions(b, end, count)?;
    } else {
        let offset = i64::from(b.i32(n + 0x24)?);
        if offset != 0 && offset != end as i64 - (n + 0x24) as i64 {
            return Err(refuse(format!(
                "the machine at {n:#X} has no transitions and an offset at +0x24 that leads elsewhere"
            )));
        }
    }
    if b.rel(n + 0x54)? != end {
        return Err(refuse(format!(
            "the machine at {n:#X} has its entries elsewhere than after its transitions"
        )));
    }
    let mut entries = Vec::new();
    for _ in 0..b.u32(n + 0x50)? {
        let size = b.u16(end)?;
        if size < 16 || end + usize::from(size) > b.len() {
            return Err(refuse(format!(
                "the machine entry at {end:#X} states {size} bytes"
            )));
        }
        let flag = b.u8(end + 3)?;
        if flag > 1 {
            return Err(refuse(format!(
                "the machine entry at {end:#X} has flag {flag}"
            )));
        }
        let next = end + usize::from(size);
        entries.push(MachineEntry {
            size,
            flag,
            state: b.rel(end + 4)?,
            conditions: conditions(b, end + 8, u32::from(b.u8(end + 2)?), next)?,
        });
        end = next;
    }
    let states = (0..usize::from(states))
        .map(|k| Ok((b.u32(table + 8 * k)?, b.rel(table + 8 * k + 4)?)))
        .collect::<Result<_>>()?;
    let inputs = (0..input_count)
        .map(|k| b.rel(n + 0x5C + 4 * k))
        .collect::<Result<_>>()?;
    let machine = Machine {
        initial: b.rel(n + 8).ok(),
        states,
        inputs,
        transitions,
        entries,
    };
    Ok((end, machine))
}

/// A state at `n`: its end, after its transitions and binding blocks, and what it holds.
fn state(b: &Body, n: usize) -> Result<(usize, State)> {
    let (children, count) = (b.u8(n + 0x12)?, b.u8(n + 0x13)?);
    if b.rel(n + 0x24)? != n + 0x30 {
        return Err(refuse(format!(
            "the state at {n:#X} has its transitions elsewhere than at +0x30"
        )));
    }
    let (transitions, mut end) = read_transitions(b, n + 0x30, count)?;
    if b.rel(n + 0x28)? != end {
        return Err(refuse(format!(
            "the state at {n:#X} has its bindings elsewhere than after its transitions"
        )));
    }
    let mut bindings = Vec::new();
    let mut last = None;
    for _ in 0..b.u32(n + 0x2C)? {
        let (binding, size) = binding(b, end, children, last)?;
        last = Some(binding.child);
        bindings.push(binding);
        end += size;
    }
    let state = State {
        output: b.rel(n + 8)?,
        children,
        transitions,
        bindings,
    };
    Ok((end, state))
}

/// A binding block at `q` and the bytes it takes, its child after `last` and under `children`.
fn binding(b: &Body, q: usize, children: u8, last: Option<u8>) -> Result<(Binding, usize)> {
    let stated = b.u32(q)?;
    let child = b.u8(q + 4)?;
    let [a, bb, c, d, ops] = [6, 7, 8, 9, 10].map(|i| b.u8(q + i));
    let (a, bb, c, d, ops) = (a?, bb?, c?, d?, ops?);
    if b.u8(q + 5)? != 0
        || b.u8(q + 11)? != 0
        || last.is_some_and(|l| child <= l)
        || child >= children
    {
        return Err(refuse(format!(
            "the binding at {q:#X} binds child {child} of {children}, or its zero bytes are not zero"
        )));
    }
    let mut at = q + BINDING_HEAD;
    let mut group = |count: u8| {
        let read = records::<4>(b, at, count);
        at += 16 * usize::from(count);
        read
    };
    let (inputs_a, inputs_b, outputs) = (group(a)?, group(bb)?, group(c)?);
    let events = records::<2>(b, at, d)?;
    at += 8 * usize::from(d);
    let mut operations = Vec::new();
    for _ in 0..ops {
        let (op, size) = operation(b, at)?;
        operations.push(op);
        at += size;
    }
    if at as u64 != q as u64 + u64::from(stated) {
        return Err(refuse(format!(
            "the binding at {q:#X} states {stated} bytes and holds {}",
            at - q
        )));
    }
    let binding = Binding {
        child,
        inputs_a,
        inputs_b,
        outputs,
        events,
        operations,
    };
    Ok((binding, at - q))
}

fn records<const N: usize>(b: &Body, at: usize, count: u8) -> Result<Vec<[u32; N]>> {
    (0..usize::from(count))
        .map(|i| {
            let mut words = [0; N];
            for (j, word) in words.iter_mut().enumerate() {
                *word = b.u32(at + 4 * (N * i + j))?;
            }
            Ok(words)
        })
        .collect()
}

/// An operation in GTA V's form at `p` and its size, which its size word must state.
fn operation(b: &Body, p: usize) -> Result<(Operation, usize)> {
    let stated = b.u16(p + 4)?;
    let wrong = |why: String| refuse(format!("the operation at {p:#X}: {why}"));
    let mut q = p + 8;
    let mut operators = Vec::new();
    loop {
        let (operator_type, packet) = (b.u32(q)?, b.u32(q + 4)?);
        q += 8;
        let fcurve = if operator_type == Operator::FCURVE {
            let keys = b.u32(q + 8)?;
            let keyframes_offset = b.i32(q + 12)?;
            let held = (q - p) as u64 + 16 + 16 * u64::from(keys);
            if packet != 4 || keyframes_offset != 4 || held > u64::from(stated) {
                return Err(wrong(format!(
                    "a curve of {keys} keys with packet {packet} and key offset {keyframes_offset}"
                )));
            }
            let first = q + 16;
            let keyframes = (0..keys as usize)
                .map(|k| {
                    let at = first + 16 * k;
                    Ok(Keyframe {
                        keyframe_type: b.u32(at)?,
                        key: b.f32(at + 4)?,
                        constants: [b.f32(at + 8)?, b.f32(at + 12)?],
                    })
                })
                .collect::<Result<_>>()?;
            let curve = FCurve {
                minimum: b.f32(q)?,
                maximum: b.f32(q + 4)?,
                keyframes_offset,
                keyframes,
            };
            q = first + 16 * keys as usize;
            Some(curve)
        } else {
            None
        };
        operators.push(Operator {
            operator_type,
            packet,
            fcurve,
        });
        if operator_type == Operator::END {
            break;
        }
        if operator_type > Operator::FCURVE || q - p > usize::from(stated) {
            return Err(wrong(format!("operator {operator_type} at {:#X}", q - 8)));
        }
    }
    if q - p != usize::from(stated) {
        return Err(wrong(format!(
            "it states {stated} bytes and holds {}",
            q - p
        )));
    }
    let operation = Operation {
        node_index: b.u16(p)?,
        type_index: b.u16(p + 2)?,
        size: stated,
        index: b.u16(p + 6)?,
        operators,
    };
    Ok((operation, q - p))
}

fn read_transitions(b: &Body, at: usize, count: u8) -> Result<(Vec<MoveTransition>, usize)> {
    let mut p = at;
    let mut out = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let size = b.u32(p + 0x10)?;
        if size < TRANSITION_MIN || p as u64 + u64::from(size) > b.len() as u64 {
            return Err(refuse(format!(
                "the transition at {p:#X} states {size} bytes"
            )));
        }
        let flags = b.u32(p)?;
        if flags & (1 << 9) != 0 {
            return Err(refuse(format!(
                "the transition at {p:#X} has bit 9 of its flags set"
            )));
        }
        let end = p + size as usize;
        let from = p + TRANSITION_MIN as usize;
        let mut words = [0; TRANSITION_WORDS];
        for (k, word) in words.iter_mut().enumerate() {
            *word = b.u32(p + 4 * k)?;
        }
        out.push(MoveTransition {
            at: p,
            size,
            target: b.rel(p + 0x48)?,
            words,
            conditions: conditions(b, from, (flags >> 6) & 0xF, end)?,
        });
        p = end;
    }
    Ok((out, p))
}

/// The `count` conditions from `at`, which must end at `end`, each held to the measured rules.
fn conditions(b: &Body, at: usize, count: u32, end: usize) -> Result<Vec<MoveCondition>> {
    let mut p = at;
    let mut out = Vec::new();
    for _ in 0..count {
        let (kind, flag) = (b.u16(p)?, b.u16(p + 2)?);
        let words = match kind {
            0 | 1 => 4,
            2..=4 | 9 | 10 => 1,
            5..=8 | 12..=14 => 2,
            _ => {
                return Err(refuse(format!(
                    "the condition at {p:#X} is of kind {kind}, which no rule reads"
                )))
            }
        };
        if flag > 1 || (flag == 1 && !(2..=4).contains(&kind)) {
            return Err(refuse(format!(
                "the condition at {p:#X} of kind {kind} has flag {flag}"
            )));
        }
        let words = (0..words)
            .map(|k| b.u32(p + 4 + 4 * k))
            .collect::<Result<Vec<_>>>()?;
        let broken = match (kind, words.as_slice()) {
            (0 | 1, [_, upper, lower, halves]) => {
                f32::from_bits(*upper) < f32::from_bits(*lower) || halves & !0x1_0001 != 0
            }
            (12, [_, value]) => *value > 1,
            _ => false,
        };
        if broken {
            return Err(refuse(format!(
                "the condition at {p:#X} of kind {kind} holds {words:X?}, which no measured one holds"
            )));
        }
        p += 4 + 4 * words.len();
        out.push(MoveCondition { kind, flag, words });
    }
    if p != end {
        return Err(refuse(format!(
            "the conditions from {at:#X} end at {p:#X}, not at {end:#X}"
        )));
    }
    Ok(out)
}

/// A leaf node's size and inputs, or why no size rule reads it.
type Leaf = std::result::Result<(usize, Vec<usize>), StopReason>;

/// A leaf node by its kind's size rule.
fn leaf(b: &Body, n: usize, kind: u16) -> Result<Leaf> {
    if matches!(kind, 18 | 23) {
        return Ok(Ok((8, Vec::new())));
    }
    let f = b.u32(n + 8)?;
    let sel = |shift: u32| (f >> shift) & 3;
    let word = |s: u32| if s == 0 { 0 } else { 4 };
    // A value is two hashes, a signal one; selector 3 holds neither.
    let pair = |s: u32| match s {
        0 => Ok(0),
        1 => Ok(8),
        2 => Ok(4),
        _ => Err(refuse(format!(
            "the node at {n:#X} has a pair block with selector 3"
        ))),
    };
    let ins = |fields: &[usize]| {
        fields
            .iter()
            .map(|at| b.rel(n + at))
            .collect::<Result<Vec<_>>>()
    };
    let words = |from: u32, count: u32| (from..count).map(|k| word(sel(2 * k))).sum::<usize>();
    let sized = match kind {
        TAIL | 16 => (12, Vec::new()),
        4 | 5 => (0x1C + word(sel(0)) + pair(sel(2))? + 8, ins(&[0x10, 0x14])?),
        19 => (0x18 + pair(sel(0))? + 8, ins(&[0x0C, 0x10])?),
        6 | 39 => (0x10 + pair(sel(0))?, ins(&[0x0C])?),
        8 => (12 + word(sel(0)) + word(sel(4)), Vec::new()),
        15 => (16 + word(sel(0)) + word(sel(4)), ins(&[0x0C])?),
        11 => {
            let block = match sel(0) {
                0 => 0,
                1 => {
                    if b.u32(n + 12)? == 3 {
                        return Ok(Err(StopReason::ClipContext));
                    }
                    12
                }
                2 => 4,
                _ => return Err(refuse(format!("the clip at {n:#X} has selector 3"))),
            };
            (12 + block + words(1, 10), Vec::new())
        }
        10 | 17 | 21 => {
            if sel(0) != 0 {
                return Err(refuse(format!(
                    "the node at {n:#X} of kind {kind} has a filter block"
                )));
            }
            let count = (f >> 25) as usize;
            let mut size = 16 + pair(sel(2))? + 8;
            let inputs = (0..count)
                .map(|i| b.rel(n + size + 4 * i))
                .collect::<Result<Vec<_>>>()?;
            size += 4 * count;
            let child_words = n + size;
            size += 4 * (count / 4 + 1);
            for i in 0..count {
                let w = u32::from(b.u8(child_words + i)?);
                size += word(w & 3) + pair((w >> 4) & 3)?;
            }
            (size, inputs)
        }
        20 => (12 + word(sel(0)), Vec::new()),
        25 => (16 + pair(sel(0))?, Vec::new()),
        27 | 40 => {
            let count = b.u32(n + 0x10)?;
            b.after(n + 0x14, count, 4)?;
            let inputs = (0..count as usize)
                .map(|i| b.rel(n + 0x14 + 4 * i))
                .collect::<Result<Vec<_>>>()?;
            (20 + 4 * count as usize + pair(sel(0))?, inputs)
        }
        31 | 33 => (16, ins(&[0x0C])?),
        43 => (36, ins(&[0x0C])?),
        32 if f >> 16 != 0 || !matches!((f >> 8) & 0xFF, 0 | 2) => {
            return Ok(Err(StopReason::WideCount))
        }
        // Byte 9 set to 2 adds one word after the pairs.
        32 => (
            20 + 8 * (f & 0xFF) as usize + if f >> 8 == 2 { 4 } else { 0 },
            ins(&[0x10])?,
        ),
        34 => (16 + words(0, 16), ins(&[0x0C])?),
        36 => (16 + word(sel(0)) + word(sel(2)), ins(&[0x0C])?),
        37 => (
            16 + word(sel(0)) + word(sel(2)) + pair(sel(4))?,
            ins(&[0x0C])?,
        ),
        41 => (12 + words(0, 16), Vec::new()),
        9 | 12 | 28 => return Ok(Err(StopReason::Keyed)),
        13 if f & !3 == 0x14 && matches!(f & 3, 1 | 2) => (28, ins(&[0x0C])?),
        // The word at +0x08 is the offset to its input.
        30 if f == 0x14 && b.bytes::<16>(n + 0x0C)? == [0; 16] => (28, ins(&[0x08])?),
        35 if f == 0 => (40, ins(&[0x0C])?),
        13 | 30 | 35 => return Ok(Err(StopReason::Unmeasured)),
        _ => {
            return Err(refuse(format!(
                "the node at {n:#X} is of kind {kind}, which no rule reads"
            )))
        }
    };
    Ok(Ok(sized))
}

/// A body whose every read is bounded by its length.
struct Body<'a>(&'a [u8]);

impl Body<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn bytes<const N: usize>(&self, at: usize) -> Result<[u8; N]> {
        at.checked_add(N)
            .and_then(|end| self.0.get(at..end))
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| refuse(format!("a read at {at:#X} runs past the body")))
    }

    fn u8(&self, at: usize) -> Result<u8> {
        Ok(self.bytes::<1>(at)?[0])
    }

    fn u16(&self, at: usize) -> Result<u16> {
        self.bytes(at).map(u16::from_le_bytes)
    }

    fn u32(&self, at: usize) -> Result<u32> {
        self.bytes(at).map(u32::from_le_bytes)
    }

    fn i32(&self, at: usize) -> Result<i32> {
        self.bytes(at).map(i32::from_le_bytes)
    }

    fn f32(&self, at: usize) -> Result<f32> {
        self.bytes(at).map(f32::from_le_bytes)
    }

    /// Where the offset at `at`, relative to itself, leads.
    fn rel(&self, at: usize) -> Result<usize> {
        let to = at as i64 + i64::from(self.i32(at)?);
        usize::try_from(to)
            .ok()
            .filter(|&to| to <= self.len())
            .ok_or_else(|| refuse(format!("the offset at {at:#X} leaves the body")))
    }

    /// The end of `count` items of `size` bytes from `at`, refused past the body.
    fn after(&self, at: usize, count: u32, size: u64) -> Result<usize> {
        let end = at as u64 + u64::from(count) * size;
        if end > self.len() as u64 {
            return Err(refuse(format!(
                "{count} items of {size} bytes from {at:#X} run past the body"
            )));
        }
        Ok(end as usize)
    }
}

fn refuse(why: String) -> IoError {
    IoError::InvalidData {
        context: format!("{WHAT}: {why}"),
    }
}

#[cfg(test)]
#[path = "move_network_tests.rs"]
mod tests;
