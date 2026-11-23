use crate::container::resource::{RSC_PHYSICAL_LEAF_SIZE, RSC_VIRTUAL_LEAF_SIZE};
use crate::desc::PagingModel;
use crate::Generation;

use super::allocator::{SinglePassAllocator, MAX_CHUNKS};
use super::chunk_map::DatResourceSizeInfo;
use super::heap::ResourceHeap;

pub use super::heap::PgPointerFixup;

pub use super::resource::{is_physical_address, is_virtual_address};

#[derive(Debug)]
pub struct PgRscBuilder {
    heap: ResourceHeap,

    virtual_allocator: Option<SinglePassAllocator>,

    physical_allocator: Option<SinglePassAllocator>,

    build_ready: bool,
}

impl PgRscBuilder {
    pub fn new_bucketed(virtual_heap_size: usize, physical_heap_size: usize) -> Self {
        Self {
            heap: ResourceHeap::with_capacity(virtual_heap_size, physical_heap_size),
            virtual_allocator: Some(SinglePassAllocator::new(
                true,
                virtual_heap_size,
                RSC_VIRTUAL_LEAF_SIZE as usize,
            )),
            physical_allocator: Some(SinglePassAllocator::new(
                false,
                physical_heap_size,
                RSC_PHYSICAL_LEAF_SIZE as usize,
            )),
            build_ready: false,
        }
    }

    pub fn new_bucketed_default() -> Self {
        Self::new_bucketed(4 * 1024 * 1024, 4 * 1024 * 1024)
    }

    pub fn targeting(mut self, generation: Generation) -> Self {
        self.heap.set_generation(generation);
        self
    }

    pub fn heap(&self) -> &ResourceHeap {
        &self.heap
    }

    pub fn heap_mut(&mut self) -> &mut ResourceHeap {
        &mut self.heap
    }

    /// Where the addresses this builder invents come from.
    ///
    /// Reaches both halves: the heap's linear append and the allocator the
    /// packer hands addresses out of. ⚠ Both, because
    /// either one left behind emits a file whose two spaces disagree, and
    /// `a_second_paging_model_on_the_write_half` builds through the bucketed
    /// path precisely so that a model threaded into one of them fails.
    #[must_use]
    pub fn with_paging(mut self, paging: PagingModel) -> Self {
        self.heap = std::mem::take(&mut self.heap).with_paging(paging);
        self.virtual_allocator = self.virtual_allocator.take().map(|a| a.with_paging(paging));
        self.physical_allocator = self
            .physical_allocator
            .take()
            .map(|a| a.with_paging(paging));
        self
    }

    pub fn paging(&self) -> PagingModel {
        self.heap.paging()
    }

    pub fn ready_build(&mut self) -> (usize, usize) {
        if self.build_ready {
            return (
                self.virtual_allocator
                    .as_ref()
                    .map(|a| a.bucket_count())
                    .unwrap_or(0),
                self.physical_allocator
                    .as_ref()
                    .map(|a| a.bucket_count())
                    .unwrap_or(0),
            );
        }

        let virtual_len = self.heap.virtual_data().len();
        let physical_len = self.heap.physical_data().len();

        if let Some(ref mut alloc) = self.virtual_allocator {
            if virtual_len != 0 {
                alloc.allocate(virtual_len, 16);
            }
        }

        if let Some(ref mut alloc) = self.physical_allocator {
            if physical_len != 0 {
                alloc.allocate(physical_len, 16);
            }
        }

        let virtual_bucket_limit = MAX_CHUNKS / 2;
        let physical_bucket_limit = MAX_CHUNKS - virtual_bucket_limit;

        let virtual_count = if let Some(ref mut alloc) = self.virtual_allocator {
            alloc.ready_build(virtual_bucket_limit)
        } else {
            0
        };

        let physical_count = if let Some(ref mut alloc) = self.physical_allocator {
            alloc.ready_build(physical_bucket_limit)
        } else {
            0
        };

        self.build_ready = true;

        (virtual_count, physical_count)
    }

    pub fn virtual_size_info(&self) -> Option<DatResourceSizeInfo> {
        self.virtual_allocator.as_ref().map(size_info_of)
    }

    pub fn physical_size_info(&self) -> Option<DatResourceSizeInfo> {
        self.physical_allocator.as_ref().map(size_info_of)
    }

    pub fn finish(self) -> (Vec<u8>, Vec<u8>) {
        self.heap.finish()
    }
}

fn size_info_of(alloc: &SinglePassAllocator) -> DatResourceSizeInfo {
    let (leaf_shift, head16, head8, head4, head2, base, t2, t4, t8, t16) = alloc.to_size_fields();
    DatResourceSizeInfo {
        leaf_shift,
        head16_count: head16,
        head8_count: head8,
        head4_count: head4,
        head2_count: head2,
        base_count: base,
        has_tail2: t2,
        has_tail4: t4,
        has_tail8: t8,
        has_tail16: t16,
    }
}
