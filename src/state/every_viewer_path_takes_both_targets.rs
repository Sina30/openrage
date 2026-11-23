//! An Enhanced file draws, textures, previews and selects in the viewer as its
//! Legacy counterpart does, and no path the viewer runs matches a Legacy asset
//! without its Enhanced twin.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use or_io::decl::{Gen8, Gen9};
use or_io::gta5::{
    CrBoneData, CrSkeletonData, DrawableTarget, FragDrawable, GtaDrawable, GtaFragType,
    PgDictionary, PtxFxList,
};
use or_test_support::{skip_corpus, Corpus, TARGET_GEN8, TARGET_GEN9};

use super::models::{models_in, ModelListing};
use super::preview_scene::{
    mesh_sources_in, skeleton_data_from, skeleton_sources_in, SkeletonSource,
};
use super::texture_resolution::{report, resolve, ResolutionRule};
use super::textures::{textures_in, TextureListing};
use super::{AppState, DocPayload, Document, DocumentId};
use crate::render::lines::build_lines_for_active_document;
use crate::render::meshes::build_meshes_for_active_document;
use crate::render::viewport::compute_active_doc_bounds;
use crate::ui::panels::selection_ids::{dict_entry_id, DOC_ROOT_SEL};

/// The code a test dictionary keys its one entry by.
const ENTRY: u32 = 0x5EED_0001;

/// `drawable` as a `.ydr`, a `.ydd`'s one entry, a `.yft`'s common drawable and a `.ypt`'s one model.
// The struct update the lint asks for does not compile: both types hold private fields.
#[allow(clippy::field_reassign_with_default)]
fn containers<T: DrawableTarget + Clone>(
    drawable: GtaDrawable<T>,
) -> (
    GtaDrawable<T>,
    PgDictionary<GtaDrawable<T>>,
    GtaFragType<T>,
    PtxFxList<T>,
) {
    let mut common = FragDrawable::<T>::default();
    common.base = drawable.base.clone();
    let mut fragment = GtaFragType::<T>::default();
    fragment.base.common_drawable = Some(Box::new(common));
    let mut list = PtxFxList::<T>::default();
    list.model_dict = Some(Box::new(PgDictionary {
        codes: vec![ENTRY],
        entries: vec![Some(Box::new(drawable.base.clone()))],
        ..Default::default()
    }));
    let dictionary = PgDictionary {
        codes: vec![ENTRY],
        entries: vec![Some(Box::new(drawable.clone()))],
        ..Default::default()
    };
    (drawable, dictionary, fragment, list)
}

/// A drawable `asset` in each of the four files, at its own target, with the selection that names it.
fn in_every_file(asset: or_io::Asset) -> Vec<(&'static str, DocPayload, Option<u64>)> {
    use or_io::Asset::{
        Drawable, DrawableDict, DrawableDictGen9, DrawableGen9, Fragment, FragmentGen9, Particles,
        ParticlesGen9,
    };
    let (ydr, ydd, yft, ypt) = match asset {
        Drawable(d) => {
            let (ydr, ydd, yft, ypt) = containers(d);
            (
                Drawable(ydr),
                DrawableDict(ydd),
                Fragment(yft),
                Particles(ypt),
            )
        }
        DrawableGen9(d) => {
            let (ydr, ydd, yft, ypt) = containers(d);
            (
                DrawableGen9(ydr),
                DrawableDictGen9(ydd),
                FragmentGen9(yft),
                ParticlesGen9(ypt),
            )
        }
        other => panic!("not a drawable: {}", other.ext()),
    };
    let entry = Some(dict_entry_id(ENTRY));
    vec![
        (".ydr", DocPayload::Resource(ydr), None),
        (".ydd", DocPayload::Resource(ydd), entry),
        (".yft", DocPayload::Resource(yft), None),
        (".ypt", DocPayload::Resource(ypt), entry),
    ]
}

/// A state with `payload` as its active document and `selection` selected in it.
fn showing(payload: DocPayload, selection: Option<u64>) -> (AppState, DocumentId) {
    let mut state = AppState::new();
    let doc = Document::new(0, None, payload);
    let id = doc.id;
    state.documents.push(doc);
    state.active_document = Some(id);
    if let Some(selection) = selection {
        state.selection.select(id, selection);
    }
    (state, id)
}

