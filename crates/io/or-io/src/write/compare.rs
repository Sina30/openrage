//! Do two rebuilt resources hold the same content, ignoring where it landed?
//!
//! Byte-identity against the compiled writer fails for two measured reasons:
//! alignment — a `.ybn`'s `current_matrices` lands 0–15 bytes apart and every
//! later allocation shifts with it, which is the only difference in 19 of 21
//! files — and fields a description cannot rebuild, where the interpreter
//! writes zero and the compiled writer an allocation. So both outputs are read
//! back through the same pack and compared as value trees, every address
//! replaced by where in the tree it is owned ([`canonical_sharing`]): every
//! value, run length, capacity, opaque payload and **sharing relationship**
//! has to agree, and only where things landed may differ.
//!
//! [`blank_unrebuildable`] asks the description which fields a rebuild cannot
//! reproduce rather than listing them, so the exclusion turns itself off the
//! moment a pack describes the field. ⚠ It is only safe alongside
//! [`live_unrebuildable`], asserted *first*: "we do not compare this field"
//! must not also mean "and the interpreter may start writing one".

use std::collections::HashMap;

use crate::desc::value::{ElementTypes, ScalarValue};
use crate::desc::{CountSource, FieldRepr, Pack, Pointee, Record, RecordRef, RecordRun, Value};

fn no_elements() -> &'static ElementTypes {
    static NONE: std::sync::OnceLock<ElementTypes> = std::sync::OnceLock::new();
    NONE.get_or_init(ElementTypes::new)
}

/// The sites a caller's blob codec rebuilds, which the description cannot say.
///
/// ⚠ **Without this the exclusion is applied to fields that *are* rebuilt**,
/// and `same_content` refuses outright, because [`live_unrebuildable`] finds a
/// live pointer in a field it believes nothing can reproduce: every `.ydr`,
/// `.ydd` and `.yft` in the corpus refused at `CrSkeletonData::_bone_tags_ptr`
/// or `GrcInstanceData::params`, both of which a
/// [`BlobWriter`](crate::write::BlobWriter) writes in full.
#[derive(Debug, Clone, Default)]
pub struct CoveredSites {
    sites: Vec<(String, String)>,
    arrivals: Vec<(String, String)>,
}

impl CoveredSites {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn at(mut self, type_name: &str, field: &str) -> Self {
        self.sites.push((type_name.to_string(), field.to_string()));
        self
    }

    #[must_use]
    pub fn reached_through(mut self, type_name: &str, field: &str) -> Self {
        self.arrivals
            .push((type_name.to_string(), field.to_string()));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.sites.is_empty() && self.arrivals.is_empty()
    }

    fn covers(&self, arrival: Option<(&str, &str)>, site: (&str, &str)) -> bool {
        let named = |list: &[(String, String)], (ty, field): (&str, &str)| {
            list.iter().any(|(t, f)| t == ty && f == field)
        };
        arrival.is_some_and(|a| named(&self.arrivals, a)) || named(&self.sites, site)
    }
}

fn nothing_covered() -> &'static CoveredSites {
    static NONE: std::sync::OnceLock<CoveredSites> = std::sync::OnceLock::new();
    NONE.get_or_init(CoveredSites::new)
}

fn is_rebuildable(
    repr: &FieldRepr,
    arrival: Option<(&str, &str)>,
    site: (&str, &str),
    elements: &ElementTypes,
    covered: &CoveredSites,
) -> bool {
    if covered.covers(arrival, site) {
        return true;
    }
    let pointee = match repr {
        FieldRepr::Ptr(pointee) | FieldRepr::AtArray(pointee) => pointee,
        FieldRepr::PtrSlots { pointee, .. } => pointee,
        FieldRepr::Trailing { count, .. } => {
            return !matches!(count, CountSource::Unknown | CountSource::Inline)
        }
        _ => return true,
    };
    match pointee {
        Pointee::Opaque => false,
        Pointee::Bytes { length } => !matches!(length, CountSource::Unknown),
        Pointee::Many { count, .. } => !matches!(count, CountSource::Unknown),
        Pointee::ManyPtr { type_name, count } => {
            (type_name.is_some() || elements.resolve(arrival, site).is_some())
                && !matches!(count, CountSource::Unknown)
        }
        Pointee::Members { count, .. } | Pointee::Strings { count } => {
            !matches!(count, CountSource::Unknown)
        }
        Pointee::One { .. } | Pointee::Str | Pointee::Member { .. } => true,
    }
}

