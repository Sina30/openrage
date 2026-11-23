use crate::desc::PagingModel;

pub const MAX_CHUNKS: usize = 128;

pub const RESOURCE_16N_BITS: u8 = 1;
pub const RESOURCE_8N_BITS: u8 = 2;
pub const RESOURCE_4N_BITS: u8 = 4;
pub const RESOURCE_2N_BITS: u8 = 6;
pub const RESOURCE_N_BITS: u8 = 7;

pub const BUCKET_MAX: [u8; 9] = [
    (1 << RESOURCE_16N_BITS) - 1,
    (1 << RESOURCE_8N_BITS) - 1,
    (1 << RESOURCE_4N_BITS) - 1,
    (1 << RESOURCE_2N_BITS) - 1,
    (1 << RESOURCE_N_BITS) - 1,
    1,
    1,
    1,
    1,
];

#[derive(Debug, Clone, Default)]
pub struct DatAllocation {
    pub src_offset: usize,
    pub dest_offset: usize,
    pub size: u32,
    pub align_mask: u16,
    pub bucket: u16,
    pub touched: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Bucket {
    pub base_offset: usize,
    pub size: usize,
    pub offset: usize,
    pub shift: u8,
    pub split_allocation: Option<usize>,
    pub alloc_count: u32,
}

#[derive(Debug)]
pub struct SinglePassAllocator {
    is_virtual: bool,
    leaf_size: usize,
    heap_offset: usize,
    #[allow(dead_code)]
    heap_size: usize,
    allocations: Vec<DatAllocation>,
    buckets: Vec<Bucket>,
    bucket_counts: [u8; 9],
    base_alloc: usize,
    largest_allocation: usize,
    paging: PagingModel,
}

impl SinglePassAllocator {
    pub fn new(is_virtual: bool, heap_size: usize, leaf_size: usize) -> Self {
        Self {
            is_virtual,
            leaf_size,
            heap_offset: 0,
            heap_size,
            allocations: Vec::with_capacity(heap_size / 256),
            buckets: Vec::new(),
            bucket_counts: [0; 9],
            base_alloc: 0,
            largest_allocation: 0,
            paging: PagingModel::RAGE,
        }
    }

    /// Which base the addresses this allocator hands out sit above.
    ///
    /// ⚠ The **bucket structure** is not parameterised and must not be: 8 KB
    /// leaves and a 128-chunk maximum are RSC7's page table, not RAGE's
    /// address model, and a second container replaces this whole file rather
    /// than configuring it. What travels is only where an address starts.
    #[must_use]
    pub fn with_paging(mut self, paging: PagingModel) -> Self {
        self.paging = paging;
        self
    }

    #[inline]
    fn base(&self) -> u64 {
        if self.is_virtual {
            self.paging.virtual_base
        } else {
            self.paging.physical_base
        }
    }

    pub fn allocate(&mut self, size: usize, align: usize) -> u64 {
        let size = if size == 0 { 1 } else { size };
        let align = if align == 0 { 16 } else { align };

        let align_mask = align - 1;
        let size = (size + align_mask) & !align_mask;

        let offset = (self.heap_offset + align_mask + 1) & !align_mask;

        self.allocations.push(DatAllocation {
            src_offset: offset,
            dest_offset: 0,
            size: size as u32,
            align_mask: align_mask as u16,
            bucket: 0,
            touched: false,
        });

        self.heap_offset = offset + size;

        self.base() + offset as u64
    }

    pub fn heap_offset(&self) -> usize {
        self.heap_offset
    }

    pub fn allocation_count(&self) -> usize {
        self.allocations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.allocations.is_empty()
    }

    pub fn bucket_counts(&self) -> &[u8; 9] {
        &self.bucket_counts
    }

    pub fn base_alloc(&self) -> usize {
        self.base_alloc
    }

    pub fn leaf_size(&self) -> usize {
        self.leaf_size
    }

    pub fn allocations(&self) -> &[DatAllocation] {
        &self.allocations
    }

