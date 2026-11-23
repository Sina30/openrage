//! Two descriptions of the same bytes compared by position, never by name.
//!
//! Walked from a root in each, in parallel: at every type the size, and at
//! every field its offset, width, kind and what it points at. Pairs are
//! matched by where they sit, so the two sides may name nothing alike, and a
//! public description can be held to an independent private one without a
//! name crossing.

use std::collections::BTreeSet;

use or_io::desc::{CountSource, FieldRepr, Pack, PackField, PackType, Pointee};

/// One disagreement, located by the left side's names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    pub path: String,
    pub what: String,
}

impl std::fmt::Display for Difference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.what)
    }
}

/// Every place the two descriptions disagree, walking both from their roots.
pub fn structural(left: &Pack, left_root: &str, right: &Pack, right_root: &str) -> Vec<Difference> {
    walk(left, left_root, right, right_root).out
}

/// What the right states at a left field's offset and the left does not: bits, enum values, notes.
pub fn annotations(
    left: &Pack,
    left_root: &str,
    right: &Pack,
    right_root: &str,
) -> Vec<Difference> {
    // ⚠ Name-free: each is said in the left side's names, offsets, bit
    // positions, values and counts, never the right side's names or note text,
    // so a private description can be read against a public one in public.
    walk(left, left_root, right, right_root).annotations
}

fn walk<'a>(left: &'a Pack, left_root: &str, right: &'a Pack, right_root: &str) -> Walk<'a> {
    let mut walk = Walk {
        left,
        right,
        out: Vec::new(),
        annotations: Vec::new(),
        seen: BTreeSet::new(),
    };
    walk.types(left_root, right_root, left_root.to_string());
    walk
}

/// Types the two sides place at the same address with the same size: the
/// roots [`structural`] can compare where neither side can name the other's.
///
/// `left` and `right` are `(offset, type)` for every instance each walk
/// found; a pair is kept once however often it recurs.
pub fn paired_by_address(
    left_pack: &Pack,
    left: &[(usize, String)],
    right_pack: &Pack,
    right: &[(usize, String)],
) -> Vec<(String, String)> {
    // ⚠ A base and its variant can share an address and a size — a box bound
    // is its header and nothing more — so a pair also agrees on whether it is
    // tagged, and among what is left the closest field count wins.
    let shape = |pack: &Pack, name: &str| {
        pack.type_by_name(name).map(|t| {
            let fields = t
                .fields
                .iter()
                .filter(|f| !matches!(f.repr, FieldRepr::Padding { .. }))
                .count();
            (t.size, t.selector.is_some(), fields)
        })
    };
    let mut by_offset: std::collections::BTreeMap<usize, Vec<&str>> = Default::default();
    for (at, name) in right {
        by_offset.entry(*at).or_default().push(name);
    }
    let mut pairs = BTreeSet::new();
    for (at, lname) in left {
        let (Some(candidates), Some((size, tagged, fields))) =
            (by_offset.get(at), shape(left_pack, lname))
        else {
            continue;
        };
        let best = candidates
            .iter()
            .filter_map(|r| shape(right_pack, r).map(|s| (r, s)))
            .filter(|(_, (s, t, _))| *s == size && *t == tagged)
            .min_by_key(|(_, (_, _, f))| f.abs_diff(fields));
        if let Some((rname, _)) = best {
            pairs.insert((lname.clone(), rname.to_string()));
        }
    }
    pairs.into_iter().collect()
}

