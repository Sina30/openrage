//! The rules: what is wrong with a module's weapons, as data with a code, a severity and a waiver.
//!
//! A rule is a row of [`RULES`], a finding is one place a rule applies, and a waiver is a line of the model that says why a finding
//! stands. Only an error that no reason could mend, such as a source that is not there, refuses to be waived.
//!
//! ⚠ Every rule is one this build can decide from the files: a name held twice, a source missing, a document that does not open, a
//! weapon its metadata does not define. Whether a component attaches to a bone its model has is not here, since no reader of a
//! model source gives the bones of one; and nothing here says a weapon loads in game.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::facts::{Fact, Facts, SourceFact};
use crate::metadata::{Defined, DefinedWeapon};
use crate::model::{Weapon, Weapons};

pub use or_project::content::rule::{Rule, Severity};

const fn rule(
    code: &'static str,
    severity: Severity,
    waivable: bool,
    summary: &'static str,
) -> Rule {
    Rule::new(code, severity, waivable, summary)
}

use Severity::{Error, Info, Warning};

/// Every rule, in the order a report lists them.
pub const RULES: &[Rule] = &[
    rule(
        "weapon-name",
        Error,
        false,
        "a weapon has no name the game resolves it by",
    ),
    rule(
        "weapon-twice",
        Error,
        false,
        "two weapons of the module have one name",
    ),
    rule(
        "source-escapes",
        Error,
        false,
        "a source is named by a path that leaves the module's folder",
    ),
    rule(
        "source-missing",
        Error,
        false,
        "a source a weapon names is not there",
    ),
    rule(
        "source-unreadable",
        Error,
        false,
        "a source a weapon names cannot be cooked",
    ),
    rule(
        "meta-unreadable",
        Error,
        false,
        "the metadata of a weapon is not a document OpenRage reads",
    ),
    rule(
        "meta-no-entry",
        Error,
        true,
        "no metadata the weapon names defines it under its name",
    ),
    rule(
        "file-twice",
        Error,
        false,
        "two sources cook to one file of the resource",
    ),
    rule(
        "no-metadata",
        Warning,
        true,
        "a weapon names no metadata that defines it",
    ),
    rule("no-model", Warning, true, "a weapon names no model"),
    rule(
        "model-differs",
        Warning,
        true,
        "the model the metadata gives a weapon is not the name of any model it lists",
    ),
    rule(
        "source-unused",
        Warning,
        true,
        "a source folder of the module is named by no weapon, so a build ships nothing of it",
    ),
    rule(
        "component-undefined",
        Info,
        true,
        "a weapon attaches a component that no metadata of the module defines",
    ),
    rule(
        "waiver-without-reason",
        Error,
        false,
        "a waiver gives no reason",
    ),
    rule("waiver-unknown", Warning, false, "a waiver names no rule"),
];

pub fn rule_of(code: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.code == code)
}

/// One place a rule applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub code: &'static str,
    pub severity: Severity,
    /// The id of the weapon, none for the module.
    pub weapon: Option<String>,
    pub message: String,
    /// The reason of the waiver that sets it aside.
    pub waived: Option<String>,
    /// For a finding about a waiver, which one: its index in the model's list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiver: Option<usize>,
}

impl Finding {
    /// Whether a build must stop for it.
    pub fn blocks(&self) -> bool {
        self.severity == Severity::Error && self.waived.is_none()
    }
}

/// What to call a weapon in a sentence.
pub fn name_of(weapon: &Weapon) -> String {
    match (&weapon.label, weapon.name.trim()) {
        (Some(label), _) => format!("`{label}`"),
        (None, "") => "a weapon with no name".to_string(),
        (None, name) => format!("`{name}`"),
    }
}

/// Every metadata source a weapon lists that opened, with its path.
fn documents<'a>(
    weapon: &'a Weapon,
    facts: &'a Facts,
) -> impl Iterator<Item = (&'a str, &'a SourceFact, &'a Defined)> {
    weapon.sources.iter().filter_map(move |path| {
        let source = facts.source(path)?;
        let Some(Ok(defined)) = &source.defined else {
            return None;
        };
        Some((path.as_str(), source, defined))
    })
}

