//! A document's declared structure under the engine's names: the fields its
//! owned value shows, with the engine's bookkeeping behind "Show internals",
//! each written here or saying why not.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use or_editor::{Edit, EditKind, Row, Summary};
use or_io::desc::{PackVariant, ScalarValue};

use super::*;

/// What a format declares of the types it reaches, by type name.
pub(crate) struct Declared {
    /// The fields each owned type shows.
    shown: HashMap<String, Vec<&'static str>>,
    /// The fields declared a `Hash32`.
    hashes: HashMap<String, &'static [&'static str]>,
}

/// What the format of `extension`, at the target RSC7 `version` names, declares of each type it reaches.
pub(crate) fn declared_for(extension: &str, version: u32) -> Option<Arc<Declared>> {
    type Cache = Mutex<HashMap<(&'static str, or_io::Generation), Arc<Declared>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let format = or_io::gta5::formats::format(extension)?;
    let target = format.target_of_version(version).ok()?;
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(shown) = cache.get(&(format.ext, target)) {
        return Some(shown.clone());
    }
    let reach = format.reach(target).ok()?;
    let shown = (reach.types())
        .filter_map(|ty| {
            let public = reach.owned_public(&ty.name)?;
            Some((
                ty.name.clone(),
                public.iter().map(|(name, _)| *name).collect(),
            ))
        })
        .collect();
    let hashes = (reach.types())
        .map(|ty| (ty.name.clone(), reach.hash_fields(&ty.name)))
        .collect();
    let declared = Arc::new(Declared { shown, hashes });
    cache.insert((format.ext, target), declared.clone());
    Some(declared)
}

/// What `doc`'s format, at the target its version names, declares of each type it reaches.
fn declared_in(doc: &crate::state::Document) -> Option<Arc<Declared>> {
    declared_for(doc.kind.extension(), doc.resource.as_ref()?.seal.version())
}

/// The declared fields of the `type_name` at `at` in `doc`, internal where its owned value keeps them to itself.
pub(crate) fn summary_at(
    doc: &crate::state::Document,
    type_name: &str,
    at: usize,
) -> Option<Summary> {
    let image = doc.resource.as_ref()?;
    let (pack, _) = crate::state::described_root(doc)?;
    summary_in(
        &pack,
        declared_in(doc).as_deref(),
        image.view(),
        type_name,
        at,
    )
}

/// [`summary_at`] for an image no document holds, `declared` being what [`declared_for`] says of its format.
pub(crate) fn summary_in(
    pack: &or_io::desc::Pack,
    declared: Option<&Declared>,
    image: or_io::desc::Image<'_>,
    type_name: &str,
    at: usize,
) -> Option<Summary> {
    // A type with no owned declaration shows every field.
    let shows = |ty: &str, field: &str| {
        (declared.and_then(|d| d.shown.get(ty))).is_none_or(|fields| fields.contains(&field))
    };
    let is_a_hash = |ty: &str, field: &str| {
        (declared.and_then(|d| d.hashes.get(ty))).is_some_and(|fields| fields.contains(&field))
    };
    Summary::of_shown(pack, type_name, image, at, &shows, &is_a_hash)
}

/// A hash as the inspector spells one: `0x` and eight digits, then the name it was made from where one is known.
pub(super) fn hash_text(hash: u32, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("0x{hash:08X} ({name})"),
        None => format!("0x{hash:08X}"),
    }
}

/// A shader's effect by its name, or by its hash where the shader names do not hold it.
pub(super) fn shader_name(effect: u32) -> String {
    or_io::gta5::shaders::shader_names::lookup(effect)
        .map_or_else(|| hash_text(effect, None), |name| name.to_string())
}

