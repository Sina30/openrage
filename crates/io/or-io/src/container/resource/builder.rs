pub use crate::container::paging::{FIXED_PHYSICAL_BASE, FIXED_VIRTUAL_BASE};

/// An RSC7 flag word's page shape, `datResourceInfo::Sizes`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DatResourceSizes {
    pub leaf_shift: u8,
    pub head16_count: u8,
    pub head8_count: u8,
    pub head4_count: u8,
    pub head2_count: u8,
    pub base_count: u8,
    pub has_tail2: bool,
    pub has_tail4: bool,
    pub has_tail8: bool,
    pub has_tail16: bool,
    pub version: u8,
}

pub const RSC_VIRTUAL_LEAF_SIZE: u32 = 8192;
pub const RSC_PHYSICAL_LEAF_SIZE: u32 = 8192;

/// The page size the game refuses a resource over, in bytes: its error names the limit as 100 MB and a 128 MiB page as 134.22 MB.
pub const GAME_PAGE_LIMIT: u32 = 100_000_000;

impl DatResourceSizes {
    pub fn from_flags(flags: u32) -> Self {
        Self {
            leaf_shift: (flags & 0xF) as u8,
            head16_count: ((flags >> 4) & 0x1) as u8,
            head8_count: ((flags >> 5) & 0x3) as u8,
            head4_count: ((flags >> 7) & 0xF) as u8,
            head2_count: ((flags >> 11) & 0x3F) as u8,
            base_count: ((flags >> 17) & 0x7F) as u8,
            has_tail2: (flags >> 24) & 0x1 != 0,
            has_tail4: (flags >> 25) & 0x1 != 0,
            has_tail8: (flags >> 26) & 0x1 != 0,
            has_tail16: (flags >> 27) & 0x1 != 0,
            version: ((flags >> 28) & 0xF) as u8,
        }
    }

    pub fn from_size_info(
        info: &crate::container::paging::chunk_map::DatResourceSizeInfo,
        version: u8,
    ) -> Self {
        Self {
            leaf_shift: info.leaf_shift,
            head16_count: info.head16_count,
            head8_count: info.head8_count,
            head4_count: info.head4_count,
            head2_count: info.head2_count,
            base_count: info.base_count,
            has_tail2: info.has_tail2,
            has_tail4: info.has_tail4,
            has_tail8: info.has_tail8,
            has_tail16: info.has_tail16,
            version,
        }
    }

    pub fn to_flags(&self) -> u32 {
        debug_assert!(
            self.validate_tail_chunks(),
            "Invalid tail chunk configuration for leaf_shift={}: has_tail2={}, has_tail4={}, has_tail8={}, has_tail16={}",
            self.leaf_shift, self.has_tail2, self.has_tail4, self.has_tail8, self.has_tail16
        );

        let mut flags = 0u32;
        flags |= (self.leaf_shift as u32) & 0xF;
        flags |= ((self.head16_count as u32) & 0x1) << 4;
        flags |= ((self.head8_count as u32) & 0x3) << 5;
        flags |= ((self.head4_count as u32) & 0xF) << 7;
        flags |= ((self.head2_count as u32) & 0x3F) << 11;
        flags |= ((self.base_count as u32) & 0x7F) << 17;
        if self.has_tail2 {
            flags |= 1 << 24;
        }
        if self.has_tail4 {
            flags |= 1 << 25;
        }
        if self.has_tail8 {
            flags |= 1 << 26;
        }
        if self.has_tail16 {
            flags |= 1 << 27;
        }
        flags |= ((self.version as u32) & 0xF) << 28;
        flags
    }

    pub fn get_size(&self, leaf_size: u32) -> u32 {
        let base_chunk_size = (leaf_size as u64) << (self.leaf_shift.min(31) as u64);

        let main_chunks = ((self.head16_count as u64) << 4)
            + ((self.head8_count as u64) << 3)
            + ((self.head4_count as u64) << 2)
            + ((self.head2_count as u64) << 1)
            + (self.base_count as u64);
        let main_size = main_chunks.saturating_mul(base_chunk_size);

        let tail_size = if self.has_tail2 {
            base_chunk_size >> 1
        } else {
            0
        } + if self.has_tail4 {
            base_chunk_size >> 2
        } else {
            0
        } + if self.has_tail8 {
            base_chunk_size >> 3
        } else {
            0
        } + if self.has_tail16 {
            base_chunk_size >> 4
        } else {
            0
        };

        main_size.saturating_add(tail_size).min(u32::MAX as u64) as u32
    }

