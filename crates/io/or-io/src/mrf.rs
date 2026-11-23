//! `.mrf`: a move network definition (`mvNetworkDef`), the graph of state
//! machines, states, transitions and animation nodes MoVE runs.
//!
//! The state kinds are read field by field; every other node keeps the bytes
//! after its id, measured by its flags. Offsets between nodes stay as stored,
//! relative to the field holding them, so an unedited network saves
//! byte-identical.

use crate::error::{IoError, Result};

const MAGIC: [u8; 4] = *b"MoVE";
const HEADER_LEN: usize = 32;
/// The MoVE version every GTA V network is written at.
pub const VERSION: [u32; 4] = [2, 0, 0, 0];

const FLAG_IGNORED: u32 = 0;
const FLAG_VALUE: u32 = 1;
const FLAG_SIGNAL: u32 = 2;
const SYNCHRONIZER_TAG: u32 = 1;
const CLIP_CONTEXT_LOCAL_FILE: u32 = 3;
/// Floats a literal `FilterN` block holds.
const FILTER_N_VALUES: usize = 19;

/// A whole `.mrf`: its header, cross-references, requests, flags, nodes and string table.
#[derive(Debug, Clone, PartialEq)]
pub struct MvNetworkDef {
    /// Major, minor, patch and revision.
    pub version: [u32; 4],
    /// Paths of networks this one references, each as stored.
    pub xrefs: Vec<Vec<u8>>,
    pub requests: Vec<KeyIndex>,
    pub flags: Vec<KeyIndex>,
    /// Every node in file order; the first is the root state.
    pub nodes: Vec<MvNodeDef>,
    pub string_table: Vec<u8>,
}

/// A `request_t` or `flag_t`: a name hash and its bit in the network's bit set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyIndex {
    pub key: u32,
    pub index: u32,
}

/// One node definition (`mvNodeDef`).
#[derive(Debug, Clone, PartialEq)]
pub struct MvNodeDef {
    pub node_type: NodeType,
    /// Its index in the owning state's children.
    pub index: u16,
    /// The hash of the node's name.
    pub id: u32,
    pub body: NodeBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeBody {
    StateMachine(StateMachineDef),
    InlinedStateMachine(InlinedStateMachineDef),
    State(StateDef),
    /// Every byte after `m_id`, as stored.
    Other(Vec<u8>),
}

macro_rules! node_types {
    ($($name:ident = $value:literal,)*) => {
        /// `mvNode`'s node kinds.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum NodeType {
            $($name = $value,)*
        }

        impl NodeType {
            pub const ALL: &'static [NodeType] = &[$(NodeType::$name,)*];

            pub fn from_u16(value: u16) -> Option<Self> {
                match value {
                    $($value => Some(Self::$name),)*
                    _ => None,
                }
            }

            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$name => stringify!($name),)*
                }
            }
        }
    };
}

node_types! {
    StateMachine = 1,
    Tail = 2,
    InlinedStateMachine = 3,
    Animation = 4,
    Blend = 5,
    AddSubtract = 6,
    Filter = 7,
    Mirror = 8,
    Frame = 9,
    Ik = 10,
    Ragdoll = 11,
    Mover = 12,
    BlendN = 13,
    PonyTail = 14,
    Clip = 15,
    Style = 16,
    Pm = 17,
    Extrapolate = 18,
    Expression = 19,
    Capture = 20,
    Proxy = 21,
    AddN = 22,
    Identity = 23,
    Merge = 24,
    Pose = 25,
    MergeN = 26,
    State = 27,
    Invalid = 28,
    JointLimit = 29,
    SubNetwork = 30,
    Reference = 31,
}

/// The fields every state kind shares (`mvNodeStateBaseDef`), counts aside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StateBaseDef {
    /// Offset from this field to the initial node.
    pub initial: i32,
    pub defer_block_update: u32,
    pub on_enter_enabled: u8,
    pub on_exit_enabled: u8,
    pub on_enter_id: u32,
    pub on_exit_id: u32,
    /// Offset from this field to the first transition.
    pub transitions_offset: i32,
}

/// One entry of a state machine's state table (`StateNode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateNode {
    pub id: u32,
    /// Offset from this field to the state's node.
    pub node: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateMachineDef {
    pub base: StateBaseDef,
    pub states: Vec<StateNode>,
    pub transitions: Vec<MvTransition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InlinedStateMachineDef {
    pub base: StateBaseDef,
    /// Offset from this field to the input node.
    pub input: i32,
    pub states: Vec<StateNode>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateDef {
    pub base: StateBaseDef,
    /// Children, not counting tails.
    pub child_count: u8,
    pub param_inputs_offset: i32,
    pub events_offset: i32,
    pub param_outputs_offset: i32,
    pub operations_offset: i32,
    pub transitions: Vec<MvTransition>,
    pub param_inputs: Vec<Parameter>,
    pub events: Vec<Event>,
    pub param_outputs: Vec<Parameter>,
    pub operations: Vec<Operation>,
}

/// A parameter hookup between the parameter buffer and a child node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parameter {
    pub parameter_crc: u32,
    pub node_index: u16,
    pub type_index: u16,
    pub parameter_index: u32,
}

/// An event hookup from a child node into the parameter buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub node_index: u16,
    pub type_index: u16,
    pub event_crc: u32,
}

