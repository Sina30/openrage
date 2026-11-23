//! A file opened without naming its type: the path's extension picks the
//! format, the header's RSC7 version picks the target, and the pair picks the
//! owned root.

use std::path::Path;

use crate::container::resource::{ResourceEnvelope, Rsc7};
use crate::decl::{kind_name, File, HasOrigin, Heap, Origin, Repair, Resource};
use crate::error::Error;
use crate::gta5::formats::{self, Format, FORMATS};
use crate::gta5::gen8::GrcTexture;
#[cfg(feature = "gen9")]
use crate::gta5::gen9::GrcTextureG9;
use crate::gta5::text::{Gxt2, GXT2_MAGIC};
use crate::gta5::{
    CNavMesh, CPathRegion, CVehicleRecording, CharacterCloth, CrClipDictionary,
    CrExpressionsDictionary, CrFrameFilterDictionary, FwWaypointRecordingRoute, GtaDrawable,
    GtaFragType, PgDictionary, PhBoundAny, PtxFxList,
};
use crate::Generation;

/// How an owned root's file becomes an [`Asset`].
type Load = fn(&[u8]) -> Result<Asset, Error>;

macro_rules! assets {
    (
        $($(#[doc = $doc:literal])* $(#[cfg($cfg:meta)])? $variant:ident($root:ty),)*
        ; $($(#[doc = $fdoc:literal])* $(#[cfg($fcfg:meta)])? $fvariant:ident($froot:ty),)*
    ) => {
        /// A file [`open`] or [`from_bytes`] read, as the owned root its header names.
        ///
        /// # Example
        ///
        /// ```
        /// use or_io::decl::Open;
        /// use or_io::gta5::*;
        /// use or_io::Asset;
        ///
        /// # fn main() -> Result<(), or_io::Error> {
        /// # let filter = CrFrameFilterMultiWeight {
        /// #     base: CrFrameFilter { r#type: Open::new(EFrameFilterType::MultiWeight), ..Default::default() },
        /// #     weights: vec![1.0],
        /// #     ..Default::default()
        /// # };
        /// # let bytes = CrFrameFilterDictionary {
        /// #     codes: vec![0x100],
        /// #     entries: vec![Some(Box::new(CrFrameFilterAny::CrFrameFilterMultiWeight(filter)))],
        /// #     ..Default::default()
        /// # }
        /// # .to_bytes()?;
        /// let Asset::FrameFilterDict(mut dict) = or_io::from_bytes(&bytes)? else {
        ///     unreachable!()
        /// };
        /// if let Some(filter) = dict.entries[0].as_multi_weight_mut() {
        ///     filter.weights[0] = 0.5;
        /// }
        /// assert_eq!(dict.entries[0].as_multi_weight().unwrap().weights, [0.5]);
        /// # Ok(())
        /// # }
        /// ```
        #[derive(Debug, Clone, PartialEq)]
        #[non_exhaustive]
        // A `.ybn` root is the bound itself, held by value like every other root.
        #[allow(clippy::large_enum_variant)]
        pub enum Asset {
            $($(#[doc = $doc])* $(#[cfg($cfg)])? $variant($root),)*
            $($(#[doc = $fdoc])* $(#[cfg($fcfg)])? $fvariant($froot),)*
        }

        /// Every owned root, by the extension and target it opens.
        const OWNED: &[(&str, Generation, Load)] = &[
            $($(#[cfg($cfg)])? (
                <$root>::EXT,
                <$root as Resource>::TARGET,
                |bytes| <$root as File>::from_bytes(bytes).map(Asset::$variant),
            ),)*
        ];

        impl Asset {
            /// The extension its files carry, lowercase and without the dot.
            pub fn ext(&self) -> &'static str {
                match self {
                    $($(#[cfg($cfg)])? Asset::$variant(_) => <$root>::EXT,)*
                    $($(#[cfg($fcfg)])? Asset::$fvariant(_) => <$froot>::EXTENSION,)*
                }
            }

            /// The target whose layout it saves at; `Gen8` for a format
            /// `OWNED` does not list, which ships the same layout at every target.
            pub fn target(&self) -> Generation {
                match self {
                    $($(#[cfg($cfg)])? Asset::$variant(_) => <$root as Resource>::TARGET,)*
                    $($(#[cfg($fcfg)])? Asset::$fvariant(_) => Generation::Gen8,)*
                }
            }

            /// The whole file; refused if the game would misread it.
            pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
                match self {
                    $($(#[cfg($cfg)])? Asset::$variant(v) => v.to_bytes(),)*
                    $($(#[cfg($fcfg)])? Asset::$fvariant(v) => v.to_bytes(),)*
                }
            }

            /// [`Asset::to_bytes`], rewriting a count the file states against its run as `repair` allows, and each count it rewrote.
            pub fn to_bytes_repairing(&self, repair: Repair) -> Result<(Vec<u8>, Vec<String>), Error> {
                match self {
                    $($(#[cfg($cfg)])? Asset::$variant(v) => v.to_bytes_repairing(repair),)*
                    $($(#[cfg($fcfg)])? Asset::$fvariant(v) => v.to_bytes_repairing(repair),)*
                }
            }

            /// What the value holds on the heap; see [`Heap`].
            pub fn heap_bytes(&self) -> usize {
                match self {
                    $($(#[cfg($cfg)])? Asset::$variant(v) => v.heap_bytes(),)*
                    $($(#[cfg($fcfg)])? Asset::$fvariant(v) => v.heap_bytes(),)*
                }
            }
        }

        impl HasOrigin for Asset {
            fn origin(&self) -> &Origin {
                match self {
                    $($(#[cfg($cfg)])? Asset::$variant(v) => v.origin(),)*
                    $($(#[cfg($fcfg)])? Asset::$fvariant(v) => v.origin(),)*
                }
            }

            fn origin_mut(&mut self) -> &mut Origin {
                match self {
                    $($(#[cfg($cfg)])? Asset::$variant(v) => v.origin_mut(),)*
                    $($(#[cfg($fcfg)])? Asset::$fvariant(v) => v.origin_mut(),)*
                }
            }
        }
    };
}

assets! {
    /// A `.yfd`.
    FrameFilterDict(CrFrameFilterDictionary),
    /// A `.yed`.
    ExpressionDict(CrExpressionsDictionary),
    /// A `.ynd`.
    PathRegion(CPathRegion),
    /// A `.ynv`.
    NavMesh(CNavMesh),
    /// A `.yvr`.
    VehicleRecording(CVehicleRecording),
    /// A `.ywr`.
    WaypointRecording(FwWaypointRecordingRoute),
    /// A Legacy `.ytd`.
    TextureDict(PgDictionary<GrcTexture>),
    /// A `.ybn`.
    Bounds(PhBoundAny),
    /// A `.ybd`.
    BoundsDict(PgDictionary<PhBoundAny>),
    /// An Enhanced `.ytd`, at the Gen9 layout.
    #[cfg(feature = "gen9")]
    TextureDictGen9(PgDictionary<GrcTextureG9>),
    /// A `.yld`.
    ClothDict(PgDictionary<CharacterCloth>),
    /// A `.ycd`.
    Clips(CrClipDictionary),
    /// A Legacy `.ydr`.
    Drawable(GtaDrawable),
    /// An Enhanced `.ydr`, at the Gen9 layout.
    #[cfg(feature = "gen9")]
    DrawableGen9(GtaDrawable<crate::decl::Gen9>),
    /// A Legacy `.ydd`.
    DrawableDict(PgDictionary<GtaDrawable>),
    /// An Enhanced `.ydd`, at the Gen9 layout.
    #[cfg(feature = "gen9")]
    DrawableDictGen9(PgDictionary<GtaDrawable<crate::decl::Gen9>>),
    /// A Legacy `.yft`.
    Fragment(GtaFragType),
    /// An Enhanced `.yft`, at the Gen9 layout.
    #[cfg(feature = "gen9")]
    FragmentGen9(GtaFragType<crate::decl::Gen9>),
    /// A Legacy `.ypt`.
    Particles(PtxFxList),
    /// An Enhanced `.ypt`, at the Gen9 layout.
    #[cfg(feature = "gen9")]
    ParticlesGen9(PtxFxList<crate::decl::Gen9>),
    // Owned files that are not RSC7 resources, which `OWNED` does not list.
    ;
    /// A `.gxt2`.
    TextTable(Gxt2),
}

/// The file at `path`, as the owned root its extension and header name.
pub fn open(path: impl AsRef<Path>) -> Result<Asset, Error> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| Error::FileReadFailed {
        path: path.to_path_buf(),
        source,
    })?;
    let ext = path.extension().and_then(|e| e.to_str());
    let mut asset = match ext {
        Some(e) if e.eq_ignore_ascii_case(Gxt2::EXTENSION) => {
            Gxt2::from_bytes(&bytes).map(Asset::TextTable)?
        }
        _ => read(&bytes, ext.and_then(formats::format))?,
    };
    *asset.origin_mut() = Origin::new(path);
    Ok(asset)
}

/// A whole file, as the owned root its header names; a version two formats share is refused.
pub fn from_bytes(bytes: &[u8]) -> Result<Asset, Error> {
    if bytes.starts_with(&GXT2_MAGIC.to_le_bytes()) {
        return Gxt2::from_bytes(bytes).map(Asset::TextTable);
    }
    read(bytes, None)
}

/// ⚠ A declared extension decides the format: an Enhanced install ships a
/// `.ytd` at version 2, which is `.ynv`'s, and it is not a navmesh.
fn read(bytes: &[u8], named: Option<&'static Format>) -> Result<Asset, Error> {
    let version = Rsc7::peek(bytes)
        .map_err(|e| Error::NotAResource(e.to_string()))?
        .version();
    let format = match named {
        Some(format) => format,
        None => only_format_at(version)?,
    };
    let target = format.target_of_version(version)?;
    let load = OWNED
        .iter()
        .find(|(ext, t, _)| *ext == format.ext && *t == target)
        .map(|(_, _, load)| load)
        .ok_or_else(|| Error::NotOwned {
            ext: format.ext,
            kind: kind_name(format.ext),
            edition: target.edition_label(),
        })?;
    load(bytes)
}

fn only_format_at(version: u32) -> Result<&'static Format, Error> {
    let found: Vec<&'static Format> = FORMATS
        .iter()
        .filter(|f| f.target_of_version(version).is_ok())
        .collect();
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(Error::UnknownVersion { version }),
        [first @ .., last] => Err(Error::Ambiguous {
            version,
            formats: format!(
                "{} or {}",
                first
                    .iter()
                    .map(|f| kind_name(f.ext))
                    .collect::<Vec<_>>()
                    .join(", "),
                kind_name(last.ext)
            ),
        }),
    }
}