/// A [`Difference`]'s path resolved through `pack` to the `Type` or `Type.field` it ends on.
pub fn resolve(pack: &Pack, path: &str) -> String {
    let Some(root) = pack
        .types
        .iter()
        .map(|t| t.name.as_str())
        .filter(|n| path.starts_with(n))
        .max_by_key(|n| n.len())
    else {
        return path.to_string();
    };
    let into = |ty: &str, f: &str| -> Option<String> {
        match &pack.type_by_name(ty)?.field_by_name(f)?.repr {
            FieldRepr::Inline { type_name } | FieldRepr::InlineArray { type_name, .. } => {
                Some(type_name.clone())
            }
            FieldRepr::Ptr(p) | FieldRepr::AtArray(p) | FieldRepr::PtrSlots { pointee: p, .. } => {
                target(p).map(str::to_string)
            }
            FieldRepr::Trailing { type_name, .. } => type_name.clone(),
            _ => None,
        }
    };
    let mut ty = root.to_string();
    let mut field: Option<String> = None;
    let mut rest = &path[root.len()..];
    while !rest.is_empty() {
        if let Some(f) = field.take() {
            match into(&ty, &f) {
                Some(t) => ty = t,
                None => return path.to_string(),
            }
        }
        if let Some(r) = rest.strip_prefix("->").or_else(|| rest.strip_prefix("[]")) {
            rest = r;
        } else if let Some(r) = rest.strip_prefix('<') {
            let Some(end) = r.find('>') else {
                return path.to_string();
            };
            let chosen = r[..end].parse::<i64>().ok().and_then(|tag| {
                let variant = pack.type_by_name(&ty)?.selector.as_ref()?.type_for(tag)?;
                Some(variant.to_string())
            });
            match chosen {
                Some(t) => ty = t,
                None => return path.to_string(),
            }
            rest = &r[end + 1..];
        } else if let Some(r) = rest.strip_prefix('.') {
            let end = r.find(['.', '-', '[', '<']).unwrap_or(r.len());
            field = Some(r[..end].to_string());
            rest = &r[end..];
        } else {
            return path.to_string();
        }
    }
    match field {
        Some(f) => format!("{ty}.{f}"),
        None => ty,
    }
}

struct Walk<'a> {
    left: &'a Pack,
    right: &'a Pack,
    out: Vec<Difference>,
    annotations: Vec<Difference>,
    seen: BTreeSet<(String, String)>,
}

fn kind(repr: &FieldRepr) -> &'static str {
    match repr {
        FieldRepr::Scalar(_) => "scalar",
        FieldRepr::ScalarArray { .. } => "scalar array",
        FieldRepr::Inline { .. } => "embedded",
        FieldRepr::InlineArray { .. } => "embedded array",
        FieldRepr::Ptr(_) => "pointer",
        FieldRepr::AtArray(_) => "counted array",
        FieldRepr::PtrSlots { .. } => "pointer slots",
        FieldRepr::Enum { .. } => "enum",
        FieldRepr::Padding { .. } => "padding",
        FieldRepr::Opaque { .. } => "opaque",
        FieldRepr::Trailing { .. } => "trailing run",
    }
}

fn pointee_kind(p: &Pointee) -> &'static str {
    match p {
        Pointee::One { .. } => "one",
        Pointee::Many { .. } => "run",
        Pointee::ManyPtr { .. } => "pointer run",
        Pointee::Bytes { .. } => "bytes",
        Pointee::Str => "string",
        Pointee::Opaque => "opaque",
        Pointee::Member { .. } => "member",
        Pointee::Members { .. } => "members",
        Pointee::Strings { .. } => "strings",
    }
}

fn target(p: &Pointee) -> Option<&str> {
    match p {
        Pointee::One { type_name }
        | Pointee::Member { type_name }
        | Pointee::Members { type_name, .. } => Some(type_name),
        Pointee::Many { type_name, .. } | Pointee::ManyPtr { type_name, .. } => {
            type_name.as_deref()
        }
        _ => None,
    }
}

fn count_kind(c: &CountSource) -> &'static str {
    match c {
        CountSource::Field(_) => "field",
        CountSource::CountAndCapacity { .. } => "count and capacity",
        CountSource::MaskedField { .. } => "masked",
        CountSource::Fixed(_) => "fixed",
        CountSource::Inline => "inline",
        CountSource::Unknown => "unknown",
        CountSource::Product { .. } => "product",
        CountSource::PrefixedAboveOne { .. } => "prefixed above one",
        CountSource::Sum { .. } => "sum",
        CountSource::MaskedOrMipChain { .. } => "masked or mip chain",
    }
}

/// An enum field's values, each with whether it carries a note; empty for any other field.
fn enum_values(pack: &Pack, repr: &FieldRepr) -> Vec<(i64, bool)> {
    repr.referenced_enum()
        .and_then(|name| pack.enum_by_name(name))
        .map(|e| {
            e.variants
                .iter()
                .map(|v| (v.value, v.note.is_some()))
                .collect()
        })
        .unwrap_or_default()
}

