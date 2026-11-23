use or_io::desc::{
    Cursor, FieldRepr, Image, IntWidth, Kind, Pack, PackField, PackType, PackVariant, Plans,
    Pointee, Scalar, ScalarValue, TypePlan,
};
use std::collections::BTreeSet;
use std::fmt::Write as _;

pub mod outline;
mod shapes;
pub use outline::{resolve_count, Cut, Hop, Node, Outline, PackPath, MAX_DEPTH, MAX_ELEMENTS};
pub use shapes::shapes_data;
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Row {
    pub label: String,
    pub value: String,
    pub note: Option<String>,
    pub edit: Option<Edit>,
    /// The scalar the bytes hold, which an edit starts from: `value` is rounded to be read.
    pub held: Option<ScalarValue>,
    /// What an array of scalars or an opaque region holds to the last digit or byte, where `value` is rounded or says only how much there is.
    pub exact: Option<String>,
    /// An enum field's declared variants, the values it is chosen among.
    pub choices: Option<Vec<PackVariant>>,
    /// A 32-bit field the declaration types a hash or the engine names one, whose `value` reads `0x` and eight digits.
    pub hash: bool,
    /// What the owned value keeps to itself (padding, a count, a reference count, a vft word,
    /// an address) or follows as a pointer, which a default view leaves out.
    pub internal: bool,
    /// Why a row with no `edit` is not written here.
    pub fixed: Option<&'static str>,
}

const COUNTED: &str = "A count, which a save keeps in step with what it counts.";
const TAG: &str =
    "It says how the bytes after it are laid out, so another value would misread them.";
const KEPT: &str = "The engine's own bookkeeping, which a save writes back as the file has it.";
const FOLLOWED: &str =
    "Where something lies in the file, which a save works out; the outline lists what it holds.";
const PAST_THE_END: &str = "Past the end of the file's bytes.";
const TEXT: &str = "Text is not edited here.";
const NOT_A_VALUE: &str = "Not one value: the outline lists what it holds.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edit {
    pub offset: usize,
    pub kind: EditKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Bool,
    F32,
    F64,
    Int(IntWidth),
    Enum(IntWidth),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Summary {
    pub title: String,
    pub rows: Vec<Row>,
}

impl Summary {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn push(&mut self, label: impl Into<String>, value: impl Into<String>) {
        self.rows.push(Row {
            label: label.into(),
            value: value.into(),
            ..Row::default()
        });
    }

    pub fn editable(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|r| r.edit.is_some())
    }

    /// Every field of the struct of `type_name` at `at`, read through the pack's type plan.
    pub fn of(pack: &Pack, type_name: &str, image: Image<'_>, at: usize) -> Option<Self> {
        Self::of_shown(pack, type_name, image, at, &|_, _| true, &|_, _| false)
    }

    /// [`Summary::of`], with each field `shown(type, field)` refuses internal and read-only, and each `hash(type, field)` accepts a hash.
    pub fn of_shown(
        pack: &Pack,
        type_name: &str,
        image: Image<'_>,
        at: usize,
        shown: &dyn Fn(&str, &str) -> bool,
        hash: &dyn Fn(&str, &str) -> bool,
    ) -> Option<Self> {
        let plans = pack.plans();
        let plan = plans.get(type_name).ok()?;

        let plan = match plan
            .selector()
            .and_then(|_| resolve_tag(pack, plan.pack_type(), image.at(at)))
            .and_then(|concrete| plans.get(&concrete.name).ok())
        {
            Some(concrete) => concrete,
            None => plan,
        };

        let mut rows = Vec::new();
        let walk = Walk {
            pack,
            plans,
            image,
            shown,
            hash,
        };
        rows_of(&walk, plan, at, "", 0, false, &mut rows);
        Some(Self {
            title: plan.name().to_string(),
            rows,
        })
    }
}

const MAX_INLINE_DEPTH: usize = 4;
const MAX_EXPLODED_ELEMENTS: usize = 4;

/// What a walk of one struct reads through, and the fields its owned value shows.
struct Walk<'a, 'i> {
    pack: &'a Pack,
    plans: &'a Plans,
    image: Image<'i>,
    shown: &'a dyn Fn(&str, &str) -> bool,
    hash: &'a dyn Fn(&str, &str) -> bool,
}