fn arrival_of<'a>(
    repr: Option<&FieldRepr>,
    enclosing: Option<(&'a str, &'a str)>,
    site: (&'a str, &'a str),
) -> Option<(&'a str, &'a str)> {
    match repr {
        Some(r) if r.is_indirect() => Some(site),
        _ => enclosing,
    }
}

fn null_ptr() -> Value {
    Value::Ptr {
        source: 0,
        target: None,
    }
}

pub fn blank_unrebuildable(pack: &Pack, value: &Value, also: &[(&str, &str)]) -> Value {
    blank_unrebuildable_with(pack, value, also, no_elements())
}

pub fn blank_unrebuildable_with(
    pack: &Pack,
    value: &Value,
    also: &[(&str, &str)],
    elements: &ElementTypes,
) -> Value {
    blank_unrebuildable_using(pack, value, also, elements, nothing_covered())
}

pub fn blank_unrebuildable_using(
    _pack: &Pack,
    value: &Value,
    also: &[(&str, &str)],
    elements: &ElementTypes,
    covered: &CoveredSites,
) -> Value {
    #[allow(clippy::too_many_arguments)]
    fn go(
        value: &Value,
        also: &[(&str, &str)],
        elements: &ElementTypes,
        covered: &CoveredSites,
        arrival: Option<(&str, &str)>,
    ) -> Value {
        match value {
            Value::Struct(record) => {
                let rec = record.as_ref();
                let mut out = record.clone();
                for site in rec.walk() {
                    let key = (site.owner.name.as_str(), site.field.name.as_str());
                    let named = also.iter().any(|(t, f)| (*t, *f) == key);
                    if !site.is_child() {
                        if named {
                            let width = site.field.repr.inline_size().unwrap_or(0);
                            if let Some(bytes) = out.bytes_mut().get_mut(site.at..site.at + width) {
                                bytes.fill(0);
                            }
                        }
                        continue;
                    }
                    let Some(child) = rec.child_at(site.at) else {
                        continue;
                    };
                    let repr = &site.field.repr;
                    let compared = !named && is_rebuildable(repr, arrival, key, elements, covered);
                    let replaced = if compared {
                        let inner = arrival_of(Some(repr), arrival, key);
                        go(child, also, elements, covered, inner)
                    } else {
                        null_ptr()
                    };
                    if let Some(slot) = out.child_at_mut(site.at) {
                        *slot = replaced;
                    }
                }
                Value::Struct(out)
            }
            Value::Records(run) => {
                let Some(first) = run.get(0) else {
                    return value.clone();
                };
                let named: Vec<(usize, usize)> = first
                    .walk()
                    .into_iter()
                    .filter(|site| {
                        let key = (site.owner.name.as_str(), site.field.name.as_str());
                        !site.is_child() && also.iter().any(|(t, f)| (*t, *f) == key)
                    })
                    .map(|site| (site.at, site.field.repr.inline_size().unwrap_or(0)))
                    .collect();
                if named.is_empty() {
                    return value.clone();
                }
                let mut out = run.clone();
                let stride = out.stride();
                for element in out.bytes_mut().chunks_exact_mut(stride) {
                    for &(at, width) in &named {
                        if let Some(bytes) = element.get_mut(at..at + width) {
                            bytes.fill(0);
                        }
                    }
                }
                Value::Records(out)
            }
            // A codec's own parts describe nothing the pack could rule
            // unrebuildable; the arrival is the codec's pointer, unchanged.
            Value::Blob { codec, parts } => Value::Blob {
                codec: codec.clone(),
                parts: parts
                    .iter()
                    .map(|(name, part)| {
                        if also.iter().any(|(t, f)| t == codec && f == name) {
                            (name.clone(), null_ptr())
                        } else {
                            (name.clone(), go(part, also, elements, covered, arrival))
                        }
                    })
                    .collect(),
            },
            Value::Array {
                elements: items,
                capacity,
            } => Value::Array {
                elements: items
                    .iter()
                    .map(|e| go(e, also, elements, covered, arrival))
                    .collect(),
                capacity: *capacity,
            },
            Value::Ptr { source, target } => Value::Ptr {
                source: *source,
                target: target
                    .as_ref()
                    .map(|t| Box::new(go(t, also, elements, covered, arrival))),
            },
            other => other.clone(),
        }
    }
    go(value, also, elements, covered, None)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Divergence {
    pub field: String,
    pub path: String,
    pub compiled: String,
    pub rebuilt: String,
}

pub fn differences(
    pack: &Pack,
    compiled: &Value,
    rebuilt: &Value,
    also_blank: &[(&str, &str)],
) -> Vec<Divergence> {
    differences_with(pack, compiled, rebuilt, also_blank, no_elements())
}

pub fn differences_with(
    pack: &Pack,
    compiled: &Value,
    rebuilt: &Value,
    also_blank: &[(&str, &str)],
    elements: &ElementTypes,
) -> Vec<Divergence> {
    differences_using(
        pack,
        compiled,
        rebuilt,
        also_blank,
        elements,
        nothing_covered(),
    )
}

pub fn differences_using(
    pack: &Pack,
    compiled: &Value,
    rebuilt: &Value,
    also_blank: &[(&str, &str)],
    elements: &ElementTypes,
    covered: &CoveredSites,
) -> Vec<Divergence> {
    let a = canonical_sharing(&blank_unrebuildable_using(
        pack, compiled, also_blank, elements, covered,
    ));
    let b = canonical_sharing(&blank_unrebuildable_using(
        pack, rebuilt, also_blank, elements, covered,
    ));
    let mut out = Vec::new();
    collect(&a, &b, "root", "<root>", &mut out);
    out
}

fn collect(a: &Value, b: &Value, path: &str, owner: &str, out: &mut Vec<Divergence>) {
    if same_value(a, b) {
        return;
    }
    match (a, b) {
        (Value::Struct(ra), Value::Struct(rb)) if ra.type_name() == rb.type_name() => {
            collect_fields(ra.as_ref(), rb.as_ref(), path, out);
        }
        (Value::Records(ra), Value::Records(rb))
            if ra.type_name() == rb.type_name() && ra.len() == rb.len() =>
        {
            for (i, (x, y)) in ra.iter().zip(rb.iter()).enumerate() {
                if !same_fields(x, y) {
                    collect_fields(x, y, &format!("{path}[{i}]"), out);
                }
            }
        }
        (
            Value::Blob {
                codec: ca,
                parts: pa,
            },
            Value::Blob {
                codec: cb,
                parts: pb,
            },
        ) if ca == cb && pa.len() == pb.len() => {
            for ((na, va), (nb, vb)) in pa.iter().zip(pb) {
                if na != nb {
                    break;
                }
                collect(va, vb, &format!("{path}.{na}"), &format!("{ca}::{na}"), out);
            }
        }
        (Value::Array { elements: ea, .. }, Value::Array { elements: eb, .. })
            if ea.len() == eb.len() =>
        {
            for (i, (va, vb)) in ea.iter().zip(eb).enumerate() {
                collect(va, vb, &format!("{path}[{i}]"), owner, out);
            }
        }
        (
            Value::Ptr {
                target: Some(ta), ..
            },
            Value::Ptr {
                target: Some(tb), ..
            },
        ) => collect(ta, tb, path, owner, out),
        _ => out.push(Divergence {
            field: owner.to_string(),
            path: path.to_string(),
            compiled: shape(a),
            rebuilt: shape(b),
        }),
    }
}

fn collect_fields(a: RecordRef<'_>, b: RecordRef<'_>, path: &str, out: &mut Vec<Divergence>) {
    for site in a.walk() {
        let here = format!("{path}.{}", site.path);
        let field = format!("{}::{}", site.owner.name, site.field.name);
        if site.is_child() {
            match (a.child_at(site.at), b.child_at(site.at)) {
                (Some(va), Some(vb)) => collect(va, vb, &here, &field, out),
                (None, None) => {}
                (va, vb) => out.push(Divergence {
                    field,
                    path: here,
                    compiled: va.map(shape).unwrap_or_else(|| "nothing".into()),
                    rebuilt: vb.map(shape).unwrap_or_else(|| "nothing".into()),
                }),
            }
            continue;
        }
        if let FieldRepr::ScalarArray { element, count } = &site.field.repr {
            for i in 0..*count {
                let at = site.at + i * element.size();
                let (x, y) = (
                    bytes_at(a, at, element.size()),
                    bytes_at(b, at, element.size()),
                );
                if x != y {
                    let element_path = format!("{}[{i}]", site.path);
                    out.push(Divergence {
                        field: field.clone(),
                        path: format!("{here}[{i}]"),
                        compiled: shape_at(a, &element_path),
                        rebuilt: shape_at(b, &element_path),
                    });
                }
            }
            continue;
        }
        let width = site.field.repr.inline_size().unwrap_or(0);
        if bytes_at(a, site.at, width) != bytes_at(b, site.at, width) {
            out.push(Divergence {
                field,
                path: here,
                compiled: shape_at(a, &site.path),
                rebuilt: shape_at(b, &site.path),
            });
        }
    }
}

fn bytes_at<'a>(rec: RecordRef<'a>, at: usize, width: usize) -> &'a [u8] {
    rec.bytes().get(at..at + width).unwrap_or(&[])
}

