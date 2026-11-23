//! A recook of a source reads again every tab open on the file it wrote, in place, and what a model is drawn from.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder: no install and no keys. Each
//! case cooks a textures source whose one image is dark red, opens the cooked `.ytd` as a tab, and
//! saves another image beside the DDS the way an image editor would.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use or_io::authoring::{FormatChoice, TextureBuildOptions};
use or_io::codec::texture::{decode_level, ChannelView, GrcTextureFormat, RgbaImage};
use or_io::gta5::{GtaDrawable, PgDictionary};
use or_io::Asset;
use or_source::cook::Recook;

use super::document::{DocPayload, Document, DocumentId};
use super::edit_history::Change;
use super::opening_a_ydd::finish_load;
use super::texture_cache::TextureKey;
use super::textures::{self, TextureListing};
use super::the_project_workspace::{opened, png, row, until, write};
use super::workbench::AssetState;
use super::{AppState, Prefs, TextureSource};
use crate::render::viewport::MeshCacheKey;

/// A project with a textures source cooked once, and its cooked file open as a tab.
struct Cooked {
    state: AppState,
    source: PathBuf,
    output: PathBuf,
    tab: DocumentId,
    /// Last, so the workers and the watchers are gone before their folder is.
    _dir: tempfile::TempDir,
}

fn a_cooked_tab() -> Cooked {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    let source = row(&state, "logo").path.clone();
    let output = row(&state, "logo").output.clone().expect("an output");
    let mut cooked = Cooked {
        state,
        source,
        output,
        tab: 0,
        _dir: dir,
    };
    cooked.save("textures/logo.png");
    let (output, tab_of) = (
        cooked.output.clone(),
        row(&cooked.state, "logo").key.clone(),
    );
    until(&mut cooked.state, "the first cook", |s| {
        output.is_file() && s.workbench.state_of(row(s, "logo")) == AssetState::Cooked
    });
    cooked.state.open_asset(&tab_of, None);
    cooked.tab = finish_load(&mut cooked.state).expect("the cooked file opens as a tab");
    cooked
}

impl Cooked {
    /// What the watcher reports when `file` in the source is saved.
    fn save(&mut self, file: &str) {
        let long_ago = Instant::now() - Duration::from_secs(1);
        let path = self.source.join(file);
        self.state.workbench.file_saved(path, long_ago);
    }

    /// The image editor saves another image beside the DDS.
    fn save_an_image(&mut self, shade: u8) {
        png(&self.source.join("textures/logo.png"), shade);
        self.save("textures/logo.png");
    }

    fn tab(&self) -> &Document {
        self.state.document(self.tab).expect("the tab is open")
    }

    fn recooks(&self) -> usize {
        let history = self.state.workbench.history();
        history
            .iter()
            .filter(|e| e.text.starts_with("Compiled"))
            .count()
    }

    /// The red of the first pixel of the first texture the tab shows.
    fn red(&self) -> u8 {
        red_of(&self.state, self.tab)
    }

    /// The preview panel's decode of the tab's texture, as it keeps it.
    fn warm_the_preview(&mut self) -> TextureKey {
        let (name, source) = self.first_texture();
        let texture = textures::texture_at(&self.tab().payload, source, &name)
            .expect("the texture")
            .into_owned();
        let key = (self.tab, name, ChannelView::Rgba);
        let ctx = egui::Context::default();
        let cached = self
            .state
            .texture_cache
            .get_or_decode(&ctx, key.clone(), &texture);
        assert!(cached.is_some(), "the preview decodes");
        key
    }

    fn first_texture(&self) -> (String, TextureSource) {
        let TextureListing::Textures { textures, source } =
            textures::textures_in(&self.tab().payload, None)
        else {
            panic!("the tab holds no textures");
        };
        (textures[0].name.clone(), source)
    }

    /// A step the person made in the tab: another image over the texture.
    fn replace_the_texture(&mut self) {
        let (name, source) = self.first_texture();
        let image = RgbaImage::new(8, 8);
        let dxt5 = FormatChoice::Fixed(GrcTextureFormat::DXT5);
        let options = TextureBuildOptions::default();
        let tab = self.tab;
        let done = self
            .state
            .import_texture_over(tab, source, &name, &image, dxt5, &options);
        done.expect("the tab's dictionary takes another image");
    }
}

