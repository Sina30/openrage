//! A row of the Search panel, or of a search inside archives, opens its file,
//! reveals it in the tree and extracts with the rest of the selection.
//!
//! ⚠ Every case clicks the row through real frames, and every hit it acts on
//! is inside an archive the tree has not opened, so a row that acted only on
//! the rows the tree has loaded fails.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use or_io::container::rpf::BodyForm;
use or_io::container::NoOpCryptoProvider;

use super::document::{DocumentId, RpfNodeId};
use super::extract_job::TypeFilter;
use super::navigation::{resolve_path, Arrival, Resolved};
use super::nested_archives_survive_a_save::{archive, open, Opened};
use super::search_session::{SearchMode, SearchRequest};
use super::{AppState, DocPayload, DocumentKind, Game, Prefs};
use crate::ui::dock::Workspace;
use crate::ui::panels::search::RESULTS;

const NONE: egui::Modifiers = egui::Modifiers::NONE;

/// One panel drawn through real frames, ticking what the editor's redraw ticks.
struct Screen {
    ctx: egui::Context,
    time: f64,
    draw: fn(&mut egui::Ui, &mut AppState),
    /// Every text put on the clipboard so far.
    copied: Vec<String>,
}

impl Screen {
    fn new(draw: fn(&mut egui::Ui, &mut AppState)) -> Self {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        Self {
            ctx,
            time: 0.0,
            draw,
            copied: Vec::new(),
        }
    }

