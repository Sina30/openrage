// RSC7 header parsing and writing for RAGE resource files.

use flate2::Compression;
use tracing::debug;

use super::builder::{
    dat_encode_physical_flags, dat_encode_resource_flags, dat_encode_resource_flags_v2,
    DatResourceSizes, RSC_PHYSICAL_LEAF_SIZE, RSC_VIRTUAL_LEAF_SIZE,
};
use super::compression::{dat_compress, dat_decompress_expecting};
use super::versions::{
    YBD_FILE_VERSION, YBN_FILE_VERSION, YCD_FILE_VERSION, YDD_FILE_VERSION, YDR_FILE_VERSION,
    YED_FILE_VERSION, YFD_FILE_VERSION, YFT_FILE_VERSION, YLD_FILE_VERSION, YND_FILE_VERSION,
    YNV_FILE_VERSION, YPT_FILE_VERSION, YSC_FILE_VERSION, YTD_FILE_VERSION, YVR_FILE_VERSION,
    YWR_FILE_VERSION,
};
use super::versions_registry::{validate_version, ResourceType, TitleId, VersionError, GTA5};
use crate::container::paging::chunk_map::DatResourceSizeInfo;
use crate::decl::*;
use crate::error::{IoError, Result};
use crate::Generation;

/// `RSC7`, little-endian. Private to `resource/`: a caller sniffs a file with
/// [`Rsc7::detect`](super::envelope::Rsc7::detect).
pub(super) const DAT_RESOURCE_MAGIC: u32 = 0x37435352;

pub(super) const DAT_RESOURCE_HEADER_SIZE: usize = 16;

/// A flag word per heap giving its page sizes and counts, virtual then physical.
///
/// ⚠ Each flag word's top four bits are the version again, split across the
/// two words, so a header whose words disagree with `version` is not the same
/// file under another name.
#[record(size = 0x08)]
pub struct DatResourceInfo {
    pub r#virtual: u32,
    pub physical: u32,
}

/// The sixteen bytes every RSC7 file starts with: magic, version and each heap's page shape.
#[record(size = 0x10)]
pub struct DatResourceFileHeader {
    pub magic: u32,
    pub version: u32,
    pub info: DatResourceInfo,
}