/// A stack program whose result sets a child node's parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    pub node_index: u16,
    pub type_index: u16,
    pub size: u16,
    pub index: u16,
    /// Ends with an `END` operator.
    pub operators: Vec<Operator>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Operator {
    pub operator_type: u32,
    /// A parameter hash, a float's bits or, for an `FCURVE`, an offset to its curve.
    pub packet: u32,
    pub fcurve: Option<FCurve>,
}

impl Operator {
    pub const END: u32 = 0;
    pub const PUSH_VALUE: u32 = 1;
    pub const FIND_AND_PUSH_VALUE: u32 = 2;
    pub const ADD: u32 = 3;
    pub const MULTIPLY: u32 = 4;
    pub const FCURVE: u32 = 5;
}

#[derive(Debug, Clone, PartialEq)]
pub struct FCurve {
    pub minimum: f32,
    pub maximum: f32,
    /// Offset from this field to the first keyframe.
    pub keyframes_offset: i32,
    pub keyframes: Vec<Keyframe>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keyframe {
    pub keyframe_type: u32,
    pub key: f32,
    pub constants: [f32; 2],
}

/// One transition out of a state (`mvTransition`), with its conditions.
#[derive(Debug, Clone, PartialEq)]
pub struct MvTransition {
    /// As stored; a save rewrites the size, condition count and filter bits from the content.
    pub flags: u32,
    pub tag: u32,
    pub duration: f32,
    pub signal: u32,
    pub transition_weight_signal: u32,
    /// Offset from this field to the target state's node.
    pub target: i32,
    /// All must hold for the transition to fire.
    pub conditions: Vec<MvCondition>,
    /// Frame filter dictionary and filter hashes.
    pub filter: Option<[u32; 2]>,
}

const TRANSITION_SIZE_SHIFT: u32 = 4;
const TRANSITION_SIZE_MASK: u32 = 0x3fff << TRANSITION_SIZE_SHIFT;
const TRANSITION_CONDITIONS_SHIFT: u32 = 20;
const TRANSITION_CONDITIONS_MASK: u32 = 0xf << TRANSITION_CONDITIONS_SHIFT;
const TRANSITION_FILTER: u32 = 1 << 30;
const TRANSITION_FIXED_LEN: usize = 24;

impl MvTransition {
    /// Duration comes from the parameter named by `signal`.
    pub fn duration_from_signal(&self) -> bool {
        self.flags & (1 << 3) != 0
    }

    /// `EaseInOut`, `EaseOut`, `EaseIn`, `Linear` or `Step`.
    pub fn modifier(&self) -> &'static str {
        match (self.flags >> 24) & 7 {
            0 => "EaseInOut",
            1 => "EaseOut",
            2 => "EaseIn",
            3 => "Linear",
            4 => "Step",
            _ => "?",
        }
    }

    fn len(&self) -> usize {
        TRANSITION_FIXED_LEN
            + self.conditions.iter().map(MvCondition::len).sum::<usize>()
            + if self.filter.is_some() { 8 } else { 0 }
    }
}

/// One condition on a transition (`mvCondition`); `set` lets a passed condition stay passed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MvCondition {
    pub set: u16,
    pub test: Test,
}

/// What a condition tests, by `kCondition*` kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Test {
    InRange { signal: u32, upper: f32, lower: f32 },
    OutOfRange { signal: u32, upper: f32, lower: f32 },
    OnRequest { request: u32, not_set: u32 },
    OnFlag { flag: u32, not_set: u32 },
    AtEvent { event: u32, not_set: u32 },
    GreaterThan { signal: u32, value: f32 },
    GreaterThanEqual { signal: u32, value: f32 },
    LessThan { signal: u32, value: f32 },
    LessThanEqual { signal: u32, value: f32 },
    LifetimeGreaterThan { lifetime: f32 },
    LifetimeLessThan { lifetime: f32 },
    OnMoveEventTag { tag: u32, not_set: u32 },
    BoolEquals { signal: u32, value: u32 },
}

impl MvCondition {
    fn len(&self) -> usize {
        match self.test {
            Test::InRange { .. } | Test::OutOfRange { .. } => 16,
            Test::LifetimeGreaterThan { .. } | Test::LifetimeLessThan { .. } => 8,
            _ => 12,
        }
    }

    fn read(r: &mut Reader) -> Result<Self> {
        let kind = r.u16()?;
        let set = r.u16()?;
        let test = match kind {
            0 | 1 => {
                let (signal, upper, lower) = (r.u32()?, r.f32()?, r.f32()?);
                if kind == 0 {
                    Test::InRange {
                        signal,
                        upper,
                        lower,
                    }
                } else {
                    Test::OutOfRange {
                        signal,
                        upper,
                        lower,
                    }
                }
            }
            2 => Test::OnRequest {
                request: r.u32()?,
                not_set: r.u32()?,
            },
            3 => Test::OnFlag {
                flag: r.u32()?,
                not_set: r.u32()?,
            },
            4 => Test::AtEvent {
                event: r.u32()?,
                not_set: r.u32()?,
            },
            5 => Test::GreaterThan {
                signal: r.u32()?,
                value: r.f32()?,
            },
            6 => Test::GreaterThanEqual {
                signal: r.u32()?,
                value: r.f32()?,
            },
            7 => Test::LessThan {
                signal: r.u32()?,
                value: r.f32()?,
            },
            8 => Test::LessThanEqual {
                signal: r.u32()?,
                value: r.f32()?,
            },
            9 => Test::LifetimeGreaterThan { lifetime: r.f32()? },
            10 => Test::LifetimeLessThan { lifetime: r.f32()? },
            11 => Test::OnMoveEventTag {
                tag: r.u32()?,
                not_set: r.u32()?,
            },
            12 => Test::BoolEquals {
                signal: r.u32()?,
                value: r.u32()?,
            },
            other => return Err(refuse(format!("condition kind {other} at {}", r.pos - 4))),
        };
        Ok(Self { set, test })
    }

