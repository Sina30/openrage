//! Every described instance a resource holds, and whether it sits where its
//! type's alignment says it must: the measurement a view's `&T` rests on.
//!
//! A view casts file bytes to `&T`, so `T`'s alignment has to hold at every
//! offset an instance of `T` sits at. The design expects natural alignment,
//! because a resource is a memory image; this is what says so, per type, and
//! a type with one misaligned instance is declared `packed` before its view ships.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use or_io::desc::{CountSource, FieldRepr, Image, IntWidth, Pack, PackType, Pointee, Scalar};
use or_io::write::build::natural_alignment;

/// One type's instances in one walk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    pub instances: usize,
    pub misaligned: usize,
    /// Up to eight offsets of misaligned instances.
    pub examples: Vec<usize>,
    pub alignment: usize,
}

/// Instances per type name, found by walking the description from a root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Census {
    pub types: BTreeMap<String, Tally>,
    /// Pointers or counts the walk could not follow, with why.
    pub unreached: Vec<String>,
    /// Pointer fields holding a word that is not an address, which a read carries as data.
    pub not_addresses: usize,
    /// Every tag value read at each tagged type, with how many instances held it.
    pub tags: BTreeMap<String, BTreeMap<i64, usize>>,
    /// Tagged instances whose tag selects no declared variant.
    pub undeclared: Vec<String>,
}

impl Census {
    pub fn misaligned(&self) -> impl Iterator<Item = (&str, &Tally)> {
        self.types
            .iter()
            .filter(|(_, t)| t.misaligned > 0)
            .map(|(n, t)| (n.as_str(), t))
    }

    pub fn fold(&mut self, other: Census) {
        for (name, tally) in other.types {
            let mine = self.types.entry(name).or_default();
            mine.alignment = tally.alignment;
            mine.instances += tally.instances;
            mine.misaligned += tally.misaligned;
            for e in tally.examples {
                if mine.examples.len() < 8 {
                    mine.examples.push(e);
                }
            }
        }
        self.unreached.extend(other.unreached);
        self.not_addresses += other.not_addresses;
        for (name, values) in other.tags {
            let mine = self.tags.entry(name).or_default();
            for (value, n) in values {
                *mine.entry(value).or_default() += n;
            }
        }
        self.undeclared.extend(other.undeclared);
    }
}

/// Walk `root` at `offset` and every instance its pointers, runs and embedded fields reach.
pub fn take(pack: &Pack, image: Image<'_>, root: &str, offset: usize) -> Census {
    run(pack, image, root, offset).census
}

/// [`take`], with every described instance it visited as `(type, offset)`.
pub fn walk(
    pack: &Pack,
    image: Image<'_>,
    root: &str,
    offset: usize,
) -> (Census, BTreeSet<(String, usize)>) {
    let Walk {
        census,
        seen,
        types,
        ..
    } = run(pack, image, root, offset);
    let sites = seen
        .into_iter()
        .filter(|(name, _)| types.get(name).is_some_and(Option::is_some))
        .map(|(name, at)| (name.to_string(), at))
        .collect();
    (census, sites)
}

fn run<'p, 'i>(pack: &'p Pack, image: Image<'i>, root: &str, offset: usize) -> Walk<'p, 'i> {
    let mut walk = Walk {
        pack,
        image,
        census: Census::default(),
        seen: HashSet::new(),
        types: HashMap::new(),
    };
    match pack.type_by_name(root) {
        Some(ty) => walk.visit(&ty.name, offset, 0),
        None => walk
            .census
            .unreached
            .push(format!("`{root}` is not described")),
    }
    walk
}

/// A run of a type's bytes its description does not claim as data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    pub type_name: String,
    /// The field declaring it, or `after <field>` for bytes no field covers.
    pub label: String,
    pub at: usize,
    pub len: usize,
    /// A slot holding a pointer at something the description does not describe.
    pub pointer: bool,
}