fn shape_at(rec: RecordRef<'_>, path: &str) -> String {
    rec.value(path)
        .map(|v| shape(&v))
        .unwrap_or_else(|| "nothing".into())
}

/// Two records agree in every byte a field describes and in every child.
///
/// Pointer slots and bytes no field covers are not compared: where things
/// landed is the one thing two builds may disagree about.
fn same_record(a: &Record, b: &Record) -> bool {
    a.type_name() == b.type_name()
        && a.bytes().len() == b.bytes().len()
        && a.plan()
            .copy_ranges()
            .iter()
            .all(|&(start, end)| a.bytes()[start..end] == b.bytes()[start..end])
        && a.children().len() == b.children().len()
        && a.children()
            .iter()
            .zip(b.children())
            .all(|((oa, va), (ob, vb))| oa == ob && same_value(va, vb))
}

/// [`same_record`] for two structs with no children: every described byte.
fn same_fields(a: RecordRef<'_>, b: RecordRef<'_>) -> bool {
    a.type_name() == b.type_name()
        && a.bytes().len() == b.bytes().len()
        && a.plan()
            .copy_ranges()
            .iter()
            .all(|&(start, end)| a.bytes()[start..end] == b.bytes()[start..end])
}

fn same_records(a: &RecordRun, b: &RecordRun) -> bool {
    if a.type_name() != b.type_name() || a.capacity() != b.capacity() || a.len() != b.len() {
        return false;
    }
    if a.plan().copy_ranges() == [(0, a.stride())] {
        return a.bytes() == b.bytes();
    }
    a.iter().zip(b.iter()).all(|(x, y)| same_fields(x, y))
}