impl DatResourceFileHeader {
    pub fn new(version: u32, virtual_flags: u32, physical_flags: u32) -> Self {
        Self {
            magic: DAT_RESOURCE_MAGIC,
            version,
            info: DatResourceInfo {
                r#virtual: virtual_flags,
                physical: physical_flags,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn write_to(&self, output: &mut Vec<u8>) {
        let mut raw = <Raw<Self> as Plain>::zeroed();
        raw.magic = self.magic;
        raw.version = self.version;
        raw.info.r#virtual = self.info.r#virtual;
        raw.info.physical = self.info.physical;
        output.extend_from_slice(Plain::as_bytes(&raw));
    }

    /// The header a file starts with, refused unless it carries the magic.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let Some(raw) = Raw::<Self>::read_from(data) else {
            return Err(IoError::DataTooSmall {
                expected: DAT_RESOURCE_HEADER_SIZE,
                actual: data.len(),
            });
        };
        if raw.magic != DAT_RESOURCE_MAGIC {
            return Err(IoError::InvalidMagic { found: raw.magic });
        }
        Ok(Self::new(
            raw.version,
            raw.info.r#virtual,
            raw.info.physical,
        ))
    }

    // Assumes GTA V: an RSC7 header never says which game wrote it.
    pub fn validate(&self, resource_type: ResourceType) -> Result<()> {
        self.validate_for(GTA5, resource_type)
    }

    pub fn validate_for(&self, title: TitleId, resource_type: ResourceType) -> Result<()> {
        validate_version(title, resource_type, self.version).map_err(|e| {
            IoError::UnsupportedVersionDetailed {
                found: self.version,
                resource_type: resource_type.extension().to_uppercase(),
                expected: match &e {
                    VersionError::UnsupportedVersion {
                        expected_versions, ..
                    } => expected_versions.clone(),
                    VersionError::UnknownResourceType { .. } => "unknown".to_string(),
                },
            }
        })
    }

    pub fn virtual_page_size(&self) -> u32 {
        let sizes = DatResourceSizes::from_flags(self.info.r#virtual);
        sizes.get_size(RSC_VIRTUAL_LEAF_SIZE)
    }

    pub fn physical_page_size(&self) -> u32 {
        let sizes = DatResourceSizes::from_flags(self.info.physical);
        sizes.get_size(RSC_PHYSICAL_LEAF_SIZE)
    }

    pub fn total_size(&self) -> u32 {
        self.virtual_page_size()
            .saturating_add(self.physical_page_size())
    }
}

/// The combined version byte, from the nibble each flag word carries in bits
/// 28-31: virtual in the high four bits, physical in the low four.
#[inline]
pub(super) fn extract_version(system_flags: u32, graphics_flags: u32) -> u8 {
    let virtual_version = ((system_flags >> 28) & 0xF) as u8;
    let physical_version = ((graphics_flags >> 28) & 0xF) as u8;
    (virtual_version << 4) | physical_version
}

#[inline]
pub(super) const fn extract_version_nibble(flags: u32) -> u8 {
    ((flags >> 28) & 0xF) as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatResourceFileType {
    Ypt,
    Ytd,
    Ydr,
    Ydd,
    Yft,
    Ybn,
    Yld,
    Ycd,
    Yed,
    Yfd,
    Ybd,
    Ynd,
    Ynv,
    Ysc,
    Yvr,
    Ywr,
    Custom(u32),
}

impl DatResourceFileType {
    pub fn version(self) -> u32 {
        match self {
            Self::Ypt => YPT_FILE_VERSION,
            Self::Ytd => YTD_FILE_VERSION,
            Self::Ydr => YDR_FILE_VERSION,
            Self::Ydd => YDD_FILE_VERSION,
            Self::Yft => YFT_FILE_VERSION,
            Self::Ybn => YBN_FILE_VERSION,
            Self::Yld => YLD_FILE_VERSION,
            Self::Ycd => YCD_FILE_VERSION,
            Self::Yed => YED_FILE_VERSION,
            Self::Yfd => YFD_FILE_VERSION,
            Self::Ybd => YBD_FILE_VERSION,
            Self::Ynd => YND_FILE_VERSION,
            Self::Ynv => YNV_FILE_VERSION,
            Self::Ysc => YSC_FILE_VERSION,
            Self::Yvr => YVR_FILE_VERSION,
            Self::Ywr => YWR_FILE_VERSION,
            Self::Custom(v) => v,
        }
    }

    pub fn version_for(self, generation: Generation) -> Option<u32> {
        match generation {
            Generation::Gen8 => Some(self.version()),
            Generation::Gen9 => match self {
                Self::Custom(v) => Some(v),
                _ => {
                    let rt = self.resource_type()?;
                    rt.gen9_versions().first().copied()
                }
            },
            _ => None,
        }
    }

    pub fn has_gen9_variant(self) -> bool {
        self.resource_type()
            .is_some_and(|rt| !rt.gen9_versions().is_empty())
    }

    pub fn effective_version_for(self, generation: Generation) -> u32 {
        self.version_for(generation)
            .unwrap_or_else(|| self.version())
    }

    // Version nibbles to stamp into the virtual and physical RSC7 flag words.
    pub fn version_nibbles(self) -> (u8, u8) {
        Self::nibbles_of(self.version())
    }

    /// Version nibbles for `generation`'s version of this type; the Gen9
    /// census confirmed the same derivation for every Gen9 version (YDR 159
    /// is `0x9F`, so `(0x9, 0xF)`).
    pub fn version_nibbles_for(self, generation: Generation) -> (u8, u8) {
        Self::nibbles_of(self.effective_version_for(generation))
    }

    fn nibbles_of(version: u32) -> (u8, u8) {
        (((version >> 4) & 0xF) as u8, (version & 0xF) as u8)
    }

    pub fn resource_type(self) -> Option<ResourceType> {
        Some(match self {
            Self::Ypt => ResourceType::Ypt,
            Self::Ytd => ResourceType::Ytd,
            Self::Ydr => ResourceType::Ydr,
            Self::Ydd => ResourceType::Ydd,
            Self::Yft => ResourceType::Yft,
            Self::Ybn => ResourceType::Ybn,
            Self::Yld => ResourceType::Yld,
            Self::Ycd => ResourceType::Ycd,
            Self::Yed => ResourceType::Yed,
            Self::Yfd => ResourceType::Yfd,
            Self::Ybd => ResourceType::Ybd,
            Self::Ynd => ResourceType::Ynd,
            Self::Ynv => ResourceType::Ynv,
            Self::Ysc => ResourceType::Ysc,
            Self::Yvr => ResourceType::Yvr,
            Self::Ywr => ResourceType::Ywr,
            Self::Custom(_) => return None,
        })
    }

    pub fn generation_of_version(self, version: u32) -> Option<Generation> {
        self.resource_type()?.generation_of_version(version)
    }
}

pub(super) struct DatResourceWriter;

impl DatResourceWriter {
    pub fn write(
        file_type: DatResourceFileType,
        virtual_data: Vec<u8>,
        physical_data: Vec<u8>,
        compression: Compression,
    ) -> Result<Vec<u8>> {
        let mut virtual_data = virtual_data;
        let mut physical_data = physical_data;

        let virtual_flags = dat_encode_resource_flags(virtual_data.len() as u32);
        let physical_flags = dat_encode_physical_flags(physical_data.len() as u32);

        let virtual_sizes = DatResourceSizes::from_flags(virtual_flags);
        let physical_sizes = DatResourceSizes::from_flags(physical_flags);
        let virtual_page_size = virtual_sizes.get_size(RSC_VIRTUAL_LEAF_SIZE) as usize;
        let physical_page_size = physical_sizes.get_size(RSC_PHYSICAL_LEAF_SIZE) as usize;

        if virtual_data.len() < virtual_page_size {
            virtual_data.resize(virtual_page_size, 0);
        }

        if physical_data.len() < physical_page_size {
            physical_data.resize(physical_page_size, 0);
        }

        let mut raw_data = virtual_data;
        raw_data.extend(physical_data);

        debug!(
            virtual_size = virtual_page_size,
            physical_size = physical_page_size,
            total_uncompressed = raw_data.len(),
            "Compressing resource data"
        );

        let compressed = dat_compress(&raw_data, compression)?;

        debug!(
            compressed_size = compressed.len(),
            ratio = format!(
                "{:.1}%",
                (compressed.len() as f64 / raw_data.len() as f64) * 100.0
            ),
            "Compression complete"
        );

        let header = DatResourceFileHeader::new(file_type.version(), virtual_flags, physical_flags);

        let mut output = Vec::with_capacity(DAT_RESOURCE_HEADER_SIZE + compressed.len());
        header.write_to(&mut output);
        output.extend(compressed);

        Ok(output)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn write_with_size_info_for(
        file_type: DatResourceFileType,
        generation: Generation,
        virtual_data: Vec<u8>,
        physical_data: Vec<u8>,
        virtual_size_info: &DatResourceSizeInfo,
        physical_size_info: Option<&DatResourceSizeInfo>,
        compression: Compression,
    ) -> Result<Vec<u8>> {
        let mut virtual_data = virtual_data;
        let mut physical_data = physical_data;
        let file_version = file_type.effective_version_for(generation);

        let (virtual_version, physical_version) = file_type.version_nibbles_for(generation);

        let virtual_sizes_from_info =
            DatResourceSizes::from_size_info(virtual_size_info, virtual_version);
        let virtual_page_from_info =
            virtual_sizes_from_info.get_size(RSC_VIRTUAL_LEAF_SIZE) as usize;

        let (virtual_sizes, virtual_flags) = if virtual_page_from_info >= virtual_data.len() {
            let flags = virtual_sizes_from_info.to_flags();
            (virtual_sizes_from_info, flags)
        } else {
            tracing::warn!(
                bucket_page_size = virtual_page_from_info,
                actual_data_len = virtual_data.len(),
                "Bucket-based virtual page size too small, falling back to heuristic encoding"
            );
            let flags = dat_encode_resource_flags_v2(
                virtual_data.len() as u32,
                RSC_VIRTUAL_LEAF_SIZE,
                virtual_version,
            );
            (DatResourceSizes::from_flags(flags), flags)
        };

        let (physical_sizes, physical_flags) = if let Some(phys_info) = physical_size_info {
            let sizes = DatResourceSizes::from_size_info(phys_info, physical_version);
            let page_size = sizes.get_size(RSC_PHYSICAL_LEAF_SIZE) as usize;
            if page_size >= physical_data.len() {
                let flags = sizes.to_flags();
                (sizes, flags)
            } else {
                tracing::warn!(
                    bucket_page_size = page_size,
                    actual_data_len = physical_data.len(),
                    "Bucket-based physical page size too small, falling back to heuristic encoding"
                );
                let flags = dat_encode_resource_flags_v2(
                    physical_data.len() as u32,
                    RSC_PHYSICAL_LEAF_SIZE,
                    physical_version,
                );
                (DatResourceSizes::from_flags(flags), flags)
            }
        } else if physical_data.is_empty() {
            (DatResourceSizes::default(), 0u32)
        } else {
            let flags = dat_encode_resource_flags_v2(
                physical_data.len() as u32,
                RSC_PHYSICAL_LEAF_SIZE,
                physical_version,
            );
            (DatResourceSizes::from_flags(flags), flags)
        };

        let virtual_page_size = virtual_sizes.get_size(RSC_VIRTUAL_LEAF_SIZE) as usize;
        let physical_page_size = physical_sizes.get_size(RSC_PHYSICAL_LEAF_SIZE) as usize;

        if virtual_data.len() < virtual_page_size {
            virtual_data.resize(virtual_page_size, 0);
        }
        if physical_data.len() < physical_page_size {
            physical_data.resize(physical_page_size, 0);
        }

        let mut raw_data = virtual_data;
        raw_data.extend(physical_data);

        debug!(
            virtual_size = virtual_page_size,
            physical_size = physical_page_size,
            virtual_flags = format!("{:#010X}", virtual_flags),
            physical_flags = format!("{:#010X}", physical_flags),
            total_uncompressed = raw_data.len(),
            "Compressing resource data (bucket-based)"
        );

        let compressed = dat_compress(&raw_data, compression)?;

        debug!(
            compressed_size = compressed.len(),
            ratio = format!(
                "{:.1}%",
                (compressed.len() as f64 / raw_data.len() as f64) * 100.0
            ),
            "Compression complete"
        );

        let header = DatResourceFileHeader::new(file_version, virtual_flags, physical_flags);

        let mut output = Vec::with_capacity(DAT_RESOURCE_HEADER_SIZE + compressed.len());
        header.write_to(&mut output);
        output.extend(compressed);

        Ok(output)
    }
}

pub(super) struct DatResourceContents {
    pub header: DatResourceFileHeader,
    pub data: Vec<u8>,
}

pub(super) struct DatResourceReader;

impl DatResourceReader {
    pub fn read(data: &[u8]) -> Result<DatResourceContents> {
        let header = DatResourceFileHeader::parse(data)?;

        debug!(
            version = header.version,
            virtual_flags = format!("{:#X}", header.info.r#virtual),
            physical_flags = format!("{:#X}", header.info.physical),
            "Parsed RSC7 header"
        );

        let compressed = &data[DAT_RESOURCE_HEADER_SIZE..];
        let decompressed = dat_decompress_expecting(compressed, header.total_size() as usize)?;

        debug!(
            compressed = compressed.len(),
            decompressed = decompressed.len(),
            "Decompression complete"
        );

        Ok(DatResourceContents {
            header,
            data: decompressed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rsc7_header(version: u32) -> [u8; 16] {
        let mut data = [0u8; 16];
        data[0..4].copy_from_slice(&DAT_RESOURCE_MAGIC.to_le_bytes());
        data[4..8].copy_from_slice(&version.to_le_bytes());
        data[8..12].copy_from_slice(&1u32.to_le_bytes());
        data[12..16].copy_from_slice(&0u32.to_le_bytes());
        data
    }

    #[test]
    fn test_parse_valid_ypt_header() {
        let data = make_rsc7_header(YPT_FILE_VERSION);

        let header = DatResourceFileHeader::parse(&data).unwrap();
        assert_eq!(header.magic, DAT_RESOURCE_MAGIC);
        assert_eq!(header.version, YPT_FILE_VERSION);
        header.validate(ResourceType::Ypt).unwrap();
    }

    #[test]
    fn test_parse_invalid_magic() {
        let mut data = make_rsc7_header(YPT_FILE_VERSION);
        data[0..4].copy_from_slice(&0u32.to_le_bytes());

        let result = DatResourceFileHeader::parse(&data);
        assert!(matches!(result, Err(IoError::InvalidMagic { .. })));
    }

    #[test]
    fn test_parse_wrong_version() {
        let unsupported_version = 99u32;
        let data = make_rsc7_header(unsupported_version);

        let header = DatResourceFileHeader::parse(&data).unwrap();
        let result = header.validate(ResourceType::Ypt);
        assert!(matches!(
            result,
            Err(IoError::UnsupportedVersionDetailed { found, .. }) if found == unsupported_version
        ));
    }

    #[test]
    fn gen9_ypt_version_validates() {
        let data = make_rsc7_header(71);
        let header = DatResourceFileHeader::parse(&data).unwrap();
        assert!(header.validate(ResourceType::Ypt).is_ok());
    }

    #[test]
    fn write_version_follows_the_requested_generation() {
        for (ft, gen8, gen9) in [
            (DatResourceFileType::Ytd, 13u32, 5u32),
            (DatResourceFileType::Ydr, 165, 159),
            (DatResourceFileType::Ydd, 165, 159),
            (DatResourceFileType::Yft, 162, 171),
            (DatResourceFileType::Ypt, 68, 71),
        ] {
            assert_eq!(ft.version_for(Generation::Gen8), Some(gen8));
            assert_eq!(ft.version_for(Generation::Gen9), Some(gen9));
            assert_eq!(ft.effective_version_for(Generation::Gen9), gen9);
            assert!(ft.has_gen9_variant());
        }
    }

    #[test]
    fn formats_identical_across_generations_have_no_gen9_version() {
        for ft in [
            DatResourceFileType::Ybn,
            DatResourceFileType::Ycd,
            DatResourceFileType::Yld,
        ] {
            assert!(!ft.has_gen9_variant());
            assert_eq!(ft.version_for(Generation::Gen9), None);
            assert_eq!(ft.effective_version_for(Generation::Gen9), ft.version());
        }
    }

    #[test]
    fn gen9_nibbles_are_derived_not_special_cased() {
        assert_eq!(
            DatResourceFileType::Ydr.version_nibbles_for(Generation::Gen9),
            (0x9, 0xF)
        );
        assert_eq!(
            DatResourceFileType::Yft.version_nibbles_for(Generation::Gen9),
            (0xA, 0xB)
        );
        assert_eq!(
            DatResourceFileType::Ytd.version_nibbles_for(Generation::Gen9),
            (0x0, 0x5)
        );
        assert_eq!(
            DatResourceFileType::Ypt.version_nibbles_for(Generation::Gen9),
            (0x4, 0x7)
        );
        assert_eq!(
            DatResourceFileType::Ydr.version_nibbles_for(Generation::Gen8),
            DatResourceFileType::Ydr.version_nibbles()
        );
    }

    #[test]
    fn custom_versions_are_left_alone_by_both_generations() {
        let ft = DatResourceFileType::Custom(1234);
        assert_eq!(ft.version_for(Generation::Gen8), Some(1234));
        assert_eq!(ft.version_for(Generation::Gen9), Some(1234));
    }

    #[test]
    fn test_validate_with_resource_type() {
        let ypt_data = make_rsc7_header(YPT_FILE_VERSION);
        let header = DatResourceFileHeader::parse(&ypt_data).unwrap();
        assert!(header.validate(ResourceType::Ypt).is_ok());
        assert!(header.validate(ResourceType::Ytd).is_err());

        let ytd_data = make_rsc7_header(YTD_FILE_VERSION);
        let ytd_header = DatResourceFileHeader::parse(&ytd_data).unwrap();
        assert!(ytd_header.validate(ResourceType::Ytd).is_ok());
        assert!(ytd_header.validate(ResourceType::Ytd).is_ok());
    }

    #[test]
    fn test_parse_data_too_small() {
        let data = [0x52, 0x53, 0x43, 0x37];

        let result = DatResourceFileHeader::parse(&data);
        assert!(matches!(result, Err(IoError::DataTooSmall { .. })));
    }

    #[test]
    fn test_file_type_versions() {
        assert_eq!(DatResourceFileType::Ypt.version(), YPT_FILE_VERSION);
        assert_eq!(DatResourceFileType::Ytd.version(), YTD_FILE_VERSION);
        assert_eq!(DatResourceFileType::Ydr.version(), YDR_FILE_VERSION);
        assert_eq!(DatResourceFileType::Ydd.version(), YDD_FILE_VERSION);
        assert_eq!(DatResourceFileType::Ybn.version(), YBN_FILE_VERSION);
        assert_eq!(DatResourceFileType::Yld.version(), YLD_FILE_VERSION);
        assert_eq!(DatResourceFileType::Custom(7).version(), 7);

        assert_eq!(YLD_FILE_VERSION, 8);
        assert_eq!(
            u32::from(super::extract_version(0x0000_0000, 0x8000_0000)),
            YLD_FILE_VERSION
        );
        assert_eq!(
            DatResourceFileType::Ydd.version(),
            DatResourceFileType::Ydr.version()
        );
    }

    #[test]
    fn test_version_nibbles_match_derived_values() {
        for (file_type, expected) in [
            (DatResourceFileType::Ydr, (0xA, 0x5)),
            (DatResourceFileType::Ydd, (0xA, 0x5)),
            (DatResourceFileType::Yld, (0x0, 0x8)),
            (DatResourceFileType::Ytd, (0x0, 0xD)),
        ] {
            assert_eq!(
                file_type.version_nibbles(),
                expected,
                "{file_type:?} nibbles disagree with the table in write_with_size_info"
            );
        }
    }

    #[test]
    fn test_yld_writes_empty_physical_heap() {
        let size_info = DatResourceSizeInfo::default();
        let bytes = DatResourceWriter::write_with_size_info_for(
            DatResourceFileType::Yld,
            Generation::Gen8,
            vec![0u8; 16],
            Vec::new(),
            &size_info,
            None,
            Compression::fast(),
        )
        .expect("write header");

        let header = DatResourceFileHeader::parse(&bytes).expect("parse header");
        assert_eq!(header.version, YLD_FILE_VERSION);
        assert_eq!(super::extract_version_nibble(header.info.r#virtual), 0x0);
        assert_eq!(header.info.physical, 0, "no physical pages were allocated");
    }

    const SURVEYED_NIBBLES: &[(DatResourceFileType, u8, u8)] = &[
        (DatResourceFileType::Ytd, 0x0, 0xD),
        (DatResourceFileType::Ybn, 0x2, 0xB),
        (DatResourceFileType::Ypt, 0x4, 0x4),
        (DatResourceFileType::Yft, 0xA, 0x2),
        (DatResourceFileType::Ydr, 0xA, 0x5),
        (DatResourceFileType::Ydd, 0xA, 0x5),
        (DatResourceFileType::Yld, 0x0, 0x8),
    ];

    #[test]
    fn version_nibbles_match_retail_files() {
        for &(file_type, virt, phys) in SURVEYED_NIBBLES {
            assert_eq!(
                file_type.version_nibbles(),
                (virt, phys),
                "{file_type:?} nibbles"
            );
        }
    }

    #[test]
    fn written_headers_carry_the_surveyed_nibbles() {
        let size_info = DatResourceSizeInfo::default();
        for &(file_type, virt, phys) in SURVEYED_NIBBLES {
            let bytes = DatResourceWriter::write_with_size_info_for(
                file_type,
                Generation::Gen8,
                vec![0u8; 16],
                vec![0u8; 16],
                &size_info,
                None,
                Compression::fast(),
            )
            .expect("write header");
            let header = DatResourceFileHeader::parse(&bytes).expect("parse header");
            assert_eq!(
                super::extract_version_nibble(header.info.r#virtual),
                virt,
                "{file_type:?} virtual nibble"
            );
            assert_eq!(
                super::extract_version_nibble(header.info.physical),
                phys,
                "{file_type:?} physical nibble"
            );
            assert_eq!(
                u32::from(super::extract_version(
                    header.info.r#virtual,
                    header.info.physical
                )),
                file_type.version(),
                "{file_type:?} version round-trip"
            );
        }
    }

    #[test]
    fn test_extract_version_nibble() {
        assert_eq!(super::extract_version_nibble(0x00000000), 0x0);
        assert_eq!(super::extract_version_nibble(0xD0000000), 0xD);
        assert_eq!(super::extract_version_nibble(0xF0000000), 0xF);
        assert_eq!(super::extract_version_nibble(0xD0040000), 0xD);
        assert_eq!(super::extract_version_nibble(0x00060001), 0x0);

        assert_eq!(super::extract_version_nibble(0xD0FFFFFF), 0xD);
        assert_eq!(super::extract_version_nibble(0xABCDEF12), 0xA);
    }

    #[test]
    fn test_extract_version() {
        assert_eq!(super::extract_version(0xD0040000, 0x00060001), 0xD0);

        assert_eq!(super::extract_version(0xA0000000, 0xB0000000), 0xAB);

        assert_eq!(super::extract_version(0x00000000, 0x00000000), 0x00);

        assert_eq!(super::extract_version(0xF0000000, 0xF0000000), 0xFF);

        assert_eq!(super::extract_version(0x00040000, 0xD0060001), 0x0D);
    }
}
