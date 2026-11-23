//! A model opened from a folder is drawn with the dictionary that belongs to it: the one of its
//! own name, or for a ped garment the variant chosen among the files that dress it; never the
//! one a model opened before it left open.
//!
//! ⚠ Each case opens real files in a real folder, because what is under test is which rows of a
//! folder's tree a model looks beside.

use or_io::codec::texture::GrcTextureFormat;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::{GtaDrawable, PgDictionary};
use or_io::Asset;

use super::document::DocumentId;
use super::opening_a_ydd::{finish_load, node_at};
use super::texture_resolution::{resolve, ResolutionRule};
use super::{AppState, DocumentKind, Prefs, RpfEntryBinding};

/// A `.ytd` holding one 8×8 texture called `texture`.
fn ytd(texture: &str) -> Vec<u8> {
    let mut dict = PgDictionary::<GrcTexture>::default();
    let mut image = GrcTexture {
        name: texture.to_string(),
        width: 8,
        height: 8,
        depth: 1,
        mip_stride: 16,
        format: GrcTextureFormat::DXT1.to_raw_format(),
        mip_count: 1,
        backing_store: vec![0; 32],
        ..Default::default()
    };
    image.set_template_type(1 << 28);
    dict.insert(image);
    Asset::TextureDict(dict).to_bytes().expect("a dictionary")
}

/// A `.ydd` holding no drawable.
fn ydd() -> Vec<u8> {
    Asset::DrawableDict(PgDictionary::<GtaDrawable>::default())
        .to_bytes()
        .expect("a drawable dictionary")
}

struct Folder {
    _dir: tempfile::TempDir,
    state: AppState,
    folder: DocumentId,
}

impl Folder {
    fn of(files: &[(&str, Vec<u8>)]) -> Self {
        let dir = tempfile::tempdir().expect("a scratch folder");
        for (name, bytes) in files {
            std::fs::write(dir.path().join(name), bytes).expect("a file");
        }
        let mut state = AppState::from_prefs(Prefs::default());
        state.prefs.explorer = Default::default();
        state.open_folder(dir.path().to_path_buf());
        let folder = state.active_document.expect("the folder opens");
        Self {
            _dir: dir,
            state,
            folder,
        }
    }

    /// Open `name` as a click in Files does, and the document it leaves active.
    fn open(&mut self, name: &str) -> DocumentId {
        let node = node_at(&mut self.state, self.folder, name);
        self.state.clear_error();
        self.state.open_rpf_entry(self.folder, node);
        finish_load(&mut self.state).unwrap_or_else(|why| panic!("{name}: {why}"))
    }

    /// What the active model's first texture is drawn as, and by which rule.
    fn drawn_with(&self, asked: &str) -> Option<(String, ResolutionRule)> {
        let found = resolve(&self.state, None, None, asked)?;
        Some((found.texture.name().to_string(), found.origin.rule))
    }

    fn dictionaries(&self) -> usize {
        (self.state.documents.iter())
            .filter(|d| d.kind == DocumentKind::Ytd)
            .count()
    }
}

#[test]
fn a_model_opens_its_own_dictionary_though_another_one_holds_the_names_it_states() {
    let mut f = Folder::of(&[
        ("a.ydd", ydd()),
        ("a.ytd", ytd("diffuse")),
        ("b.ydd", ydd()),
        ("b.ytd", ytd("diffuse")),
    ]);
    f.open("a.ydd");
    assert!(f.state.try_auto_open_texture_sibling(), "a.ytd opens");
    assert_eq!(f.dictionaries(), 1);

    let b = f.open("b.ydd");
    assert_eq!(
        f.drawn_with("diffuse"),
        None,
        "a's dictionary is not b's, though it holds the name"
    );
    assert!(
        f.state.try_auto_open_texture_sibling(),
        "b asks for its own though no name is missing"
    );
    assert_eq!(f.dictionaries(), 2);
    let (_, rule) = f.drawn_with("diffuse").expect("b.ytd answers");
    assert_eq!(rule, ResolutionRule::SameStem);
    assert_eq!(f.state.active_document, Some(b));

    let found = resolve(&f.state, None, None, "diffuse").expect("b.ytd answers");
    assert!(
        found.also_in.is_empty(),
        "a dictionary opened for another model is no candidate: {:?}",
        found.also_in
    );
}

