//! The rules: what is wrong with a module's peds, as data with a code, a severity and a waiver.
//!
//! A rule is a row of [`RULES`], a finding is one place a rule applies, and a waiver is a line of the
//! model that says why a finding stands. Only an error that no reason could mend, such as a file that
//! is not there, refuses to be waived.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::facts::{Fact, Facts};
use crate::layout::{resource_path, STREAM};
use crate::metadata::Entries;
use crate::model::{is_identifier, AddonPeds, Ped};

pub use or_project::content::rule::{Rule, Severity};

const fn rule(
    code: &'static str,
    severity: Severity,
    waivable: bool,
    summary: &'static str,
) -> Rule {
    Rule::new(code, severity, waivable, summary)
}

use Severity::{Error, Warning};

/// Every rule, in the order a report lists them.
pub const RULES: &[Rule] = &[
    rule(
        "ped-name",
        Error,
        false,
        "a ped's name is not a lower case name the game resolves",
    ),
    rule(
        "ped-twice",
        Error,
        false,
        "two peds of the module have one name",
    ),
    rule(
        "no-model",
        Error,
        false,
        "a ped lists no .yft, which is its model",
    ),
    rule(
        "model-name",
        Error,
        true,
        "the model of a ped streams under another name than the ped's",
    ),
    rule(
        "file-escapes",
        Error,
        false,
        "a file, or the data file, is named by a path that leaves the module's folder",
    ),
    rule(
        "file-missing",
        Error,
        false,
        "a file a ped names, or the data file, is not a file",
    ),
    rule(
        "file-unreadable",
        Error,
        true,
        "the reader for a file refuses it",
    ),
    rule(
        "place-escapes",
        Error,
        false,
        "a file of a ped lands where the game does not stream from",
    ),
    rule(
        "file-twice",
        Error,
        false,
        "two files land at one place in the resource",
    ),
    rule(
        "meta-no-entry",
        Error,
        true,
        "the data file has no entry under a ped's name",
    ),
    rule("no-textures", Warning, true, "a ped lists no .ytd"),
    rule(
        "meta-missing",
        Warning,
        true,
        "the module states no data file that defines its peds",
    ),
    rule(
        "duplicate-content",
        Warning,
        true,
        "two files ship the same bytes",
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
    /// The id of the ped, none for the module.
    pub ped: Option<String>,
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

/// What to call a ped in a sentence.
pub fn name_of(ped: &Ped) -> String {
    match &ped.label {
        Some(label) => format!("`{label}`"),
        None => format!("`{}`", ped.name),
    }
}

/// The extension of a path, lower case.
fn extension(path: &str) -> String {
    path.rsplit_once('.')
        .map_or(String::new(), |(_, e)| e.to_ascii_lowercase())
}

/// The name a resource path streams under: its last name without extensions, lower case.
fn streams_as(resource: &str) -> String {
    let name = resource.rsplit('/').next().unwrap_or(resource);
    name.split_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_lowercase()
}

struct Report<'a> {
    model: &'a AddonPeds,
    found: Vec<Finding>,
}

impl Report<'_> {
    fn add(&mut self, code: &'static str, ped: Option<&Ped>, message: String) {
        let severity = rule_of(code).map_or(Error, |r| r.severity);
        self.found.push(Finding {
            code,
            severity,
            ped: ped.map(|p| p.id.clone()),
            message,
            waived: None,
            waiver: None,
        });
    }
}

/// Every finding of `model`, with the waivers applied.
///
/// `entries` is what the data file the model states defines peds under, none when the model states none.
pub fn check(model: &AddonPeds, facts: &Facts, entries: Option<&Entries>) -> Vec<Finding> {
    let mut report = Report {
        model,
        found: Vec::new(),
    };
    names(&mut report);
    for ped in &model.peds {
        one_ped(&mut report, ped, facts);
    }
    places(&mut report, facts);
    meta(&mut report, facts, entries);
    waivers(&mut report);
    report.found
}

