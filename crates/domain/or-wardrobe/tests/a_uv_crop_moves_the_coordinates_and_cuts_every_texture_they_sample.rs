//! UV crop over a garment: its models' texture coordinates move into the part of the texture they use, the colour texture of its pack and the normal and specular maps embedded in the model are cut to the same part, and every vertex still samples the pixel it sampled.
//!
//! ⚠ Reads a ped component staged in the corpus; a machine without it skips, and says so.

mod common;

use std::path::{Path, PathBuf};

use common::{garment, snapshot, JBIB};
use or_interchange::{drawable_scene, ExtractOptions, LodSelection, LodSlot};
use or_io::authoring::TextureBuildOptions;
use or_io::codec::texture::{decode_level, GrcTextureFormat, RgbaImage};
use or_io::decl::{File, Gen8};
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::texture_dict::gen8::PROCESSED;
use or_io::gta5::{GtaDrawable, PgDictionary};
use or_source::asset::Source;
use or_source::game_import::{decompile, Facts};
use or_source::models::{self, Extraction};
use or_test_support::{Corpus, TARGET_GEN8};
use or_wardrobe::edit::{undo_revision, Watch};
use or_wardrobe::mesh::{examine, run, Op};
use or_wardrobe::{Ped, Variation, Wardrobe};

fn staged(name: &str) -> Option<PathBuf> {
    let found = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydd")
        .target(TARGET_GEN8)
        .find()
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n == name));
    if found.is_none() {
        or_test_support::skip_corpus(
            &format!("the ped component {name}"),
            "not staged".to_string(),
        );
    }
    found
}

/// A `.ytd` of one texture of 256 by 256, DXT1, with every mip down to four texels, whose pixels differ from place to place.
fn ytd(name: &str) -> Vec<u8> {
    let mut image = RgbaImage::new(256, 256);
    for y in 0..256u32 {
        for x in 0..256u32 {
            image.set_pixel(
                x,
                y,
                [
                    (x % 256) as u8,
                    (y % 256) as u8,
                    ((x / 8 + y / 8) * 5 % 256) as u8,
                    255,
                ],
            );
        }
    }
    let mut texture = GrcTexture {
        name: name.to_string(),
        resource_type_and_conversion_flags: PROCESSED,
        ..Default::default()
    };
    let options = TextureBuildOptions {
        mip_levels: Some(7),
        ..TextureBuildOptions::default()
    };
    texture
        .set_pixels(GrcTextureFormat::DXT1, &image, &options)
        .unwrap();
    let mut dictionary = PgDictionary::<GrcTexture>::default();
    dictionary.insert(texture);
    dictionary.to_bytes().unwrap()
}

/// A pack whose jacket names a source made of the staged `file` and a colour texture of 256 by 256.
fn a_pack(dir: &Path, file: &Path) -> (PathBuf, String) {
    let pack = dir.join("clothing");
    std::fs::create_dir_all(pack.join("models")).unwrap();
    models::extract_drawables(
        &std::fs::read(file).unwrap(),
        "model",
        &pack.join("models"),
        &Extraction {
            from_game: false,
            virtual_path: None,
            textures: false,
            skeleton: None,
        },
    )
    .expect("a staged ped component extracts");
    decompile(
        &ytd("diff"),
        "diff.ytd",
        &pack.join("textures"),
        &Facts::default(),
    )
    .unwrap();
    let mut w = Wardrobe::new("demo");
    let mut g = garment(Ped::Male, JBIB, None, "models/model.drawables.src", &[]);
    g.variations = vec![Variation {
        texture: Some("textures/diff.textures.src".to_string()),
        ..Variation::default()
    }];
    let id = g.id.clone();
    w.garments.push(g);
    w.write(&pack).unwrap();
    (pack, id)
}

fn compiled(pack: &Path, rel: &str) -> Vec<u8> {
    models::compile(&Source::open(&pack.join(rel)).unwrap())
        .expect("a source that compiles")
        .bytes
}

