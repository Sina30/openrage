//! A resource compared field by field, from the parts and rows the inspector
//! reads: parts matched by their path, each field read from the value as
//! OpenRage saves it, so another layout of the same value differs in nothing.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use or_editor::{resolve_count, Cut, Hop, Node, Outline, Row, MAX_ELEMENTS};
use or_io::desc::{CountSource, FieldRepr, Image, Pack, PackField, PackType, Pointee, ScalarValue};
use or_io::Asset;

use super::align::align;
use super::archive::weighed;
use super::found::{Change, FieldChange};
use super::DEPTH;
use crate::state::{document_io, DocumentKind, ResourceImage};
use crate::ui::panels::inspector::{declared_for, summary_in, Declared};

/// One side: a resource's image, how its type is declared, and the parts it holds.
pub(super) struct Resource {
    image: Arc<ResourceImage>,
    pack: Arc<Pack>,
    declared: Option<Arc<Declared>>,
    outline: Outline,
}

impl Resource {
    /// `bytes`, a file called `name`, as a resource its format declares at the target its version names.
    fn open(name: &str, bytes: &[u8]) -> Option<Self> {
        let kind = DocumentKind::from_path(Path::new(name))?;
        kind.resource_type()?;
        let image = document_io::resource_image_for(kind, bytes)?;
        let format = or_io::gta5::formats::format(kind.extension())?;
        let (pack, root) = document_io::described_for(format, &image)?;
        let outline = Outline::of_to(&pack, &root, image.view(), 0, DEPTH)?;
        let declared = declared_for(kind.extension(), image.seal.version());
        Some(Self {
            image,
            pack,
            declared,
            outline,
        })
    }

    /// The physical segment, where the vertices, indices and pixels lie, which no field row lists.
    fn bulk(&self) -> &[u8] {
        (self.image.data.get(self.image.virtual_size..)).unwrap_or_default()
    }

    /// The fields of the `type_name` at `at` that the owned value does not keep to itself.
    fn fields(&self, type_name: &str, at: usize) -> Vec<Row> {
        let image = self.image.view();
        let summary = summary_in(&self.pack, self.declared.as_deref(), image, type_name, at);
        (summary.into_iter().flat_map(|s| s.rows))
            .filter(|row| !row.internal)
            .collect()
    }

    /// The blobs of the `type_name` at `at`, each under its field's name in the inspector.
    fn blobs(&self, type_name: &str, at: usize) -> Vec<(String, &[u8])> {
        let Some(ty) = self.pack.type_by_name(type_name) else {
            return Vec::new();
        };
        let image = self.image.view();
        let held = |field: &PackField| Some((field.display_name(), blob(ty, field, image, at)?));
        ty.fields.iter().filter_map(held).collect()
    }

    /// How the field `hop` of `owner` lies in it, where `hop` names one.
    fn repr(&self, owner: Option<&str>, hop: Option<&Hop>) -> Option<&FieldRepr> {
        let (Some(owner), Some(Hop::Field(name))) = (owner, hop) else {
            return None;
        };
        let field = (self.pack.type_by_name(owner)).and_then(|ty| ty.field_by_name(name))?;
        Some(&field.repr)
    }

    /// Whether the field `hop` of `owner` is a struct inside it, whose fields the owner's own rows already list.
    fn inline(&self, owner: Option<&str>, hop: Option<&Hop>) -> bool {
        matches!(self.repr(owner, hop), Some(FieldRepr::Inline { .. }))
    }

    /// Whether the field `hop` of `owner` is a run with no element type, which [`blob`] compares.
    fn untyped_run(&self, owner: Option<&str>, hop: Option<&Hop>) -> bool {
        matches!(
            self.repr(owner, hop),
            Some(
                FieldRepr::Trailing {
                    type_name: None,
                    ..
                } | FieldRepr::Ptr(Pointee::Many {
                    type_name: None,
                    ..
                }) | FieldRepr::AtArray(Pointee::Many {
                    type_name: None,
                    ..
                })
            )
        )
    }