/// A packed run against a run of records: the same structs, whichever way each is held.
fn same_run_either_way(run: &RecordRun, elements: &[Value], capacity: Option<usize>) -> bool {
    run.capacity() == capacity
        && run.len() == elements.len()
        && run.iter().zip(elements).all(|(x, y)| {
            y.record()
                .is_some_and(|y| y.children().is_empty() && same_fields(x, y.as_ref()))
        })
}

pub fn live_unrebuildable(pack: &Pack, value: &Value) -> Option<String> {
    live_unrebuildable_with(pack, value, no_elements())
}

pub fn live_unrebuildable_with(
    pack: &Pack,
    value: &Value,
    elements: &ElementTypes,
) -> Option<String> {
    live_unrebuildable_using(pack, value, elements, nothing_covered())
}

pub fn live_unrebuildable_using(
    _pack: &Pack,
    value: &Value,
    elements: &ElementTypes,
    covered: &CoveredSites,
) -> Option<String> {
    fn go(
        value: &Value,
        path: &str,
        elements: &ElementTypes,
        covered: &CoveredSites,
        arrival: Option<(&str, &str)>,
    ) -> Option<String> {
        match value {
            Value::Struct(record) => {
                let rec = record.as_ref();
                for site in rec.walk() {
                    if !site.is_child() {
                        continue;
                    }
                    let Some(field) = rec.child_at(site.at) else {
                        continue;
                    };
                    let here = format!("{path}.{}", site.path);
                    let repr = &site.field.repr;
                    let key = (site.owner.name.as_str(), site.field.name.as_str());
                    if !is_rebuildable(repr, arrival, key, elements, covered) {
                        if carries_content(field) {
                            return Some(format!("{here} (in a `{}`)", site.owner.name));
                        }
                        continue;
                    }
                    let inner = arrival_of(Some(repr), arrival, key);
                    if let Some(found) = go(field, &here, elements, covered, inner) {
                        return Some(found);
                    }
                }
                None
            }
            Value::Blob { parts, .. } => parts.iter().find_map(|(name, part)| {
                go(part, &format!("{path}.{name}"), elements, covered, arrival)
            }),
            Value::Array {
                elements: items, ..
            } => items
                .iter()
                .enumerate()
                .find_map(|(i, e)| go(e, &format!("{path}[{i}]"), elements, covered, arrival)),
            Value::Ptr {
                target: Some(inner),
                ..
            } => go(inner, path, elements, covered, arrival),
            _ => None,
        }
    }
    go(value, "root", elements, covered, None)
}

