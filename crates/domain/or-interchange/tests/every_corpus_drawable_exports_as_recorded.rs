//! Every Gen8 corpus drawable exports the `.glb` it exported when the
//! baseline was recorded: every LOD, with its textures, composed as
//! `openrage-cli mesh export` composes it.

mod baseline;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use baseline::{Key, Now};
use or_interchange::gltf::{self, Container};
use or_interchange::{ExtractOptions, LodSelection, SceneBuilder};
use or_io::decl::Gen8;
use or_io::gta5::{File, GtaDrawable, GtaFragType, PgDictionary, RmcDrawable};
use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};

const KIND: &str = "glb";
const EXTENSIONS: [&str; 3] = ["ydr", "ydd", "yft"];

fn name_of(drawable: &RmcDrawable<Gen8>, fallback: &str) -> String {
    if drawable.name.is_empty() {
        fallback.to_string()
    } else {
        drawable.name.clone()
    }
}

/// The `.glb` a drawable-bearing file exports, `None` when it holds no
/// geometry, or why it would not read.
fn export(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("mesh")
        .to_string();
    let options = ExtractOptions {
        lods: LodSelection::All,
        textures: true,
    };
    let mut builder = SceneBuilder::new(&stem);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let loaded = match ext.as_str() {
        "ydr" => GtaDrawable::from_bytes(&bytes).map(|d| {
            builder.push(&name_of(&d, &stem), &d, &options);
        }),
        "ydd" => PgDictionary::<GtaDrawable>::from_bytes(&bytes).map(|dict| {
            for (index, entry) in dict.entries.iter().enumerate() {
                let Some(entry) = entry.as_deref() else {
                    continue;
                };
                let hash = dict.codes.get(index).copied().unwrap_or(0);
                let fallback = format!("entry_{hash:08X}");
                builder.push(&name_of(entry, &fallback), entry, &options);
            }
        }),
        _ => GtaFragType::from_bytes(&bytes).map(|frag| {
            for (index, drawable) in frag
                .common_drawable
                .iter()
                .chain(frag.extra_drawables.iter())
                .enumerate()
            {
                let fallback = format!("part_{index}");
                builder.push(&name_of(drawable, &fallback), drawable, &options);
            }
        }),
    };
    loaded.map_err(|e| e.to_string())?;
    let scene = builder.finish();
    if scene.objects.iter().all(|o| o.meshes.is_empty()) {
        return Ok(None);
    }
    Ok(Some(gltf::write(&scene, &stem, Container::Glb).primary))
}

fn corpus(what: &str) -> Option<(PathBuf, Vec<PathBuf>)> {
    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for ext in EXTENSIONS {
        files.extend(Corpus::with_root(&root, ext).target(TARGET_GEN8).find());
    }
    if files.is_empty() {
        or_test_support::skip(what, &root, "ydr, .ydd or .yft");
        return None;
    }
    Some((root, files))
}

#[test]
fn every_corpus_drawable_exports_as_recorded() {
    let what = "every_corpus_drawable_exports_as_recorded";
    let Some((root, files)) = corpus(what) else {
        return;
    };
    let mut now: BTreeMap<Key, Now> = BTreeMap::new();
    for path in &files {
        let outcome = match export(path) {
            Ok(Some(glb)) => Now::Digest(baseline::digest(&glb)),
            Ok(None) => continue,
            Err(why) => Now::Refused(why),
        };
        now.insert(baseline::key(&root, path, None, TARGET_GEN8), outcome);
    }
    baseline::settle(env!("CARGO_MANIFEST_DIR"), KIND, &now);
}

#[test]
fn a_perturbed_glb_names_its_file() {
    let what = "a_perturbed_glb_names_its_file";
    let Some((root, files)) = corpus(what) else {
        return;
    };
    let (path, glb) = files
        .iter()
        .find_map(|path| Some((path, export(path).ok()??)))
        .expect("some corpus drawable exports geometry");
    let at = glb.len() - 1;
    baseline::a_perturbed_stream_is_named(baseline::key(&root, path, None, TARGET_GEN8), &glb, at);
}