    /// What `hop` of `owner` is called in the inspector's rows.
    fn called(&self, owner: Option<&str>, hop: &Hop) -> String {
        match (hop, owner) {
            (Hop::Index(n), _) => format!("[{n}]"),
            (Hop::Field(name), Some(owner)) => {
                let field = (self.pack.type_by_name(owner)).and_then(|ty| ty.field_by_name(name));
                field.map_or_else(|| name.clone(), |field| field.display_name())
            }
            (Hop::Field(name), None) => name.clone(),
        }
    }

    /// A part and everything under it, as one string that is the same for two parts that hold the same.
    fn print(&self, node: &Node, out: &mut String) {
        match &node.type_name {
            Some(ty) => {
                out.push_str(ty);
                for row in self.fields(ty, node.at) {
                    let _ = write!(out, "|{}={}", row.label, exact(&row));
                }
            }
            None => out.push_str(&node.label),
        }
        for child in &node.children {
            let _ = write!(out, "{{{:?}:", child.hop);
            self.print(child, out);
            out.push('}');
        }
    }
}

/// What `field` of the `ty` at `at` holds where it holds bytes and nothing else, in the virtual segment: the blob a pointer reaches, or a run with no element type, trailing the struct or reached by a pointer or a counted array.
///
/// ⚠ Its row is internal, and the outline has no part for a pointer's blob or
/// only a label with the count for a run, which [`Walk::children`] leaves to
/// this, so this is the one place its bytes and length are compared. A blob in the physical segment is not read here:
/// [`Resource::bulk`] tells that segment whole. A pointer that reaches nothing
/// holds nothing; `None` where the field is no such blob or the pack does not
/// record its length.
fn blob<'i>(ty: &PackType, field: &PackField, image: Image<'i>, at: usize) -> Option<&'i [u8]> {
    let here = at + field.offset;
    let counted = |count: &CountSource, width: Option<usize>| {
        resolve_count(ty, count, image, at)?.checked_mul(width.unwrap_or(1))
    };
    let reached = || {
        let target = (image.pointer_at(here).ok()).filter(|&address| address != 0);
        target.and_then(|address| image.resolve(address).ok())
    };
    let (start, len) = match &field.repr {
        FieldRepr::Trailing {
            type_name: None,
            count,
            element_size,
        } => (Some(here), counted(count, *element_size)?),
        FieldRepr::Ptr(Pointee::Bytes { length }) => (reached(), counted(length, None)?),
        FieldRepr::Ptr(Pointee::Many {
            type_name: None,
            count,
            element_size,
        }) => (reached(), counted(count, *element_size)?),
        FieldRepr::AtArray(Pointee::Many {
            type_name: None,
            element_size,
            ..
        }) => match image.at_array(here) {
            Ok(Some(held)) => (
                Some(held.first().base()),
                held.count().checked_mul(element_size.unwrap_or(1))?,
            ),
            Ok(None) => (None, 0),
            Err(_) => return None,
        },
        _ => return None,
    };
    if start.is_some_and(|start| start >= image.virtual_size()) {
        return None;
    }
    Some((start.and_then(|start| image.slice_at(start, len).ok())).unwrap_or_default())
}

/// What a row holds, to the last digit.
fn exact(row: &Row) -> String {
    match (&row.held, &row.exact) {
        (Some(held), _) => format!("{held:?}"),
        (None, Some(exact)) => exact.clone(),
        (None, None) => row.value.clone(),
    }
}

/// What a row reads as: its value, in full where the other side's reads the same.
fn reads(row: &Row, other: &Row) -> String {
    if row.value != other.value {
        return row.value.clone();
    }
    match (&row.held, &row.exact) {
        (Some(ScalarValue::F32(v)), _) => v.to_string(),
        (Some(ScalarValue::F64(v)), _) => v.to_string(),
        (_, Some(exact)) => exact.clone(),
        _ => row.value.clone(),
    }
}

