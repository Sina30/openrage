//! GTA VI's world-state selectors: the bodies of a game-shape archive whose root holds placed
//! nodes, each node's selectors choosing a configuration that switches script-managed map groups
//! on and off, and the rules that pick a configuration by the time of day.
//!
//! ⚠ What a selector picks with no rule that holds is its default configuration; that the first
//! rule that holds wins, and that the end hour of a time of day is inside it, are taken from the
//! member names and the paired halves of the day the bodies state, not from the engine.

use std::collections::HashSet;

use crate::hash::literal_string_hash as lit;
use crate::meta::binary::hash_index::default_hash_index;
use crate::meta::binary::pso::table_form;
use crate::meta::binary::rbm::schema::RbmModel;
use crate::meta::{MetaInstance, Value};

/// The root class of a world-state body.
pub const WORLD_STATE: u32 = 0xD52B_2D2F;
const NODES: u32 = lit(b"Nodes");
const COMPONENTS: u32 = lit(b"Components");
const SELECTOR_COMPONENT: u32 = 0x13D6_866D;
const SELECTORS: u32 = lit(b"Selectors");
const DEFAULT_CONFIG: u32 = lit(b"DefaultConfig");
const CONFIGS: u32 = lit(b"Configs");
const CHANGES: u32 = lit(b"Changes");
/// The on, off and state-2 lists of the two change classes: words, and pairs of words.
const WORD_LISTS: [u32; 3] = [0x2699_5CF4, 0xC8BD_754A, 0x07DD_7D6D];
const PAIR_LISTS: [u32; 3] = [0x8105_B9F3, 0x67F6_A736, 0x666D_D869];
/// The group word of a pair of words.
const PAIR_WORD: u32 = 0xE255_4B92;
const RULE_COMPONENT: u32 = 0x8740_CA71;
const RULE_SETS: u32 = 0xA646_5A62;
const RULE_SELECTOR: u32 = 0xAABB_E900;
const RULES: u32 = lit(b"Rules");
const RULE_CONFIG: u32 = lit(b"Config");
const CONDITION: u32 = lit(b"Condition");
const CONDITIONS: u32 = lit(b"Conditions");
const TIME_OF_DAY: u32 = lit(b"CAIConditionTimeOfDay");
const TIME_WITH_MINUTES: u32 = lit(b"CAIConditionTimeOfDayWithMinutes");
const NOT: u32 = lit(b"CAIConditionNot");
const AND: u32 = lit(b"CAIConditionAnd");
const OR: u32 = lit(b"CAIConditionOr");
const TRUE: u32 = lit(b"CAIConditionTrue");
const RAINING: u32 = lit(b"CAIConditionRaining");
const RAIN_AMOUNT: u32 = lit(b"RainAmount");
const START_HOUR: u32 = lit(b"startHour");
const START_MINUTE: u32 = lit(b"startMinute");
const END_HOUR: u32 = lit(b"endHour");
const END_MINUTE: u32 = lit(b"endMinute");

/// Why a body is not a world-state body.
#[derive(Debug, thiserror::Error)]
pub enum WorldStateError {
    #[error(transparent)]
    Meta(#[from] crate::meta::MetaError),
    #[error("the root structure {0:#010X} is not a world-state body (hash_D52B2D2F)")]
    NotWorldState(u32),
    #[error("the world-state body is broken: {0}")]
    Broken(&'static str),
}

/// The selectors of one world-state body, every node's in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldState {
    pub selectors: Vec<Selector>,
}

/// A selector: the configurations it chooses among, its default, and the rules that pick one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Selector {
    pub key: u32,
    /// The key of the configuration it takes when no rule holds; 0 for none.
    pub default: u32,
    pub configs: Vec<Config>,
    /// In the order the body states them.
    pub rules: Vec<Rule>,
}

/// A configuration: the group words its changes switch on, off and to state 2.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub key: u32,
    pub on: Vec<u64>,
    pub off: Vec<u64>,
    pub state2: Vec<u64>,
}

/// A rule: the configuration it picks while its condition holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub config: u32,
    pub condition: Condition,
}

/// A condition of a rule.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// From the start hour to the end hour, both inside, past midnight where the start is later.
    TimeOfDay {
        start: i32,
        end: i32,
    },
    /// The same to the minute: hour and minute of the start, then of the end.
    TimeWithMinutes {
        start: (i32, i32),
        end: (i32, i32),
    },
    Not(Box<Condition>),
    And(Vec<Condition>),
    Or(Vec<Condition>),
    True,
    Raining(f32),
    /// A class this does not evaluate, by the name the value tree gives it.
    Other(String),
}

