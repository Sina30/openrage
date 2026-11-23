//! How two files differ, as their kind reads: metadata by record and field,
//! a texture dictionary by texture, a resource by part and field, an archive
//! by entry, and text by line.

use std::collections::HashMap;

use or_io::meta::{MetaInstance, Value};

use crate::state::textures::Texture;
use crate::ui::panels::meta_editor::item_summary;

/// How a record, field, texture or line differs between the two sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Added,
    Removed,
    Changed,
}

/// What two files are compared by, where it is not their lines or bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    RecordAndField,
    Texture,
    /// A resource: each part its declared structure reaches, and the fields of that.
    PartAndField,
    /// An archive: each file it holds, at its path.
    Entry,
}

impl By {
    pub fn words(self) -> &'static str {
        match self {
            Self::RecordAndField => "record and field",
            Self::Texture => "texture",
            Self::PartAndField => "part and field",
            Self::Entry => "entry",
        }
    }

    /// The headings over where a change is: what holds it, and what of that.
    pub fn columns(self) -> (&'static str, &'static str) {
        match self {
            Self::RecordAndField => ("Record", "Field"),
            Self::Texture => ("Texture", "Property"),
            Self::PartAndField => ("Part", "Field"),
            Self::Entry => ("Entry", "Property"),
        }
    }

    pub fn nothing_differs(self) -> &'static str {
        match self {
            Self::RecordAndField => "The two hold the same records and fields.",
            Self::Texture => "The two dictionaries hold the same textures.",
            Self::PartAndField => "The two hold the same parts, fields and data.",
            Self::Entry => "The two archives hold the same entries, byte for byte.",
        }
    }
}

/// What differs between two files.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    /// Each record and field, or texture and property, that differs, and what a limit left unread.
    Fields {
        by: By,
        changes: Vec<FieldChange>,
        unread: Option<String>,
    },
    /// Every line of both texts, side by side.
    Lines(Vec<LinePair>),
    /// Two files read neither field by field nor line by line: whether their bytes are the same, and their sizes.
    Bytes { same: bool, sizes: [usize; 2] },
}

/// A record or field one side holds otherwise, or alone.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldChange {
    /// The record, as the metadata editor indexes it (`Models[3]`), or the texture's name; empty for the root.
    pub record: String,
    /// What the metadata editor shows beside a record's index: its type and the name it goes by.
    pub named: String,
    /// The field inside the record (`Bounds.min`); empty for the whole record.
    pub field: String,
    pub change: Change,
    pub left: String,
    pub right: String,
}

/// A line of either text, or of both, at the same height.
#[derive(Debug, Clone, PartialEq)]
pub struct LinePair {
    /// The line's number from 1 and its text; `None` where this side has no line here.
    pub left: Option<(usize, String)>,
    pub right: Option<(usize, String)>,
    /// `None` for a line both texts hold.
    pub change: Option<Change>,
}

/// Each record and field `left` and `right` differ in, walked as the metadata editor walks them.
pub fn records(left: &MetaInstance, right: &MetaInstance) -> Vec<FieldChange> {
    let mut out = Vec::new();
    instance(&At::default(), left, right, &mut out);
    out
}

/// Where a change is: the record, as the metadata editor indexes and names it, and the field inside it.
#[derive(Clone, Default)]
struct At {
    record: String,
    named: String,
    field: String,
}

impl At {
    fn field(&self, name: &str) -> Self {
        let field = match self.field.is_empty() {
            true => name.to_string(),
            false => format!("{}.{name}", self.field),
        };
        Self {
            field,
            ..self.clone()
        }
    }

    /// Element `index` of the run this place names, which holds `value`.
    fn element(&self, index: usize, value: &Value) -> Self {
        self.entry(&format!("[{index}]"), value)
    }

    /// The entry `key` of the map this place names, which holds `value`.
    fn keyed(&self, key: &Value, value: &Value) -> Self {
        self.entry(&format!("[{}]", show(key)), value)
    }

    fn entry(&self, subscript: &str, value: &Value) -> Self {
        let record = match (self.record.is_empty(), self.field.is_empty()) {
            (_, true) => format!("{}{subscript}", self.record),
            (true, false) => format!("{}{subscript}", self.field),
            (false, false) => format!("{}.{}{subscript}", self.record, self.field),
        };
        Self {
            record,
            named: item_summary(value),
            field: String::new(),
        }
    }

    fn change(&self, change: Change, left: String, right: String) -> FieldChange {
        FieldChange {
            record: self.record.clone(),
            named: self.named.clone(),
            field: self.field.clone(),
            change,
            left,
            right,
        }
    }
}

