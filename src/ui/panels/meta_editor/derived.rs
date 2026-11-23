//! A field list derived from a decoded document rather than from a schema, so
//! the visual editor works on a build with no schema snapshot staged.
//!
//! ⚠ **Derive the widget from the `Value` variant, write the same variant
//! back, never choose a representation.** A [`FieldType`] chosen here picks a
//! *widget*; the `Value` keeps its own type, and the binary writer resolves
//! the real width from the file's own layout. A mapping that narrows a
//! `Value::UInt` into a signed control or flattens a `Value::Hash` into a
//! plain string makes this module a second model of the file.
//!
//! ⚠ **A variant that cannot be edited safely is rendered read-only with the
//! reason, not guessed at.** [`READ_ONLY`] is that list.

use or_io::meta::instance::{MetaInstance, Value};
use or_io::meta::schema::{
    ArrayKind, BitsetStorage, Field, FieldHint, FieldType, PointerPolicy, StringKind,
};

/// Variants no widget here will write, and why.
///
/// ⚠ Each reason names what the value *is*, never "unsupported", which a user
/// cannot tell from broken.
pub(super) const READ_ONLY: &[(&str, &str)] = &[
    ("Mat34", "a 3x4 matrix - edit it in the Text tab"),
    ("Raw", "an undecoded node - edit it in the Text tab"),
    ("Null", "absent - the document does not carry this field"),
];

fn read_only(kind: &str) -> FieldType {
    let reason = READ_ONLY
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, why)| *why)
        .unwrap_or(kind);
    FieldType::Unknown(reason.to_string())
}

/// The widget type for a value, chosen so the widget reads and writes the
/// value's own variant.
///
/// ⚠ **Widths are the widest of their family** — `S64`, `U64`, `Double`.
/// `Value::Int` does not say whether the file stored one byte or eight, and
/// guessing narrow silently truncates; the writer takes the real width from
/// the layout.
pub(super) fn widget_type(value: &Value) -> FieldType {
    match value {
        Value::Bool(_) => FieldType::Bool,
        Value::Int(_) => FieldType::S64,
        Value::UInt(_) => FieldType::U64,
        Value::Float(_) => FieldType::Double,
        Value::String(_) => FieldType::String(StringKind::Plain),
        // ⚠ Not `Plain`. A hashed string's widget keeps the hash beside the
        // text; `Plain` would render it as text and write back a
        // `Value::String`, losing the hash the file is keyed by.
        Value::Hash(_) => FieldType::String(StringKind::AtHashString),
        Value::Vec2(_) => FieldType::Vector2,
        Value::Vec3(_) => FieldType::Vector3,
        Value::Vec4(_) => FieldType::Vector4,
        Value::Color(_) => FieldType::Color32,
        Value::Enum { type_name, .. } => FieldType::Enum(type_name.clone()),
        // ⚠ `Variable` rather than a fixed width, for the same reason the
        // integers take the widest: `Value::Bitset` carries `u128` of bits
        // and does not say what the file allocated.
        Value::Bitset { type_name, .. } => FieldType::Bitset {
            storage: BitsetStorage::Variable,
            values: type_name.clone(),
        },
        Value::Struct(inner) => FieldType::Struct(inner.type_name.clone()),
        Value::Pointer(target) => FieldType::Pointer {
            base: target
                .as_ref()
                .map(|i| i.type_name.clone())
                .unwrap_or_default(),
            policy: PointerPolicy::default(),
        },
        Value::Array(items) => FieldType::Array {
            elem: Box::new(
                items
                    .first()
                    .map(widget_type)
                    // An empty array says nothing about its element, and
                    // inventing one would let the editor append a value of a
                    // type the file never held.
                    .unwrap_or_else(|| read_only("Null")),
            ),
            kind: ArrayKind::default(),
        },
        Value::Map(pairs) => FieldType::Map {
            key: Box::new(
                pairs
                    .first()
                    .map(|(k, _)| widget_type(k))
                    .unwrap_or_else(|| read_only("Null")),
            ),
            value: Box::new(
                pairs
                    .first()
                    .map(|(_, v)| widget_type(v))
                    .unwrap_or_else(|| read_only("Null")),
            ),
        },
        Value::Mat34(_) => read_only("Mat34"),
        Value::ByteArray(_) => FieldType::Unknown(super::bytes::TAG.to_string()),
        Value::Raw(_) => read_only("Raw"),
        Value::Null => read_only("Null"),
    }
}