    pub fn total_bucket_size(&self) -> usize {
        self.buckets.iter().map(|b| b.size).sum()
    }

    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    pub fn pack_into_buckets(&mut self, base_alloc: usize, bucket_limit: usize) -> Option<usize> {
        let mut total_size = 0usize;
        self.buckets.clear();
        self.bucket_counts = [0; 9];

        for a in 0..self.allocations.len() {
            let alloc = &self.allocations[a];
            let alloc_size = alloc.size as usize;
            let alloc_align_mask = alloc.align_mask as usize;

            let mut found_bucket: Option<usize> = None;
            for (b, bucket) in self.buckets.iter().enumerate() {
                let new_offset =
                    ((bucket.offset + alloc_align_mask) & !alloc_align_mask) + alloc_size;
                if new_offset <= bucket.size {
                    found_bucket = Some(b);
                    break;
                }
            }

            let bucket_idx = match found_bucket {
                Some(b) => b,
                None => {
                    if self.buckets.len() >= bucket_limit {
                        return None;
                    }

                    let bucket_size = if a == 0 {
                        self.largest_allocation
                    } else {
                        alloc_size
                    };

                    let leaf_count = bucket_size.div_ceil(self.leaf_size);

                    let mut leaf_count = leaf_count.next_power_of_two();

                    if leaf_count < base_alloc {
                        leaf_count = base_alloc;
                    }

                    let base_log = log2_pow2(base_alloc);
                    let leaf_log = log2_pow2(leaf_count);
                    let count_idx = if leaf_log >= base_log {
                        (base_log as i32 - leaf_log as i32 + 4).max(0) as usize
                    } else {
                        (4 + base_log - leaf_log) as usize
                    };

                    let count_idx = count_idx.min(8);

                    if self.bucket_counts[count_idx] >= BUCKET_MAX[count_idx] {
                        return None;
                    }
                    self.bucket_counts[count_idx] += 1;

                    let new_size = leaf_count * self.leaf_size;
                    let new_bucket_idx = self.buckets.len();

                    self.buckets.push(Bucket {
                        base_offset: 0,
                        size: new_size,
                        offset: 0,
                        shift: count_idx as u8,
                        split_allocation: None,
                        alloc_count: 0,
                    });

                    total_size += new_size;
                    new_bucket_idx
                }
            };

            let bucket = &mut self.buckets[bucket_idx];
            let alloc = &mut self.allocations[a];
            let dest_offset =
                (bucket.offset + alloc.align_mask as usize) & !(alloc.align_mask as usize);
            alloc.dest_offset = dest_offset;
            alloc.bucket = bucket_idx as u16;

            let half_size = bucket.size >> 1;
            if bucket.alloc_count > 0
                && bucket.offset <= half_size
                && dest_offset + alloc.size as usize > half_size
            {
                bucket.split_allocation = Some(a);
            }

            bucket.offset = dest_offset + alloc.size as usize;
            bucket.alloc_count += 1;
        }

        Some(total_size)
    }

    fn optimize_buckets(&mut self, mut base_alloc: usize) -> usize {
        let max_shift = 8usize;
        let mut total_size: usize = self.buckets.iter().map(|b| b.size).sum();

        loop {
            if self.buckets.is_empty() {
                break;
            }

            let last_idx = self.buckets.len() - 1;
            let last_bucket = &self.buckets[last_idx];

            let can_split = last_bucket.split_allocation.is_some()
                && last_bucket.offset <= (last_bucket.size * 3) / 4
                && (last_bucket.shift as usize) < max_shift - 1
                && base_alloc > 2
                && self.buckets.len() < MAX_CHUNKS;

            if !can_split {
                break;
            }

            let split_alloc_idx = last_bucket.split_allocation.unwrap();
            let new_size = last_bucket.size >> 2;

            let mut new_offset = 0usize;
            let mut new_count = 0u32;
            let mut fits = true;

            for alloc in &self.allocations[split_alloc_idx..] {
                if alloc.bucket as usize == last_idx {
                    new_count += 1;
                    new_offset = ((new_offset + alloc.align_mask as usize)
                        & !(alloc.align_mask as usize))
                        + alloc.size as usize;
                    if new_offset > new_size {
                        fits = false;
                        break;
                    }
                }
            }

            if !fits {
                break;
            }

            let old_shift = self.buckets[last_idx].shift;
            let new_shift = old_shift + 1;

            let new_bucket_idx = self.buckets.len();
            new_offset = 0;
            let mut new_split: Option<usize> = None;

            for a in split_alloc_idx..self.allocations.len() {
                let alloc = &mut self.allocations[a];
                if alloc.bucket as usize == last_idx {
                    alloc.bucket = new_bucket_idx as u16;
                    let dest =
                        (new_offset + alloc.align_mask as usize) & !(alloc.align_mask as usize);

                    if a != split_alloc_idx
                        && new_offset <= (new_size >> 1)
                        && dest + alloc.size as usize > (new_size >> 1)
                    {
                        new_split = Some(a);
                    }

                    alloc.dest_offset = dest;
                    new_offset = dest + alloc.size as usize;
                }
            }

            {
                let last_bucket = &mut self.buckets[last_idx];
                last_bucket.offset = self.allocations[split_alloc_idx].dest_offset;
                last_bucket.size >>= 1;
                total_size -= last_bucket.size >> 1;

                self.bucket_counts[last_bucket.shift as usize] -= 1;
                last_bucket.shift += 1;
                self.bucket_counts[last_bucket.shift as usize] += 1;

                last_bucket.split_allocation = None;
                last_bucket.alloc_count -= new_count;
            }

            self.bucket_counts[new_shift as usize + 1] += 1;
            self.buckets.push(Bucket {
                base_offset: 0,
                size: new_size,
                offset: new_offset,
                shift: new_shift + 1,
                split_allocation: new_split,
                alloc_count: new_count,
            });

            base_alloc >>= 2;
        }

        if !self.buckets.is_empty() {
            let last_idx = self.buckets.len() - 1;
            loop {
                let last_bucket = &self.buckets[last_idx];
                if last_bucket.offset > (last_bucket.size >> 1)
                    || (last_bucket.shift as usize) >= max_shift
                    || base_alloc <= 1
                {
                    break;
                }

                let last_bucket = &mut self.buckets[last_idx];
                last_bucket.size >>= 1;
                total_size -= last_bucket.size;

                self.bucket_counts[last_bucket.shift as usize] -= 1;
                last_bucket.shift += 1;
                self.bucket_counts[last_bucket.shift as usize] += 1;

                base_alloc >>= 1;
            }
        }

        total_size
    }