fn count_of(p: &Pointee) -> Option<&CountSource> {
    match p {
        Pointee::Many { count, .. }
        | Pointee::ManyPtr { count, .. }
        | Pointee::Members { count, .. } => Some(count),
        Pointee::Bytes { length } => Some(length),
        _ => None,
    }
}

impl Walk<'_> {
    fn width(&self, pack: &Pack, repr: &FieldRepr) -> Option<usize> {
        repr.inline_size().or_else(|| match repr {
            FieldRepr::Inline { type_name } => pack.type_by_name(type_name).map(|t| t.size),
            FieldRepr::InlineArray { type_name, count } => {
                pack.type_by_name(type_name).map(|t| t.size * count)
            }
            _ => None,
        })
    }

    fn differ(&mut self, path: &str, what: String) {
        self.out.push(Difference {
            path: path.to_string(),
            what,
        });
    }

    fn annotate(&mut self, path: &str, l: &PackField, r: &PackField) {
        let mut said = Vec::new();
        let mut runs: Vec<(u32, u32)> = r
            .bits
            .iter()
            .filter(|rb| {
                !l.bits
                    .iter()
                    .any(|lb| (lb.low, lb.width) == (rb.low, rb.width))
            })
            .map(|b| (b.low, b.end()))
            .collect();
        runs.sort_unstable();
        let runs: Vec<String> = runs.iter().map(|(a, b)| format!("{a}..{b}")).collect();
        if !runs.is_empty() {
            said.push(format!(
                "the right side splits bits {} the left does not",
                runs.join(", ")
            ));
        }
        let unnoted_runs = r
            .bits
            .iter()
            .filter(|rb| rb.note.is_some())
            .filter(|rb| {
                !l.bits
                    .iter()
                    .any(|lb| (lb.low, lb.width) == (rb.low, rb.width) && lb.note.is_some())
            })
            .count();
        if unnoted_runs > 0 {
            said.push(format!(
                "the right side notes {unnoted_runs} bit run(s) the left does not"
            ));
        }
        let (lv, rv) = (
            enum_values(self.left, &l.repr),
            enum_values(self.right, &r.repr),
        );
        let unnamed: Vec<String> = rv
            .iter()
            .filter(|(v, _)| !lv.iter().any(|(w, _)| w == v))
            .map(|(v, _)| v.to_string())
            .collect();
        if !unnamed.is_empty() {
            said.push(format!(
                "the right side names {} value(s) the left does not: {}",
                unnamed.len(),
                unnamed.join(", ")
            ));
        }
        let unnoted_values = rv
            .iter()
            .filter(|(v, noted)| *noted && !lv.iter().any(|(w, n)| w == v && *n))
            .count();
        if unnoted_values > 0 {
            said.push(format!(
                "the right side notes {unnoted_values} value(s) the left does not"
            ));
        }
        if r.note.is_some() && l.note.is_none() {
            said.push("the right side notes it and the left does not".into());
        }
        for what in said {
            self.annotations.push(Difference {
                path: path.to_string(),
                what: format!("at {:#x}: {what}", l.offset),
            });
        }
    }

    fn types(&mut self, l: &str, r: &str, path: String) {
        if !self.seen.insert((l.to_string(), r.to_string())) {
            return;
        }
        let (Some(lt), Some(rt)) = (self.left.type_by_name(l), self.right.type_by_name(r)) else {
            let missing = if self.left.type_by_name(l).is_none() {
                "left"
            } else {
                "right"
            };
            self.differ(&path, format!("the {missing} side does not describe it"));
            return;
        };
        if lt.size != rt.size {
            self.differ(&path, format!("size {:#x} against {:#x}", lt.size, rt.size));
        }
        self.fields(lt, rt, &path);
        self.selectors(lt, rt, &path);
    }

    /// A tag at the same place selecting, value for value, the same layouts.
    fn selectors(&mut self, lt: &PackType, rt: &PackType, path: &str) {
        let (ls, rs) = match (&lt.selector, &rt.selector) {
            (None, None) => return,
            (Some(l), Some(r)) => (l, r),
            (l, _) => {
                let side = if l.is_some() { "left" } else { "right" };
                return self.differ(path, format!("only the {side} side is tagged"));
            }
        };
        let offset = |t: &PackType, tag: &str| t.field_by_name(tag).map(|f| f.offset);
        if offset(lt, &ls.tag) != offset(rt, &rs.tag) || ls.tag_bits != rs.tag_bits {
            self.differ(path, "the tag is not in the same place".into());
        }
        for (value, lname) in &ls.variants {
            match rs.type_for(*value) {
                Some(rname) => {
                    let rname = rname.to_string();
                    self.types(lname, &rname, format!("{path}<{value}>"));
                }
                None => self.differ(
                    path,
                    format!("tag {value} selects a layout on the left only"),
                ),
            }
        }
        for (value, _) in &rs.variants {
            if ls.type_for(*value).is_none() {
                self.differ(
                    path,
                    format!("tag {value} selects a layout on the right only"),
                );
            }
        }
    }

    fn fields(&mut self, lt: &PackType, rt: &PackType, path: &str) {
        let data = |t: &PackType| -> Vec<(usize, FieldRepr, String)> {
            let mut v: Vec<_> = t
                .fields
                .iter()
                .filter(|f| !matches!(f.repr, FieldRepr::Padding { .. }))
                .map(|f| (f.offset, f.repr.clone(), f.name.clone()))
                .collect();
            v.sort_by_key(|(o, ..)| *o);
            v
        };
        let (lf, rf) = (data(lt), data(rt));
        let mut j = 0;
        for (offset, lrepr, lname) in &lf {
            let here = format!("{path}.{lname}");
            if let (Some(l), Some(r)) = (
                lt.field_by_name(lname),
                rt.fields
                    .iter()
                    .find(|f| f.offset == *offset && !matches!(f.repr, FieldRepr::Padding { .. })),
            ) {
                self.annotate(&here, l, r);
            }
            while j < rf.len() && rf[j].0 < *offset {
                self.differ(
                    path,
                    format!(
                        "the right side has a field at {:#x} the left does not",
                        rf[j].0
                    ),
                );
                j += 1;
            }
            if j >= rf.len() || rf[j].0 != *offset {
                self.differ(
                    &here,
                    format!("at {offset:#x} on the left and at no offset on the right"),
                );
                continue;
            }
            let rrepr = rf[j].1.clone();
            j += 1;
            self.field(&here, lrepr, &rrepr);
        }
        for (offset, ..) in &rf[j..] {
            self.differ(
                path,
                format!("the right side has a field at {offset:#x} the left does not"),
            );
        }
    }

    fn field(&mut self, path: &str, l: &FieldRepr, r: &FieldRepr) {
        if kind(l) != kind(r) {
            self.differ(path, format!("{} against {}", kind(l), kind(r)));
            return;
        }
        let (lw, rw) = (self.width(self.left, l), self.width(self.right, r));
        if lw != rw {
            self.differ(path, format!("{lw:?} bytes against {rw:?}"));
        }
        match (l, r) {
            (FieldRepr::Scalar(a), FieldRepr::Scalar(b)) if a != b => {
                self.differ(path, format!("{a:?} against {b:?}"));
            }
            (FieldRepr::Inline { type_name: a }, FieldRepr::Inline { type_name: b })
            | (
                FieldRepr::InlineArray { type_name: a, .. },
                FieldRepr::InlineArray { type_name: b, .. },
            ) => {
                self.types(a, b, path.to_string());
            }
            (FieldRepr::Ptr(a), FieldRepr::Ptr(b))
            | (FieldRepr::AtArray(a), FieldRepr::AtArray(b))
            | (FieldRepr::PtrSlots { pointee: a, .. }, FieldRepr::PtrSlots { pointee: b, .. }) => {
                self.pointees(path, a, b);
            }
            (
                FieldRepr::Trailing {
                    type_name: a,
                    count: ca,
                    ..
                },
                FieldRepr::Trailing {
                    type_name: b,
                    count: cb,
                    ..
                },
            ) => {
                if count_kind(ca) != count_kind(cb) {
                    self.differ(
                        path,
                        format!("length by {} against {}", count_kind(ca), count_kind(cb)),
                    );
                }
                if let (Some(a), Some(b)) = (a, b) {
                    self.types(a, b, format!("{path}[]"));
                }
            }
            _ => {}
        }
    }

    fn pointees(&mut self, path: &str, a: &Pointee, b: &Pointee) {
        if pointee_kind(a) != pointee_kind(b) {
            self.differ(
                path,
                format!("points at {} against {}", pointee_kind(a), pointee_kind(b)),
            );
            return;
        }
        if let (Some(ca), Some(cb)) = (count_of(a), count_of(b)) {
            if count_kind(ca) != count_kind(cb) {
                self.differ(
                    path,
                    format!("length by {} against {}", count_kind(ca), count_kind(cb)),
                );
            }
        }
        match (target(a), target(b)) {
            (Some(ta), Some(tb)) => self.types(ta, tb, format!("{path}->")),
            (None, None) => {}
            _ => self.differ(
                path,
                "one side describes the target and the other does not".into(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::desc::{BitRange, IntWidth, PackField, Scalar};

    fn one_word(bits: Vec<BitRange>) -> Pack {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Word".into(),
            size: 4,
            fields: vec![PackField {
                name: "flags".into(),
                offset: 0,
                repr: FieldRepr::Scalar(Scalar::Int(IntWidth::U32)),
                space: None,
                bits,
                note: None,
            }],
            selector: None,
        });
        pack
    }

    #[test]
    fn a_field_whose_bits_alone_differ_is_named() {
        let run = |low, width| BitRange {
            name: "private".into(),
            low,
            width,
            note: Some("private".into()),
        };
        let left = one_word(Vec::new());
        let right = one_word(vec![run(0, 3), run(3, 5)]);
        assert!(
            structural(&left, "Word", &right, "Word").is_empty(),
            "the layout agrees"
        );
        let said: Vec<String> = annotations(&left, "Word", &right, "Word")
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            said,
            [
                "Word.flags: at 0x0: the right side splits bits 0..3, 3..8 the left does not",
                "Word.flags: at 0x0: the right side notes 2 bit run(s) the left does not",
            ]
        );
        assert!(said.iter().all(|s| !s.contains("private")), "name-free");
    }

    #[test]
    fn a_path_resolves_to_the_type_and_field_it_ends_on() {
        let mut pack = one_word(Vec::new());
        pack.types.push(PackType {
            name: "Holder".into(),
            size: 8,
            fields: vec![PackField {
                name: "word".into(),
                offset: 0,
                repr: FieldRepr::Ptr(Pointee::One {
                    type_name: "Word".into(),
                }),
                space: None,
                bits: Vec::new(),
                note: None,
            }],
            selector: None,
        });
        assert_eq!(resolve(&pack, "Holder.word->.flags"), "Word.flags");
        assert_eq!(resolve(&pack, "Holder.word"), "Holder.word");
        assert_eq!(
            resolve(&pack, "Holder.nothing->.flags"),
            "Holder.nothing->.flags"
        );
    }

    #[test]
    fn an_enum_and_a_note_only_the_right_carries_are_named_without_their_names() {
        let left = one_word(Vec::new());
        let mut right = one_word(Vec::new());
        let flags = &mut right.types[0].fields[0];
        flags.repr = FieldRepr::Enum {
            type_name: "PrivateKind".into(),
            width: IntWidth::U32,
        };
        flags.note = Some("private".into());
        right.enums.push(or_io::desc::PackEnum {
            name: "PrivateKind".into(),
            width: IntWidth::U32,
            variants: [0, 4]
                .into_iter()
                .map(|value| or_io::desc::PackVariant {
                    name: "private".into(),
                    value,
                    note: Some("private".into()),
                })
                .collect(),
        });
        let said: Vec<String> = annotations(&left, "Word", &right, "Word")
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            said,
            [
                "Word.flags: at 0x0: the right side names 2 value(s) the left does not: 0, 4",
                "Word.flags: at 0x0: the right side notes 2 value(s) the left does not",
                "Word.flags: at 0x0: the right side notes it and the left does not",
            ]
        );
        assert!(annotations(&right, "Word", &right, "Word").is_empty());
    }
}
