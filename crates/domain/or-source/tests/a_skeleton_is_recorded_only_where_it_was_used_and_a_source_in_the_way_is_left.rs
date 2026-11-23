//! The reference skeleton a ped component is imported with is recorded in the source it was embedded into and in none other, and an
//! import that falls back to the plain extraction never removes a source folder that was there before it.

mod common;
mod model_fixtures;

use common::{legacy_dictionary, ramp, Spec};
use model_fixtures::{drawable, geometry, reference, ydr};
use or_io::codec::texture::GrcTextureFormat;
use or_source::asset::Source;
use or_source::game_import::{decompile, Facts, Skeleton};

fn with_a_skeleton() -> Facts {
    Facts {
        skeleton: Skeleton::of(reference(8)),
        ..Facts::default()
    }
}

#[test]
fn a_textures_source_is_not_said_to_hold_a_skeleton_and_a_models_source_is() {
    let parent = tempfile::tempdir().unwrap();
    let ytd = legacy_dictionary(&[Spec::new("art", GrcTextureFormat::DXT5, ramp(16, 16, 1))]);
    let textures = decompile(&ytd, "art.ytd", parent.path(), &with_a_skeleton()).unwrap();
    let source = Source::open(&textures).unwrap();
    assert!(
        !source
            .manifest
            .origin
            .extra
            .contains_key("reference_skeleton"),
        "a texture takes no skeleton: {:?}",
        source.manifest.origin.extra
    );

    let model = ydr(drawable("jbib_000_u", vec![geometry(6, true, 0.0)], None));
    let folder = decompile(&model, "jbib_000_u.ydr", parent.path(), &with_a_skeleton()).unwrap();
    assert_eq!(
        Source::open(&folder).unwrap().manifest.origin.extra["reference_skeleton"],
        serde_json::json!("embedded")
    );
}

#[test]
fn a_source_already_in_the_way_is_left_where_it_is_whatever_the_skeleton() {
    let parent = tempfile::tempdir().unwrap();
    let model = ydr(drawable("jbib_000_u", vec![geometry(6, true, 0.0)], None));
    let folder = decompile(&model, "jbib_000_u.ydr", parent.path(), &Facts::default()).unwrap();
    let hand_made = folder.join("hand-made.txt");
    std::fs::write(&hand_made, b"kept").unwrap();

    let why = decompile(&model, "jbib_000_u.ydr", parent.path(), &with_a_skeleton());
    assert!(why.is_err(), "the folder is taken: {why:?}");
    assert_eq!(
        std::fs::read(&hand_made).unwrap(),
        b"kept",
        "a source that was there is not removed to make room"
    );
}