/// What a part is, in a change's words.
fn text(node: &Node) -> String {
    node.type_name.clone().unwrap_or_else(|| node.label.clone())
}

/// What the limits of both sides' outlines left out, which no row of the comparison covers.
#[derive(Default)]
struct Unread {
    runs: usize,
    deep: usize,
}

impl Unread {
    fn add(&mut self, node: &Node) {
        match node.cut {
            Some(Cut::Elements(_)) => self.runs += 1,
            Some(Cut::Depth) => self.deep += 1,
            None => {}
        }
        for child in &node.children {
            self.add(child);
        }
    }

    /// In a sentence's words; `None` where nothing was left out.
    fn words(&self) -> Option<String> {
        let counted = |n: usize, one: &str, many: &str| match n {
            1 => format!("1 {one}"),
            n => format!("{n} {many}"),
        };
        let mut left_out = Vec::new();
        if self.runs > 0 {
            let runs = counted(self.runs, "run", "runs");
            left_out.push(format!("elements past the {MAX_ELEMENTS}th in {runs}"));
        }
        if self.deep > 0 {
            let parts = counted(self.deep, "part", "parts");
            left_out.push(format!("whatever lies under {parts} {DEPTH} levels down"));
        }
        (!left_out.is_empty())
            .then(|| format!("Not read, in either file: {}.", left_out.join("; ")))
    }
}

type Pair = (Option<usize>, Option<usize>);

/// `pairs` of two runs, less the elements past the last pair that the other run, cut short, may still hold.
fn without_overhang(pairs: Vec<Pair>, left_cut: bool, right_cut: bool) -> Vec<Pair> {
    let Some(last) = pairs.iter().rposition(|p| matches!(p, (Some(_), Some(_)))) else {
        return pairs;
    };
    let past = |n: usize, pair: &Pair| {
        n > last
            && match pair {
                (Some(_), None) => right_cut,
                (None, Some(_)) => left_cut,
                _ => false,
            }
    };
    let kept = pairs.iter().enumerate().filter(|(n, pair)| !past(*n, pair));
    kept.map(|(_, pair)| *pair).collect()
}

/// The pairs of two runs of parts that line up: each part with its equal, then those between paired where they stand, and the rest alone.
fn lined_up(left: &[String], right: &[String]) -> Vec<(Option<usize>, Option<usize>)> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    for (pi, pj) in align(left, right)
        .into_iter()
        .chain([(left.len(), right.len())])
    {
        let paired = (pi - i).min(pj - j);
        out.extend((0..paired).map(|k| (Some(i + k), Some(j + k))));
        out.extend((i + paired..pi).map(|k| (Some(k), None)));
        out.extend((j + paired..pj).map(|k| (None, Some(k))));
        if pi < left.len() {
            out.push((Some(pi), Some(pj)));
        }
        (i, j) = (pi + 1, pj + 1);
    }
    out
}

struct Walk<'a> {
    left: &'a Resource,
    right: &'a Resource,
    out: Vec<FieldChange>,
}

