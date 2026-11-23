//! Files opened without naming their type, with invented content:
//! `or_io::open` and `or_io::from_bytes` return the owned root the header
//! names at the target its version names, and a file type outside RSC7 opens
//! and saves through the calls a resource uses. Needs no game file.

use std::path::{Path, PathBuf};

use or_io::container::resource::{Authoring, DatResourceFileType, Rsc7};
use or_io::decl::Origin;
use or_io::gta5::expression_dict::key_of;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::*;
use or_io::{Asset, Error};

fn dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("owned_asset");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn filters() -> CrFrameFilterDictionary {
    let filter = CrFrameFilterMultiWeight {
        base: CrFrameFilter {
            r#type: or_io::decl::Open::new(EFrameFilterType::MultiWeight),
            ..Default::default()
        },
        track_id_indices: vec![TrackIdIndex::new(0, 10, 0)],
        weights: vec![0.5],
        ..Default::default()
    };
    CrFrameFilterDictionary {
        codes: vec![0x100],
        entries: vec![Some(Box::new(CrFrameFilterAny::CrFrameFilterMultiWeight(
            filter,
        )))],
        ..Default::default()
    }
}

fn expressions() -> CrExpressionsDictionary {
    let name = "pack:/wheel.expr";
    CrExpressionsDictionary {
        codes: vec![key_of(name)],
        entries: vec![Some(Box::new(CrExpressions {
            name: AtString::new(name),
            ..Default::default()
        }))],
        ..Default::default()
    }
}