/// The texture names a document's meshes ask for that its own listing holds and the preview decodes.
fn embedded_and_asked_for(state: &AppState, selection: Option<u64>) -> BTreeSet<String> {
    let asked: BTreeSet<String> = build_meshes_for_active_document(state)
        .iter()
        .filter_map(|upload| upload.texture_name.clone())
        .collect();
    let Some(doc) = state.active_document() else {
        return BTreeSet::new();
    };
    let TextureListing::Textures { textures, .. } = textures_in(&doc.payload, selection) else {
        return BTreeSet::new();
    };
    textures
        .iter()
        .filter(|t| t.format != or_io::codec::texture::GrcTextureFormat::None)
        .map(|t| t.name.to_ascii_lowercase())
        .filter(|name| asked.contains(name))
        .collect()
}

#[test]
fn an_enhanced_drawable_finds_its_textures_in_material_mode_as_a_legacy_one_does() {
    const WHAT: &str =
        "an_enhanced_drawable_finds_its_textures_in_material_mode_as_a_legacy_one_does";
    for target in [TARGET_GEN8, TARGET_GEN9] {
        let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ydr")
            .target(target)
            .find();
        files.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX));
        let staged = files.into_iter().find_map(|path| {
            let asset = or_io::open(&path).ok()?;
            let (state, _) = showing(DocPayload::Resource(asset.clone()), None);
            let names = embedded_and_asked_for(&state, None);
            (!names.is_empty()).then_some((path, asset, names))
        });
        let Some((path, asset, names)) = staged else {
            skip_corpus(
                WHAT,
                format_args!("no {target} .ydr whose meshes ask for a texture it embeds"),
            );
            continue;
        };
        for (file, payload, selection) in in_every_file(asset) {
            let (state, _) = showing(payload, selection);
            let found = report(
                &state,
                selection,
                names.iter().map(|n| (None, n.clone())),
                0,
            );
            for outcome in &found.outcomes {
                assert_eq!(
                    outcome.found.as_ref().map(|origin| origin.rule),
                    Some(ResolutionRule::Embedded),
                    "{} as a {target} {file}: Material mode does not find {}",
                    path.display(),
                    outcome.name
                );
                let texture = resolve(&state, None, selection, &outcome.name).map(|r| r.texture);
                assert!(
                    texture.is_some_and(|t| t.has_pixel_data()),
                    "{} as a {target} {file}: {} resolves to no pixels",
                    path.display(),
                    outcome.name
                );
            }
        }
    }
}

/// A drawable of each target carrying a one-bone skeleton.
fn rigged() -> [or_io::Asset; 2] {
    let skeleton = CrSkeletonData {
        parent_indices: vec![-1],
        bones: vec![CrBoneData {
            name: "root".to_string(),
            parent_index: -1,
            next_index: -1,
            ..CrBoneData::default()
        }],
        ..CrSkeletonData::default()
    };
    let mut legacy = GtaDrawable::<Gen8>::default();
    legacy.base.skeleton = Some(Box::new(skeleton.clone()));
    let mut enhanced = GtaDrawable::<Gen9>::default();
    enhanced.base.skeleton = Some(Box::new(skeleton));
    [
        or_io::Asset::Drawable(legacy),
        or_io::Asset::DrawableGen9(enhanced),
    ]
}

#[test]
fn the_animation_workspace_offers_an_enhanced_drawable_and_skeleton_as_a_legacy_one() {
    let offers = |asset: or_io::Asset| -> Vec<(&str, usize, Vec<SkeletonSource>, usize)> {
        in_every_file(asset)
            .into_iter()
            .map(|(file, payload, _)| {
                let meshes = mesh_sources_in(7, &payload, "rig").len();
                let skeletons = skeleton_sources_in(7, &payload);
                let bones = skeletons
                    .iter()
                    .filter_map(|source| skeleton_data_from(&payload, *source))
                    .map(|data| data.bones.len())
                    .sum();
                (file, meshes, skeletons, bones)
            })
            .collect()
    };
    let [legacy, enhanced] = rigged();
    let legacy = offers(legacy);
    assert_eq!(
        legacy,
        [
            (".ydr", 1, vec![SkeletonSource::Drawable(7)], 1),
            (".ydd", 2, vec![SkeletonSource::DictionaryEntry(7, 0)], 1),
            (".yft", 1, vec![SkeletonSource::Fragment(7)], 1),
            (".ypt", 0, vec![], 0),
        ],
        "what the workspace offers of a Legacy drawable"
    );
    assert_eq!(
        offers(enhanced),
        legacy,
        "an Enhanced drawable is not offered as its Legacy counterpart is"
    );
}