fn red_of(state: &AppState, tab: DocumentId) -> u8 {
    let payload = &state.document(tab).expect("the tab is open").payload;
    let TextureListing::Textures { textures, source } = textures::textures_in(payload, None) else {
        panic!("the tab holds no textures");
    };
    let texture = textures::texture_at(payload, source, &textures[0].name).expect("the texture");
    decode_level(&texture, 0).expect("it decodes").data[0]
}

#[test]
fn a_saved_image_changes_the_pixels_of_a_tab_open_on_the_cooked_file() {
    let mut c = a_cooked_tab();
    assert!(c.red() < 100, "the first image is dark: {}", c.red());
    let key = c.warm_the_preview();
    let (tab, tabs, epoch) = (c.tab, c.state.documents.len(), c.tab().payload_epoch());

    c.save_an_image(200);
    until(&mut c.state, "the tab to show the new image", |s| {
        red_of(s, tab) > 150
    });

    assert_eq!(c.state.documents.len(), tabs, "no tab was opened or closed");
    assert!(
        c.tab().payload_epoch() != epoch,
        "what is drawn from it is keyed on this"
    );
    assert!(
        c.state.texture_cache.get(&key).is_none(),
        "the preview still holds the old pixels, keyed by the tab and the texture's name"
    );
    assert!(
        !c.tab().dirty,
        "a tab read again from disk has no unsaved edits"
    );
}

#[test]
fn a_failed_cook_leaves_the_pixels_of_an_open_tab_and_lists_a_problem() {
    let mut c = a_cooked_tab();
    let (before, epoch, recooked) = (c.red(), c.tab().payload_epoch(), c.state.recooked);

    write(
        &c.source.join("textures.json"),
        b"{ \"textures\": [ { \"name\": \"logo\", \"pack\": {} } ] }",
    );
    c.save("textures.json");
    until(&mut c.state, "the failure", |s| {
        s.workbench.state_of(row(s, "logo")) == AssetState::Failed
    });
    until(&mut c.state, "the problem", |s| {
        s.workbench
            .findings()
            .iter()
            .any(|f| f.text.starts_with("logo:"))
    });

    assert_eq!(c.red(), before, "the tab shows what it showed");
    assert_eq!(c.tab().payload_epoch(), epoch, "nothing was read again");
    assert_eq!(
        c.state.recooked, recooked,
        "nothing drawn from the file was told it changed"
    );
}

#[test]
fn a_cooked_file_the_app_cannot_read_leaves_the_tab_and_lists_a_problem() {
    let mut c = a_cooked_tab();
    let (before, epoch) = (c.red(), c.tab().payload_epoch());

    std::fs::write(&c.output, b"not a resource").unwrap();
    let recook = Recook::Cooked {
        source: c.source.clone(),
        output: c.output.clone(),
    };
    c.state.took_recook(recook);
    c.state.poll_workbench();

    assert_eq!(c.red(), before);
    assert_eq!(c.tab().payload_epoch(), epoch);
    assert_eq!(
        c.state.workbench.state_of(row(&c.state, "logo")),
        AssetState::Failed
    );
    let said = c
        .state
        .workbench
        .findings()
        .iter()
        .find(|f| f.text.starts_with("logo:"));
    let said = said.map(|f| f.text.as_str()).unwrap_or_default();
    assert!(said.contains("Could not read logo.ytd again"), "{said}");
}

/// ⚠ A step that holds a value of the old contents, a patch of its image or a whole texture, goes with them.
#[test]
fn an_undo_after_a_reload_does_not_put_the_pixels_of_the_cook_before_back() {
    let mut c = a_cooked_tab();
    let (tab, first) = (c.tab, c.red());
    c.replace_the_texture();
    assert!(c.red() != first, "the step changed the pixels");
    c.state.document_mut(tab).unwrap().mark_saved();

    c.save_an_image(200);
    until(&mut c.state, "the tab to show the new image", |s| {
        red_of(s, tab) > 150
    });

    c.state.set_active_document(tab);
    assert_eq!(
        c.state.undo(),
        None,
        "the step holds the first cook's pixels"
    );
    assert!(c.red() > 150, "the tab shows the new cook: {}", c.red());
    assert!(!c.tab().dirty, "nothing was written over the file");
}

