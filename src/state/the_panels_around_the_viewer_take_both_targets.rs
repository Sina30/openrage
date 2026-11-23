//! An Enhanced `.ypt` fills the Outline, the status bar, the texture browser, the Inspector and the
//! timeline as a Legacy one does, and an Enhanced `.yft`'s textures are listed as a Legacy one's.

use std::collections::BTreeSet;
use std::path::PathBuf;

use or_io::codec::texture::GrcTextureFormat;
use or_io::decl::{Gen8, Gen9, Open};
use or_io::gta5::particle::PtxEventEmitter;
use or_io::gta5::{
    gen8, gen9, DrawableTarget, FragDrawable, GrmShaderGroup, GtaFragType, PgDictionary, PtxFxList,
    RmcDrawable,
};
use or_particle::list;
use or_test_support::{skip_corpus, Corpus, TARGET_GEN9};

use super::prefs::Prefs;
use super::textures::{
    texture_at, texture_for_selection, textures_in, TextureListing, TextureResolution,
    TextureSource, FRAGMENT_ENTRY_HASH,
};
use super::{capabilities, AppState, DocPayload, DocumentId};
use crate::ui::panels::selection_ids::{
    dict_entry_part_id, ypt_effect_id, ypt_model_id, ypt_model_textures_section_id, ypt_texture_id,
    DOC_ROOT_SEL, YPT_MODEL_DICT, YPT_MODEL_KIND_TEXTURE,
};

/// The code the fixture's one model is keyed by.
pub(super) const MODEL: u32 = 0x5EED_0002;

/// What a target needs a fixture to say that its other half need not.
pub(super) trait Fixture: DrawableTarget + Clone + 'static {
    fn texture(name: &str) -> Self::Texture;
    fn particles(list: PtxFxList<Self>) -> DocPayload;
    fn fragment(fragment: GtaFragType<Self>) -> DocPayload;
}

impl Fixture for Gen8 {
    fn texture(name: &str) -> gen8::GrcTexture {
        gen8::GrcTexture {
            name: name.to_string(),
            width: 16,
            height: 8,
            format: GrcTextureFormat::DXT5.to_raw_format(),
            ..Default::default()
        }
    }

    fn particles(list: PtxFxList<Self>) -> DocPayload {
        DocPayload::Resource(or_io::Asset::Particles(list))
    }

    fn fragment(fragment: GtaFragType<Self>) -> DocPayload {
        DocPayload::Resource(or_io::Asset::Fragment(fragment))
    }
}

impl Fixture for Gen9 {
    fn texture(name: &str) -> gen9::GrcTextureG9 {
        gen9::GrcTextureG9 {
            name: name.to_string(),
            width: 16,
            height: 8,
            format: Open::new(gen9::PixelFormat::Bc3UnormSrgb),
            ..Default::default()
        }
    }

    fn particles(list: PtxFxList<Self>) -> DocPayload {
        DocPayload::Resource(or_io::Asset::ParticlesGen9(list))
    }

    fn fragment(fragment: GtaFragType<Self>) -> DocPayload {
        DocPayload::Resource(or_io::Asset::FragmentGen9(fragment))
    }
}

/// A dictionary holding the one texture `name`.
fn dictionary_of<T: Fixture>(name: &str) -> PgDictionary<T::Texture> {
    PgDictionary {
        codes: vec![or_io::hash::string_hash_str(name)],
        entries: vec![Some(Box::new(T::texture(name)))],
        ..Default::default()
    }
}

/// A drawable carrying the texture `texture` in its shader group's dictionary.
// The struct update the lint asks for does not compile: both types hold private fields.
#[allow(clippy::field_reassign_with_default)]
fn drawable_with<T: Fixture>(texture: &str) -> RmcDrawable<T> {
    let mut group = GrmShaderGroup::<T>::default();
    group.texture_dictionary = Some(Box::new(dictionary_of::<T>(texture)));
    let mut drawable = RmcDrawable::<T>::default();
    drawable.shader_group = Some(Box::new(group));
    drawable
}