#[test]
fn the_gallery_lists_an_enhanced_files_models_as_a_legacy_ones() {
    let listed = |asset: or_io::Asset| -> Vec<(&str, Vec<u64>)> {
        in_every_file(asset)
            .into_iter()
            .map(|(file, payload, _)| match models_in(&payload) {
                ModelListing::Models(models) => {
                    (file, models.iter().map(|m| m.selection_id).collect())
                }
                ModelListing::Nothing(_) => (file, Vec::new()),
            })
            .collect()
    };
    let [legacy, enhanced] = rigged();
    let legacy = listed(legacy);
    assert_eq!(
        legacy,
        [
            (".ydr", vec![DOC_ROOT_SEL]),
            (".ydd", vec![dict_entry_id(ENTRY)]),
            (".yft", vec![DOC_ROOT_SEL]),
            (".ypt", vec![dict_entry_id(ENTRY)]),
        ],
        "the tiles a Legacy file lists"
    );
    assert_eq!(
        listed(enhanced),
        legacy,
        "an Enhanced file lists other tiles"
    );
}

/// The code of the first model in `list` whose first level of detail holds a geometry.
fn a_model_with_geometry<T: DrawableTarget>(list: &PtxFxList<T>) -> Option<u32> {
    or_particle::list::models(list).find_map(|(code, model)| {
        let lod = &model.lod_group.lods[0];
        lod.models
            .iter()
            .any(|m| !m.geometries.is_empty())
            .then_some(code)
    })
}

#[test]
fn a_model_selected_in_an_enhanced_particle_list_draws_and_frames_as_a_legacy_one() {
    const WHAT: &str =
        "a_model_selected_in_an_enhanced_particle_list_draws_and_frames_as_a_legacy_one";
    for target in [TARGET_GEN8, TARGET_GEN9] {
        let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ypt")
            .target(target)
            .find();
        files.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX));
        let staged = files.into_iter().find_map(|path| {
            let asset = or_io::open(&path).ok()?;
            let code = match &asset {
                or_io::Asset::Particles(list) => a_model_with_geometry(list),
                or_io::Asset::ParticlesGen9(list) => a_model_with_geometry(list),
                _ => None,
            }?;
            Some((path, asset, code))
        });
        let Some((path, asset, code)) = staged else {
            skip_corpus(WHAT, format_args!("no {target} .ypt model with geometry"));
            continue;
        };
        let payload = DocPayload::Resource(asset);
        let tile = dict_entry_id(code);
        assert!(
            matches!(models_in(&payload), ModelListing::Models(m) if m.iter().any(|t| t.selection_id == tile)),
            "{}: the gallery lists no tile for model #{code:08X}",
            path.display()
        );
        let (state, _) = showing(payload, Some(tile));
        assert!(
            !build_meshes_for_active_document(&state).is_empty(),
            "{}: the selected model draws no mesh",
            path.display()
        );
        assert!(
            !build_lines_for_active_document(&state).is_empty(),
            "{}: the selected model draws no wireframe",
            path.display()
        );
        let framed = compute_active_doc_bounds(&state);
        assert!(
            framed.is_some_and(|(_, radius)| radius > 0.0),
            "{}: the selected model frames as {framed:?}",
            path.display()
        );
    }
}

