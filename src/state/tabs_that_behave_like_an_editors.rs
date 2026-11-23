//! Tabs that behave like an editor's: every document has one, with a close
//! button and one unsaved mark; an archive entry's names its archive; a file
//! opened twice has one; a single click in Files opens a preview tab; and
//! `Ctrl+Tab` and `Ctrl+Shift+T` go back to tabs used and closed before.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use super::document::{DocumentId, RpfNodeId, TextDocument};
use super::navigation::Arrival;
use super::nested_archives_survive_a_save::archive;
use super::opening_a_ydd::{finish_load, node_at};
use super::two_readers_of_one_archive_across_a_save::install;
use super::{AppState, DocPayload, DocumentKind, Prefs};
use crate::ui::dock::Workspace;
use crate::ui::{commands, shortcuts};

/// The tab strip and Files, drawn as the editor draws them, on a screen of its own.
struct Screen {
    ctx: egui::Context,
    size: egui::Vec2,
    time: f64,
}

/// A text one frame painted in the tab strip, and whether it was set in italics.
type Painted = (String, bool);

impl Screen {
    fn new(width: f32) -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        Self {
            ctx,
            size: egui::vec2(width, 900.0),
            time: 0.0,
        }
    }

    fn frame_with(
        &mut self,
        state: &mut AppState,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> egui::FullOutput {
        self.time += 0.05;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
            time: Some(self.time),
            modifiers,
            events,
            ..Default::default()
        };
        self.ctx.run_ui(input, |ui| {
            crate::ui::panels::document_tabs::draw(ui, state);
            crate::ui::panels::hierarchy::draw(ui, state);
        })
    }

    fn frame(&mut self, state: &mut AppState) -> egui::FullOutput {
        self.frame_with(state, Vec::new(), egui::Modifiers::NONE)
    }

    /// Every labelled widget in the strip, with its bounds.
    fn tabs(&mut self, state: &mut AppState) -> Vec<(String, egui::Rect)> {
        self.frame(state);
        let output = self.frame(state);
        let strip = crate::ui::theme::TAB_STRIP_HEIGHT;
        output
            .platform_output
            .accesskit_update
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        let b = node.bounds()?;
                        let rect = egui::Rect::from_min_max(
                            egui::pos2(b.x0 as f32, b.y0 as f32),
                            egui::pos2(b.x1 as f32, b.y1 as f32),
                        );
                        let label = node.label()?.to_string();
                        (rect.center().y < strip).then_some((label, rect))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Every text the strip painted.
    fn painted(&mut self, state: &mut AppState) -> Vec<Painted> {
        fn walk(shape: &egui::Shape, strip: f32, out: &mut Vec<Painted>) {
            match shape {
                egui::Shape::Text(text) if text.pos.y < strip => {
                    let italics = text.galley.job.sections.iter().any(|s| s.format.italics);
                    out.push((text.galley.text().to_string(), italics));
                }
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, strip, out)),
                _ => {}
            }
        }
        self.frame(state);
        let output = self.frame(state);
        let mut out = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, crate::ui::theme::TAB_STRIP_HEIGHT, &mut out);
        }
        out
    }

    /// `times` presses and releases of `button` at `at`, close enough to make a double click of two.
    fn click(
        &mut self,
        state: &mut AppState,
        at: egui::Pos2,
        button: egui::PointerButton,
        times: usize,
    ) {
        // ⚠ Far enough from the last click that egui does not take it for a double click.
        self.time += 1.0;
        self.frame_with(
            state,
            vec![egui::Event::PointerMoved(at)],
            egui::Modifiers::NONE,
        );
        for _ in 0..times {
            for pressed in [true, false] {
                let event = egui::Event::PointerButton {
                    pos: at,
                    button,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                self.frame_with(state, vec![event], egui::Modifiers::NONE);
            }
        }
    }

    fn click_row(&mut self, state: &mut AppState, doc: DocumentId, row: RpfNodeId) {
        self.frame(state);
        let at = crate::ui::panels::hierarchy::ui_state(state)
            .row_rects
            .iter()
            .find(|(d, n, _)| *d == doc && *n == row)
            .map(|(_, _, rect)| rect.center())
            .expect("Files drew the row");
        self.click(state, at, egui::PointerButton::Primary, 1);
    }
}