fn names(r: &mut Report) {
    let mut seen: BTreeMap<String, &Ped> = BTreeMap::new();
    for ped in &r.model.peds {
        if !is_identifier(&ped.name) {
            r.add(
                "ped-name",
                Some(ped),
                format!(
                    "{} is named {:?}, which is not lower case letters, digits and underscores from a letter",
                    name_of(ped),
                    ped.name
                ),
            );
        }
        if let Some(first) = seen.insert(ped.name.to_lowercase(), ped) {
            r.add(
                "ped-twice",
                Some(ped),
                format!(
                    "{} and {} are both called `{}`",
                    name_of(first),
                    name_of(ped),
                    ped.name
                ),
            );
        }
    }
}

fn one_ped(r: &mut Report, ped: &Ped, facts: &Facts) {
    let who = name_of(ped);
    let models: Vec<_> = ped
        .files
        .iter()
        .filter(|f| extension(&f.path) == "yft")
        .collect();
    if models.is_empty() {
        r.add("no-model", Some(ped), format!("{who} lists no .yft"));
    }
    for model in models {
        let place = resource_path(&ped.name, model);
        if streams_as(&place) != ped.name.to_lowercase() {
            r.add(
                "model-name",
                Some(ped),
                format!(
                    "the model of {who} streams as `{place}`, and the game finds a ped's model under the ped's name `{}`",
                    ped.name
                ),
            );
        }
    }
    if !ped.files.iter().any(|f| extension(&f.path) == "ytd") {
        r.add("no-textures", Some(ped), format!("{who} lists no .ytd"));
    }
    for file in &ped.files {
        match facts.get(&file.path) {
            Some(Fact::Escapes) => r.add(
                "file-escapes",
                Some(ped),
                format!("`{}` of {who} leaves the module's folder", file.path),
            ),
            Some(Fact::Missing) | Some(Fact::Folder) | None => r.add(
                "file-missing",
                Some(ped),
                format!("`{}` of {who} is not a file", file.path),
            ),
            Some(Fact::File {
                read: Some(Err(why)),
                ..
            }) => r.add(
                "file-unreadable",
                Some(ped),
                format!("`{}` of {who} does not open: {why}", file.path),
            ),
            Some(Fact::File { .. }) => {}
        }
        let place = resource_path(&ped.name, file);
        let inside = place.starts_with(&format!("{STREAM}/"))
            && place.split('/').all(or_project::paths::is_component);
        if !inside {
            r.add(
                "place-escapes",
                Some(ped),
                format!(
                    "`{}` of {who} lands at `{place}`, and the game streams a ped's files from {STREAM}/ only",
                    file.path
                ),
            );
        }
    }
}

/// Two files at one place, and two places with the same bytes.
fn places(r: &mut Report, facts: &Facts) {
    let mut at: BTreeMap<String, (&Ped, &str)> = BTreeMap::new();
    let mut bytes: BTreeMap<&str, (&Ped, String)> = BTreeMap::new();
    for ped in &r.model.peds {
        for file in &ped.files {
            let place = resource_path(&ped.name, file);
            if let Some((first, from)) = at.get(&place.to_lowercase()) {
                let said = format!(
                    "`{}` of {} and `{from}` of {} both land at `{place}`",
                    file.path,
                    name_of(ped),
                    name_of(first)
                );
                r.add("file-twice", Some(ped), said);
                continue;
            }
            at.insert(place.to_lowercase(), (ped, &file.path));
            if let Some(Fact::File {
                digest: Some(d),
                bytes: size,
                ..
            }) = facts.get(&file.path)
            {
                if *size == 0 {
                    continue;
                }
                match bytes.get(d.as_str()) {
                    Some((first, other)) => {
                        let said = format!(
                            "`{}` of {} ships the same bytes as `{other}` of {}",
                            file.path,
                            name_of(ped),
                            name_of(first)
                        );
                        r.add("duplicate-content", Some(ped), said);
                    }
                    None => {
                        bytes.insert(d, (ped, file.path.clone()));
                    }
                }
            }
        }
    }
}