impl Condition {
    /// Whether it holds at the whole hour `hour` of clear weather; `None` where nothing here says.
    pub fn holds(&self, hour: u32) -> Option<bool> {
        let within = |at: f32, start: f32, end: f32| match start <= end {
            true => start <= at && at <= end,
            false => at >= start || at <= end,
        };
        Some(match self {
            Self::TimeOfDay { start, end } => within(hour as f32, *start as f32, *end as f32),
            Self::TimeWithMinutes { start, end } => {
                let at = |(h, m): (i32, i32)| h as f32 + m as f32 / 60.0;
                within(hour as f32 + 0.5, at(*start), at(*end))
            }
            Self::Not(inner) => !inner.holds(hour)?,
            Self::And(all) => {
                let held: Vec<_> = all.iter().map(|c| c.holds(hour)).collect();
                if held.contains(&Some(false)) {
                    false
                } else {
                    held.iter().all(Option::is_some).then_some(true)?
                }
            }
            Self::Or(any) => {
                let held: Vec<_> = any.iter().map(|c| c.holds(hour)).collect();
                if held.contains(&Some(true)) {
                    true
                } else {
                    held.iter().all(Option::is_some).then_some(false)?
                }
            }
            Self::True => true,
            Self::Raining(_) => false,
            Self::Other(_) => return None,
        })
    }
}

impl Selector {
    /// The configuration taken at the whole hour `hour`: the first rule that holds picks it, else the default; `None` for a key no configuration has, or no default.
    pub fn pick(&self, hour: u32) -> Option<&Config> {
        let key = (self.rules.iter())
            .find(|rule| rule.condition.holds(hour) == Some(true))
            .map_or(self.default, |rule| rule.config);
        (key != 0)
            .then(|| self.configs.iter().find(|c| c.key == key))
            .flatten()
    }
}

/// The group words on at the whole hour `hour` under `selectors`: those a picked configuration turns on or to state 2 and none turns off.
pub fn words_on<'a>(selectors: impl IntoIterator<Item = &'a Selector>, hour: u32) -> HashSet<u64> {
    let (mut on, mut off) = (HashSet::new(), HashSet::new());
    for config in selectors.into_iter().filter_map(|s| s.pick(hour)) {
        on.extend(config.on.iter().chain(&config.state2).copied());
        off.extend(config.off.iter().copied());
    }
    on.retain(|word| !off.contains(word));
    on
}

impl WorldState {
    /// The selectors of the table-form document `bytes`.
    pub fn open(bytes: &[u8]) -> Result<Self, WorldStateError> {
        Self::read(&table_form::model_of(bytes)?)
    }

    /// The selectors of a table-form document's type table and blocks.
    pub fn read(model: &RbmModel) -> Result<Self, WorldStateError> {
        let root = (model.block(model.root_block_id as i32)).map_or(0, |b| b.data_type);
        if root != WORLD_STATE {
            return Err(WorldStateError::NotWorldState(root));
        }
        let tree = table_form::decode(model)?;
        let Some(Value::Array(nodes)) = tree.get(&name(NODES)) else {
            return Err(WorldStateError::Broken("its nodes are not an array"));
        };
        let mut selectors = Vec::new();
        for node in nodes.iter().filter_map(object) {
            let components: Vec<&MetaInstance> = match node.get(&name(COMPONENTS)) {
                Some(Value::Array(items)) => items.iter().filter_map(object).collect(),
                _ => {
                    return Err(WorldStateError::Broken(
                        "a node's components are not an array",
                    ))
                }
            };
            let (rule_class, selector_class) = (name(RULE_COMPONENT), name(SELECTOR_COMPONENT));
            // The rule set of a selector names its key, in the same node; a later one stands.
            let mut rules: Vec<(u32, Vec<Rule>)> = Vec::new();
            for component in components.iter().filter(|c| c.type_name == rule_class) {
                for (_, set) in pairs(component, RULE_SETS) {
                    let Some(key) = set.get(&name(RULE_SELECTOR)).and_then(key32) else {
                        continue;
                    };
                    rules.retain(|(k, _)| *k != key);
                    rules.push((key, rules_of(set)));
                }
            }
            for component in components.iter().filter(|c| c.type_name == selector_class) {
                for (key, selector) in pairs(component, SELECTORS) {
                    let configs = (pairs(selector, CONFIGS).into_iter())
                        .map(|(key, config)| config_of(key, config))
                        .collect();
                    selectors.push(Selector {
                        key,
                        default: selector
                            .get(&name(DEFAULT_CONFIG))
                            .and_then(key32)
                            .unwrap_or(0),
                        configs,
                        rules: (rules.iter().find(|(k, _)| *k == key))
                            .map_or_else(Vec::new, |(_, r)| r.clone()),
                    });
                }
            }
        }
        Ok(Self { selectors })
    }
}

