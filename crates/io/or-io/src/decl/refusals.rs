//! Declarations `#[record]` refuses at compile time. Each block below is a
//! doctest that must fail to build, pinned to its error code where the
//! compiler gives one, so a block failing for some other reason is caught.
//!
//! Offsets are never written: `repr(C)` computes them from field order and
//! natural alignment. What a declaration states is the engine's size, and any padding the engine's layout has.
//!
//! The raw form, written in on-disk types, which the owned form replaced:
//!
//! ```compile_fail
//! use or_io::decl::*;
//!
//! #[record(raw, size = 0x04)]
//! pub struct Sample {
//!     pub width: u32,
//! }
//! ```
//!
//! The accepted shape, which compiles:
//!
//! ```
//! use or_io::decl::*;
//!
//! #[record(size = 0x10)]
//! pub struct Sample {
//!     pub width: u16,
//!     pub height: u16,
//!     pub flags: u32,
//!     pub data: Option<Box<Sample>>,
//! }
//! assert_eq!(std::mem::offset_of!(Raw<Sample>, data), 0x08);
//! assert_eq!(std::mem::size_of::<Raw<Sample>>(), 0x10);
//! ```
//!
//! Padding the compiler would insert, declared with `#[pad_after]`:
//!
//! ```
//! use or_io::decl::*;
//!
//! #[record(size = 0x10)]
//! pub struct Sample {
//!     #[pad_after(6)]
//!     pub width: u16,
//!     pub data: Option<Box<Sample>>,
//! }
//! assert_eq!(std::mem::offset_of!(Raw<Sample>, data), 0x08);
//! ```
//!
//! A gap nobody declared, which the compiler would otherwise fill silently:
//!
//! ```compile_fail,E0080
//! use or_io::decl::*;
//!
//! #[record(size = 0x10)]
//! pub struct Sample {
//!     pub width: u16,
//!     pub data: Option<Box<Sample>>,
//! }
//! ```
//!
//! A computed size that is not the engine's:
//!
//! ```compile_fail,E0080
//! use or_io::decl::*;
//!
//! #[record(size = 0x18)]
//! pub struct Sample {
//!     pub width: u32,
//!     pub height: u32,
//!     pub data: Option<Box<Sample>>,
//! }
//! ```
//!
//! An optional `#[at]` cross-check the computed layout disagrees with:
//!
//! ```compile_fail,E0080
//! use or_io::decl::*;
//!
//! #[record(size = 0x10)]
//! pub struct Sample {
//!     pub width: u16,
//!     #[at(0x04)] pub height: u16,
//!     pub flags: u32,
//!     pub data: Option<Box<Sample>>,
//! }
//! ```
//!
//! A type not valid for every bit pattern — a `bool`:
//!
//! ```compile_fail,E0277
//! use or_io::decl::*;
//!
//! #[record(size = 0x08)]
//! pub struct Sample {
//!     #[pad_after(7)]
//!     pub set: bool,
//! }
//! ```
//!
//! A type parameter held by value, so its offset would depend on it:
//!
//! ```compile_fail
//! use or_io::decl::*;
//!
//! #[record(size = 0x08)]
//! pub struct Sample<V> {
//!     pub value: V,
//! }
//! ```
//!
//! An owned record over a target holds what the target lays out its own way
//! behind a pointer:
//!
//! ```
//! use or_io::decl::*;
//!
//! pub trait Side: TargetMarker {
//!     type Sheet: Owned<Raw: Record> + Default + std::fmt::Debug + Clone + PartialEq;
//! }
//!
//! #[record(size = 0x04)]
//! pub struct Sheet {
//!     pub width: u32,
//! }
//!
//! impl Side for Gen8 {
//!     type Sheet = Sheet;
//! }
//!
//! #[record(size = 0x08)]
//! pub struct Sample<T: Side = Gen8> {
//!     pub sheet: Option<Box<T::Sheet>>,
//! }
//! ```
//!
//! and never by value, where its offset would depend on the target:
//!
//! ```compile_fail
//! use or_io::decl::*;
//!
//! pub trait Side: TargetMarker {
//!     type Sheet: Owned<Raw: Record> + Default + std::fmt::Debug + Clone + PartialEq;
//! }
//!
//! #[record(size = 0x04)]
//! pub struct Sheet {
//!     pub width: u32,
//! }
//!
//! impl Side for Gen8 {
//!     type Sheet = Sheet;
//! }
//!
//! #[record(size = 0x04)]
//! pub struct Sample<T: Side = Gen8> {
//!     pub sheet: T::Sheet,
//! }
//! ```
//!
//! The owned form: domain types, and offsets the compiler computes.
//!
//! ```
//! use or_io::decl::*;
//!
//! #[record(size = 0x28)]
//! pub struct Sample {
//!     pub name: String,
//!     pub items: Vec<u32>,
//!     pub next: Option<Box<Sample>>,
//!     #[pad_after(4)]
//!     pub count: u32,
//! }
//!
//! let s = Sample { name: "a".into(), items: vec![1, 2], ..Default::default() };
//! assert_eq!(std::mem::size_of::<Raw<Sample>>(), 0x28);
//! assert_eq!(std::mem::offset_of!(Raw<Sample>, count), 0x20);
//! assert_eq!(s.items.len(), 2);
//! ```
//!
//! A field named for what nobody knows:
//!
//! ```compile_fail
//! use or_io::decl::*;
//!
//! #[record(size = 0x04)]
//! pub struct Sample {
//!     unknown_0x00: u32,
//! }
//! ```
//!
//! ```compile_fail
//! use or_io::decl::*;
//!
//! #[record(size = 0x04)]
//! pub struct Sample {
//!     pub unk_4: u32,
//! }
//! ```
//!
//! A run that does not say where its length is:
//!
//! ```compile_fail
//! use or_io::decl::*;
//!
//! #[record(size = 0x10)]
//! pub struct Sample {
//!     #[phys]
//!     pub items: Vec<u32>,
//!     #[pad_after(4)]
//!     pub count: u32,
//! }
//! ```
//!
//! Named ranges of a field's bits, described on the field, which a patch then refuses whole:
//!
//! ```
//! use or_io::decl::*;
//!
//! #[record(size = 0x04)]
//! pub struct Sample {
//!     #[bits(index, 0, 24)]
//!     #[bits(kind, 24, 8, "what the index counts")]
//!     pub word: u32,
//! }
//! ```
//!
//! A range that ends past the field's width:
//!
//! ```compile_fail,E0080
//! use or_io::decl::*;
//!
//! #[record(size = 0x04)]
//! pub struct Sample {
//!     #[bits(index, 0, 24)]
//!     #[bits(kind, 24, 16)]
//!     pub word: u32,
//! }
//! ```
//!
//! Two ranges over the same bits:
//!
//! ```compile_fail
//! use or_io::decl::*;
//!
//! #[record(size = 0x04)]
//! pub struct Sample {
//!     #[bits(index, 0, 24)]
//!     #[bits(kind, 20, 8)]
//!     pub word: u32,
//! }
//! ```
//!
//! A reference count an owned value shows, where a save derives it:
//!
//! ```compile_fail
//! use or_io::decl::*;
//!
//! #[record(size = 0x04)]
//! pub struct Sample {
//!     #[refs]
//!     pub ref_count: u32,
//! }
//! ```
//!
//! An inline union, owned as the arm its sibling field selects:
//!
//! ```
//! use or_io::decl::*;
//!
//! #[record(size = 0x04)]
//! pub struct Arm {
//!     pub value: u32,
//! }
//!
//! #[record(union, size = 0x04)]
//! pub enum Either {
//!     Arm(Arm),
//! }
//!
//! #[record(size = 0x08)]
//! pub struct Sample {
//!     pub kind: u32,
//!     #[select(kind = 1 => Arm)]
//!     pub body: Either,
//! }
//! ```
//!
//! and never by a name that is neither a field nor a bit range:
//!
//! ```compile_fail
//! use or_io::decl::*;
//!
//! #[record(size = 0x04)]
//! pub struct Arm {
//!     pub value: u32,
//! }
//!
//! #[record(union, size = 0x04)]
//! pub enum Either {
//!     Arm(Arm),
//! }
//!
//! #[record(size = 0x08)]
//! pub struct Sample {
//!     pub kind: u32,
//!     #[select(sort = 1 => Arm)]
//!     pub body: Either,
//! }
//! ```
//!
//! A run of pointers keyed by one key rather than a key each:
//!
//! ```compile_fail
//! use or_io::decl::*;
//!
//! #[record(size = 0x08)]
//! pub struct Tile {
//!     pub width: u32,
//!     pub height: u32,
//! }
//!
//! #[record(size = 0x18)]
//! pub struct Rack {
//!     #[pad_after(4)]
//!     pub code: u32,
//!     #[keyed_by(code)]
//!     pub entries: Vec<Option<Box<Tile>>>,
//! }
//! ```
//!
//! A `#[shares]` path through an embedded record to a field it does not have:
//!
//! ```compile_fail,E0609
//! use or_io::decl::*;
//!
//! #[record(size = 0x08)]
//! pub struct Tile {
//!     pub width: u32,
//!     pub height: u32,
//! }
//!
//! #[record(size = 0x08)]
//! pub struct Inner {
//!     pub tile: Option<Box<Tile>>,
//! }
//!
//! #[record(size = 0x10)]
//! pub struct Outer {
//!     pub inner: Inner,
//!     #[shares(inner.tiles)]
//!     pub again: Shared<Option<Box<Tile>>>,
//! }
//! ```
//!
//! A reference to a run, whose count a path does not state:
//!
//! ```compile_fail,E0277
//! use or_io::decl::*;
//!
//! #[record(size = 0x10)]
//! pub struct Strip {
//!     pub bytes: Ref<Vec<u8>>,
//! }
//! ```
//!
//! A pointer given a constructor — there is none, because the writer owns addresses:
//!
//! ```compile_fail,E0451
//! use or_io::decl::*;
//! let _ = Ptr::<u32> { raw: 0x5000_0000, _t: std::marker::PhantomData };
//! ```
//!
//! A count assigned on an owned value, which has no count field because a save derives it:
//!
//! ```compile_fail,E0609
//! use or_io::gta5::path_region::CPathRegion;
//! let mut region = CPathRegion::default();
//! region.num_nodes = 5;
//! ```
//!
//! ```compile_fail,E0609
//! use or_io::gta5::bounds::PhBoundGeometry;
//! let mut geometry = PhBoundGeometry::default();
//! geometry.num_vertices = 5;
//! ```
//!
//! A block count assigned on an owned Enhanced texture, which a save derives from its pixels:
//!
//! ```compile_fail,E0609
//! use or_io::gta5::gen9::GrcTextureG9;
//! let mut texture = GrcTextureG9::default();
//! texture.block_count = 5;
//! ```
//!
//! An entry count assigned on an owned RPF7 header, which the archive's table derives:
//!
//! ```compile_fail,E0609
//! use or_io::container::rpf::FiPackHeader;
//! let mut header = FiPackHeader::default();
//! header.entry_count = 5;
//! ```

