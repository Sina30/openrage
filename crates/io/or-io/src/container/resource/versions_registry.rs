//! The RSC7 versions each resource format is read and written at, keyed by
//! title. Gen8 and Gen9 versions of a format are registered together, since
//! one install contains both.
//!
//! ⚠ Every number is GTA V's. A loose file carries a version and never says
//! which game wrote it, so asking "is version 68 supported" has already
//! assumed a title; [`DatResourceFileHeader::validate`] assumes [`GTA5`].
//!
//! [`DatResourceFileHeader::validate`]: super::header::DatResourceFileHeader::validate
//!
//! # Example
//!
//! ```rust
//! use or_io::Generation;
//! use or_io::container::resource::versions_registry::{ResourceType, VersionRegistry, GTA5};
//!
//! let registry = VersionRegistry::default();
//!
//! // Both generations of a format are readable.
//! assert!(registry.is_supported(GTA5, ResourceType::Ypt, 68)); // Gen8 (Legacy)
//! assert!(registry.is_supported(GTA5, ResourceType::Ypt, 71)); // Gen9 (Enhanced)
//!
//! // Which layout to parse with comes from the file's own version.
//! assert_eq!(
//!     ResourceType::Ypt.generation_of_version(71),
//!     Some(Generation::Gen9)
//! );
//!
//! // A version from neither generation is refused, with the accepted ones named.
//! registry.validate(GTA5, ResourceType::Ypt, 99).unwrap_err();
//!
//! // A title nobody registered has no versions at all, rather than GTA V's.
//! assert!(!registry.is_supported("rdr2", ResourceType::Ypt, 68));
//! ```

use std::collections::BTreeSet;
use std::fmt;

use crate::Generation;

use super::versions::{
    YBD_FILE_VERSION, YBN_FILE_VERSION, YCD_FILE_VERSION, YDD_FILE_VERSION, YDD_FILE_VERSION_GEN9,
    YDR_FILE_VERSION, YDR_FILE_VERSION_GEN8_ALT, YDR_FILE_VERSION_GEN9, YDR_FILE_VERSION_GEN9_ALT,
    YED_FILE_VERSION, YFD_FILE_VERSION, YFT_FILE_VERSION, YFT_FILE_VERSION_GEN8_ALT,
    YFT_FILE_VERSION_GEN9, YLD_FILE_VERSION, YND_FILE_VERSION, YNV_FILE_VERSION, YPM_FILE_VERSION,
    YPT_FILE_VERSION, YPT_FILE_VERSION_GEN9, YSC_FILE_VERSION, YSC_FILE_VERSION_GEN8_ALT,
    YTD_FILE_VERSION, YTD_FILE_VERSION_GEN9, YVR_FILE_VERSION, YWR_FILE_VERSION,
};

pub type TitleId = &'static str;

pub const GTA5: TitleId = "gta5";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResourceType {
    Ypt,

    Ytd,

    Ydr,

    Ydd,

    Yft,

    Ybn,

    Ycd,

    Yld,

    Yed,

    Ypm,

    Yfd,

    Ybd,

    Ynd,

    Ynv,

    Ysc,

    Yvr,

    Ywr,
}

