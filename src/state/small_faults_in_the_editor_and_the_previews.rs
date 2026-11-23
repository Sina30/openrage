//! Small faults in the editor and the previews: the find bar says nothing
//! until it has searched, Go to Line and the encode bar fit a narrow editor,
//! the Particle workspace's texture preview shows a texture dictionary, a
//! list over one document closes when another takes its place and the
//! Outline starts each document at its top.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Pos2, Rect};

use super::nothing_draws_over_what_it_belongs_to::{
    a_texture_on_screen, app_context, areas_over_the_panels, default_and_narrow, draw_editor,
    editing, input, lines_and_the_rest, long_lines, painted, widgets, window, within, Panel,
};
use super::{AppState, DocPayload, Document, DocumentId, Prefs};
use crate::ui::dock::{Tab, Workspace};
use crate::ui::panels::text_editor::editor_state::FIND_DEBOUNCE;

const DEADLINE: Duration = Duration::from_secs(10);

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// `text` in an editor of `rect`'s size with its caret placed by a click, as a user starts: it has the focus.
fn focused_editor(rect: Rect, text: &str) -> (AppState, Panel) {
    let mut state = editing("notes.txt", text);
    let mut panel = Panel::new(rect);
    let at = rect.center();
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    for events in [
        Vec::new(),
        vec![egui::Event::PointerMoved(at), press(true)],
        vec![press(false)],
    ] {
        draw_editor(&mut panel, &mut state, events);
    }
    (state, panel)
}

/// What the find bar's status row says in `output`: a count, "Searching…" or "No results".
fn the_bars_status(output: &egui::FullOutput) -> Option<String> {
    let counts = |text: &str| {
        let (active, all) = text.split_once(" of ")?;
        let all = all.split_whitespace().next()?.trim_end_matches('+');
        Some(active.parse::<usize>().is_ok() && all.parse::<usize>().is_ok())
    };
    painted(output)
        .into_iter()
        .map(|part| part.text)
        .find(|text| {
            matches!(text.as_str(), "No results" | "Searching…") || counts(text) == Some(true)
        })
}

/// When, after the typing, a frame was drawn, and what the find bar said in it.
type Statuses = Vec<(Duration, Option<String>)>;

/// What the find bar said in each frame while `query` is typed into it over an editor holding
/// `text`, drawing frames for as long as `keep_on` says to.
fn the_bars_statuses(text: &str, query: &str, keep_on: impl Fn(&Statuses) -> bool) -> Statuses {
    let rect = default_and_narrow(Workspace::Data, Tab::TEXT_EDITOR)[0];
    let (mut state, mut panel) = focused_editor(rect, text);
    let find = key(egui::Key::F, crate::ui::shortcuts::PRIMARY_EGUI);
    for events in [vec![find], Vec::new(), Vec::new()] {
        draw_editor(&mut panel, &mut state, events);
    }
    let started = Instant::now();
    let mut said = Vec::new();
    let mut events = vec![egui::Event::Text(query.to_string())];
    while (said.is_empty() || keep_on(&said)) && started.elapsed() < DEADLINE {
        let output = draw_editor(&mut panel, &mut state, std::mem::take(&mut events));
        said.push((started.elapsed(), the_bars_status(&output)));
        std::thread::sleep(Duration::from_millis(5));
    }
    said
}

/// When `said` read as missing.
fn read_as_missing(said: &Statuses) -> Vec<Duration> {
    (said.iter())
        .filter(|(_, status)| status.as_deref() == Some("No results"))
        .map(|(at, _)| *at)
        .collect()
}