/// Every type's regions: declared padding, opaque bytes, pointers at nothing
/// described, and bytes no field covers.
pub fn regions(pack: &Pack) -> Vec<Region> {
    let width = |repr: &FieldRepr| {
        repr.inline_size().or_else(|| match repr {
            FieldRepr::Inline { type_name } => pack.type_by_name(type_name).map(|t| t.size),
            FieldRepr::InlineArray { type_name, count } => {
                pack.type_by_name(type_name).map(|t| t.size * count)
            }
            _ => None,
        })
    };
    let mut out = Vec::new();
    for ty in &pack.types {
        let mut fields: Vec<_> = ty.fields.iter().filter(|f| !f.repr.is_trailing()).collect();
        fields.sort_by_key(|f| f.offset);
        let region = |label: String, at: usize, len: usize, pointer: bool| Region {
            type_name: ty.name.clone(),
            label,
            at,
            len,
            pointer,
        };
        // `None` once a field's width is not stated: no gap is guessed past it.
        let mut covered = Some(0usize);
        let mut previous = "the start";
        for f in fields {
            if let Some(end) = covered.filter(|end| *end < f.offset) {
                out.push(region(
                    format!("after {previous}"),
                    end,
                    f.offset - end,
                    false,
                ));
            }
            let len = width(&f.repr);
            match (&f.repr, f.repr.pointee(), len) {
                (FieldRepr::Padding { .. } | FieldRepr::Opaque { .. }, _, Some(len)) => {
                    out.push(region(f.name.clone(), f.offset, len, false))
                }
                // Only the pointer: an opaque counted array's count and capacity are data.
                (FieldRepr::AtArray(_), Some(Pointee::Opaque), _) => {
                    out.push(region(f.name.clone(), f.offset, 8, true))
                }
                (_, Some(Pointee::Opaque), Some(len)) => {
                    out.push(region(f.name.clone(), f.offset, len, true))
                }
                _ => {}
            }
            covered = covered.zip(len).map(|(end, len)| end.max(f.offset + len));
            previous = &f.name;
        }
        if let Some(end) = covered.filter(|end| *end < ty.size) {
            out.push(region(
                format!("after {previous}"),
                end,
                ty.size - end,
                false,
            ));
        }
    }
    out
}

/// How often a region holds something, over the instances a walk reached.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Liveness {
    pub reached: usize,
    pub non_zero: usize,
    /// Pointer slots whose word resolves inside the resource to eight bytes not all zero.
    pub leads_somewhere: usize,
}

impl Liveness {
    pub fn add(&mut self, other: Liveness) {
        self.reached += other.reached;
        self.non_zero += other.non_zero;
        self.leads_somewhere += other.leads_somewhere;
    }
}

/// Each region's liveness at `sites`, keyed by `(type, label)`.
pub fn liveness(
    regions: &[Region],
    image: Image<'_>,
    sites: &BTreeSet<(String, usize)>,
) -> BTreeMap<(String, String), Liveness> {
    let mut at: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (name, offset) in sites {
        at.entry(name).or_default().push(*offset);
    }
    let mut out: BTreeMap<(String, String), Liveness> = BTreeMap::new();
    for region in regions {
        let Some(offsets) = at.get(region.type_name.as_str()) else {
            continue;
        };
        let slot = out
            .entry((region.type_name.clone(), region.label.clone()))
            .or_default();
        for offset in offsets {
            let Ok(run) = image.slice_at(offset + region.at, region.len) else {
                continue;
            };
            slot.reached += 1;
            if run.iter().all(|b| *b == 0) {
                continue;
            }
            slot.non_zero += 1;
            // ⚠ A captured runtime address is non-zero and means nothing: what
            // counts is a word that resolves inside this resource to real bytes.
            if region.pointer {
                let head = image
                    .pointer_at(offset + region.at)
                    .ok()
                    .and_then(|word| image.resolve(word).ok())
                    .and_then(|to| image.slice_at(to, 8).ok());
                if head.is_some_and(|h| h.iter().any(|b| *b != 0)) {
                    slot.leads_somewhere += 1;
                }
            }
        }
    }
    out
}

/// Sites a canonical save writes null by decision, whatever the format, and why.
pub const RULED: &[(&str, &str)] = &[(
    "PgBase.first_node",
    "pgBase's list head, the defragmenter's runtime pointer: the eight bytes it leads \
         to in a shipped file are zero, and a carried word would name bytes the rebuild moved",
)];

// ⚠ The one list of what a save loses: `what_a_save_drops_is_the_list` holds
// it exact in both directions, and the padding census holds every region a
// save writes null to it or to `RULED`.
/// `(extension, target, Type.field, class)` for every site a canonical save still loses.
pub const STILL_LOST: &[(&str, &str, &str, &str)] =
    &[("ydr", "gen8", "GrmShaderGroup.shader_datas", "null on save")];

