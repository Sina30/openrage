//! The Build tab and the Builds window after a second look: what a build changed said once and in one place, the reason a build stopped
//! as a person names the file, the check of a build in sentences with no code in them, and a Builds window whose form and buttons are
//! on the screen of the smallest window.
//!
//! Read through AccessKit from whole frames of the shell.

use std::path::Path;
use std::time::{Duration, Instant};

use or_project::OutputKind;

use super::frames_of_the_shell::Shell;
use super::the_project_screens_on_screen::{a_project_shell, names, shows, until, write};
use crate::state::front_door::{NewProject, Template};
use crate::state::workbench::LastBuild;
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;
use crate::ui::panels::registry::ids;

const OUTPUT: &str = "fivem";
const SMALLEST: (u32, u32) = (1280, 720);
const C: &str = "mp_m_freemode_01_mp_m_tshirts";

/// A clothing project of three garments with nothing missing, so that it builds, and the Build tab on top of the bottom panels.
fn a_clothing_project_that_builds(dir: &Path) -> Shell {
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("tshirts".into());
    let file = draft.create().expect("a clothing project").file;
    let folder = file.parent().unwrap().to_path_buf();
    let mut dropped = Vec::new();
    for n in 0..3 {
        let jacket = format!("jacket {n}");
        dropped.push(write(
            &dir.join("drop").join(format!("{C}^jbib_{n:03}_u.ydd")),
            &or_source::sample::drawables_of(&jacket),
        ));
        dropped.push(write(
            &dir.join("drop")
                .join(format!("{C}^jbib_diff_{n:03}_a_uni.ytd")),
            &or_source::sample::textures_of(&format!("texture {n}")),
        ));
    }
    or_wardrobe::edit::add_clothes(&folder.join("clothing"), &dropped).expect("the garments");
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.state.active_workspace = Workspace::Project;
    shell
        .state
        .workspace_docks
        .reveal(Workspace::Project, ids::BUILD);
    until(&mut shell, "the garments", |s| {
        s.state.workbench.packs().first().is_some_and(|p| {
            p.read
                .as_ref()
                .is_ok_and(|read| read.wardrobe.garments.len() == 3)
        }) && !s.state.workbench.resources_of(OUTPUT).is_empty()
    });
    shell.settled_sized(SMALLEST);
    shell
}

/// Draw frames until a build of `OUTPUT` that ended after `after` is there, and settle the window.
fn until_built_after(shell: &mut Shell, after: Option<std::time::SystemTime>) {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        shell.state.poll_workbench();
        shell.draw_sized(SMALLEST, Vec::new());
        let ended = match shell.state.workbench.builds().last(OUTPUT) {
            Some(LastBuild::Built(done)) => after.is_none_or(|at| done.at > at),
            Some(LastBuild::Failed(_)) => after.is_none(),
            None => false,
        };
        if ended {
            break;
        }
        assert!(Instant::now() < deadline, "the build never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
    shell.settled_sized(SMALLEST);
}

fn built_at(shell: &Shell) -> Option<std::time::SystemTime> {
    match shell.state.workbench.builds().last(OUTPUT) {
        Some(LastBuild::Built(done)) => Some(done.at),
        _ => None,
    }
}

#[test]
fn what_a_build_changed_is_said_once_where_the_changes_are_and_never_as_nothing_beside_a_removal() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_project_that_builds(dir.path());
    shell.state.build_selected();
    until_built_after(&mut shell, None);
    assert!(
        !shows(&shell, "0 added, 0 changed, 0 moved, 0 removed"),
        "the first build has no earlier one, so a line of noughts is noise: {:?}",
        names(&shell)
    );

    let id = shell.state.workbench.packs()[0]
        .read
        .as_ref()
        .expect("a pack")
        .wardrobe
        .garments[2]
        .id
        .clone();
    shell.state.remove_garments(vec![id]);
    until(&mut shell, "the plan without the garment", |s| {
        s.state.workbench.resources_of(OUTPUT)[0]
            .since
            .as_ref()
            .is_some_and(|diff| !diff.is_empty())
    });
    shell.settled_sized(SMALLEST);
    assert!(
        shows(
            &shell,
            "Since the last build: 0 added, 0 changed, 0 moved, 1 removed"
        ),
        "what the next build would change is not said: {:?}",
        names(&shell)
    );

    let first = built_at(&shell);
    shell.state.build_selected();
    until_built_after(&mut shell, first);
    // The plan is read again once the build has ended, and then it differs from nothing.
    until(&mut shell, "the plan read again", |s| {
        (s.state.workbench.resources_of(OUTPUT).first())
            .is_some_and(|r| r.since.as_ref().is_some_and(|diff| diff.is_empty()))
    });
    shell.settled_sized(SMALLEST);
    let said = names(&shell);
    assert!(
        shows(
            &shell,
            "This build changed: 0 added, 0 changed, 0 moved, 1 removed"
        ),
        "the build that removed a garment does not say so: {said:?}"
    );
    assert!(
        !shows(&shell, "Since the last build: 0 added"),
        "a build that just removed a garment says that nothing changed: {said:?}"
    );
    assert!(
        !shows(&shell, "No change"),
        "the fold says no change beside a removal: {said:?}"
    );
    assert!(
        !shows(&shell, "It changed:"),
        "the same difference is said a second time under Verification: {said:?}"
    );
    assert!(
        shows(&shell, "removed  "),
        "the removed garment is not named in What changed: {said:?}"
    );
}