    pub fn ready_build(&mut self, bucket_limit: usize) -> usize {
        let bucket_limit = bucket_limit.min(MAX_CHUNKS);

        if self.allocations.is_empty() {
            return 0;
        }

        let start_idx = if self.is_virtual { 1 } else { 0 };
        if self.allocations.len() > start_idx {
            self.allocations[start_idx..].sort_by(|a, b| match b.size.cmp(&a.size) {
                std::cmp::Ordering::Equal => b.align_mask.cmp(&a.align_mask),
                other => other,
            });
        }

        let largest_alloc_bytes = if self.is_virtual && self.allocations.len() > 1 {
            let root_size = self.allocations[0].size as usize;
            let largest = &self.allocations[1];
            let aligned_root =
                (root_size + largest.align_mask as usize) & !(largest.align_mask as usize);
            aligned_root + largest.size as usize
        } else if !self.allocations.is_empty() {
            self.allocations[0].size as usize
        } else {
            0
        };

        self.largest_allocation = largest_alloc_bytes;

        let largest_alloc = largest_alloc_bytes
            .div_ceil(self.leaf_size)
            .next_power_of_two();

        let largest_alloc = largest_alloc.max(16);

        let mut base_alloc: usize;
        let mut used = 0usize;
        let mut iteration = 0u32;

        loop {
            base_alloc = (largest_alloc << iteration) >> 4;
            if base_alloc == 0 {
                base_alloc = 1;
            }
            iteration += 1;

            if let Some(size) = self.pack_into_buckets(base_alloc, bucket_limit) {
                used = size;
                break;
            }

            if iteration > 20 {
                base_alloc = largest_alloc;
                if let Some(size) = self.pack_into_buckets(base_alloc, bucket_limit) {
                    used = size;
                }
                break;
            }
        }

        let max_alloc = 8usize;
        while base_alloc < max_alloc {
            if let Some(used2) = self.pack_into_buckets(base_alloc * 2, bucket_limit) {
                if used2 < used {
                    base_alloc *= 2;
                    used = used2;
                } else {
                    self.pack_into_buckets(base_alloc, bucket_limit);
                    break;
                }
            } else {
                self.pack_into_buckets(base_alloc, bucket_limit);
                break;
            }
        }

        self.optimize_buckets(base_alloc);
        self.base_alloc = base_alloc;

        let mut next_offset = 0usize;
        for bucket in &mut self.buckets {
            bucket.base_offset = next_offset;
            next_offset += bucket.size;
        }

        self.allocations.sort_by_key(|a| a.src_offset);

        for alloc in &mut self.allocations {
            let bucket = &self.buckets[alloc.bucket as usize];
            alloc.dest_offset += bucket.base_offset;
        }

        self.buckets.len()
    }

