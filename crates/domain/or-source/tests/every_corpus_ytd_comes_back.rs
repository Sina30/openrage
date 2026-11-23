//! Every `.ytd` in the corpus extracts to a source folder and compiles back to the same pixel bytes in every mip.

mod common;

use common::{compile, enhanced, legacy, mips};
use or_io::codec::texture::TextureSource;
use or_io::container::resource::Rsc7;
use or_io::container::ResourceEnvelope;
use or_io::gta5::formats::format;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::gen9::GrcTextureG9;
use or_io::Generation;
use or_source::asset::{sha256_hex, AssetManifest};

/// Every header word of a Legacy texture that a compile has to give back.
fn legacy_words(t: &GrcTexture) -> [u64; 15] {
    [
        t.width.into(),
        t.height.into(),
        t.depth.into(),
        t.format.into(),
        t.mip_count.into(),
        t.mip_stride.into(),
        t.info_bits.into(),
        t.resource_type_and_conversion_flags.into(),
        t.extra_flags.into(),
        t.handle_index.into(),
        t.image_type.into(),
        t.layer_count.into(),
        t.cut_mip_levels.into(),
        t.stereo_rt_mode as u32 as u64,
        t.template_type().into(),
    ]
}

fn enhanced_words(t: &GrcTextureG9) -> [u64; 10] {
    [
        t.width.into(),
        t.height.into(),
        t.depth.into(),
        t.format.raw().into(),
        t.tile_mode.raw().into(),
        t.mip_levels.into(),
        t.flags.into(),
        t.usage_data.into(),
        t.anti_alias_type.into(),
        t.dimension.raw().into(),
    ]
}

#[test]
fn every_corpus_ytd_extracts_to_a_source_and_compiles_back_to_the_same_pixels() {
    let Some(files) = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "ytd")
        .all_targets()
        .find_or_skip("every corpus .ytd extracts and compiles back")
    else {
        return;
    };
    let ytd = format("ytd").unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let (mut legacy_textures, mut enhanced_textures, mut compared_mips) = (0usize, 0usize, 0usize);
    let (mut undeclared, mut tail_bytes) = (Vec::new(), 0usize);

    for path in &files {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(path).unwrap();
        let version = Rsc7::peek(&bytes)
            .unwrap_or_else(|e| panic!("{file}: {e}"))
            .version();
        let Ok(target) = ytd.target_of_version(version) else {
            undeclared.push(format!("{file} (version {version})"));
            continue;
        };

        let name = file.trim_end_matches(".ytd");
        let folder = or_source::textures::extract(
            &bytes,
            name,
            scratch.path(),
            &or_source::textures::Extraction {
                from_game: true,
                virtual_path: Some(&file),
            },
        )
        .unwrap_or_else(|problems| panic!("{file}: extract: {problems:#?}"));
        let manifest =
            AssetManifest::parse(&std::fs::read_to_string(folder.join("asset.json")).unwrap())
                .unwrap();
        assert_eq!(manifest.origin.sha256, Some(sha256_hex(&bytes)), "{file}");
        assert!(manifest.origin.from_game, "{file}");
        assert_eq!(manifest.target, target.slug(), "{file}");

        let rebuilt = compile(&folder);
        assert_eq!(
            rebuilt,
            compile(&folder),
            "{file}: two compiles of one folder differ"
        );

        if target == Generation::Gen9 {
            let (before, after) = (enhanced(&bytes), enhanced(&rebuilt));
            assert_eq!(before.codes, after.codes, "{file}");
            for (a, b) in before
                .entries
                .iter()
                .flatten()
                .zip(after.entries.iter().flatten())
            {
                assert_eq!(a.name, b.name, "{file}");
                assert_eq!(enhanced_words(a), enhanced_words(b), "{file}:{}", a.name);
                let (ma, mb) = (mips(&**a), mips(&**b));
                assert_eq!(ma, mb, "{file}:{} pixel bytes", a.name);
                compared_mips += ma.len();
                enhanced_textures += 1;
            }
        } else {
            let (before, after) = (legacy(&bytes), legacy(&rebuilt));
            assert_eq!(before.codes, after.codes, "{file}");
            for (a, b) in before
                .entries
                .iter()
                .flatten()
                .zip(after.entries.iter().flatten())
            {
                assert_eq!(a.name, b.name, "{file}");
                assert_eq!(legacy_words(a), legacy_words(b), "{file}:{}", a.name);
                let (ma, mb) = (mips(&**a), mips(&**b));
                assert_eq!(ma, mb, "{file}:{} pixel bytes", a.name);
                compared_mips += ma.len();
                legacy_textures += 1;
                let declared: usize = ma.iter().map(Vec::len).sum();
                tail_bytes += usize::from(a.payload()[declared..].iter().any(|b| *b != 0));
            }
        }
        std::fs::remove_dir_all(&folder).unwrap();
    }

    println!(
        "{legacy_textures} Legacy and {enhanced_textures} Enhanced textures in {} files came back \
         with {compared_mips} mips byte for byte; {tail_bytes} Legacy textures held non-zero bytes \
         after their declared mips, which a source does not keep; at a version no layout \
         declares: {}",
        files.len() - undeclared.len(),
        undeclared.join(", ")
    );
    assert!(legacy_textures > 0, "no Legacy texture was compared");
    assert!(enhanced_textures > 0, "no Enhanced texture was compared");
}