fn texture_of(pack: &Path, rel: &str) -> GrcTexture {
    let source = Source::open(&pack.join(rel)).unwrap();
    let bytes = or_source::textures::compile(&source)
        .expect("a textures source that compiles")
        .bytes;
    let dictionary = PgDictionary::<GrcTexture>::from_bytes(&bytes).unwrap();
    let first = dictionary
        .entries
        .into_iter()
        .flatten()
        .next()
        .expect("one texture");
    *first
}

fn uvs(bytes: &[u8]) -> Vec<[f32; 2]> {
    let dictionary = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(bytes).unwrap();
    let entry = dictionary.entries.iter().flatten().next().unwrap();
    let scene = drawable_scene(
        "d",
        &entry.base,
        &ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        },
    );
    scene
        .primitives()
        .filter(|p| p.lod == LodSlot::Level(0))
        .flat_map(|p| p.uvs[0].clone())
        .collect()
}

fn embedded(bytes: &[u8]) -> Vec<(String, u32, u32, u32)> {
    let dictionary = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(bytes).unwrap();
    let entry = dictionary.entries.iter().flatten().next().unwrap();
    let dict = entry
        .base
        .shader_group
        .as_deref()
        .and_then(|g| g.texture_dictionary.as_deref())
        .expect("embedded textures");
    dict.entries
        .iter()
        .flatten()
        .map(|t| {
            (
                t.name.clone(),
                u32::from(t.width),
                u32::from(t.height),
                u32::from(t.mip_count),
            )
        })
        .collect()
}