    pub fn to_size_fields(&self) -> (u8, u8, u8, u8, u8, u8, bool, bool, bool, bool) {
        let leaf_shift = log2_pow2(self.base_alloc);

        (
            leaf_shift,
            self.bucket_counts[0],
            self.bucket_counts[1],
            self.bucket_counts[2],
            self.bucket_counts[3],
            self.bucket_counts[4],
            self.bucket_counts[5] > 0,
            self.bucket_counts[6] > 0,
            self.bucket_counts[7] > 0,
            self.bucket_counts[8] > 0,
        )
    }
}

#[inline]
pub fn log2_pow2(x: usize) -> u8 {
    x.checked_ilog2().unwrap_or(0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{FIXED_PHYSICAL_BASE, FIXED_VIRTUAL_BASE};
    use crate::container::resource::{RSC_PHYSICAL_LEAF_SIZE, RSC_VIRTUAL_LEAF_SIZE};

    #[test]
    fn log2_pow2_on_zero_one_and_powers_of_two() {
        assert_eq!(log2_pow2(0), 0);
        for k in 0..usize::BITS {
            assert_eq!(log2_pow2(1usize << k), k as u8, "2^{k}");
        }
    }

    #[test]
    fn test_allocator_basic() {
        let mut alloc = SinglePassAllocator::new(true, 1024 * 1024, RSC_VIRTUAL_LEAF_SIZE as usize);

        let addr1 = alloc.allocate(100, 16);
        let addr2 = alloc.allocate(200, 16);

        assert!(addr1 >= FIXED_VIRTUAL_BASE);
        assert!(addr2 > addr1);
        assert_eq!(alloc.allocation_count(), 2);
    }

    #[test]
    fn test_allocator_alignment() {
        let mut alloc = SinglePassAllocator::new(true, 1024 * 1024, RSC_VIRTUAL_LEAF_SIZE as usize);

        let addr = alloc.allocate(100, 256);
        let offset = (addr - FIXED_VIRTUAL_BASE) as usize;

        assert_eq!(offset % 256, 0);
    }

    #[test]
    fn test_ready_build_simple() {
        let mut alloc = SinglePassAllocator::new(true, 1024 * 1024, RSC_VIRTUAL_LEAF_SIZE as usize);

        alloc.allocate(64, 16);
        alloc.allocate(1024, 16);
        alloc.allocate(4096, 16);

        let bucket_count = alloc.ready_build(MAX_CHUNKS / 2);

        assert!(bucket_count > 0, "Should create at least one bucket");
        assert!(
            alloc.total_bucket_size() >= alloc.heap_offset(),
            "Bucket size should cover all allocations"
        );
    }

    #[test]
    fn test_ready_build_sizes() {
        let mut alloc = SinglePassAllocator::new(true, 1024 * 1024, RSC_VIRTUAL_LEAF_SIZE as usize);

        alloc.allocate(100, 16);

        alloc.ready_build(MAX_CHUNKS / 2);

        let (leaf_shift, head16, head8, head4, head2, base, _, _, _, _) = alloc.to_size_fields();

        let total = head16 + head8 + head4 + head2 + base;
        assert!(total > 0, "Should have at least one chunk");
        assert!(leaf_shift < 16, "Leaf shift should be reasonable");
    }

    #[test]
    fn test_pack_into_buckets_resets() {
        let mut alloc = SinglePassAllocator::new(true, 1024 * 1024, RSC_VIRTUAL_LEAF_SIZE as usize);

        alloc.allocate(1024, 16);
        alloc.allocate(2048, 16);

        let size1 = alloc.pack_into_buckets(1, 64);
        let size2 = alloc.pack_into_buckets(1, 64);

        assert!(size1.is_some());
        assert!(size2.is_some());
        assert_eq!(size1, size2, "Same input should produce same output");
    }

    #[test]
    fn test_physical_allocator() {
        let mut alloc =
            SinglePassAllocator::new(false, 1024 * 1024, RSC_PHYSICAL_LEAF_SIZE as usize);

        let addr = alloc.allocate(1024, 16);

        assert!(
            addr >= FIXED_PHYSICAL_BASE,
            "Physical addresses should use physical base"
        );
        assert!(
            addr < FIXED_PHYSICAL_BASE + 0x10000000,
            "Address should be in physical range"
        );
    }
}