impl Walk<'_> {
    fn push(&mut self, place: &str, named: &str, field: &str, change: Change, sides: [String; 2]) {
        let [left, right] = sides;
        self.out.push(FieldChange {
            record: place.to_string(),
            named: named.to_string(),
            field: field.to_string(),
            change,
            left,
            right,
        });
    }

    /// The same part of both sides, at `place`.
    fn pair(&mut self, place: &str, a: &Node, b: &Node, inline: bool) {
        if a.type_name != b.type_name {
            return self.push(place, "", "", Change::Changed, [text(a), text(b)]);
        }
        match &a.type_name {
            // An inline struct's fields are among its owner's rows; its blobs are its own.
            Some(ty) if !inline => {
                self.rows(place, ty, a.at, b.at);
                self.blobs(place, ty, a.at, b.at);
            }
            Some(ty) => self.blobs(place, ty, a.at, b.at),
            None if a.children.is_empty() && b.children.is_empty() && a.label != b.label => {
                self.push(
                    place,
                    "",
                    "",
                    Change::Changed,
                    [a.label.clone(), b.label.clone()],
                );
            }
            _ => {}
        }
        self.children(place, a, b);
    }

    /// The fields of the `ty` at `a` on the left and `b` on the right, by name.
    fn rows(&mut self, place: &str, ty: &str, a: usize, b: usize) {
        let (left, right) = (self.left.fields(ty, a), self.right.fields(ty, b));
        for x in &left {
            match right.iter().find(|y| y.label == x.label) {
                Some(y) if exact(x) != exact(y) => {
                    self.push(
                        place,
                        ty,
                        &x.label,
                        Change::Changed,
                        [reads(x, y), reads(y, x)],
                    );
                }
                Some(_) => {}
                None => self.push(
                    place,
                    ty,
                    &x.label,
                    Change::Removed,
                    [x.value.clone(), String::new()],
                ),
            }
        }
        for y in right
            .iter()
            .filter(|y| left.iter().all(|x| x.label != y.label))
        {
            self.push(
                place,
                ty,
                &y.label,
                Change::Added,
                [String::new(), y.value.clone()],
            );
        }
    }

    /// The blobs of the `ty` at `a` on the left and `b` on the right that differ, by field.
    fn blobs(&mut self, place: &str, ty: &str, a: usize, b: usize) {
        let (left, right) = (self.left.blobs(ty, a), self.right.blobs(ty, b));
        for (name, x) in &left {
            let Some((_, y)) = right.iter().find(|(other, _)| other == name) else {
                continue;
            };
            if x != y {
                self.push(place, ty, name, Change::Changed, weighed(x, y));
            }
        }
    }

    /// The parts under `a` and `b`: fields by name, and a run's elements by position, or lined up by what each holds where the runs differ in length.
    ///
    /// ⚠ A run is read to its first [`MAX_ELEMENTS`] elements: its length is what
    /// it holds, not what it shows, and an element one side pushes past the
    /// limit is not reported as removed from the other.
    ///
    /// ⚠ A run with no element type is no part here: its length is in its label,
    /// and its blob.s row in [`Walk::blobs`] says it with what differs.
    fn children(&mut self, place: &str, a: &Node, b: &Node) {
        let indexed =
            (a.children.iter().chain(&b.children)).any(|c| matches!(c.hop, Some(Hop::Index(_))));
        let held = |node: &Node| match node.cut {
            Some(Cut::Elements(total)) => total,
            _ => node.children.len(),
        };
        let even = a.children.len() == b.children.len() && held(a) == held(b);
        let pairs = if !indexed || even {
            by_hop(&a.children, &b.children)
        } else {
            let print = |side: &Resource, nodes: &[Node]| -> Vec<String> {
                let one = |node: &Node| {
                    let mut out = String::new();
                    side.print(node, &mut out);
                    out
                };
                nodes.iter().map(one).collect()
            };
            let pairs = lined_up(
                &print(self.left, &a.children),
                &print(self.right, &b.children),
            );
            let short = |node: &Node| held(node) > node.children.len();
            without_overhang(pairs, short(a), short(b))
        };
        let (owner_a, owner_b) = (a.type_name.as_deref(), b.type_name.as_deref());
        for pair in pairs {
            match pair {
                (Some(i), Some(j)) => {
                    let (x, y) = (&a.children[i], &b.children[j]);
                    if self.right.untyped_run(owner_b, y.hop.as_ref()) {
                        continue;
                    }
                    let place = under(self.right, place, owner_b, y);
                    let inline = self.right.inline(owner_b, y.hop.as_ref());
                    self.pair(&place, x, y, inline);
                }
                (Some(i), None) => {
                    let x = &a.children[i];
                    let place = under(self.left, place, owner_a, x);
                    self.push(&place, "", "", Change::Removed, [text(x), String::new()]);
                }
                (None, Some(j)) => {
                    let y = &b.children[j];
                    let place = under(self.right, place, owner_b, y);
                    self.push(&place, "", "", Change::Added, [String::new(), text(y)]);
                }
                (None, None) => {}
            }
        }
    }
}

