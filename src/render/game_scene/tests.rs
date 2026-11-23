use std::cell::Cell;

use super::*;

fn key(document: DocumentId, epoch: u64, name: &str) -> DecodeKey {
    (document, epoch, None, 0, name.to_string())
}

fn grey() -> Option<Image> {
    Some(Image {
        width: 1,
        height: 1,
        rgba: vec![9; 4],
        srgb: false,
    })
}

#[test]
fn a_texture_is_decoded_once_across_scenes_until_its_dictionary_changes() {
    let decodes = Cell::new(0);
    let decode = || {
        decodes.set(decodes.get() + 1);
        grey()
    };
    let mut cache = DecodedTextures::default();
    for (scene, (epoch, expected)) in [(1, 1), (1, 1), (2, 2), (2, 2)].into_iter().enumerate() {
        cache.begin();
        assert!(
            cache.get(key(7, epoch, "dirt"), decode).is_some(),
            "scene {scene}"
        );
        cache.end();
        assert_eq!(decodes.get(), expected, "scene {scene}");
    }
}

#[test]
fn a_texture_a_scene_does_not_name_is_let_go_and_one_that_will_not_decode_is_not_tried_again() {
    let decodes = Cell::new(0);
    let mut cache = DecodedTextures::default();
    cache.begin();
    cache.get(key(1, 1, "kept"), grey);
    cache.get(key(1, 1, "dropped"), grey);
    cache.end();
    assert_eq!(cache.made.len(), 2);
    cache.begin();
    cache.get(key(1, 1, "kept"), grey);
    cache.end();
    assert_eq!(cache.made.len(), 1);
    for _ in 0..2 {
        cache.begin();
        let broken = cache.get(key(1, 1, "broken"), || {
            decodes.set(decodes.get() + 1);
            None
        });
        assert!(broken.is_none());
        cache.end();
    }
    assert_eq!(decodes.get(), 1);
}

#[test]
fn a_skinned_surface_is_bound_to_the_bone_of_the_posed_skeleton_that_has_its_tag_and_keeps_a_bone_that_has_none(
) {
    let surface = |joints: Vec<[u16; 4]>| {
        (
            Surface {
                skinned: true,
                weights: vec![[1.0, 0.0, 0.0, 0.0]; joints.len()],
                joints,
                ..Surface::default()
            },
            Material::default(),
        )
    };
    // The posed skeleton has the tag 10 at its bone 1 and the tag 20 at its bone 0, and none of 30
    let bound = |tag: u16| match tag {
        10 => Some(1),
        20 => Some(0),
        _ => None,
    };
    let mut surfaces = [surface(vec![[0, 1, 2, 3]])];
    rebind_joints(&mut surfaces, &[10, 20, 30, 40], bound);
    // The third and the fourth bones have no tag there, so they keep their indices
    assert_eq!(surfaces[0].0.joints, [[1, 0, 2, 3]]);
    // A drawable with no skeleton of its own is weighted by the posed one's indices already
    let mut kept = [surface(vec![[0, 1, 2, 3]])];
    rebind_joints(&mut kept, &[], bound);
    assert_eq!(kept[0].0.joints, [[0, 1, 2, 3]]);
}