/// `summary`'s fields, read from `doc`, under the engine's names, the internal ones while "Show internals" is on; an edit is written into `doc` through the value tree.
pub(super) fn draw_editable_summary(
    ui: &mut egui::Ui,
    id: &str,
    state: &mut AppState,
    doc: DocumentId,
    summary: &Summary,
) {
    let internals = shows_internals(state);
    // Each hash row's name, read before the rows' menus borrow the state.
    let names: Vec<Option<String>> = (summary.rows.iter())
        .map(|r| match (&r.held, r.hash) {
            (Some(ScalarValue::Int(v)), true) => {
                state.name_index.name_of(*v as u32).map(str::to_string)
            }
            _ => None,
        })
        .collect();
    let mut pending = None;
    egui::CollapsingHeader::new(&summary.title)
        .id_salt(("structure", id))
        .default_open(true)
        .show(ui, |ui| {
            if !summary.rows.iter().any(|r| internals || !r.internal) {
                ui.label(
                    egui::RichText::new(
                        "Only the engine's bookkeeping is declared here: Show internals lists it.",
                    )
                    .small()
                    .color(theme::TEXT_SECONDARY),
                );
                return;
            }
            grid(ui, id, |ui| {
                let shown =
                    (summary.rows.iter().enumerate()).filter(|(_, r)| internals || !r.internal);
                for (at, r) in shown {
                    let menu = |label: &egui::Response| {
                        super::field_menu::field_menu(label, state, doc, summary, at)
                    };
                    if let Some(edit) = field_row(ui, r, names[at].as_deref(), menu) {
                        pending = Some((r.label.as_str(), edit));
                    }
                }
            });
        });

    let Some((field, (edit, value))) = pending else {
        return;
    };
    state.patch_scalar(doc, field, edit, value);
}

/// `label`, or its end behind an ellipsis where the whole path would push the value out of the panel.
fn leaf_end<'a>(ui: &egui::Ui, label: &'a str) -> std::borrow::Cow<'a, str> {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let width = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font.clone(), egui::Color32::PLACEHOLDER)
            .size()
            .x
    };
    let room = ui.clip_rect().width() * 0.62;
    if width(label) <= room {
        return label.into();
    }
    let mut rest = label;
    while let Some((_, end)) = rest.split_once(" > ") {
        rest = end;
        let short = format!("… > {rest}");
        if width(&short) <= room {
            return short.into();
        }
    }
    format!("… > {rest}").into()
}

/// One field: a choice, a switch or a number where this panel writes it, with `menu` on its label, else its value and why not.
pub(super) fn field_row(
    ui: &mut egui::Ui,
    r: &Row,
    hash_name: Option<&str>,
    menu: impl FnOnce(&egui::Response),
) -> Option<(Edit, ScalarValue)> {
    if !filter_matches(ui.ctx(), &r.label) {
        return None;
    }
    let shown = leaf_end(ui, &r.label);
    let text = egui::RichText::new(shown.as_ref())
        .small()
        .color(theme::TEXT_SECONDARY);
    let label = match r.edit {
        Some(_) => {
            let label = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
            menu(&label);
            label
        }
        None => ui.label(text),
    };
    let label = match shown {
        std::borrow::Cow::Borrowed(_) => label,
        std::borrow::Cow::Owned(_) => {
            crate::ui::accessible_name::named(label, &r.label).on_hover_text(&r.label)
        }
    };
    if let Some(note) = r.note.as_deref() {
        label.on_hover_text(note);
    }
    let changed = match (&r.choices, r.edit, &r.held) {
        (Some(choices), ..) => choose(ui, r, choices),
        (None, Some(edit), Some(held)) => write(ui, r, edit, held, hash_name),
        _ => {
            value(ui, r, hash_name);
            None
        }
    };
    ui.end_row();
    changed
}

/// An enum field as a choice of its declared variants; a value none names stays as it is until another is picked.
fn choose(ui: &mut egui::Ui, r: &Row, choices: &[PackVariant]) -> Option<(Edit, ScalarValue)> {
    let current = match r.held {
        Some(ScalarValue::Int(v)) => Some(v),
        _ => None,
    };
    let mut picked = None;
    let combo = ui
        .add_enabled_ui(r.edit.is_some(), |ui| {
            egui::ComboBox::from_id_salt(&r.label)
                .selected_text(&r.value)
                .show_ui(ui, |ui| {
                    for variant in choices {
                        let value = i128::from(variant.value);
                        let item = ui.selectable_label(
                            current == Some(value),
                            format!("{} ({})", variant.name, variant.value),
                        );
                        let item = match &variant.note {
                            Some(note) => item.on_hover_text(note),
                            None => item,
                        };
                        if item.clicked() && current != Some(value) {
                            picked = Some(value);
                        }
                    }
                })
                .response
        })
        .inner;
    if let Some(why) = r.fixed {
        combo.on_disabled_hover_text(why);
    }
    Some((r.edit?, ScalarValue::Int(picked?)))
}

