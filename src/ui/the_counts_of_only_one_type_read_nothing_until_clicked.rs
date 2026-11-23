//! The submenu Only one type lists the types the tree already holds and reads no
//! archive until Count types in archives… is clicked.
//!
//! ⚠ Driven through the real menu with pointer events and read through
//! AccessKit, as `every_context_menu_draws_from_the_registry` does: a count
//! started by drawing the submenu is what this exists to catch.

use std::time::{Duration, Instant};

use or_io::container::rpf::RpfBuilder;

use super::every_context_menu_draws_from_the_registry::{find, frame, in_files, shell, Drawn};
use crate::state::{AppState, DocPayload, DocumentId, Prefs, RpfNodeId};

/// An unencrypted archive holding `bytes` as `path`.
fn archive_of(path: &str, bytes: &[u8]) -> Vec<u8> {
    RpfBuilder::default()
        .insert_with(path, bytes, false)
        .expect("a file in the archive")
        .to_bytes()
        .expect("the archive is written")
}

fn button(at: egui::Pos2, pressed: bool, which: egui::PointerButton) -> egui::Event {
    egui::Event::PointerButton {
        pos: at,
        button: which,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

/// A click at `at`, drawn as three frames, and what the last one drew.
fn click(
    ctx: &egui::Context,
    state: &mut AppState,
    at: egui::Pos2,
    which: egui::PointerButton,
) -> Vec<Drawn> {
    let moved = egui::Event::PointerMoved(at);
    frame(ctx, state, vec![moved, button(at, true, which)], shell);
    frame(ctx, state, vec![button(at, false, which)], shell);
    frame(ctx, state, Vec::new(), shell)
}

/// The centre of the widget whose label starts with `label`, in the frame `drawn`.
fn item(drawn: &[Drawn], label: &str) -> egui::Pos2 {
    (drawn.iter())
        .find(|d| d.label.starts_with(label))
        .unwrap_or_else(|| {
            let labels: Vec<_> = drawn.iter().map(|d| &d.label).collect();
            panic!("nothing labelled {label} was drawn: {labels:?}")
        })
        .centre
}

fn drawn_now(ctx: &egui::Context, state: &mut AppState) -> Vec<Drawn> {
    frame(ctx, state, Vec::new(), shell)
}

fn settle(state: &mut AppState, doc: DocumentId) {
    let until = Instant::now() + Duration::from_secs(30);
    while state.expander.is_listing(doc) {
        assert!(Instant::now() < until, "the tree's scan never finished");
        state.poll_expansions();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn rows_in(state: &AppState, doc: DocumentId) -> usize {
    match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Rpf(rpf)) => rpf.session.all_nodes().len(),
        _ => panic!("the folder's tree"),
    }
}

fn child(state: &AppState, doc: DocumentId, parent: RpfNodeId, name: &str) -> RpfNodeId {
    let Some(DocPayload::Rpf(rpf)) = state.document(doc).map(|d| &d.payload) else {
        panic!("the folder's tree");
    };
    (rpf.session.children_of(parent).into_iter())
        .find(|&id| rpf.session.node(id).is_some_and(|n| n.name == name))
        .unwrap_or_else(|| panic!("the tree has no {name}"))
}

#[test]
fn the_submenu_of_only_one_type_reads_no_archive_until_count_types_is_clicked() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).expect("the folder");
    std::fs::write(sub.join("one.txt"), b"one").expect("a file");
    for (name, inside) in [("x.rpf", "a.txt"), ("y.rpf", "b.txt")] {
        std::fs::write(sub.join(name), archive_of(inside, b"inside")).expect("an archive");
    }
    let mut state = AppState::from_prefs(Prefs::default());
    state.prefs.explorer = Default::default();
    state.open_folder(dir.path().to_path_buf());
    let doc = state.active_document.expect("the folder opens");
    let root = match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Rpf(rpf)) => rpf.session.root_id(),
        _ => panic!("the folder's tree"),
    };
    state.request_expand(doc, root);
    settle(&mut state, doc);
    let folder = child(&state, doc, root, "sub");
    state.request_expand(doc, folder);
    settle(&mut state, doc);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();

    let at = find(&ctx, &mut state, shell, in_files, "sub");
    let menu = click(&ctx, &mut state, at, egui::PointerButton::Secondary);
    let more = item(&menu, "More extract options");
    let menu = click(&ctx, &mut state, more, egui::PointerButton::Primary);
    let one = item(&menu, "Only one type");
    let before = rows_in(&state, doc);
    let submenu = click(&ctx, &mut state, one, egui::PointerButton::Primary);

    assert!(
        submenu.iter().any(|d| d.label == ".txt — 1"),
        "the submenu lists what the tree holds: {:?}",
        submenu.iter().map(|d| &d.label).collect::<Vec<_>>()
    );
    let count = item(&submenu, "Count types in archives…");
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        drawn_now(&ctx, &mut state);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !state.expander.is_listing(doc),
        "drawing the submenu asked for a read"
    );
    assert_eq!(
        rows_in(&state, doc),
        before,
        "an archive was read before the click"
    );

    click(&ctx, &mut state, count, egui::PointerButton::Primary);
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        let drawn = drawn_now(&ctx, &mut state);
        if drawn.iter().any(|d| d.label == ".txt — 3") {
            break;
        }
        assert!(
            Instant::now() < until,
            "the click did not count the archives, or closed the submenu: {:?}",
            drawn.iter().map(|d| &d.label).collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