    /// Every labelled widget one frame lays out, and its centre.
    fn frame(
        &mut self,
        state: &mut AppState,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> Vec<(String, egui::Pos2)> {
        state.poll_load_result();
        state.poll_expansions();
        state.tick_navigation();
        state.poll_extract_job();
        self.time += 1.0 / 30.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 900.0),
            )),
            time: Some(self.time),
            modifiers,
            events,
            ..Default::default()
        };
        let draw = self.draw;
        let output = self.ctx.run_ui(input, |ui| draw(ui, state));
        for command in &output.platform_output.commands {
            if let egui::OutputCommand::CopyText(text) = command {
                self.copied.push(text.clone());
            }
        }
        let Some(update) = output.platform_output.accesskit_update else {
            return Vec::new();
        };
        update
            .nodes
            .iter()
            .filter_map(|(_, node)| {
                let text = node.label().or_else(|| node.value())?;
                let b = node.bounds()?;
                let centre = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
                Some((text.to_string(), centre))
            })
            .collect()
    }

    /// Where the widget whose label starts with `label` is, drawing until it is there.
    fn find(&mut self, state: &mut AppState, label: &str) -> egui::Pos2 {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let widgets = self.frame(state, Vec::new(), NONE);
            if let Some((_, at)) = widgets.iter().find(|(text, _)| text.starts_with(label)) {
                return *at;
            }
            assert!(
                Instant::now() < deadline,
                "nothing labelled {label} was drawn: {widgets:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn click(
        &mut self,
        state: &mut AppState,
        at: egui::Pos2,
        button: egui::PointerButton,
        modifiers: egui::Modifiers,
    ) {
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers,
        };
        self.frame(
            state,
            vec![egui::Event::PointerMoved(at), press(true)],
            modifiers,
        );
        self.frame(state, vec![press(false)], modifiers);
    }

    /// The labels of the menu a right click on the row labelled `row` opens.
    fn menu(&mut self, state: &mut AppState, row: &str) -> Vec<(String, egui::Pos2)> {
        let at = self.find(state, row);
        self.click(state, at, egui::PointerButton::Secondary, NONE);
        self.frame(state, Vec::new(), NONE)
    }

    /// Choose `item` from the menu of the row labelled `row`.
    fn choose(&mut self, state: &mut AppState, row: &str, item: &str) {
        let menu = self.menu(state, row);
        let at = menu
            .iter()
            .find(|(text, _)| text == item)
            .map(|(_, at)| *at)
            .unwrap_or_else(|| panic!("the menu of {row} offers no {item}: {menu:?}"));
        self.click(state, at, egui::PointerButton::Primary, NONE);
    }

    /// Draw until `done` holds.
    fn until(&mut self, state: &mut AppState, what: &str, done: impl Fn(&AppState) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(state) {
            let said: Vec<&str> = state.toasts.iter().map(|t| t.message.as_str()).collect();
            assert!(Instant::now() < deadline, "{what}; the app said {said:?}");
            self.frame(state, Vec::new(), NONE);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// The path inside `doc` of each archive entry open as a document of its own.
fn opened_entries(state: &AppState, doc: DocumentId) -> Vec<String> {
    let Some(DocPayload::Rpf(rpf)) = state.document(doc).map(|d| &d.payload) else {
        return Vec::new();
    };
    state
        .documents
        .iter()
        .filter_map(|d| d.rpf_entry_binding)
        .filter(|binding| binding.parent_doc_id == doc)
        .filter_map(|binding| rpf.session.path_of(binding.entry_id))
        .collect()
}

/// The row `path` names in `doc`, if the tree has loaded it.
fn row_at(state: &AppState, doc: DocumentId, path: &str) -> Option<RpfNodeId> {
    match state.document(doc).map(|d| &d.payload) {
        Some(DocPayload::Rpf(rpf)) => match resolve_path(&rpf.session, path) {
            Resolved::Found(row) => Some(row),
            _ => None,
        },
        _ => None,
    }
}

/// An install holding `x64a.rpf`, with `data/one.txt` in it and `two.txt` in
/// the archive `inner.rpf` inside it.
fn install() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("an install folder");
    let inner = archive(&[("two.txt", b"RAILGUN two")]);
    let x64a = archive(&[
        ("data/one.txt", b"RAILGUN one"),
        ("data/other.txt", b"nothing"),
        ("inner.rpf", &inner),
    ]);
    std::fs::write(dir.path().join("x64a.rpf"), x64a).expect("x64a.rpf");
    dir
}

/// An install whose title update holds the DLC `testpack`, with `notes.txt` in the pack's
/// `dlc1.rpf`, so that two archives hold it, one inside the other.
fn install_with_a_sub_pack_in_the_title_update() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("an install folder");
    let setup = or_title::SetupData {
        name_hash: "testpack".into(),
        device_name: "dlc_testpack".into(),
        dat_file: "content.xml".into(),
        sub_pack_count: 1,
        ..Default::default()
    };
    let dlc = archive(&[
        ("setup2.xml", setup.to_xml().as_bytes()),
        (
            "content.xml",
            or_title::ContentXml::default().to_xml().as_bytes(),
        ),
    ]);
    let sub = archive(&[("levels/dlc/notes.txt", b"RAILGUN of the pack")]);
    let update = archive(&[
        ("dlcpacks/testpack/dlc.rpf", &dlc),
        ("dlcpacks/testpack/dlc1.rpf", &sub),
    ]);
    std::fs::create_dir_all(dir.path().join("update")).expect("update folder");
    std::fs::write(dir.path().join("update/update.rpf"), update).expect("update.rpf");
    dir
}

/// `root` opened as the remembered install, and the Search panel's search of
/// its `.txt` files for RAILGUN run to the end.
fn searched(root: &Path) -> (AppState, DocumentId) {
    searched_for(root, 2)
}