/// ⚠ Visited once an instance, so nothing here clones or allocates per
/// instance: a `.ynv` corpus is tens of millions of them, and a
/// `Pack::type_by_name` is a scan of every type.
struct Walk<'p, 'i> {
    pack: &'p Pack,
    image: Image<'i>,
    census: Census,
    seen: HashSet<(&'p str, usize)>,
    /// Each type named, its description and natural alignment, looked up once.
    types: HashMap<&'p str, Option<(&'p PackType, usize)>>,
}

const DEPTH: usize = 64;

impl<'p> Walk<'p, '_> {
    fn described(&mut self, type_name: &'p str) -> Option<(&'p PackType, usize)> {
        let pack = self.pack;
        *self.types.entry(type_name).or_insert_with(|| {
            pack.type_by_name(type_name)
                .map(|ty| (ty, natural_alignment(pack, type_name)))
        })
    }

    fn visit(&mut self, type_name: &'p str, at: usize, depth: usize) {
        if depth > DEPTH || !self.seen.insert((type_name, at)) {
            return;
        }
        let Some((ty, alignment)) = self.described(type_name) else {
            self.census
                .unreached
                .push(format!("`{type_name}` is not described"));
            return;
        };
        if !self.census.types.contains_key(type_name) {
            self.census
                .types
                .insert(type_name.to_string(), Tally::default());
        }
        let tally = self
            .census
            .types
            .get_mut(type_name)
            .expect("inserted above");
        tally.alignment = alignment;
        tally.instances += 1;
        if !at.is_multiple_of(alignment) {
            tally.misaligned += 1;
            if tally.examples.len() < 8 {
                tally.examples.push(at);
            }
        }
        if let Some(selector) = &ty.selector {
            if let Some(tag) = selector.read_tag(ty, self.image.bytes(), at) {
                *self
                    .census
                    .tags
                    .entry(type_name.to_string())
                    .or_default()
                    .entry(tag)
                    .or_default() += 1;
                match selector.type_for(tag) {
                    Some(variant) => self.visit(variant, at, depth + 1),
                    None => self.census.undeclared.push(format!(
                        "`{type_name}` at {at:#x}: tag {tag} selects no declared variant"
                    )),
                }
            }
        }
        for field in &ty.fields {
            let here = at + field.offset;
            match &field.repr {
                FieldRepr::Inline { type_name } => self.visit(type_name, here, depth + 1),
                FieldRepr::InlineArray { type_name, count } => {
                    let size = self.size_of(type_name);
                    for i in 0..*count {
                        self.visit(type_name, here + i * size, depth + 1);
                    }
                }
                FieldRepr::Ptr(pointee) => {
                    let Some(target) = self.follow(here, &ty.name, &field.name) else {
                        continue;
                    };
                    let n = self.length(ty, at, pointee);
                    self.pointee(pointee, target, n, depth);
                }
                FieldRepr::AtArray(pointee) => {
                    let Some(target) = self.follow(here, &ty.name, &field.name) else {
                        continue;
                    };
                    let n = self.u16_at(here + 8).map(usize::from);
                    self.pointee(pointee, target, n, depth);
                }
                FieldRepr::PtrSlots { pointee, count } => {
                    for i in 0..*count {
                        if let Some(target) = self.follow(here + 8 * i, &ty.name, &field.name) {
                            self.pointee(pointee, target, Some(1), depth);
                        }
                    }
                }
                FieldRepr::Trailing {
                    type_name: Some(element),
                    count,
                    ..
                } => {
                    let n = self.count(ty, at, count).unwrap_or(0);
                    let size = self.size_of(element);
                    for i in 0..n {
                        self.visit(element, at + ty.size + i * size, depth + 1);
                    }
                }
                _ => {}
            }
        }
    }

    fn pointee(&mut self, pointee: &'p Pointee, target: usize, n: Option<usize>, depth: usize) {
        match pointee {
            Pointee::One { type_name } => self.visit(type_name, target, depth + 1),
            Pointee::Many {
                type_name: Some(element),
                ..
            } => {
                let size = self.size_of(element);
                for i in 0..n.unwrap_or(0) {
                    self.visit(element, target + i * size, depth + 1);
                }
            }
            Pointee::ManyPtr {
                type_name: Some(element),
                ..
            } => {
                for i in 0..n.unwrap_or(0) {
                    if let Some(slot) = self.follow(target + 8 * i, element, "<slot>") {
                        self.visit(element, slot, depth + 1);
                    }
                }
            }
            _ => {}
        }
    }

    fn size_of(&mut self, type_name: &'p str) -> usize {
        self.described(type_name).map_or(0, |(t, _)| t.size)
    }

    fn u16_at(&self, at: usize) -> Option<u16> {
        self.image.u16_at(at).ok()
    }

    fn follow(&mut self, at: usize, owner: &str, field: &str) -> Option<usize> {
        let address = self.image.pointer_at(at).ok()?;
        if address == 0 {
            return None;
        }
        match self.image.resolve(address) {
            Ok(offset) => Some(offset),
            Err(or_io::desc::Unresolvable::NotAnAddress { .. }) => {
                self.census.not_addresses += 1;
                None
            }
            Err(why) => {
                self.census
                    .unreached
                    .push(format!("`{owner}.{field}` at {at:#x}: {why}"));
                None
            }
        }
    }

    fn length(&self, ty: &or_io::desc::PackType, at: usize, pointee: &Pointee) -> Option<usize> {
        match pointee {
            Pointee::Many { count, .. } | Pointee::ManyPtr { count, .. } => {
                self.count(ty, at, count)
            }
            _ => Some(1),
        }
    }

    fn count(&self, ty: &or_io::desc::PackType, at: usize, source: &CountSource) -> Option<usize> {
        let raws: Option<Vec<i128>> = source
            .reads_fields()
            .iter()
            .map(|name| {
                let field = ty.field_by_name(name)?;
                let width = match &field.repr {
                    FieldRepr::Scalar(Scalar::Int(w)) | FieldRepr::Enum { width: w, .. } => *w,
                    _ => IntWidth::U32,
                };
                width.read(self.image.bytes(), at + field.offset)
            })
            .collect();
        source.length_from(&raws?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_io::desc::{PackField, PackType};

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

    fn u32_at(name: &str, offset: usize) -> PackField {
        field(name, offset, FieldRepr::Scalar(Scalar::Int(IntWidth::U32)))
    }

    #[test]
    fn a_pad_holding_a_non_zero_byte_is_named() {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Padded".into(),
            size: 0x18,
            fields: vec![
                u32_at("value", 0),
                field("_pad_after_value", 4, FieldRepr::Padding { size: 4 }),
                field("cache", 8, FieldRepr::Ptr(Pointee::Opaque)),
                // 0x10..0x14 is covered by no field.
                u32_at("tail", 0x14),
            ],
            selector: None,
        });
        let found = regions(&pack);
        let labels: Vec<(&str, usize, usize)> = found
            .iter()
            .map(|r| (r.label.as_str(), r.at, r.len))
            .collect();
        assert_eq!(
            labels,
            [
                ("_pad_after_value", 4, 4),
                ("cache", 8, 8),
                ("after cache", 0x10, 4)
            ]
        );

        let mut bytes = vec![0u8; 0x18];
        bytes[5] = 0x7f;
        bytes[0x12] = 1;
        let image = Image::new(&bytes, bytes.len());
        let (_, sites) = walk(&pack, image, "Padded", 0);
        let live = liveness(&found, image, &sites);
        let of = |label: &str| live[&("Padded".to_string(), label.to_string())];
        assert_eq!(
            of("_pad_after_value"),
            Liveness {
                reached: 1,
                non_zero: 1,
                leads_somewhere: 0
            }
        );
        assert_eq!(of("cache").non_zero, 0);
        assert_eq!(of("after cache").non_zero, 1);
    }

    #[test]
    fn a_tag_no_variant_is_declared_for_is_named() {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(PackType {
            name: "Base".into(),
            size: 4,
            fields: vec![u32_at("kind", 0)],
            selector: Some(or_io::desc::PackSelector {
                tag: "kind".into(),
                tag_bits: None,
                variants: vec![(1, "One".into())],
            }),
        });
        pack.types.push(PackType {
            name: "One".into(),
            size: 4,
            fields: vec![field(
                "base",
                0,
                FieldRepr::Inline {
                    type_name: "Base".into(),
                },
            )],
            selector: None,
        });
        let declared = take(&pack, Image::new(&1u32.to_le_bytes(), 4), "Base", 0);
        assert!(declared.undeclared.is_empty());
        assert_eq!(declared.tags["Base"][&1], 1);
        assert_eq!(declared.types["One"].instances, 1);

        let stray = take(&pack, Image::new(&7u32.to_le_bytes(), 4), "Base", 0);
        assert_eq!(
            stray.undeclared,
            ["`Base` at 0x0: tag 7 selects no declared variant"]
        );
    }
}