fn meta(r: &mut Report, facts: &Facts, entries: Option<&Entries>) {
    if r.model.peds.is_empty() {
        return;
    }
    let Some(entries) = entries else {
        r.add(
            "meta-missing",
            None,
            "the module states no data file, so nothing defines its peds to the game".to_string(),
        );
        return;
    };
    // ⚠ The data file is no ped's file, so no ped's rule has looked at it: its own fact is read here.
    let path = r.model.meta.clone().unwrap_or_default();
    let refusal = match (facts.get(&path), entries) {
        (Some(Fact::Escapes), _) => Some(("file-escapes", "leaves the module's folder".into())),
        (Some(Fact::Missing | Fact::Folder) | None, _) => {
            Some(("file-missing", "is not a file".into()))
        }
        // The names it defines peds under are what a ped's name is held to: no entry can be compared with a file that is not text.
        (
            Some(Fact::File {
                read: Some(Err(why)),
                ..
            }),
            _,
        )
        | (Some(Fact::File { .. }), Err(why)) => {
            Some(("file-unreadable", format!("does not open: {why}")))
        }
        (Some(Fact::File { .. }), Ok(_)) => None,
    };
    if let Some((code, what)) = refusal {
        r.add(code, None, format!("the data file `{path}` {what}"));
        return;
    }
    let Ok(names) = entries else {
        return;
    };
    let missing: Vec<_> = r
        .model
        .peds
        .iter()
        .filter(|p| !names.contains(&p.name.to_lowercase()))
        .collect();
    for ped in missing {
        r.add(
            "meta-no-entry",
            Some(ped),
            format!(
                "`{path}` has no entry named `{}`, which {} is called",
                ped.name,
                name_of(ped)
            ),
        );
    }
}

