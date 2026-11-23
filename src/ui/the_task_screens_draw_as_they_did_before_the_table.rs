//! The Project and Wardrobe screens draw what they drew before the table of task screens: every named widget AccessKit describes, with its rectangle, in a 1280 by 720 window, against the record in `screens_before_the_table_of_task_screens.txt`.
//!
//! ⚠ The record was taken on the tree before the Wardrobe moved onto the table. A change to what these screens draw on purpose records it again with
//! `OPENRAGE_RECORD_SCREENS=1 cargo nextest run --bin openrage the_task_screens_draw_as_they_did`, and the pull request says what the new record shows.
//! The record holds no text of the machine: a path is `<path>`, and git's line ends do not matter.

use std::fmt::Write as _;
use std::path::Path;

use super::frames_of_the_shell::{key, Described, Shell};
use super::the_front_door_on_screen::{a_first_launch, a_project, waits_for};
use super::the_project_screens_on_screen::{
    a_project_shell, centre_of, click_nearest, names, right_click_on, the_questions, until,
};
use crate::state::front_door::{NewProject, Step, Template};
use crate::state::ThemeChoice;
use crate::ui::commands;
use crate::ui::dock::Workspace;

const RECORD: &str = include_str!("screens_before_the_table_of_task_screens.txt");
const SMALLEST: (u32, u32) = (1280, 720);

/// Whether `now` is the record, which git may have checked out with CRLF line ends.
fn same_as_record(record: &str, now: &str) -> bool {
    record.replace("\r\n", "\n") == now
}

/// The bottom bar of the window, which a list above it does not push.
const STATUS_BAR: f32 = 24.0;

/// The most lines a path may break a message into before the list below it is pushed into the bottom bar.
const MOST_LINES: usize = 2;

/// What a widget that names a path says with the path left out, because a path is the machine's: where its scratch folder is and how long that is.
fn without_the_path(said: &str) -> Option<String> {
    if let Some((head, _)) = said.split_once(" is missing:") {
        return Some(format!("{head} is missing: <path>"));
    }
    (said.ends_with(".openrage-proj") && said.contains(['\\', '/', '…']))
        .then(|| "<path>".to_string())
}

/// What `described` drew, one line a named widget.
///
/// ⚠ A path breaks across lines where the machine's scratch folder is long, and the list below it moves down by the lines it added;
/// those lines are taken out again, so the record is of a message on one line whatever the folder is.
fn drawn(described: &[Described]) -> String {
    let bar = SMALLEST.1 as f32 - STATUS_BAR;
    let added: Vec<(f32, f32)> = (described.iter())
        .filter_map(|w| {
            let said = w.name.as_deref()?;
            let lines = said.matches('\n').count() + 1;
            if lines == 1 || without_the_path(said).is_none() {
                return None;
            }
            assert!(
                lines <= MOST_LINES,
                "a path broke into {lines} lines, and the record takes out {MOST_LINES} at most: draw in a shorter scratch folder"
            );
            let row = w.bounds.height() / lines as f32;
            Some((w.bounds.max.y, row * (lines - 1) as f32))
        })
        .collect();
    let mut lines: Vec<String> = (described.iter())
        .filter_map(|w| {
            let said = w.name.as_deref()?;
            let role = format!(
                "{:?}{}",
                w.role,
                if w.disabled { " (disabled)" } else { "" }
            );
            // ⚠ What a path measures is not part of the record, and nor is what a file to be made is called.
            if let Some(name) = without_the_path(said) {
                return Some(format!("{role} {name:?}"));
            }
            if said.contains(".openrage-proj") {
                return Some(format!("{role} {said:?}"));
            }
            let b = w.bounds;
            let up: f32 = (added.iter())
                .filter(|(bottom, _)| b.min.y >= bottom - 0.5 && b.min.y < bar)
                .map(|(_, rows)| rows)
                .sum();
            Some(format!(
                "{role} {said:?} {:.1} {:.1} {:.1} {:.1}",
                b.min.x,
                b.min.y - up,
                b.max.x,
                b.max.y - up
            ))
        })
        .collect();
    lines.sort();
    lines.join("\n")
}

fn settled(shell: &mut Shell) {
    shell.settled_sized(SMALLEST);
}

/// The palette's entries for the workspaces and the buttons that pin them, as a person reads them.
fn workspace_commands(shell: &Shell) -> String {
    let mut text = String::new();
    for command in commands::workspaces(&shell.state)
        .into_iter()
        .chain(commands::workspace_pins(&shell.state))
    {
        let _ = writeln!(
            text,
            "{:?} | {} | {:?} | {:?} | {:?}",
            command.id, command.label, command.hover, command.checked, command.binding
        );
    }
    text.trim_end().to_string()
}