/// A switch or a number that starts from what the bytes hold, which `value` rounds.
fn write(
    ui: &mut egui::Ui,
    r: &Row,
    edit: Edit,
    held: &ScalarValue,
    hash_name: Option<&str>,
) -> Option<(Edit, ScalarValue)> {
    let written = match (edit.kind, held) {
        (EditKind::Bool, ScalarValue::Bool(on)) => {
            let mut on = *on;
            ui.checkbox(&mut on, "")
                .changed()
                .then_some(ScalarValue::Bool(on))
        }
        // ⚠ Shown whole, so retyping one digit keeps the rest of the value.
        (EditKind::F32, ScalarValue::F32(v)) => {
            let mut v = f64::from(*v);
            drag_value(ui, &mut v, |d| {
                d.speed(0.25)
                    .custom_formatter(|v, _| (v as f32).to_string())
            })
            .inner
            .then_some(ScalarValue::F32(v as f32))
        }
        (EditKind::F64, ScalarValue::F64(v)) => {
            let mut v = *v;
            drag_value(ui, &mut v, |d| {
                d.speed(0.25).custom_formatter(|v, _| v.to_string())
            })
            .inner
            .then_some(ScalarValue::F64(v))
        }
        (EditKind::Int(_), ScalarValue::Int(v)) => whole(ui, *v, r.hash, hash_name),
        _ => {
            value(ui, r, hash_name);
            None
        }
    };
    Some((edit, written?))
}

