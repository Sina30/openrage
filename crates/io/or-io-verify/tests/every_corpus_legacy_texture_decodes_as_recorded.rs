//! Every Legacy corpus texture decodes to the RGBA and sRGB flag it decoded to
//! when the baseline was recorded (OD-TEXTURE-DIGESTS): the check that
//! outlives the compiled reader it was first held to.

#[path = "../../../domain/or-interchange/tests/baseline/mod.rs"]
mod baseline;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use baseline::{Key, Now};
use or_io::codec::texture::decode_level;
use or_io::gta5::{gen8::GrcTexture, File as _, PgDictionary};
use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};

const KIND: &str = "texture";

/// What level 0 of `texture` decodes to: its RGBA, then its sRGB flag.
fn decoded(texture: &GrcTexture) -> Result<Vec<u8>, String> {
    let image = decode_level(texture, 0).map_err(|e| e.to_string())?;
    let mut bytes = image.data;
    bytes.push(u8::from(image.is_srgb));
    Ok(bytes)
}

/// Each texture of the dictionary at `path` by name, or why it would not open.
fn textures(path: &Path) -> Result<Vec<GrcTexture>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let dict = PgDictionary::<GrcTexture>::from_bytes(&bytes).map_err(|e| e.to_string())?;
    Ok(dict.entries.into_iter().flatten().map(|t| *t).collect())
}

fn corpus(what: &str) -> Option<(PathBuf, Vec<PathBuf>)> {
    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let files = Corpus::with_root(&root, "ytd").target(TARGET_GEN8).find();
    if files.is_empty() {
        or_test_support::skip(what, &root, "ytd");
        return None;
    }
    Some((root, files))
}

#[test]
fn every_corpus_legacy_texture_decodes_as_recorded() {
    let what = "every_corpus_legacy_texture_decodes_as_recorded";
    let Some((root, files)) = corpus(what) else {
        return;
    };
    let mut now: BTreeMap<Key, Now> = BTreeMap::new();
    for path in &files {
        let textures = match textures(path) {
            Ok(textures) => textures,
            Err(why) => {
                now.insert(
                    baseline::key(&root, path, None, TARGET_GEN8),
                    Now::Refused(why),
                );
                continue;
            }
        };
        for texture in &textures {
            let outcome = match decoded(texture) {
                Ok(bytes) => Now::Digest(baseline::digest(&bytes)),
                Err(why) => Now::Refused(why),
            };
            let key = baseline::key(&root, path, Some(&texture.name), TARGET_GEN8);
            assert!(
                now.insert(key.clone(), outcome).is_none(),
                "{}: two textures share a name",
                key.0
            );
        }
    }
    baseline::settle(env!("CARGO_MANIFEST_DIR"), KIND, &now);
}

#[test]
fn a_perturbed_texture_names_its_file() {
    let what = "a_perturbed_texture_names_its_file";
    let Some((root, files)) = corpus(what) else {
        return;
    };
    let (path, name, bytes) = files
        .iter()
        .find_map(|path| {
            textures(path).ok()?.iter().find_map(|t| {
                let bytes = decoded(t).ok()?;
                Some((path, t.name.clone(), bytes))
            })
        })
        .expect("some corpus texture decodes");
    baseline::a_perturbed_stream_is_named(
        baseline::key(&root, path, Some(&name), TARGET_GEN8),
        &bytes,
        0,
    );
}