fn instance(at: &At, left: &MetaInstance, right: &MetaInstance, out: &mut Vec<FieldChange>) {
    if left.type_name != right.type_name {
        out.push(at.change(
            Change::Changed,
            left.type_name.clone(),
            right.type_name.clone(),
        ));
        return;
    }
    let mut names: Vec<&str> = Vec::new();
    for (name, _) in left.fields.iter().chain(&right.fields) {
        if !names.contains(&name.as_str()) {
            names.push(name);
        }
    }
    for name in names {
        let (l, r) = (named(&left.fields, name), named(&right.fields, name));
        let here = at.field(name);
        match (&l[..], &r[..]) {
            ([l], [r]) => value(&here, l, r, out),
            ([l], []) => out.push(here.change(Change::Removed, show(l), String::new())),
            ([], [r]) => out.push(here.change(Change::Added, String::new(), show(r))),
            // A name the file repeats, as a root read with no schema repeats `Item`: a run of its own.
            _ => run(&here, &l, &r, out),
        }
    }
}

/// The values of every field called `name`.
fn named<'a>(fields: &'a [(String, Value)], name: &str) -> Vec<&'a Value> {
    (fields.iter().filter(|(n, _)| n == name))
        .map(|(_, v)| v)
        .collect()
}

fn value(at: &At, left: &Value, right: &Value, out: &mut Vec<FieldChange>) {
    if left == right {
        return;
    }
    match (left, right) {
        (
            Value::Struct(l) | Value::Pointer(Some(l)),
            Value::Struct(r) | Value::Pointer(Some(r)),
        ) => instance(at, l, r, out),
        (Value::Array(l), Value::Array(r)) => {
            run(
                at,
                &l.iter().collect::<Vec<_>>(),
                &r.iter().collect::<Vec<_>>(),
                out,
            );
        }
        (Value::Map(l), Value::Map(r)) => map(at, l, r, out),
        _ => out.push(at.change(Change::Changed, show(left), show(right))),
    }
}

/// The entries of two maps: each lined up by its key, wherever it stands, and each the other side lacks alone.
fn map(at: &At, left: &[(Value, Value)], right: &[(Value, Value)], out: &mut Vec<FieldChange>) {
    let keyed = |entries: &[(Value, Value)]| -> HashMap<String, usize> {
        (entries.iter().enumerate())
            .map(|(i, (key, _))| (show(key), i))
            .collect()
    };
    let (in_left, in_right) = (keyed(left), keyed(right));
    for (key, l) in left {
        let here = at.keyed(key, l);
        match in_right.get(&show(key)) {
            Some(&i) => value(&here, l, &right[i].1, out),
            None => out.push(here.change(Change::Removed, show(l), String::new())),
        }
    }
    for (key, r) in right {
        if !in_left.contains_key(&show(key)) {
            let here = at.keyed(key, r);
            out.push(here.change(Change::Added, String::new(), show(r)));
        }
    }
}

/// What a record goes by: its first hash or text, else the whole of it, which an unnamed record is.
fn key(value: &Value) -> (bool, String) {
    let named = match value {
        Value::Struct(record) | Value::Pointer(Some(record)) => {
            record.fields.iter().find_map(|(field, v)| match v {
                Value::Hash(hash) => Some(format!("{field}=#{:08X}", hash.hash)),
                Value::String(text) if !text.is_empty() => Some(format!("{field}={text}")),
                _ => None,
            })
        }
        _ => None,
    };
    match named {
        Some(key) => (true, key),
        None => (false, format!("{value:?}")),
    }
}

/// The records of two runs: lined up by what each goes by, an unnamed one
/// paired with the one in its place, and each the other side lacks alone.
fn run(at: &At, left: &[&Value], right: &[&Value], out: &mut Vec<FieldChange>) {
    let (lk, rk): (Vec<_>, Vec<_>) = (
        left.iter().map(|v| key(v)).collect(),
        right.iter().map(|v| key(v)).collect(),
    );
    let pairs = super::align::align(
        &lk.iter().map(|(_, k)| k.as_str()).collect::<Vec<_>>(),
        &rk.iter().map(|(_, k)| k.as_str()).collect::<Vec<_>>(),
    );
    let (mut i, mut j) = (0, 0);
    for (pi, pj) in pairs.into_iter().chain([(left.len(), right.len())]) {
        let paired = (i..pi)
            .zip(j..pj)
            .take_while(|&(a, b)| !lk[a].0 && !rk[b].0)
            .count();
        for k in 0..paired {
            value(
                &at.element(j + k, right[j + k]),
                left[i + k],
                right[j + k],
                out,
            );
        }
        for (k, removed) in left.iter().enumerate().take(pi).skip(i + paired) {
            let at = at.element(k, removed);
            out.push(at.change(Change::Removed, show(removed), String::new()));
        }
        for (k, added) in right.iter().enumerate().take(pj).skip(j + paired) {
            let at = at.element(k, added);
            out.push(at.change(Change::Added, String::new(), show(added)));
        }
        if pi < left.len() {
            value(&at.element(pj, right[pj]), left[pi], right[pj], out);
        }
        (i, j) = (pi + 1, pj + 1);
    }
}

