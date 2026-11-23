//! The descriptor pack: described types, fields, enums and selectors.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

use crate::desc::plan::{PlanCache, PlanError, Plans, TypePlan};
use crate::desc::repr::{AddressSpace, BitRange, FieldRepr, IntWidth, Scalar};

pub fn prettify_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut capitalise = true;
    for c in name.chars() {
        if c == '_' {
            capitalise = true;
        } else if capitalise {
            out.extend(c.to_uppercase());
            capitalise = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackField {
    pub name: String,

    pub offset: usize,

    pub repr: FieldRepr,

    #[serde(default)]
    pub space: Option<AddressSpace>,

    #[serde(default)]
    pub bits: Vec<BitRange>,

    #[serde(default)]
    pub note: Option<String>,
}

impl PackField {
    pub fn display_name(&self) -> String {
        prettify_name(&self.name)
    }

    pub fn is_physical(&self) -> bool {
        self.space == Some(AddressSpace::Physical)
    }

    pub fn is_bit_packed(&self) -> bool {
        !self.bits.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackSelector {
    pub tag: String,

    /// Which bits of that field are the tag; `None` means the whole field.
    ///
    /// A collision polygon's primitive type is the low three bits of its `area`
    /// **float**, and the rest of the float is the area: 6,176 boxes, 1,050
    /// capsules, 107 cylinders and 86 spheres across 132,911 polygon slots in
    /// the corpus.
    #[serde(default)]
    pub tag_bits: Option<BitRange>,

    pub variants: Vec<(i64, String)>,
}

impl PackSelector {
    pub fn type_for(&self, tag: i64) -> Option<&str> {
        self.variants
            .iter()
            .find(|(value, _)| *value == tag)
            .map(|(_, name)| name.as_str())
    }

    pub fn read_tag(&self, ty: &PackType, bytes: &[u8], at: usize) -> Option<i64> {
        let field = ty.field_by_name(&self.tag)?;

        let size = match &field.repr {
            FieldRepr::Scalar(scalar) => scalar.size(),
            FieldRepr::Enum { width, .. } => width.size(),
            _ => return None,
        };

        let float = matches!(field.repr, FieldRepr::Scalar(Scalar::F32 | Scalar::F64));
        if float && self.tag_bits.is_none() {
            return None;
        }

        let offset = at.checked_add(field.offset)?;
        let slice = bytes.get(offset..offset.checked_add(size)?)?;
        let mut buf = [0u8; 8];
        buf[..size].copy_from_slice(slice);
        let raw = u64::from_le_bytes(buf);

        // ⚠ An unsigned whole-field tag is its value, not its sign-extension:
        // a `ptxBehaviour`'s tag is a `u32` type hash, half of them past 2^31.
        let unsigned = match &field.repr {
            FieldRepr::Scalar(Scalar::Int(w)) | FieldRepr::Enum { width: w, .. } => !w.is_signed(),
            _ => false,
        };
        if unsigned && self.tag_bits.is_none() && size < 8 {
            return Some(raw as i64);
        }
        Some(self.tag_of(raw, size))
    }

    pub fn tag_of(&self, raw: u64, size: usize) -> i64 {
        let Some(range) = &self.tag_bits else {
            let bits = (size * 8) as u32;
            if bits == 0 || bits >= 64 {
                return raw as i64;
            }
            let shift = 64 - bits;
            return ((raw << shift) as i64) >> shift;
        };

        let shifted = raw >> range.low.min(63);
        let mask = if range.width >= 64 {
            u64::MAX
        } else {
            (1u64 << range.width) - 1
        };
        (shifted & mask) as i64
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackType {
    pub name: String,

    pub size: usize,

    pub fields: Vec<PackField>,

    pub selector: Option<PackSelector>,
}

impl PackType {
    pub fn field_by_name(&self, name: &str) -> Option<&PackField> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub fn trailing(&self) -> Option<&PackField> {
        self.fields.iter().find(|f| f.repr.is_trailing())
    }

    pub fn structural_fields(&self) -> BTreeSet<&str> {
        let mut out = BTreeSet::new();
        for field in &self.fields {
            out.extend(field.repr.count_field());
            out.extend(field.repr.capacity_field());
            if let Some(source @ super::CountSource::Sum { .. }) = field.repr.length_source() {
                out.extend(source.reads_fields());
            }
        }
        if let Some(selector) = &self.selector {
            out.insert(selector.tag.as_str());
        }
        out
    }

    pub fn is_structural(&self, field_name: &str) -> bool {
        self.structural_fields().contains(field_name)
    }

    /// Ways this description contradicts itself, in the pack's own terms: two
    /// fields over one byte, or a field running past the end of its type.
    ///
    /// It needs no Rust type, so it is the check left for a target that a
    /// declaration's `offset_of!` mirror does not describe (`layout_target`).
    ///
    /// ⚠ **A coherent description can still be wrong about the file**, and
    /// nothing derived from *one* pack can say otherwise: shifting every field
    /// of a type up by four bytes and growing its size by four passes this and
    /// reads garbage. A round trip cannot see it either, because reading and
    /// writing through the same description cancels: `gen9_roundtrip` stayed
    /// byte-for-byte green under two wrong-but-coherent gen9 descriptions of
    /// `RmcLodGroup`, one of which moved a pointer array. What caught both is
    /// `what_the_gen9_collapse_still_needs.rs::no_shared_type_diverges_in_offsets_alone`,
    /// which compares the two packs; it needs both staged, so it runs only in the
    /// self-hosted `layouts` job (`local-gate.yml`), not in hosted CI.
    ///
    /// A field whose width the pack does not state (an embedded type it does not
    /// carry) is skipped rather than guessed: `audit` already reports the unknown
    /// type, and a guessed width would add a second, wrong finding. A trailing
    /// run begins at `size` and occupies nothing inside the type; its placement
    /// is `or_io::desc::export::audit`'s.
    pub fn incoherences(&self, describe: impl Fn(&str) -> Option<usize>) -> Vec<String> {
        let mut problems = Vec::new();

        // Name, start, end. Only fields whose width the pack states.
        let mut extents: Vec<(&str, usize, usize)> = Vec::new();
        for field in &self.fields {
            if field.repr.is_trailing() {
                continue;
            }
            let Some(width) = field.repr.inline_size().or_else(|| match &field.repr {
                FieldRepr::Inline { type_name } => describe(type_name),
                FieldRepr::InlineArray { type_name, count } => {
                    describe(type_name).map(|size| size * count)
                }
                _ => None,
            }) else {
                continue;
            };
            let end = field.offset.saturating_add(width);
            if end > self.size {
                problems.push(format!(
                    "`{}.{}` is {width:#x} bytes at {:#x} and ends at {end:#x}, past the \
                     {:#x} `{}` claims to be",
                    self.name, field.name, field.offset, self.size, self.name
                ));
            }
            // Zero-width fields cannot overlap anything and are not worth
            // reporting as a collision with their neighbour.
            if width > 0 {
                extents.push((field.name.as_str(), field.offset, end));
            }
        }

        // Sorted by start, so a collision is a comparison with one neighbour.
        extents.sort_unstable_by_key(|(name, start, _)| (*start, *name));
        for pair in extents.windows(2) {
            let [(a, a_start, a_end), (b, b_start, _)] = pair else {
                continue;
            };
            if a_end > b_start {
                problems.push(format!(
                    "`{}.{a}` covers {a_start:#x}..{a_end:#x} and `{}.{b}` starts at \
                     {b_start:#x}, so the two describe the same bytes",
                    self.name, self.name
                ));
            }
        }

        problems
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackVariant {
    pub name: String,
    pub value: i64,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackEnum {
    pub name: String,
    pub width: IntWidth,
    pub variants: Vec<PackVariant>,
}

impl PackEnum {
    pub fn name_of(&self, value: i64) -> Option<&str> {
        self.variants
            .iter()
            .find(|v| v.value == value)
            .map(|v| v.name.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pack {
    pub title: String,

    pub target: String,

    #[serde(default)]
    pub description: String,

    #[serde(default)]
    pub built_by: String,

    pub types: Vec<PackType>,

    #[serde(default)]
    pub enums: Vec<PackEnum>,

    #[serde(skip)]
    plans: PlanCache,

    #[serde(skip)]
    names: NameIndex,
}

/// Each type's position by name, for [`Pack::type_by_name`]. Not serialised,
/// and a clone starts empty, as [`PlanCache`] does.
#[derive(Default)]
struct NameIndex(OnceLock<HashMap<String, usize>>);

impl Clone for NameIndex {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for NameIndex {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for NameIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.get().is_some() {
            "NameIndex(built)"
        } else {
            "NameIndex(empty)"
        })
    }
}

impl Pack {
    pub fn new(title: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            target: target.into(),
            description: String::new(),
            built_by: String::new(),
            types: Vec::new(),
            enums: Vec::new(),
            plans: PlanCache::default(),
            names: NameIndex::default(),
        }
    }

    /// Every type's plan, built on the first ask.
    ///
    /// ⚠ Built once: a pack whose `types` are edited after this keeps the
    /// plans it had. Edit a clone instead — a clone starts with none.
    pub fn plans(&self) -> &Arc<Plans> {
        self.plans.get_or_build(self)
    }

    pub fn plan(&self, type_name: &str) -> Result<&TypePlan, PlanError> {
        self.plans().get(type_name)
    }

    pub fn enum_by_name(&self, name: &str) -> Option<&PackEnum> {
        self.enums.iter().find(|e| e.name == name)
    }

    pub fn id(&self) -> String {
        format!("{}/{}", self.title, self.target)
    }

    /// The first type named `name`, through an index built on the first ask.
    ///
    /// ⚠ The index is never trusted over `types`: a position whose type no
    /// longer carries the name, or a name the index lacks, is answered by a
    /// scan, so a pack edited after the index was built reads as it always did.
    pub fn type_by_name(&self, name: &str) -> Option<&PackType> {
        let index = self.names.0.get_or_init(|| {
            let mut index = HashMap::with_capacity(self.types.len());
            for (i, ty) in self.types.iter().enumerate() {
                index.entry(ty.name.clone()).or_insert(i);
            }
            index
        });
        index
            .get(name)
            .and_then(|&i| self.types.get(i))
            .filter(|ty| ty.name == name)
            .or_else(|| self.types.iter().find(|t| t.name == name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desc::repr::{CountSource, IntWidth, Pointee, Scalar};

    #[test]
    fn a_type_found_by_name_is_the_one_a_scan_finds_after_the_types_change() {
        let named = |name: &str, size: usize| PackType {
            name: name.into(),
            size,
            fields: Vec::new(),
            selector: None,
        };
        let mut pack = Pack::new("probe", "gen8");
        pack.types.extend([named("A", 1), named("B", 2)]);
        assert_eq!(pack.type_by_name("B").map(|t| t.size), Some(2));

        // The index was built above; every edit below leaves it stale.
        pack.types.insert(0, named("C", 3));
        pack.types[2].name = "D".into();
        pack.types.push(named("A", 4));
        for name in ["A", "B", "C", "D", "E"] {
            assert_eq!(
                pack.type_by_name(name).map(|t| t.size),
                pack.types.iter().find(|t| t.name == name).map(|t| t.size),
                "{name}"
            );
        }
    }

    #[test]
    fn prettifying_capitalises_each_underscore_separated_word() {
        assert_eq!(prettify_name("vertex_count"), "VertexCount");
        assert_eq!(prettify_name("bone_index_count"), "BoneIndexCount");
        assert_eq!(prettify_name("data"), "Data");
        assert_eq!(prettify_name("unknown_78"), "Unknown78");
    }

    #[test]
    fn prettifying_a_name_that_is_already_capitalised_changes_nothing() {
        assert_eq!(prettify_name("VertexBuffer"), "VertexBuffer");
        assert_eq!(prettify_name("Geometry"), "Geometry");
    }

    #[test]
    fn a_field_displays_as_its_declared_name_title_cased() {
        let field = PackField {
            name: "vertex_count".into(),
            offset: 0,
            repr: FieldRepr::Scalar(Scalar::Int(IntWidth::U16)),
            space: None,
            bits: Vec::new(),
            note: None,
        };
        assert_eq!(field.name, "vertex_count");
        assert_eq!(field.display_name(), "VertexCount");
    }

    fn sample() -> Pack {
        let mut pack = Pack::new("gta5", "gen8");
        pack.types.push(PackType {
            name: "GrmGeometry".into(),
            size: 0x98,
            fields: vec![PackField {
                name: "vertex_count".into(),
                offset: 0x60,
                repr: FieldRepr::Scalar(Scalar::Int(IntWidth::U16)),
                space: None,
                bits: Vec::new(),
                note: None,
            }],
            selector: None,
        });
        pack
    }

    #[test]
    fn a_pack_is_looked_up_by_name() {
        let pack = sample();

        let ty = pack.type_by_name("GrmGeometry").expect("GrmGeometry");
        let field = ty.field_by_name("vertex_count").expect("vertex_count");

        assert_eq!(field.offset, 0x60);
        assert_eq!(field.display_name(), "VertexCount");
    }

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

    fn geometry_shaped() -> PackType {
        PackType {
            name: "Probe".into(),
            size: 0x40,
            fields: vec![
                field(
                    "vertices",
                    0,
                    FieldRepr::Ptr(Pointee::Many {
                        type_name: Some("Vertex".into()),
                        count: CountSource::CountAndCapacity {
                            count: "num_vertices".into(),
                            capacity: "max_vertices".into(),
                        },
                        element_size: None,
                    }),
                ),
                field(
                    "num_vertices",
                    8,
                    FieldRepr::Scalar(Scalar::Int(IntWidth::U16)),
                ),
                field(
                    "max_vertices",
                    10,
                    FieldRepr::Scalar(Scalar::Int(IntWidth::U16)),
                ),
                field("margin", 12, FieldRepr::Scalar(Scalar::F32)),
            ],
            selector: None,
        }
    }

    #[test]
    fn a_count_and_its_capacity_are_both_structural() {
        let ty = geometry_shaped();
        let structural = ty.structural_fields();

        assert!(structural.contains("num_vertices"));
        assert!(structural.contains("max_vertices"));
        assert!(ty.is_structural("num_vertices"));
    }

    #[test]
    fn an_ordinary_scalar_beside_them_is_not() {
        let ty = geometry_shaped();
        assert!(!ty.is_structural("margin"));
        assert!(!ty.structural_fields().contains("margin"));
    }

    #[test]
    fn a_tagged_bases_selector_is_structural() {
        let ty = PackType {
            name: "PhBound".into(),
            size: 0x70,
            fields: vec![
                field(
                    "bound_type",
                    0,
                    FieldRepr::Scalar(Scalar::Int(IntWidth::U8)),
                ),
                field("radius", 4, FieldRepr::Scalar(Scalar::F32)),
            ],
            selector: Some(PackSelector {
                tag: "bound_type".into(),
                tag_bits: None,
                variants: vec![(0, "PhBoundSphere".into())],
            }),
        };

        assert!(ty.is_structural("bound_type"));
        assert!(!ty.is_structural("radius"));
    }

    #[test]
    fn a_type_with_no_arrays_and_no_tag_has_nothing_structural() {
        let ty = PackType {
            name: "Plain".into(),
            size: 8,
            fields: vec![field("a", 0, FieldRepr::Scalar(Scalar::F32))],
            selector: None,
        };
        assert!(ty.structural_fields().is_empty());
    }

    /// Nothing describes anything, so nothing can be sized.
    fn nothing_is_described(_: &str) -> Option<usize> {
        None
    }

    #[test]
    fn a_description_whose_fields_do_not_touch_is_coherent() {
        let ty = PackType {
            name: "Plain".into(),
            size: 0x10,
            fields: vec![
                field("a", 0x00, FieldRepr::Scalar(Scalar::Int(IntWidth::U32))),
                field("b", 0x04, FieldRepr::Scalar(Scalar::F32)),
                field("c", 0x08, FieldRepr::Ptr(Pointee::Str)),
            ],
            selector: None,
        };
        assert!(ty.incoherences(nothing_is_described).is_empty());
    }

    #[test]
    fn two_fields_over_the_same_byte_are_reported_by_both_names() {
        // The shape a mistyped offset takes: a pointer at 0x09 lies over
        // `stride`, and both still read as plausible numbers.
        let ty = PackType {
            name: "GrcVertexBuffer".into(),
            size: 0x18,
            fields: vec![
                field(
                    "stride",
                    0x08,
                    FieldRepr::Scalar(Scalar::Int(IntWidth::U16)),
                ),
                field(
                    "data",
                    0x09,
                    FieldRepr::Ptr(Pointee::Bytes {
                        length: CountSource::Unknown,
                    }),
                ),
            ],
            selector: None,
        };

        let problems = ty.incoherences(nothing_is_described);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("stride"), "{problems:?}");
        assert!(problems[0].contains("data"), "{problems:?}");
    }

    #[test]
    fn a_field_running_past_the_end_of_its_type_is_reported() {
        // Distinct from the audit's `offset >= size`, which this type passes:
        // the field *starts* inside and ends outside, so only its width says
        // so. A dropped digit in a size lands here.
        let ty = PackType {
            name: "GrcIndexBuffer".into(),
            size: 0x18,
            fields: vec![field(
                "indices",
                0x14,
                FieldRepr::Ptr(Pointee::Bytes {
                    length: CountSource::Unknown,
                }),
            )],
            selector: None,
        };

        let problems = ty.incoherences(nothing_is_described);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("indices"), "{problems:?}");
        assert!(problems[0].contains("0x1c"), "{problems:?}");
    }

    #[test]
    fn a_trailing_run_neither_overflows_nor_collides() {
        // It begins at `size` and occupies no bytes inside the struct: the one
        // shape that is supposed to sit at the boundary.
        let ty = PackType {
            name: "FragTransforms".into(),
            size: 0x20,
            fields: vec![
                field("count", 0x10, FieldRepr::Scalar(Scalar::Int(IntWidth::U16))),
                field(
                    "matrices",
                    0x20,
                    FieldRepr::Trailing {
                        type_name: None,
                        count: CountSource::Field("count".into()),
                        element_size: Some(0x30),
                    },
                ),
            ],
            selector: None,
        };
        assert!(ty.incoherences(nothing_is_described).is_empty());
    }

    #[test]
    fn an_embedded_type_is_measured_through_the_pack_rather_than_skipped() {
        // `inline_size()` cannot answer for a struct stored by value — only the
        // pack knows how big it is.
        let ty = PackType {
            name: "Outer".into(),
            size: 0x20,
            fields: vec![
                field(
                    "inner",
                    0x00,
                    FieldRepr::Inline {
                        type_name: "Inner".into(),
                    },
                ),
                field("after", 0x08, FieldRepr::Scalar(Scalar::Int(IntWidth::U32))),
            ],
            selector: None,
        };

        let sized = |name: &str| (name == "Inner").then_some(0x10usize);
        let problems = ty.incoherences(sized);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("inner"), "{problems:?}");
        assert!(problems[0].contains("after"), "{problems:?}");

        // Clean when nobody can size `Inner`: `audit` reports the missing type.
        assert!(ty.incoherences(nothing_is_described).is_empty());
    }

    /// ⚠ The limit: every field is four bytes further along than the file puts
    /// it and the size has grown to match, so the description is coherent and
    /// reads garbage. [`PackType::incoherences`] says what does catch it.
    #[test]
    fn a_description_shifted_wholesale_is_coherent_and_wrong() {
        let shifted = PackType {
            name: "GrmGeometry".into(),
            size: 0x1C,
            fields: vec![
                field("a", 0x04, FieldRepr::Scalar(Scalar::Int(IntWidth::U32))),
                field("b", 0x08, FieldRepr::Scalar(Scalar::F32)),
                field("c", 0x0C, FieldRepr::Ptr(Pointee::Str)),
            ],
            selector: None,
        };
        assert!(shifted.incoherences(nothing_is_described).is_empty());
    }
}