/// An effect firing an emitter rule and a particle rule, a texture of its own and a model carrying another.
pub(super) fn a_list<T: Fixture>() -> PtxFxList<T> {
    let mut fx = PtxFxList::<T>::default();
    assert!(list::add_effect(&mut fx, "fx"));
    assert!(list::add_emitter_rule(&mut fx, "em"));
    assert!(list::add_particle_rule(&mut fx, "pt"));
    if let Some(effect) = list::effect_rules_mut(&mut fx).next() {
        effect.timeline.events.push(Some(Box::new(PtxEventEmitter {
            emitter_rule_name: "em".into(),
            particle_rule_name: "pt".into(),
            ..Default::default()
        })));
    }
    fx.texture_dict = Some(Box::new(dictionary_of::<T>("spark")));
    fx.model_dict = Some(Box::new(PgDictionary {
        codes: vec![MODEL],
        entries: vec![Some(Box::new(drawable_with::<T>("smoke")))],
        ..Default::default()
    }));
    fx
}

/// A fragment whose common drawable carries the texture `smoke`.
// The struct update the lint asks for does not compile: both types hold private fields.
#[allow(clippy::field_reassign_with_default)]
pub(super) fn a_fragment<T: Fixture>() -> GtaFragType<T> {
    let mut common = FragDrawable::<T>::default();
    common.base = drawable_with::<T>("smoke");
    let mut fragment = GtaFragType::<T>::default();
    fragment.base.common_drawable = Some(Box::new(common));
    fragment
}

/// `check` run on the same particle file at each target, named by its edition.
fn at_both_targets(mut check: impl FnMut(&str, DocPayload)) {
    check("Legacy", Gen8::particles(a_list::<Gen8>()));
    check("Enhanced", Gen9::particles(a_list::<Gen9>()));
}

fn opened(payload: DocPayload) -> (AppState, DocumentId) {
    let mut state = AppState::from_prefs(Prefs::default());
    let doc = state.insert_document(PathBuf::from("probe.ypt"), payload, None, None);
    (state, doc)
}

/// Every text `draw` paints over a few frames, as a user reads it.
fn painted(draw: fn(&mut egui::Ui, &mut AppState), state: &mut AppState) -> BTreeSet<String> {
    fn walk(shape: &egui::Shape, into: &mut BTreeSet<String>) {
        match shape {
            egui::Shape::Text(text) => {
                into.insert(text.galley.text().to_string());
            }
            egui::Shape::Vec(inner) => inner.iter().for_each(|shape| walk(shape, into)),
            _ => {}
        }
    }
    let ctx = egui::Context::default();
    let mut seen = BTreeSet::new();
    for frame in 0..12 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700.0, 1100.0),
            )),
            time: Some(f64::from(frame) / 30.0),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| draw(ui, state));
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut seen);
        }
    }
    seen
}

fn shows(painted: &BTreeSet<String>, text: &str) -> bool {
    painted.iter().any(|shown| shown.contains(text))
}

#[test]
fn an_enhanced_particle_file_fills_the_outline_as_a_legacy_one_does() {
    at_both_targets(|edition, payload| {
        let (mut state, _) = opened(payload);
        let shown = painted(crate::ui::panels::hierarchy::draw_outline, &mut state);
        for row in [
            "Effects (1)",
            "fx",
            "Emitter Rules (1)",
            "Particle Rules (1)",
            "Textures (1)",
            "Models (1)",
        ] {
            assert!(
                shows(&shown, row),
                "a {edition} particle file's Outline shows no `{row}`: {shown:?}"
            );
        }
    });
}

#[test]
fn the_status_bar_names_an_enhanced_particle_files_effects_as_a_legacy_ones() {
    let summary = |payload: &DocPayload| {
        let row = capabilities::for_payload(payload).and_then(|row| row.selection_summary);
        row.map_or_else(String::new, |summary| summary(payload, DOC_ROOT_SEL))
    };
    let legacy = summary(&Gen8::particles(a_list::<Gen8>()));
    assert!(
        legacy.starts_with("1 effect"),
        "a Legacy file says `{legacy}`"
    );
    assert_eq!(
        summary(&Gen9::particles(a_list::<Gen9>())),
        format!("{legacy} (Enhanced)"),
        "an Enhanced file does not say what a Legacy one does, with its edition"
    );
}