impl ResourceType {
    pub const ALL: [ResourceType; 17] = [
        Self::Ypt,
        Self::Ytd,
        Self::Ydr,
        Self::Ydd,
        Self::Yft,
        Self::Ybn,
        Self::Ycd,
        Self::Yld,
        Self::Yed,
        Self::Ypm,
        Self::Yfd,
        Self::Ybd,
        Self::Ynd,
        Self::Ynv,
        Self::Ysc,
        Self::Yvr,
        Self::Ywr,
    ];

    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Ypt => &["ypt"],
            Self::Ytd => &["ytd"],
            Self::Ydr => &["ydr"],
            Self::Ydd => &["ydd"],
            Self::Yft => &["yft"],
            Self::Ybn => &["ybn"],
            Self::Ycd => &["ycd"],
            Self::Yld => &["yld"],
            Self::Yed => &["yed"],
            Self::Ypm => &["ypm"],
            Self::Yfd => &["yfd"],
            Self::Ybd => &["ybd"],
            Self::Ynd => &["ynd"],
            Self::Ynv => &["ynv"],
            Self::Ysc => &["ysc"],
            Self::Yvr => &["yvr"],
            Self::Ywr => &["ywr"],
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Ypt => "Particle effect library",
            Self::Ytd => "Texture dictionary",
            Self::Ydr => "Drawable",
            Self::Ydd => "Drawable dictionary",
            Self::Yft => "Fragment",
            Self::Ybn => "Collision bounds",
            Self::Ycd => "Clip dictionary",
            Self::Yld => "Cloth",
            Self::Yed => "Expression dictionary",
            Self::Ypm => "Parameterised motion",
            Self::Yfd => "Frame filter dictionary",
            Self::Ybd => "Bound dictionary",
            Self::Ynd => "Path node region",
            Self::Ynv => "Navigation mesh",
            Self::Ysc => "Script program",
            Self::Yvr => "Vehicle recording",
            Self::Ywr => "Waypoint recording",
        }
    }

    pub const fn has_gen9_variant(self) -> bool {
        !self.gen9_versions().is_empty()
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Ypt => "ypt",
            Self::Ytd => "ytd",
            Self::Ydr => "ydr",
            Self::Ydd => "ydd",
            Self::Yft => "yft",
            Self::Ybn => "ybn",
            Self::Ycd => "ycd",
            Self::Yld => "yld",
            Self::Yed => "yed",
            Self::Ypm => "ypm",
            Self::Yfd => "yfd",
            Self::Ybd => "ybd",
            Self::Ynd => "ynd",
            Self::Ynv => "ynv",
            Self::Ysc => "ysc",
            Self::Yvr => "yvr",
            Self::Ywr => "ywr",
        }
    }

    pub const fn canonical_version(self) -> u32 {
        match self {
            Self::Ypt => YPT_FILE_VERSION,
            Self::Ytd => YTD_FILE_VERSION,
            Self::Ydr => YDR_FILE_VERSION,
            Self::Ydd => YDD_FILE_VERSION,
            Self::Yft => YFT_FILE_VERSION,
            Self::Ybn => YBN_FILE_VERSION,
            Self::Ycd => YCD_FILE_VERSION,
            Self::Yld => YLD_FILE_VERSION,
            Self::Yed => YED_FILE_VERSION,
            Self::Ypm => YPM_FILE_VERSION,
            Self::Yfd => YFD_FILE_VERSION,
            Self::Ybd => YBD_FILE_VERSION,
            Self::Ynd => YND_FILE_VERSION,
            Self::Ynv => YNV_FILE_VERSION,
            Self::Ysc => YSC_FILE_VERSION,
            Self::Yvr => YVR_FILE_VERSION,
            Self::Ywr => YWR_FILE_VERSION,
        }
    }

    pub const fn rage_type_name(self) -> &'static str {
        match self {
            Self::Ypt => "ptxFxList",
            Self::Ytd => "pgDictionary<grcTexture>",
            Self::Ydr => "rmcDrawable",
            Self::Ydd => "pgDictionary<rmcDrawable>",
            Self::Yft => "fragType",
            Self::Ybn => "phBound",
            Self::Ycd => "crClipDictionary",
            Self::Yld => "phVerletCloth",
            Self::Yed => "crExpressions",
            Self::Ypm => "crpmParameterizedMotionDictionary",
            Self::Yfd => "pgDictionary<crFrameFilter>",
            Self::Ybd => "pgDictionary<phBound>",
            Self::Ysc => "scrProgram",
            Self::Yvr => "CVehicleRecording",
            Self::Ywr => "fwWaypointRecordingRoute",
            // ⚠ The two names in this column not confirmed against an engine
            // header. `CPathRegion` and `CNavMesh` are what GTAMods and
            // CodeWalker between them describe; the *layouts* below them are
            // measured off the corpus, not read off a struct.
            Self::Ynd => "CPathRegion",
            Self::Ynv => "CNavMesh",
        }
    }

    pub const fn gen8_alt_versions(self) -> &'static [u32] {
        match self {
            Self::Yft => &[YFT_FILE_VERSION_GEN8_ALT],
            Self::Ydr => &[YDR_FILE_VERSION_GEN8_ALT],
            Self::Ysc => &[YSC_FILE_VERSION_GEN8_ALT],
            _ => &[],
        }
    }

    pub const fn gen9_versions(self) -> &'static [u32] {
        match self {
            Self::Ypt => &[YPT_FILE_VERSION_GEN9],
            Self::Ytd => &[YTD_FILE_VERSION_GEN9],
            Self::Ydr => &[YDR_FILE_VERSION_GEN9, YDR_FILE_VERSION_GEN9_ALT],
            Self::Ydd => &[YDD_FILE_VERSION_GEN9],
            Self::Yft => &[YFT_FILE_VERSION_GEN9],
            // ⚠ `.ysc` belongs here and not beside `.ytd`. Both installs ship
            // both of its versions, so its two numbers are alternates of one
            // another rather than one per generation — see
            // [`YSC_FILE_VERSION_GEN8_ALT`].
            Self::Ybn
            | Self::Ycd
            | Self::Yld
            | Self::Yed
            | Self::Ypm
            | Self::Yfd
            | Self::Ybd
            | Self::Ysc
            | Self::Ynd
            | Self::Ynv
            | Self::Yvr
            | Self::Ywr => &[],
        }
    }

    pub fn generation_of_version(self, version: u32) -> Option<Generation> {
        if version == self.canonical_version() || self.gen8_alt_versions().contains(&version) {
            return Some(Generation::Gen8);
        }
        if self.gen9_versions().contains(&version) {
            return Some(Generation::Gen9);
        }
        None
    }

    pub fn read_versions(self) -> Vec<u32> {
        let mut all: BTreeSet<u32> = BTreeSet::new();
        all.insert(self.canonical_version());
        all.extend(self.gen8_alt_versions().iter().copied());
        all.extend(self.gen9_versions().iter().copied());
        all.into_iter().collect()
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "ypt" => Some(Self::Ypt),
            "ytd" => Some(Self::Ytd),
            "ydr" => Some(Self::Ydr),
            "ydd" => Some(Self::Ydd),
            "yft" => Some(Self::Yft),
            "ybn" => Some(Self::Ybn),
            "ycd" => Some(Self::Ycd),
            "yld" => Some(Self::Yld),
            "yed" => Some(Self::Yed),
            "ypm" => Some(Self::Ypm),
            "yfd" => Some(Self::Yfd),
            "ybd" => Some(Self::Ybd),
            "ynd" => Some(Self::Ynd),
            "ynv" => Some(Self::Ynv),
            "ysc" => Some(Self::Ysc),
            "yvr" => Some(Self::Yvr),
            "ywr" => Some(Self::Ywr),
            _ => None,
        }
    }
}

