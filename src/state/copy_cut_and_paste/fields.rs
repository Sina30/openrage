//! A field's value copied in one file's grid and pasted over a field of its type in another.

use or_io::desc::ScalarValue;

use super::*;
use crate::state::{described_root, Prefs};

/// A blank drawable open as `id`.
fn drawable(state: &mut AppState, id: DocumentId) -> DocumentId {
    let format = or_io::gta5::formats::format("ydr").expect(".ydr is declared");
    let version = or_io::container::resource::ResourceType::from_extension("ydr")
        .expect("a resource type")
        .read_versions()
        .into_iter()
        .find(|v| format.target_of_version(*v).ok() == Some(or_io::Generation::Gen8))
        .expect("a Legacy version");
    let mut doc = crate::ui::panels::hierarchy::described_tests::zeroed_document("ydr", version);
    doc.id = id;
    state.documents.push(doc);
    id
}

/// The grid the inspector shows of `doc`'s root.
fn grid(state: &AppState, doc: DocumentId) -> or_editor::Summary {
    let doc = state.document(doc).expect("open");
    let (pack, root) = described_root(doc).expect("a drawable is described");
    let image = doc.resource.as_ref().expect("its bytes");
    or_editor::Summary::of(&pack, &root, image.view(), 0).expect("its grid")
}

fn row(grid: &or_editor::Summary, ending: &str) -> usize {
    grid.rows
        .iter()
        .position(|r| r.label.ends_with(ending) && r.edit.is_some())
        .unwrap_or_else(|| {
            let labels: Vec<&str> = grid.rows.iter().map(|r| r.label.as_str()).collect();
            panic!("no editable `…{ending}` among {labels:?}")
        })
}

/// What `doc` holds where row `at` of `grid` edits.
fn value(state: &AppState, doc: DocumentId, grid: &or_editor::Summary, at: usize) -> f32 {
    let edit = grid.rows[at].edit.expect("an editable row");
    let image = state
        .document(doc)
        .and_then(|d| d.resource.as_ref())
        .expect("its bytes");
    let bytes = &image.view().bytes()[edit.offset..edit.offset + 4];
    f32::from_le_bytes(bytes.try_into().expect("four bytes"))
}

/// The four floats of the vector `field` in `doc`.
fn vector(state: &AppState, doc: DocumentId, field: &str) -> [f32; 4] {
    let grid = grid(state, doc);
    ["X", "Y", "Z", "W"]
        .map(|part| value(state, doc, &grid, row(&grid, &format!("{field} > {part}"))))
}

const CORNER: [f32; 4] = [1.5, -2.25, 3.0, 0.125];

/// Two blank drawables, the first with `CORNER` as its box's lower corner.
fn two_drawables() -> (AppState, DocumentId, DocumentId) {
    let mut state = AppState::from_prefs(Prefs::default());
    let (a, b) = (drawable(&mut state, 9001), drawable(&mut state, 9002));
    let a_grid = grid(&state, a);
    for (part, v) in ["X", "Y", "Z", "W"].into_iter().zip(CORNER) {
        let at = row(&a_grid, &format!("BoundsBox > Min > {part}"));
        let edit = a_grid.rows[at].edit.expect("editable");
        let set = state.patch_scalar(a, &a_grid.rows[at].label, edit, ScalarValue::F32(v));
        assert!(set, "the source drawable takes its corner");
    }
    (state, a, b)
}

/// One frame of the inspector, and each labelled widget it laid out with its centre.
fn inspector(
    ctx: &egui::Context,
    state: &mut AppState,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Pos2)> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1200.0, 2000.0),
        )),
        events,
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| crate::ui::panels::inspector::draw(ui, state));
    let Some(update) = output.platform_output.accesskit_update else {
        return Vec::new();
    };
    (update.nodes.iter())
        .filter_map(|(_, node)| {
            let b = node.bounds()?;
            let centre = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
            Some((node.label().or_else(|| node.value())?.to_string(), centre))
        })
        .collect()
}

/// Click `label` in what the inspector shows now with `button`.
fn click(ctx: &egui::Context, state: &mut AppState, label: &str, button: egui::PointerButton) {
    inspector(ctx, state, Vec::new());
    let shown = inspector(ctx, state, Vec::new());
    let at = (shown.iter().find(|(text, _)| text == label))
        .map(|(_, at)| *at)
        .unwrap_or_else(|| panic!("no `{label}` in {shown:?}"));
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    inspector(ctx, state, vec![egui::Event::PointerMoved(at), press(true)]);
    inspector(ctx, state, vec![press(false)]);
}

/// ⚠ Through the inspector's own grid, right-clicked and chosen from as a user does.
#[test]
fn a_rows_menu_in_the_grid_copies_its_field_and_pastes_it_over_another() {
    let (mut state, a, b) = two_drawables();
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let (right, left) = (egui::PointerButton::Secondary, egui::PointerButton::Primary);
    state.set_active_document(a);
    click(
        &ctx,
        &mut state,
        "Base > LodGroup > BoundsBox > Min > X",
        right,
    );
    click(&ctx, &mut state, "Copy Min", left);
    state.set_active_document(b);
    click(
        &ctx,
        &mut state,
        "Base > LodGroup > BoundsBox > Max > Y",
        right,
    );
    click(&ctx, &mut state, "Paste into Max", left);
    assert_eq!(vector(&state, b, "BoundsBox > Max"), CORNER);
}

/// ⚠ The copy is taken from one drawable and pasted into another's other
/// vector, through a row of the destination that is not its first.
#[test]
fn a_vector_copied_from_one_field_pastes_into_another() {
    let (mut state, a, b) = two_drawables();
    state.set_active_document(a);
    let a_grid = grid(&state, a);
    state
        .copy_field(a, &a_grid, row(&a_grid, "BoundsBox > Min > Z"))
        .expect("the copy");
    state.set_active_document(b);
    let b_grid = grid(&state, b);
    state
        .paste_field(b, &b_grid, row(&b_grid, "BoundsBox > Max > Y"))
        .expect("the paste");
    assert_eq!(
        vector(&state, b, "BoundsBox > Max"),
        CORNER,
        "the whole vector lands in the field pasted into"
    );
    assert_eq!(
        vector(&state, b, "BoundsBox > Min"),
        [0.0; 4],
        "and nowhere else"
    );

    assert_eq!(state.undo().as_deref(), Some("Paste Min into Max"));
    assert_eq!(
        vector(&state, b, "BoundsBox > Max"),
        [0.0; 4],
        "one Undo takes the whole vector back"
    );

    let b_grid = grid(&state, b);
    let lod = row(&b_grid, "LodDistances [0]");
    let refused = state
        .paste_field(b, &b_grid, lod)
        .expect_err("a vector does not go into four distances");
    assert!(
        refused.contains("LodDistances") && refused.contains("Min"),
        "the refusal names both fields: {refused}"
    );
    assert_eq!(
        value(&state, b, &b_grid, lod),
        0.0,
        "the refusal wrote nothing"
    );
}