fn carries_content(value: &Value) -> bool {
    match value {
        Value::Ptr { target: None, .. } => false,
        Value::Ptr {
            target: Some(inner),
            ..
        } => carries_content(inner),
        Value::Array { elements, .. } => elements.iter().any(carries_content),
        Value::Records(run) => !run.is_empty(),
        Value::Bytes(bytes) | Value::Opaque(bytes) => !bytes.is_empty(),
        Value::UndescribedPtr(address) | Value::NotAnAddress(address) => *address != 0,
        Value::Alias(_) | Value::Member { .. } | Value::Struct(_) | Value::Blob { .. } => true,
        Value::Scalar(_) | Value::Enum { .. } => false,
    }
}

/// A value tree with every address replaced by **where in the tree it is
/// owned**, so two trees can be compared for sharing rather than for placement.
///
/// ⚠ Not an ordinal by first appearance: one extra allocation — exactly what a
/// writer that duplicates a shared target produces — renumbers every alias
/// after it. On the fragment corpus, 61 physics LODs where one writer shares a
/// composite and the other copies it reported as 61 divergences plus 58 more
/// at `PhBoundComposite::last_matrices`, where both trees said "an alias" and
/// only the numbering disagreed. A path does not drift, so a real un-aliasing
/// is still a difference and a downstream renumbering is not.
///
/// The path is hashed rather than stored, because [`Value::Alias`] holds a
/// `u64`.
pub fn canonical_sharing(value: &Value) -> Value {
    fn hash(path: &str) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in path.as_bytes() {
            h ^= u64::from(*byte);
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        h
    }

    fn owners_of(value: &Value, path: &str, owners: &mut HashMap<u64, u64>) {
        match value {
            Value::Ptr { source, target } => {
                // ⚠ **An allocation of nothing cannot own an address**, the
                // rule `or_io::desc::value`'s reader applies one level down. A
                // shipped file can hold a captured runtime pointer beside a
                // stated length of zero, landing on somebody else's allocation:
                // on `dinghy.yft` and `sanchez.yft` a `grcTexture::pixel_data`
                // sits exactly on `children[4].undamaged_drawable.bound`.
                // Counted as an owner, it attributes the composite's fourth
                // child to a dead texture pointer, and a rebuild, which
                // correctly drops the dead pointer, reads as having changed the
                // sharing.
                if *source != 0 && target.as_deref().is_some_and(carries_content) {
                    owners.entry(*source).or_insert_with(|| hash(path));
                }
                if let Some(target) = target {
                    owners_of(target, path, owners);
                }
            }
            Value::Struct(record) => {
                let rec = record.as_ref();
                for site in rec.walk().into_iter().filter(|s| s.is_child()) {
                    if let Some(child) = rec.child_at(site.at) {
                        owners_of(child, &format!("{path}.{}", site.path), owners);
                    }
                }
            }
            Value::Blob { parts, .. } => {
                for (name, part) in parts {
                    owners_of(part, &format!("{path}.{name}"), owners);
                }
            }
            Value::Array { elements, .. } => {
                for (i, element) in elements.iter().enumerate() {
                    owners_of(element, &format!("{path}[{i}]"), owners);
                }
            }
            _ => {}
        }
    }

    fn go(value: &Value, path: &str, owners: &mut HashMap<u64, u64>) -> Value {
        match value {
            Value::Ptr { target, .. } => Value::Ptr {
                source: 0,
                target: target.as_ref().map(|t| Box::new(go(t, path, owners))),
            },
            Value::Alias(address) => Value::Alias(owners.get(address).copied().unwrap_or(0)),
            Value::UndescribedPtr(address) => Value::UndescribedPtr(u64::from(*address != 0)),
            Value::Struct(record) => {
                let mut out = record.clone();
                for site in record.as_ref().walk().into_iter().filter(|s| s.is_child()) {
                    if let Some(child) = record.child_at(site.at) {
                        let replaced = go(child, &format!("{path}.{}", site.path), owners);
                        if let Some(slot) = out.child_at_mut(site.at) {
                            *slot = replaced;
                        }
                    }
                }
                Value::Struct(out)
            }
            Value::Blob { codec, parts } => Value::Blob {
                codec: codec.clone(),
                parts: parts
                    .iter()
                    .map(|(name, part)| (name.clone(), go(part, &format!("{path}.{name}"), owners)))
                    .collect(),
            },
            Value::Array { elements, capacity } => Value::Array {
                elements: elements
                    .iter()
                    .enumerate()
                    .map(|(i, e)| go(e, &format!("{path}[{i}]"), owners))
                    .collect(),
                capacity: *capacity,
            },
            other => other.clone(),
        }
    }

    let mut owners = HashMap::new();
    owners_of(value, "root", &mut owners);
    go(value, "root", &mut owners)
}