#[test]
fn the_coordinates_move_into_the_part_they_use_every_texture_is_cut_to_it_and_every_vertex_still_samples_its_pixel(
) {
    let Some(file) = staged("accs_000_u.ydd") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (pack, id) = a_pack(dir.path(), &file);
    let (old_model, old_texture) = ("models/model.drawables.src", "textures/diff.textures.src");
    let before_files = snapshot(&pack);
    let was = uvs(&compiled(&pack, old_model));
    let wide = texture_of(&pack, old_texture);

    // The look says how much of the texture the model uses and what a crop would keep.
    let looked = examine(&pack, &id, None).unwrap();
    assert!(
        looked.models[0].uv_used.is_some_and(|u| u < 0.1),
        "{:?}",
        looked.models[0]
    );
    assert!(
        looked
            .lines()
            .iter()
            .any(|l| l.contains("1/8 of the width by 1/2 of the height")),
        "{:?}",
        looked.lines()
    );

    let (report, revision) =
        run(&pack, &id, &Op::UvCrop, None, &Watch::default()).expect("the garment is cropped");
    assert_eq!(report.models.len(), 1, "{report:?}");
    let done = &report.models[0];
    assert!(
        done.lines
            .iter()
            .any(|l| l.contains("1/8 of the width by 1/2 of the height")),
        "{:?}",
        done.lines
    );
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("diff") && n.contains("256 by 256") && n.contains("32 by 128")),
        "{:?}",
        report.notes
    );

    // The garment names the new sources, and the ones it named are as they were.
    let w = Wardrobe::read(&pack).unwrap();
    assert_eq!(w.garments[0].model.as_deref(), Some(done.into.as_str()));
    let new_texture = w.garments[0].variations[0].texture.clone().unwrap();
    assert_ne!(new_texture, old_texture);
    for (path, bytes) in before_files
        .iter()
        .filter(|(p, _)| p.starts_with("models/") || p.starts_with("textures/"))
    {
        assert_eq!(snapshot(&pack).get(path), Some(bytes), "{path} was touched");
    }

    // The colour texture is the window of the old one, and the maps embedded in the model are cut to the same window.
    let narrow = texture_of(&pack, &new_texture);
    assert_eq!(
        (
            u32::from(narrow.width),
            u32::from(narrow.height),
            u32::from(narrow.mip_count)
        ),
        (32, 128, 4)
    );
    let model = compiled(&pack, &done.into);
    let maps = embedded(&model);
    assert!(
        maps.contains(&("accs_normal_000".to_string(), 64, 256, 5)),
        "{maps:?}"
    );
    assert!(
        maps.contains(&("accs_spec_000".to_string(), 32, 128, 4)),
        "{maps:?}"
    );

    // Every vertex samples the pixel it sampled: the texel of its coordinates in the wide texture is the texel of its new ones in the narrow one.
    let now = uvs(&model);
    assert_eq!(now.len(), was.len());
    let (wide_pixels, narrow_pixels) = (
        decode_level(&wide, 0).unwrap(),
        decode_level(&narrow, 0).unwrap(),
    );
    let mut compared = 0;
    for (old, new) in was.iter().zip(&now) {
        assert!(
            new.iter().all(|c| (0.0..=1.0).contains(c)),
            "{new:?} is outside the texture"
        );
        let texel = |image: &RgbaImage, uv: &[f32; 2]| {
            let x = ((uv[0] * image.width as f32) as u32).min(image.width - 1);
            let y = ((uv[1] * image.height as f32) as u32).min(image.height - 1);
            image.get_pixel(x, y)
        };
        // A coordinate within a rounding of a texel's edge may fall either side of it.
        let near_edge = |uv: &[f32; 2], size: [f32; 2]| {
            uv.iter()
                .zip(size)
                .any(|(c, s)| (c * s - (c * s).round()).abs() < 1e-3)
        };
        if near_edge(old, [256.0, 256.0]) || near_edge(new, [32.0, 128.0]) {
            continue;
        }
        assert_eq!(
            texel(&wide_pixels, old),
            texel(&narrow_pixels, new),
            "vertex at {old:?} now at {new:?}"
        );
        compared += 1;
    }
    assert!(compared > 0, "no vertex was compared");

    // Undo names the old sources again and takes the new ones out.
    undo_revision(&pack, &revision.expect("a step of Undo")).unwrap();
    let w = Wardrobe::read(&pack).unwrap();
    assert_eq!(
        (
            w.garments[0].model.as_deref(),
            w.garments[0].variations[0].texture.as_deref()
        ),
        (Some(old_model), Some(old_texture))
    );
    assert!(!pack.join(&done.into).exists() && !pack.join(&new_texture).exists());
}

#[test]
fn a_garment_whose_coordinates_fill_the_texture_is_not_cropped_and_says_why() {
    let Some(file) = staged("jbib_000_u_1.ydd") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (pack, id) = a_pack(dir.path(), &file);
    let before = snapshot(&pack);

    let (report, revision) = run(&pack, &id, &Op::UvCrop, None, &Watch::default()).unwrap();

    assert!(!report.changed() && revision.is_none(), "{report:?}");
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("nothing was cropped") && n.contains("percent")),
        "{:?}",
        report.notes
    );
    assert_eq!(
        snapshot(&pack).keys().collect::<Vec<_>>(),
        before.keys().collect::<Vec<_>>()
    );
}

#[test]
fn a_garment_whose_texture_is_not_a_source_of_the_pack_is_refused_and_nothing_is_written() {
    let Some(file) = staged("accs_000_u.ydd") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (pack, id) = a_pack(dir.path(), &file);
    let mut w = Wardrobe::read(&pack).unwrap();
    w.garments[0].variations[0].texture = Some("textures/gone.ytd".to_string());
    w.write(&pack).unwrap();
    let before = snapshot(&pack);

    let (report, revision) = run(&pack, &id, &Op::UvCrop, None, &Watch::default()).unwrap();

    assert!(
        report.refused.iter().any(|r| r.file == "textures/gone.ytd"),
        "{report:?}"
    );
    assert!(revision.is_none());
    assert_eq!(snapshot(&pack), before);
}