    fn write(&self, out: &mut Vec<u8>) {
        let (kind, words): (u16, [u32; 3]) = match self.test {
            Test::InRange {
                signal,
                upper,
                lower,
            } => (0, [signal, upper.to_bits(), lower.to_bits()]),
            Test::OutOfRange {
                signal,
                upper,
                lower,
            } => (1, [signal, upper.to_bits(), lower.to_bits()]),
            Test::OnRequest { request, not_set } => (2, [request, not_set, 0]),
            Test::OnFlag { flag, not_set } => (3, [flag, not_set, 0]),
            Test::AtEvent { event, not_set } => (4, [event, not_set, 0]),
            Test::GreaterThan { signal, value } => (5, [signal, value.to_bits(), 0]),
            Test::GreaterThanEqual { signal, value } => (6, [signal, value.to_bits(), 0]),
            Test::LessThan { signal, value } => (7, [signal, value.to_bits(), 0]),
            Test::LessThanEqual { signal, value } => (8, [signal, value.to_bits(), 0]),
            Test::LifetimeGreaterThan { lifetime } => (9, [lifetime.to_bits(), 0, 0]),
            Test::LifetimeLessThan { lifetime } => (10, [lifetime.to_bits(), 0, 0]),
            Test::OnMoveEventTag { tag, not_set } => (11, [tag, not_set, 0]),
            Test::BoolEquals { signal, value } => (12, [signal, value, 0]),
        };
        put16(out, kind);
        put16(out, self.set);
        for word in &words[..(self.len() - 4) / 4] {
            put32(out, *word);
        }
    }
}

impl MvNetworkDef {
    /// Reads a whole `.mrf`, refusing any count, offset or length the bytes do not bear out.
    pub fn open(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader { bytes, pos: 0 };
        if r.take(4)? != MAGIC {
            return Err(refuse("the magic is not `MoVE`".into()));
        }
        let version = [r.u32()?, r.u32()?, r.u32()?, r.u32()?];
        if version != VERSION {
            return Err(refuse(format!(
                "version {version:?}; only MoVE {VERSION:?} is read"
            )));
        }
        let definition_len = r.u32()? as usize;
        let string_table_len = r.u32()? as usize;
        let xref_count = r.u32()?;
        let xrefs = (0..xref_count)
            .map(|_| {
                let len = r.u32()? as usize;
                Ok(r.take(len)?.to_vec())
            })
            .collect::<Result<Vec<_>>>()?;
        let definition_end = r.pos + definition_len;
        let key_indices = |r: &mut Reader| -> Result<Vec<KeyIndex>> {
            let count = r.u32()?;
            (0..count)
                .map(|_| {
                    Ok(KeyIndex {
                        key: r.u32()?,
                        index: r.u32()?,
                    })
                })
                .collect()
        };
        let requests = key_indices(&mut r)?;
        let flags = key_indices(&mut r)?;
        let mut nodes = Vec::new();
        while r.pos < definition_end {
            nodes.push(read_node(&mut r)?);
        }
        if r.pos != definition_end {
            return Err(refuse(format!(
                "the nodes end at {}, past the definition's end at {definition_end}",
                r.pos
            )));
        }
        let string_table = r.take(string_table_len)?.to_vec();
        if r.pos != bytes.len() {
            return Err(refuse(format!(
                "{} bytes follow the string table",
                bytes.len() - r.pos
            )));
        }
        Ok(Self {
            version,
            xrefs,
            requests,
            flags,
            nodes,
            string_table,
        })
    }

    /// The canonical bytes, which for an unedited network are the bytes it was read from.
    pub fn save(&self) -> Result<Vec<u8>> {
        let mut definition = Vec::new();
        for list in [&self.requests, &self.flags] {
            put32(&mut definition, count(list.len(), "keys")?);
            for k in list {
                put32(&mut definition, k.key);
                put32(&mut definition, k.index);
            }
        }
        for node in &self.nodes {
            write_node(node, &mut definition)?;
        }
        let mut out = Vec::with_capacity(HEADER_LEN + definition.len() + self.string_table.len());
        out.extend_from_slice(&MAGIC);
        for v in self.version {
            put32(&mut out, v);
        }
        put32(&mut out, count(definition.len(), "definition bytes")?);
        put32(
            &mut out,
            count(self.string_table.len(), "string table bytes")?,
        );
        put32(&mut out, count(self.xrefs.len(), "cross-references")?);
        for xref in &self.xrefs {
            put32(&mut out, count(xref.len(), "cross-reference bytes")?);
            out.extend_from_slice(xref);
        }
        out.extend_from_slice(&definition);
        out.extend_from_slice(&self.string_table);
        Ok(out)
    }

    /// Each node's offset in the saved file.
    pub fn node_offsets(&self) -> Vec<usize> {
        let mut at = HEADER_LEN
            + self.xrefs.iter().map(|x| 4 + x.len()).sum::<usize>()
            + 8
            + 8 * (self.requests.len() + self.flags.len());
        self.nodes
            .iter()
            .map(|node| {
                let start = at;
                let mut scratch = Vec::new();
                // A node that cannot save has no offset after it; the save refuses it.
                let _ = write_node(node, &mut scratch);
                at += scratch.len();
                start
            })
            .collect()
    }