#[test]
fn the_find_bar_says_nothing_until_it_has_searched() {
    let _serial = crate::ui::theme::serial();
    let counted = |said: &Statuses| {
        let last = said.last().and_then(|(_, status)| status.as_deref());
        last.is_some_and(|status| status.contains(" of ") && !status.contains("earching"))
    };
    let said = the_bars_statuses(&long_lines(), "word", |said| !counted(said));
    let missing = read_as_missing(&said);
    assert!(
        missing.is_empty(),
        "a word the file holds 480 times read as missing at {missing:?} after it was typed"
    );
    let last = said.last().and_then(|(_, status)| status.clone());
    assert!(
        last.as_deref()
            .is_some_and(|status| status.contains(" of 480")),
        "the search found {last:?} for a word the file holds 480 times"
    );
}

#[test]
fn the_find_bar_says_no_results_only_once_a_search_found_nothing() {
    let _serial = crate::ui::theme::serial();
    let said = the_bars_statuses(&long_lines(), "zzqx", |said| {
        said.last().is_none_or(|(at, status)| {
            *at < FIND_DEBOUNCE * 2 || status.as_deref() != Some("No results")
        })
    });
    let early: Vec<Duration> = read_as_missing(&said)
        .into_iter()
        .filter(|at| *at < FIND_DEBOUNCE)
        .collect();
    assert!(
        early.is_empty(),
        "a word typed read as missing at {early:?}, before the search could have run"
    );
    assert_eq!(
        said.last().and_then(|(_, status)| status.as_deref()),
        Some("No results"),
        "a word the file never holds should read as missing once searched: {said:?}"
    );
}

/// The default text editor's panel, `width` points across.
fn editor_of_width(width: f32) -> Rect {
    let default = default_and_narrow(Workspace::Data, Tab::TEXT_EDITOR)[0];
    Rect::from_min_size(default.min, egui::vec2(width, default.height()))
}

#[test]
fn go_to_lines_field_stays_within_a_narrow_editor() {
    let _serial = crate::ui::theme::serial();
    let text = long_lines();
    let mut past_the_edge = Vec::new();
    // From about the narrowest column a dock layout makes down to far narrower than any does.
    for width in (120..=300).step_by(10).map(|width| width as f32) {
        let rect = editor_of_width(width);
        let (mut state, mut panel) = focused_editor(rect, &text);
        let go_to_line = key(egui::Key::G, crate::ui::shortcuts::PRIMARY_EGUI);
        draw_editor(&mut panel, &mut state, vec![go_to_line]);
        let output = draw_editor(&mut panel, &mut state, Vec::new());
        let (lines, _) = lines_and_the_rest(&output, &text);
        let editor = (lines.iter().find(|p| p.text == "1"))
            .map(|p| p.clip)
            .unwrap_or_else(|| panic!("the editor drew no first line in {rect:?}"));
        let fields = areas_over_the_panels(&panel.ctx);
        assert!(!fields.is_empty(), "Ctrl+G opened no field in {rect:?}");
        past_the_edge.extend(
            (fields.into_iter())
                .filter(|field| !within(editor, *field))
                .map(|field| format!("{width}: the field at {field:?}, the editor at {editor:?}")),
        );
    }
    assert!(
        past_the_edge.is_empty(),
        "Go to Line's field passes the edge of the editor: {past_the_edge:#?}"
    );
}