/// The screens drawn so far, each with what it drew.
struct Record {
    screens: Vec<(String, String)>,
}

impl Record {
    fn add(&mut self, name: &str, shell: &Shell) {
        self.screens
            .push((name.to_string(), drawn(&shell.described)));
    }
}

/// A scratch folder named `name` inside `dir`, which a project is made in.
fn folder(dir: &Path, name: &str) -> std::path::PathBuf {
    let made = dir.join(name);
    std::fs::create_dir_all(&made).unwrap();
    made
}

fn the_screens(dir: &Path) -> Vec<(String, String)> {
    let mut record = Record {
        screens: Vec::new(),
    };

    // The Project workspace of a clothing project, and every question it asks.
    let mut shell = a_project_shell(&folder(dir, "project"), ThemeChoice::Dark);
    settled(&mut shell);
    record.add("project", &shell);
    let content = names(&shell)
        .into_iter()
        .find(|n| n.starts_with("Clothing") && n.contains("garment") && !n.contains('\n'))
        .expect("a row for the clothing content");
    for (what, target) in [
        ("a content", content),
        ("an asset", "client.lua".to_string()),
    ] {
        let at = right_click_on(&mut shell, &target);
        click_nearest(&mut shell, "Open in", at);
        settled(&mut shell);
        record.add(&format!("project, the menu of {what}"), &shell);
        shell.draw(vec![key(egui::Key::Escape)]);
        shell.draw(vec![key(egui::Key::Escape)]);
        settled(&mut shell);
    }
    for (title, ask) in the_questions(&shell) {
        ask(&mut shell);
        settled(&mut shell);
        record.add(&format!("project, {title}"), &shell);
        shell.draw(vec![key(egui::Key::Escape)]);
        settled(&mut shell);
    }
    let commands_text = workspace_commands(&shell);
    record
        .screens
        .push(("the workspace commands".to_string(), commands_text));

    // The Wardrobe, and the menu of a garment.
    let mut shell = a_project_shell(&folder(dir, "wardrobe"), ThemeChoice::Dark);
    shell.state.active_workspace = Workspace::Wardrobe;
    until(&mut shell, "the garments", |s| {
        names(s).iter().any(|n| n.contains("jbib_000"))
    });
    settled(&mut shell);
    record.add("wardrobe", &shell);
    let at = centre_of(&shell, "jbib_000");
    shell.click(at);
    settled(&mut shell);
    record.add("wardrobe, a garment chosen", &shell);
    right_click_on(&mut shell, "jbib_000");
    settled(&mut shell);
    record.add("wardrobe, the menu of a garment", &shell);
    shell.draw(vec![key(egui::Key::Escape)]);
    settled(&mut shell);

    // The Wardrobe with nothing to show: a project that holds no clothing, on the screen of clothing.
    let file = a_project(&folder(dir, "files"), "plain", Template::Files);
    let mut shell = a_first_launch();
    shell.state.load_project_from(file);
    shell.state.active_workspace = Workspace::Wardrobe;
    settled(&mut shell);
    record.add("wardrobe, no clothing", &shell);
    shell.state.active_workspace = Workspace::Project;
    settled(&mut shell);
    record.add("project, no clothing", &shell);

    // The start page with a clothing project's card, and the dialog that makes one.
    let clothes = a_project(&folder(dir, "start"), "clothes", Template::Clothing);
    let mut shell = a_first_launch();
    shell.state.prefs.touch_recent_project(clothes);
    shell.draw(Vec::new());
    waits_for(&mut shell, "Not built");
    settled(&mut shell);
    record.add("the start page", &shell);
    for step in [Step::Gallery, Step::Details] {
        let mut draft = NewProject::new(Template::Clothing, folder(dir, "new"));
        draft.set_name("police".into());
        draft.step = step;
        shell.state.front_door.new_project = Some(draft);
        for _ in 0..12 {
            shell.draw_sized(SMALLEST, Vec::new());
        }
        record.add(&format!("new project, {step:?}"), &shell);
        shell.draw(vec![key(egui::Key::Escape)]);
    }
    record.screens
}

fn written(screens: &[(String, String)]) -> String {
    screens
        .iter()
        .map(|(name, body)| format!("## {name}\n{body}\n"))
        .collect()
}