    /// The largest single page the flags describe, in bytes.
    ///
    /// A page is one run-time allocation, and the game refuses one over [`GAME_PAGE_LIMIT`] (`ERR_STR_FAILURE_3: page 0 in paged
    /// resource ... is over 100 MB`): with 8 KiB leaves that is a 128 MiB page, the answer of [`Self::single_chunk`] for a heap
    /// of more than 64 MiB.
    pub fn largest_page(&self, leaf_size: u32) -> u32 {
        let base = (leaf_size as u64) << (self.leaf_shift.min(31) as u64);
        let main = [
            (self.head16_count, 16),
            (self.head8_count, 8),
            (self.head4_count, 4),
            (self.head2_count, 2),
            (self.base_count, 1),
        ]
        .into_iter()
        .find_map(|(count, pages)| (count > 0).then_some(pages * base));
        let tail = [
            (self.has_tail2, base >> 1),
            (self.has_tail4, base >> 2),
            (self.has_tail8, base >> 3),
            (self.has_tail16, base >> 4),
        ]
        .into_iter()
        .find_map(|(has, size)| has.then_some(size));
        main.or(tail).unwrap_or(0).min(u32::MAX as u64) as u32
    }

    pub fn get_chunk_count(&self) -> u32 {
        (self.head16_count as u32)
            + (self.head8_count as u32)
            + (self.head4_count as u32)
            + (self.head2_count as u32)
            + (self.base_count as u32)
            + (self.has_tail2 as u32)
            + (self.has_tail4 as u32)
            + (self.has_tail8 as u32)
            + (self.has_tail16 as u32)
    }

    #[inline]
    pub fn is_tail2_allowed(&self) -> bool {
        self.leaf_shift > 0
    }

    #[inline]
    pub fn is_tail4_allowed(&self) -> bool {
        self.leaf_shift > 1
    }

    #[inline]
    pub fn is_tail8_allowed(&self) -> bool {
        self.leaf_shift > 2
    }

    #[inline]
    pub fn is_tail16_allowed(&self) -> bool {
        self.leaf_shift > 3
    }

    pub fn validate_tail_chunks(&self) -> bool {
        if self.leaf_shift == 0
            && (self.has_tail2 || self.has_tail4 || self.has_tail8 || self.has_tail16)
        {
            return false;
        }
        if self.leaf_shift <= 1 && (self.has_tail4 || self.has_tail8 || self.has_tail16) {
            return false;
        }
        if self.leaf_shift <= 2 && (self.has_tail8 || self.has_tail16) {
            return false;
        }
        if self.leaf_shift <= 3 && self.has_tail16 {
            return false;
        }
        true
    }

    /// The smallest shape of **one** chunk that covers `len`.
    ///
    /// ⚠ **One chunk is a page nothing can straddle.** A multi-chunk page is
    /// several separate run-time allocations, so an object crossing a
    /// boundary is an object whose pointer the fixup resolves against the
    /// wrong chunk. Shipped content never does it: 0 of 29,059 locatable
    /// regions across 2,338 RBM documents cross a boundary, 730 of which
    /// declare more than one chunk — measured by
    /// `what_shipped_rbm_pages_are_shaped_like` at `709c1431`. A packer that
    /// assigns allocations to chunks may use the wider set; a caller laying
    /// bytes out flat may not, and this is the flat answer.
    ///
    /// The cost is that the reachable sizes are the power-of-two multiples of
    /// `leaf_size` and nothing between.
    pub fn single_chunk(len: u32, leaf_size: u32, version: u8) -> Option<Self> {
        if leaf_size == 0 {
            return None;
        }
        for leaf_shift in 0u8..16 {
            let base = u64::from(leaf_size) << leaf_shift;
            if base > u32::MAX as u64 {
                break;
            }
            for (head16, head8, head4, head2, base_count, multiple) in [
                (0u8, 0u8, 0u8, 0u8, 1u8, 1u64),
                (0, 0, 0, 1, 0, 2),
                (0, 0, 1, 0, 0, 4),
                (0, 1, 0, 0, 0, 8),
                (1, 0, 0, 0, 0, 16),
            ] {
                let size = base * multiple;
                if size > u32::MAX as u64 || size < u64::from(len) {
                    continue;
                }
                return Some(Self {
                    leaf_shift,
                    head16_count: head16,
                    head8_count: head8,
                    head4_count: head4,
                    head2_count: head2,
                    base_count,
                    has_tail2: false,
                    has_tail4: false,
                    has_tail8: false,
                    has_tail16: false,
                    version,
                });
            }
        }
        None
    }

