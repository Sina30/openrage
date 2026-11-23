pub mod builder;
pub mod compression;
pub mod envelope;
mod header;
pub mod versions;
pub mod versions_registry;

pub use envelope::{Authoring, Opened, ResourceEnvelope, Rsc7, Rsc7Seal};

pub use compression::{
    dat_compress, dat_compress_best, dat_compress_default, dat_decompress, dat_decompress_bounded,
    dat_decompress_expecting,
};

pub use flate2::Compression;

pub use builder::{
    dat_encode_physical_flags, dat_encode_resource_flags, DatResourceSizes, StringTable,
    GAME_PAGE_LIMIT, RSC_PHYSICAL_LEAF_SIZE, RSC_VIRTUAL_LEAF_SIZE,
};

pub use crate::container::paging::{PgPointerFixup, PgRscBuilder};

pub use crate::container::paging::{FIXED_PHYSICAL_BASE, FIXED_VIRTUAL_BASE, NULL_PTR};

// The magic constant and the version-nibble helpers stay `pub(super)`: a
// caller writing through `Rsc7` names only which format it is writing.
pub use header::{DatResourceFileHeader, DatResourceFileType, DatResourceInfo};

pub use versions::{
    YBD_FILE_VERSION, YBN_FILE_VERSION, YCD_FILE_VERSION, YDD_FILE_VERSION, YDR_FILE_VERSION,
    YED_FILE_VERSION, YFD_FILE_VERSION, YFT_FILE_VERSION, YLD_FILE_VERSION, YND_FILE_VERSION,
    YNV_FILE_VERSION, YPT_FILE_VERSION, YSC_FILE_VERSION, YSC_FILE_VERSION_GEN8_ALT,
    YTD_FILE_VERSION, YVR_FILE_VERSION, YWR_FILE_VERSION,
};

pub use versions_registry::{
    canonical_write_version, is_version_supported, validate_version, ResourceType, TitleId,
    VersionError, VersionRegistry, VersionSupport, DEFAULT_REGISTRY, GTA5,
};