fn rows_of(
    walk: &Walk<'_, '_>,
    plan: &TypePlan,
    base: usize,
    prefix: &str,
    depth: usize,
    hidden: bool,
    out: &mut Vec<Row>,
) {
    let (image, ty) = (walk.image, plan.pack_type());
    // One check for the whole struct; a struct the image cannot hold whole
    // falls back to a checked read per field, so a short one still has rows.
    let record = plan.record(image.bytes(), base);
    let scalar_at = |path: &str, at: usize, scalar: Scalar| match (record, plan.field(path)) {
        (Some(rec), Some(field)) if field.kind.is_some() => rec.scalar(field),
        _ => {
            let kind = Kind::of_scalar(scalar);
            image
                .bytes()
                .get(at..at + kind.size())
                .and_then(|b| kind.decode(b))
        }
    };
    let counted = counted(ty);
    for field in &ty.fields {
        let label = if prefix.is_empty() {
            field.display_name()
        } else {
            format!("{prefix} > {}", field.display_name())
        };
        let at = base + field.offset;
        let internal = hidden
            || !(walk.shown)(&ty.name, &field.name)
            || counted.contains(field.name.as_str())
            || keeps_an_address(&field.repr)
            || matches!(field.repr, FieldRepr::Padding { .. });
        let row = |label: String| Row {
            label,
            note: field.note.clone(),
            internal,
            ..Row::default()
        };
        // The scalar at `path`, `at`, and how this row writes it.
        let scalar_row = |label: String, path: &str, at: usize, scalar: Scalar, decimals: usize| {
            let held = scalar_at(path, at, scalar);
            let why = fixed(ty, field, at, image, internal);
            let hash = reads_as_a_hash(field, scalar, (walk.hash)(&ty.name, &field.name));
            Row {
                value: held
                    .as_ref()
                    .map_or_else(short_read, |v| render_value(v, decimals, hash)),
                edit: why.is_none().then_some(Edit {
                    offset: at,
                    kind: edit_kind(scalar),
                }),
                held,
                hash,
                fixed: why,
                ..row(label)
            }
        };

        match &field.repr {
            FieldRepr::Inline { type_name } => match walk.plans.get(type_name) {
                Ok(inner) if depth < MAX_INLINE_DEPTH => {
                    rows_of(walk, inner, at, &label, depth + 1, internal, out)
                }
                _ => out.push(Row {
                    value: format!("({type_name})"),
                    fixed: Some(NOT_A_VALUE),
                    ..row(label)
                }),
            },
            FieldRepr::ScalarArray { element, count } if *count <= MAX_EXPLODED_ELEMENTS => {
                for i in 0..*count {
                    out.push(scalar_row(
                        format!("{label} [{i}]"),
                        &format!("{}[{i}]", field.name),
                        at + i * element.size(),
                        *element,
                        3,
                    ));
                }
            }
            FieldRepr::Scalar(scalar) => out.push(scalar_row(label, &field.name, at, *scalar, 4)),
            FieldRepr::Enum { type_name, width } => {
                let held = width.read(image.bytes(), at);
                let declared = walk.pack.enum_by_name(type_name);
                let why = fixed(ty, field, at, image, internal);
                out.push(Row {
                    value: held.map_or_else(short_read, |v| {
                        match declared.and_then(|e| e.name_of(v as i64)) {
                            Some(name) => format!("{name} ({v})"),
                            None => v.to_string(),
                        }
                    }),
                    edit: (why.is_none() && held.is_some()).then_some(Edit {
                        offset: at,
                        kind: EditKind::Enum(*width),
                    }),
                    held: held.map(ScalarValue::Int),
                    choices: Some(declared.map(|e| e.variants.clone()).unwrap_or_default()),
                    fixed: why,
                    ..row(label)
                });
            }
            _ => out.push(Row {
                value: render(walk.pack, ty, field, image, base),
                exact: exact(&field.repr, image.bytes(), at),
                fixed: Some(match &field.repr {
                    FieldRepr::Ptr(Pointee::Str) => TEXT,
                    FieldRepr::Padding { .. } => KEPT,
                    repr if keeps_an_address(repr) => FOLLOWED,
                    _ => NOT_A_VALUE,
                }),
                ..row(label)
            }),
        }
    }
}

/// The fields of `ty` a run's length reads, which a save keeps in step; never its tag.
fn counted(ty: &PackType) -> BTreeSet<&str> {
    let mut out = ty.structural_fields();
    if let Some(selector) = &ty.selector {
        out.remove(selector.tag.as_str());
    }
    out
}

/// A pointer, a run or a trailing run: an address the owned value follows rather than shows.
fn keeps_an_address(repr: &FieldRepr) -> bool {
    repr.is_trailing() || (repr.is_indirect() && !matches!(repr, FieldRepr::Ptr(Pointee::Str)))
}

/// Why `field` of `ty` at `at` is not written here, if it is not.
fn fixed(
    ty: &PackType,
    field: &PackField,
    at: usize,
    image: Image<'_>,
    internal: bool,
) -> Option<&'static str> {
    let width = field.repr.inline_size().unwrap_or(0);
    if ty.selector.as_ref().is_some_and(|s| s.tag == field.name) {
        Some(TAG)
    } else if ty.is_structural(&field.name) {
        Some(COUNTED)
    } else if let Some(why) = shapes_data(&ty.name, &field.name) {
        Some(why)
    } else if internal {
        Some(KEPT)
    } else if at.checked_add(width).is_none_or(|end| end > image.len()) {
        Some(PAST_THE_END)
    } else {
        None
    }
}

fn edit_kind(scalar: Scalar) -> EditKind {
    match scalar {
        Scalar::Bool => EditKind::Bool,
        Scalar::F32 => EditKind::F32,
        Scalar::F64 => EditKind::F64,
        Scalar::Int(w) => EditKind::Int(w),
    }
}

/// A 32-bit field the declaration types a hash, or the engine names one.
fn reads_as_a_hash(field: &PackField, scalar: Scalar, declared: bool) -> bool {
    matches!(scalar, Scalar::Int(IntWidth::U32 | IntWidth::I32))
        && (declared || field.name.split('_').any(|word| word == "hash"))
}