#[test]
fn a_dictionary_the_user_opened_still_answers_for_every_model() {
    let mut f = Folder::of(&[("a.ydd", ydd()), ("shared.ytd", ytd("diffuse"))]);
    f.open("shared.ytd");
    f.open("a.ydd");
    let (_, rule) = f.drawn_with("diffuse").expect("an open dictionary answers");
    assert_ne!(rule, ResolutionRule::SameStem);
}

fn wardrobe() -> Folder {
    Folder::of(&[
        ("pack^accs_026_u.ydd", ydd()),
        ("pack^accs_diff_026_b_uni.ytd", ytd("1")),
        ("pack^accs_diff_026_a_uni.ytd", ytd("2")),
        ("pack^accs_diff_026_c_uni.ytd", ytd("15")),
        ("pack^accs_027_u.ydd", ydd()),
        ("pack^accs_diff_027_a_uni.ytd", ytd("26")),
        ("pack^readme.txt", b"not a texture".to_vec()),
    ])
}

fn labels(f: &Folder, garment: DocumentId) -> Vec<String> {
    f.state
        .garment_variants(garment)
        .iter()
        .map(|v| v.label())
        .collect()
}

#[test]
fn a_garment_is_drawn_with_the_first_of_the_files_that_dress_it_whatever_they_hold() {
    let mut f = wardrobe();
    let garment = f.open("pack^accs_026_u.ydd");

    assert!(f.state.ensure_garment_dressed(), "it is dressed on sight");
    assert_eq!(
        labels(&f, garment),
        ["a \u{00B7} uni", "b \u{00B7} uni", "c \u{00B7} uni"],
        "the other garment's file and the note dress nothing"
    );
    assert_eq!(
        f.drawn_with("whatever the material names"),
        Some(("2".to_string(), ResolutionRule::Clothing)),
        "variant a holds a texture called 2"
    );
    assert!(!f.state.ensure_garment_dressed(), "once");
    assert_eq!(f.dictionaries(), 1);
}

#[test]
fn choosing_another_variant_draws_it_and_closes_the_one_it_replaces() {
    let mut f = wardrobe();
    let garment = f.open("pack^accs_026_u.ydd");
    f.state.ensure_garment_dressed();
    let b = f.state.garment_variants(garment)[1].node;

    f.state.choose_garment_variant(garment, b).expect("b opens");

    assert_eq!(f.state.garment_variant(garment), Some(b));
    assert_eq!(
        f.drawn_with("x"),
        Some(("1".to_string(), ResolutionRule::Clothing))
    );
    assert_eq!(f.dictionaries(), 1, "a is closed, which the app opened");
}

#[test]
fn a_file_that_does_not_dress_the_garment_is_refused() {
    let mut f = wardrobe();
    let garment = f.open("pack^accs_026_u.ydd");
    f.state.ensure_garment_dressed();
    let other = node_at(&mut f.state, f.folder, "pack^accs_diff_027_a_uni.ytd");

    let why = f.state.choose_garment_variant(garment, other).unwrap_err();

    assert!(why.contains("does not dress"), "{why}");
    assert_eq!(f.drawn_with("x").map(|(name, _)| name), Some("2".into()));
}

#[test]
fn each_garment_keeps_its_own_variant_and_closing_it_closes_the_dictionary() {
    let mut f = wardrobe();
    let first = f.open("pack^accs_026_u.ydd");
    f.state.ensure_garment_dressed();
    let second = f.open("pack^accs_027_u.ydd");
    f.state.ensure_garment_dressed();

    assert_eq!(labels(&f, second), ["a \u{00B7} uni"]);
    assert_eq!(
        f.drawn_with("x").map(|(name, _)| name),
        Some("26".into()),
        "the active garment's"
    );
    f.state.set_active_document(first);
    assert_eq!(f.drawn_with("x").map(|(name, _)| name), Some("2".into()));
    assert_eq!(f.dictionaries(), 2);

    f.state.close_document(first);
    f.state.close_document(second);
    assert_eq!(f.dictionaries(), 0, "nothing the app opened is left open");
}

#[test]
fn a_dictionary_the_user_opens_stays_when_the_garment_is_drawn_with_another() {
    let mut f = wardrobe();
    let garment = f.open("pack^accs_026_u.ydd");
    f.state.ensure_garment_dressed();
    let a = f.state.garment_variants(garment)[0].node;
    let b = f.state.garment_variants(garment)[1].node;

    f.state.open_rpf_entry(f.folder, a);
    f.state.set_active_document(garment);
    f.state.choose_garment_variant(garment, b).expect("b opens");

    assert_eq!(f.dictionaries(), 2, "a is the user's and stays open");
    let row = RpfEntryBinding {
        parent_doc_id: f.folder,
        entry_id: a,
    };
    let opened = f.state.entry_document(row).expect("a is open");
    assert!(
        f.state.document(opened).is_some_and(|d| f.state.has_tab(d)),
        "a has a tab, as a file the user opened does"
    );
}

