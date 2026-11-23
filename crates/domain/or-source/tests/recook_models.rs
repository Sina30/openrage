//! Saving a glb inside a model source recooks that one asset into the shadow folder, as saving an image does a texture.

mod model_fixtures;

use std::fs;
use std::path::{Path, PathBuf};

use model_fixtures::{drawable, geometry, meshes, read_ydd, read_ydr, same_positions, ydd, ydr};
use or_interchange::gltf::{self, Container};
use or_interchange::{drawable_scene, ExtractOptions, LodSelection};
use or_source::asset::Source;
use or_source::cook::{Cooker, Ignored, Recook};
use or_source::models::{self, Extraction};

fn how() -> Extraction<'static> {
    Extraction {
        from_game: false,
        virtual_path: None,
        textures: false,
        skeleton: None,
    }
}

struct Project {
    root: tempfile::TempDir,
}

impl Project {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }

    fn shadow(&self) -> PathBuf {
        self.root.path().join("build").join("shadow")
    }

    fn cooker(&self) -> Cooker {
        Cooker::new(self.shadow())
    }

    fn id(dir: &Path) -> String {
        Source::open(dir).unwrap().manifest.id
    }
}

fn subject() -> Vec<u8> {
    ydr(drawable(
        "subject",
        vec![geometry(6, false, 0.0), geometry(9, false, 40.0)],
        None,
    ))
}

fn lifted_glb(bytes: &[u8], by: f32) -> Vec<u8> {
    let mut scene = drawable_scene(
        "subject",
        &read_ydr(bytes).base,
        &ExtractOptions {
            lods: LodSelection::All,
            textures: false,
        },
    );
    for primitive in scene
        .objects
        .iter_mut()
        .flat_map(|o| o.meshes.iter_mut())
        .flat_map(|m| m.primitives.iter_mut())
    {
        for position in &mut primitive.positions {
            position[2] += by;
        }
    }
    gltf::write(&scene, "subject", Container::Glb).primary
}

#[test]
fn saving_a_glb_recooks_that_one_drawable_into_the_shadow() {
    let project = Project::new();
    let bytes = subject();
    let folder = models::extract_drawable(&bytes, "subject", project.root.path(), &how()).unwrap();
    let output = project
        .shadow()
        .join(Project::id(&folder))
        .join("subject.ydr");
    let mut cooker = project.cooker();

    let first = cooker.recook(&folder.join("model.glb"));
    assert_eq!(
        first,
        Recook::Cooked {
            source: folder.clone(),
            output: output.clone()
        }
    );
    let cooked = fs::read(&output).unwrap();
    assert_eq!(
        cooked,
        models::compile(&Source::open(&folder).unwrap())
            .unwrap()
            .bytes
    );
    // Nothing on disk is the temporary a publish made.
    assert_eq!(fs::read_dir(output.parent().unwrap()).unwrap().count(), 1);

    // The same inputs again cook nothing.
    assert_eq!(
        cooker.recook(&folder.join("model.glb")),
        Recook::Unchanged {
            source: folder.clone()
        }
    );

    // An edit cooks again, and the output is the edit.
    fs::write(folder.join("model.glb"), lifted_glb(&bytes, 2.0)).unwrap();
    assert!(matches!(
        cooker.recook(&folder.join("model.glb")),
        Recook::Cooked { .. }
    ));
    let edited = meshes(&read_ydr(&fs::read(&output).unwrap()).base);
    let expected: Vec<Vec<[f32; 3]>> = meshes(&read_ydr(&bytes).base)
        .positions
        .iter()
        .map(|run| run.iter().map(|p| [p[0], p[1], p[2] + 2.0]).collect())
        .collect();
    assert!(same_positions(&expected, &edited.positions));
}

#[test]
fn a_glb_that_does_not_import_is_a_problem_and_leaves_the_previous_output() {
    let project = Project::new();
    let bytes = subject();
    let folder = models::extract_drawable(&bytes, "subject", project.root.path(), &how()).unwrap();
    let output = project
        .shadow()
        .join(Project::id(&folder))
        .join("subject.ydr");
    let mut cooker = project.cooker();
    cooker.recook(&folder.join("model.glb"));
    let good = fs::read(&output).unwrap();

    fs::write(folder.join("model.glb"), b"glTF, said no one").unwrap();
    let Recook::Failed { problems, .. } = cooker.recook(&folder.join("model.glb")) else {
        panic!("a broken glb was cooked");
    };
    assert_eq!(problems[0].file.as_deref(), Some("model.glb"));
    assert_eq!(fs::read(&output).unwrap(), good);
}

#[test]
fn saving_the_glb_of_one_entry_recooks_the_dictionary() {
    let project = Project::new();
    let bytes = ydd(vec![
        (
            "alpha",
            drawable("alpha", vec![geometry(6, false, 0.0)], None),
        ),
        (
            "beta",
            drawable("beta", vec![geometry(9, false, 20.0)], None),
        ),
    ]);
    let folder = models::extract_drawables(&bytes, "pair", project.root.path(), &how()).unwrap();
    let output = project.shadow().join(Project::id(&folder)).join("pair.ydd");
    let mut cooker = project.cooker();

    let glb = folder.join("drawables").join("beta").join("model.glb");
    assert!(matches!(cooker.recook(&glb), Recook::Cooked { .. }));
    assert_eq!(read_ydd(&fs::read(&output).unwrap()).len(), 2);
    assert!(matches!(
        cooker.recook(&folder.join("drawables").join("beta").join("drawable.json")),
        Recook::Unchanged { .. }
    ));
}

#[test]
fn what_the_cook_wrote_and_what_is_no_input_is_ignored() {
    let project = Project::new();
    let folder =
        models::extract_drawable(&subject(), "subject", project.root.path(), &how()).unwrap();
    let mut cooker = project.cooker();
    cooker.recook(&folder.join("model.glb"));

    let own = project
        .shadow()
        .join(Project::id(&folder))
        .join("subject.ydr");
    assert_eq!(cooker.recook(&own), Recook::Ignored(Ignored::OwnWrite));
    fs::write(folder.join("notes.txt"), "mine").unwrap();
    assert_eq!(
        cooker.recook(&folder.join("notes.txt")),
        Recook::Ignored(Ignored::NotAnInput)
    );
}

#[test]
fn a_changed_base_is_an_input_too() {
    let project = Project::new();
    let bytes = subject();
    let folder = models::extract_drawable(&bytes, "subject", project.root.path(), &how()).unwrap();
    let mut cooker = project.cooker();
    cooker.recook(&folder.join("model.glb"));

    // A base that is not the one the manifest names is refused when it is recooked.
    fs::write(folder.join("base.ydr"), &subject()[..64]).unwrap();
    assert!(matches!(
        cooker.recook(&folder.join("base.ydr")),
        Recook::Failed { .. }
    ));
}