/// The metadata definition of `weapon` among the documents it lists, found by its name without regard to case.
pub fn defined_by<'a>(weapon: &'a Weapon, facts: &'a Facts) -> Option<&'a DefinedWeapon> {
    documents(weapon, facts)
        .flat_map(|(_, _, defined)| &defined.weapons)
        .find(|d| d.name.eq_ignore_ascii_case(weapon.name.trim()))
}

/// The metadata source among those `weapon` lists that defines it, by its path in the module, found by the weapon's name without regard to case.
pub fn defining_source<'a>(weapon: &'a Weapon, facts: &Facts) -> Option<&'a str> {
    weapon.sources.iter().map(String::as_str).find(|path| {
        facts
            .source(path)
            .and_then(|s| s.defined.as_ref()?.as_ref().ok())
            .is_some_and(|d| {
                d.weapons
                    .iter()
                    .any(|w| w.name.eq_ignore_ascii_case(weapon.name.trim()))
            })
    })
}

/// The stem of a name: what is before its first dot, in lower case.
pub fn stem(name: &str) -> String {
    name.split_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_ascii_lowercase()
}

/// Every rule applied to `model`, `facts` being what the disk said of the sources it names.
///
/// A finding of a rule a waiver names carries the waiver's reason and no longer blocks; a waiver without a reason, or for a rule
/// that does not exist, is itself a finding.
pub fn check(model: &Weapons, facts: &Facts) -> Vec<Finding> {
    let mut found: Vec<Finding> = Vec::new();
    let mut say = |code: &'static str, weapon: Option<&Weapon>, message: String| {
        let severity = rule_of(code).map_or(Error, |r| r.severity);
        found.push(Finding {
            code,
            severity,
            weapon: weapon.map(|w| w.id.clone()),
            message,
            waived: None,
            waiver: None,
        });
    };

    let mut by_name: BTreeMap<String, Vec<&Weapon>> = BTreeMap::new();
    for weapon in &model.weapons {
        if weapon.name.trim().is_empty() {
            say(
                "weapon-name",
                Some(weapon),
                format!(
                    "{} has no name for the game to resolve it by",
                    name_of(weapon)
                ),
            );
        } else {
            by_name
                .entry(weapon.name.trim().to_ascii_lowercase())
                .or_default()
                .push(weapon);
        }
    }
    for held in by_name.values().filter(|held| held.len() > 1) {
        for weapon in held {
            say(
                "weapon-twice",
                Some(weapon),
                format!(
                    "{} is held {} times: the game resolves a name once, whatever its case",
                    name_of(weapon),
                    held.len()
                ),
            );
        }
    }

    let mut cooked_to: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for weapon in &model.weapons {
        let who = name_of(weapon);
        let (mut models, mut metadata) = (0usize, 0usize);
        for path in &weapon.sources {
            match facts.get(path) {
                Some(Fact::Escapes) | None => say(
                    "source-escapes",
                    Some(weapon),
                    format!("{who} names `{path}`, which leaves the folder of its weapons"),
                ),
                Some(Fact::Missing) => say(
                    "source-missing",
                    Some(weapon),
                    format!("{who} names `{path}`, which is not in the folder of its weapons"),
                ),
                Some(Fact::Unreadable(why)) => say(
                    "source-unreadable",
                    Some(weapon),
                    format!("`{path}` of {who} {why}"),
                ),
                Some(Fact::Source(source)) => {
                    models += usize::from(source.is_model());
                    metadata += usize::from(source.is_metadata());
                    let (file, kind) = if source.is_metadata() {
                        (crate::layout::data_path(&source.file_name()), "data")
                    } else {
                        (crate::layout::stream_path(&source.file_name()), "stream")
                    };
                    let _ = kind;
                    let places = cooked_to.entry(file.to_ascii_lowercase()).or_default();
                    if !places.contains(&path.as_str()) {
                        places.push(path);
                    }
                    if let Some(Err(why)) = &source.defined {
                        say(
                            "meta-unreadable",
                            Some(weapon),
                            format!("the metadata `{path}` of {who} does not open: {why}"),
                        );
                    }
                }
            }
        }
        if models == 0 {
            say("no-model", Some(weapon), format!("{who} names no model"));
        }
        if metadata == 0 {
            say(
                "no-metadata",
                Some(weapon),
                format!("{who} names no metadata that defines it"),
            );
        }
        if metadata > 0 && !weapon.name.trim().is_empty() {
            match defined_by(weapon, facts) {
                None if documents(weapon, facts).next().is_some() => say(
                    "meta-no-entry",
                    Some(weapon),
                    format!(
                        "no metadata {who} names defines a weapon called `{}`",
                        weapon.name.trim()
                    ),
                ),
                None => {}
                Some(defined) => {
                    let wanted = defined.model.to_ascii_lowercase();
                    let listed: BTreeSet<String> = weapon
                        .sources
                        .iter()
                        .filter_map(|p| facts.source(p))
                        .filter(|s| s.is_model())
                        .map(|s| stem(&s.name))
                        .collect();
                    if models > 0 && !wanted.is_empty() && !listed.contains(&wanted) {
                        say(
                            "model-differs",
                            Some(weapon),
                            format!(
                                "the metadata of {who} gives it the model `{}`, and the models it names are {}",
                                defined.model,
                                listed.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(", ")
                            ),
                        );
                    }
                    let known: BTreeSet<String> = documents(weapon, facts)
                        .flat_map(|(_, _, d)| &d.components)
                        .map(|c| c.name.to_ascii_lowercase())
                        .collect();
                    for point in &defined.attach_points {
                        for (component, _) in &point.components {
                            if !known.contains(&component.to_ascii_lowercase()) {
                                say(
                                    "component-undefined",
                                    Some(weapon),
                                    format!(
                                        "{who} attaches `{component}` at `{}`, and no metadata of its weapons defines it (the game may)",
                                        point.bone
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    for path in &facts.unused {
        say(
            "source-unused",
            None,
            format!("`{path}` is named by no weapon, so a build ships nothing of it"),
        );
    }
    for (file, places) in cooked_to.iter().filter(|(_, places)| places.len() > 1) {
        say(
            "file-twice",
            None,
            format!("`{file}` is what {} all cook to", places.join(", ")),
        );
    }
    waive(model, &mut found);
    found
}

/// Set aside the findings a waiver of the model names, and add the findings about the waivers.
fn waive(model: &Weapons, found: &mut Vec<Finding>) {
    for (index, waiver) in model.waivers.iter().enumerate() {
        let Some(rule) = rule_of(&waiver.code) else {
            found.push(Finding {
                code: "waiver-unknown",
                severity: Warning,
                weapon: waiver.weapon.clone(),
                message: format!("a waiver names `{}`, which is no rule", waiver.code),
                waived: None,
                waiver: Some(index),
            });
            continue;
        };
        if waiver.reason.trim().is_empty() {
            found.push(Finding {
                code: "waiver-without-reason",
                severity: Error,
                weapon: waiver.weapon.clone(),
                message: format!("the waiver of `{}` gives no reason", waiver.code),
                waived: None,
                waiver: Some(index),
            });
            continue;
        }
        if !rule.waivable {
            continue;
        }
        for finding in found.iter_mut().filter(|f| {
            f.code == waiver.code
                && f.waived.is_none()
                && waiver
                    .weapon
                    .as_ref()
                    .is_none_or(|id| f.weapon.as_ref() == Some(id))
        }) {
            finding.waived = Some(waiver.reason.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Waiver;

    fn source(kind: &str, name: &str, ext: &str, defined: Option<Defined>) -> Fact {
        Fact::Source(Box::new(SourceFact {
            kind: kind.into(),
            name: name.into(),
            extension: ext.into(),
            bytes: 10,
            game: Vec::new(),
            digest: Some("d".into()),
            defined: defined.map(Ok),
        }))
    }

    fn defines(name: &str, model: &str) -> Defined {
        Defined {
            root: "CWeaponInfoBlob".into(),
            weapons: vec![DefinedWeapon {
                name: name.into(),
                model: model.into(),
                audio: String::new(),
                slot: String::new(),
                group: String::new(),
                attach_points: Vec::new(),
            }],
            ..Defined::default()
        }
    }

    fn weapon(name: &str, sources: &[&str]) -> Weapon {
        let mut w = Weapon::new(name);
        w.sources = sources.iter().map(|s| s.to_string()).collect();
        w
    }

    fn codes(found: &[Finding]) -> Vec<&'static str> {
        found.iter().map(|f| f.code).collect()
    }

    #[test]
    fn a_weapon_that_is_defined_and_modelled_has_nothing_wrong() {
        let mut facts = Facts::default();
        facts.insert(
            "w_pi_x.drawable.src",
            source("drawable", "w_pi_x", "ydr", None),
        );
        facts.insert(
            "x.metadata.src",
            source("metadata", "x", "meta", Some(defines("WEAPON_X", "w_pi_x"))),
        );
        let mut model = Weapons::new();
        model.weapons.push(weapon(
            "weapon_x",
            &["w_pi_x.drawable.src", "x.metadata.src"],
        ));
        assert_eq!(check(&model, &facts), []);
    }

    #[test]
    fn every_rule_that_stops_a_build_is_found_and_a_waiver_sets_aside_only_a_rule_that_allows_it() {
        let mut facts = Facts::default();
        facts.insert(
            "x.metadata.src",
            source(
                "metadata",
                "x",
                "meta",
                Some(defines("WEAPON_X", "w_pi_other")),
            ),
        );
        facts.insert("m.drawable.src", source("drawable", "m", "ydr", None));
        facts.insert("gone.drawable.src", Fact::Missing);
        facts.insert("../up.drawable.src", Fact::Escapes);
        facts.insert(
            "bad.textures.src",
            Fact::Unreadable("has no asset.json".into()),
        );
        let mut model = Weapons::new();
        model.weapons.push(weapon(
            "WEAPON_A",
            &[
                "gone.drawable.src",
                "../up.drawable.src",
                "bad.textures.src",
            ],
        ));
        model
            .weapons
            .push(weapon("weapon_b", &["m.drawable.src", "x.metadata.src"]));
        model.weapons.push(weapon("WEAPON_B", &["m.drawable.src"]));
        model.weapons.push(weapon("  ", &[]));
        let found = check(&model, &facts);
        let got = codes(&found);
        for expected in [
            "source-missing",
            "source-escapes",
            "source-unreadable",
            "weapon-twice",
            "weapon-name",
            "meta-no-entry",
            "no-metadata",
            "no-model",
        ] {
            assert!(
                got.contains(&expected),
                "{expected} is missing from {got:?}"
            );
        }
        assert!(found.iter().filter(|f| f.blocks()).count() >= 6);

        // meta-no-entry may be waived and a missing source may not.
        model.waivers.push(Waiver {
            code: "meta-no-entry".into(),
            weapon: None,
            reason: "the game defines it".into(),
        });
        model.waivers.push(Waiver {
            code: "source-missing".into(),
            weapon: None,
            reason: "I will add it".into(),
        });
        model.waivers.push(Waiver {
            code: "no-model".into(),
            weapon: None,
            reason: String::new(),
        });
        model.waivers.push(Waiver {
            code: "not-a-rule".into(),
            weapon: None,
            reason: "x".into(),
        });
        let waived = check(&model, &facts);
        let of = |code: &str| waived.iter().filter(|f| f.code == code).collect::<Vec<_>>();
        assert!(of("meta-no-entry").iter().all(|f| f.waived.is_some()));
        assert!(of("source-missing").iter().all(|f| f.blocks()));
        assert_eq!(of("waiver-without-reason").len(), 1);
        assert_eq!(of("waiver-unknown").len(), 1);
    }

    #[test]
    fn two_sources_that_cook_to_one_file_and_a_model_that_is_not_the_one_defined_are_found() {
        let mut facts = Facts::default();
        facts.insert("a.drawable.src", source("drawable", "same", "ydr", None));
        facts.insert("b.drawable.src", source("drawable", "same", "ydr", None));
        facts.insert(
            "x.metadata.src",
            source(
                "metadata",
                "x",
                "meta",
                Some(defines("WEAPON_X", "w_pi_other")),
            ),
        );
        let mut model = Weapons::new();
        model.weapons.push(weapon(
            "WEAPON_X",
            &["a.drawable.src", "b.drawable.src", "x.metadata.src"],
        ));
        let found = check(&model, &facts);
        let got = codes(&found);
        assert!(got.contains(&"file-twice"), "{got:?}");
        assert!(got.contains(&"model-differs"), "{got:?}");
        assert!(rule_of("model-differs").unwrap().waivable);
        assert!(!rule_of("file-twice").unwrap().waivable);
    }
}