fn region() -> CPathRegion {
    CPathRegion {
        first_node: Some(Box::new(PgBasePageMap {
            subtype: 2,
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn textures() -> PgDictionary<GrcTexture> {
    let mut dict = PgDictionary::default();
    dict.insert(GrcTexture {
        name: "stone".into(),
        width: 4,
        height: 4,
        ..Default::default()
    });
    dict
}

/// A file that is only an RSC7 header at `version` and a page of zeros.
fn bare(version: u32) -> Vec<u8> {
    Rsc7::author(
        Authoring::new(
            DatResourceFileType::Custom(version),
            flate2::Compression::default(),
        ),
        vec![0; 0x2000],
        Vec::new(),
    )
    .unwrap()
}

fn written(name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn or_io_open_names_the_variant_its_header_holds() {
    let yfd = written("filters.yfd", &filters().to_bytes().unwrap());
    let yed = written("wheel.yed", &expressions().to_bytes().unwrap());
    let ynd = written("nodes10.ynd", &region().to_bytes().unwrap());
    let ytd = written("rocks.ytd", &textures().to_bytes().unwrap());
    // The header alone names a format whose version no other format ships.
    let unnamed = written("rocks.bin", &textures().to_bytes().unwrap());

    assert!(matches!(or_io::open(&yfd).unwrap(), Asset::FrameFilterDict(d) if d == filters()));
    assert!(matches!(or_io::open(&yed).unwrap(), Asset::ExpressionDict(d) if d == expressions()));
    assert!(matches!(or_io::open(&ynd).unwrap(), Asset::PathRegion(r) if r == region()));
    assert!(matches!(or_io::open(&ytd).unwrap(), Asset::TextureDict(d) if d == textures()));
    assert!(matches!(
        or_io::open(&unnamed).unwrap(),
        Asset::TextureDict(_)
    ));
    let bytes = std::fs::read(&yfd).unwrap();
    assert!(matches!(
        or_io::from_bytes(&bytes).unwrap(),
        Asset::FrameFilterDict(_)
    ));
}

#[test]
fn an_asset_opened_from_a_path_saves_back_to_it() {
    let path = written("saved_back.ytd", &textures().to_bytes().unwrap());
    let Asset::TextureDict(mut dict) = or_io::open(&path).unwrap() else {
        panic!("not a texture dictionary");
    };
    dict.get_mut("stone").unwrap().width = 8;
    dict.save().unwrap();
    let again = PgDictionary::<GrcTexture>::open(&path).unwrap();
    assert_eq!(again.get("stone").unwrap().width, 8);
}

#[test]
fn a_ypt_opens_owned_at_the_target_its_version_names() {
    let enhanced = or_io::open(written("enhanced.ypt", &bare(71))).unwrap();
    assert!(matches!(enhanced, Asset::ParticlesGen9(_)), "{enhanced:?}");
    let legacy = or_io::open(written("legacy.ypt", &bare(68))).unwrap();
    assert!(matches!(legacy, Asset::Particles(_)), "{legacy:?}");
    assert_eq!(
        or_io::from_bytes(&bare(71)).unwrap().target(),
        or_io::Generation::Gen9
    );
}

#[test]
fn a_version_two_formats_share_takes_its_extension() {
    let why = or_io::from_bytes(&bare(43)).unwrap_err().to_string();
    assert!(
        why.contains("version 43") && why.contains(".ybn") && !why.contains(".ycd"),
        "{why}"
    );
    let opened = or_io::open(written("bounds.ybd", &bare(43)));
    assert!(matches!(opened, Ok(Asset::BoundsDict(_))), "{opened:?}");
    let why = or_io::open(written("clips.ycd", &bare(43)))
        .unwrap_err()
        .to_string();
    assert!(why.contains(".ycd at RSC7 version 43"), "{why}");
}

#[test]
fn a_version_nothing_declares_is_named() {
    let why = or_io::from_bytes(&bare(250)).unwrap_err().to_string();
    assert!(why.contains("version 250"), "{why}");
}

#[test]
fn a_roots_extension_reads_unqualified_through_the_prelude() {
    assert_eq!(CrFrameFilterDictionary::EXT, "yfd");
    assert_eq!(<CrFrameFilterDictionary as File>::EXTENSION, "yfd");
}

/// A file type outside RSC7: a line of text per note.
#[derive(Debug, Default, PartialEq)]
struct Notes {
    lines: Vec<String>,
    origin: Origin,
}

impl HasOrigin for Notes {
    fn origin(&self) -> &Origin {
        &self.origin
    }
    fn origin_mut(&mut self) -> &mut Origin {
        &mut self.origin
    }
}

impl File for Notes {
    const EXTENSION: &'static str = "notes";

    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let text = std::str::from_utf8(bytes).map_err(|e| Error::InvalidData {
            context: e.to_string(),
        })?;
        Ok(Self {
            lines: text.lines().map(str::to_string).collect(),
            origin: Origin::default(),
        })
    }

    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        Ok(self
            .lines
            .iter()
            .flat_map(|l| format!("{l}\n").into_bytes())
            .collect())
    }
}

/// Saved to `name`, opened, edited by `edit`, saved in place and opened again.
fn round_trip<F: File + PartialEq + std::fmt::Debug>(value: F, name: &str, edit: fn(&mut F)) {
    let path = dir().join(name);
    value.save_as(&path).unwrap();
    let mut opened = F::open(&path).unwrap();
    assert_eq!(opened, value);
    edit(&mut opened);
    opened.save().unwrap();
    let again = F::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(again, opened);
    assert_ne!(again, value);
}

#[test]
fn a_file_type_outside_rsc7_opens_and_saves_through_the_same_calls() {
    let notes = Notes {
        lines: vec!["first".into()],
        ..Default::default()
    };
    round_trip(notes, "list.notes", |n| n.lines.push("second".into()));
    round_trip(filters(), "list.yfd", |d| d.codes[0] = 0x200);

    let why = Notes::default().save().unwrap_err().to_string();
    assert!(why.contains("save_as"), "{why}");
    let yfd = written("not_notes.yfd", &filters().to_bytes().unwrap());
    assert_eq!(
        Notes::open(yfd).unwrap_err().to_string(),
        "this is a frame filter dictionary (.yfd), not a .notes file"
    );
}