/// A value as the comparison writes it: a record as the metadata editor names it.
fn show(value: &Value) -> String {
    let joined = |v: &[f32]| v.iter().map(f32::to_string).collect::<Vec<_>>().join(", ");
    match value {
        Value::Null | Value::Pointer(None) => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::UInt(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::String(text) | Value::Raw(text) => text.clone(),
        Value::Hash(hash) => match hash.to_xml_text() {
            text if text.is_empty() => "0".into(),
            text => text,
        },
        Value::Vec2(v) => joined(v),
        Value::Vec3(v) => joined(v),
        Value::Vec4(v) => joined(v),
        Value::Mat34(rows) => rows
            .iter()
            .map(|r| joined(r))
            .collect::<Vec<_>>()
            .join("; "),
        Value::Color(argb) => format!("0x{argb:08X}"),
        Value::Enum { variant, .. } => variant.clone(),
        Value::Bitset { bits, .. } => format!("0x{bits:X}"),
        Value::Struct(record) | Value::Pointer(Some(record)) => match item_summary(value) {
            named if named.is_empty() => record.type_name.clone(),
            named => named,
        },
        Value::Array(items) => format!("{} items", items.len()),
        Value::Map(entries) => format!("{} entries", entries.len()),
        Value::ByteArray(bytes) => format!("{} bytes", bytes.len()),
    }
}

/// Each texture one dictionary holds alone, and each property of a texture both hold that differs.
pub fn textures(left: &[Texture<'_>], right: &[Texture<'_>]) -> Vec<FieldChange> {
    use or_io::codec::texture::TextureSource;

    let lower = |t: &Texture<'_>| t.name().to_ascii_lowercase();
    let mut names: Vec<String> = left.iter().chain(right).map(lower).collect();
    names.sort_unstable();
    names.dedup();
    let size = |t: &Texture<'_>| format!("{} × {} px", t.width(), t.height());
    let described = |t: &Texture<'_>| {
        let mips = match t.mip_count() {
            1 => "1 mip level".to_string(),
            n => format!("{n} mip levels"),
        };
        format!("{:?}, {}, {mips}", t.format(), size(t))
    };
    let mut out = Vec::new();
    for name in names {
        let l = left.iter().find(|t| lower(t) == name);
        let r = right.iter().find(|t| lower(t) == name);
        let change = |record: &str, field: &str, change, left, right| FieldChange {
            record: record.to_string(),
            named: String::new(),
            field: field.to_string(),
            change,
            left,
            right,
        };
        match (l, r) {
            (Some(l), Some(r)) => {
                let bytes = |t: &Texture<'_>| or_job::human_bytes(t.payload().len() as u64);
                let properties = [
                    (
                        "Format",
                        format!("{:?}", l.format()),
                        format!("{:?}", r.format()),
                    ),
                    ("Size", size(l), size(r)),
                    (
                        "Mip levels",
                        l.mip_count().to_string(),
                        r.mip_count().to_string(),
                    ),
                ];
                for (field, a, b) in properties.into_iter().filter(|(_, a, b)| a != b) {
                    out.push(change(l.name(), field, Change::Changed, a, b));
                }
                if l.payload() != r.payload() {
                    out.push(change(
                        l.name(),
                        "Pixels",
                        Change::Changed,
                        format!("{} of pixels", bytes(l)),
                        format!("{} of different pixels", bytes(r)),
                    ));
                }
            }
            (Some(l), None) => out.push(change(
                l.name(),
                "",
                Change::Removed,
                described(l),
                String::new(),
            )),
            (None, Some(r)) => out.push(change(
                r.name(),
                "",
                Change::Added,
                String::new(),
                described(r),
            )),
            (None, None) => {}
        }
    }
    out
}