/// Where the first labelled widget in the strip naming `name` is.
fn tab_named(tabs: &[(String, egui::Rect)], name: &str) -> Option<egui::Rect> {
    tabs.iter()
        .find(|(label, _)| label.contains(name))
        .map(|(_, rect)| *rect)
}

/// Polls as the editor's frame does until no load or walk is under way.
fn settle(state: &mut AppState) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        state.poll_load_result();
        state.poll_expansions();
        state.tick_navigation();
        if state.load_receiver.is_none() && state.navigation.pending_place().is_none() {
            return;
        }
        assert!(Instant::now() < deadline, "the open never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A text file at `path`, open and unedited.
fn text_tab(state: &mut AppState, path: &Path) -> DocumentId {
    state.insert_document(
        path.to_path_buf(),
        DocPayload::Text(TextDocument::decode(b"text")),
        None,
        None,
    )
}

fn open_docs(state: &AppState, path: &Path) -> usize {
    state
        .documents
        .iter()
        .filter(|d| d.path.as_deref() == Some(path))
        .count()
}

/// Runs what a key press with `mods` runs, as the window's key handler does; whether it ran anything.
fn press(
    state: &mut AppState,
    key: Key,
    code: KeyCode,
    mods: ModifiersState,
    typing: bool,
) -> bool {
    let action =
        shortcuts::action_for_press(&key, PhysicalKey::Code(code), mods, typing, &state.prefs);
    if let Some(action) = action {
        commands::run_action(state, &egui::Context::default(), action);
    }
    action.is_some()
}

fn ctrl_tab(state: &mut AppState) -> bool {
    let tab = Key::Named(NamedKey::Tab);
    press(state, tab, KeyCode::Tab, ModifiersState::CONTROL, false)
}

#[test]
fn opening_one_file_twice_leaves_one_tab() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let notes = dir.path().join("notes.txt");
    std::fs::write(&notes, b"notes").expect("the file on disk");
    let mut state = AppState::from_prefs(Prefs::default());

    state.load_file(notes.clone());
    state.load_file(notes.clone());
    settle(&mut state);
    state.load_file(notes.clone());
    settle(&mut state);
    let recent = commands::recent_files(&state)
        .into_iter()
        .find(|c| c.hover.as_deref() == Some(notes.to_string_lossy().as_ref()))
        .expect("the file is a recent one");
    commands::run(&mut state, &egui::Context::default(), &recent);
    settle(&mut state);
    state.walk_to(&notes, vec![String::new()], Arrival::Open);
    settle(&mut state);
    assert_eq!(
        open_docs(&state, &notes),
        1,
        "opened from disk, again, from recents and by a walk"
    );
    let mut screen = Screen::new(1600.0);
    let tabs = screen.tabs(&mut state);
    let named = tabs.iter().filter(|(l, _)| l.contains("notes.txt")).count();
    assert_eq!(named, 1, "the strip: {tabs:?}");

    let root = tempfile::tempdir().expect("a scratch install");
    let (mut state, tree) = install(root.path(), &[("a.rpf", archive(&[("b.txt", b"b")]))]);
    let row = node_at(&mut state, tree, "a.rpf/b.txt");
    state.open_rpf_entry(tree, row);
    state.open_rpf_entry(tree, row);
    settle(&mut state);
    state.walk_to(root.path(), vec!["a.rpf/b.txt".into()], Arrival::Open);
    settle(&mut state);
    let opened = state
        .documents
        .iter()
        .filter(|d| d.rpf_entry_binding.is_some_and(|b| b.entry_id == row))
        .count();
    assert_eq!(opened, 1, "an archive entry opened twice and by a walk");
}

