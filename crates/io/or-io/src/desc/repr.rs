use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntWidth {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
}

impl IntWidth {
    pub const fn size(self) -> usize {
        match self {
            IntWidth::U8 | IntWidth::I8 => 1,
            IntWidth::U16 | IntWidth::I16 => 2,
            IntWidth::U32 | IntWidth::I32 => 4,
            IntWidth::U64 | IntWidth::I64 => 8,
        }
    }

    pub const fn is_signed(self) -> bool {
        matches!(
            self,
            IntWidth::I8 | IntWidth::I16 | IntWidth::I32 | IntWidth::I64
        )
    }

    pub fn read(self, bytes: &[u8], offset: usize) -> Option<i128> {
        let size = self.size();
        let slice = bytes.get(offset..offset.checked_add(size)?)?;

        let mut buf = [0u8; 8];
        buf[..size].copy_from_slice(slice);
        let raw = u64::from_le_bytes(buf);

        Some(if self.is_signed() {
            let shift = 64 - (size * 8);
            ((raw << shift) as i64 >> shift) as i128
        } else {
            raw as i128
        })
    }

    pub const fn holds(self, value: i128) -> bool {
        let bits = (self.size() * 8) as u32;
        if self.is_signed() {
            let limit = 1i128 << (bits - 1);
            value >= -limit && value < limit
        } else {
            value >= 0 && value < (1i128 << bits)
        }
    }

    pub fn write(self, bytes: &mut [u8], offset: usize, value: i128) -> bool {
        if !self.holds(value) {
            return false;
        }
        let size = self.size();
        let Some(end) = offset.checked_add(size) else {
            return false;
        };
        let Some(slot) = bytes.get_mut(offset..end) else {
            return false;
        };
        let raw = (value as u64).to_le_bytes();
        slot.copy_from_slice(&raw[..size]);
        true
    }
}

/// Which heap a pointer's storage belongs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AddressSpace {
    // System memory: structs, arrays, strings. `0x50000000` and up.
    Virtual,
    // Graphics memory: vertex and index buffers, pixels. `0x60000000` and up.
    Physical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BitRange {
    pub name: String,
    pub low: u32,
    pub width: u32,
    #[serde(default)]
    pub note: Option<String>,
}

impl BitRange {
    pub fn mask(&self) -> u64 {
        if self.width >= 64 {
            return u64::MAX;
        }
        (((1u64 << self.width) - 1) as u128 as u64) << self.low.min(63)
    }