impl fmt::Display for ResourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({})",
            self.extension().to_uppercase(),
            self.rage_type_name()
        )
    }
}

#[derive(Debug, Clone)]
pub struct VersionSupport {
    pub title: TitleId,

    pub resource_type: ResourceType,

    pub supported_read_versions: BTreeSet<u32>,

    pub write_version: u32,
}

impl VersionSupport {
    pub fn single(title: TitleId, resource_type: ResourceType, version: u32) -> Self {
        let mut supported = BTreeSet::new();
        supported.insert(version);
        Self {
            title,
            resource_type,
            supported_read_versions: supported,
            write_version: version,
        }
    }

    pub fn multiple(
        title: TitleId,
        resource_type: ResourceType,
        read_versions: impl IntoIterator<Item = u32>,
        write_version: u32,
    ) -> Self {
        Self {
            title,
            resource_type,
            supported_read_versions: read_versions.into_iter().collect(),
            write_version,
        }
    }

    pub fn is_read_supported(&self, version: u32) -> bool {
        self.supported_read_versions.contains(&version)
    }

    pub fn add_read_version(&mut self, version: u32) {
        self.supported_read_versions.insert(version);
    }

    pub fn supported_versions_string(&self) -> String {
        let versions: Vec<_> = self
            .supported_read_versions
            .iter()
            .map(|v| v.to_string())
            .collect();
        match versions.len() {
            0 => "none".to_string(),
            1 => versions[0].clone(),
            2 => format!("{} or {}", versions[0], versions[1]),
            _ => {
                let last = versions.last().unwrap();
                let rest = &versions[..versions.len() - 1];
                format!("{}, or {}", rest.join(", "), last)
            }
        }
    }
}