/// Every text the gallery says in `frames` frames.
fn gallery_says(state: &mut AppState, frames: usize) -> Vec<String> {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut said = Vec::new();
    for _ in 0..frames {
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            crate::ui::panels::gallery::draw(ui, state, None)
        });
        said.extend(
            (output.platform_output.accesskit_update.into_iter())
                .flat_map(|update| update.nodes)
                .filter_map(|(_, node)| node.label().or_else(|| node.value()).map(str::to_string)),
        );
    }
    said
}

#[test]
fn the_gallery_lists_a_garments_variants_under_a_heading_of_their_own() {
    let mut f = wardrobe();
    f.open("pack^accs_026_u.ydd");

    let said = gallery_says(&mut f.state, 3);

    for want in ["Clothing textures", "3 textures"] {
        assert!(said.iter().any(|t| t == want), "{want}: {said:?}");
    }
    assert!(
        !said.iter().any(|t| t == "Embedded in this model"),
        "this garment embeds nothing, so that heading would name an empty group: {said:?}"
    );
    assert_eq!(
        f.drawn_with("x").map(|(name, _)| name),
        Some("2".into()),
        "drawing the gallery dressed the garment"
    );
}

#[test]
fn a_model_that_nothing_dresses_shows_no_clothing_group() {
    let mut f = Folder::of(&[
        ("prop_bench_01.ydd", ydd()),
        ("prop_bench_01.ytd", ytd("x")),
    ]);
    f.open("prop_bench_01.ydd");

    let said = gallery_says(&mut f.state, 2);

    assert!(
        !said.iter().any(|t| t.contains("Clothing")),
        "a prop has no variants: {said:?}"
    );
}

/// A `.yft` holding no fragment.
fn yft() -> Vec<u8> {
    Asset::Fragment(or_io::gta5::GtaFragType::default())
        .to_bytes()
        .expect("a fragment")
}

#[test]
fn a_vehicle_opens_the_dictionaries_every_vehicle_shares_only_while_a_name_is_still_missing() {
    use super::texture_resolution::report;
    let mut f = Folder::of(&[
        ("car.yft", yft()),
        ("car.ytd", ytd("own")),
        ("vehshare.ytd", ytd("generic")),
        ("vehshare_worn.ytd", ytd("worn")),
    ]);
    f.open("car.yft");
    assert!(f.state.try_auto_open_texture_sibling(), "car.ytd opens");
    assert!(
        !f.state.try_auto_open_shared_dictionary(),
        "no name is missing, so no shared dictionary is asked for"
    );
    let wanted = |names: &[&str]| {
        names
            .iter()
            .map(|n| (None, (*n).to_string()))
            .collect::<Vec<_>>()
    };
    f.state.texture_report = report(&f.state, None, wanted(&["generic", "worn"]), 1);
    assert_eq!(f.drawn_with("generic"), None);

    assert!(
        f.state.try_auto_open_shared_dictionary(),
        "vehshare.ytd opens"
    );
    assert!(
        f.drawn_with("generic").is_some(),
        "and answers for the name it holds"
    );
    assert_eq!(f.dictionaries(), 2);

    // `worn` is still missing, so the next one is asked for, once
    f.state.texture_report = report(&f.state, None, wanted(&["worn"]), 2);
    assert!(
        f.state.try_auto_open_shared_dictionary(),
        "vehshare_worn.ytd opens"
    );
    assert!(f.drawn_with("worn").is_some());
    f.state.texture_report = report(&f.state, None, wanted(&["nothing"]), 3);
    assert!(
        !f.state.try_auto_open_shared_dictionary(),
        "no one is asked twice, and the rest are not in the folder"
    );
}

#[test]
fn a_model_that_is_no_vehicle_never_opens_the_shared_dictionaries() {
    use super::texture_resolution::report;
    let mut f = Folder::of(&[("a.ydd", ydd()), ("vehshare.ytd", ytd("generic"))]);
    f.open("a.ydd");
    f.state.texture_report = report(&f.state, None, [(None, "generic".to_string())], 1);
    assert!(!f.state.try_auto_open_shared_dictionary());
}
