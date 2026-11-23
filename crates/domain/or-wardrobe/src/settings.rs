//! The rules a pack holds itself to: which edition's limits count, and for each rule that can change, whether it is on, how serious it is and where its limit stands.
//!
//! They are the `rules` of `wardrobe.json`, so a project keeps them in Git beside the garments they judge. A rule is
//! written only where a person changed it, and only a rule that a reason could already set aside can change: an
//! error such as a file that is not there is not for a project to switch off.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::rules::{rule_of, Rule, Severity};

/// The game a pack is built for, which decides the limits that depend on it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Legacy,
    Enhanced,
}

/// How serious a finding is, as `wardrobe.json` spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warning,
    Error,
}

impl From<Level> for Severity {
    fn from(level: Level) -> Self {
        match level {
            Level::Info => Self::Info,
            Level::Warning => Self::Warning,
            Level::Error => Self::Error,
        }
    }
}

impl From<Severity> for Level {
    fn from(severity: Severity) -> Self {
        match severity {
            Severity::Info => Self::Info,
            Severity::Warning => Self::Warning,
            Severity::Error => Self::Error,
        }
    }
}

/// What a project says of one rule; a word left out is the rule's own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    /// `false` switches the rule off, so it finds nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Level>,
    /// Where the rule's limit stands, in the unit the rule lists it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<u64>,
}

impl Setting {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The rules of one pack.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// The edition the pack is built for, where it is built for one: a limit that depends on it holds only for that edition. Both when it says none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    /// What the project changed, by the code of the rule.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, Setting>,
    /// Whether a build that goes through freezes the drawables it shipped in `wardrobe.lock.json`; it does unless this says no.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freeze: Option<bool>,
    /// What the pack does with the places its dummies hold.
    #[serde(default, skip_serializing_if = "Dummies::is_default")]
    pub dummies: Dummies,
    /// The peds whose files replace the game's own instead of adding a collection: a DLC collection of the game, or the ped itself. Such files are written with no metadata, and a build says so, and that nobody has seen the game load them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub replace: BTreeMap<crate::names::Ped, Replace>,
}

/// What one ped's files of a pack replace of the game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Replace {
    /// The collection of the game the files replace, as the game names it (`male_freemode_beach`); none for the drawables of the ped itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dlc: Option<String>,
}

/// What a pack does with a dummy, a garment that holds a place and ships no model.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Dummies {
    /// A garment added to a slot takes the place of the first dummy there, and the dummy goes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replace_on_add: bool,
    /// A garment taken out of a slot leaves a dummy at its place, so the garments after it keep theirs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub leave_on_delete: bool,
}

impl Dummies {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// What a rule comes to for a pack: the rule as the project sets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effective {
    pub on: bool,
    pub severity: Severity,
    pub threshold: Option<u64>,
}

/// The code of the rule whose limit is the cap of drawables one collection holds of a slot.
pub const SLOT_CAP: &str = "slot-cap";
/// The cap three clothing tools document, and the one a pack has unless its project says another.
pub const DEFAULT_CAP: u32 = 128;
/// The most drawables a collection holds of a slot: a selection set states 255 for none.
pub const MAX_CAP: u32 = 255;

impl Settings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether a build freezes the drawables it ships.
    pub fn freezes(&self) -> bool {
        self.freeze.unwrap_or(true)
    }

    /// How many drawables of a slot one collection holds before a build writes the next: the project's limit, held between 1 and [`MAX_CAP`], else [`DEFAULT_CAP`].
    ///
    /// ⚠ Switching the rule off silences the warning and does not move the cut, which is a layout and not a judgement.
    pub fn slot_cap(&self) -> u32 {
        self.settings
            .get(SLOT_CAP)
            .and_then(|set| set.threshold)
            .map_or(DEFAULT_CAP, |limit| {
                u32::try_from(limit).unwrap_or(MAX_CAP).clamp(1, MAX_CAP)
            })
    }

    /// Whether the pack is held to the limits of `target`: it is when it says that edition or none.
    pub fn holds(&self, target: Target) -> bool {
        self.target.is_none_or(|own| own == target)
    }

    /// What `rule` comes to: its own row, with what the project changed of it.
    ///
    /// ⚠ A rule no reason could mend stays as it is, and so does a limit on a rule that has none.
    pub fn effective(&self, rule: &Rule) -> Effective {
        let own = Effective {
            on: true,
            severity: rule.severity,
            threshold: rule.threshold.map(|t| t.default),
        };
        let Some(set) = self.settings.get(rule.code) else {
            return own;
        };
        Effective {
            on: match rule.waivable {
                true => set.on.unwrap_or(true),
                false => true,
            },
            severity: match rule.waivable {
                true => set.severity.map_or(own.severity, Severity::from),
                false => own.severity,
            },
            threshold: rule.threshold.map(|t| set.threshold.unwrap_or(t.default)),
        }
    }

    /// One sentence for each thing the project says that changes nothing: a code that is no rule, a rule no reason mends, a limit a rule has none of.
    pub fn unheeded(&self) -> Vec<(String, String)> {
        let mut said = Vec::new();
        for (code, set) in &self.settings {
            let sentence = match rule_of(code) {
                None => "which is no rule".to_string(),
                Some(rule) if !rule.waivable && (set.on.is_some() || set.severity.is_some()) => {
                    "which no reason mends, so it is on and as serious as it is".to_string()
                }
                Some(rule) if set.threshold.is_some() && rule.threshold.is_none() => {
                    "which has no limit to set".to_string()
                }
                Some(_)
                    if code == SLOT_CAP
                        && set
                            .threshold
                            .is_some_and(|t| !(1..=u64::from(MAX_CAP)).contains(&t)) =>
                {
                    format!(
                        "which is outside 1 to {MAX_CAP}, so the cap stands at {}",
                        self.slot_cap()
                    )
                }
                Some(_) => continue,
            };
            said.push((code.clone(), sentence));
        }
        said
    }

    /// Say how `code` stands. A setting that says what the rule says already is not kept, and neither is one with nothing in it.
    pub fn set(&mut self, code: &str, change: impl FnOnce(&mut Setting)) {
        let mut set = self.settings.remove(code).unwrap_or_default();
        change(&mut set);
        if let Some(rule) = rule_of(code) {
            if set.on == Some(true) {
                set.on = None;
            }
            if set.severity.map(Severity::from) == Some(rule.severity) {
                set.severity = None;
            }
            if set.threshold == rule.threshold.map(|t| t.default) {
                set.threshold = None;
            }
        }
        if !set.is_empty() {
            self.settings.insert(code.to_string(), set);
        }
    }
}