fn first_difference(before: &str, now: &str) -> String {
    let (before, now): (Vec<&str>, Vec<&str>) = (before.lines().collect(), now.lines().collect());
    let at = (0..before.len().max(now.len()))
        .find(|i| before.get(*i) != now.get(*i))
        .unwrap_or(0);
    let screen = (before.iter().take(at + 1))
        .rev()
        .find(|line| line.starts_with("## "))
        .unwrap_or(&"");
    format!(
        "{screen}, line {at}\n  before: {:?}\n  now:    {:?}",
        before.get(at),
        now.get(at)
    )
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "the record is of the screens as Windows and Linux draw them: a Mac names a key Cmd, and a label is as wide as its text"
)]
fn the_task_screens_draw_as_they_did_before_the_table() {
    // ⚠ The record is English whatever language the machine speaks or a test of this process chose.
    let _english = crate::i18n::pin("en");
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let now = written(&the_screens(dir.path()));
    if std::env::var_os("OPENRAGE_RECORD_SCREENS").is_some() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/ui/screens_before_the_table_of_task_screens.txt");
        std::fs::write(path, &now).unwrap();
        return;
    }
    assert!(
        RECORD.trim().len() > 1000,
        "the record of the screens is empty: record it"
    );
    if !same_as_record(RECORD, &now) {
        panic!(
            "a screen draws differently from the record; OPENRAGE_RECORD_SCREENS=1 writes what is drawn now into it, and git shows the difference\n{}",
            first_difference(RECORD, &now)
        );
    }
}

#[test]
fn a_record_checked_out_with_crlf_line_ends_is_still_the_record() {
    assert!(same_as_record(
        "## a\r\nLabel \"b\"\r\n",
        "## a\nLabel \"b\"\n"
    ));
    assert!(!same_as_record(
        "## a\r\nLabel \"b\"\r\n",
        "## a\nLabel \"c\"\n"
    ));
}

#[test]
fn a_path_is_left_out_of_what_names_one_however_it_breaks_across_lines() {
    for said in [
        r"gone.ydd is missing: C:\Temp\.tmpA\project\gone.ydd",
        "gone.ydd is missing: /tmp/.tmpA/project/gone.ydd",
        "gone.ydd is missing:\n/tmp/.tmpA/project/gone.ydd",
        "gone.ydd is missing: /tmp/.tmpA/\nproject/gone.ydd",
    ] {
        assert_eq!(
            without_the_path(said).as_deref(),
            Some("gone.ydd is missing: <path>"),
            "{said:?}"
        );
    }
    assert_eq!(
        without_the_path(r"C:\Users\x\AppData\Local\Temp\.…tart\clothes\clothes.openrage-proj")
            .as_deref(),
        Some("<path>")
    );
    assert_eq!(without_the_path("+   police.openrage-proj"), None);
    assert_eq!(without_the_path("Fix…"), None);
}

/// A message about a missing file on `lines` lines, and the button of the next row under it.
fn a_message_and_the_row_after_it(lines: usize) -> Vec<Described> {
    let at = |x0: f32, y0: f32, x1: f32, y1: f32| {
        egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1))
    };
    let said = ["gone.ydd is missing:", "/tmp/.tmpA/", "project/gone.ydd"][..lines].join("\n");
    let described = |role, name: &str, bounds| Described {
        role,
        name: Some(name.to_string()),
        bounds,
        disabled: false,
    };
    vec![
        described(
            egui::accesskit::Role::Label,
            &said,
            at(288.0, 543.5, 900.0, 558.5 + 15.0 * (lines - 1) as f32),
        ),
        described(
            egui::accesskit::Role::Button,
            "Fix…",
            at(
                896.8,
                589.5 + 15.0 * (lines - 1) as f32,
                942.0,
                606.5 + 15.0 * (lines - 1) as f32,
            ),
        ),
        described(
            egui::accesskit::Role::Button,
            "Build",
            at(144.2, 698.0, 295.6, 718.0),
        ),
    ]
}

#[test]
fn a_message_that_a_long_path_breaks_leaves_the_rows_after_it_where_they_are() {
    let on_one_line = drawn(&a_message_and_the_row_after_it(1));
    assert!(on_one_line.contains("Button \"Fix…\" 896.8 589.5 942.0 606.5"));
    assert!(on_one_line.contains("Button \"Build\" 144.2 698.0 295.6 718.0"));
    assert_eq!(drawn(&a_message_and_the_row_after_it(2)), on_one_line);
}

#[test]
fn the_record_holds_no_text_of_the_machine_it_was_taken_on() {
    for machine in [
        ":\\\\",
        "Users\\\\",
        "AppData",
        "/home/",
        "/Users/",
        "/tmp/",
        "/var/",
    ] {
        assert!(
            !RECORD.contains(machine),
            "the record names `{machine}`, which is a folder of the machine that took it"
        );
    }
}