/// Each of `a` with the one of `b` on its hop, and each of `b` that none of `a` is.
fn by_hop(a: &[Node], b: &[Node]) -> Vec<(Option<usize>, Option<usize>)> {
    let mut out: Vec<(Option<usize>, Option<usize>)> = (a.iter().enumerate())
        .map(|(i, x)| (Some(i), b.iter().position(|y| y.hop == x.hop)))
        .collect();
    let paired: Vec<usize> = out.iter().filter_map(|(_, j)| *j).collect();
    out.extend(
        (0..b.len())
            .filter(|j| !paired.contains(j))
            .map(|j| (None, Some(j))),
    );
    out
}

/// Where `child`, a part of the `owner` at `place`, is, as `side` calls it: a run's element after its run's name, any other part after a separator.
fn under(side: &Resource, place: &str, owner: Option<&str>, child: &Node) -> String {
    match &child.hop {
        Some(hop @ Hop::Index(_)) => format!("{place}{}", side.called(owner, hop)),
        Some(hop) if place.is_empty() => side.called(owner, hop),
        Some(hop) => format!("{place} > {}", side.called(owner, hop)),
        None => place.to_string(),
    }
}

/// Each field of `left`'s parts and `right`'s that differs, by the rows both read.
fn changes(left: &Resource, right: &Resource) -> Vec<FieldChange> {
    let mut walk = Walk {
        left,
        right,
        out: Vec::new(),
    };
    walk.pair("", &left.outline.root, &right.outline.root, false);
    walk.out
}

/// How two resources of one kind differ: each field their parts' rows name, and the data those rows do not list.
///
/// ⚠ Each side is read as OpenRage saves it, which is one layout for one value:
/// a file holds words its value does not (a runtime id, what an allocation
/// left), so two values are the same where their saves are, not where they
/// compare equal. `None` where either is not a resource its format declares.
///
/// ⚠ The data no row lists is told by the physical segments, which a field edit
/// leaves as it was, and by each blob a part's fields reach, compared on its
/// own; any other difference in what the virtual segment holds beyond the rows
/// is told only where no row differs, since a changed field moves it.
///
/// ⚠ The parts are the outline's, which keeps [`MAX_ELEMENTS`] elements of a
/// run and [`DEPTH`] levels, so a difference past either is not a row:
/// the second value says what was left out. Lifting the element limit took
/// one staged `.ypt` from 292,709 nodes to 38,875,685, so it stays.
pub(super) fn compare(
    left: (&str, &[u8]),
    right: (&str, &[u8]),
    a: &Asset,
    b: &Asset,
) -> Option<(Vec<FieldChange>, Option<String>)> {
    let saved = |asset: &Asset, file: &[u8]| asset.to_bytes().unwrap_or_else(|_| file.to_vec());
    let (x, y) = (saved(a, left.1), saved(b, right.1));
    if x == y {
        return Some((Vec::new(), None));
    }
    let (l, r) = (Resource::open(left.0, &x)?, Resource::open(right.0, &y)?);
    let mut out = changes(&l, &r);
    let bulk_differs = l.bulk() != r.bulk();
    if out.is_empty() || bulk_differs {
        // The rows list fields: vertices, indices, pixels and the rest behind them differ here.
        let [held_left, held_right] = match bulk_differs {
            true => weighed(l.bulk(), r.bulk()),
            false => weighed(&x, &y),
        };
        out.push(FieldChange {
            record: String::new(),
            named: String::new(),
            field: "Other data".to_string(),
            change: Change::Changed,
            left: held_left,
            right: held_right,
        });
    }
    let mut unread = Unread::default();
    unread.add(&l.outline.root);
    unread.add(&r.outline.root);
    Some((out, unread.words()))
}

#[cfg(test)]
mod tests {
    use or_io::decl::Gen8;
    use or_io::gta5::{File as _, GtaDrawable};
    use or_test_support::{skip_corpus, Corpus};