/// A hash as `0x` and eight digits.
fn render_value(value: &ScalarValue, decimals: usize, hash: bool) -> String {
    match value {
        ScalarValue::Bool(false) => "no".into(),
        ScalarValue::Bool(true) => "yes".into(),
        ScalarValue::F32(v) => format!("{v:.decimals$}"),
        ScalarValue::F64(v) => format!("{v:.decimals$}"),
        ScalarValue::Int(v) if hash => format!("0x{:08X}", *v as u32),
        ScalarValue::Int(v) => v.to_string(),
    }
}

/// What an array of scalars, or an opaque region, at `at` holds in full, which `render` rounds or leaves as its size.
fn exact(repr: &FieldRepr, bytes: &[u8], at: usize) -> Option<String> {
    match repr {
        FieldRepr::ScalarArray { element, count } => {
            let kind = Kind::of_scalar(*element);
            let each = |i: usize| {
                let from = at + i * element.size();
                let held = bytes
                    .get(from..from + kind.size())
                    .and_then(|b| kind.decode(b));
                held.map_or_else(short_read, |v| match v {
                    ScalarValue::F32(v) => v.to_string(),
                    ScalarValue::F64(v) => v.to_string(),
                    v => render_value(&v, 0, false),
                })
            };
            Some((0..*count).map(each).collect::<Vec<_>>().join(", "))
        }
        FieldRepr::Opaque { size } => {
            let held = bytes.get(at..at.checked_add(*size)?)?;
            Some(
                held.iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        }
        _ => None,
    }
}

fn resolve_tag<'p>(pack: &'p Pack, base: &PackType, at: Cursor<'_>) -> Option<&'p PackType> {
    let selector = base.selector.as_ref()?;
    let tag = selector.read_tag(base, at.bytes(), at.base())?;
    pack.type_by_name(selector.type_for(tag)?)
}

fn render(pack: &Pack, ty: &PackType, field: &PackField, image: Image<'_>, base: usize) -> String {
    let at = base + field.offset;
    let bytes = image.bytes();

    match &field.repr {
        FieldRepr::Scalar(Scalar::Bool) => match bytes.get(at) {
            Some(0) => "no".into(),
            Some(_) => "yes".into(),
            None => short_read(),
        },
        FieldRepr::Scalar(Scalar::F32) => read_f32(bytes, at)
            .map(|v| format!("{v:.4}"))
            .unwrap_or_else(short_read),
        FieldRepr::Scalar(Scalar::F64) => read_f64(bytes, at)
            .map(|v| format!("{v:.4}"))
            .unwrap_or_else(short_read),
        FieldRepr::Scalar(Scalar::Int(w)) => w
            .read(bytes, at)
            .map(|v| v.to_string())
            .unwrap_or_else(short_read),

        FieldRepr::Enum { type_name, width } => match width.read(bytes, at) {
            Some(v) => match pack
                .enums
                .iter()
                .find(|e| &e.name == type_name)
                .and_then(|e| e.name_of(v as i64))
            {
                Some(name) => format!("{name} ({v})"),
                None => format!("{v} (no name in {type_name})"),
            },
            None => short_read(),
        },

        FieldRepr::ScalarArray { element, count } => {
            let mut out = String::new();
            for i in 0..*count {
                if i > 0 {
                    out.push_str(", ");
                }
                let elem_at = at + i * element.size();
                match element {
                    Scalar::F32 => {
                        let _ = match read_f32(bytes, elem_at) {
                            Some(v) => write!(out, "{v:.3}"),
                            None => write!(out, "?"),
                        };
                    }
                    Scalar::Int(w) => {
                        let _ = match w.read(bytes, elem_at) {
                            Some(v) => write!(out, "{v}"),
                            None => write!(out, "?"),
                        };
                    }
                    _ => out.push('?'),
                }
            }
            out
        }

        FieldRepr::Inline { type_name } => format!("({type_name})"),
        FieldRepr::InlineArray { type_name, count } => format!("{count} x {type_name}"),
        FieldRepr::Ptr(pointee) => render_pointer(image, at, pointee),
        FieldRepr::AtArray(pointee) => match image.at_array(at) {
            Ok(Some(elements)) => format!("{} {}", elements.count(), plural_of(pointee)),
            Ok(None) => "empty".into(),
            Err(e) => e.to_string(),
        },
        FieldRepr::PtrSlots { pointee, count } => {
            let filled = (0..*count)
                .filter(|i| {
                    image
                        .pointer_at(at + i * or_io::desc::POINTER_SIZE)
                        .is_ok_and(|p| p != 0)
                })
                .count();
            format!("{filled} of {count} {}", plural_of(pointee))
        }

        FieldRepr::Opaque { size } => format!("{size} bytes"),
        FieldRepr::Trailing {
            type_name, count, ..
        } => {
            let what = type_name.as_deref().unwrap_or("bytes");
            let mut raws = Vec::with_capacity(2);
            for name in count.reads_fields() {
                let Some(sibling) = ty.field_by_name(name) else {
                    break;
                };
                let read = match &sibling.repr {
                    FieldRepr::Scalar(Scalar::Int(w)) => w.read(bytes, base + sibling.offset),
                    FieldRepr::Enum { width, .. } => width.read(bytes, base + sibling.offset),
                    _ => None,
                };
                match read {
                    Some(raw) => raws.push(raw),
                    None => break,
                }
            }
            match count.length_from(&raws) {
                Some(n) => format!("{n} trailing {what}"),
                None => format!("trailing {what}"),
            }
        }

        FieldRepr::Padding { size } => format!("{size} bytes of padding"),
    }
}

fn render_pointer(image: Image<'_>, at: usize, pointee: &Pointee) -> String {
    match image.pointer_at(at) {
        Err(e) => e.to_string(),
        Ok(0) => "none".into(),
        Ok(_) => match pointee {
            Pointee::Str => image.string_via(at).unwrap_or_else(|e| format!("<{e}>")),
            Pointee::One { type_name } => format!("-> {type_name}"),
            Pointee::Many { type_name, .. } | Pointee::ManyPtr { type_name, .. } => match type_name
            {
                Some(name) => format!("-> [{name}]"),
                None => "-> [..]".into(),
            },
            Pointee::Bytes { .. } => "-> bytes".into(),
            Pointee::Strings { .. } => "-> [strings]".into(),
            Pointee::Opaque => "set".into(),
            Pointee::Member { type_name } => format!("-> its own {type_name}"),
            Pointee::Members { type_name, .. } => format!("-> [its own {type_name}]"),
        },
    }
}

fn plural_of(pointee: &Pointee) -> String {
    match pointee {
        Pointee::One { type_name }
        | Pointee::Member { type_name }
        | Pointee::Members { type_name, .. }
        | Pointee::Many {
            type_name: Some(type_name),
            ..
        }
        | Pointee::ManyPtr {
            type_name: Some(type_name),
            ..
        } => type_name.clone(),
        _ => "entries".into(),
    }
}

fn short_read() -> String {
    "<past the end of the resource>".into()
}

fn read_f32(bytes: &[u8], at: usize) -> Option<f32> {
    Some(f32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn read_f64(bytes: &[u8], at: usize) -> Option<f64> {
    Some(f64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::desc::{IntWidth, PackEnum, PackSelector, PackVariant};
    fn scalar(name: &str, offset: usize, s: Scalar) -> PackField {
        PackField {
            name: name.into(),
            offset,
            repr: FieldRepr::Scalar(s),
            space: None,
            bits: Vec::new(),
            note: None,
        }
    }

    fn pack_with(name: &str, size: usize, fields: Vec<PackField>) -> Pack {
        let mut pack = Pack::new("t", "g");
        pack.types.push(PackType {
            name: name.into(),
            size,
            fields,
            selector: None,
        });
        pack
    }

    #[test]
    fn a_described_type_produces_a_whole_grid_with_no_per_field_code() {
        let pack = pack_with(
            "GrmGeometry",
            0x20,
            vec![
                scalar("vertices_count", 0x00, Scalar::Int(IntWidth::U32)),
                scalar("vertex_stride", 0x04, Scalar::Int(IntWidth::U16)),
                scalar("is_skinned", 0x06, Scalar::Bool),
            ],
        );

        let mut bytes = vec![0u8; 0x20];
        bytes[0x00..0x04].copy_from_slice(&1204u32.to_le_bytes());
        bytes[0x04..0x06].copy_from_slice(&36u16.to_le_bytes());
        bytes[0x06] = 1;

        let image = Image::new(&bytes, bytes.len());
        let summary = Summary::of(&pack, "GrmGeometry", image, 0).expect("described");

        assert_eq!(summary.title, "GrmGeometry");
        assert_eq!(summary.len(), 3);
        assert_eq!(summary.rows[0].label, "VerticesCount");
        assert_eq!(summary.rows[0].value, "1204");
        assert_eq!(summary.rows[2].value, "yes");
    }

    #[test]
    fn an_enum_field_shows_its_name_and_a_stranger_shows_its_number() {
        let mut pack = pack_with(
            "PhBound",
            0x10,
            vec![PackField {
                name: "bound_type".into(),
                offset: 0,
                repr: FieldRepr::Enum {
                    type_name: "PhBoundType".into(),
                    width: IntWidth::U8,
                },
                space: None,
                bits: Vec::new(),
                note: None,
            }],
        );
        pack.enums.push(PackEnum {
            name: "PhBoundType".into(),
            width: IntWidth::U8,
            variants: vec![PackVariant {
                name: "Geometry".into(),
                value: 4,
                note: None,
            }],
        });

        let mut bytes = vec![0u8; 0x10];
        bytes[0] = 4;
        let image = Image::new(&bytes, bytes.len());
        assert_eq!(
            Summary::of(&pack, "PhBound", image, 0).unwrap().rows[0].value,
            "Geometry (4)"
        );

        bytes[0] = 77;
        let image = Image::new(&bytes, bytes.len());
        let value = &Summary::of(&pack, "PhBound", image, 0).unwrap().rows[0].value;
        assert!(value.starts_with("77"), "got {value}");
    }

    #[test]
    fn a_tagged_base_summarises_the_layout_its_tag_selects() {
        let mut pack = Pack::new("t", "g");
        pack.types.push(PackType {
            name: "PhBound".into(),
            size: 0x10,
            fields: vec![scalar("bound_type", 0, Scalar::Int(IntWidth::U8))],
            selector: Some(PackSelector {
                tag: "bound_type".into(),
                tag_bits: None,
                variants: vec![(4, "PhBoundGeometry".into())],
            }),
        });
        pack.types.push(PackType {
            name: "PhBoundGeometry".into(),
            size: 0x10,
            fields: vec![
                scalar("bound_type", 0, Scalar::Int(IntWidth::U8)),
                scalar("num_polygons", 4, Scalar::Int(IntWidth::U32)),
            ],
            selector: None,
        });

        let mut bytes = vec![0u8; 0x10];
        bytes[0] = 4;
        bytes[4..8].copy_from_slice(&900u32.to_le_bytes());

        let summary = Summary::of(&pack, "PhBound", Image::new(&bytes, bytes.len()), 0).unwrap();
        assert_eq!(summary.title, "PhBoundGeometry");
        assert_eq!(summary.len(), 2);
        assert_eq!(summary.rows[1].value, "900");
    }

    #[test]
    fn a_field_the_bytes_cannot_supply_says_so_instead_of_vanishing() {
        let pack = pack_with(
            "GrmGeometry",
            0x20,
            vec![scalar("vertices_count", 0x1C, Scalar::Int(IntWidth::U64))],
        );
        let bytes = vec![0u8; 0x20];

        let summary =
            Summary::of(&pack, "GrmGeometry", Image::new(&bytes, bytes.len()), 0).unwrap();
        assert_eq!(summary.len(), 1, "the row must still be there");
        assert!(summary.rows[0].value.contains("past the end"));
    }

    #[test]
    fn padding_is_an_internal_row() {
        let pack = pack_with(
            "T",
            0x10,
            vec![
                scalar("count", 0, Scalar::Int(IntWidth::U32)),
                PackField {
                    name: "_pad".into(),
                    offset: 4,
                    repr: FieldRepr::Padding { size: 12 },
                    space: None,
                    bits: Vec::new(),
                    note: None,
                },
            ],
        );
        let bytes = vec![0u8; 0x10];
        let summary = Summary::of(&pack, "T", Image::new(&bytes, bytes.len()), 0).unwrap();
        let internal: Vec<bool> = summary.rows.iter().map(|r| r.internal).collect();
        assert_eq!(internal, [false, true]);
        assert!(summary.rows[1].edit.is_none());
    }

    #[test]
    fn a_null_pointer_reads_as_none_rather_than_as_zero() {
        let pack = pack_with(
            "T",
            0x10,
            vec![PackField {
                name: "skeleton".into(),
                offset: 0,
                repr: FieldRepr::Ptr(Pointee::One {
                    type_name: "CrSkeletonData".into(),
                }),
                space: Some(or_io::desc::AddressSpace::Virtual),
                bits: Vec::new(),
                note: None,
            }],
        );
        let bytes = vec![0u8; 0x10];
        let summary = Summary::of(&pack, "T", Image::new(&bytes, bytes.len()), 0).unwrap();
        assert_eq!(summary.rows[0].value, "none");
    }

    #[test]
    fn a_note_travels_with_the_row() {
        let pack = pack_with(
            "T",
            0x10,
            vec![PackField {
                name: "index_count".into(),
                offset: 0,
                repr: FieldRepr::Scalar(Scalar::Int(IntWidth::U32)),
                space: None,
                bits: Vec::new(),
                note: Some("Shares its top byte with flags.".into()),
            }],
        );
        let bytes = vec![0u8; 0x10];
        let summary = Summary::of(&pack, "T", Image::new(&bytes, bytes.len()), 0).unwrap();
        assert_eq!(
            summary.rows[0].note.as_deref(),
            Some("Shares its top byte with flags.")
        );
    }

    #[test]
    fn an_array_of_many_scalars_and_an_opaque_region_say_what_they_hold_in_full() {
        let field = |name: &str, offset: usize, repr: FieldRepr| PackField {
            name: name.into(),
            offset,
            repr,
            space: None,
            bits: Vec::new(),
            note: None,
        };
        let pack = pack_with(
            "T",
            0x30,
            vec![
                field(
                    "weights",
                    0x00,
                    FieldRepr::ScalarArray {
                        element: Scalar::F32,
                        count: 6,
                    },
                ),
                field("words", 0x18, FieldRepr::Opaque { size: 4 }),
                scalar("count", 0x1C, Scalar::Int(IntWidth::U32)),
            ],
        );
        let mut bytes = vec![0u8; 0x30];
        for (i, weight) in [1.0f32, 2.0, 3.0004, 4.0, 5.0, 6.5].iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&weight.to_le_bytes());
        }
        bytes[0x18..0x1C].copy_from_slice(&[0x01, 0xAB, 0x00, 0xFF]);

        let summary = Summary::of(&pack, "T", Image::new(&bytes, bytes.len()), 0).unwrap();
        assert_eq!(
            summary.rows[0].value,
            "1.000, 2.000, 3.000, 4.000, 5.000, 6.500"
        );
        assert_eq!(
            summary.rows[0].exact.as_deref(),
            Some("1, 2, 3.0004, 4, 5, 6.5"),
            "the row reads at three decimals; the value is not rounded"
        );
        assert_eq!(summary.rows[1].value, "4 bytes");
        assert_eq!(summary.rows[1].exact.as_deref(), Some("01 AB 00 FF"));
        assert_eq!(
            summary.rows[2].exact, None,
            "a scalar's own `held` is exact"
        );
    }

    #[test]
    fn a_type_the_pack_does_not_describe_is_none_not_an_empty_grid() {
        let pack = pack_with("T", 0x10, vec![]);
        let bytes = vec![0u8; 0x10];
        assert!(Summary::of(&pack, "SomethingElse", Image::new(&bytes, 0x10), 0).is_none());
    }

    fn nested_pack() -> Pack {
        let mut pack = Pack::new("t", "g");
        pack.types.push(PackType {
            name: "Outer".into(),
            size: 0x80,
            fields: vec![PackField {
                name: "base".into(),
                offset: 0x10,
                repr: FieldRepr::Inline {
                    type_name: "Middle".into(),
                },
                space: None,
                bits: Vec::new(),
                note: None,
            }],
            selector: None,
        });
        pack.types.push(PackType {
            name: "Middle".into(),
            size: 0x60,
            fields: vec![PackField {
                name: "lod_group".into(),
                offset: 0x20,
                repr: FieldRepr::Inline {
                    type_name: "Leaf".into(),
                },
                space: None,
                bits: Vec::new(),
                note: None,
            }],
            selector: None,
        });
        pack.types.push(PackType {
            name: "Leaf".into(),
            size: 0x30,
            fields: vec![
                PackField {
                    name: "lod_distances".into(),
                    offset: 0x00,
                    repr: FieldRepr::ScalarArray {
                        element: Scalar::F32,
                        count: 4,
                    },
                    space: None,
                    bits: Vec::new(),
                    note: None,
                },
                PackField {
                    name: "num_items".into(),
                    offset: 0x10,
                    repr: FieldRepr::Scalar(Scalar::Int(IntWidth::U16)),
                    space: None,
                    bits: Vec::new(),
                    note: None,
                },
                PackField {
                    name: "items".into(),
                    offset: 0x18,
                    repr: FieldRepr::Ptr(or_io::desc::Pointee::Many {
                        type_name: Some("Item".into()),
                        count: or_io::desc::CountSource::Field("num_items".into()),
                        element_size: None,
                    }),
                    space: Some(or_io::desc::AddressSpace::Virtual),
                    bits: Vec::new(),
                    note: None,
                },
            ],
            selector: None,
        });
        pack
    }

    const LEAF: usize = 0x30;
    #[test]
    fn an_embedded_struct_is_walked_into_rather_than_named() {
        let pack = nested_pack();
        let mut bytes = vec![0u8; 0x100];
        bytes[LEAF..LEAF + 4].copy_from_slice(&25.0f32.to_le_bytes());

        let summary = Summary::of(&pack, "Outer", Image::new(&bytes, 0x80), 0).expect("bind");
        let labels: Vec<&str> = summary.rows.iter().map(|r| r.label.as_str()).collect();

        assert!(
            labels.contains(&"Base > LodGroup > LodDistances [0]"),
            "the walk did not reach the leaf: {labels:?}"
        );
        assert_eq!(summary.rows[0].value, "25.000");
    }

    #[test]
    fn a_short_run_of_scalars_becomes_one_row_each() {
        let pack = nested_pack();
        let bytes = vec![0u8; 0x100];
        let summary = Summary::of(&pack, "Outer", Image::new(&bytes, 0x80), 0).expect("bind");

        let exploded = summary
            .rows
            .iter()
            .filter(|r| r.label.contains("LodDistances"))
            .count();
        assert_eq!(exploded, 4);
    }

    #[test]
    fn every_exploded_element_is_writable_at_its_own_offset() {
        let pack = nested_pack();
        let bytes = vec![0u8; 0x100];
        let summary = Summary::of(&pack, "Outer", Image::new(&bytes, 0x80), 0).expect("bind");

        let offsets: Vec<usize> = summary
            .rows
            .iter()
            .filter(|r| r.label.contains("LodDistances"))
            .map(|r| r.edit.expect("an f32 element is writable").offset)
            .collect();

        assert_eq!(offsets, vec![LEAF, LEAF + 4, LEAF + 8, LEAF + 12]);
        assert!(summary
            .rows
            .iter()
            .filter(|r| r.label.contains("LodDistances"))
            .all(|r| r.edit.map(|e| e.kind) == Some(EditKind::F32)));
    }

    #[test]
    fn a_count_offers_no_handle_however_editable_it_looks() {
        let pack = nested_pack();
        let bytes = vec![0u8; 0x100];
        let summary = Summary::of(&pack, "Outer", Image::new(&bytes, 0x80), 0).expect("bind");

        let count = summary
            .rows
            .iter()
            .find(|r| r.label.ends_with("NumItems"))
            .expect("the count is still shown");
        assert!(count.edit.is_none(), "a count must not be editable");
    }

    #[test]
    fn a_pointer_offers_no_handle_either() {
        let pack = nested_pack();
        let bytes = vec![0u8; 0x100];
        let summary = Summary::of(&pack, "Outer", Image::new(&bytes, 0x80), 0).expect("bind");

        let items = summary
            .rows
            .iter()
            .find(|r| r.label.ends_with("Items"))
            .expect("the pointer is still shown");
        assert!(items.edit.is_none());
    }

    #[test]
    fn a_field_past_the_end_of_the_bytes_offers_no_handle() {
        let pack = nested_pack();
        let bytes = vec![0u8; LEAF + 2];
        let summary = Summary::of(&pack, "Outer", Image::new(&bytes, LEAF), 0).expect("bind");

        assert!(summary.rows.iter().all(|r| r.edit.is_none()));
    }

    #[test]
    fn a_type_that_embeds_itself_truncates_rather_than_recursing_forever() {
        let mut pack = Pack::new("t", "g");
        pack.types.push(PackType {
            name: "Loop".into(),
            size: 0x10,
            fields: vec![PackField {
                name: "self_".into(),
                offset: 0,
                repr: FieldRepr::Inline {
                    type_name: "Loop".into(),
                },
                space: None,
                bits: Vec::new(),
                note: None,
            }],
            selector: None,
        });

        let bytes = vec![0u8; 0x100];
        let summary = Summary::of(&pack, "Loop", Image::new(&bytes, 0x80), 0).expect("bind");
        assert_eq!(summary.rows.len(), 1, "one row, at the depth cap");
    }

    fn kinded(kind: u8) -> (Pack, Vec<u8>) {
        let mut pack = pack_with(
            "PhBound",
            0x10,
            vec![PackField {
                name: "bound_type".into(),
                offset: 0,
                repr: FieldRepr::Enum {
                    type_name: "PhBoundType".into(),
                    width: IntWidth::U8,
                },
                space: None,
                bits: Vec::new(),
                note: None,
            }],
        );
        pack.enums.push(PackEnum {
            name: "PhBoundType".into(),
            width: IntWidth::U8,
            variants: vec![PackVariant {
                name: "Geometry".into(),
                value: 4,
                note: Some("Polygons.".into()),
            }],
        });
        let mut bytes = vec![0u8; 0x10];
        bytes[0] = kind;
        (pack, bytes)
    }

    #[test]
    fn an_enum_row_offers_its_declared_variants_and_holds_its_number() {
        for kind in [4u8, 77] {
            let (pack, bytes) = kinded(kind);
            let row = &Summary::of(&pack, "PhBound", Image::new(&bytes, bytes.len()), 0)
                .unwrap()
                .rows[0];
            assert_eq!(row.held, Some(ScalarValue::Int(i128::from(kind))));
            assert_eq!(
                row.choices.as_deref().map(|c| c.len()),
                Some(1),
                "{kind}: {row:?}"
            );
            assert_eq!(row.edit.map(|e| e.kind), Some(EditKind::Enum(IntWidth::U8)));
        }
        let (pack, bytes) = kinded(77);
        let row = &Summary::of(&pack, "PhBound", Image::new(&bytes, bytes.len()), 0)
            .unwrap()
            .rows[0];
        assert_eq!(row.value, "77", "a value no variant names is its number");
    }

    #[test]
    fn a_float_row_holds_what_the_bytes_hold_rather_than_its_rounding() {
        let pack = pack_with("T", 0x10, vec![scalar("radius", 0, Scalar::F32)]);
        let mut bytes = vec![0u8; 0x10];
        bytes[0..4].copy_from_slice(&0.123_456_79f32.to_le_bytes());
        let row = &Summary::of(&pack, "T", Image::new(&bytes, bytes.len()), 0)
            .unwrap()
            .rows[0];
        assert_eq!(row.value, "0.1235");
        assert_eq!(row.held, Some(ScalarValue::F32(0.123_456_79)));
    }

    #[test]
    fn what_the_owned_value_keeps_to_itself_is_internal_and_read_only() {
        let pack = pack_with(
            "T",
            0x10,
            vec![
                scalar("vft", 0, Scalar::Int(IntWidth::U64)),
                scalar("radius", 8, Scalar::F32),
            ],
        );
        let bytes = vec![0u8; 0x10];
        let shows_radius = |ty: &str, field: &str| ty == "T" && field == "radius";
        let rows = Summary::of_shown(
            &pack,
            "T",
            Image::new(&bytes, 0x10),
            0,
            &shows_radius,
            &|_, _| false,
        )
        .unwrap()
        .rows;
        assert!(rows[0].internal && rows[0].edit.is_none() && rows[0].fixed.is_some());
        assert!(!rows[1].internal && rows[1].edit.is_some() && rows[1].fixed.is_none());
    }

    #[test]
    fn a_count_and_a_pointer_are_internal_whatever_the_owned_value_shows() {
        let pack = nested_pack();
        let bytes = vec![0u8; 0x100];
        let summary = Summary::of(&pack, "Outer", Image::new(&bytes, 0x80), 0).expect("bind");
        let internal = |end: &str| {
            summary
                .rows
                .iter()
                .find(|r| r.label.ends_with(end))
                .map(|r| r.internal)
        };
        assert_eq!(internal("NumItems"), Some(true));
        assert_eq!(internal("> Items"), Some(true));
        assert_eq!(internal("LodDistances [0]"), Some(false));
    }

    #[test]
    fn a_field_the_engine_names_a_hash_reads_as_one() {
        let pack = pack_with(
            "T",
            0x10,
            vec![
                scalar("name_hash", 0, Scalar::Int(IntWidth::U32)),
                scalar("light_hash", 4, Scalar::Int(IntWidth::U8)),
                scalar("flags", 8, Scalar::Int(IntWidth::U32)),
            ],
        );
        let mut bytes = vec![0u8; 0x10];
        bytes[0..4].copy_from_slice(&0x1A2Bu32.to_le_bytes());
        let rows = Summary::of(&pack, "T", Image::new(&bytes, 0x10), 0)
            .unwrap()
            .rows;
        assert_eq!(rows[0].value, "0x00001A2B");
        let hashes: Vec<bool> = rows.iter().map(|r| r.hash).collect();
        assert_eq!(hashes, [true, false, false]);
    }

    #[test]
    fn a_field_the_declaration_types_a_hash_reads_as_one_whatever_it_is_called() {
        let pack = pack_with(
            "T",
            0x10,
            vec![
                scalar("name", 0, Scalar::Int(IntWidth::U32)),
                scalar("flags", 4, Scalar::Int(IntWidth::U32)),
                scalar("light", 8, Scalar::Int(IntWidth::U8)),
            ],
        );
        let mut bytes = vec![0u8; 0x10];
        bytes[0..4].copy_from_slice(&0x1A2Bu32.to_le_bytes());
        let (image, all) = (Image::new(&bytes, 0x10), &|_: &str, _: &str| true);
        let is_a_hash = |ty: &str, field: &str| ty == "T" && matches!(field, "name" | "light");
        let rows = Summary::of_shown(&pack, "T", image, 0, all, &is_a_hash)
            .unwrap()
            .rows;
        assert_eq!(rows[0].value, "0x00001A2B");
        let hashes: Vec<bool> = rows.iter().map(|r| r.hash).collect();
        assert_eq!(hashes, [true, false, false], "a hash is 32 bits wide");
        let rows = Summary::of(&pack, "T", image, 0).unwrap().rows;
        assert!(!rows[0].hash, "nothing declared it");
    }

    #[test]
    fn a_read_only_row_says_why() {
        let pack = nested_pack();
        let bytes = vec![0u8; LEAF + 2];
        let summary = Summary::of(&pack, "Outer", Image::new(&bytes, LEAF), 0).expect("bind");
        assert!(
            summary
                .rows
                .iter()
                .all(|r| r.edit.is_some() || r.fixed.is_some()),
            "{:?}",
            summary.rows
        );
        let why = |end: &str| {
            summary
                .rows
                .iter()
                .find(|r| r.label.ends_with(end))
                .and_then(|r| r.fixed)
        };
        assert_eq!(why("NumItems"), Some(COUNTED));
        assert_eq!(why("LodDistances [0]"), Some(PAST_THE_END));
    }

    #[test]
    fn a_polygon_slot_holding_a_primitive_draws_as_that_primitive_with_its_type_read_only() {
        let mut pack = Pack::new("t", "g");
        pack.types.push(PackType {
            name: "PhPolygon".into(),
            size: 0x10,
            fields: vec![scalar("area", 0, Scalar::F32)],
            selector: Some(PackSelector {
                tag: "area".into(),
                tag_bits: Some(or_io::desc::BitRange {
                    name: "type".into(),
                    low: 0,
                    width: 3,
                    note: None,
                }),
                variants: vec![(3, "PhPrimBox".into())],
            }),
        });
        pack.types.push(PackType {
            name: "PhPrimBox".into(),
            size: 0x10,
            fields: vec![
                scalar("prim_type", 0, Scalar::Int(IntWidth::U8)),
                scalar("vertex_index", 4, Scalar::Int(IntWidth::U16)),
            ],
            selector: None,
        });
        let mut bytes = vec![0u8; 0x10];
        bytes[0] = 3;

        let summary = Summary::of(&pack, "PhPolygon", Image::new(&bytes, 0x10), 0).unwrap();
        assert_eq!(
            summary.title, "PhPrimBox",
            "the slot's tag bits were not read"
        );
        let why = shapes_data("PhPrimBox", "prim_type");
        assert!(why.is_some(), "a primitive's type is not listed");
        let rows = &summary.rows;
        assert!(
            rows[0].edit.is_none() && rows[0].fixed == why,
            "{:?}",
            rows[0]
        );
        assert!(rows[1].edit.is_some(), "{:?}", rows[1]);
    }

    #[test]
    fn a_field_that_shapes_data_elsewhere_is_shown_read_only_with_where_it_changes() {
        let pack = pack_with(
            "GrcTexture",
            0x10,
            vec![
                scalar("width", 0, Scalar::Int(IntWidth::U16)),
                scalar("extra_flags", 4, Scalar::Int(IntWidth::U32)),
            ],
        );
        let bytes = vec![0u8; 0x10];
        let rows = Summary::of(&pack, "GrcTexture", Image::new(&bytes, 0x10), 0)
            .unwrap()
            .rows;
        let why = shapes_data("GrcTexture", "width");
        assert!(why.is_some(), "a texture's width is not listed");
        assert!(
            !rows[0].internal && rows[0].edit.is_none() && rows[0].fixed == why,
            "{:?}",
            rows[0]
        );
        assert!(rows[1].edit.is_some(), "{:?}", rows[1]);
    }
}