#[test]
fn an_archive_entry_has_a_tab_named_with_its_archive_and_closing_it_keeps_the_archive() {
    let vehicles = archive(&[("adder.txt", b"the adder")]);
    let root = tempfile::tempdir().expect("a scratch install");
    let (mut state, tree) = install(
        root.path(),
        &[("x64e.rpf", archive(&[("levels/vehicles.rpf", &vehicles)]))],
    );
    let row = node_at(&mut state, tree, "x64e.rpf/levels/vehicles.rpf/adder.txt");
    state.open_rpf_entry(tree, row);
    let entry = finish_load(&mut state).expect("the entry opens");

    let mut screen = Screen::new(1600.0);
    let tabs = screen.tabs(&mut state);
    let tab = tab_named(&tabs, "adder.txt — vehicles.rpf");
    assert!(tab.is_some(), "no tab names the entry's archive: {tabs:?}");

    let close = tabs
        .iter()
        .find(|(label, rect)| label == "Close" && tab.is_some_and(|t| t.contains(rect.center())))
        .map(|(_, rect)| rect.center())
        .expect("the entry's tab has a close button");
    screen.click(&mut state, close, egui::PointerButton::Primary, 1);
    assert!(
        state.document(entry).is_none(),
        "the close button left it open"
    );
    let kept = match state.document(tree).map(|d| &d.payload) {
        Some(DocPayload::Rpf(rpf)) => rpf.session.node(row).is_some(),
        _ => false,
    };
    assert!(kept, "closing the entry's tab closed its archive");
}

#[test]
fn every_tab_has_a_close_button_and_those_that_do_not_fit_are_in_a_list() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mut state = AppState::from_prefs(Prefs::default());
    let only = text_tab(&mut state, &dir.path().join("only.txt"));
    let mut screen = Screen::new(900.0);
    let tabs = screen.tabs(&mut state);
    assert!(
        tab_named(&tabs, "only.txt").is_some(),
        "one document has no tab: {tabs:?}"
    );
    let close = tabs
        .iter()
        .find(|(label, _)| label == "Close")
        .map(|(_, rect)| rect.center())
        .expect("the tab has a close button");
    state.document_mut(only).expect("open").mark_edited();
    screen.click(&mut state, close, egui::PointerButton::Primary, 1);
    assert!(
        state.document(only).is_some() && state.pending_close.is_some(),
        "the close button dropped unsaved work without asking"
    );
    state.answer_close(super::CloseAnswer::DontSave);
    assert!(state.document(only).is_none());

    let docs: Vec<DocumentId> = (0..30)
        .map(|n| {
            text_tab(
                &mut state,
                &dir.path().join(format!("handling_notes_{n:02}.txt")),
            )
        })
        .collect();
    let tabs = screen.tabs(&mut state);
    let past_the_edge: Vec<&String> = tabs
        .iter()
        .filter(|(_, rect)| rect.right() > 900.0)
        .map(|(label, _)| label)
        .collect();
    assert!(
        past_the_edge.is_empty(),
        "the strip runs off the window: {past_the_edge:?}"
    );
    assert!(
        tab_named(&tabs, "handling_notes_29.txt").is_some(),
        "the active tab is not on screen: {tabs:?}"
    );
    let list = tabs
        .iter()
        .find(|(label, _)| label.starts_with(egui_phosphor::regular::CARET_DOWN))
        .map(|(_, rect)| rect.center())
        .expect("the tabs that do not fit are in a list at the strip's end");
    screen.click(&mut state, list, egui::PointerButton::Primary, 1);
    let listed = screen.frame(&mut state);
    let hidden = listed
        .platform_output
        .accesskit_update
        .and_then(|update| {
            update.nodes.iter().find_map(|(_, node)| {
                let b = node.bounds()?;
                let listed = node.label()?.contains("handling_notes_15.txt")
                    && f64::from(crate::ui::theme::TAB_STRIP_HEIGHT) < b.y0;
                listed
                    .then(|| egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))
            })
        })
        .expect("the list names a tab that does not fit");
    screen.click(&mut state, hidden, egui::PointerButton::Primary, 1);
    assert_eq!(state.active_document, Some(docs[15]), "the listed tab");

    let middle = tab_named(&screen.tabs(&mut state), "handling_notes_15.txt")
        .expect("the active tab is on screen")
        .center();
    screen.click(&mut state, middle, egui::PointerButton::Middle, 1);
    assert!(
        state.document(docs[15]).is_none(),
        "a middle click left it open"
    );
}

