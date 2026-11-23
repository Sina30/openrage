//! The viewport and the thumbnails draw an embedded texture as it stands after a re-encode or an
//! import, and the Animation workspace draws an edit to any document its preview draws.

use or_io::authoring::{FormatChoice, TextureBuildOptions};
use or_io::codec::texture::{GrcTextureFormat, RgbaImage};
use or_io::desc::ScalarValue;
use or_test_support::{skip_corpus, Corpus};

use super::an_edit_stays_where_it_was_made::{
    drawable_with_a_material, edit_a_material, grid_row, open,
};
use super::textures::{textures_in, TextureListing};
use super::{
    AppState, DocPayload, Document, DocumentId, MeshSource, SkeletonSource, TextureSource,
};
use crate::render::gpu_budget::{GpuTextureKey, GpuTextures};
use crate::render::meshes::build_meshes_for_active_document;
use crate::render::thumbnails::ThumbKey;
use crate::render::viewport::{bound_texture, MeshCacheKey};
use crate::ui::dock::Workspace;
use crate::ui::panels::selection_ids::DOC_ROOT_SEL;

/// A staged Legacy drawable or fragment, open and active, whose meshes bind a texture it embeds beside another.
struct Embedding {
    state: AppState,
    doc: DocumentId,
    source: TextureSource,
    bound: String,
    other: String,
    format: GrcTextureFormat,
}

/// The smallest such file, drawables before fragments.
fn embedding() -> Option<Embedding> {
    for ext in ["ydr", "yft"] {
        let mut files = Corpus::new(env!("CARGO_MANIFEST_DIR"), ext).find();
        files.sort_by_key(|p| std::fs::metadata(p).map_or(u64::MAX, |m| m.len()));
        for path in files {
            let Ok(asset) = or_io::open(&path) else {
                continue;
            };
            let mut state = AppState::new();
            let doc = Document::new(1, Some(path), DocPayload::Resource(asset));
            let TextureListing::Textures { textures, source } = textures_in(&doc.payload, None)
            else {
                continue;
            };
            state.documents = vec![doc];
            state.active_document = Some(1);
            let names: Vec<String> = (build_meshes_for_active_document(&state).into_iter())
                .filter_map(|upload| upload.texture_name)
                .collect();
            let resolved = |name: &str| bound_texture(&state, None, None, name).is_some();
            let embedded: Vec<(String, GrcTextureFormat)> = (textures.iter())
                .map(|t| (t.name.to_ascii_lowercase(), t.format))
                .filter(|(name, _)| resolved(name))
                .collect();
            let Some((bound, format)) = embedded.iter().find(|(name, _)| names.contains(name))
            else {
                continue;
            };
            let Some((other, _)) = embedded.iter().find(|(name, _)| name != bound) else {
                continue;
            };
            let (bound, other, format) = (bound.clone(), other.clone(), *format);
            return Some(Embedding {
                state,
                doc: 1,
                source,
                bound,
                other,
                format,
            });
        }
    }
    None
}

impl Embedding {
    /// Where the viewport and a thumbnail keep their copies of `name`.
    fn copies(name: &str) -> [GpuTextureKey; 2] {
        [
            (None, name.to_string()),
            (Some(DOC_ROOT_SEL), name.to_string()),
        ]
    }

    /// What the viewport's meshes and the thumbnail are compared on before they are built again.
    fn drawn(&self) -> (MeshCacheKey, ThumbKey) {
        let document = self.state.document(self.doc).expect("open");
        (
            MeshCacheKey::of(&self.state, self.doc, 0),
            ThumbKey::of(document, DOC_ROOT_SEL),
        )
    }

    /// Bind both textures as the viewport and a thumbnail do, and say which copies were uploaded again.
    fn draw(&self, gpu: &mut GpuTextures<()>) -> Vec<GpuTextureKey> {
        let mut uploaded = Vec::new();
        for key in Self::copies(&self.bound)
            .into_iter()
            .chain(Self::copies(&self.other))
        {
            let (_, stamp) =
                bound_texture(&self.state, None, key.0, &key.1).expect("it still resolves");
            if gpu.get(&key, stamp).is_none() {
                gpu.insert(key.clone(), (), 1, stamp);
                uploaded.push(key);
            }
        }
        uploaded
    }

    /// Make `edit` and draw: the meshes, the thumbnail and the edited texture's copies alone are built again.
    fn after(&mut self, gpu: &mut GpuTextures<()>, what: &str, edit: impl FnOnce(&mut AppState)) {
        // As the shell does between two clicks, so each edit is a step of its own.
        self.state.seal_edits();
        let before = self.drawn();
        edit(&mut self.state);
        let now = self.drawn();
        assert!(now.0 != before.0, "{what} did not build the meshes again");
        assert!(now.1 != before.1, "{what} did not draw the thumbnail again");
        assert_eq!(
            self.draw(gpu),
            Self::copies(&self.bound),
            "{what}: the copies uploaded again are not the edited texture's alone"
        );
        assert_eq!(gpu.bytes(), 4, "{what}: a dropped copy is still counted");
    }
}