    use super::super::{compared, opened, Found};
    use super::*;
    use crate::state::DocPayload;

    fn changes_of(found: Found) -> Vec<FieldChange> {
        match found {
            Found::Fields { changes, .. } => changes,
            other => panic!("not compared field by field: {other:?}"),
        }
    }

    /// The smallest `count` staged files of `extension`: Gen8 ones, or of every target where the read does not depend on the layout.
    fn staged(extension: &str, count: usize, every_target: bool) -> Vec<(String, Vec<u8>)> {
        let corpus = Corpus::new(env!("CARGO_MANIFEST_DIR"), extension);
        let mut files = match every_target {
            true => corpus.all_targets().find(),
            false => corpus.find(),
        };
        files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
        let named = |path: &std::path::PathBuf| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some((name, std::fs::read(path).ok()?))
        };
        files.iter().filter_map(named).take(count).collect()
    }

    fn virtual_field(name: &str, offset: usize, repr: FieldRepr) -> PackField {
        let space = matches!(repr, FieldRepr::Ptr(_) | FieldRepr::AtArray(_))
            .then_some(or_io::desc::AddressSpace::Virtual);
        PackField {
            name: name.into(),
            offset,
            repr,
            space,
            bits: Vec::new(),
            note: None,
        }
    }

    /// Where a pointer at `at` in the synthetic image points.
    fn address(at: usize) -> [u8; 8] {
        (0x5000_0000_u64 | at as u64).to_le_bytes()
    }

    /// What the synthetic `Root` holds: six floats, sixteen opaque bytes, and two runs of `u16` with no element type, one behind a pointer and one a counted array.
    struct Held {
        weights: [f32; 6],
        words: [u8; 16],
        run: Vec<u16>,
        table: Vec<u16>,
    }

    impl Default for Held {
        fn default() -> Self {
            Self {
                weights: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
                words: std::array::from_fn(|i| i as u8),
                run: vec![7, 8, 9],
                table: vec![10, 11],
            }
        }
    }

    /// `held` laid out as `Root` declares it, as one side of a comparison.
    fn synthetic(held: &Held) -> Resource {
        let mut pack = Pack::new("t", "g");
        let scalar = |width| FieldRepr::Scalar(or_io::desc::Scalar::Int(width));
        let untyped = |count| Pointee::Many {
            type_name: None,
            count,
            element_size: Some(2),
        };
        pack.types.push(PackType {
            name: "Root".into(),
            size: 0x50,
            fields: vec![
                virtual_field(
                    "weights",
                    0x00,
                    FieldRepr::ScalarArray {
                        element: or_io::desc::Scalar::F32,
                        count: 6,
                    },
                ),
                virtual_field("words", 0x18, FieldRepr::Opaque { size: 16 }),
                virtual_field("n", 0x28, scalar(or_io::desc::IntWidth::U16)),
                virtual_field(
                    "run",
                    0x30,
                    FieldRepr::Ptr(untyped(CountSource::Field("n".into()))),
                ),
                virtual_field(
                    "table",
                    0x38,
                    FieldRepr::AtArray(untyped(CountSource::Inline)),
                ),
            ],
            selector: None,
        });
        let mut data = vec![0u8; 0x100];
        for (i, weight) in held.weights.iter().enumerate() {
            data[i * 4..i * 4 + 4].copy_from_slice(&weight.to_le_bytes());
        }
        data[0x18..0x28].copy_from_slice(&held.words);
        data[0x28..0x2A].copy_from_slice(&(held.run.len() as u16).to_le_bytes());
        data[0x30..0x38].copy_from_slice(&address(0x80));
        data[0x38..0x40].copy_from_slice(&address(0xA0));
        data[0x40..0x42].copy_from_slice(&(held.table.len() as u16).to_le_bytes());
        for (at, values) in [(0x80, &held.run), (0xA0, &held.table)] {
            for (i, value) in values.iter().enumerate() {
                data[at + i * 2..at + i * 2 + 2].copy_from_slice(&value.to_le_bytes());
            }
        }
        // Only an envelope's seal is needed, and a real one is the one thing a test cannot write by hand.
        let drawable = GtaDrawable::<Gen8>::default()
            .to_bytes()
            .expect("a drawable saves");
        let seal = (document_io::resource_image_for(DocumentKind::Ydr, &drawable))
            .expect("a drawable's image")
            .seal
            .clone();
        let image = Arc::new(ResourceImage {
            virtual_size: data.len(),
            data,
            seal,
        });
        let outline = Outline::of_to(&pack, "Root", image.view(), 0, DEPTH).expect("an outline");
        Resource {
            image,
            pack: Arc::new(pack),
            declared: None,
            outline,
        }
    }

    /// Each change between two synthetic values: where, which field, and both values.
    fn between(left: &Held, right: &Held) -> Vec<(String, String, String, String)> {
        let found = changes(&synthetic(left), &synthetic(right));
        (found.into_iter())
            .map(|c| (c.record, c.field, c.left, c.right))
            .collect()
    }

    fn said(parts: [&str; 4]) -> (String, String, String, String) {
        let [record, field, left, right] = parts.map(str::to_string);
        (record, field, left, right)
    }

    #[test]
    fn two_values_that_hold_the_same_differ_in_nothing() {
        assert_eq!(between(&Held::default(), &Held::default()), vec![]);
    }

    #[test]
    fn a_scalar_array_changed_past_its_third_decimal_is_reported_with_both_values() {
        let right = Held {
            weights: [1.0, 2.0, 3.0004, 4.0, 5.0, 6.0],
            ..Held::default()
        };
        assert_eq!(
            between(&Held::default(), &right),
            vec![said([
                "",
                "Weights",
                "1, 2, 3, 4, 5, 6",
                "1, 2, 3.0004, 4, 5, 6"
            ])]
        );
    }

    #[test]
    fn an_opaque_field_changed_is_reported_with_both_values() {
        let mut words: [u8; 16] = std::array::from_fn(|i| i as u8);
        words[2] = 0xFF;
        let right = Held {
            words,
            ..Held::default()
        };
        assert_eq!(
            between(&Held::default(), &right),
            vec![said([
                "",
                "Words",
                "00 01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F",
                "00 01 FF 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F"
            ])]
        );
    }

    #[test]
    fn a_run_with_no_element_type_changed_in_its_bytes_is_reported_whether_a_pointer_or_a_count_reaches_it(
    ) {
        let run = Held {
            run: vec![7, 8, 10],
            ..Held::default()
        };
        assert_eq!(
            between(&Held::default(), &run),
            vec![said(["", "Run", "6 B", "6 B, differing from byte 4"])]
        );
        let table = Held {
            table: vec![10, 12],
            ..Held::default()
        };
        assert_eq!(
            between(&Held::default(), &table),
            vec![said(["", "Table", "4 B", "4 B, differing from byte 2"])]
        );
    }

    #[test]
    fn a_run_with_no_element_type_that_changes_length_is_one_row_for_each_run() {
        let longer = Held {
            run: vec![7, 8, 9, 10],
            table: vec![10],
            ..Held::default()
        };
        assert_eq!(
            between(&Held::default(), &longer),
            vec![
                said(["", "Run", "6 B", "8 B"]),
                said(["", "Table", "4 B", "2 B"]),
            ]
        );
    }

    #[test]
    fn a_shipped_resource_and_its_save_in_another_layout_compare_as_the_same() {
        const WHAT: &str = "a shipped resource compared with its own save";
        const PER_FORMAT: usize = 3;
        let (mut tried, mut failures, mut unreached) = (0, Vec::new(), Vec::new());
        for format in or_io::gta5::formats::FORMATS {
            // Opened by name, as the app opens it: `or_io::from_bytes` refuses a version two formats share.
            let files = staged(format.ext, 12, true);
            let (mut compared_here, mut refused) = (0, Vec::new());
            for (name, shipped) in &files {
                if compared_here == PER_FORMAT {
                    break;
                }
                let Some(DocPayload::Resource(asset)) = opened(name, shipped) else {
                    refused.push(name.as_str());
                    continue;
                };
                let Ok(saved) = asset.to_bytes() else {
                    failures.push(format!("{name}: opens but does not save"));
                    continue;
                };
                compared_here += 1;
                let changes = changes_of(compared((name, shipped), (name, &saved)));
                if !changes.is_empty() {
                    failures.push(format!("{name}: {:?}", &changes[..changes.len().min(3)]));
                }
            }
            tried += compared_here;
            if !files.is_empty() && compared_here == 0 {
                unreached.push(format!(".{}: none of {refused:?} opens", format.ext));
            }
        }
        if tried == 0 && unreached.is_empty() {
            skip_corpus(WHAT, "no resource staged");
            return;
        }
        failures.extend(unreached);
        assert!(failures.is_empty(), "of {tried}: {}", failures.join("\n"));
    }

    #[test]
    fn an_edit_to_a_shipped_drawable_is_the_only_difference_its_save_has_from_it() {
        const WHAT: &str = "a shipped drawable compared with its edited save";
        // `GtaDrawable::<Gen8>` reads the Gen8 layout, so a Gen9 file is not one to open with it.
        let Some((name, shipped)) = staged("ydr", 1, false).into_iter().next() else {
            skip_corpus(WHAT, "no staged Gen8 .ydr");
            return;
        };
        let mut edited =
            GtaDrawable::<Gen8>::from_bytes(&shipped).expect("a shipped drawable opens");
        let before = edited.base.lod_group.lod_distances[0];
        edited.base.lod_group.lod_distances[0] = before + 25.0;
        let saved = edited.to_bytes().expect("the edit saves");
        assert_ne!(shipped, saved, "{name}: a save lays the file out again");

        let changes = changes_of(compared((&name, &shipped), (&name, &saved)));
        assert_eq!(changes.len(), 1, "{name}: {changes:#?}");
        assert_eq!(changes[0].field, "Base > LodGroup > LodDistances [0]");
        assert_eq!(changes[0].change, Change::Changed);
        assert_eq!(changes[0].left, format!("{before:.3}"));
        assert_eq!(changes[0].right, format!("{:.3}", before + 25.0));
    }

    #[test]
    fn what_the_limits_left_out_reads_in_words_and_nothing_left_out_reads_as_nothing() {
        assert_eq!(Unread::default().words(), None);
        let one = Unread { runs: 1, deep: 1 };
        assert_eq!(
            one.words().as_deref(),
            Some(
                format!(
                    "Not read, in either file: elements past the {MAX_ELEMENTS}th in 1 run; \
                     whatever lies under 1 part {DEPTH} levels down."
                )
                .as_str()
            )
        );
        let some = Unread { runs: 0, deep: 3 };
        assert_eq!(
            some.words().as_deref(),
            Some(
                format!(
                    "Not read, in either file: whatever lies under 3 parts {DEPTH} levels down."
                )
                .as_str()
            )
        );
    }

    #[test]
    fn an_element_a_cut_run_may_hold_past_its_window_is_not_reported_alone() {
        let pairs = vec![
            (Some(0), Some(0)),
            (None, Some(1)),
            (Some(1), Some(2)),
            (Some(2), None),
        ];
        assert_eq!(without_overhang(pairs.clone(), false, false), pairs);
        assert_eq!(without_overhang(pairs.clone(), true, false), pairs);
        assert_eq!(
            without_overhang(pairs.clone(), false, true),
            pairs[..3].to_vec(),
            "the right run is cut, so the left's last may be held past it"
        );
        let nothing_shared = vec![(Some(0), None), (None, Some(0))];
        assert_eq!(
            without_overhang(nothing_shared.clone(), true, true),
            nothing_shared
        );
    }
}