/// Registry of supported versions for RAGE resource files, keyed by title.
///
/// Two titles are free to ship the same version number meaning different
/// things, so every lookup names a [`TitleId`].
///
/// # Example
///
/// Accepting a version beyond the two generations registered by default:
///
/// ```rust
/// use or_io::container::resource::versions_registry::{ResourceType, VersionRegistry, GTA5};
///
/// let mut registry = VersionRegistry::default();
/// registry.add_read_version(GTA5, ResourceType::Ypt, 99);
/// assert!(registry.is_supported(GTA5, ResourceType::Ypt, 99));
/// ```
///
/// Note that accepting a *version* is not the same as knowing its *layout*:
/// [`ResourceType::generation_of_version`] would still return `None` for 99,
/// and readers pick their struct layout from that.
#[derive(Debug, Clone)]
pub struct VersionRegistry {
    entries: Vec<VersionSupport>,
}

impl VersionRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn register(&mut self, support: VersionSupport) {
        self.entries
            .retain(|e| e.title != support.title || e.resource_type != support.resource_type);
        self.entries.push(support);
    }

    pub fn titles(&self) -> Vec<TitleId> {
        let mut out: Vec<TitleId> = Vec::new();
        for entry in &self.entries {
            if !out.contains(&entry.title) {
                out.push(entry.title);
            }
        }
        out
    }

    pub fn get(&self, title: TitleId, resource_type: ResourceType) -> Option<&VersionSupport> {
        self.entries
            .iter()
            .find(|e| e.title == title && e.resource_type == resource_type)
    }

    pub fn get_mut(
        &mut self,
        title: TitleId,
        resource_type: ResourceType,
    ) -> Option<&mut VersionSupport> {
        self.entries
            .iter_mut()
            .find(|e| e.title == title && e.resource_type == resource_type)
    }

    pub fn is_supported(&self, title: TitleId, resource_type: ResourceType, version: u32) -> bool {
        self.get(title, resource_type)
            .map(|s| s.is_read_supported(version))
            .unwrap_or(false)
    }

    pub fn add_read_version(&mut self, title: TitleId, resource_type: ResourceType, version: u32) {
        if let Some(support) = self.get_mut(title, resource_type) {
            support.add_read_version(version);
        }
    }

    pub fn write_version(&self, title: TitleId, resource_type: ResourceType) -> Option<u32> {
        self.get(title, resource_type).map(|s| s.write_version)
    }

    pub fn validate(
        &self,
        title: TitleId,
        resource_type: ResourceType,
        version: u32,
    ) -> Result<(), VersionError> {
        match self.get(title, resource_type) {
            None => Err(VersionError::UnknownResourceType {
                title,
                resource_type,
            }),
            Some(support) if support.is_read_supported(version) => Ok(()),
            Some(support) => Err(VersionError::UnsupportedVersion {
                title,
                resource_type,
                found_version: version,
                expected_versions: support.supported_versions_string(),
            }),
        }
    }
}

impl Default for VersionRegistry {
    fn default() -> Self {
        let mut registry = Self::new();

        for resource_type in ResourceType::ALL {
            registry.register(VersionSupport::multiple(
                GTA5,
                resource_type,
                resource_type.read_versions(),
                resource_type.canonical_version(),
            ));
        }

        registry
    }
}

#[derive(Debug, Clone)]
pub enum VersionError {
    UnknownResourceType {
        title: TitleId,
        resource_type: ResourceType,
    },

    UnsupportedVersion {
        title: TitleId,
        resource_type: ResourceType,
        found_version: u32,
        expected_versions: String,
    },
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownResourceType {
                title,
                resource_type,
            } => {
                write!(f, "Unknown resource type for {}: {}", title, resource_type)
            }
            Self::UnsupportedVersion {
                resource_type,
                found_version,
                expected_versions,
                ..
            } => {
                write!(
                    f,
                    "Unsupported {} version: found {}, expected {}",
                    resource_type.extension().to_uppercase(),
                    found_version,
                    expected_versions
                )
            }
        }
    }
}

impl std::error::Error for VersionError {}

use std::sync::LazyLock;