#[test]
fn a_reencoded_or_imported_embedded_texture_is_uploaded_again_and_no_other() {
    const WHAT: &str = "a_reencoded_or_imported_embedded_texture_is_uploaded_again_and_no_other";
    let Some(mut e) = embedding() else {
        skip_corpus(
            WHAT,
            "no staged Legacy .ydr or .yft binds an embedded texture beside another",
        );
        return;
    };
    let mut gpu = GpuTextures::new(usize::MAX);
    assert_eq!(
        e.draw(&mut gpu).len(),
        4,
        "the first draw uploads every copy"
    );

    let (doc, source, name) = (e.doc, e.source, e.bound.clone());
    let format = match e.format {
        GrcTextureFormat::DXT1 => GrcTextureFormat::DXT5,
        _ => GrcTextureFormat::DXT1,
    };
    let options = TextureBuildOptions::default();
    e.after(&mut gpu, "the re-encode", |state| {
        let fixed = FormatChoice::Fixed(format);
        let reencoded = state.reencode_texture_in_place(doc, source, &name, fixed, &options);
        reencoded.expect("the bound texture re-encodes");
    });
    e.after(&mut gpu, "its undo", |state| {
        assert!(state.undo().is_some())
    });

    // Two images of one size into one format, so the second import changes the pixels alone.
    let import = |state: &mut AppState, colour: [u8; 4]| {
        let mut image = RgbaImage::new(16, 16);
        image.set_pixel(3, 5, colour);
        let fixed = FormatChoice::Fixed(GrcTextureFormat::DXT1);
        let imported = state.import_texture_over(doc, source, &name, &image, fixed, &options);
        imported.expect("an image imports over the bound texture");
    };
    e.after(&mut gpu, "the import", |state| {
        import(state, [200, 40, 90, 255])
    });
    e.after(&mut gpu, "a second import", |state| {
        import(state, [20, 220, 60, 255])
    });
    e.after(&mut gpu, "its undo", |state| {
        assert!(state.undo().is_some())
    });
    e.after(&mut gpu, "the first import's undo", |state| {
        assert!(state.undo().is_some())
    });
}

#[test]
fn an_edit_to_a_document_the_animation_preview_draws_builds_its_meshes_again() {
    const WHAT: &str = "an_edit_to_a_document_the_animation_preview_draws_builds_its_meshes_again";
    let dir = tempfile::tempdir().expect("a scratch directory");
    let Some((path, material)) = drawable_with_a_material(dir.path()) else {
        skip_corpus(WHAT, "no staged Legacy .ydr carries a material value");
        return;
    };
    let mut state = AppState::new();
    state.documents.clear();
    let drawable = open(&mut state, &path);
    let clips = DocPayload::Resource(or_io::Asset::Clips(Default::default()));
    let active = state.insert_document(dir.path().join("clips.ycd"), clips, None, None);
    state.active_workspace = Workspace::Animation;
    state.preview_scene.add_mesh(MeshSource {
        document: drawable,
        entry: None,
    });
    assert!(
        !build_meshes_for_active_document(&state).is_empty(),
        "{}: the preview draws no mesh of it",
        path.display()
    );
    let drawn = |state: &AppState| MeshCacheKey::of(state, active, 0);

    // A drag in an inspector pinned to the drawable, read back once the gesture ends.
    let opened = drawn(&state);
    let (label, edit) = grid_row(&state, drawable, "Distances [0]");
    assert!(state.patch_scalar(drawable, &label, edit, ScalarValue::F32(1234.5)));
    state.seal_edits();
    let dragged = drawn(&state);
    assert!(
        dragged != opened,
        "a grid edit of a mesh source was not drawn"
    );

    edit_a_material(&mut state, drawable, material);
    let edited = drawn(&state);
    assert!(
        edited != dragged,
        "a material edit of a mesh source was not drawn"
    );

    // With no mesh chosen, the preview draws the skeleton's own document.
    state.preview_scene.remove_mesh(MeshSource {
        document: drawable,
        entry: None,
    });
    state
        .preview_scene
        .pin_skeleton(Some(SkeletonSource::Drawable(drawable)));
    let skeleton = drawn(&state);
    edit_a_material(&mut state, drawable, material);
    assert!(
        drawn(&state) != skeleton,
        "a material edit of the skeleton's document was not drawn"
    );
    assert_eq!(
        state.active_document,
        Some(active),
        "the edits moved the focus"
    );
}