    /// Every node's offsets resolved to node indices, in node order.
    pub fn edges(&self) -> Vec<Edges> {
        let offsets = self.node_offsets();
        let node_at = |field: usize, relative: i32| {
            let target = field.checked_add_signed(relative as isize)?;
            offsets.binary_search(&target).ok()
        };
        let states = |table: usize, states: &[StateNode]| {
            states
                .iter()
                .enumerate()
                .map(|(i, s)| node_at(table + 8 * i + 4, s.node))
                .collect()
        };
        let transitions = |mut at: usize, transitions: &[MvTransition]| {
            transitions
                .iter()
                .map(|t| {
                    let target = node_at(at + 0x14, t.target);
                    at += t.len();
                    target
                })
                .collect()
        };
        self.nodes
            .iter()
            .zip(&offsets)
            .map(|(node, &at)| {
                let initial = state_base(&node.body).and_then(|b| node_at(at + 8, b.initial));
                match &node.body {
                    NodeBody::StateMachine(sm) => Edges {
                        initial,
                        states: states(at + 0x20, &sm.states),
                        transitions: transitions(at + 0x20 + 8 * sm.states.len(), &sm.transitions),
                    },
                    NodeBody::InlinedStateMachine(ism) => Edges {
                        initial,
                        states: states(at + 0x24, &ism.states),
                        transitions: Vec::new(),
                    },
                    NodeBody::State(s) => Edges {
                        initial,
                        states: Vec::new(),
                        transitions: transitions(at + 0x40, &s.transitions),
                    },
                    NodeBody::Other(_) => Edges::default(),
                }
            })
            .collect()
    }

    /// Each node kind present and how many nodes are of it, in kind order.
    pub fn node_counts(&self) -> Vec<(NodeType, usize)> {
        NodeType::ALL
            .iter()
            .map(|&kind| {
                (
                    kind,
                    self.nodes.iter().filter(|n| n.node_type == kind).count(),
                )
            })
            .filter(|&(_, n)| n > 0)
            .collect()
    }
}

/// Where a node's offsets lead, as node indices; `None` where no node starts at the offset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edges {
    /// A state kind's initial node.
    pub initial: Option<usize>,
    /// A state machine's states, one per [`StateNode`].
    pub states: Vec<Option<usize>>,
    /// Each transition's target state.
    pub transitions: Vec<Option<usize>>,
}

/// A state kind's shared fields.
pub fn state_base(body: &NodeBody) -> Option<&StateBaseDef> {
    match body {
        NodeBody::StateMachine(sm) => Some(&sm.base),
        NodeBody::InlinedStateMachine(ism) => Some(&ism.base),
        NodeBody::State(s) => Some(&s.base),
        NodeBody::Other(_) => None,
    }
}

fn read_node(r: &mut Reader) -> Result<MvNodeDef> {
    let start = r.pos;
    let raw = r.u16()?;
    let node_type =
        NodeType::from_u16(raw).ok_or_else(|| refuse(format!("node kind {raw} at {start}")))?;
    let index = r.u16()?;
    let id = r.u32()?;
    let body = match node_type {
        NodeType::StateMachine => {
            let (base, child_count, transition_count) = read_state_base(r)?;
            let states = read_states(r, child_count)?;
            let transitions = read_transitions(r, start, &base, transition_count)?;
            NodeBody::StateMachine(StateMachineDef {
                base,
                states,
                transitions,
            })
        }
        NodeType::InlinedStateMachine => {
            let (base, child_count, transition_count) = read_state_base(r)?;
            if transition_count != 0 {
                return Err(refuse(format!(
                    "inlined state machine at {start} states {transition_count} transitions"
                )));
            }
            let input = r.i32()?;
            let states = read_states(r, child_count)?;
            NodeBody::InlinedStateMachine(InlinedStateMachineDef {
                base,
                input,
                states,
            })
        }
        NodeType::State => {
            let (base, child_count, transition_count) = read_state_base(r)?;
            let mut pairs = [(0i32, 0u32); 4];
            for pair in &mut pairs {
                *pair = (r.i32()?, r.u32()?);
            }
            let transitions = read_transitions(r, start, &base, transition_count)?;
            let at = |r: &mut Reader, i: usize| -> Result<u32> {
                let (offset, n) = pairs[i];
                expect_at(r, start + 0x20 + 8 * i, offset, n)?;
                Ok(n)
            };
            let n = at(r, 0)?;
            let param_inputs = (0..n).map(|_| read_parameter(r)).collect::<Result<_>>()?;
            let n = at(r, 1)?;
            let events = (0..n)
                .map(|_| {
                    Ok(Event {
                        node_index: r.u16()?,
                        type_index: r.u16()?,
                        event_crc: r.u32()?,
                    })
                })
                .collect::<Result<_>>()?;
            let n = at(r, 2)?;
            let param_outputs = (0..n).map(|_| read_parameter(r)).collect::<Result<_>>()?;
            let n = at(r, 3)?;
            let operations = (0..n).map(|_| read_operation(r)).collect::<Result<_>>()?;
            NodeBody::State(StateDef {
                base,
                child_count,
                param_inputs_offset: pairs[0].0,
                events_offset: pairs[1].0,
                param_outputs_offset: pairs[2].0,
                operations_offset: pairs[3].0,
                transitions,
                param_inputs,
                events,
                param_outputs,
                operations,
            })
        }
        other => {
            let body_start = r.pos;
            skip_body(r, other)?;
            NodeBody::Other(r.bytes[body_start..r.pos].to_vec())
        }
    };
    Ok(MvNodeDef {
        node_type,
        index,
        id,
        body,
    })
}