pub fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Scalar(ScalarValue::F32(x)), Value::Scalar(ScalarValue::F32(y))) => {
            x.to_bits() == y.to_bits()
        }
        (Value::Scalar(ScalarValue::F64(x)), Value::Scalar(ScalarValue::F64(y))) => {
            x.to_bits() == y.to_bits()
        }
        (Value::Struct(ra), Value::Struct(rb)) => same_record(ra, rb),
        (Value::Records(ra), Value::Records(rb)) => same_records(ra, rb),
        (Value::Records(run), Value::Array { elements, capacity })
        | (Value::Array { elements, capacity }, Value::Records(run)) => {
            same_run_either_way(run, elements, *capacity)
        }
        (
            Value::Blob {
                codec: ca,
                parts: pa,
            },
            Value::Blob {
                codec: cb,
                parts: pb,
            },
        ) => {
            ca == cb
                && pa.len() == pb.len()
                && pa
                    .iter()
                    .zip(pb)
                    .all(|((na, va), (nb, vb))| na == nb && same_value(va, vb))
        }
        (
            Value::Array {
                elements: ea,
                capacity: ca,
            },
            Value::Array {
                elements: eb,
                capacity: cb,
            },
        ) => ca == cb && ea.len() == eb.len() && ea.iter().zip(eb).all(|(x, y)| same_value(x, y)),
        (
            Value::Ptr {
                source: sa,
                target: ta,
            },
            Value::Ptr {
                source: sb,
                target: tb,
            },
        ) => {
            sa == sb
                && match (ta, tb) {
                    (None, None) => true,
                    (Some(x), Some(y)) => same_value(x, y),
                    _ => false,
                }
        }
        _ => a == b,
    }
}