/// The files whose code draws, frames, textures, lists and offers a document in the viewer, and the panels around it that read a particle file.
const VIEWER: &[&str] = &[
    "src/render",
    "src/state/animation_session.rs",
    "src/state/capabilities.rs",
    "src/state/models.rs",
    "src/state/particle_textures.rs",
    "src/state/preview_scene.rs",
    "src/state/texture_resolution.rs",
    "src/state/textures.rs",
    "src/ui/panels/gallery.rs",
    "src/ui/panels/hierarchy/ypt",
    "src/ui/panels/inspector/ypt.rs",
    "src/ui/panels/inspector/ypt_behaviour.rs",
    "src/ui/panels/inspector/ypt_model.rs",
    "src/ui/panels/inspector/ypt_selected.rs",
    "src/ui/panels/inspector/ypt_steps.rs",
    "src/ui/panels/preview_settings.rs",
    "src/ui/panels/selection_ids.rs",
    "src/ui/panels/timeline.rs",
    "src/ui/panels/viewport.rs",
];

/// The paths in those files that match a Legacy asset and that the viewer never runs, with whose they are.
const NOT_THE_VIEWERS: &[(&str, &str, &str)] = &[
    (
        "src/render/viewport/mod.rs",
        "refresh_particles",
        "the particle simulation's, which builds from a Legacy list only",
    ),
    (
        "src/render/game_scene.rs",
        "surfaces_shown",
        "the ground under the particle simulation's effect, which builds from a Legacy list only",
    ),
    (
        "src/state/textures.rs",
        "legacy_dictionary",
        "the texture editor's, which has no Enhanced twin: the encoder writes the Legacy layout",
    ),
    (
        "src/state/textures.rs",
        "legacy_dictionary_mut",
        "the texture editor's",
    ),
];