fn waivers(r: &mut Report) {
    let model = r.model;
    let mut applied: BTreeSet<usize> = BTreeSet::new();
    for (i, waiver) in model.waivers.iter().enumerate() {
        let before = r.found.len();
        match rule_of(&waiver.code) {
            None => r.add(
                "waiver-unknown",
                None,
                format!("a waiver names `{}`, which is no rule", waiver.code),
            ),
            Some(_) if waiver.reason.trim().is_empty() => r.add(
                "waiver-without-reason",
                None,
                format!("the waiver of `{}` gives no reason", waiver.code),
            ),
            Some(rule) if !rule.waivable => r.add(
                "waiver-unknown",
                None,
                format!("`{}` cannot be waived: no reason mends it", waiver.code),
            ),
            Some(_) => {
                applied.insert(i);
            }
        }
        for added in &mut r.found[before..] {
            added.waiver = Some(i);
        }
    }
    for finding in &mut r.found {
        let waiver = applied.iter().map(|i| &model.waivers[*i]).find(|waiver| {
            waiver.code == finding.code && (waiver.ped.is_none() || waiver.ped == finding.ped)
        });
        if let Some(waiver) = waiver {
            finding.waived = Some(waiver.reason.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::named_paths;
    use crate::model::{PedFile, Waiver};

    fn ped(name: &str, files: &[&str]) -> Ped {
        let mut ped = Ped::new(name);
        ped.files = files
            .iter()
            .map(|path| PedFile {
                path: (*path).into(),
                at: None,
            })
            .collect();
        ped
    }

    /// A module of `peds` with a data file, every file there and distinct.
    fn model(peds: Vec<Ped>) -> AddonPeds {
        let mut model = AddonPeds::new();
        model.meta = Some("peds.meta".into());
        model.peds = peds;
        model
    }

    fn facts_for(model: &AddonPeds) -> Facts {
        Facts::from_files(named_paths(model).map(|p| (p, 10, p)))
    }

    fn entries(names: &[&str]) -> Entries {
        Ok(names.iter().map(|n| (*n).to_string()).collect())
    }

    fn codes(model: &AddonPeds, facts: &Facts, entries: &Entries) -> Vec<&'static str> {
        check(model, facts, Some(entries))
            .iter()
            .filter(|f| f.waived.is_none())
            .map(|f| f.code)
            .collect()
    }

    #[test]
    fn a_ped_with_its_files_and_an_entry_has_no_finding() {
        let m = model(vec![ped(
            "my_ped",
            &["my_ped.yft", "my_ped.ytd", "head_000_r.ydd"],
        )]);
        assert_eq!(
            codes(&m, &facts_for(&m), &entries(&["my_ped"])),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn a_name_that_is_not_lower_case_or_is_taken_is_an_error_no_waiver_mends() {
        let m = model(vec![
            ped("My_Ped", &["My_Ped.yft", "My_Ped.ytd"]),
            ped("other", &["other.yft", "other.ytd"]),
            ped("OTHER", &["o.yft", "o.ytd"]),
        ]);
        let got = codes(&m, &facts_for(&m), &entries(&["my_ped", "other"]));
        assert!(got.contains(&"ped-name"), "{got:?}");
        assert!(got.contains(&"ped-twice"), "{got:?}");
        for code in ["ped-name", "ped-twice"] {
            assert!(!rule_of(code).unwrap().waivable, "{code}");
        }
    }

    #[test]
    fn a_ped_with_no_model_or_a_model_under_another_name_or_no_texture_is_told_so() {
        let m = model(vec![
            ped("a", &["a.ytd"]),
            ped("b", &["not_b.yft", "b.ytd"]),
            ped("c", &["c.yft"]),
        ]);
        let got = codes(&m, &facts_for(&m), &entries(&["a", "b", "c"]));
        assert_eq!(got, ["no-model", "model-name", "no-textures"]);
    }

    #[test]
    fn a_file_that_is_missing_escapes_or_will_not_open_is_an_error_naming_it() {
        let m = model(vec![ped("a", &["a.yft", "a.ytd", "../x.ydd", "gone.ydd"])]);
        let mut facts = facts_for(&m);
        facts.insert(
            "a.ytd",
            Fact::File {
                bytes: 3,
                digest: None,
                read: Some(Err("bad magic".into())),
                measured: None,
            },
        );
        facts.insert("../x.ydd", Fact::Escapes);
        facts.insert("gone.ydd", Fact::Missing);
        let found = check(&m, &facts, Some(&entries(&["a"])));
        let said: Vec<_> = found.iter().map(|f| (f.code, f.message.as_str())).collect();
        assert!(
            said.iter()
                .any(|(c, m)| *c == "file-unreadable" && m.contains("bad magic")),
            "{said:?}"
        );
        assert!(
            said.iter()
                .any(|(c, m)| *c == "file-escapes" && m.contains("../x.ydd")),
            "{said:?}"
        );
        assert!(
            said.iter()
                .any(|(c, m)| *c == "file-missing" && m.contains("gone.ydd")),
            "{said:?}"
        );
    }

    #[test]
    fn a_file_that_lands_outside_stream_or_on_another_is_an_error() {
        let mut m = model(vec![ped(
            "a",
            &["a.yft", "a.ytd", "b/head.ydd", "c/head.ydd"],
        )]);
        m.peds[0].files[2].at = Some("data/head.ydd".into());
        m.peds[0].files[3].at = Some("stream/a/head.ydd".into());
        m.peds[0].files.push(PedFile {
            path: "other/head.ydd".into(),
            at: Some("STREAM/a/Head.ydd".into()),
        });
        let got = codes(&m, &facts_for(&m), &entries(&["a"]));
        assert!(got.contains(&"place-escapes"), "{got:?}");
        assert!(
            got.contains(&"file-twice"),
            "two places that differ in case only are one: {got:?}"
        );
    }

    #[test]
    fn the_data_file_must_name_every_ped_and_a_module_without_one_is_warned() {
        let m = model(vec![
            ped("a", &["a.yft", "a.ytd"]),
            ped("b", &["b.yft", "b.ytd"]),
        ]);
        let got = check(&m, &facts_for(&m), Some(&entries(&["a"])));
        assert_eq!(
            got.iter()
                .map(|f| (f.code, f.ped.as_deref() == Some(m.peds[1].id.as_str())))
                .collect::<Vec<_>>(),
            [("meta-no-entry", true)]
        );
        let mut none = m.clone();
        none.meta = None;
        let got = check(&none, &facts_for(&none), None);
        assert_eq!(
            got.iter().map(|f| f.code).collect::<Vec<_>>(),
            ["meta-missing"]
        );
        assert_eq!(got[0].severity, Severity::Warning);
    }

    #[test]
    fn a_data_file_that_is_missing_a_folder_outside_the_module_or_not_text_stops_the_build() {
        let m = model(vec![ped("a", &["a.yft", "a.ytd"])]);
        let blocking = |facts: &Facts, entries: &Entries| -> Vec<(&'static str, bool)> {
            check(&m, facts, Some(entries))
                .iter()
                .map(|f| (f.code, f.blocks() && f.ped.is_none()))
                .collect()
        };
        let ok = entries(&["a"]);
        for (fact, code) in [
            (Fact::Missing, "file-missing"),
            (Fact::Folder, "file-missing"),
            (Fact::Escapes, "file-escapes"),
        ] {
            let mut facts = facts_for(&m);
            facts.insert("peds.meta", fact.clone());
            assert_eq!(blocking(&facts, &ok), [(code, true)], "{fact:?}");
        }
        let mut unread = facts_for(&m);
        unread.insert(
            "peds.meta",
            Fact::File {
                bytes: 4,
                digest: None,
                read: Some(Err("bad magic".into())),
                measured: None,
            },
        );
        assert_eq!(
            blocking(&unread, &ok),
            [("file-unreadable", true)],
            "the reader refuses it"
        );
        let not_text: Entries = Err("`peds.meta` is not text".into());
        assert_eq!(
            blocking(&facts_for(&m), &not_text),
            [("file-unreadable", true)],
            "its names cannot be read"
        );
        let said = check(&m, &facts_for(&m), Some(&not_text));
        assert!(said[0].message.contains("peds.meta"), "{:?}", said[0]);
    }

    #[test]
    fn two_files_with_the_same_bytes_are_a_warning() {
        let m = model(vec![ped("a", &["a.yft", "a.ytd", "x.ydd", "y.ydd"])]);
        let mut facts = facts_for(&m);
        for path in ["x.ydd", "y.ydd"] {
            facts.insert(
                path,
                Fact::File {
                    bytes: 5,
                    digest: Some("same".into()),
                    read: None,
                    measured: None,
                },
            );
        }
        let got = check(&m, &facts, Some(&entries(&["a"])));
        assert_eq!(
            got.iter().map(|f| f.code).collect::<Vec<_>>(),
            ["duplicate-content"]
        );
    }

    #[test]
    fn a_waiver_with_a_reason_sets_a_finding_aside_and_one_without_is_itself_an_error() {
        let mut m = model(vec![ped("a", &["a.yft"])]);
        m.waivers.push(Waiver {
            code: "no-textures".into(),
            ped: None,
            reason: "it wears the base game's".into(),
        });
        let found = check(&m, &facts_for(&m), Some(&entries(&["a"])));
        assert_eq!(found[0].waived.as_deref(), Some("it wears the base game's"));
        assert!(!found[0].blocks());

        m.waivers[0].reason = "  ".into();
        let found = check(&m, &facts_for(&m), Some(&entries(&["a"])));
        assert!(found
            .iter()
            .any(|f| f.code == "waiver-without-reason" && f.blocks()));
        assert!(found
            .iter()
            .any(|f| f.code == "no-textures" && f.waived.is_none()));

        m.waivers[0].code = "ped-name".into();
        m.waivers[0].reason = "because".into();
        let found = check(&m, &facts_for(&m), Some(&entries(&["a"])));
        assert!(
            found.iter().any(|f| f.code == "waiver-unknown"),
            "a rule no reason mends is not waived"
        );
    }

    #[test]
    fn a_waiver_for_one_ped_does_not_reach_another() {
        let mut m = model(vec![ped("a", &["a.yft"]), ped("b", &["b.yft"])]);
        m.waivers.push(Waiver {
            code: "no-textures".into(),
            ped: Some(m.peds[0].id.clone()),
            reason: "ok".into(),
        });
        let found = check(&m, &facts_for(&m), Some(&entries(&["a", "b"])));
        let open: Vec<_> = found
            .iter()
            .filter(|f| f.waived.is_none())
            .map(|f| f.ped.clone())
            .collect();
        assert_eq!(open, [Some(m.peds[1].id.clone())]);
    }
}