/// The vertical rules a frame drew, which is what a separator in a row is.
fn rules(output: &egui::FullOutput) -> Vec<Rect> {
    fn walk(shape: &egui::Shape, out: &mut Vec<Rect>) {
        match shape {
            egui::Shape::LineSegment { points, .. }
                if points[0].x == points[1].x && (points[0].y - points[1].y).abs() > 4.0 =>
            {
                out.push(Rect::from_two_pos(points[0], points[1]));
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

#[test]
fn no_row_of_the_encode_bar_starts_with_a_separator() {
    let _serial = crate::ui::theme::serial();
    let default = default_and_narrow(Workspace::Texture, Tab::TEXTURE_PREVIEW)[0];
    let (mut seen, mut starting_a_row, mut first) = (0, Vec::new(), None);
    // One point at a time: a separator starts a row only for the few points of width where it is
    // the one thing that no longer fits the row before it.
    for width in (150..=520).map(|width| width as f32) {
        let rect = Rect::from_min_size(default.min, egui::vec2(width, default.height()));
        let mut state = a_texture_on_screen();
        let mut panel = Panel::new(rect);
        let mut output = None;
        for _ in 0..3 {
            output = Some(panel.frame(Vec::new(), |ui| {
                crate::ui::panels::texture_preview::draw(ui, &mut state)
            }));
        }
        let output = output.expect("drawn");
        let parts = widgets(&output);
        for rule in rules(&output) {
            seen += 1;
            let before_it = |(_, part): &&(String, Rect)| {
                let shared = part.bottom().min(rule.bottom()) - part.top().max(rule.top());
                shared > rule.height() / 2.0 && part.right() <= rule.left() + 0.5
            };
            if !parts.iter().any(|part| before_it(&part)) {
                starting_a_row.push(width);
                first.get_or_insert_with(|| {
                    format!("{width} wide: the separator at {rule:?}, the widgets {parts:?}")
                });
            }
        }
    }
    assert!(seen > 0, "the encode bar drew no separator at all");
    assert!(
        starting_a_row.is_empty(),
        "a row starts with a separator in a texture view of these widths: {starting_a_row:?}; the first: {first:?}"
    );
}

#[test]
fn a_texture_dictionary_active_in_the_particle_workspace_shows_in_its_texture_preview() {
    let _serial = crate::ui::theme::serial();
    let rect = default_and_narrow(Workspace::Particle, Tab::TEXTURE_PREVIEW)[0];
    let mut state = a_texture_on_screen();
    state.active_workspace = Workspace::Particle;
    let mut panel = Panel::new(rect);
    let mut output = None;
    for _ in 0..3 {
        output = Some(panel.frame(Vec::new(), |ui| {
            crate::ui::panels::texture_preview::draw(ui, &mut state)
        }));
    }
    let shown: Vec<String> = widgets(&output.expect("drawn"))
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    assert!(
        shown
            .iter()
            .any(|text| text == "prop_vehicle_generic_doorshut_n"),
        "the preview does not show the dictionary's texture: {shown:?}"
    );
    assert!(
        !shown
            .iter()
            .any(|text| text.starts_with("Open a texture dictionary")),
        "the preview asks for a texture dictionary while one is active: {shown:?}"
    );
}

/// A dictionary of `count` textures named `<prefix>_000` on, open as a document of its own.
fn a_dictionary_of(state: &mut AppState, prefix: &str, count: usize) -> DocumentId {
    use or_io::codec::texture::GrcTextureFormat;
    use or_io::gta5::{gen8, PgDictionary};

    let mut dictionary = PgDictionary::default();
    for n in 0..count {
        dictionary.insert(gen8::GrcTexture {
            name: format!("{prefix}_{n:03}"),
            width: 4,
            height: 4,
            depth: 1,
            format: GrcTextureFormat::DXT1.to_raw_format(),
            mip_count: 1,
            backing_store: vec![0; 8],
            ..Default::default()
        });
    }
    let id = state.alloc_doc_id();
    state.documents.push(Document::new(
        id,
        Some(PathBuf::from(format!("C:/mods/{prefix}.ytd"))),
        DocPayload::Resource(or_io::Asset::TextureDict(dictionary)),
    ));
    id
}

/// Two dictionaries of `count` textures each, named `first_000` and `second_000` on, the first active.
fn two_dictionaries(count: usize) -> (AppState, [DocumentId; 2]) {
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    let ids = ["first", "second"].map(|prefix| a_dictionary_of(&mut state, prefix, count));
    state.set_active_document(ids[0]);
    state.active_workspace = Workspace::Texture;
    (state, ids)
}

#[test]
fn an_open_list_closes_when_the_document_under_it_changes() {
    let _serial = crate::ui::theme::serial();
    let (mut state, [first, second]) = two_dictionaries(1);
    let ctx = app_context();
    let screen = window(crate::app::EDITOR_SIZE);
    let mut time = 0.0;
    let list = std::cell::Cell::new(None);
    // A list as the viewport's level-of-detail switch draws it, in a corner of the window.
    let mut frame = |state: &mut AppState| {
        time += 1.0 / 30.0;
        ctx.begin_pass(input(screen, time, Vec::new()));
        crate::ui::draw(&ctx, state, None);
        egui::Area::new(egui::Id::new("a_list_over_the_viewport"))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::new(900.0, 400.0))
            .show(&ctx, |ui| {
                let switch = egui::ComboBox::from_id_salt("a_list")
                    .selected_text("High LOD to 10 m")
                    .show_ui(ui, |ui| ui.label("Medium LOD to 30 m"));
                list.set(Some(switch.response.id));
            });
        let _ = ctx.end_pass();
    };
    frame(&mut state);
    frame(&mut state);
    let list = list.get().expect("the switch is drawn");
    // The id egui keeps a combo box's popup under.
    egui::Popup::open_id(&ctx, list.with("popup"));
    frame(&mut state);
    frame(&mut state);
    assert!(
        egui::ComboBox::is_open(&ctx, list),
        "the list did not stay open, so nothing is tested"
    );
    state.set_active_document(second);
    frame(&mut state);
    frame(&mut state);
    assert!(
        !egui::ComboBox::is_open(&ctx, list),
        "the list opened over {first} stayed open over {second}"
    );
}

/// The first texture of the dictionary `doc` holds, which is the first row of its Outline.
fn the_first_texture(state: &AppState, doc: DocumentId) -> String {
    let payload = &state.document(doc).expect("the document is open").payload;
    let dictionary = crate::state::textures::legacy_dictionary(payload).expect("a dictionary");
    let first = dictionary.entries.iter().flatten().next();
    first.expect("a texture").name.clone()
}

#[test]
fn the_outline_starts_each_document_at_its_top() {
    let _serial = crate::ui::theme::serial();
    let (mut state, [first, second]) = two_dictionaries(120);
    let tops = [first, second].map(|doc| the_first_texture(&state, doc));
    let rect = Rect::from_min_size(Pos2::new(0.0, 60.0), egui::vec2(300.0, 300.0));
    let mut panel = Panel::new(rect);
    let mut draw = |state: &mut AppState, events: Vec<egui::Event>| {
        panel.frame(events, |ui| {
            crate::ui::panels::hierarchy::draw_outline(ui, state)
        })
    };
    let shows = |output: &egui::FullOutput, name: &str| {
        (painted(output).iter()).any(|part| part.text.contains(name) && within(rect, part.rect))
    };
    let texts = |output: &egui::FullOutput| -> Vec<String> {
        (painted(output).into_iter())
            .map(|part| format!("{} at y {:.0}", part.text, part.rect.top()))
            .collect()
    };
    let mut output = draw(&mut state, Vec::new());
    for _ in 0..2 {
        output = draw(&mut state, Vec::new());
    }
    assert!(
        shows(&output, &tops[0]),
        "the Outline does not start at the top of the first document: {:?}",
        texts(&output)
    );
    let wheel = |dy| egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, dy),
        modifiers: egui::Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    };
    draw(&mut state, vec![egui::Event::PointerMoved(rect.center())]);
    // Then the frames it takes the wheel's smoothed scrolling to run out, which a later frame would
    // otherwise apply to the next document.
    for frame in 0..30 {
        let events = if frame < 6 {
            vec![wheel(-400.0)]
        } else {
            Vec::new()
        };
        output = draw(&mut state, events);
    }
    assert!(
        !shows(&output, &tops[0]),
        "the wheel did not scroll the first document's Outline: {:?}",
        texts(&output)
    );

    state.set_active_document(second);
    for _ in 0..3 {
        output = draw(&mut state, Vec::new());
    }
    assert!(
        shows(&output, &tops[1]),
        "the second document's Outline, which starts at {}, starts where the first was scrolled to: {:?}",
        tops[1],
        texts(&output)
    );
}