    pub fn end(&self) -> u32 {
        self.low.saturating_add(self.width)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scalar {
    Int(IntWidth),
    F32,
    F64,
    Bool,
}

impl Scalar {
    pub const fn size(self) -> usize {
        match self {
            Scalar::Int(w) => w.size(),
            Scalar::F32 => 4,
            Scalar::F64 => 8,
            Scalar::Bool => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CountSource {
    Field(String),

    CountAndCapacity {
        count: String,
        capacity: String,
    },

    MaskedField {
        field: String,
        mask: u64,
    },

    Fixed(usize),
    Inline,
    Unknown,

    Product {
        count: String,
        stride: String,
    },

    /// `count` entries, preceded by one more when `count` is above one: RAGE
    /// puts the union of a run of per-element boxes in front of them only once
    /// there is more than one to union.
    ///
    /// ⚠ [`CountSource::Field`] on such a field describes the array one entry
    /// short, and the loss is on the *write* path: a rebuild writes the shorter
    /// run while the reader asks its own rule for the count, so the last
    /// element reads back as whatever followed it in the heap.
    PrefixedAboveOne {
        count: String,
    },

    /// The sum of sibling fields: a run made of several back-to-back parts,
    /// each counted by its own field and none of them the whole — an
    /// expression program's three operand and opcode runs after its header.
    Sum {
        fields: Vec<String>,
    },

    /// `field` masked, or where the mask leaves zero, the bytes of the mip
    /// chain the sibling `format`, `width`, `height`, `depth`, `mip_count`,
    /// `cut_mip_levels` and `layer_count` imply: a Legacy texture whose
    /// producer left the word unfilled beside live pixels.
    ///
    /// ⚠ Built-in descriptions only (OD-PACKVER): no `.orpack` reader older
    /// than this variant knows it.
    MaskedOrMipChain {
        field: String,
        mask: u64,
    },
}

/// The siblings [`CountSource::MaskedOrMipChain`] reads after its masked field, in order.
pub const MIP_CHAIN_FIELDS: [&str; 7] = [
    "format",
    "width",
    "height",
    "depth",
    "mip_count",
    "cut_mip_levels",
    "layer_count",
];

impl CountSource {
    /// The rule [`CountSource::PrefixedAboveOne`] states, as a function.
    ///
    /// ⚠ **The one statement of it.** `codec::mesh::aabb_slots` calls
    /// this rather than restating it: a reader and a pack that disagree about
    /// an array's length produce a file that opens and saves short.
    pub const fn prefixed_above_one(count: usize) -> usize {
        if count > 1 {
            count + 1
        } else {
            count
        }
    }

    /// The bytes a texture's stored mip chain holds; `layer_count` is the raw field, one less than the layers.
    ///
    /// ⚠ `GrcTextureFormat::calculate_physical_size` over the levels the cut
    /// leaves, summed in `u64` so a hostile header cannot overflow it.
    pub fn mip_chain_len(
        format: u32,
        width: u32,
        height: u32,
        depth: u32,
        mip_count: u32,
        cut_mip_levels: u32,
        layer_count: u32,
    ) -> usize {
        let format = crate::codec::texture::GrcTextureFormat::from(format);
        let (block, per_block) = (
            u64::from(format.block_size().max(1)),
            u64::from(format.bytes_per_block()),
        );
        let cut = cut_mip_levels.min(31);
        let (mut w, mut h) = (
            u64::from((width.max(1) >> cut).max(1)),
            u64::from((height.max(1) >> cut).max(1)),
        );
        let volume = depth > 1;
        let mut d = u64::from(if volume { (depth >> cut).max(1) } else { 1 });
        let mut total = 0u64;
        for _ in 0..mip_count.saturating_sub(cut_mip_levels) {
            let slice = w.div_ceil(block).max(1) * h.div_ceil(block).max(1) * per_block;
            total = total.saturating_add(slice.saturating_mul(d));
            w = (w / 2).max(1);
            h = (h / 2).max(1);
            if volume {
                d = (d / 2).max(1);
            }
        }
        let layers = u64::from(layer_count) + 1;
        usize::try_from(total.saturating_mul(layers)).unwrap_or(usize::MAX)
    }

    pub fn count_field(&self) -> Option<&str> {
        match self {
            CountSource::Field(name)
            | CountSource::CountAndCapacity { count: name, .. }
            | CountSource::MaskedField { field: name, .. }
            | CountSource::MaskedOrMipChain { field: name, .. }
            | CountSource::Product { count: name, .. }
            | CountSource::PrefixedAboveOne { count: name } => Some(name),
            _ => None,
        }
    }

    pub fn stride_field(&self) -> Option<&str> {
        match self {
            CountSource::Product { stride, .. } => Some(stride),
            _ => None,
        }
    }

    pub fn reads_fields(&self) -> Vec<&str> {
        match self {
            CountSource::Product { count, stride } => vec![count.as_str(), stride.as_str()],
            CountSource::Sum { fields } => fields.iter().map(String::as_str).collect(),
            CountSource::MaskedOrMipChain { field, .. } => std::iter::once(field.as_str())
                .chain(MIP_CHAIN_FIELDS)
                .collect(),
            _ => self.count_field().into_iter().collect(),
        }
    }

    /// [`length_from`](Self::length_from) for a run with `room` bytes after its pointer: a
    /// [`CountSource::MaskedOrMipChain`] word that states more than that is a producer's mistake, and
    /// the chain the texture's own dimensions imply is the length when that fits, else the most of its
    /// leading levels that do.
    pub fn length_within(&self, raws: &[i128], room: usize) -> Option<usize> {
        let stated = self.length_from(raws)?;
        if stated <= room {
            return Some(stated);
        }
        match self {
            CountSource::MaskedOrMipChain { mask, .. } => {
                let mut unstated = raws.to_vec();
                *unstated.first_mut()? &= !i128::from(*mask);
                self.length_from(&unstated)
                    .filter(|chain| *chain <= room)
                    .or_else(|| Self::leading_levels_within(&unstated, room))
                    .or(Some(stated))
            }
            _ => Some(stated),
        }
    }

    /// The bytes of the most whole leading levels of the mip chain `raws` state that fit in `room`; `None` where not even the first does.
    ///
    /// ⚠ Files that state a chain over pixels holding only its first levels exist, so the reader takes the
    /// levels the file holds and leaves the header as the file states it.
    fn leading_levels_within(raws: &[i128], room: usize) -> Option<usize> {
        let [_, format, width, height, depth, mips, cut, layers] = raws else {
            return None;
        };
        let u = |v: &i128| (*v).clamp(0, i128::from(u32::MAX)) as u32;
        // No chain has more than 32 levels, and a hostile header must not make this loop for billions.
        let (mips, cut) = (u(mips).min(32), u(cut).min(32));
        (cut + 1..mips).rev().find_map(|levels| {
            let len = Self::mip_chain_len(
                u(format),
                u(width),
                u(height),
                u(depth),
                levels,
                cut,
                u(layers),
            );
            (len <= room).then_some(len)
        })
    }

    pub fn length_from(&self, raws: &[i128]) -> Option<usize> {
        match self {
            CountSource::Fixed(n) => Some(*n),
            CountSource::Inline | CountSource::Unknown => None,

            // A file's negative count must not become an enormous allocation.
            CountSource::Product { .. } => {
                let [count, stride] = raws else { return None };
                let (count, stride) = ((*count).max(0) as u128, (*stride).max(0) as u128);
                Some(count.saturating_mul(stride).min(usize::MAX as u128) as usize)
            }
            CountSource::PrefixedAboveOne { .. } => {
                let [raw] = raws else { return None };
                let count = (*raw).max(0).min(usize::MAX as i128) as usize;
                Some(Self::prefixed_above_one(count))
            }
            CountSource::Sum { fields } => {
                if raws.len() != fields.len() {
                    return None;
                }
                let total = raws.iter().map(|r| (*r).max(0) as u128).sum::<u128>();
                Some(total.min(usize::MAX as u128) as usize)
            }
            CountSource::MaskedOrMipChain { mask, .. } => {
                let [stated, format, width, height, depth, mips, cut, layers] = raws else {
                    return None;
                };
                let masked = ((*stated).max(0) as u64) & mask;
                if masked != 0 {
                    return Some(masked as usize);
                }
                let u = |v: &i128| (*v).clamp(0, i128::from(u32::MAX)) as u32;
                Some(Self::mip_chain_len(
                    u(format),
                    u(width),
                    u(height),
                    u(depth),
                    u(mips),
                    u(cut),
                    u(layers),
                ))
            }
            _ => {
                let [raw] = raws else { return None };
                Some(self.count_from((*raw).max(0) as u64) as usize)
            }
        }
    }

    pub fn count_mask(&self) -> Option<u64> {
        match self {
            CountSource::MaskedField { mask, .. } | CountSource::MaskedOrMipChain { mask, .. } => {
                Some(*mask)
            }
            _ => None,
        }
    }

    pub fn count_from(&self, raw: u64) -> u64 {
        match self.count_mask() {
            Some(mask) => raw & mask,
            None => raw,
        }
    }

    pub fn capacity_field(&self) -> Option<&str> {
        match self {
            CountSource::CountAndCapacity { capacity, .. } => Some(capacity),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pointee {
    One {
        type_name: String,
    },

    Many {
        #[serde(default)]
        type_name: Option<String>,
        count: CountSource,
        #[serde(default)]
        element_size: Option<usize>,
    },

    ManyPtr {
        #[serde(default)]
        type_name: Option<String>,
        count: CountSource,
    },

    Bytes {
        length: CountSource,
    },

    Str,

    Opaque,

    /// One pointer at a described member of the record holding this field.
    ///
    /// ⚠ The record, not the struct declaring the field: a `ptxBehaviour`
    /// base declares the table and the variant embedding it holds the
    /// members, so a member is found in the owner's concrete layout.
    Member {
        type_name: String,
    },

    /// A run of pointers, each at a described member of that record.
    Members {
        type_name: String,
        count: CountSource,
    },

    /// A run of pointers, each at a NUL-terminated string of its own.
    Strings {
        count: CountSource,
    },
}

impl Pointee {
    pub fn is_member(&self) -> bool {
        matches!(self, Pointee::Member { .. } | Pointee::Members { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldRepr {
    Scalar(Scalar),

    ScalarArray {
        element: Scalar,
        count: usize,
    },

    Inline {
        type_name: String,
    },

    InlineArray {
        type_name: String,
        count: usize,
    },

    Ptr(Pointee),

    AtArray(Pointee),

    PtrSlots {
        pointee: Pointee,
        count: usize,
    },

    Enum {
        type_name: String,
        width: IntWidth,
    },

    Padding {
        size: usize,
    },

    Opaque {
        size: usize,
    },

    Trailing {
        #[serde(default)]
        type_name: Option<String>,
        count: CountSource,
        #[serde(default)]
        element_size: Option<usize>,
    },
}

pub const POINTER_SIZE: usize = 8;

pub const AT_ARRAY_SIZE: usize = 16;

impl FieldRepr {
    pub fn inline_size(&self) -> Option<usize> {
        match self {
            FieldRepr::Scalar(s) => Some(s.size()),
            FieldRepr::ScalarArray { element, count } => Some(element.size() * count),
            FieldRepr::Enum { width, .. } => Some(width.size()),
            FieldRepr::Padding { size } | FieldRepr::Opaque { size } => Some(*size),
            FieldRepr::Ptr(_) => Some(POINTER_SIZE),
            FieldRepr::AtArray(_) => Some(AT_ARRAY_SIZE),
            FieldRepr::PtrSlots { count, .. } => Some(POINTER_SIZE * count),
            FieldRepr::Trailing { .. } => Some(0),
            FieldRepr::Inline { .. } | FieldRepr::InlineArray { .. } => None,
        }
    }

    pub fn is_indirect(&self) -> bool {
        self.pointee().is_some()
    }

    pub fn pointee(&self) -> Option<&Pointee> {
        match self {
            FieldRepr::Ptr(p) | FieldRepr::AtArray(p) | FieldRepr::PtrSlots { pointee: p, .. } => {
                Some(p)
            }
            _ => None,
        }
    }

    pub fn is_trailing(&self) -> bool {
        matches!(self, FieldRepr::Trailing { .. })
    }

    pub fn trailing_element_size(&self) -> Option<usize> {
        match self {
            FieldRepr::Trailing {
                type_name: None,
                element_size,
                ..
            } => Some(element_size.unwrap_or(1)),
            _ => None,
        }
    }

    pub fn describes_its_target(&self) -> bool {
        !matches!(self.pointee(), None | Some(Pointee::Opaque))
    }

    pub fn referenced_enum(&self) -> Option<&str> {
        match self {
            FieldRepr::Enum { type_name, .. } => Some(type_name),
            _ => None,
        }
    }

    pub fn referenced_type(&self) -> Option<&str> {
        match self {
            FieldRepr::Inline { type_name } | FieldRepr::InlineArray { type_name, .. } => {
                Some(type_name)
            }
            FieldRepr::Trailing { type_name, .. } => type_name.as_deref(),
            FieldRepr::Ptr(p) | FieldRepr::AtArray(p) | FieldRepr::PtrSlots { pointee: p, .. } => {
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
            _ => None,
        }
    }

    pub(crate) fn length_source(&self) -> Option<&CountSource> {
        let pointee = match self {
            FieldRepr::Trailing { count, .. } => return Some(count),
            FieldRepr::Ptr(p) | FieldRepr::AtArray(p) | FieldRepr::PtrSlots { pointee: p, .. } => p,
            _ => return None,
        };
        match pointee {
            Pointee::Many { count, .. }
            | Pointee::ManyPtr { count, .. }
            | Pointee::Members { count, .. }
            | Pointee::Bytes { length: count } => Some(count),
            _ => None,
        }
    }

    /// This pointer, pointing into its owner rather than at an allocation of
    /// its own: `One` becomes `Member` and `ManyPtr` becomes `Members`.
    ///
    /// `None` for a repr with no such reading, which the macro refuses.
    pub fn points_into_owner(self) -> Option<FieldRepr> {
        let into = |pointee: Pointee| match pointee {
            Pointee::One { type_name } => Some(Pointee::Member { type_name }),
            Pointee::ManyPtr {
                type_name: Some(type_name),
                count,
            } => Some(Pointee::Members { type_name, count }),
            _ => None,
        };
        match self {
            FieldRepr::Ptr(p) => into(p).map(FieldRepr::Ptr),
            FieldRepr::AtArray(p) => into(p).map(FieldRepr::AtArray),
            _ => None,
        }
    }

    pub fn count_field(&self) -> Option<&str> {
        self.length_source()?.count_field()
    }

    pub fn capacity_field(&self) -> Option<&str> {
        self.length_source()?.capacity_field()
    }

    pub fn count_mask(&self) -> Option<u64> {
        self.length_source()?.count_mask()
    }

    pub fn stride_field(&self) -> Option<&str> {
        self.length_source()?.stride_field()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsigned_integers_read_at_every_width() {
        let bytes = [0xFFu8, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(IntWidth::U8.read(&bytes, 0), Some(0xFF));
        assert_eq!(IntWidth::U16.read(&bytes, 0), Some(0xFFFF));
        assert_eq!(IntWidth::U32.read(&bytes, 0), Some(0xFFFF_FFFF));
        assert_eq!(IntWidth::U64.read(&bytes, 0), Some(0xFFFF_FFFF_FFFF_FFFF));
    }

    #[test]
    fn signed_integers_sign_extend_from_their_declared_width() {
        let bytes = [0xFFu8, 0xFF, 0x00, 0x00];
        assert_eq!(IntWidth::I16.read(&bytes, 0), Some(-1));
        assert_eq!(IntWidth::U16.read(&bytes, 0), Some(65535));

        let bytes = [0x80u8, 0xFF, 0xFF, 0xFF];
        assert_eq!(IntWidth::I32.read(&bytes, 0), Some(-128));
    }

    #[test]
    fn a_read_past_the_end_is_none_rather_than_a_panic() {
        let bytes = [0u8; 4];
        assert_eq!(IntWidth::U64.read(&bytes, 0), None);
        assert_eq!(IntWidth::U32.read(&bytes, 1), None);
        assert_eq!(IntWidth::U8.read(&bytes, 99), None);
    }

    #[test]
    fn inline_size_is_the_field_not_the_target() {
        assert_eq!(
            FieldRepr::Ptr(Pointee::Bytes {
                length: CountSource::Inline
            })
            .inline_size(),
            Some(8)
        );
        assert_eq!(
            FieldRepr::AtArray(Pointee::One {
                type_name: "Bone".into()
            })
            .inline_size(),
            Some(16)
        );
        assert_eq!(
            FieldRepr::ScalarArray {
                element: Scalar::F32,
                count: 4
            }
            .inline_size(),
            Some(16)
        );
        assert_eq!(
            FieldRepr::PtrSlots {
                pointee: Pointee::Opaque,
                count: 4
            }
            .inline_size(),
            Some(32)
        );
    }

    #[test]
    fn an_embedded_type_has_no_size_until_the_pack_is_whole() {
        assert_eq!(
            FieldRepr::Inline {
                type_name: "VertexBuffer".into()
            }
            .inline_size(),
            None
        );
    }

    #[test]
    fn the_referenced_type_is_findable_through_every_indirection() {
        let cases = [
            FieldRepr::Inline {
                type_name: "T".into(),
            },
            FieldRepr::InlineArray {
                type_name: "T".into(),
                count: 2,
            },
            FieldRepr::Ptr(Pointee::One {
                type_name: "T".into(),
            }),
            FieldRepr::AtArray(Pointee::Many {
                type_name: Some("T".into()),
                count: CountSource::Inline,
                element_size: None,
            }),
            FieldRepr::PtrSlots {
                pointee: Pointee::One {
                    type_name: "T".into(),
                },
                count: 4,
            },
        ];
        for case in cases {
            assert_eq!(case.referenced_type(), Some("T"), "{case:?}");
        }

        assert_eq!(FieldRepr::Scalar(Scalar::Bool).referenced_type(), None);
        assert_eq!(FieldRepr::Ptr(Pointee::Str).referenced_type(), None);
    }

    #[test]
    fn a_count_field_dependency_is_reportable() {
        let field = FieldRepr::Ptr(Pointee::Many {
            type_name: None,
            count: CountSource::Field("bone_index_count".into()),
            element_size: Some(2),
        });
        assert_eq!(field.count_field(), Some("bone_index_count"));
        assert_eq!(field.capacity_field(), None);

        assert_eq!(
            FieldRepr::AtArray(Pointee::Many {
                type_name: None,
                count: CountSource::Inline,
                element_size: None
            })
            .count_field(),
            None
        );
    }

    #[test]
    fn capacity_and_count_are_both_reachable() {
        let field = FieldRepr::Ptr(Pointee::Many {
            type_name: Some("PhBound".into()),
            count: CountSource::CountAndCapacity {
                count: "num_bounds".into(),
                capacity: "max_num_bounds".into(),
            },
            element_size: None,
        });

        assert_eq!(field.count_field(), Some("num_bounds"));
        assert_eq!(field.capacity_field(), Some("max_num_bounds"));
    }

    #[test]
    fn a_trailing_run_occupies_nothing_at_its_own_offset() {
        let field = FieldRepr::Trailing {
            type_name: Some("Mat34V".into()),
            count: CountSource::Field("matrix_count".into()),
            element_size: None,
        };
        assert_eq!(field.inline_size(), Some(0));
        assert!(field.is_trailing());
    }

    #[test]
    fn a_trailing_run_is_not_a_pointer_and_says_so() {
        let field = FieldRepr::Trailing {
            type_name: Some("Mat34V".into()),
            count: CountSource::Field("matrix_count".into()),
            element_size: None,
        };
        assert_eq!(field.pointee(), None);
        assert!(!field.is_indirect());
        assert!(!field.describes_its_target());
    }

    #[test]
    fn a_trailing_run_reaches_its_element_type_and_its_count_like_any_other_run() {
        let described = FieldRepr::Trailing {
            type_name: Some("ShaderParamInfo".into()),
            count: CountSource::Field("num_params".into()),
            element_size: None,
        };
        assert_eq!(described.referenced_type(), Some("ShaderParamInfo"));
        assert_eq!(described.count_field(), Some("num_params"));
        assert_eq!(described.trailing_element_size(), None);

        let payload = FieldRepr::Trailing {
            type_name: None,
            count: CountSource::Field("data_size".into()),
            element_size: Some(1),
        };
        assert_eq!(payload.referenced_type(), None);
        assert_eq!(payload.trailing_element_size(), Some(1));
    }

    #[test]
    fn a_plain_count_reports_no_capacity_rather_than_repeating_itself() {
        let plain = CountSource::Field("n".into());
        assert_eq!(plain.count_field(), Some("n"));
        assert_eq!(plain.capacity_field(), None);
    }
}