/// A whole number, or a hash in hex with the name it was made from beside it.
fn whole(ui: &mut egui::Ui, held: i128, hash: bool, name: Option<&str>) -> Option<ScalarValue> {
    fn style(d: egui::DragValue<'_>, hash: bool) -> egui::DragValue<'_> {
        if hash {
            d.hexadecimal(8, false, true).prefix("0x")
        } else {
            d.speed(1.0)
        }
    }
    ui.horizontal(|ui| {
        let edited = if let Ok(mut n) = i64::try_from(held) {
            (drag_value(ui, &mut n, |d| style(d, hash)).inner).then(|| i128::from(n))
        } else if let Ok(mut n) = u64::try_from(held) {
            (drag_value(ui, &mut n, |d| style(d, hash)).inner).then(|| i128::from(n))
        } else {
            None
        };
        if let Some(name) = name {
            ui.label(egui::RichText::new(name).color(theme::TEXT_SECONDARY));
        }
        edited.map(ScalarValue::Int)
    })
    .inner
}

/// A value this panel does not write: why on hover, and a click copies it.
fn value(ui: &mut egui::Ui, r: &Row, hash_name: Option<&str>) {
    let text = match (&r.held, r.hash) {
        (Some(ScalarValue::Int(v)), true) => hash_text(*v as u32, hash_name),
        _ => r.value.clone(),
    };
    let hover = match r.fixed {
        Some(why) => format!("{why} Click to copy."),
        None => "Click to copy.".to_string(),
    };
    let shown = ui
        .add(egui::Label::new(egui::RichText::new(&text).monospace()).sense(egui::Sense::click()))
        .on_hover_text(hover);
    if shown.clicked() {
        ui.ctx().copy_text(text);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui::accesskit::Role;
    use or_io::desc::{FieldRepr, Image, IntWidth, Pack, PackEnum, PackField, PackType, Scalar};

    use super::*;

    fn field(name: &str, offset: usize, repr: FieldRepr) -> PackField {
        PackField {
            name: name.into(),
            offset,
            repr,
            space: None,
            bits: Vec::new(),
            note: None,
        }
    }

    /// `Probe`: a `kind` no variant of `ProbeKind` names, then a `radius`.
    fn probe() -> Pack {
        let mut pack = Pack::new("t", "g");
        pack.types.push(PackType {
            name: "Probe".into(),
            size: 0x10,
            fields: vec![
                field(
                    "kind",
                    0,
                    FieldRepr::Enum {
                        type_name: "ProbeKind".into(),
                        width: IntWidth::U8,
                    },
                ),
                field("radius", 4, FieldRepr::Scalar(Scalar::F32)),
            ],
            selector: None,
        });
        pack.enums.push(PackEnum {
            name: "ProbeKind".into(),
            width: IntWidth::U8,
            variants: vec![PackVariant {
                name: "Sphere".into(),
                value: 0,
                note: Some("A sphere.".into()),
            }],
        });
        pack
    }

    const UNNAMED: u8 = 77;
    const RADIUS: f32 = 0.123_456_79;

    /// The active document of a state, over `Probe`'s bytes.
    fn holding_a_probe() -> AppState {
        let mut data = vec![0u8; 0x20];
        data[0] = UNNAMED;
        data[4..8].copy_from_slice(&RADIUS.to_le_bytes());
        let mut state = AppState::default();
        let id = 1;
        let doc = crate::state::Document::new(
            id,
            None,
            DocPayload::Resource(or_io::Asset::Drawable(Default::default())),
        )
        .with_resource(Some(Arc::new(crate::state::ResourceImage {
            seal: or_io::container::resource::Rsc7::invent_single_chunk_seal(165, 0x10)
                .expect("a seal"),
            data,
            virtual_size: 0x10,
        })));
        state.documents.push(doc);
        state.set_active_document(id);
        state
    }

    fn bytes(state: &AppState) -> Vec<u8> {
        let doc = state.active_document().expect("a document");
        doc.resource.as_ref().expect("an image").data.clone()
    }

    /// One frame of `Probe`'s grid, read back as each widget's role, words, number and place.
    fn frame(
        ctx: &egui::Context,
        state: &mut AppState,
        pack: &Pack,
        events: Vec<egui::Event>,
    ) -> Vec<(Role, String, Option<f64>, egui::Pos2)> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            let data = bytes(state);
            let summary = or_editor::Summary::of(pack, "Probe", Image::new(&data, 0x10), 0)
                .expect("described");
            let doc = state.active_document.expect("a document");
            draw_editable_summary(ui, "probe", state, doc, &summary);
        });
        let Some(update) = output.platform_output.accesskit_update else {
            return Vec::new();
        };
        (update.nodes.iter())
            .filter_map(|(_, node)| {
                let b = node.bounds()?;
                let text = node.label().or_else(|| node.value()).unwrap_or_default();
                Some((
                    node.role(),
                    text.to_string(),
                    node.numeric_value(),
                    egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32),
                ))
            })
            .collect()
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn click(at: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Types `digit` over the first character of the number beside `label`, as a user retyping one digit does.
    fn retype_first_digit(
        ctx: &egui::Context,
        state: &mut AppState,
        pack: &Pack,
        label: &str,
        digit: &str,
    ) {
        let drawn = frame(ctx, state, pack, Vec::new());
        let row = (drawn.iter())
            .find(|(_, text, ..)| text == label)
            .unwrap_or_else(|| panic!("no `{label}` row in {drawn:?}"))
            .3;
        let number = (drawn.iter())
            .filter(|(role, ..)| *role == Role::SpinButton)
            .min_by(|a, b| (a.3.y - row.y).abs().total_cmp(&(b.3.y - row.y).abs()))
            .expect("a number beside the label")
            .3;
        frame(
            ctx,
            state,
            pack,
            vec![egui::Event::PointerMoved(number), click(number, true)],
        );
        frame(ctx, state, pack, vec![click(number, false)]);
        frame(
            ctx,
            state,
            pack,
            vec![
                key(egui::Key::Home),
                key(egui::Key::Delete),
                egui::Event::Text(digit.into()),
            ],
        );
        frame(ctx, state, pack, vec![key(egui::Key::Enter)]);
        frame(ctx, state, pack, Vec::new());
    }

    #[test]
    fn an_unnamed_enum_value_survives_a_view_and_an_edit_of_another_field() {
        let (ctx, pack) = (egui::Context::default(), probe());
        ctx.enable_accesskit();
        let mut state = holding_a_probe();

        let drawn = frame(&ctx, &mut state, &pack, Vec::new());
        assert!(
            drawn
                .iter()
                .any(|(_, text, number, _)| text.starts_with("77")
                    || *number == Some(f64::from(UNNAMED))),
            "the unnamed value is not shown as its number: {drawn:?}"
        );

        retype_first_digit(&ctx, &mut state, &pack, "Radius", "2");
        let data = bytes(&state);
        assert_ne!(
            f32::from_le_bytes(data[4..8].try_into().expect("four bytes")),
            RADIUS,
            "the other field was not edited"
        );
        assert_eq!(data[0], UNNAMED, "editing another field rewrote the enum");
        let drawn = frame(&ctx, &mut state, &pack, Vec::new());
        assert!(
            drawn
                .iter()
                .any(|(_, text, number, _)| text.starts_with("77")
                    || *number == Some(f64::from(UNNAMED))),
            "after the edit the unnamed value is not shown as its number: {drawn:?}"
        );
    }

    #[test]
    fn a_float_keeps_its_full_value_after_an_edit_of_another_digit() {
        let (ctx, pack) = (egui::Context::default(), probe());
        ctx.enable_accesskit();
        let mut state = holding_a_probe();

        retype_first_digit(&ctx, &mut state, &pack, "Radius", "2");

        let data = bytes(&state);
        let written = f32::from_le_bytes(data[4..8].try_into().expect("four bytes"));
        let expected = format!("2{}", &RADIUS.to_string()[1..])
            .parse::<f32>()
            .expect("a number");
        assert_eq!(
            written, expected,
            "retyping the units digit of {RADIUS} wrote {written}"
        );
        assert_eq!(state.undo().as_deref(), Some("Set Radius"));
        assert_eq!(&bytes(&state)[4..8], &RADIUS.to_le_bytes(), "undone");
    }

    #[test]
    fn picking_a_variant_writes_it_as_a_step_that_undoes() {
        let (ctx, pack) = (egui::Context::default(), probe());
        ctx.enable_accesskit();
        let mut state = holding_a_probe();

        let press = |at: egui::Pos2| vec![egui::Event::PointerMoved(at), click(at, true)];
        let centre_of = |drawn: &[(Role, String, Option<f64>, egui::Pos2)], role, text: &str| {
            (drawn.iter())
                .find(|(r, t, ..)| *r == role && t == text)
                .unwrap_or_else(|| panic!("no {role:?} `{text}` in {drawn:?}"))
                .3
        };
        let drawn = frame(&ctx, &mut state, &pack, Vec::new());
        let choice = centre_of(&drawn, Role::ComboBox, "77");
        frame(&ctx, &mut state, &pack, press(choice));
        frame(&ctx, &mut state, &pack, vec![click(choice, false)]);
        // The list lays itself out over two frames.
        let drawn = frame(&ctx, &mut state, &pack, Vec::new());
        let sphere = centre_of(&drawn, Role::Button, "Sphere (0)");
        frame(&ctx, &mut state, &pack, press(sphere));
        frame(&ctx, &mut state, &pack, vec![click(sphere, false)]);

        assert_eq!(bytes(&state)[0], 0, "the pick wrote nothing");
        assert_eq!(state.undo().as_deref(), Some("Set Kind"));
        assert_eq!(
            bytes(&state)[0],
            UNNAMED,
            "undone, the value is not the file's"
        );
    }

    /// The enum fields of `ty` and of what it embeds, as a grid walks them.
    fn enum_fields(pack: &Pack, ty: &PackType, depth: usize) -> usize {
        ty.fields
            .iter()
            .map(|f| match &f.repr {
                FieldRepr::Enum { .. } => 1,
                FieldRepr::Inline { type_name } if depth < 4 => pack
                    .type_by_name(type_name)
                    .map_or(0, |inner| enum_fields(pack, inner, depth + 1)),
                _ => 0,
            })
            .sum()
    }

    #[test]
    fn every_enum_field_draws_as_a_choice() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut state = AppState::default();
        let mut checked = 0;
        for format in or_io::gta5::formats::FORMATS {
            for target in format.targets() {
                let pack = format.description(target).expect("a description");
                for ty in pack.types.iter().filter(|t| t.selector.is_none()) {
                    let enums = enum_fields(&pack, ty, 0);
                    if enums == 0 {
                        continue;
                    }
                    let data = vec![0u8; ty.size.max(1) * 2];
                    let summary =
                        or_editor::Summary::of(&pack, &ty.name, Image::new(&data, data.len()), 0)
                            .expect("described");
                    for internals in [true, false] {
                        state.prefs.show_internals = internals;
                        let shown = (summary.rows.iter())
                            .filter(|r| r.choices.is_some() && !r.internal)
                            .count();
                        let expected = if internals { enums } else { shown };
                        let mut combos = 0;
                        for _ in 0..2 {
                            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                                let doc = DocumentId::default();
                                draw_editable_summary(ui, &ty.name, &mut state, doc, &summary)
                            });
                            combos = output.platform_output.accesskit_update.map_or(0, |u| {
                                (u.nodes.iter())
                                    .filter(|(_, n)| n.role() == Role::ComboBox)
                                    .count()
                            });
                        }
                        assert_eq!(
                            combos,
                            expected,
                            "`{}` of .{} for {} draws {combos} choices of {expected} enum fields, \
                             internals {internals}",
                            ty.name,
                            format.ext,
                            target.slug()
                        );
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked > 0, "no declared type holds an enum field");
    }
}
