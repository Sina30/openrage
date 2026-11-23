//! The rules a pack is held to are set from the Problems list as one step of Undo that the list answers at once, and what the
//! list shows is written to a file by the name chosen, as is the list of rules.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder, as the Wardrobe's other cases are: no install, no
//! keys, and a worker for each edit that a case waits on.

use std::path::Path;

use or_wardrobe::settings::{Level, Settings};
use or_wardrobe::Wardrobe;

use super::the_wardrobe_workspace::{drop_files, garments, opened, until, C};
use super::workbench::{ProblemFormat, ProblemGroup, ProblemShow, ProblemView, Question, Severity};
use super::AppState;
use crate::platform::file_dialogs;

/// Two jackets of one model, which is what the rule of duplicates finds.
fn two_of_one_jacket(dir: &Path) -> Vec<std::path::PathBuf> {
    drop_files(
        dir,
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"the same jacket"),
            (&format!("{C}^jbib_001_u.ydd"), b"the same jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"t0"),
            (&format!("{C}^jbib_diff_001_a_uni.ytd"), b"t0"),
        ],
    )
}

fn duplicate_findings(state: &AppState) -> usize {
    state
        .workbench
        .findings()
        .iter()
        .filter(|f| f.code == Some("duplicate-content"))
        .count()
}

fn idle(state: &mut AppState, what: &str) {
    until(state, what, |s| s.workbench.wardrobe.is_idle());
}

fn on_disk(project: &Path) -> Wardrobe {
    Wardrobe::read(&project.parent().unwrap().join("clothing")).expect("a pack on disk")
}

#[test]
fn the_rules_window_switches_a_rule_off_and_raises_another_as_one_step_that_undo_takes_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.add_clothes(two_of_one_jacket(dir.path()));
    until(&mut state, "the duplicate", |s| duplicate_findings(s) == 1);
    idle(&mut state, "the drop");
    let finding = state
        .workbench
        .findings()
        .iter()
        .find(|f| f.code == Some("duplicate-content"))
        .cloned()
        .unwrap();
    assert_eq!(
        finding.severity,
        Severity::Warning,
        "a warning by its rule's own word"
    );
    let before = on_disk(&file);

    // The window opens on what the pack states, and what is drafted changes nothing until Apply.
    let module = state.wardrobe_module_id().unwrap();
    state.ask_about_rules(module.clone());
    let Some(Question::Rules { draft, .. }) = state.workbench.asking.clone() else {
        panic!("the rules were asked");
    };
    assert_eq!(draft, before.rules);
    assert_eq!(on_disk(&file), before);

    let mut draft = draft;
    draft.set("duplicate-content", |s| s.severity = Some(Level::Error));
    state.workbench.asking = None;
    state.apply_rules(module.clone(), draft.clone());
    until(&mut state, "the rules", |s| {
        s.workbench
            .findings()
            .iter()
            .any(|f| f.code == Some("duplicate-content") && f.severity == Severity::Error)
    });
    idle(&mut state, "the edit");
    assert_eq!(
        on_disk(&file).rules,
        draft,
        "the pack keeps the rules in its wardrobe.json"
    );
    assert_eq!(state.workbench.wardrobe.undo_words(), "Undo rules");

    draft.set("duplicate-content", |s| s.on = Some(false));
    state.apply_rules(module, draft);
    until(&mut state, "the rule off", |s| duplicate_findings(s) == 0);
    idle(&mut state, "the second edit");
    assert_eq!(garments(&state), 2, "no garment was touched");

    // Each is a step of its own, and Undo takes them back one at a time to the pack as it was.
    state.undo_last_change();
    until(&mut state, "the rule on again", |s| {
        duplicate_findings(s) == 1
    });
    idle(&mut state, "the undo");
    state.undo_last_change();
    until(&mut state, "the rule as it was", |s| {
        s.workbench
            .findings()
            .iter()
            .any(|f| f.code == Some("duplicate-content") && f.severity == Severity::Warning)
    });
    idle(&mut state, "the second undo");
    assert_eq!(on_disk(&file), before);
}

#[test]
fn a_pack_that_did_not_read_has_no_rules_to_show() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    state.toasts.clear();
    state.ask_about_rules("no-such-content".to_string());
    assert!(state.workbench.asking.is_none());
    assert!(state
        .toasts
        .iter()
        .any(|t| t.message.contains("rules cannot be shown")));
    assert_eq!(state.rules_of_pack("no-such-content"), None::<Settings>);
}

#[test]
fn the_problems_the_list_shows_are_written_to_the_file_named_in_the_format_its_extension_asks_for()
{
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    state.add_clothes(two_of_one_jacket(dir.path()));
    until(&mut state, "the duplicate", |s| duplicate_findings(s) == 1);
    idle(&mut state, "the drop");

    for (name, format) in [
        ("problems.csv", ProblemFormat::Csv),
        ("problems.json", ProblemFormat::Json),
        ("problems.txt", ProblemFormat::Text),
    ] {
        let to = dir.path().join(name);
        file_dialogs::answer_next(to.clone());
        state.export_problems(&ProblemView::default());
        let written = std::fs::read_to_string(&to).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            written.contains("duplicate-content") || written.contains("Ships the same model"),
            "{name}: {written}"
        );
        match format {
            ProblemFormat::Csv => {
                assert!(written.starts_with("severity,rule,content,about,text\n"))
            }
            ProblemFormat::Json => {
                assert!(serde_json::from_str::<serde_json::Value>(&written).is_ok())
            }
            ProblemFormat::Text => assert!(
                written.starts_with("warning: ") || written.starts_with("error: "),
                "{written}"
            ),
        }
    }

    // What the filter leaves out is not written, and a list with nothing in it is not a file.
    let none = dir.path().join("none.txt");
    file_dialogs::answer_next(none.clone());
    state.toasts.clear();
    state.export_problems(&ProblemView {
        text: "no problem has these words".into(),
        show: ProblemShow::All,
        group: ProblemGroup::None,
        ..ProblemView::default()
    });
    assert!(!none.exists());
    assert!(state
        .toasts
        .iter()
        .any(|t| t.message == "There are no problems to export"));
}

#[test]
fn the_list_of_rules_is_written_as_the_document_a_script_reads() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    let to = dir.path().join("rules.json");
    file_dialogs::answer_next(to.clone());
    state.export_rules();
    let document: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&to).unwrap()).unwrap();
    let rows = or_wardrobe::rules::read_export(&document).expect("the list reads back");
    let want: Vec<_> = or_wardrobe::RULES
        .iter()
        .map(or_wardrobe::rules::Exported::from)
        .collect();
    assert_eq!(rows, want);
}