/// Each function outside test code in `src`, by its module path, with the `Asset` variants its body names.
fn variants_named(src: &str) -> BTreeMap<String, BTreeSet<String>> {
    use crate::edition_strings::{is_ident, lex, test_items};

    let code = lex(src).code;
    let (tests, _) = test_items(&code);
    let word = |at: usize| {
        let end = (at..code.len())
            .find(|&j| !is_ident(code[j]))
            .unwrap_or(code.len());
        String::from_utf8_lossy(&code[at..end]).into_owned()
    };
    let mut named: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // Each open `mod` or `fn` body: whether it is a function, its name, and the depth it opened at.
    let mut scopes: Vec<(bool, String, usize)> = Vec::new();
    let mut pending: Option<(bool, String)> = None;
    let (mut depth, mut square, mut i) = (0, 0, 0);
    while i < code.len() {
        if let Some(&(_, end)) = tests.iter().find(|&&(from, _)| from == i) {
            i = end + 1;
            continue;
        }
        let c = code[i];
        if is_ident(c) && (i == 0 || !is_ident(code[i - 1])) {
            let w = word(i);
            let after = (i + w.len()..code.len())
                .find(|&j| !code[j].is_ascii_whitespace())
                .unwrap_or(code.len());
            let next = code.get(after).copied().unwrap_or(b' ');
            if (w == "fn" || w == "mod") && (next.is_ascii_alphabetic() || next == b'_') {
                pending = Some((w == "fn", word(after)));
            } else if w == "Asset" && code[i + w.len()..].starts_with(b"::") {
                let modules = scopes.iter().take_while(|s| !s.0).map(|s| s.1.as_str());
                let function = scopes.iter().find(|s| s.0).map(|s| s.1.as_str());
                let path: Vec<&str> = modules.chain(function).collect();
                named
                    .entry(path.join("::"))
                    .or_default()
                    .insert(word(i + w.len() + 2));
            }
            i += w.len();
            continue;
        }
        match c {
            b'{' => {
                if let Some((function, name)) = pending.take() {
                    scopes.push((function, name, depth));
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if scopes.last().is_some_and(|s| s.2 == depth) {
                    scopes.pop();
                }
            }
            b'[' => square += 1,
            b']' => square -= 1,
            b';' if square == 0 => pending = None,
            _ => {}
        }
        i += 1;
    }
    named
}

/// Each Legacy `Asset` variant with its Enhanced twin, as `or_io::Asset` declares them.
fn twins() -> Vec<(String, String)> {
    use crate::edition_strings::{is_ident, lex};

    let asset = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/io/or-io/src/asset.rs");
    let src =
        std::fs::read_to_string(&asset).unwrap_or_else(|e| panic!("{}: {e}", asset.display()));
    let code = lex(&src).code;
    // Every word an opening parenthesis follows, which takes in each tuple variant.
    let called: BTreeSet<String> = (0..code.len())
        .filter(|&at| code[at] == b'(')
        .map(|at| {
            let start = code[..at]
                .iter()
                .rposition(|&c| !is_ident(c))
                .map_or(0, |p| p + 1);
            String::from_utf8_lossy(&code[start..at]).into_owned()
        })
        .collect();
    called
        .iter()
        .filter_map(|variant| {
            let legacy = variant.strip_suffix("Gen9")?;
            called
                .contains(legacy)
                .then(|| (legacy.to_string(), variant.clone()))
        })
        .collect()
}

#[test]
fn every_viewer_path_that_matches_a_legacy_asset_handles_its_enhanced_twin() {
    let twins = twins();
    assert!(
        twins.iter().any(|(legacy, _)| legacy == "Drawable") && twins.len() >= 5,
        "the twins read from or_io::Asset are {twins:?}"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for rel in VIEWER {
        let path = root.join(rel);
        if path.is_dir() {
            crate::edition_strings::rust_files(&path, &mut files);
        } else {
            files.push(path);
        }
    }
    files.sort();

    let (mut handled, mut legacy_only) = (0, BTreeSet::new());
    for path in &files {
        let src =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let rel = path.strip_prefix(root).unwrap_or(path);
        let rel = rel.to_string_lossy().replace('\\', "/");
        for (function, variants) in variants_named(&src) {
            for (legacy, enhanced) in &twins {
                if !variants.contains(legacy) {
                    continue;
                }
                if variants.contains(enhanced) {
                    handled += 1;
                } else {
                    legacy_only.insert((rel.clone(), function.clone(), legacy.clone()));
                }
            }
        }
    }
    // A scan that stopped finding functions would find no gap and pass, so it is held to a floor.
    assert!(
        handled > 20,
        "only {handled} viewer paths over {} files handle both targets",
        files.len()
    );
    let found: BTreeSet<(String, String)> = legacy_only
        .iter()
        .map(|(file, function, _)| (file.clone(), function.clone()))
        .collect();
    let known: BTreeSet<(String, String)> = NOT_THE_VIEWERS
        .iter()
        .map(|(file, function, _)| (file.to_string(), function.to_string()))
        .collect();
    let unexplained: Vec<String> = legacy_only
        .iter()
        .filter(|(file, function, _)| !known.contains(&(file.clone(), function.clone())))
        .map(|(file, function, legacy)| {
            format!("  {file}: {function} matches Asset::{legacy} alone")
        })
        .collect();
    let stale: Vec<String> = known
        .difference(&found)
        .map(|(file, function)| {
            format!("  {file}: {function} is listed and matches no Legacy asset alone")
        })
        .collect();
    assert!(
        unexplained.is_empty() && stale.is_empty(),
        "a viewer path matches a Legacy asset without its Enhanced twin, or NOT_THE_VIEWERS \
         no longer describes the tree:\n{}",
        unexplained
            .into_iter()
            .chain(stale)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn the_scan_names_each_match_by_the_function_it_sits_in() {
    let src = r##"
        mod rows {
            // Asset::Drawable( in a comment
            pub fn both(p: &P) -> [u8; 4] {
                match p { Asset::Drawable(_) => [0; 4], Asset::DrawableGen9(_) => [1; 4] }
            }
            pub fn one(p: &P) -> &str {
                fn inner() {}
                let _ = "Asset::Fragment(";
                match p { Asset::Particles(_) => "", _ => "" }
            }
        }
        trait T { fn declared(&self); }
        fn top(p: &P) -> bool { matches!(p, Asset::TextureDict(_)) }
        #[cfg(test)]
        mod tests { fn t() { let _ = Asset::Fragment(x); } }
    "##;
    let named = variants_named(src);
    let expect: BTreeMap<String, BTreeSet<String>> = [
        ("rows::both", &["Drawable", "DrawableGen9"][..]),
        ("rows::one", &["Particles"][..]),
        ("top", &["TextureDict"][..]),
    ]
    .into_iter()
    .map(|(path, variants)| {
        let variants = variants.iter().map(|v| v.to_string()).collect();
        (path.to_string(), variants)
    })
    .collect();
    assert_eq!(named, expect);
}