pub fn first_difference(a: &Value, b: &Value, path: &str) -> String {
    match (a, b) {
        (Value::Struct(ra), Value::Struct(rb)) => {
            let (ta, tb) = (ra.type_name(), rb.type_name());
            if ta != tb {
                return format!("{path}: type `{ta}` against `{tb}`");
            }
            let (a, b) = (ra.as_ref(), rb.as_ref());
            for site in a.walk() {
                let here = format!("{path}.{}", site.path);
                if site.is_child() {
                    match (a.child_at(site.at), b.child_at(site.at)) {
                        (Some(va), Some(vb)) if !same_value(va, vb) => {
                            return first_difference(va, vb, &here)
                        }
                        (Some(_), None) | (None, Some(_)) => {
                            return format!("{here}: a child in one record and not the other")
                        }
                        _ => {}
                    }
                    continue;
                }
                let width = site.field.repr.inline_size().unwrap_or(0);
                if bytes_at(a, site.at, width) != bytes_at(b, site.at, width) {
                    return format!(
                        "{here}: {} against {}",
                        shape_at(a, &site.path),
                        shape_at(b, &site.path)
                    );
                }
            }
            format!("{path} ({ta}): equal field by field, which should not happen here")
        }
        (
            Value::Blob {
                codec: ca,
                parts: pa,
            },
            Value::Blob {
                codec: cb,
                parts: pb,
            },
        ) => {
            if ca != cb {
                return format!("{path}: codec `{ca}` against `{cb}`");
            }
            for ((na, va), (nb, vb)) in pa.iter().zip(pb) {
                if na != nb {
                    return format!("{path} ({ca}): part `{na}` against `{nb}`");
                }
                if !same_value(va, vb) {
                    return first_difference(va, vb, &format!("{path}.{na}"));
                }
            }
            format!("{path} ({ca}): {} parts against {}", pa.len(), pb.len())
        }
        (Value::Array { elements: ea, .. }, Value::Array { elements: eb, .. }) => {
            if ea.len() != eb.len() {
                return format!("{path}: {} elements against {}", ea.len(), eb.len());
            }
            for (i, (va, vb)) in ea.iter().zip(eb).enumerate() {
                if !same_value(va, vb) {
                    return first_difference(va, vb, &format!("{path}[{i}]"));
                }
            }
            format!("{path}: equal element by element, which should not happen here")
        }
        (Value::Records(ra), Value::Records(rb)) => {
            let (ta, tb) = (ra.type_name(), rb.type_name());
            if ta != tb {
                return format!("{path}: a run of `{ta}` against `{tb}`");
            }
            if ra.len() != rb.len() {
                return format!("{path}: {} elements against {}", ra.len(), rb.len());
            }
            for (i, (a, b)) in ra.iter().zip(rb.iter()).enumerate() {
                if same_fields(a, b) {
                    continue;
                }
                for site in a.walk() {
                    let width = site.field.repr.inline_size().unwrap_or(0);
                    if bytes_at(a, site.at, width) != bytes_at(b, site.at, width) {
                        return format!(
                            "{path}[{i}].{}: {} against {}",
                            site.path,
                            shape_at(a, &site.path),
                            shape_at(b, &site.path)
                        );
                    }
                }
            }
            format!(
                "{path}: capacity {:?} against {:?}",
                ra.capacity(),
                rb.capacity()
            )
        }
        (
            Value::Ptr {
                target: Some(ta), ..
            },
            Value::Ptr {
                target: Some(tb), ..
            },
        ) => first_difference(ta, tb, path),
        (Value::Bytes(ba), Value::Bytes(bb)) if ba.len() != bb.len() => {
            format!("{path}: {} bytes against {}", ba.len(), bb.len())
        }
        (Value::Opaque(oa), Value::Opaque(ob)) if oa.len() != ob.len() => {
            format!("{path}: {} opaque bytes against {}", oa.len(), ob.len())
        }
        _ => format!("{path}: {} against {}", shape(a), shape(b)),
    }
}