#[test]
fn the_check_of_a_build_is_in_sentences_with_no_markup_no_engine_type_and_no_file_of_the_tool() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_project_that_builds(dir.path());
    shell.state.build_selected();
    until_built_after(&mut shell, None);
    let said = names(&shell);
    for (what, why) in [
        ("`", "a backtick marks code in a document"),
        ("(s)", "a plural is spelled out"),
        (
            "wardrobe.lock.json",
            "a file of the tool is not a sentence a person reads",
        ),
        ("CPedVariationInfo", "the name of an engine type"),
        ("PgDictionary", "the name of an engine type"),
        ("GrcTexture", "the name of an engine type"),
        (
            dir.path().to_string_lossy().as_ref(),
            "a path of the person's disk",
        ),
    ] {
        assert!(
            !said.iter().any(|n| n.contains(what)),
            "the tab says `{what}` ({why}): {said:?}"
        );
    }
    assert!(
        !said
            .iter()
            .any(|n| n.starts_with("ok ") || n.starts_with("ok\u{a0}")),
        "a check that passed says ok in letters and not in a mark: {said:?}"
    );
    assert!(
        said.iter()
            .any(|n| n.contains("locked, so a later build cannot move")),
        "what freezing did is not said in a sentence: {said:?}"
    );
}

#[test]
fn a_build_that_stopped_names_the_file_as_the_project_does_and_the_plan_says_it_the_same_way() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    shell
        .state
        .workspace_docks
        .reveal(Workspace::Project, ids::BUILD);
    shell.state.build_selected();
    until_built_after(&mut shell, None);
    let said = names(&shell);
    assert!(
        said.iter()
            .any(|n| n.contains("missing_hat.ydd is missing")),
        "the reason is not the file's name and that it is missing: {said:?}"
    );
    let root = dir.path().to_string_lossy().into_owned();
    assert!(
        !said.iter().any(|n| n.contains(&root)),
        "the reason is a path of the person's disk: {said:?}"
    );
    assert!(
        !said.iter().any(|n| n.contains('`')),
        "a backtick is left in what the tab says: {said:?}"
    );
}

/// The Builds window over a Files project that declares a singleplayer DLC too, in the smallest window.
fn the_builds_window_with_a_dlc(dir: &Path) -> Shell {
    let mut shell = super::the_build_tab_on_screen::a_files_project(dir);
    shell
        .state
        .add_build_taking(
            "shopcase_dlc",
            OutputKind::SingleplayerDlc,
            None,
            &["files".into()],
        )
        .expect("a build");
    until(&mut shell, "the lock", |s| {
        s.state
            .workbench
            .cannot_ship("files", &OutputKind::SingleplayerDlc)
            .is_some()
    });
    shell.state.ask_about_builds(false);
    shell.settled_sized(SMALLEST);
    shell
}

fn bottom_of(shell: &Shell, name: &str) -> f32 {
    (shell.described.iter())
        .filter(|w| w.name.as_deref().is_some_and(|n| n.starts_with(name)))
        .map(|w| w.bounds.max.y)
        .min_by(f32::total_cmp)
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)))
}

#[test]
fn the_form_of_the_build_and_the_buttons_that_add_and_remove_are_on_screen_in_the_smallest_window()
{
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let shell = the_builds_window_with_a_dlc(dir.path());
    let window = SMALLEST.1 as f32;
    for name in ["Remove build", "Add a build…", "Done"] {
        assert!(
            bottom_of(&shell, name) <= window,
            "{name} is below the window, behind a scroll nobody is told of: {:?}",
            names(&shell)
        );
    }
    assert!(
        !shows(
            &shell,
            "A singleplayer DLC tells the game a type for each file"
        ),
        "File types is open and full height for a person who has no file to type: {:?}",
        names(&shell)
    );
    assert!(shows(&shell, "File types"), "{:?}", names(&shell));
    let add = (shell.described.iter()).find(|w| w.name.as_deref() == Some("Add a build…"));
    let done = (shell.described.iter()).find(|w| w.name.as_deref() == Some("Done"));
    let (Some(add), Some(done)) = (add, done) else {
        panic!("the footer lacks its buttons: {:?}", names(&shell));
    };
    assert!(
        (add.bounds.center().y - done.bounds.center().y).abs() < 1.0,
        "Add a build… is not pinned beside Done: {:?} {:?}",
        add.bounds,
        done.bounds
    );
}

#[test]
fn go_to_file_types_opens_the_fold_and_the_types_say_which_file_and_which_type() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = the_builds_window_with_a_dlc(dir.path());
    let at = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Go to File types"))
        .expect("the way to the types")
        .bounds
        .center();
    shell.click(at);
    shell.settled_sized(SMALLEST);
    let said = names(&shell);
    for text in ["File", "Type"] {
        assert!(
            said.iter().any(|n| n == text),
            "the two fields of File types have no visible label `{text}`: {said:?}"
        );
    }
    assert!(
        said.iter().any(|n| n.contains("Set type")),
        "File types lacks Set type: {said:?}"
    );
    assert!(
        said.iter().any(|n| n == "client.lua"),
        "the files that have no type are not listed to pick from: {said:?}"
    );
    assert!(
        !said.iter().any(|n| n == "State type"),
        "the button of the types is still called State type: {said:?}"
    );
    let lock = shell
        .state
        .workbench
        .cannot_ship("files", &OutputKind::SingleplayerDlc)
        .expect("a lock")
        .to_string();
    assert!(
        lock.starts_with("Build a FiveM resource to ship"),
        "the way out most people want is the last clause of the sentence: {lock}"
    );
}
