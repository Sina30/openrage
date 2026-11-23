//! The optimiser over a pack reaches the textures embedded in the models of its garments, which are a ped component's normal and
//! specular maps: the model becomes a new source with the textures made lighter, its geometry is the model's own, and one Undo takes it back.
//!
//! ⚠ Reads a ped component staged in the corpus; a machine without it skips, and says so.

mod common;

use std::path::{Path, PathBuf};

use common::{garment, snapshot, JBIB};
use or_interchange::{drawable_scene, ExtractOptions, LodSelection, LodSlot};
use or_io::decl::{File, Gen8};
use or_io::gta5::{GtaDrawable, PgDictionary};
use or_source::asset::Source;
use or_source::models::{self, Extraction};
use or_source::optimise::Scale;
use or_test_support::{Corpus, TARGET_GEN8};
use or_wardrobe::edit::{undo_revision, Watch};
use or_wardrobe::textures::{optimise, safe_options};
use or_wardrobe::{Ped, Wardrobe};

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
    let mut w = Wardrobe::new("demo");
    let g = garment(Ped::Male, JBIB, None, "models/model.drawables.src", &[]);
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

#[test]
fn the_textures_embedded_in_a_model_are_cut_with_the_pack_and_the_model_becomes_a_source_undo_takes_back(
) {
    let Some(file) = staged("accs_000_u.ydd") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let (pack, _id) = a_pack(dir.path(), &file);
    let old = "models/model.drawables.src";
    let before_files = snapshot(&pack);
    let was = embedded(&compiled(&pack, old));
    let geometry = uvs(&compiled(&pack, old));
    assert!(
        was.iter().any(|(_, w, h, _)| *w.min(h) >= 16),
        "the staged model embeds textures worth cutting: {was:?}"
    );
    let options = or_source::optimise::Options {
        scale: Scale::Ratio(0.5),
        ..safe_options(&Wardrobe::read(&pack).unwrap())
    };

    let (report, revision) = optimise(&pack, None, &options, &Watch::default()).expect("it runs");

    assert!(report.textures() > 0, "{report:?}");
    assert_eq!(report.models.len(), 1, "{report:?}");
    let now_names = Wardrobe::read(&pack).unwrap().garments[0]
        .model
        .clone()
        .unwrap();
    assert_ne!(now_names, old, "the garment names the new source");
    assert_eq!(report.models[0].into, now_names);
    let now = embedded(&compiled(&pack, &now_names));
    assert_eq!(now.len(), was.len());
    for ((name, w, h, _), (name_now, w_now, h_now, _)) in was.iter().zip(&now) {
        assert_eq!(name, name_now);
        if *w.min(h) >= 16 {
            assert_eq!((*w_now, *h_now), (w / 2, h / 2), "{name} is cut in half");
        }
    }
    assert_eq!(
        uvs(&compiled(&pack, &now_names)),
        geometry,
        "the geometry is the model's own"
    );
    for (path, bytes) in &before_files {
        if !path.ends_with("wardrobe.json") {
            assert_eq!(snapshot(&pack).get(path), Some(bytes), "{path} was changed");
        }
    }

    undo_revision(&pack, &revision.expect("a step of Undo")).unwrap();
    assert!(!pack.join(&now_names).exists());
    let undone = snapshot(&pack);
    for (path, bytes) in &before_files {
        assert_eq!(undone.get(path), Some(bytes), "{path} is not as it was");
    }
}