    #[inline]
    pub fn max_allowed_tail_divisor(&self) -> u8 {
        match self.leaf_shift {
            0 => 0,
            1 => 2,
            2 => 4,
            3 => 8,
            _ => 16,
        }
    }
}

pub fn dat_encode_resource_flags_v2(data_size: u32, leaf_size: u32, version: u8) -> u32 {
    if data_size == 0 {
        return 0;
    }

    for leaf_shift in 0u8..16 {
        let base_chunk_size = (leaf_size as u64) << leaf_shift;

        if let Some(sizes) = try_fit_size(data_size, base_chunk_size, leaf_shift, version) {
            return sizes.to_flags();
        }
    }

    tracing::warn!(
        data_size,
        leaf_size,
        version,
        "Flag encoding fallback triggered - this indicates a bug"
    );
    let sizes = DatResourceSizes {
        leaf_shift: 15,
        base_count: 127,
        version,
        ..Default::default()
    };
    sizes.to_flags()
}

fn try_fit_size(
    data_size: u32,
    base_chunk_size: u64,
    leaf_shift: u8,
    version: u8,
) -> Option<DatResourceSizes> {
    if base_chunk_size > u32::MAX as u64 || base_chunk_size == 0 {
        return None;
    }

    let mut sizes = DatResourceSizes {
        leaf_shift,
        version,
        ..Default::default()
    };

    let mut remaining = data_size as u64;

    let chunk_16x = base_chunk_size * 16;
    if remaining >= chunk_16x {
        sizes.head16_count = 1;
        remaining -= chunk_16x;
    }

    let chunk_8x = base_chunk_size * 8;
    let count_8x = (remaining / chunk_8x).min(3);
    if count_8x > 0 {
        sizes.head8_count = count_8x as u8;
        remaining -= count_8x * chunk_8x;
    }

    let chunk_4x = base_chunk_size * 4;
    let count_4x = (remaining / chunk_4x).min(15);
    if count_4x > 0 {
        sizes.head4_count = count_4x as u8;
        remaining -= count_4x * chunk_4x;
    }

    let chunk_2x = base_chunk_size * 2;
    let count_2x = (remaining / chunk_2x).min(63);
    if count_2x > 0 {
        sizes.head2_count = count_2x as u8;
        remaining -= count_2x * chunk_2x;
    }

    let count_base = (remaining / base_chunk_size).min(127);
    if count_base > 0 {
        sizes.base_count = count_base as u8;
        remaining -= count_base * base_chunk_size;
    }

    let tail2 = base_chunk_size / 2;
    let tail4 = base_chunk_size / 4;
    let tail8 = base_chunk_size / 8;
    let tail16 = base_chunk_size / 16;

    let can_use_tail2 = sizes.is_tail2_allowed();
    let can_use_tail4 = sizes.is_tail4_allowed();
    let can_use_tail8 = sizes.is_tail8_allowed();
    let can_use_tail16 = sizes.is_tail16_allowed();

    if remaining > 0 {
        let mut tail_remaining = remaining;

        if tail_remaining > 0 && can_use_tail2 && tail2 > 0 {
            if tail_remaining <= tail2 {
                sizes.has_tail2 = true;
                tail_remaining = 0;
            } else {
                sizes.has_tail2 = true;
                tail_remaining -= tail2;
            }
        }

        if tail_remaining > 0 && can_use_tail4 && tail4 > 0 {
            if tail_remaining <= tail4 {
                sizes.has_tail4 = true;
                tail_remaining = 0;
            } else {
                sizes.has_tail4 = true;
                tail_remaining -= tail4;
            }
        }

        if tail_remaining > 0 && can_use_tail8 && tail8 > 0 {
            if tail_remaining <= tail8 {
                sizes.has_tail8 = true;
                tail_remaining = 0;
            } else {
                sizes.has_tail8 = true;
                tail_remaining -= tail8;
            }
        }

        if tail_remaining > 0 && can_use_tail16 && tail16 > 0 && tail_remaining <= tail16 {
            sizes.has_tail16 = true;
            tail_remaining = 0;
        }

        if tail_remaining > 0 {
            if sizes.base_count < 127 {
                sizes.base_count += 1;
                remaining = 0;
            } else if sizes.head2_count < 63 {
                sizes.head2_count += 1;
                remaining = 0;
            } else {
                return None;
            }
        } else {
            remaining = 0;
        }
    }

    if remaining > 0 {
        return None;
    }

    let allocated = sizes.get_size(RSC_VIRTUAL_LEAF_SIZE);
    if allocated >= data_size && sizes.get_chunk_count() > 0 {
        Some(sizes)
    } else {
        None
    }
}