fn read_state_base(r: &mut Reader) -> Result<(StateBaseDef, u8, u8)> {
    let initial = r.i32()?;
    let defer_block_update = r.u32()?;
    let on_enter_enabled = r.u8()?;
    let on_exit_enabled = r.u8()?;
    let child_count = r.u8()?;
    let transition_count = r.u8()?;
    let on_enter_id = r.u32()?;
    let on_exit_id = r.u32()?;
    let transitions_offset = r.i32()?;
    let base = StateBaseDef {
        initial,
        defer_block_update,
        on_enter_enabled,
        on_exit_enabled,
        on_enter_id,
        on_exit_id,
        transitions_offset,
    };
    Ok((base, child_count, transition_count))
}

fn read_states(r: &mut Reader, count: u8) -> Result<Vec<StateNode>> {
    (0..count)
        .map(|_| {
            Ok(StateNode {
                id: r.u32()?,
                node: r.i32()?,
            })
        })
        .collect()
}

fn read_transitions(
    r: &mut Reader,
    node: usize,
    base: &StateBaseDef,
    count: u8,
) -> Result<Vec<MvTransition>> {
    expect_at(r, node + 0x1c, base.transitions_offset, u32::from(count))?;
    (0..count).map(|_| read_transition(r)).collect()
}

/// `n` entries must sit where the offset stored at `field` says, since a save
/// writes them there; an empty array's offset is kept as stored.
fn expect_at(r: &Reader, field: usize, offset: i32, n: u32) -> Result<()> {
    let stated = field.checked_add_signed(offset as isize);
    if n > 0 && stated != Some(r.pos) {
        return Err(refuse(format!(
            "{n} entries are stated at {stated:?} and sit at {}",
            r.pos
        )));
    }
    Ok(())
}

fn read_transition(r: &mut Reader) -> Result<MvTransition> {
    let start = r.pos;
    let flags = r.u32()?;
    let tag = r.u32()?;
    let duration = r.f32()?;
    let signal = r.u32()?;
    let transition_weight_signal = r.u32()?;
    let target = r.i32()?;
    let conditions = (0..(flags & TRANSITION_CONDITIONS_MASK) >> TRANSITION_CONDITIONS_SHIFT)
        .map(|_| MvCondition::read(r))
        .collect::<Result<_>>()?;
    let filter = if flags & TRANSITION_FILTER != 0 {
        Some([r.u32()?, r.u32()?])
    } else {
        None
    };
    let stated = ((flags & TRANSITION_SIZE_MASK) >> TRANSITION_SIZE_SHIFT) as usize;
    if stated != r.pos - start {
        return Err(refuse(format!(
            "transition at {start} states {stated} bytes and holds {}",
            r.pos - start
        )));
    }
    Ok(MvTransition {
        flags,
        tag,
        duration,
        signal,
        transition_weight_signal,
        target,
        conditions,
        filter,
    })
}

fn read_parameter(r: &mut Reader) -> Result<Parameter> {
    Ok(Parameter {
        parameter_crc: r.u32()?,
        node_index: r.u16()?,
        type_index: r.u16()?,
        parameter_index: r.u32()?,
    })
}