/// A listed texture's name, width, height and format.
type Tile = (String, u32, u32, GrcTextureFormat);

/// What the texture browser lists of `payload` under `selection`, each tile checked to resolve back to its own texture.
fn browsed(payload: &DocPayload, selection: Option<u64>) -> Option<(Vec<Tile>, TextureSource)> {
    let TextureListing::Textures { textures, source } = textures_in(payload, selection) else {
        return None;
    };
    let mut rows = Vec::new();
    for tile in &textures {
        let resolved = texture_for_selection(payload, tile.selection_id);
        let TextureResolution::Found { name, source: back } = resolved else {
            panic!("{}: its own tile names no texture: {resolved:?}", tile.name);
        };
        assert_eq!((name.as_str(), back), (tile.name.as_str(), source));
        let fetched = texture_at(payload, source, &name)
            .unwrap_or_else(|| panic!("{name}: resolved and could not be fetched"));
        assert_eq!(
            (
                u32::from(fetched.width()),
                u32::from(fetched.height()),
                fetched.format()
            ),
            (tile.width, tile.height, tile.format),
            "{name}: the preview reads other than the tile says"
        );
        rows.push((tile.name.clone(), tile.width, tile.height, tile.format));
    }
    Some((rows, source))
}

#[test]
fn the_texture_browser_lists_an_enhanced_particle_files_textures_as_a_legacy_ones() {
    let spark = vec![("spark".to_string(), 16, 8, GrcTextureFormat::DXT5)];
    let smoke = vec![("smoke".to_string(), 16, 8, GrcTextureFormat::DXT5)];
    at_both_targets(|edition, payload| {
        assert_eq!(
            browsed(&payload, None),
            Some((spark.clone(), TextureSource::Document)),
            "the textures a {edition} particle file lists of its own"
        );
        let model = Some(ypt_model_textures_section_id(MODEL));
        assert_eq!(
            browsed(&payload, model),
            Some((smoke.clone(), TextureSource::Entry { hash: MODEL })),
            "the textures a {edition} particle file's model lists"
        );
    });
}

#[test]
fn the_texture_browser_lists_an_enhanced_fragments_textures_as_a_legacy_ones() {
    let smoke = vec![("smoke".to_string(), 16, 8, GrcTextureFormat::DXT5)];
    let entry = TextureSource::Entry {
        hash: FRAGMENT_ENTRY_HASH,
    };
    for (edition, payload) in [
        ("Legacy", Gen8::fragment(a_fragment::<Gen8>())),
        ("Enhanced", Gen9::fragment(a_fragment::<Gen9>())),
    ] {
        assert_eq!(
            browsed(&payload, None),
            Some((smoke.clone(), entry)),
            "the textures a {edition} fragment lists"
        );
    }
}

#[test]
fn an_enhanced_fragments_selected_texture_is_inspected_as_an_enhanced_texture() {
    let (mut state, doc) = opened(Gen9::fragment(a_fragment::<Gen9>()));
    let selection = dict_entry_part_id(FRAGMENT_ENTRY_HASH, YPT_MODEL_KIND_TEXTURE, 0);
    state.selection.select(doc, selection);
    let shown = painted(crate::ui::panels::inspector::draw, &mut state);
    assert!(
        shows(&shown, "Texture (Enhanced)"),
        "an Enhanced fragment's Inspector shows no texture: {shown:?}"
    );
}

#[test]
fn an_enhanced_particle_file_has_the_inspector_a_legacy_one_has() {
    let texture = dict_entry_part_id(MODEL, YPT_MODEL_KIND_TEXTURE, 0);
    for (selection, heading) in [
        (ypt_effect_id(0), "Effect: fx"),
        (ypt_texture_id("spark"), "Embedded Texture"),
        (YPT_MODEL_DICT, "Embedded Model Dictionary"),
        (ypt_model_id(MODEL), "Embedded Model"),
        (texture, "Drawable Texture"),
    ] {
        at_both_targets(|edition, payload| {
            let (mut state, doc) = opened(payload);
            state.selection.select(doc, selection);
            let shown = painted(crate::ui::panels::inspector::draw, &mut state);
            assert!(
                shows(&shown, heading),
                "a {edition} particle file's Inspector shows no `{heading}`: {shown:?}"
            );
        });
    }
}

