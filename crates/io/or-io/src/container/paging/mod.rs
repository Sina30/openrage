pub mod allocator;
pub mod builder;
pub mod chunk_map;
pub mod heap;
pub mod resource;

pub use resource::{
    is_physical_address, is_virtual_address, page_base, page_offset, read_pointer, read_u16_raw,
    DatResource, FIXED_PHYSICAL_BASE, FIXED_VIRTUAL_BASE, NULL_PTR, PHYSICAL_PAGE_BASE,
    POINTER_SIZE, VIRTUAL_PAGE_BASE,
};

pub use builder::{PgPointerFixup, PgRscBuilder};

pub use heap::{ResourceHeap, DEFAULT_CANVAS_FILL, POISON_CANVAS_FILL};

pub use allocator::{log2_pow2, Bucket, DatAllocation, SinglePassAllocator, MAX_CHUNKS};

pub use chunk_map::DatResourceSizeInfo;