fn read_operation(r: &mut Reader) -> Result<Operation> {
    let node_index = r.u16()?;
    let type_index = r.u16()?;
    let size = r.u16()?;
    let index = r.u16()?;
    let mut operators = Vec::new();
    loop {
        let operator_type = r.u32()?;
        let packet = r.u32()?;
        let fcurve = if operator_type == Operator::FCURVE {
            let minimum = r.f32()?;
            let maximum = r.f32()?;
            let keyframe_count = r.u32()?;
            let keyframes_offset = r.i32()?;
            let keyframes = (0..keyframe_count)
                .map(|_| {
                    Ok(Keyframe {
                        keyframe_type: r.u32()?,
                        key: r.f32()?,
                        constants: [r.f32()?, r.f32()?],
                    })
                })
                .collect::<Result<_>>()?;
            Some(FCurve {
                minimum,
                maximum,
                keyframes_offset,
                keyframes,
            })
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
    }
    Ok(Operation {
        node_index,
        type_index,
        size,
        index,
        operators,
    })
}

/// Steps over the fields of a node kind kept as bytes, as its flags lay them out.
fn skip_body(r: &mut Reader, kind: NodeType) -> Result<()> {
    use NodeType::*;
    let two = |flags: u32, shift: u32| (flags >> shift) & 3;
    // A real, boolean or frame block: present unless ignored.
    let word = |r: &mut Reader, flag: u32| r.skip(if flag == FLAG_IGNORED { 0 } else { 4 });
    // A filter or expression block: a parameter hash, or a dictionary and name.
    let pair = |r: &mut Reader, flag: u32| {
        r.skip(match flag {
            FLAG_SIGNAL => 4,
            FLAG_VALUE => 8,
            _ => 0,
        })
    };
    // An animation or parameterized motion block: a parameter hash, or a length and that many bytes.
    let blob = |r: &mut Reader, flag: u32| -> Result<()> {
        match flag {
            FLAG_SIGNAL => r.skip(4),
            FLAG_VALUE => {
                let len = r.u32()? as usize;
                r.skip(len)
            }
            _ => Ok(()),
        }
    };
    match kind {
        Tail | Identity | Invalid | Ik => {}
        Proxy | SubNetwork => r.skip(4)?,
        Blend | AddSubtract => {
            let flags = r.u32()?;
            r.skip(8)?;
            if two(flags, 19) == SYNCHRONIZER_TAG {
                r.skip(4)?;
            }
            word(r, two(flags, 0))?;
            pair(r, two(flags, 2))?;
        }
        Merge => {
            let flags = r.u32()?;
            r.skip(8)?;
            if two(flags, 19) == SYNCHRONIZER_TAG {
                r.skip(4)?;
            }
            pair(r, two(flags, 0))?;
        }
        Filter | Mirror | JointLimit => {
            let flags = r.u32()?;
            r.skip(4)?;
            pair(r, two(flags, 0))?;
        }
        Frame | Capture => {
            let flags = r.u32()?;
            if kind == Capture {
                r.skip(4)?;
            }
            word(r, two(flags, 0))?;
            word(r, two(flags, 4))?;
        }
        Pose => {
            let flags = r.u32()?;
            word(r, two(flags, 0))?;
        }
        Extrapolate => {
            let flags = r.u32()?;
            r.skip(4)?;
            word(r, two(flags, 0))?;
        }
        Expression => {
            let flags = r.u32()?;
            r.skip(4)?;
            pair(r, two(flags, 0))?;
            word(r, two(flags, 2))?;
            for i in 0..(flags >> 28) {
                r.skip(4)?;
                word(r, two(flags, 4 + 2 * i))?;
            }
        }
        Clip => {
            let flags = r.u32()?;
            match two(flags, 0) {
                FLAG_SIGNAL => r.skip(4)?,
                FLAG_VALUE => {
                    let context = r.u32()?;
                    r.skip(if context == CLIP_CONTEXT_LOCAL_FILE {
                        4
                    } else {
                        8
                    })?;
                }
                _ => {}
            }
            for shift in [2, 4, 6, 8] {
                word(r, two(flags, shift))?;
            }
        }
        Animation => {
            let flags = r.u32()?;
            blob(r, two(flags, 0))?;
            for shift in [2, 4, 6, 8, 10] {
                word(r, two(flags, shift))?;
            }
        }
        Pm => {
            let flags = r.u32()?;
            blob(r, two(flags, 0))?;
            for shift in [2, 4, 6] {
                word(r, two(flags, shift))?;
            }
            for i in 0..(flags >> 10) & 0xf {
                word(r, two(flags, 19 + 2 * i))?;
            }
        }
        BlendN | AddN | MergeN => {
            let flags = r.u32()?;
            if two(flags, 19) == SYNCHRONIZER_TAG {
                r.skip(4)?;
            }
            r.skip(match two(flags, 0) {
                FLAG_SIGNAL => 4,
                FLAG_VALUE => 4 * FILTER_N_VALUES,
                _ => 0,
            })?;
            pair(r, two(flags, 2))?;
            let count = (flags >> 26) as usize;
            r.skip(4 * count)?;
            let words = (0..count / 4 + 1)
                .map(|_| r.u32())
                .collect::<Result<Vec<_>>>()?;
            for i in 0..count {
                let child = words[i / 4] >> (8 * (i % 4));
                word(r, child & 3)?;
                pair(r, (child >> 4) & 3)?;
            }
        }
        Reference => {
            r.skip(8)?;
            let source_parameters = r.u32()? as usize;
            let renames = r.u32()? as usize + r.u32()? as usize + r.u32()? as usize;
            r.skip(8 * renames + 12 * source_parameters)?;
        }
        StateMachine | InlinedStateMachine | State | Ragdoll | Mover | PonyTail | Style => {
            return Err(refuse(format!(
                "no layout is known for a {} node",
                kind.name()
            )))
        }
    }
    Ok(())
}

fn write_node(node: &MvNodeDef, out: &mut Vec<u8>) -> Result<()> {
    let typed = match &node.body {
        NodeBody::StateMachine(_) => Some(NodeType::StateMachine),
        NodeBody::InlinedStateMachine(_) => Some(NodeType::InlinedStateMachine),
        NodeBody::State(_) => Some(NodeType::State),
        NodeBody::Other(_) => None,
    };
    let state_kind = matches!(
        node.node_type,
        NodeType::StateMachine | NodeType::InlinedStateMachine | NodeType::State
    );
    if typed.map_or(state_kind, |t| t != node.node_type) {
        return Err(refuse(format!(
            "a {} node holds another kind's body",
            node.node_type.name()
        )));
    }
    put16(out, node.node_type as u16);
    put16(out, node.index);
    put32(out, node.id);
    match &node.body {
        NodeBody::StateMachine(sm) => {
            write_state_base(
                out,
                &sm.base,
                count8(sm.states.len())?,
                count8(sm.transitions.len())?,
            );
            write_states(out, &sm.states);
            write_transitions(out, &sm.transitions)?;
        }
        NodeBody::InlinedStateMachine(ism) => {
            write_state_base(out, &ism.base, count8(ism.states.len())?, 0);
            put32(out, ism.input as u32);
            write_states(out, &ism.states);
        }
        NodeBody::State(s) => {
            write_state_base(out, &s.base, s.child_count, count8(s.transitions.len())?);
            for (offset, n) in [
                (s.param_inputs_offset, s.param_inputs.len()),
                (s.events_offset, s.events.len()),
                (s.param_outputs_offset, s.param_outputs.len()),
                (s.operations_offset, s.operations.len()),
            ] {
                put32(out, offset as u32);
                put32(out, count(n, "state entries")?);
            }
            write_transitions(out, &s.transitions)?;
            for p in &s.param_inputs {
                write_parameter(out, p);
            }
            for e in &s.events {
                put16(out, e.node_index);
                put16(out, e.type_index);
                put32(out, e.event_crc);
            }
            for p in &s.param_outputs {
                write_parameter(out, p);
            }
            for op in &s.operations {
                put16(out, op.node_index);
                put16(out, op.type_index);
                put16(out, op.size);
                put16(out, op.index);
                for o in &op.operators {
                    put32(out, o.operator_type);
                    put32(out, o.packet);
                    if let Some(c) = &o.fcurve {
                        put32(out, c.minimum.to_bits());
                        put32(out, c.maximum.to_bits());
                        put32(out, count(c.keyframes.len(), "keyframes")?);
                        put32(out, c.keyframes_offset as u32);
                        for k in &c.keyframes {
                            put32(out, k.keyframe_type);
                            put32(out, k.key.to_bits());
                            put32(out, k.constants[0].to_bits());
                            put32(out, k.constants[1].to_bits());
                        }
                    }
                }
            }
        }
        NodeBody::Other(bytes) => out.extend_from_slice(bytes),
    }
    Ok(())
}

fn write_state_base(out: &mut Vec<u8>, base: &StateBaseDef, child_count: u8, transition_count: u8) {
    put32(out, base.initial as u32);
    put32(out, base.defer_block_update);
    out.extend_from_slice(&[
        base.on_enter_enabled,
        base.on_exit_enabled,
        child_count,
        transition_count,
    ]);
    put32(out, base.on_enter_id);
    put32(out, base.on_exit_id);
    put32(out, base.transitions_offset as u32);
}

fn write_states(out: &mut Vec<u8>, states: &[StateNode]) {
    for s in states {
        put32(out, s.id);
        put32(out, s.node as u32);
    }
}

fn write_transitions(out: &mut Vec<u8>, transitions: &[MvTransition]) -> Result<()> {
    for t in transitions {
        let conditions = t.conditions.len() as u32;
        let len = t.len() as u32;
        if conditions > TRANSITION_CONDITIONS_MASK >> TRANSITION_CONDITIONS_SHIFT
            || len > TRANSITION_SIZE_MASK >> TRANSITION_SIZE_SHIFT
        {
            return Err(refuse(format!(
                "a transition of {conditions} conditions and {len} bytes does not fit its flags"
            )));
        }
        let flags = (t.flags
            & !(TRANSITION_SIZE_MASK | TRANSITION_CONDITIONS_MASK | TRANSITION_FILTER))
            | len << TRANSITION_SIZE_SHIFT
            | conditions << TRANSITION_CONDITIONS_SHIFT
            | if t.filter.is_some() {
                TRANSITION_FILTER
            } else {
                0
            };
        put32(out, flags);
        put32(out, t.tag);
        put32(out, t.duration.to_bits());
        put32(out, t.signal);
        put32(out, t.transition_weight_signal);
        put32(out, t.target as u32);
        for c in &t.conditions {
            c.write(out);
        }
        if let Some([dictionary, filter]) = t.filter {
            put32(out, dictionary);
            put32(out, filter);
        }
    }
    Ok(())
}

fn write_parameter(out: &mut Vec<u8>, p: &Parameter) {
    put32(out, p.parameter_crc);
    put16(out, p.node_index);
    put16(out, p.type_index);
    put32(out, p.parameter_index);
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let slice = self
            .pos
            .checked_add(n)
            .and_then(|end| self.bytes.get(self.pos..end))
            .ok_or_else(|| {
                refuse(format!(
                    "{n} bytes at {} run past the end at {}",
                    self.pos,
                    self.bytes.len()
                ))
            })?;
        self.pos += n;
        Ok(slice)
    }

    fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
}