#[test]
fn every_unsaved_tab_carries_one_mark_and_pose_keys_not_yet_baked_are_unsaved() {
    let mut state = AppState::from_prefs(Prefs::default());
    let edited = text_tab(&mut state, Path::new("C:/mods/edited.txt"));
    state.document_mut(edited).expect("open").mark_edited();
    text_tab(&mut state, Path::new("C:/mods/clean.txt"));
    let clip = state.insert_document(
        PathBuf::from("C:/mods/walk.ycd"),
        DocPayload::Resource(or_io::Asset::Clips(Default::default())),
        None,
        None,
    );
    state.preview_scene.set_clip(clip, 0x1234);
    state.anim_edits.set(
        or_anim::EditKey {
            bone: 0,
            track: or_anim::EditTrack::Translation,
            frame: 1,
        },
        or_anim::EditValue::Vector(glam::Vec3::X),
    );

    let mut screen = Screen::new(1600.0);
    let painted = screen.painted(&mut state);
    let marks: usize = painted.iter().map(|(t, _)| t.matches('●').count()).sum();
    assert_eq!(
        marks, 2,
        "one mark for each of two unsaved tabs: {painted:?}"
    );
    assert!(
        !painted.iter().any(|(t, _)| t.contains(" *")),
        "a tab marks unsaved work twice: {painted:?}"
    );
    let tabs = screen.tabs(&mut state);
    for (name, unsaved) in [
        ("edited.txt", true),
        ("clean.txt", false),
        ("walk.ycd", true),
    ] {
        let label = tabs
            .iter()
            .map(|(l, _)| l)
            .find(|l| l.contains(name))
            .unwrap_or_else(|| panic!("no tab for {name}: {tabs:?}"));
        assert_eq!(label.contains('●'), unsaved, "{label}");
    }
}

#[test]
fn a_single_click_in_files_opens_a_preview_tab_the_next_replaces_and_a_double_click_keeps() {
    let mut state = AppState::from_prefs(Prefs::default());
    let archive = state.new_empty_document(DocumentKind::Rpf);
    let rows: Vec<RpfNodeId> = {
        let Some(DocPayload::Rpf(rpf)) = state.document_mut(archive).map(|d| &mut d.payload) else {
            panic!("an empty archive document holds an archive");
        };
        let root = rpf.session.root_id();
        ["one.txt", "two.txt", "three.txt"]
            .into_iter()
            .map(|name| {
                rpf.session
                    .add_file_copy(root, name, name.as_bytes().to_vec())
                    .expect("a file in the archive")
            })
            .collect()
    };
    state.set_active_document(archive);
    let mut screen = Screen::new(1600.0);
    screen.frame(&mut state);
    state.active_workspace = Workspace::Data;

    let shown = |screen: &mut Screen, state: &mut AppState, name: &str| {
        screen
            .painted(state)
            .into_iter()
            .find(|(text, _)| text.contains(name))
            .map(|(_, italics)| italics)
    };
    screen.click_row(&mut state, archive, rows[0]);
    settle(&mut state);
    assert_eq!(
        shown(&mut screen, &mut state, "one.txt"),
        Some(true),
        "a single click in Data opened no preview tab"
    );
    assert_eq!(state.active_workspace, Workspace::Data);

    screen.click_row(&mut state, archive, rows[1]);
    settle(&mut state);
    assert_eq!(shown(&mut screen, &mut state, "one.txt"), None, "replaced");
    assert_eq!(shown(&mut screen, &mut state, "two.txt"), Some(true));

    let two = tab_named(&screen.tabs(&mut state), "two.txt")
        .expect("its tab")
        .center();
    screen.click(&mut state, two, egui::PointerButton::Primary, 2);
    assert_eq!(
        shown(&mut screen, &mut state, "two.txt"),
        Some(false),
        "a double click on the preview tab kept it"
    );

    screen.click_row(&mut state, archive, rows[2]);
    settle(&mut state);
    assert_eq!(
        shown(&mut screen, &mut state, "two.txt"),
        Some(false),
        "kept"
    );
    assert_eq!(shown(&mut screen, &mut state, "three.txt"), Some(true));
}