#[test]
fn the_bones_the_skinned_surfaces_of_a_corpus_drawable_are_weighted_to_are_bones_of_its_skeleton() {
    use or_io::decl::Gen8;
    use or_io::gta5::{File as _, GtaDrawable, GtaFragType};
    use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};
    let test = "the_bones_the_skinned_surfaces_of_a_corpus_drawable_are_weighted_to";
    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let (mut checked, mut had_files) = (0, false);
    for ext in ["ydr", "yft"] {
        let files = Corpus::with_root(&root, ext).target(TARGET_GEN8).find();
        if files.is_empty() {
            or_test_support::skip(test, &root, ext);
            continue;
        }
        had_files = true;
        for path in &files {
            let bytes = std::fs::read(path).expect("a corpus file reads");
            let drawable = if ext == "ydr" {
                GtaDrawable::<Gen8>::from_bytes(&bytes)
                    .ok()
                    .map(|d| d.base.clone())
            } else {
                GtaFragType::<Gen8>::from_bytes(&bytes)
                    .ok()
                    .and_then(|f| f.common_drawable.as_deref().map(|d| d.base.clone()))
            };
            let Some(drawable) = drawable else { continue };
            let tags = skeleton_tags(&scene_of(&drawable));
            let surfaces = surfaces_of(&scene_of(&drawable), &Textures::new());
            let palette = or_renderer::game_shaders::mesh::bone_palette(&surfaces);
            if palette.is_empty() {
                continue;
            }
            let name = path.display().to_string();
            assert!(!tags.is_empty(), "{name}: skinned with no skeleton");
            assert!(
                palette.iter().all(|bone| usize::from(*bone) < tags.len()),
                "{name}: a bone past the skeleton's {}",
                tags.len()
            );
            assert!(
                palette.len() <= or_renderer::game_shaders::mesh::BONES,
                "{name}"
            );
            checked += 1;
        }
    }
    // Where the corpus holds drawables at all, some of them are skinned
    assert!(checked > 0 || !had_files, "no skinned corpus drawable");
}

#[test]
fn the_animation_workspace_shows_the_drawables_its_preview_draws_and_not_the_active_document() {
    use crate::state::Document;
    use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};
    let test = "the_animation_workspace_shows_the_drawables_its_preview_draws";
    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let files = Corpus::with_root(&root, "ydr").target(TARGET_GEN8).find();
    let Some(path) = files.first() else {
        or_test_support::skip(test, &root, "ydr");
        return;
    };
    let Ok(asset) = or_io::open(path) else {
        return;
    };
    let mut state = AppState::new();
    let doc = Document::new(0, Some(path.clone()), DocPayload::Resource(asset));
    let id = doc.id;
    state.documents = vec![doc];
    state.active_document = Some(id);
    let mut decoded = DecodedTextures::default();
    let model = surfaces_shown(&state, &mut decoded).expect("the model workspace shows it");
    // The preview draws what it is told to draw, and nothing where it is told none
    state.active_workspace = Workspace::Animation;
    assert!(surfaces_shown(&state, &mut decoded).is_none());
    state.preview_scene.add_mesh(MeshSource {
        document: id,
        entry: None,
    });
    let shown = surfaces_shown(&state, &mut decoded).expect("the preview draws it");
    assert_eq!(shown.len(), model.len());
}

#[test]
fn a_dictionary_the_animation_preview_names_whole_is_drawn_entry_by_entry() {
    use crate::state::Document;
    use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};
    let test = "a_dictionary_the_animation_preview_names_whole_is_drawn_entry_by_entry";
    let root = shared_corpus_root(env!("CARGO_MANIFEST_DIR"));
    let files = Corpus::with_root(&root, "ydd").target(TARGET_GEN8).find();
    let mut state = AppState::new();
    let mut found = None;
    for path in &files {
        let Ok(asset) = or_io::open(path) else {
            continue;
        };
        let held = match &asset {
            or_io::Asset::DrawableDict(dict) => dict.entries.iter().flatten().count(),
            or_io::Asset::DrawableDictGen9(dict) => dict.entries.iter().flatten().count(),
            _ => 0,
        };
        if held > 0 {
            let doc = Document::new(0, Some(path.clone()), DocPayload::Resource(asset));
            found = Some((doc, held));
            break;
        }
    }
    let Some((doc, _held)) = found else {
        or_test_support::skip(test, &root, "ydd");
        return;
    };
    let id = doc.id;
    state.documents = vec![doc];
    state.active_document = Some(id);
    state.active_workspace = Workspace::Animation;
    // Named whole, as the preview's menu names a dictionary, it is every drawable it holds
    state.preview_scene.add_mesh(MeshSource {
        document: id,
        entry: None,
    });
    let whole = surfaces_shown(&state, &mut DecodedTextures::default());
    assert!(
        whole.is_some_and(|surfaces| !surfaces.is_empty()),
        "a dictionary named whole drew nothing"
    );
}