pub fn shape(value: &Value) -> String {
    match value {
        Value::Scalar(s) => format!("{s:?}"),
        Value::Enum { enum_name, value } => format!("{enum_name}({value})"),
        Value::Struct(record) => format!("a `{}`", record.type_name()),
        Value::Blob { codec, .. } => format!("a `{codec}`"),
        Value::Array { elements, .. } => format!("a run of {}", elements.len()),
        Value::Records(run) => format!("a run of {}", run.len()),
        Value::Bytes(b) => format!("{} byte(s)", b.len()),
        Value::Ptr { target: None, .. } => "null".to_string(),
        Value::Ptr { .. } => "a pointer".to_string(),
        Value::Alias(a) => format!("an alias of {a}"),
        Value::Member {
            field,
            index: Some(i),
        } => format!("the member `{field}[{i}]`"),
        Value::Member { field, index: None } => format!("the member `{field}`"),
        Value::Opaque(o) => format!("{} opaque byte(s)", o.len()),
        Value::UndescribedPtr(a) => format!("an undescribed pointer to {a}"),
        Value::NotAnAddress(v) => format!("{v:#x}, which is not an address"),
    }
}

pub fn same_content(pack: &Pack, compiled: &Value, from_pack: &Value) -> Result<(), String> {
    same_content_with(pack, compiled, from_pack, no_elements())
}

pub fn same_content_with(
    pack: &Pack,
    compiled: &Value,
    from_pack: &Value,
    elements: &ElementTypes,
) -> Result<(), String> {
    same_content_using(pack, compiled, from_pack, elements, nothing_covered())
}

pub fn same_content_using(
    pack: &Pack,
    compiled: &Value,
    from_pack: &Value,
    elements: &ElementTypes,
    covered: &CoveredSites,
) -> Result<(), String> {
    if let Some(where_) = live_unrebuildable_using(pack, from_pack, elements, covered) {
        return Err(format!(
            "the interpreter wrote a live pointer at {where_}, in a field the description \
             cannot express — so it is excluded from this comparison and must be empty, or \
             the exclusion becomes a licence"
        ));
    }

    let compiled = canonical_sharing(&blank_unrebuildable_using(
        pack,
        compiled,
        &[],
        elements,
        covered,
    ));
    let from_pack = canonical_sharing(&blank_unrebuildable_using(
        pack,
        from_pack,
        &[],
        elements,
        covered,
    ));

    if same_value(&compiled, &from_pack) {
        return Ok(());
    }
    Err(first_difference(&compiled, &from_pack, "root"))
}
