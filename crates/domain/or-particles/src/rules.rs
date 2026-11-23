//! The rules: what is wrong with a module's particle assets, as data with a code, a severity and a waiver.
//!
//! A rule is a row of [`RULES`], a finding is one place a rule applies, and a waiver is a line of the model that says why a finding
//! stands. Only an error that no reason could mend, such as a source that is not there, refuses to be waived.
//!
//! ⚠ Every rule is one this build can decide from the files: a source missing, a document of rules that does not read, a name two
//! sources hold, an event naming a rule its list lacks. How the game behaves when two files define an effect of one name is not
//! measured, so that rule warns and does not refuse; and nothing here says an effect plays in game.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::facts::{Fact, Facts};
use crate::model::{Asset, Particles};

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
        "source-escapes",
        Error,
        false,
        "a source is named by a path that leaves the module's folder",
    ),
    rule(
        "source-missing",
        Error,
        false,
        "a source an asset names is not there",
    ),
    rule(
        "source-unreadable",
        Error,
        false,
        "a source an asset names cannot be cooked",
    ),
    rule(
        "rules-unreadable",
        Error,
        false,
        "the rules of a source are not a document OpenRage reads",
    ),
    rule(
        "file-twice",
        Error,
        false,
        "two sources cook to one file of the resource",
    ),
    rule(
        "source-twice",
        Warning,
        true,
        "two assets name one source, which a build ships once",
    ),
    rule(
        "effect-twice",
        Warning,
        true,
        "two sources of the module state an effect of one name, and which one the game finds is not measured",
    ),
    rule(
        "rule-unresolved",
        Warning,
        true,
        "an effect fires an emitter or a particle rule that its list does not hold",
    ),
    rule(
        "texture-missing",
        Warning,
        true,
        "a particle rule draws a texture of its own list that the source does not hold",
    ),
    rule(
        "source-unused",
        Warning,
        true,
        "a source folder of the module is named by no asset, so a build ships nothing of it",
    ),
    rule(
        "no-assets",
        Info,
        true,
        "the module names no particle source, so a build ships nothing of it",
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
    /// The id of the asset, none for the module.
    pub asset: Option<String>,
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

/// What to call an asset in a sentence.
pub fn name_of(asset: &Asset) -> String {
    match &asset.label {
        Some(label) => format!("`{label}`"),
        None => format!("`{}`", asset.source),
    }
}

/// Every rule applied to `model`, `facts` being what the disk said of the sources it names.
///
/// A finding of a rule a waiver names carries the waiver's reason and no longer blocks; a waiver without a reason, or for a rule
/// that does not exist, is itself a finding.
pub fn check(model: &Particles, facts: &Facts) -> Vec<Finding> {
    let mut found: Vec<Finding> = Vec::new();
    let mut say = |code: &'static str, asset: Option<&Asset>, message: String| {
        let severity = rule_of(code).map_or(Error, |r| r.severity);
        found.push(Finding {
            code,
            severity,
            asset: asset.map(|a| a.id.clone()),
            message,
            waived: None,
            waiver: None,
        });
    };

    if model.assets.is_empty() {
        say(
            "no-assets",
            None,
            "the module names no particle source, so a build ships nothing of it".to_string(),
        );
    }
    let mut named: BTreeMap<&str, Vec<&Asset>> = BTreeMap::new();
    let mut cooked_to: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut effects: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for asset in &model.assets {
        let (who, path) = (name_of(asset), asset.source.as_str());
        named.entry(path).or_default().push(asset);
        match facts.get(path) {
            Some(Fact::Escapes) | None => say(
                "source-escapes",
                Some(asset),
                format!("{who} names `{path}`, which leaves the folder of its module"),
            ),
            Some(Fact::Missing) => say(
                "source-missing",
                Some(asset),
                format!("{who} names `{path}`, which is not in the folder of its module"),
            ),
            Some(Fact::Unreadable(why)) => say(
                "source-unreadable",
                Some(asset),
                format!("`{path}` of {who} {why}"),
            ),
            Some(Fact::Source(source)) => {
                let places = cooked_to
                    .entry(crate::layout::stream_path(&source.file_name()).to_ascii_lowercase())
                    .or_default();
                if !places.contains(&path) {
                    places.push(path);
                }
                match &source.inside {
                    Some(Err(why)) => say(
                        "rules-unreadable",
                        Some(asset),
                        format!("the rules of `{path}` of {who}: {why}"),
                    ),
                    Some(Ok(inside)) => {
                        for effect in &inside.effects {
                            let held = effects.entry(effect.to_ascii_lowercase()).or_default();
                            if !held.contains(&path) {
                                held.push(path);
                            }
                        }
                        for sentence in &inside.unresolved {
                            say(
                                "rule-unresolved",
                                Some(asset),
                                format!("in `{path}`, {sentence}"),
                            );
                        }
                        for sentence in &inside.textures_missing {
                            say(
                                "texture-missing",
                                Some(asset),
                                format!("in `{path}`, {sentence}"),
                            );
                        }
                    }
                    None => {}
                }
            }
        }
    }
    for (path, held) in named.iter().filter(|(_, held)| held.len() > 1) {
        for asset in held {
            say(
                "source-twice",
                Some(asset),
                format!("`{path}` is named by {} assets", held.len()),
            );
        }
    }
    for (name, places) in effects.iter().filter(|(_, places)| places.len() > 1) {
        say(
            "effect-twice",
            None,
            format!(
                "the effect `{name}` is stated by {}",
                places
                    .iter()
                    .map(|p| format!("`{p}`"))
                    .collect::<Vec<_>>()
                    .join(" and ")
            ),
        );
    }
    for path in &facts.unused {
        say(
            "source-unused",
            None,
            format!("`{path}` is named by no asset, so a build ships nothing of it"),
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
fn waive(model: &Particles, found: &mut Vec<Finding>) {
    for (index, waiver) in model.waivers.iter().enumerate() {
        let Some(rule) = rule_of(&waiver.code) else {
            found.push(Finding {
                code: "waiver-unknown",
                severity: Warning,
                asset: waiver.asset.clone(),
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
                asset: waiver.asset.clone(),
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
                    .asset
                    .as_ref()
                    .is_none_or(|id| f.asset.as_ref() == Some(id))
        }) {
            finding.waived = Some(waiver.reason.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{Inside, SourceFact};
    use crate::model::Waiver;

    fn source(name: &str, inside: Option<Result<Inside, String>>) -> Fact {
        Fact::Source(Box::new(SourceFact {
            kind: "particles".into(),
            name: name.into(),
            extension: "ypt".into(),
            bytes: 10,
            game: Vec::new(),
            digest: Some("d".into()),
            inside,
        }))
    }

    fn inside(effects: &[&str]) -> Inside {
        Inside {
            effects: effects.iter().map(ToString::to_string).collect(),
            ..Inside::default()
        }
    }

    fn model(sources: &[&str]) -> Particles {
        let mut model = Particles::new();
        for (i, path) in sources.iter().enumerate() {
            let mut asset = Asset::new(path);
            asset.id = format!("a{i}");
            model.assets.push(asset);
        }
        model
    }

    fn codes(found: &[Finding]) -> Vec<&'static str> {
        found.iter().map(|f| f.code).collect()
    }

    #[test]
    fn every_rule_has_a_unique_code_and_a_source_or_a_file_that_is_wrong_refuses_a_waiver() {
        let mut seen = std::collections::BTreeSet::new();
        for rule in RULES {
            assert!(seen.insert(rule.code), "{}", rule.code);
            assert!(
                rule.waivable
                    || !rule.code.starts_with("effect") && !rule.code.starts_with("rule-"),
                "{} cannot be set aside",
                rule.code
            );
        }
    }

    #[test]
    fn a_source_that_is_missing_escapes_or_does_not_cook_is_an_error_that_no_waiver_sets_aside() {
        let mut m = model(&[
            "gone.particles.src",
            "../out.particles.src",
            "plain.particles.src",
        ]);
        m.waivers.push(Waiver {
            code: "source-missing".into(),
            asset: None,
            reason: "it comes later".into(),
        });
        let mut facts = Facts::default();
        facts.insert("gone.particles.src", Fact::Missing);
        facts.insert("../out.particles.src", Fact::Escapes);
        facts.insert(
            "plain.particles.src",
            Fact::Unreadable("is not a source".into()),
        );
        let found = check(&m, &facts);
        assert_eq!(
            codes(&found),
            ["source-missing", "source-escapes", "source-unreadable"]
        );
        assert!(found.iter().all(Finding::blocks), "{found:?}");
    }

    #[test]
    fn two_sources_that_cook_to_one_file_or_state_one_effect_are_found_and_the_second_can_be_waived(
    ) {
        let mut m = model(&["a.particles.src", "b.particles.src"]);
        let mut facts = Facts::default();
        facts.insert(
            "a.particles.src",
            source("fx", Some(Ok(inside(&["Spark"])))),
        );
        facts.insert(
            "b.particles.src",
            source("fx", Some(Ok(inside(&["spark"])))),
        );
        let found = check(&m, &facts);
        assert_eq!(codes(&found), ["effect-twice", "file-twice"]);
        assert!(found[1].blocks() && !found[0].blocks());

        m.waivers.push(Waiver {
            code: "effect-twice".into(),
            asset: None,
            reason: "the second is a replacement".into(),
        });
        let found = check(&m, &facts);
        assert_eq!(
            found[0].waived.as_deref(),
            Some("the second is a replacement")
        );
        assert!(found[1].blocks(), "a file cooked twice is not waived");
    }

    #[test]
    fn a_rule_an_event_cannot_find_and_a_texture_a_list_lacks_warn_and_a_waiver_without_a_reason_is_an_error(
    ) {
        let mut m = model(&["a.particles.src"]);
        let mut facts = Facts::default();
        let mut held = inside(&["spark"]);
        held.unresolved.push(
            "the effect `spark` fires the emitter rule `em`, which its list does not hold".into(),
        );
        held.textures_missing.push(
            "the particle rule `p` draws the texture `t`, which its list does not hold".into(),
        );
        facts.insert("a.particles.src", source("a", Some(Ok(held))));
        m.waivers.push(Waiver {
            code: "rule-unresolved".into(),
            asset: Some("a0".into()),
            reason: " ".into(),
        });
        m.waivers.push(Waiver {
            code: "no-such-rule".into(),
            asset: None,
            reason: "x".into(),
        });
        let found = check(&m, &facts);
        assert_eq!(
            codes(&found),
            [
                "rule-unresolved",
                "texture-missing",
                "waiver-without-reason",
                "waiver-unknown"
            ]
        );
        assert_eq!(found[2].severity, Severity::Error);
        assert!(
            found[0].waived.is_none(),
            "a waiver without a reason sets nothing aside"
        );
    }

    #[test]
    fn an_empty_module_and_a_source_nothing_names_are_said() {
        let found = check(&Particles::new(), &Facts::default());
        assert_eq!(codes(&found), ["no-assets"]);
        let mut facts = Facts::default();
        facts.unused.push("stray.particles.src".into());
        let found = check(&model(&[]), &facts);
        assert_eq!(codes(&found), ["no-assets", "source-unused"]);
    }
}