#[cfg(test)]
mod tests {
    use crate::container::resource::DatResourceFileHeader;
    use crate::container::rpf::{FiPackEntry, FiPackHeader};
    use crate::decl::{root, Raw, Reach};
    use crate::desc::{CountSource, FieldRepr, PackType, Pointee};
    use crate::gta5::formats::FORMATS;
    use crate::gta5::shared::{
        AtBitSet, AtString, DatBase, Mat33V, Mat34V, PgBase, PgBaseRefCounted, QuatV, SpdAABB,
        SpdSphere, Vec3V, Vec4V, Vector3,
    };
    use crate::gta5::text::{CGxt2Entry, CGxt2Header};

    /// Every field of `ty` a length names, the capacity of a counted pair included.
    fn counts(ty: &PackType) -> Vec<&str> {
        let mut out = Vec::new();
        for f in &ty.fields {
            let source = match &f.repr {
                FieldRepr::Ptr(
                    Pointee::Many { count, .. }
                    | Pointee::ManyPtr { count, .. }
                    | Pointee::Members { count, .. }
                    | Pointee::Strings { count },
                )
                | FieldRepr::Trailing { count, .. } => count,
                FieldRepr::Ptr(Pointee::Bytes { length }) => length,
                _ => continue,
            };
            match source {
                // A texture's dimensions imply its length and are its own to show.
                CountSource::MaskedOrMipChain { field, .. } => out.push(field.as_str()),
                _ => out.extend(source.reads_fields()),
            }
            if let CountSource::CountAndCapacity { capacity, .. } = source {
                out.push(capacity);
            }
        }
        out
    }