/// What the value tree names a member or class of hash `hash` by.
fn name(hash: u32) -> String {
    default_hash_index().name_or_literal(hash)
}

/// The structure a value holds in place or points at.
fn object(value: &Value) -> Option<&MetaInstance> {
    match value {
        Value::Struct(s) | Value::Pointer(Some(s)) => Some(s),
        _ => None,
    }
}

/// A 32-bit hash as a value states it.
fn key32(value: &Value) -> Option<u32> {
    match value {
        Value::Hash(h) => Some(h.hash),
        Value::UInt(u) => u32::try_from(*u).ok(),
        _ => None,
    }
}

/// A 64-bit group word as a value states it: the number, or the text a name gives it.
fn word64(value: &Value) -> Option<u64> {
    match value {
        Value::UInt(u) => Some(*u),
        Value::String(text) => Some(crate::gta6::hash::name_hash64(text.as_bytes())),
        _ => object(value).and_then(|pair| word64(pair.get(&name(PAIR_WORD))?)),
    }
}

fn number(of: &MetaInstance, member: u32) -> i32 {
    match of.get(&name(member)) {
        Some(Value::Int(v)) => *v as i32,
        Some(Value::UInt(v)) => *v as i32,
        _ => 0,
    }
}

/// The pairs of the map `member` of `owner` whose key is a hash and whose value is a structure.
fn pairs(owner: &MetaInstance, member: u32) -> Vec<(u32, &MetaInstance)> {
    let Some(Value::Map(pairs)) = owner.get(&name(member)) else {
        return Vec::new();
    };
    (pairs.iter())
        .filter_map(|(key, value)| Some((key32(key)?, object(value)?)))
        .collect()
}

fn array<'a>(owner: &'a MetaInstance, member: u32) -> &'a [Value] {
    match owner.get(&name(member)) {
        Some(Value::Array(items)) => items,
        _ => &[],
    }
}

fn config_of(key: u32, config: &MetaInstance) -> Config {
    let mut out = Config {
        key,
        ..Config::default()
    };
    for change in array(config, CHANGES).iter().filter_map(object) {
        for lists in [WORD_LISTS, PAIR_LISTS] {
            let [on, off, state2] = lists.map(|list| array(change, list).iter().filter_map(word64));
            out.on.extend(on);
            out.off.extend(off);
            out.state2.extend(state2);
        }
    }
    out
}

fn rules_of(set: &MetaInstance) -> Vec<Rule> {
    (array(set, RULES).iter().filter_map(object))
        .map(|rule| Rule {
            config: rule.get(&name(RULE_CONFIG)).and_then(key32).unwrap_or(0),
            // A rule that names no condition always holds.
            condition: match rule.get(&name(CONDITION)) {
                Some(Value::Pointer(None)) | None => Condition::True,
                Some(value) => object(value).map_or_else(unread, condition_of),
            },
        })
        .collect()
}

fn condition_of(c: &MetaInstance) -> Condition {
    let class = [TIME_OF_DAY, TIME_WITH_MINUTES, NOT, AND, OR, TRUE, RAINING]
        .into_iter()
        .find(|class| c.type_name == name(*class));
    let all = || {
        array(c, CONDITIONS)
            .iter()
            .filter_map(object)
            .map(condition_of)
            .collect()
    };
    match class {
        Some(TIME_OF_DAY) => Condition::TimeOfDay {
            start: number(c, START_HOUR),
            end: number(c, END_HOUR),
        },
        Some(TIME_WITH_MINUTES) => Condition::TimeWithMinutes {
            start: (number(c, START_HOUR), number(c, START_MINUTE)),
            end: (number(c, END_HOUR), number(c, END_MINUTE)),
        },
        Some(NOT) => Condition::Not(Box::new(
            (c.get(&name(CONDITION)).and_then(object)).map_or_else(unread, condition_of),
        )),
        Some(AND) => Condition::And(all()),
        Some(OR) => Condition::Or(all()),
        Some(TRUE) => Condition::True,
        Some(RAINING) => Condition::Raining(match c.get(&name(RAIN_AMOUNT)) {
            Some(Value::Float(v)) => *v as f32,
            _ => 0.0,
        }),
        _ => Condition::Other(c.type_name.clone()),
    }
}

/// A condition whose value is no structure: one nothing here evaluates.
fn unread() -> Condition {
    Condition::Other(String::new())
}

#[cfg(test)]
#[path = "world_state_tests.rs"]
mod tests;