/// Both texts' lines side by side, the lines they share at the same height.
pub fn lines(left: &str, right: &str) -> Vec<LinePair> {
    let (a, b): (Vec<&str>, Vec<&str>) = (left.lines().collect(), right.lines().collect());
    let line = |side: &[&str], n: usize| Some((n + 1, side[n].to_string()));
    let mut out = Vec::with_capacity(a.len().max(b.len()));
    let (mut i, mut j) = (0, 0);
    for (pi, pj) in super::align::align(&a, &b)
        .into_iter()
        .chain([(a.len(), b.len())])
    {
        let paired = (pi - i).min(pj - j);
        for k in 0..paired {
            out.push(LinePair {
                left: line(&a, i + k),
                right: line(&b, j + k),
                change: Some(Change::Changed),
            });
        }
        out.extend((i + paired..pi).map(|k| LinePair {
            left: line(&a, k),
            right: None,
            change: Some(Change::Removed),
        }));
        out.extend((j + paired..pj).map(|k| LinePair {
            left: None,
            right: line(&b, k),
            change: Some(Change::Added),
        }));
        if pi < a.len() {
            out.push(LinePair {
                left: line(&a, pi),
                right: line(&b, pj),
                change: None,
            });
        }
        (i, j) = (pi + 1, pj + 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    fn record(name: &str, weight: f64) -> Value {
        Value::Struct(Box::new(MetaInstance {
            type_name: "CItem".into(),
            fields: vec![
                ("Name".into(), Value::String(name.into())),
                ("Weight".into(), Value::Float(weight)),
                ("Bounds".into(), bounds([0.0, 0.0, 0.0])),
            ],
            extras: Vec::new(),
        }))
    }

    fn bounds(min: [f32; 3]) -> Value {
        Value::Struct(Box::new(MetaInstance {
            type_name: "CBounds".into(),
            fields: vec![("min".into(), Value::Vec3(min))],
            extras: Vec::new(),
        }))
    }

    fn set(version: i64, items: Vec<Value>) -> MetaInstance {
        MetaInstance {
            type_name: "CItemSet".into(),
            fields: vec![
                ("Version".into(), Value::Int(version)),
                ("Items".into(), Value::Array(items)),
            ],
            extras: Vec::new(),
        }
    }

    fn change(record: &str, field: &str, change: Change, left: &str, right: &str) -> FieldChange {
        FieldChange {
            record: record.into(),
            named: String::new(),
            field: field.into(),
            change,
            left: left.into(),
            right: right.into(),
        }
    }

    /// The changes without the names beside each record, which one test reads.
    fn unnamed(mut changes: Vec<FieldChange>) -> Vec<FieldChange> {
        changes.iter_mut().for_each(|c| c.named.clear());
        changes
    }

    #[test]
    fn two_metadata_trees_differ_by_record_and_field_with_both_values() {
        let left = set(
            1,
            vec![record("a", 1.0), record("b", 1.0), record("c", 1.0)],
        );
        let mut moved = record("c", 1.0);
        if let Value::Struct(c) = &mut moved {
            c.set("Bounds", bounds([0.0, 0.5, 0.0]));
        }
        let right = set(2, vec![record("a", 2.0), moved, record("d", 1.0)]);

        let found = records(&left, &right);
        assert_eq!(
            unnamed(found.clone()),
            vec![
                change("", "Version", Change::Changed, "1", "2"),
                change("Items[0]", "Weight", Change::Changed, "1", "2"),
                change("Items[1]", "", Change::Removed, "CItem  (Name=b)", ""),
                change(
                    "Items[1]",
                    "Bounds.min",
                    Change::Changed,
                    "0, 0, 0",
                    "0, 0.5, 0"
                ),
                change("Items[2]", "", Change::Added, "", "CItem  (Name=d)"),
            ]
        );
        assert_eq!(
            found[1].named, "CItem  (Name=a)",
            "named as the editor names it"
        );
        assert!(
            records(&left, &left).is_empty(),
            "a tree differs from nothing in itself"
        );
    }

    #[test]
    fn an_unnamed_record_that_changed_is_paired_with_the_one_in_its_place() {
        let point = |x: f32| Value::Vec3([x, 0.0, 0.0]);
        let left = set(1, vec![point(1.0), point(2.0), point(3.0)]);
        let right = set(1, vec![point(1.0), point(2.5), point(3.0), point(4.0)]);
        assert_eq!(
            unnamed(records(&left, &right)),
            vec![
                change("Items[1]", "", Change::Changed, "2, 0, 0", "2.5, 0, 0"),
                change("Items[3]", "", Change::Added, "", "4, 0, 0"),
            ]
        );
    }

    fn table(entries: &[(&str, i64)]) -> MetaInstance {
        let pairs = (entries.iter())
            .map(|(key, n)| (Value::String((*key).into()), Value::Int(*n)))
            .collect();
        MetaInstance {
            type_name: "CTable".into(),
            fields: vec![("Weights".into(), Value::Map(pairs))],
            extras: Vec::new(),
        }
    }

    #[test]
    fn two_maps_differ_by_key_wherever_the_entry_stands() {
        let left = table(&[("a", 1), ("b", 2), ("c", 3)]);
        let right = table(&[("c", 3), ("a", 5), ("d", 4)]);
        assert_eq!(
            unnamed(records(&left, &right)),
            vec![
                change("Weights[a]", "", Change::Changed, "1", "5"),
                change("Weights[b]", "", Change::Removed, "2", ""),
                change("Weights[d]", "", Change::Added, "", "4"),
            ]
        );
        let reordered = table(&[("c", 3), ("a", 1), ("b", 2)]);
        assert!(
            records(&left, &reordered).is_empty(),
            "the same entries in another order are the same map"
        );
    }

    fn texture(name: &str, size: u16, pixels: u8) -> or_io::gta5::gen8::GrcTexture {
        or_io::gta5::gen8::GrcTexture {
            name: name.into(),
            width: size,
            height: size,
            format: u32::from_le_bytes(*b"DXT1"),
            mip_count: 1,
            backing_store: vec![pixels; usize::from(size) * usize::from(size) / 2],
            ..Default::default()
        }
    }

    fn listed(all: &[or_io::gta5::gen8::GrcTexture]) -> Vec<Texture<'_>> {
        all.iter()
            .map(|t| Texture::Owned(Cow::Borrowed(t)))
            .collect()
    }

    #[test]
    fn two_dictionaries_differ_by_texture_and_by_what_of_a_texture_changed() {
        let left = [
            texture("body", 8, 1),
            texture("gone", 4, 1),
            texture("head", 4, 1),
        ];
        let right = [
            texture("BODY", 8, 2),
            texture("head", 8, 1),
            texture("new", 4, 1),
        ];
        let found = textures(&listed(&left), &listed(&right));
        let dxt1 = "DXT1, 4 × 4 px, 1 mip level";
        assert_eq!(
            found,
            vec![
                change(
                    "body",
                    "Pixels",
                    Change::Changed,
                    "32 B of pixels",
                    "32 B of different pixels"
                ),
                change("gone", "", Change::Removed, dxt1, ""),
                change("head", "Size", Change::Changed, "4 × 4 px", "8 × 8 px"),
                change(
                    "head",
                    "Pixels",
                    Change::Changed,
                    "8 B of pixels",
                    "32 B of different pixels"
                ),
                change("new", "", Change::Added, "", dxt1),
            ]
        );
        assert!(textures(&listed(&left), &listed(&left)).is_empty());
    }

    #[test]
    fn a_texture_whose_pixels_changed_says_the_pixels_differ_rather_than_two_equal_sizes() {
        let found = textures(
            &listed(&[texture("body", 8, 1)]),
            &listed(&[texture("body", 8, 2)]),
        );
        let [pixels] = &found[..] else {
            panic!("one change expected: {found:?}");
        };
        assert_eq!(pixels.field, "Pixels");
        assert!(
            pixels.right.contains("different pixels") && pixels.right != pixels.left,
            "the two sides read {:?} and {:?}, which do not say the pixels differ",
            pixels.left,
            pixels.right
        );
    }

    #[test]
    fn two_texts_line_up_side_by_side_their_changes_paired_where_they_stand() {
        let left = "<a>\n  <b>1</b>\n  <c>2</c>\n</a>\n";
        let right = "<a>\n  <b>5</b>\n  <c>2</c>\n  <d/>\n</a>\n";
        let line = |n: usize, text: &str| Some((n, text.to_string()));
        assert_eq!(
            lines(left, right),
            vec![
                LinePair {
                    left: line(1, "<a>"),
                    right: line(1, "<a>"),
                    change: None,
                },
                LinePair {
                    left: line(2, "  <b>1</b>"),
                    right: line(2, "  <b>5</b>"),
                    change: Some(Change::Changed),
                },
                LinePair {
                    left: line(3, "  <c>2</c>"),
                    right: line(3, "  <c>2</c>"),
                    change: None,
                },
                LinePair {
                    left: None,
                    right: line(4, "  <d/>"),
                    change: Some(Change::Added),
                },
                LinePair {
                    left: line(4, "</a>"),
                    right: line(5, "</a>"),
                    change: None,
                },
            ]
        );
    }
}