#[test]
fn ctrl_tab_returns_to_the_tab_used_before_and_with_ctrl_held_goes_further_back() {
    let mut state = AppState::from_prefs(Prefs::default());
    let mut screen = Screen::new(1600.0);
    let docs: Vec<DocumentId> = ["a.txt", "b.txt", "c.txt"]
        .into_iter()
        .map(|name| {
            let id = text_tab(&mut state, &Path::new("C:/mods").join(name));
            screen.frame(&mut state);
            id
        })
        .collect();
    let tab = || Key::Named(NamedKey::Tab);
    assert!(
        press(
            &mut state,
            tab(),
            KeyCode::Tab,
            ModifiersState::CONTROL,
            true
        ),
        "Ctrl+Tab is not the app's while a field has focus"
    );
    assert_eq!(state.active_document, Some(docs[1]), "the tab used before");

    let ctrl = egui::Modifiers::CTRL;
    screen.frame_with(&mut state, Vec::new(), ctrl);
    assert!(ctrl_tab(&mut state));
    screen.frame_with(&mut state, Vec::new(), ctrl);
    assert_eq!(state.active_document, Some(docs[0]), "Tab again, Ctrl held");

    screen.frame(&mut state);
    assert!(ctrl_tab(&mut state));
    assert_eq!(
        state.active_document,
        Some(docs[2]),
        "once Ctrl is let go, the tab used before is the one left"
    );
    screen.frame(&mut state);
    assert!(ctrl_tab(&mut state));
    assert_eq!(state.active_document, Some(docs[0]));
}

#[test]
fn ctrl_shift_t_reopens_the_tab_closed_last() {
    let root = tempfile::tempdir().expect("a scratch install");
    let loose = root.path().join("notes.txt");
    std::fs::write(&loose, b"notes").expect("a file of its own");
    let (mut state, tree) = install(root.path(), &[("a.rpf", archive(&[("b.txt", b"b")]))]);
    let row = node_at(&mut state, tree, "a.rpf/b.txt");
    state.open_rpf_entry(tree, row);
    let entry = finish_load(&mut state).expect("the entry opens");
    state.load_file(loose.clone());
    let file = finish_load(&mut state).expect("the file opens");

    state.close_document(entry);
    state.close_document(file);
    assert!(state.document(entry).is_none() && state.document(file).is_none());

    let reopen = |state: &mut AppState| {
        let t = Key::Character("T".into());
        let mods = crate::ui::shortcuts::PRIMARY_STATE | ModifiersState::SHIFT;
        let ran = press(state, t, KeyCode::KeyT, mods, true);
        settle(state);
        ran
    };
    assert!(
        reopen(&mut state),
        "Ctrl+Shift+T is not the app's while a field has focus"
    );
    assert_eq!(open_docs(&state, &loose), 1, "the tab closed last");
    assert_eq!(
        state
            .active_document
            .and_then(|id| state.document(id)?.path.clone()),
        Some(loose)
    );
    assert!(reopen(&mut state));
    let entry_back = state
        .documents
        .iter()
        .find(|d| {
            d.rpf_entry_binding
                .is_some_and(|b| b.parent_doc_id == tree && b.entry_id == row)
        })
        .map(|d| d.id);
    assert!(entry_back.is_some(), "the archive entry closed before it");
    assert_eq!(state.active_document, entry_back);
}