/// The field list an instance describes about itself.
pub(super) fn fields_of(inst: &MetaInstance) -> Vec<Field> {
    inst.fields
        .iter()
        .map(|(name, value)| Field {
            name: name.clone(),
            ty: widget_type(value),
            init: None,
            min: None,
            max: None,
            step: None,
            ui_key: false,
            hide_widgets: false,
            description: None,
            hint: FieldHint::None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::discriminant;

    fn every_variant() -> Vec<(&'static str, Value)> {
        vec![
            ("Bool", Value::Bool(true)),
            ("Int", Value::Int(-7)),
            ("UInt", Value::UInt(7)),
            ("Float", Value::Float(1.5)),
            ("String", Value::String("s".into())),
            (
                "Hash",
                Value::Hash(or_io::meta::instance::HashedString {
                    raw: "prop_bench_01a".into(),
                    hash: 0x1234_5678,
                }),
            ),
            ("Vec2", Value::Vec2([1.0, 2.0])),
            ("Vec3", Value::Vec3([1.0, 2.0, 3.0])),
            ("Vec4", Value::Vec4([1.0, 2.0, 3.0, 4.0])),
            ("Mat34", Value::Mat34([[1.0; 4]; 3])),
            ("Color", Value::Color(0x11223344)),
            (
                "Enum",
                Value::Enum {
                    type_name: "eLodLevel".into(),
                    variant: "LODTYPES_DEPTH_HD".into(),
                },
            ),
            (
                "Bitset",
                Value::Bitset {
                    type_name: "eFlags".into(),
                    bits: 0b1011,
                },
            ),
            (
                "Struct",
                Value::Struct(Box::new(MetaInstance::new("CEntityDef"))),
            ),
            (
                "Pointer",
                Value::Pointer(Some(Box::new(MetaInstance::new("CEntityDef")))),
            ),
            ("Array", Value::Array(vec![Value::UInt(1)])),
            ("Map", Value::Map(vec![(Value::UInt(1), Value::Bool(true))])),
            ("ByteArray", Value::ByteArray(vec![1, 2, 3])),
            ("Raw", Value::Raw("<node/>".into())),
            ("Null", Value::Null),
        ]
    }

    /// Drawing a derived field is side-effect-free. ⚠ It proves nothing about
    /// what a widget writes: `egui::__run_test_ui` renders with no
    /// interaction, so mapping `Value::UInt` to `S64` still passes it.
    #[test]
    fn no_variant_is_widened_or_narrowed_by_being_rendered() {
        let ctx = super::super::ctx::EditorCtx::for_test();
        let mut changed = Vec::new();

        for (label, value) in every_variant() {
            let before = discriminant(&value);
            let mut inst = MetaInstance::new("T");
            inst.set("f".to_string(), value);
            let fields = fields_of(&inst);

            let mut dirty = false;
            egui::__run_test_ui(|ui| {
                super::super::widgets::draw_struct(ui, &ctx, &fields, &mut inst, &mut dirty);
            });

            let after = inst.get("f").expect("the field survives");
            if discriminant(after) != before {
                changed.push(format!("{label} rendered as {after:?}"));
            }
        }

        assert!(
            changed.is_empty(),
            "rendering changed a value's variant, so the widget writes a type the file did \
             not hold:\n  {}",
            changed.join("\n  ")
        );
    }

    /// Compares the mapping against its own inverse: `widgets::default_value`
    /// decides which `Value` a `FieldType` manufactures, so a derived type
    /// that manufactures a different variant has a widget that writes one.
    /// Mapping `Value::UInt` to `S64` fails this, naming both.
    #[test]
    fn a_derived_type_manufactures_the_variant_it_came_from() {
        use std::mem::discriminant;

        let read_only: Vec<&str> = READ_ONLY.iter().map(|(k, _)| *k).collect();
        let mut wrong = Vec::new();

        for (label, value) in every_variant() {
            if read_only.contains(&label) {
                // Its widget writes nothing, so there is no variant to agree
                // with; every_read_only_variant_says_what_it_is is its check.
                continue;
            }
            let ty = widget_type(&value);
            let manufactured = super::super::widgets::default_value(&ty);
            if discriminant(&manufactured) != discriminant(&value) {
                wrong.push(format!(
                    "{label} derives {ty:?}, which manufactures {manufactured:?}"
                ));
            }
        }

        assert!(
            wrong.is_empty(),
            "a derived widget type does not round-trip to its own variant, so the widget \\
             writes a type the file did not hold:\n  {}",
            wrong.join("\n  ")
        );
    }

    /// A field derived from a value must not be *created* by being drawn:
    /// `draw_field_row` inserts a default when a field is missing, and a
    /// derived list is built from fields that exist, so that path must stay
    /// unreached. It fires only if the name round-trip breaks.
    #[test]
    fn a_derived_field_list_names_the_fields_that_are_there() {
        let mut inst = MetaInstance::new("T");
        inst.set("alpha".to_string(), Value::UInt(1));
        inst.set("beta".to_string(), Value::Bool(false));

        let derived = fields_of(&inst);
        let names: Vec<&str> = derived.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    /// A variant leaving [`READ_ONLY`] claims its widget now preserves its
    /// variant, which `a_derived_type_manufactures_the_variant_it_came_from`
    /// checks.
    #[test]
    fn every_read_only_variant_says_what_it_is() {
        for (kind, reason) in READ_ONLY {
            let FieldType::Unknown(rendered) = read_only(kind) else {
                panic!("{kind} is not rendered read-only");
            };
            assert_eq!(&rendered, reason);
            assert!(
                reason.len() > "unsupported".len(),
                "{kind}: a reason has to name what the value is"
            );
        }
    }

    /// An empty array or map cannot say what it holds, and the editor must
    /// not invent an element type it could then append.
    #[test]
    fn an_empty_collection_does_not_invent_its_element_type() {
        let FieldType::Array { elem, .. } = widget_type(&Value::Array(Vec::new())) else {
            panic!("not an array");
        };
        assert!(matches!(*elem, FieldType::Unknown(_)));

        let FieldType::Array { elem, .. } = widget_type(&Value::Array(vec![Value::Bool(true)]))
        else {
            panic!("not an array");
        };
        assert!(matches!(*elem, FieldType::Bool));
    }

    /// Every text egui painted, nested shapes included.
    fn painted_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
                egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    /// Two arrays each holding an element `[0]` draw without a widget ID clash.
    #[test]
    fn two_arrays_elements_do_not_share_a_widget_id() {
        let ctx = super::super::ctx::EditorCtx::for_test();
        let mut inst = MetaInstance::new("CMapData");
        for (name, element) in [
            ("physicsDictionaries", "CMapDataGroup"),
            ("txdRelationships", "CHDTxdAssetBinding"),
        ] {
            inst.set(
                name.to_string(),
                Value::Array(vec![Value::Struct(Box::new(MetaInstance::new(element)))]),
            );
        }
        let fields = fields_of(&inst);
        let egui_ctx = egui::Context::default();
        egui_ctx.options_mut(|o| o.warn_on_id_clash = true);
        let mut dirty = false;
        let out = egui_ctx.run_ui(egui::RawInput::default(), |ui| {
            super::super::widgets::draw_struct(ui, &ctx, &fields, &mut inst, &mut dirty);
        });
        let clashes: Vec<String> = painted_text(&out.shapes)
            .into_iter()
            .filter(|t| t.contains("widget ID"))
            .collect();
        assert!(
            clashes.is_empty(),
            "egui reported a widget ID clash: {clashes:?}"
        );
    }

    /// Every text egui painted, with where it was painted.
    fn painted_at(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(text) => out.push((
                    text.galley.text().to_string(),
                    text.galley.rect.translate(text.pos.to_vec2()),
                )),
                egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    /// A document read without a schema repeats a name and a type (a root
    /// `Item` array): each row draws its own value under its own ID, and a
    /// struct of a type the library lacks opens onto the fields it carries.
    #[test]
    fn repeated_names_draw_each_value_and_unknown_structs_open() {
        let ctx = super::super::ctx::EditorCtx::for_test();
        let mut inst = MetaInstance::new("dataFiles");
        for field in ["alpha", "beta"] {
            let mut item = MetaInstance::new("hash_0000000A");
            item.set(field.to_string(), Value::Bool(true));
            inst.fields
                .push(("Item".to_string(), Value::Struct(Box::new(item))));
        }
        let fields = fields_of(&inst);
        let egui_ctx = egui::Context::default();
        egui_ctx.options_mut(|o| o.warn_on_id_clash = true);
        let mut dirty = false;
        let mut frame = |events: Vec<egui::Event>, time: f64| {
            let input = egui::RawInput {
                events,
                time: Some(time),
                ..Default::default()
            };
            let out = egui_ctx.run_ui(input, |ui| {
                super::super::widgets::draw_struct(ui, &ctx, &fields, &mut inst, &mut dirty);
            });
            painted_at(&out.shapes)
        };

        let first = frame(Vec::new(), 0.0);
        let texts: Vec<&str> = first.iter().map(|(t, _)| t.as_str()).collect();
        assert!(
            !texts.iter().any(|t| t.contains("widget ID")),
            "egui reported a widget ID clash: {texts:?}"
        );
        let header = first
            .iter()
            .filter(|(t, _)| t == "hash_0000000A")
            .nth(1)
            .unwrap_or_else(|| panic!("the second `Item` was not drawn: {texts:?}"))
            .1
            .center();

        // ⚠ A press held past egui's click duration is a drag, not a click.
        let button = |pressed| egui::Event::PointerButton {
            pos: header,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(vec![egui::Event::PointerMoved(header), button(true)], 1.0);
        frame(vec![button(false)], 1.1);
        frame(Vec::new(), 3.0);
        let opened = frame(Vec::new(), 4.0);
        let texts: Vec<&str> = opened.iter().map(|(t, _)| t.as_str()).collect();
        assert!(
            texts.contains(&"beta")
                && !texts.contains(&"alpha")
                && !texts
                    .iter()
                    .any(|t| t.contains("no schema") || t.contains("widget ID")),
            "opening the second `Item` did not show its own fields alone: {texts:?}"
        );
    }

    /// A pointer to a type the library lacks draws the fields it carries.
    #[test]
    fn a_pointer_to_an_unknown_type_draws_its_fields() {
        let ctx = super::super::ctx::EditorCtx::for_test();
        let mut target = MetaInstance::new("hash_0000000C");
        target.set("gamma".to_string(), Value::Bool(true));
        let mut inst = MetaInstance::new("T");
        inst.set("p".to_string(), Value::Pointer(Some(Box::new(target))));
        let field = fields_of(&inst).remove(0);
        let value = &mut inst.fields[0].1;
        let mut dirty = false;
        let egui_ctx = egui::Context::default();
        // ⚠ Drawn outside a grid row, which lays the target out to the right
        // of the full-width pointer header; a grid's first frame paints nothing.
        let mut out = None;
        for _ in 0..2 {
            out = Some(egui_ctx.run_ui(egui::RawInput::default(), |ui| {
                super::super::widgets::draw_field(
                    ui,
                    &ctx.field("p", 0),
                    &field,
                    value,
                    &mut dirty,
                );
            }));
        }
        let texts = painted_text(&out.expect("two frames ran").shapes);
        assert!(
            texts.iter().any(|t| t == "gamma") && !texts.iter().any(|t| t.contains("no schema")),
            "the pointer's target did not show its own fields: {texts:?}"
        );
    }
}