/// [`searched`], for an install whose search finds `hits` files.
fn searched_for(root: &Path, hits: usize) -> (AppState, DocumentId) {
    let mut prefs = Prefs::default();
    prefs
        .game_paths
        .set(Game::GtaVLegacy, Some(root.to_path_buf()));
    prefs.active_game = Some(Game::GtaVLegacy);
    let mut state = AppState::from_prefs(prefs);
    let install = state
        .documents
        .iter()
        .find(|d| d.kind == DocumentKind::GameRoot)
        .expect("the install's tree")
        .id;
    let request = SearchRequest {
        pattern: "RAILGUN".into(),
        mode: SearchMode::Text,
        case_sensitive: false,
        extensions: vec!["txt".into()],
        under: String::new(),
        max_hits: 0,
    };
    state.search.start(
        request,
        root.to_path_buf(),
        Arc::new(NoOpCryptoProvider::new()),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.search.is_running() {
        assert!(Instant::now() < deadline, "the search never finished");
        state.search.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(state.search.hits.len(), hits, "{:?}", state.search.hits);
    (state, install)
}

/// An archive opened as a document, holding `top.txt` and, in the archive
/// `mid.rpf`, `deep.txt`; its tree filtered for every `.txt` through
/// "Search every archive".
fn deep_found() -> (tempfile::TempDir, AppState, DocumentId) {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let mid = archive(&[("deep.txt", b"deep")]);
    let outer = archive(&[("top.txt", b"top"), ("mid.rpf", &mid)]);
    let mut state = AppState::from_prefs(Prefs::default());
    let (doc, _, _) = open(&mut state, dir.path(), &outer, Opened::AsADocument);
    let hierarchy = crate::ui::panels::hierarchy::ui_state(&mut state);
    hierarchy.filter = "**/*.txt".into();
    hierarchy.deep = true;
    (dir, state, doc)
}

const DEEP: &str = "doc:/mid.rpf/deep.txt";

#[test]
fn a_search_panel_result_opens_from_its_rows_menu() {
    let root = install();
    let (mut state, install) = searched(root.path());
    assert!(
        row_at(&state, install, "x64a.rpf/inner.rpf/two.txt").is_none(),
        "the tree opened the archives by itself, so this cannot tell a row \
         that opens them from one that does not"
    );
    let mut screen = Screen::new(crate::ui::panels::search::draw);
    state.active_workspace = Workspace::Data;

    screen.choose(&mut state, "two.txt  (", "Open");

    let left = std::cell::Cell::new(None);
    screen.until(&mut state, "the result never opened", |state| {
        if state.active_workspace != Workspace::Data {
            left.set(Some(state.active_workspace));
        }
        opened_entries(state, install).contains(&"x64a.rpf/inner.rpf/two.txt".to_string())
    });
    assert_eq!(
        left.get(),
        None,
        "a text file opened from Data took the screen to another workspace while it loaded"
    );
}

#[test]
fn a_search_panel_result_inside_a_dlc_sub_pack_in_the_title_update_opens_from_its_rows_menu() {
    const NOTES: &str = "update/update.rpf/dlcpacks/testpack/dlc1.rpf/levels/dlc/notes.txt";
    let root = install_with_a_sub_pack_in_the_title_update();
    let (mut state, install) = searched_for(root.path(), 1);
    assert_eq!(
        crate::state::search_session::hit_path(&state.search.hits[0]),
        NOTES,
        "the hit names the whole chain of archives holding the file"
    );
    assert!(
        row_at(&state, install, NOTES).is_none(),
        "the tree opened the archives by itself, so this cannot tell a row \
         that opens them from one that does not"
    );
    let mut screen = Screen::new(crate::ui::panels::search::draw);
    state.active_workspace = Workspace::Data;

    screen.choose(&mut state, "levels/dlc/notes.txt  (", "Open");

    screen.until(&mut state, "the result never opened", |state| {
        opened_entries(state, install).contains(&NOTES.to_string())
    });
}

#[test]
fn a_match_of_a_search_inside_archives_opens_on_a_double_click() {
    let (_dir, mut state, doc) = deep_found();
    let mut screen = Screen::new(crate::ui::panels::hierarchy::draw);
    let at = screen.find(&mut state, DEEP);
    assert!(
        row_at(&state, doc, "mid.rpf/deep.txt").is_none(),
        "the tree opened mid.rpf by itself"
    );

    screen.click(&mut state, at, egui::PointerButton::Primary, NONE);
    screen.click(&mut state, at, egui::PointerButton::Primary, NONE);

    screen.until(&mut state, "the match never opened", |state| {
        opened_entries(state, doc).contains(&"mid.rpf/deep.txt".to_string())
    });
}

#[test]
fn reveal_in_tree_selects_the_matchs_row_and_the_tree_draws_it() {
    let (_dir, mut state, doc) = deep_found();
    let mut screen = Screen::new(crate::ui::panels::hierarchy::draw);

    screen.choose(&mut state, DEEP, "Reveal in tree");

    screen.until(&mut state, "the reveal never selected the row", |state| {
        state.selection.selected(doc).is_some()
            && state.selection.selected(doc) == row_at(state, doc, "mid.rpf/deep.txt")
    });
    let row = state.selection.selected(doc).expect("selected above");
    screen.until(
        &mut state,
        "the tree never drew the revealed row",
        |state| {
            crate::ui::panels::hierarchy::ui_state_ref(state)
                .and_then(|tree| tree.visible_rows.get(&doc))
                .is_some_and(|rows| rows.contains(&row))
        },
    );
}

#[test]
fn extract_writes_every_selected_result_through_the_job_and_asks_before_replacing() {
    let root = install();
    let (mut state, _install) = searched(root.path());
    let mut screen = Screen::new(crate::ui::panels::search::draw);
    let selected = |state: &AppState| -> Vec<usize> {
        let rows = state.search.selection.selected_ids(RESULTS);
        rows.into_iter().map(|row| row as usize).collect()
    };
    let one = screen.find(&mut state, "data/one.txt  (");
    let two = screen.find(&mut state, "two.txt  (");
    let primary = egui::PointerButton::Primary;
    screen.click(&mut state, one, primary, NONE);
    screen.click(&mut state, two, primary, egui::Modifiers::SHIFT);
    assert_eq!(
        selected(&state).len(),
        2,
        "Shift did not span to the second"
    );
    screen.click(&mut state, one, primary, NONE);
    assert_eq!(
        selected(&state).len(),
        1,
        "a plain click did not start over"
    );
    screen.click(&mut state, two, primary, egui::Modifiers::CTRL);
    let rows = selected(&state);
    assert_eq!(rows.len(), 2, "Ctrl did not add the second result");
    let menu = screen.menu(&mut state, "two.txt  (");
    assert!(
        menu.iter().any(|(text, _)| text == "2 selected"),
        "the menu does not act on both results: {menu:?}"
    );

    let dest = tempfile::tempdir().expect("a destination");
    let extract = || Arrival::Extract {
        dest: dest.path().to_path_buf(),
        form: BodyForm::Decoded,
        filter: TypeFilter::All,
    };
    state.go_to_search_hits(&rows, extract());
    screen.until(&mut state, "the extraction never wrote both", |state| {
        !state.extract_jobs.is_running()
            && dest.path().join("one.txt").is_file()
            && dest.path().join("two.txt").is_file()
    });
    assert_eq!(
        std::fs::read(dest.path().join("one.txt")).expect("one.txt"),
        b"RAILGUN one"
    );
    assert_eq!(
        std::fs::read(dest.path().join("two.txt")).expect("two.txt"),
        b"RAILGUN two"
    );

    state.go_to_search_hits(&rows, extract());
    screen.until(&mut state, "extracting again did not ask", |state| {
        state.extract_jobs.clash.is_some()
    });
    assert_eq!(
        state.extract_jobs.clash.as_ref().map(|c| c.clashes),
        Some(2)
    );
}

#[test]
fn copy_path_on_a_match_of_a_search_inside_archives_copies_its_path_in_the_archive() {
    let (_dir, mut state, _doc) = deep_found();
    let mut screen = Screen::new(crate::ui::panels::hierarchy::draw);

    screen.choose(&mut state, DEEP, "Copy path");

    assert_eq!(screen.copied, ["mid.rpf/deep.txt"]);
}

#[test]
fn the_search_panel_draws_the_selected_hits_path_where_it_can_be_seen() {
    let root = install();
    let (mut state, _install) = searched(root.path());
    let mut screen = Screen::new(crate::ui::panels::search::draw);
    let two = screen.find(&mut state, "two.txt  (");
    screen.click(&mut state, two, egui::PointerButton::Primary, NONE);

    let shown = screen.frame(&mut state, Vec::new(), NONE);
    let (path, at) = shown
        .iter()
        .find(|(text, _)| text.contains("  <=  "))
        .expect("the selected hit's path is drawn");
    assert!(at.y < 900.0, "{path} is drawn off the screen, at {at:?}");
}