pub fn dat_encode_resource_flags(data_size: u32) -> u32 {
    dat_encode_resource_flags_v2(data_size, RSC_VIRTUAL_LEAF_SIZE, 0)
}

pub fn dat_encode_physical_flags(data_size: u32) -> u32 {
    dat_encode_resource_flags_v2(data_size, RSC_PHYSICAL_LEAF_SIZE, 0xD)
}

#[derive(Debug, Default)]
pub struct StringTable {
    strings: Vec<String>,
}

impl StringTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, s: &str) -> u32 {
        if let Some(idx) = self.strings.iter().position(|x| x == s) {
            return idx as u32;
        }

        let idx = self.strings.len();
        self.strings.push(s.to_string());
        idx as u32
    }

    pub fn strings(&self) -> &[String] {
        &self.strings
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();

        for s in &self.strings {
            data.extend(s.as_bytes());
            data.push(0);

            while data.len() % 4 != 0 {
                data.push(0);
            }
        }

        data
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    #[test]
    fn test_string_table() {
        let mut table = StringTable::new();

        let idx1 = table.add("hello");
        let idx2 = table.add("world");
        let idx3 = table.add("hello");

        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        assert_eq!(idx3, 0);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn test_dat_resource_sizes_roundtrip() {
        let original_flags = 0xD30C0407u32;
        let sizes = DatResourceSizes::from_flags(original_flags);
        let roundtrip = sizes.to_flags();
        assert_eq!(original_flags, roundtrip, "Flags roundtrip failed");
    }

    #[test]
    fn test_dat_resource_sizes_decode() {
        let flags = 0xD30C0407u32;
        let sizes = DatResourceSizes::from_flags(flags);

        assert_eq!(sizes.version, 0xD, "Version nibble should be 0xD");
        assert_eq!(sizes.leaf_shift, 7, "LeafShift should be 7");

        let calculated_size = sizes.get_size(RSC_PHYSICAL_LEAF_SIZE);
        assert!(calculated_size > 0, "Size should be non-zero");
    }

    #[test]
    fn test_dat_resource_sizes_encode() {
        let flags = dat_encode_resource_flags(131072);
        let sizes = DatResourceSizes::from_flags(flags);
        let decoded_size = sizes.get_size(RSC_VIRTUAL_LEAF_SIZE);

        assert!(
            decoded_size >= 131072,
            "Encoded size should cover the requested size"
        );
    }

    #[test]
    fn test_dat_encode_physical_flags() {
        let flags = dat_encode_physical_flags(131072);
        let sizes = DatResourceSizes::from_flags(flags);

        assert_eq!(sizes.version, 0xD, "Physical flags should have version 0xD");
        let decoded_size = sizes.get_size(RSC_PHYSICAL_LEAF_SIZE);
        assert!(
            decoded_size >= 131072,
            "Encoded size should cover the requested size"
        );
    }

    #[test]
    fn test_dat_encode_small_sizes_get_minimum_page() {
        for size in [1, 100, 248, 1000, 4096, 8000, 8192] {
            let flags = dat_encode_resource_flags(size);
            let sizes = DatResourceSizes::from_flags(flags);
            let decoded = sizes.get_size(RSC_VIRTUAL_LEAF_SIZE);

            assert_eq!(
                decoded, 8192,
                "Size {} should encode to 8192 bytes (one page), got {} (flags={:#010X})",
                size, decoded, flags
            );

            assert_eq!(
                flags, 0x00020000,
                "Size {} should encode to flags 0x00020000, got {:#010X}",
                size, flags
            );
        }
    }

    #[test]
    fn test_dat_encode_slightly_over_one_page() {
        let flags = dat_encode_resource_flags(8193);
        let sizes = DatResourceSizes::from_flags(flags);
        let decoded = sizes.get_size(RSC_VIRTUAL_LEAF_SIZE);

        assert!(
            decoded >= 8193,
            "Size 8193 should encode to at least 8193, got {}",
            decoded
        );
    }

    #[test]
    fn test_sizes_get_size() {
        let mut sizes = DatResourceSizes::default();
        sizes.leaf_shift = 0;
        sizes.base_count = 1;

        let size = sizes.get_size(RSC_VIRTUAL_LEAF_SIZE);
        assert_eq!(size, 8192);

        sizes.leaf_shift = 1;
        let size = sizes.get_size(RSC_VIRTUAL_LEAF_SIZE);
        assert_eq!(size, 16384);

        sizes.leaf_shift = 0;
        sizes.head2_count = 1;
        let size = sizes.get_size(RSC_VIRTUAL_LEAF_SIZE);
        assert_eq!(size, 8192 + 16384);
    }

    #[test]
    fn test_validate_tail_chunks_leaf_shift_0() {
        let mut sizes = DatResourceSizes::default();
        sizes.leaf_shift = 0;
        sizes.base_count = 1;

        assert!(sizes.validate_tail_chunks());

        sizes.has_tail2 = true;
        assert!(!sizes.validate_tail_chunks());

        sizes.has_tail2 = false;
        sizes.has_tail4 = true;
        assert!(!sizes.validate_tail_chunks());

        sizes.has_tail4 = false;
        sizes.has_tail8 = true;
        assert!(!sizes.validate_tail_chunks());

        sizes.has_tail8 = false;
        sizes.has_tail16 = true;
        assert!(!sizes.validate_tail_chunks());
    }

    #[test]
    fn test_validate_tail_chunks_leaf_shift_1() {
        let mut sizes = DatResourceSizes::default();
        sizes.leaf_shift = 1;
        sizes.base_count = 1;

        assert!(sizes.validate_tail_chunks());

        sizes.has_tail2 = true;
        assert!(sizes.validate_tail_chunks());

        sizes.has_tail4 = true;
        assert!(!sizes.validate_tail_chunks());

        sizes.has_tail4 = false;
        sizes.has_tail8 = true;
        assert!(!sizes.validate_tail_chunks());

        sizes.has_tail8 = false;
        sizes.has_tail16 = true;
        assert!(!sizes.validate_tail_chunks());
    }

    #[test]
    fn test_validate_tail_chunks_leaf_shift_2() {
        let mut sizes = DatResourceSizes::default();
        sizes.leaf_shift = 2;
        sizes.base_count = 1;

        assert!(sizes.validate_tail_chunks());

        sizes.has_tail2 = true;
        assert!(sizes.validate_tail_chunks());

        sizes.has_tail4 = true;
        assert!(sizes.validate_tail_chunks());

        sizes.has_tail8 = true;
        assert!(!sizes.validate_tail_chunks());

        sizes.has_tail8 = false;
        sizes.has_tail16 = true;
        assert!(!sizes.validate_tail_chunks());
    }

    #[test]
    fn test_validate_tail_chunks_leaf_shift_3() {
        let mut sizes = DatResourceSizes::default();
        sizes.leaf_shift = 3;
        sizes.base_count = 1;

        sizes.has_tail2 = true;
        sizes.has_tail4 = true;
        sizes.has_tail8 = true;
        assert!(sizes.validate_tail_chunks());

        sizes.has_tail16 = true;
        assert!(!sizes.validate_tail_chunks());
    }

    #[test]
    fn test_validate_tail_chunks_leaf_shift_4_plus() {
        let mut sizes = DatResourceSizes::default();
        sizes.leaf_shift = 4;
        sizes.base_count = 1;

        sizes.has_tail2 = true;
        sizes.has_tail4 = true;
        sizes.has_tail8 = true;
        sizes.has_tail16 = true;
        assert!(sizes.validate_tail_chunks());

        sizes.leaf_shift = 8;
        assert!(sizes.validate_tail_chunks());

        sizes.leaf_shift = 15;
        assert!(sizes.validate_tail_chunks());
    }

    #[test]
    fn test_max_allowed_tail_divisor() {
        let mut sizes = DatResourceSizes::default();

        sizes.leaf_shift = 0;
        assert_eq!(sizes.max_allowed_tail_divisor(), 0);

        sizes.leaf_shift = 1;
        assert_eq!(sizes.max_allowed_tail_divisor(), 2);

        sizes.leaf_shift = 2;
        assert_eq!(sizes.max_allowed_tail_divisor(), 4);

        sizes.leaf_shift = 3;
        assert_eq!(sizes.max_allowed_tail_divisor(), 8);

        sizes.leaf_shift = 4;
        assert_eq!(sizes.max_allowed_tail_divisor(), 16);

        sizes.leaf_shift = 10;
        assert_eq!(sizes.max_allowed_tail_divisor(), 16);
    }

    #[test]
    fn test_encode_respects_tail_restrictions() {
        let flags = dat_encode_resource_flags(4096);
        let sizes = DatResourceSizes::from_flags(flags);
        assert!(
            sizes.validate_tail_chunks(),
            "Encoded flags should respect tail restrictions: {:?}",
            sizes
        );

        let flags = dat_encode_resource_flags(12288);
        let sizes = DatResourceSizes::from_flags(flags);
        assert!(
            sizes.validate_tail_chunks(),
            "Encoded flags should respect tail restrictions: {:?}",
            sizes
        );

        let flags = dat_encode_resource_flags(1_000_000);
        let sizes = DatResourceSizes::from_flags(flags);
        assert!(
            sizes.validate_tail_chunks(),
            "Encoded flags should respect tail restrictions: {:?}",
            sizes
        );
    }

    #[test]
    fn test_encode_physical_respects_tail_restrictions() {
        let flags = dat_encode_physical_flags(12288);
        let sizes = DatResourceSizes::from_flags(flags);
        assert!(
            sizes.validate_tail_chunks(),
            "Physical flags should respect tail restrictions: {:?}",
            sizes
        );
        assert_eq!(sizes.version, 0xD, "Physical flags should have version 0xD");
    }

    #[test]
    fn a_heap_over_64_mib_is_one_page_the_game_refuses() {
        const MIB: u32 = 1 << 20;
        let leaf = RSC_VIRTUAL_LEAF_SIZE;
        // The system flags of the hair that crashed a client with `ERR_STR_FAILURE_3`: one page of 128 MiB.
        let crashed = DatResourceSizes::from_flags(0xA000_001A);
        assert_eq!(crashed.largest_page(leaf), 128 * MIB);
        assert!(crashed.largest_page(leaf) > GAME_PAGE_LIMIT);
        // The largest heap one chunk holds under the limit is 64 MiB; a byte more is a 128 MiB page.
        let fits = DatResourceSizes::single_chunk(64 * MIB, leaf, 0).unwrap();
        assert_eq!(fits.largest_page(leaf), 64 * MIB);
        let over = DatResourceSizes::single_chunk(64 * MIB + 1, leaf, 0).unwrap();
        assert!(over.largest_page(leaf) > GAME_PAGE_LIMIT);
        // A heap of tail chunks only, and an empty one.
        let tails = DatResourceSizes::from_flags(0x0100_0004);
        assert_eq!(tails.largest_page(leaf), (leaf << 4) / 2);
        assert_eq!(DatResourceSizes::default().largest_page(leaf), 0);
    }
}