    /// What an owned field shows that the owned form keeps to itself, if anything.
    fn shows(ty: &PackType, field: &str, address: bool) -> Option<&'static str> {
        let name = field.trim_start_matches('_');
        if address {
            Some("a pointer")
        } else if name == "vft" || name == "vtable" {
            Some("a vft")
        } else if name == "ref_count" {
            Some("a reference count")
        } else if name == "pad" || name.starts_with("padding") || name.starts_with("pad_") {
            Some("padding")
        } else if counts(ty).contains(&field) {
            Some("a count")
        } else {
            None
        }
    }

    fn offenders(reach: &Reach, out: &mut std::collections::BTreeSet<String>) {
        for ty in reach.types() {
            let Some(public) = reach.owned_public(&ty.name) else {
                continue;
            };
            for (field, address) in public {
                if let Some(what) = shows(ty, field, *address) {
                    out.insert(format!("{}.{field} is {what}", ty.name));
                }
            }
        }
    }

    /// Padding, counts, reference counts, vft words and pointers are declared
    /// and described, and an owned value does not show them.
    #[test]
    fn no_owned_record_shows_padding_a_count_a_reference_count_a_vft_or_a_pointer() {
        let mut found = std::collections::BTreeSet::new();
        for format in FORMATS {
            for target in format.targets() {
                offenders(&format.reach(target).expect("reaches"), &mut found);
            }
        }
        for (ext, root) in [
            ("ybn", "PhBoundGeometry"),
            ("ybd", "PgDictionary<PhBound>"),
            ("yld", "PgDictionary<CharacterCloth>"),
        ] {
            let reach = crate::gta5::formats::format(ext)
                .and_then(|f| f.reach(crate::Generation::Gen8).ok())
                .expect("reaches");
            assert!(reach.owned_public(root).is_some(), "`{root}` is owned");
        }
        let enhanced_ytd = FORMATS.iter().find(|f| f.ext == "ytd").expect("declared");
        assert!(
            enhanced_ytd
                .reach(crate::Generation::Gen9)
                .expect("reaches")
                .owned_public("GrcTextureG9")
                .is_some(),
            "the Enhanced texture is owned"
        );
        let mut shared = Reach::new("gen8");
        for r in [
            root::<Raw<PgBase>>,
            root::<Raw<DatBase>>,
            root::<Raw<PgBaseRefCounted>>,
            root::<Raw<AtBitSet>>,
            root::<Raw<AtString>>,
            root::<Raw<Vec3V>>,
            root::<Raw<Vec4V>>,
            root::<Raw<Vector3>>,
            root::<Raw<QuatV>>,
            root::<Raw<Mat33V>>,
            root::<Raw<Mat34V>>,
            root::<Raw<SpdSphere>>,
            root::<Raw<SpdAABB>>,
            root::<Raw<DatResourceFileHeader>>,
            root::<Raw<FiPackHeader>>,
            root::<Raw<FiPackEntry>>,
            root::<Raw<CGxt2Header>>,
            root::<Raw<CGxt2Entry>>,
        ] {
            r(&mut shared);
        }
        assert!(
            shared.owned_public("PgBase").is_some(),
            "the shared primitives are owned"
        );
        assert!(
            shared.owned_public("DatResourceFileHeader").is_some(),
            "the RSC7 header is owned"
        );
        assert!(
            shared.owned_public("FiPackHeader").is_some()
                && shared.owned_public("FiPackEntry").is_some(),
            "the RPF7 header and entry are owned"
        );
        assert!(
            shared.owned_public("CGxt2Header").is_some()
                && shared.owned_public("CGxt2Entry").is_some(),
            "the GXT2 header and entry are owned"
        );
        offenders(&shared, &mut found);
        assert!(found.is_empty(), "{found:#?}");
    }
}