/// ⚠ Only a step that holds nothing of the old contents stays, which is a move of a shader by index.
#[test]
fn the_steps_that_hold_nothing_of_the_old_contents_stay_through_a_reload() {
    let mut c = a_cooked_tab();
    let tab = c.tab;
    let patch = Change::Bytes {
        at: 0,
        other: vec![0; 4],
    };
    let moved = Change::MoveShader {
        owner: 0,
        from: 0,
        to: 1,
    };
    let history = &mut c.state.document_mut(tab).unwrap().history;
    history.push("Set a value".into(), patch, None);
    history.push("Move shader".into(), moved, None);
    c.replace_the_texture();
    c.state.document_mut(tab).unwrap().mark_saved();

    c.save_an_image(200);
    until(&mut c.state, "the tab to show the new image", |s| {
        red_of(s, tab) > 150
    });

    let kept: Vec<&str> = (c.tab().history.done().iter())
        .map(|step| step.label.as_str())
        .collect();
    assert_eq!(kept, ["Move shader"]);
    assert!(
        c.tab().history.at_saved(),
        "the tab stands where the file does"
    );
}

#[test]
fn a_tab_with_unsaved_edits_waits_for_an_answer_and_is_then_read_again() {
    let mut c = a_cooked_tab();
    c.replace_the_texture();
    let (tab, epoch) = (c.tab, c.tab().payload_epoch());
    assert!(c.tab().dirty);

    c.save_an_image(200);
    until(&mut c.state, "the question", |s| {
        !s.reload_conflicts.is_empty()
    });
    assert_eq!(c.state.reload_conflicts, [c.output.clone()]);
    assert_eq!(
        c.tab().payload_epoch(),
        epoch,
        "the edits were replaced before the answer"
    );
    assert!(c.tab().dirty);

    c.state.answer_reload_conflicts(true);
    assert!(red_of(&c.state, tab) > 150, "the answer was to reload");
    assert!(!c.tab().dirty);
}

#[test]
fn a_model_drawn_from_a_recooked_dictionary_is_built_again() {
    let mut c = a_cooked_tab();
    let model = c.state.documents.iter().map(|d| d.id).max().unwrap() + 1;
    let path = c.output.with_file_name("model.ydr");
    let payload = DocPayload::Resource(or_io::Asset::DrawableDict(Default::default()));
    c.state
        .documents
        .push(Document::new(model, Some(path), payload));
    let drawn = MeshCacheKey::of(&c.state, model, 0);
    let recooks = c.recooks();

    c.save_an_image(200);
    until(&mut c.state, "the recook", |s| s.recooked > 0);

    assert!(c.recooks() > recooks);
    assert!(
        MeshCacheKey::of(&c.state, model, 0) != drawn,
        "the viewport keeps the meshes and the textures it built from the old dictionary"
    );
}

/// A `.ydd` holding `entries` empty drawables.
fn ydd(entries: u32) -> Vec<u8> {
    let mut dict = PgDictionary::<GtaDrawable>::default();
    for code in 0..entries {
        dict.codes.push(code);
        dict.entries.push(Some(Box::default()));
    }
    Asset::DrawableDict(dict)
        .to_bytes()
        .expect("a drawable dictionary")
}

#[test]
fn a_tab_of_a_model_is_read_again_in_place_whatever_its_kind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.ydd");
    std::fs::write(&path, ydd(1)).unwrap();
    let mut state = AppState::from_prefs(Prefs::default());
    state.load_file(path.clone());
    let tab = finish_load(&mut state).expect("the model opens");
    let held = |state: &AppState| match &state.document(tab).unwrap().payload {
        DocPayload::Resource(Asset::DrawableDict(dict)) => dict.entries.len(),
        _ => panic!("not a drawable dictionary"),
    };
    assert_eq!(held(&state), 1);

    std::fs::write(&path, ydd(3)).unwrap();
    assert_eq!(state.reload_open_file(&path, false), Ok(1));

    assert_eq!(
        held(&state),
        3,
        "the tab still shows the file it was opened from"
    );
    assert_eq!(state.active_document, Some(tab));
}

#[test]
fn a_plugin_is_served_the_file_a_reload_left_and_not_the_one_before() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("model.ydd");
    std::fs::write(&path, ydd(1)).unwrap();
    let mut state = AppState::from_prefs(Prefs::default());
    state.load_file(path.clone());
    let tab = finish_load(&mut state).expect("the model opens");
    let before = state.served_file(tab).expect("the file it would save");

    std::fs::write(&path, ydd(3)).unwrap();
    assert_eq!(state.reload_open_file(&path, false), Ok(1));

    let after = state.served_file(tab).expect("the file it would save");
    assert!(
        !Arc::ptr_eq(&before, &after) && before != after,
        "the plugin is still served the bytes of the file before the reload"
    );
}
