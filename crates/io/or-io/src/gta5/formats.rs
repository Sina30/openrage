//! Which declared root each format opens at, for which targets, and the read
//! and canonical save every declared format shares.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::container::paging::PgRscBuilder;
use crate::container::resource::{Authoring, DatResourceFileType, Rsc7};
use crate::decl::{describe, root, Described, Reach};
use crate::desc::value::{BlobCodecs, ElementTypes};
use crate::desc::{Image, Pack, Value};
use crate::error::Error;
use crate::write::BlobWriters;
use crate::Generation;

use super::{
    bounds, bounds_dict, clip_dict, cloth_dict, drawable, drawable_codecs, drawable_dict,
    expression_dict, fragment, frame_filter_dict, navmesh, particle, path_region, shared,
    texture_dict, vehicle_recording, waypoint_recording,
};

/// One target's root type, and the RSC7 version its files carry.
pub struct Root {
    pub target: Generation,
    pub version: u32,
    reach: fn(&mut Reach),
    name: fn() -> String,
}

const fn root_for<R: Described>(target: Generation, version: u32) -> Root {
    Root {
        target,
        version,
        reach: root::<R>,
        name: R::name,
    }
}

/// One declared format.
pub struct Format {
    pub ext: &'static str,
    /// How a user names one: "a fragment".
    pub noun: &'static str,
    pub file_type: DatResourceFileType,
    /// A root per target whose files it reads. A target that ships the other's
    /// layout under the same version is not listed twice.
    roots: &'static [Root],
    validate: fn(Image<'_>, usize, Generation) -> Result<(), Refusal>,
    /// Who reads and writes the allocations whose regions are computed.
    codecs: Option<fn() -> (BlobCodecs, BlobWriters)>,
}

/// A Legacy `.ydr` root, which reaches a shader's texture reference too: the codec follows it, and no field describes it.
const fn ydr_gen8(version: u32) -> Root {
    use crate::decl::{Gen8, Raw};
    Root {
        target: Generation::Gen8,
        version,
        reach: |r| {
            root::<Raw<drawable::GtaDrawable<Gen8>>>(r);
            root::<Raw<drawable::gen8::GrcTextureReference>>(r);
        },
        name: <Raw<drawable::GtaDrawable<Gen8>> as Described>::name,
    }
}

/// An Enhanced `.ydr` root, and the texture reference its codec follows.
#[cfg(feature = "gen9")]
const fn ydr_gen9(version: u32) -> Root {
    use crate::decl::{Gen9, Raw};
    Root {
        target: Generation::Gen9,
        version,
        reach: |r| {
            root::<Raw<drawable::GtaDrawable<Gen9>>>(r);
            root::<Raw<drawable::gen9::GrcTextureReferenceG9>>(r);
        },
        name: <Raw<drawable::GtaDrawable<Gen9>> as Described>::name,
    }
}

/// A root whose drawables' shaders reach a texture reference, which only the codec follows.
const fn with_references<R: Described, Ref: Described>(target: Generation, version: u32) -> Root {
    Root {
        target,
        version,
        reach: |r| {
            root::<R>(r);
            root::<Ref>(r);
        },
        name: R::name,
    }
}

fn drawable_codecs() -> (BlobCodecs, BlobWriters) {
    (drawable_codecs::readers(), drawable_codecs::writers())
}

fn fragment_codecs() -> (BlobCodecs, BlobWriters) {
    (fragment::readers(), fragment::writers())
}

fn bounds_codecs() -> (BlobCodecs, BlobWriters) {
    (bounds::readers(), bounds::writers())
}

/// Every format declared so far.
///
/// ⚠ **A file's target is its RSC7 version, never the install it came from.**
/// An Enhanced install ships `.yfd` at the Legacy version and three `.ytd` at
/// Legacy's 13 among 93,076 at 5, and one `.ytd` at a version 2 nothing
/// declares — measured by `openrage-cli versions` (`container::resource::versions`).
pub static FORMATS: &[Format] = &[
    Format {
        ext: "yfd",
        noun: "a frame filter dictionary",
        file_type: DatResourceFileType::Yfd,
        codecs: None,
        roots: &[root_for::<
            crate::decl::Raw<frame_filter_dict::CrFrameFilterDictionary>,
        >(Generation::Gen8, 4)],
        validate: |image, at, _| frame_filter_dict::validate(image, at).map_err(Into::into),
    },
    Format {
        ext: "ybn",
        noun: "a bounds file",
        file_type: DatResourceFileType::Ybn,
        codecs: Some(bounds_codecs),
        roots: &[root_for::<crate::decl::Raw<bounds::PhBoundAny>>(
            Generation::Gen8,
            43,
        )],
        validate: |image, at, _| bounds::validate(image, at).map_err(Into::into),
    },
    Format {
        ext: "ybd",
        noun: "a bounds dictionary",
        file_type: DatResourceFileType::Ybd,
        codecs: Some(bounds_codecs),
        roots: &[root_for::<
            crate::decl::Raw<shared::PgDictionary<bounds::PhBoundAny>>,
        >(Generation::Gen8, 43)],
        validate: |image, at, _| bounds_dict::validate(image, at).map_err(Into::into),
    },
    Format {
        ext: "yld",
        noun: "a cloth dictionary",
        file_type: DatResourceFileType::Yld,
        codecs: Some(bounds_codecs),
        roots: &[root_for::<
            crate::decl::Raw<shared::PgDictionary<cloth_dict::CharacterCloth>>,
        >(Generation::Gen8, 8)],
        validate: |image, at, _| cloth_dict::validate(image, at).map_err(Into::into),
    },
    Format {
        ext: "yed",
        noun: "an expression dictionary",
        file_type: DatResourceFileType::Yed,
        codecs: None,
        roots: &[root_for::<
            crate::decl::Raw<expression_dict::CrExpressionsDictionary>,
        >(Generation::Gen8, 25)],
        validate: |image, at, _| expression_dict::validate(image, at).map_err(Into::into),
    },
    Format {
        ext: "ynd",
        noun: "a path node region",
        file_type: DatResourceFileType::Ynd,
        codecs: None,
        roots: &[root_for::<crate::decl::Raw<path_region::CPathRegion>>(
            Generation::Gen8,
            1,
        )],
        validate: |image, at, _| path_region::validate(image, at).map_err(Into::into),
    },
    Format {
        ext: "ynv",
        noun: "a navmesh",
        file_type: DatResourceFileType::Ynv,
        codecs: None,
        roots: &[root_for::<crate::decl::Raw<navmesh::CNavMesh>>(
            Generation::Gen8,
            2,
        )],
        validate: |image, at, _| navmesh::validate(image, at).map_err(Into::into),
    },
    Format {
        ext: "yvr",
        noun: "a vehicle recording",
        file_type: DatResourceFileType::Yvr,
        codecs: None,
        roots: &[root_for::<
            crate::decl::Raw<vehicle_recording::CVehicleRecording>,
        >(Generation::Gen8, 1)],
        validate: |image, at, _| vehicle_recording::validate(image, at).map_err(Into::into),
    },
    Format {
        ext: "ywr",
        noun: "a waypoint recording",
        file_type: DatResourceFileType::Ywr,
        codecs: None,
        roots: &[root_for::<
            crate::decl::Raw<waypoint_recording::FwWaypointRecordingRoute>,
        >(Generation::Gen8, 1)],
        validate: |image, at, _| waypoint_recording::validate(image, at).map_err(Into::into),
    },
    Format {
        ext: "ydd",
        noun: "a drawable dictionary",
        file_type: DatResourceFileType::Ydd,
        codecs: Some(drawable_codecs),
        roots: &[
            with_references::<
                crate::decl::Raw<shared::PgDictionary<drawable::GtaDrawable<crate::decl::Gen8>>>,
                crate::decl::Raw<drawable::gen8::GrcTextureReference>,
            >(Generation::Gen8, 165),
            #[cfg(feature = "gen9")]
            with_references::<
                crate::decl::Raw<shared::PgDictionary<drawable::GtaDrawable<crate::decl::Gen9>>>,
                crate::decl::Raw<drawable::gen9::GrcTextureReferenceG9>,
            >(Generation::Gen9, 159),
        ],
        validate: |image, at, target| {
            drawable_dict::validate(image, at, target).map_err(Into::into)
        },
    },
    Format {
        ext: "yft",
        noun: "a fragment",
        file_type: DatResourceFileType::Yft,
        codecs: Some(fragment_codecs),
        roots: &[
            with_references::<
                crate::decl::Raw<fragment::GtaFragType<crate::decl::Gen8>>,
                crate::decl::Raw<drawable::gen8::GrcTextureReference>,
            >(Generation::Gen8, 162),
            with_references::<
                crate::decl::Raw<fragment::GtaFragType<crate::decl::Gen8>>,
                crate::decl::Raw<drawable::gen8::GrcTextureReference>,
            >(Generation::Gen8, 160),
            #[cfg(feature = "gen9")]
            with_references::<
                crate::decl::Raw<fragment::GtaFragType<crate::decl::Gen9>>,
                crate::decl::Raw<drawable::gen9::GrcTextureReferenceG9>,
            >(Generation::Gen9, 171),
        ],
        validate: |image, at, target| fragment::validate(image, at, target).map_err(Into::into),
    },
    Format {
        ext: "ydr",
        noun: "a drawable",
        file_type: DatResourceFileType::Ydr,
        codecs: Some(drawable_codecs),
        roots: &[
            ydr_gen8(165),
            ydr_gen8(164),
            #[cfg(feature = "gen9")]
            ydr_gen9(159),
            #[cfg(feature = "gen9")]
            ydr_gen9(154),
        ],
        validate: |image, at, target| drawable::validate(image, at, target).map_err(Into::into),
    },
    Format {
        ext: "ycd",
        noun: "a clip dictionary",
        file_type: DatResourceFileType::Ycd,
        codecs: None,
        roots: &[root_for::<crate::decl::Raw<clip_dict::CrClipDictionary>>(
            Generation::Gen8,
            46,
        )],
        validate: |image, at, _| clip_dict::validate(image, at).map_err(Into::into),
    },
    Format {
        ext: "ypt",
        noun: "a particle effects file",
        file_type: DatResourceFileType::Ypt,
        codecs: Some(drawable_codecs),
        roots: &[
            with_references::<
                crate::decl::Raw<particle::PtxFxList<crate::decl::Gen8>>,
                crate::decl::Raw<drawable::gen8::GrcTextureReference>,
            >(Generation::Gen8, 68),
            #[cfg(feature = "gen9")]
            with_references::<
                crate::decl::Raw<particle::PtxFxList<crate::decl::Gen9>>,
                crate::decl::Raw<drawable::gen9::GrcTextureReferenceG9>,
            >(Generation::Gen9, 71),
        ],
        validate: |image, at, target| particle::validate(image, at, target).map_err(Into::into),
    },
    Format {
        ext: "ytd",
        noun: "a texture dictionary",
        file_type: DatResourceFileType::Ytd,
        codecs: None,
        roots: &[
            root_for::<crate::decl::Raw<shared::PgDictionary<texture_dict::gen8::GrcTexture>>>(
                Generation::Gen8,
                13,
            ),
            #[cfg(feature = "gen9")]
            root_for::<crate::decl::Raw<shared::PgDictionary<texture_dict::gen9::GrcTextureG9>>>(
                Generation::Gen9,
                5,
            ),
        ],
        validate: |image, at, target| texture_dict::validate(image, at, target).map_err(Into::into),
    },
];

/// Versions retail files ship at whose layout the bytes do not fix, each with what is missing.
///
/// ⚠ Measured by `a_ybd_at_40_or_42_misreads_its_geometries_at_43s_layout`
/// over the three Legacy `.ybd` at 40 and 42; their dictionaries, composites
/// and boxes read at 43's layout, and every geometry does not.
const REFUSED: &[(&str, u32, &str)] = &[
    (
        "ybd",
        42,
        "its phBoundGeometry is 0x10 shorter than version 43's: \
         `num_convex_hull_vertices` sits two bytes earlier, where 43 keeps \
         `num_per_vertex_attribs`, and the 0x30 bytes 43 holds from `octant_vert_counts` to `material_ids` \
         are 0x20, with the vertex and polygon counts 8 bytes earlier. Every \
         vertex-attribute and octant pointer at 42 is null, so nothing fixes where \
         `num_per_vertex_attribs`, `octant_vert_counts` and `octant_verts` sit",
    ),
    (
        "ybd",
        40,
        "its phBoundGeometry is version 42's with 0x10 more after the bound header, \
         holding 0.04, 0.04, 0.04 and a `w` of bits 1 in every geometry, which \
         nothing names. It leaves `num_per_vertex_attribs`, `octant_vert_counts` and \
         `octant_verts` as unfixed as 42 does",
    ),
];

/// The declared format with this extension.
pub fn format(ext: &str) -> Option<&'static Format> {
    FORMATS.iter().find(|f| f.ext.eq_ignore_ascii_case(ext))
}