fn put16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn count(n: usize, what: &str) -> Result<u32> {
    u32::try_from(n).map_err(|_| refuse(format!("{n} {what} do not fit a u32")))
}

fn count8(n: usize) -> Result<u8> {
    u8::try_from(n).map_err(|_| refuse(format!("{n} states or transitions do not fit a u8")))
}

fn refuse(why: String) -> IoError {
    IoError::InvalidData {
        context: format!("mrf: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::string_hash_str as hash;

    fn state(id: &str, transitions: Vec<MvTransition>) -> MvNodeDef {
        MvNodeDef {
            node_type: NodeType::State,
            index: 0,
            id: hash(id),
            body: NodeBody::State(StateDef {
                base: StateBaseDef::default(),
                child_count: 1,
                param_inputs_offset: 0,
                events_offset: 0,
                param_outputs_offset: 0,
                operations_offset: 0,
                transitions,
                param_inputs: vec![],
                events: vec![],
                param_outputs: vec![],
                operations: vec![],
            }),
        }
    }

    /// A root machine of two states, the first with a filtered transition to the second on a request.
    fn two_states() -> MvNetworkDef {
        let go = MvTransition {
            flags: 3 << 24,
            tag: 0,
            duration: 0.25,
            signal: 0,
            transition_weight_signal: 0,
            target: 0,
            conditions: vec![
                MvCondition {
                    set: 0,
                    test: Test::OnRequest {
                        request: 0,
                        not_set: 0,
                    },
                },
                MvCondition {
                    set: 1,
                    test: Test::InRange {
                        signal: hash("speed"),
                        upper: 2.0,
                        lower: 1.0,
                    },
                },
            ],
            filter: Some([hash("filters"), hash("upper_body")]),
        };
        let mut network = MvNetworkDef {
            version: VERSION,
            xrefs: vec![],
            requests: vec![KeyIndex {
                key: hash("Go"),
                index: 0,
            }],
            flags: vec![],
            nodes: vec![
                MvNodeDef {
                    node_type: NodeType::StateMachine,
                    index: 0,
                    id: hash("Root"),
                    body: NodeBody::StateMachine(StateMachineDef {
                        base: StateBaseDef::default(),
                        states: vec![
                            StateNode {
                                id: hash("Idle"),
                                node: 0,
                            },
                            StateNode {
                                id: hash("Moving"),
                                node: 0,
                            },
                        ],
                        transitions: vec![],
                    }),
                },
                state("Idle", vec![go]),
                state("Moving", vec![]),
                MvNodeDef {
                    node_type: NodeType::Clip,
                    index: 0,
                    id: hash("clip"),
                    body: NodeBody::Other(0u32.to_le_bytes().to_vec()),
                },
            ],
            string_table: b"Root\0".to_vec(),
        };
        // Offsets are relative to the field holding them, so they are computed from where each node lands.
        let at = network.node_offsets();
        let rel = |field: usize, node: usize| at[node] as i32 - field as i32;
        let NodeBody::StateMachine(sm) = &mut network.nodes[0].body else {
            unreachable!()
        };
        sm.base.initial = rel(at[0] + 8, 1);
        sm.states[0].node = rel(at[0] + 0x24, 1);
        sm.states[1].node = rel(at[0] + 0x2c, 2);
        let NodeBody::State(idle) = &mut network.nodes[1].body else {
            unreachable!()
        };
        idle.base.initial = rel(at[1] + 8, 3);
        idle.base.transitions_offset = 0x40 - 0x1c;
        idle.transitions[0].target = rel(at[1] + 0x40 + 0x14, 2);
        network
    }

    #[test]
    fn a_hand_built_network_saves_reopens_and_resolves_its_edges() {
        let network = two_states();
        let bytes = network.save().expect("saves");
        let back = MvNetworkDef::open(&bytes).expect("reopens");
        assert_eq!(
            (&back.requests, &back.string_table),
            (&network.requests, &network.string_table)
        );
        assert_eq!(back.save().expect("saves again"), bytes);
        let edges = back.edges();
        assert_eq!(edges[0].initial, Some(1));
        assert_eq!(edges[0].states, vec![Some(1), Some(2)]);
        assert_eq!(edges[1].initial, Some(3));
        assert_eq!(edges[1].transitions, vec![Some(2)]);
        assert_eq!(edges[2], Edges::default());
        assert_eq!(edges[3], Edges::default());
        assert_eq!(
            back.node_counts(),
            vec![
                (NodeType::StateMachine, 1),
                (NodeType::Clip, 1),
                (NodeType::State, 2)
            ]
        );
    }

    #[test]
    fn a_save_writes_each_transitions_size_count_and_filter_bits() {
        let mut network = two_states();
        let NodeBody::State(idle) = &mut network.nodes[1].body else {
            unreachable!()
        };
        idle.transitions[0].flags |= TRANSITION_SIZE_MASK | TRANSITION_CONDITIONS_MASK;
        let back = MvNetworkDef::open(&network.save().expect("saves")).expect("reopens");
        let NodeBody::State(idle) = &back.nodes[1].body else {
            unreachable!()
        };
        let flags = idle.transitions[0].flags;
        assert_eq!(
            (flags & TRANSITION_SIZE_MASK) >> TRANSITION_SIZE_SHIFT,
            24 + 12 + 16 + 8
        );
        assert_eq!(
            (flags & TRANSITION_CONDITIONS_MASK) >> TRANSITION_CONDITIONS_SHIFT,
            2
        );
        assert_ne!(flags & TRANSITION_FILTER, 0);
        assert_eq!(idle.transitions[0].modifier(), "Linear");
    }

    #[test]
    fn bytes_that_do_not_bear_out_the_layout_are_refused() {
        let bytes = two_states().save().expect("saves");
        let mut wrong_magic = bytes.clone();
        wrong_magic[0] = b'X';
        assert!(MvNetworkDef::open(&wrong_magic).is_err());
        assert!(MvNetworkDef::open(&bytes[..bytes.len() - 1]).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(MvNetworkDef::open(&trailing).is_err());
        let mut later_version = bytes.clone();
        later_version[4] = 11;
        assert!(MvNetworkDef::open(&later_version).is_err());
        // The first transition's flags sit after the root machine and the first state's fixed fields.
        let flags_at = two_states().node_offsets()[1] + 0x40;
        let mut wrong_size = bytes.clone();
        wrong_size[flags_at] ^= 0x10;
        assert!(MvNetworkDef::open(&wrong_size).is_err());
        let mut misplaced = two_states();
        let NodeBody::State(idle) = &mut misplaced.nodes[1].body else {
            unreachable!()
        };
        idle.base.transitions_offset += 4;
        assert!(MvNetworkDef::open(&misplaced.save().expect("saves")).is_err());
    }
}