pub static DEFAULT_REGISTRY: LazyLock<VersionRegistry> = LazyLock::new(VersionRegistry::default);

pub fn is_version_supported(title: TitleId, resource_type: ResourceType, version: u32) -> bool {
    DEFAULT_REGISTRY.is_supported(title, resource_type, version)
}

pub fn validate_version(
    title: TitleId,
    resource_type: ResourceType,
    version: u32,
) -> Result<(), VersionError> {
    DEFAULT_REGISTRY.validate(title, resource_type, version)
}

pub fn canonical_write_version(title: TitleId, resource_type: ResourceType) -> u32 {
    DEFAULT_REGISTRY
        .write_version(title, resource_type)
        .unwrap_or_else(|| resource_type.canonical_version())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_type_from_extension() {
        assert_eq!(ResourceType::from_extension("ypt"), Some(ResourceType::Ypt));
        assert_eq!(ResourceType::from_extension("YPT"), Some(ResourceType::Ypt));
        assert_eq!(ResourceType::from_extension("unknown"), None);
    }

    #[test]
    fn test_default_registry_supports_canonical_versions() {
        let registry = VersionRegistry::default();

        assert!(registry.is_supported(GTA5, ResourceType::Ypt, 68));
        assert!(registry.is_supported(GTA5, ResourceType::Ytd, 13));
        assert!(registry.is_supported(GTA5, ResourceType::Ydr, 165));

        assert!(!registry.is_supported(GTA5, ResourceType::Ypt, 0));
        assert!(!registry.is_supported(GTA5, ResourceType::Ypt, 69));
    }

    #[test]
    fn the_default_registry_holds_exactly_one_title() {
        let registry = VersionRegistry::default();
        assert_eq!(registry.titles(), vec![GTA5]);
        for rt in ResourceType::ALL {
            assert!(
                registry.get(GTA5, rt).is_some(),
                "{rt} should be registered for {GTA5}"
            );
        }
    }

    #[test]
    fn default_registry_reads_gen9_versions() {
        let registry = VersionRegistry::default();

        assert!(registry.is_supported(GTA5, ResourceType::Ypt, 71));
        assert!(registry.is_supported(GTA5, ResourceType::Ytd, 5));
        assert!(registry.is_supported(GTA5, ResourceType::Ydr, 159));
        assert!(registry.is_supported(GTA5, ResourceType::Ydr, 154));
        assert!(registry.is_supported(GTA5, ResourceType::Ydd, 159));
        assert!(registry.is_supported(GTA5, ResourceType::Yft, 171));

        assert!(registry.is_supported(GTA5, ResourceType::Ypt, 68));
        assert!(registry.is_supported(GTA5, ResourceType::Yft, 162));
    }

    #[test]
    fn gen9_registration_does_not_change_the_write_version() {
        let registry = VersionRegistry::default();
        assert_eq!(registry.write_version(GTA5, ResourceType::Ydr), Some(165));
        assert_eq!(registry.write_version(GTA5, ResourceType::Ytd), Some(13));
        assert_eq!(registry.write_version(GTA5, ResourceType::Yft), Some(162));
        assert_eq!(registry.write_version(GTA5, ResourceType::Ypt), Some(68));
    }

    #[test]
    fn formats_gen9_left_alone_have_no_extra_read_versions() {
        let registry = VersionRegistry::default();
        for rt in [
            ResourceType::Ybn,
            ResourceType::Ycd,
            ResourceType::Yld,
            ResourceType::Yed,
            ResourceType::Ypm,
            ResourceType::Yfd,
        ] {
            assert!(
                rt.gen9_versions().is_empty(),
                "{rt} should have no Gen9 set"
            );
            let support = registry.get(GTA5, rt).expect("registered");
            let mut expected = vec![rt.canonical_version()];
            expected.extend(rt.gen8_alt_versions().iter().copied());
            expected.sort_unstable();
            assert_eq!(
                support
                    .supported_read_versions
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
                expected,
                "{rt} should read its Gen8 version and its Gen8 alternates, nothing else"
            );
        }
        assert_eq!(ResourceType::Ybn.canonical_version(), 43);
        assert_eq!(ResourceType::Ycd.canonical_version(), 46);
    }

    #[test]
    fn older_gen8_revisions_shipped_in_retail_stay_readable() {
        let registry = VersionRegistry::default();
        assert!(registry.is_supported(GTA5, ResourceType::Yft, 160));
        assert_eq!(
            ResourceType::Yft.generation_of_version(160),
            Some(Generation::Gen8)
        );
    }

    #[test]
    fn generation_is_resolved_from_the_files_own_version() {
        assert_eq!(
            ResourceType::Ydr.generation_of_version(165),
            Some(Generation::Gen8)
        );
        assert_eq!(
            ResourceType::Ydr.generation_of_version(159),
            Some(Generation::Gen9)
        );
        assert_eq!(
            ResourceType::Ydr.generation_of_version(154),
            Some(Generation::Gen9)
        );
        assert_eq!(ResourceType::Ydr.generation_of_version(99), None);

        assert_eq!(
            ResourceType::Ytd.generation_of_version(13),
            Some(Generation::Gen8)
        );
        assert_eq!(
            ResourceType::Ytd.generation_of_version(5),
            Some(Generation::Gen9)
        );

        assert_eq!(
            ResourceType::Ybn.generation_of_version(43),
            Some(Generation::Gen8)
        );
        assert_eq!(ResourceType::Ybn.generation_of_version(159), None);
    }

    #[test]
    fn test_registry_add_read_version() {
        let mut registry = VersionRegistry::default();

        assert!(!registry.is_supported(GTA5, ResourceType::Ypt, 99));
        registry.add_read_version(GTA5, ResourceType::Ypt, 99);

        assert!(registry.is_supported(GTA5, ResourceType::Ypt, 68));
        assert!(registry.is_supported(GTA5, ResourceType::Ypt, 99));

        registry.add_read_version("gta5 ", ResourceType::Ypt, 100);
        assert!(!registry.is_supported(GTA5, ResourceType::Ypt, 100));
        assert_eq!(registry.titles(), vec![GTA5]);
    }

    #[test]
    fn test_validate_failure() {
        let registry = VersionRegistry::default();
        let result = registry.validate(GTA5, ResourceType::Ypt, 99);

        assert!(result.is_err());
        let error = result.unwrap_err();

        match error {
            VersionError::UnsupportedVersion {
                title,
                resource_type,
                found_version,
                expected_versions,
            } => {
                assert_eq!(title, GTA5);
                assert_eq!(resource_type, ResourceType::Ypt);
                assert_eq!(found_version, 99);
                assert_eq!(expected_versions, "68 or 71");
            }
            _ => panic!("Expected UnsupportedVersion error"),
        }
    }

    #[test]
    fn an_unregistered_title_is_unknown_rather_than_wrong() {
        let registry = VersionRegistry::default();
        let err = registry
            .validate("rdr2", ResourceType::Ypt, 68)
            .expect_err("rdr2 has no table");
        match err {
            VersionError::UnknownResourceType {
                title,
                resource_type,
            } => {
                assert_eq!(title, "rdr2");
                assert_eq!(resource_type, ResourceType::Ypt);
            }
            other => panic!("expected UnknownResourceType, got {other:?}"),
        }
        assert!(registry.validate(GTA5, ResourceType::Ypt, 68).is_ok());
    }

    #[test]
    fn test_version_support_string_formatting() {
        let single = VersionSupport::single(GTA5, ResourceType::Ypt, 68);
        assert_eq!(single.supported_versions_string(), "68");

        let double = VersionSupport::multiple(GTA5, ResourceType::Ypt, [68, 71], 68);
        assert_eq!(double.supported_versions_string(), "68 or 71");

        let triple = VersionSupport::multiple(GTA5, ResourceType::Ypt, [65, 68, 71], 68);
        assert_eq!(triple.supported_versions_string(), "65, 68, or 71");
    }

    #[test]
    fn test_global_registry_functions() {
        assert!(is_version_supported(GTA5, ResourceType::Ypt, 68));
        assert!(is_version_supported(GTA5, ResourceType::Ypt, 71));
        assert!(!is_version_supported(GTA5, ResourceType::Ypt, 99));
        assert!(validate_version(GTA5, ResourceType::Ypt, 68).is_ok());
        assert_eq!(canonical_write_version(GTA5, ResourceType::Ypt), 68);
    }
}