/// Why a format's validation refused a built image: its own error type, boxed.
type Refusal = Box<dyn std::error::Error + Send + Sync>;

type Cache = Mutex<BTreeMap<(&'static str, &'static str), Arc<Pack>>>;

impl Format {
    /// The targets this format is declared for.
    pub fn targets(&self) -> impl Iterator<Item = Generation> + '_ {
        self.roots.iter().map(|r| r.target)
    }

    /// The target whose layout a file at this RSC7 version holds.
    pub fn target_of_version(&self, version: u32) -> Result<Generation, Error> {
        if let Some((.., why)) = REFUSED
            .iter()
            .find(|(ext, v, _)| *ext == self.ext && *v == version)
        {
            return Err(Error::Read {
                ext: self.ext,
                why: format!("RSC7 version {version} is refused: {why}"),
            });
        }
        self.roots
            .iter()
            .find(|r| r.version == version)
            .map(|r| r.target)
            .ok_or(Error::Version {
                ext: self.ext,
                version,
            })
    }

    fn root(&self, target: Generation) -> Result<&Root, Error> {
        self.roots
            .iter()
            .find(|r| r.target == target)
            .ok_or_else(|| Error::NoTarget {
                ext: self.ext,
                edition: target.edition_label().to_string(),
            })
    }

    /// The root type's name in `target`'s description.
    pub fn root_name(&self, target: Generation) -> Result<String, Error> {
        Ok((self.root(target)?.name)())
    }

    /// Every type this format reaches for `target`.
    pub fn reach(&self, target: Generation) -> Result<Reach, Error> {
        let root = self.root(target)?;
        let mut reach = Reach::new(target.slug());
        (root.reach)(&mut reach);
        Ok(reach)
    }

    /// The built-in description of this format for `target`, built once.
    pub fn description(&self, target: Generation) -> Result<Arc<Pack>, Error> {
        let root = self.root(target)?;
        static CACHE: OnceLock<Cache> = OnceLock::new();
        let cache = CACHE.get_or_init(Default::default);
        let key = (self.ext, target.slug());
        if let Some(pack) = cache.lock().unwrap_or_else(|p| p.into_inner()).get(&key) {
            return Ok(pack.clone());
        }
        let pack = describe("gta5", target.slug(), &[root.reach]).map_err(|problems| {
            Error::Description {
                ext: self.ext,
                problems: problems.join("; "),
            }
        })?;
        let pack = Arc::new(pack);
        cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, pack.clone());
        Ok(pack)
    }

    /// The fields this format's codecs read and write, which a comparison of two trees must not treat as lost.
    pub fn covered(&self) -> crate::write::compare::CoveredSites {
        let mut covered = crate::write::compare::CoveredSites::new();
        if let Some(codecs) = self.codecs {
            let (readers, writers) = codecs();
            for (ty, field) in readers.sites() {
                covered = covered.at(ty, field);
            }
            for (ty, field) in readers.arrivals() {
                covered = covered.reached_through(ty, field);
            }
            for (ty, field) in writers.sibling_sites() {
                covered = covered.at(ty, field);
            }
        }
        covered
    }

    /// `data` read as this format's owned root where one is declared, and otherwise whole into the record tree; the target its RSC7 version names.
    ///
    /// ⚠ The one dispatch a build's reload and the install sweep of `openrage-cli search --verify` share.
    pub fn open_declared(&self, data: &[u8]) -> Result<Generation, Error> {
        use super::{
            CPathRegion, CharacterCloth, CrExpressionsDictionary, CrFrameFilterDictionary,
            PgDictionary, PhBoundAny,
        };
        use crate::container::resource::ResourceEnvelope;

        fn owned<T: super::File>(data: &[u8]) -> Result<(), Error> {
            T::from_bytes(data).map(drop)
        }

        let version = Rsc7::peek(data)
            .map_err(|e| Error::NotAResource(e.to_string()))?
            .version();
        let target = self.target_of_version(version)?;
        match (self.ext, target) {
            ("yfd", _) => owned::<CrFrameFilterDictionary>(data)?,
            ("yed", _) => owned::<CrExpressionsDictionary>(data)?,
            ("ynd", _) => owned::<CPathRegion>(data)?,
            ("ybn", _) => owned::<PhBoundAny>(data)?,
            ("ybd", _) => owned::<PgDictionary<PhBoundAny>>(data)?,
            ("yld", _) => owned::<PgDictionary<CharacterCloth>>(data)?,
            #[cfg(feature = "gen9")]
            ("ytd", t) if t.is_gen9() => owned::<PgDictionary<super::gen9::GrcTextureG9>>(data)?,
            ("ytd", _) => owned::<PgDictionary<super::gen8::GrcTexture>>(data)?,
            (_, target) => {
                let opened = Rsc7::open(data)?;
                let pages = crate::decl::Aligned::from_bytes(&opened.data);
                let image = Image::new(pages.bytes(), opened.virtual_size as usize);
                self.read(target, image).map(drop)?
            }
        }
        Ok(target)
    }

    /// A decompressed resource read whole into the record tree, rooted at offset zero.
    pub fn read(&self, target: Generation, image: Image<'_>) -> Result<Value, Error> {
        let pack = self.description(target)?;
        let name = crate::desc::TypeName(crate::decl::intern(&self.root_name(target)?));
        let read = match self.codecs {
            Some(codecs) => crate::desc::value::read_using(
                &pack,
                name,
                image,
                0,
                &ElementTypes::new(),
                &codecs().0,
            ),
            None => crate::desc::value::read(&pack, name, image, 0),
        };
        read.map_err(|e| Error::Read {
            ext: self.ext,
            why: e.to_string(),
        })
    }

    /// The canonical form of `value` as a whole RSC7 file, refused if the game would misread it.
    pub fn save(&self, target: Generation, value: &Value) -> Result<Vec<u8>, Error> {
        self.save_reporting(target, value).map(|(bytes, _)| bytes)
    }

    /// [`Format::save`], and what the writer dropped or carried as a number to write it.
    pub fn save_reporting(
        &self,
        target: Generation,
        value: &Value,
    ) -> Result<(Vec<u8>, crate::write::BuildReport), Error> {
        let pack = self.description(target)?;
        let save = |why: String| Error::Save { ext: self.ext, why };
        let mut builder = PgRscBuilder::new_bucketed_default().targeting(target);
        let writers = self.codecs.map(|c| c().1).unwrap_or_default();
        let (root, report) =
            crate::write::build_into_reporting_with(builder.heap_mut(), &pack, value, &writers)
                .map_err(|e| save(e.to_string()))?;
        builder.ready_build();
        let virtual_sizes = builder.virtual_size_info();
        let physical_sizes = builder.physical_size_info();
        let (virtual_data, physical_data) = builder.finish();

        let at = root
            .checked_sub(crate::constants::FIXED_VIRTUAL_BASE)
            .ok_or_else(|| save(format!("the root landed at {root:#x}")))?;
        let mut whole = virtual_data.clone();
        whole.extend_from_slice(&physical_data);
        let aligned = crate::decl::Aligned::from_bytes(&whole);
        (self.validate)(
            Image::new(aligned.bytes(), virtual_data.len()),
            at as usize,
            target,
        )
        .map_err(|reason| Error::Refused {
            ext: self.ext,
            reason: reason.into(),
        })?;

        let mut spec =
            Authoring::new(self.file_type, flate2::Compression::default()).for_generation(target);
        if let Some(ref sizes) = virtual_sizes {
            spec = spec.with_sizes(sizes, physical_sizes.as_ref());
        }
        Rsc7::author(spec, virtual_data, physical_data)
            .map(|bytes| (bytes, report))
            .map_err(|e| save(e.to_string()))
    }
}