#[test]
fn an_enhanced_particle_file_has_the_timeline_a_legacy_one_has() {
    at_both_targets(|edition, payload| {
        let (mut state, _) = opened(payload);
        let shown = painted(crate::ui::panels::timeline::draw, &mut state);
        assert!(
            shows(&shown, "fx.em"),
            "a {edition} particle file's timeline shows no track for its event: {shown:?}"
        );
        assert!(
            !shows(&shown, "Timeline available when"),
            "a {edition} particle file's timeline is the placeholder for another document"
        );
    });
}

fn effects_in(payload: &DocPayload) -> usize {
    match payload {
        DocPayload::Resource(or_io::Asset::Particles(fx)) => list::effect_rules(fx).count(),
        DocPayload::Resource(or_io::Asset::ParticlesGen9(fx)) => list::effect_rules(fx).count(),
        _ => panic!("not a particle file"),
    }
}

#[test]
fn deleting_an_effect_from_an_enhanced_particle_files_outline_undoes_as_a_legacy_ones_does() {
    at_both_targets(|edition, payload| {
        let (mut state, doc) = opened(payload);
        let effects = |state: &AppState| effects_in(&state.document(doc).expect("open").payload);
        crate::ui::panels::hierarchy::delete_particle_row(&mut state, doc, ypt_effect_id(0));
        assert_eq!(effects(&state), 0, "the {edition} file keeps the effect");
        assert!(state.undo().is_some(), "the delete is not a step");
        assert_eq!(effects(&state), 1, "an undo does not bring the effect back");
        assert!(state.redo().is_some());
        assert_eq!(effects(&state), 0, "a redo does not delete it again");
    });
}

#[test]
fn a_staged_enhanced_particle_file_opens_with_its_outline_summary_and_textures() {
    const WHAT: &str =
        "a_staged_enhanced_particle_file_opens_with_its_outline_summary_and_textures";
    let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), "ypt")
        .target(TARGET_GEN9)
        .find();
    files.sort_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX));
    let staged = files.into_iter().find_map(|path| {
        let asset = or_io::open(&path).ok()?;
        matches!(asset, or_io::Asset::ParticlesGen9(_)).then_some((path, asset))
    });
    let Some((path, asset)) = staged else {
        skip_corpus(WHAT, format_args!("no Enhanced .ypt staged"));
        return;
    };
    let or_io::Asset::ParticlesGen9(fx) = &asset else {
        unreachable!("staged as an Enhanced particle file");
    };
    let effects = list::effect_rules(fx).count();
    let textures = fx.texture_dict.as_ref().map_or(0, |d| d.entries.len());
    let payload = DocPayload::Resource(asset);

    let summary = capabilities::for_payload(&payload).and_then(|row| row.selection_summary);
    let summary = summary.map_or_else(String::new, |summary| summary(&payload, DOC_ROOT_SEL));
    assert_eq!(
        summary,
        format!("{effects} effects (Enhanced)"),
        "{}",
        path.display()
    );

    let listed = match textures_in(&payload, None) {
        TextureListing::Textures { textures, .. } => textures.len(),
        TextureListing::Nothing(_) => 0,
    };
    assert_eq!(listed, textures, "{}: the texture browser", path.display());

    let (mut state, _) = opened(payload);
    let shown = painted(crate::ui::panels::hierarchy::draw_outline, &mut state);
    assert!(
        shows(&shown, &format!("Effects ({effects})")),
        "{}: the Outline shows no effects section: {shown:?}",
        path.display()
    );
    if textures > 0 {
        assert!(
            shows(&shown, &format!("Textures ({textures})")),
            "{}: the Outline shows no textures section",
            path.display()
        );
    }
}
