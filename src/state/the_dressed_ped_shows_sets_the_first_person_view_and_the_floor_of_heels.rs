//! What the dressed ped shows beyond its garments: the models a set of alternate variations changes, the view from its head with the
//! first person alternate a garment states, and the floor a ped that wears heels stands on.
//!
//! ⚠ Built with `AppState::from_prefs` and a clothing project in a scratch folder, whose garments are real models and textures from
//! the corpus, as the dressed ped's other cases are (`the_dressed_ped`); a machine without them skips, and says so.

use std::path::Path;

use or_renderer::LineVertex;
use or_wardrobe::alternate::Asset;
use or_wardrobe::dress::Sets;
use or_wardrobe::edit::Edit;
use or_wardrobe::Ped;

use super::the_dressed_ped::{first_mesh, fitted_dropping, staged, until, Fitted};
use super::workbench::AlternatesEdit;
use super::AppState;
use crate::render::viewport::CameraInputs;

/// The id of the head prop the fixture's pack holds.
fn the_hat(f: &Fitted) -> String {
    (f.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|read| {
            (read.wardrobe.garments.iter())
                .find(|g| g.slot == Some(or_wardrobe::Slot::Prop(or_wardrobe::Prop::Head)))
                .map(|g| g.id.clone())
        })
        .expect("the hat is a head prop")
}

/// The jacket given `alternate` as its alternate model, and the pack read again.
fn with_an_alternate_for_the_jacket(f: &mut Fitted, alternate: &Path) {
    let pack = f.state.wardrobe_pack().unwrap().folder.clone();
    let to = pack.join("models").join("jacket_alt.ydd");
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(alternate, &to).unwrap();
    let jacket = f.jacket.clone();
    or_wardrobe::edit::update(&pack, |w| {
        let g = w.garments.iter_mut().find(|g| g.id == jacket).unwrap();
        g.alternates = vec!["models/jacket_alt.ydd".into()];
        Ok(())
    })
    .unwrap();
    f.state.workbench.want_scan();
    until(&mut f.state, "the alternate to be read", |s| {
        s.wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|r| {
                r.wardrobe
                    .garment(&jacket)
                    .is_some_and(|g| !g.alternates.is_empty())
            })
    });
}

/// What the first mesh of the first drawable of `path` is made of.
fn mesh_of(path: &Path) -> (usize, usize) {
    let asset = or_io::open(path).unwrap();
    let mesh = &crate::render::meshes::meshes_of_asset_at(&asset, 0)[0];
    (mesh.vertices.len(), mesh.indices.len())
}

/// Poll until the first mesh drawn is made of `want` and the preview is not reading.
fn until_the_first_mesh_is(f: &mut Fitted, what: &str, want: (usize, usize)) {
    until(&mut f.state, what, |s| {
        let d = &s.workbench.wardrobe.dressed;
        !d.is_reading()
            && d.scene().is_some_and(|scene| {
                scene
                    .parts
                    .first()
                    .and_then(|p| p.geometry.first())
                    .is_some_and(|m| (m.vertices.len(), m.indices.len()) == want)
            })
    });
}

#[test]
fn a_set_in_force_draws_the_model_it_names_and_the_dressed_ped_says_which_set_drives_it() {
    let Some(models) = staged("ydd", 3) else {
        return;
    };
    let Some(mut f) = fitted_dropping(vec![models[0].clone(), models[1].clone()], true) else {
        return;
    };
    // The jacket has an alternate, and a set makes it wear that one while the hat is worn.
    with_an_alternate_for_the_jacket(&mut f, &models[2]);
    let (jacket, hat) = (f.jacket.clone(), the_hat(&f));
    f.state.edit_alternates(AlternatesEdit::New {
        ped: Ped::Male,
        name: "Hat and jacket".into(),
        join: Some(hat.clone()),
    });
    let sets = |s: &AppState| {
        (s.wardrobe_pack())
            .and_then(|p| p.read.as_ref().ok())
            .map(|r| r.wardrobe.alternates.clone())
            .unwrap_or_default()
    };
    until(&mut f.state, "the set", |s| !sets(s).is_empty());
    let set = sets(&f.state)[0].id.clone();
    f.state.edit_alternates(AlternatesEdit::AddSwitch {
        set: set.clone(),
        target: Asset::garment(&jacket),
        alt: 1,
    });
    until(&mut f.state, "the switch", |s| {
        sets(s).first().is_some_and(|set| !set.switches.is_empty())
    });
    f.state.workbench.wardrobe.dressed.outfit = vec![jacket.clone(), hat.clone()];
    f.state.workbench.wardrobe.dressed.outfit_on = true;
    f.select(&jacket);
    let (own, alternate) = (mesh_of(&models[0]), mesh_of(&models[2]));
    assert_ne!(own, alternate, "the staged models differ");

    // The hat is worn, so the set is in force as the game applies it, though the hat needs the ped's skeleton to be drawn and the fixture has no install.
    until_the_first_mesh_is(&mut f, "the alternate model", alternate);
    assert_eq!(f.scene_parts().len(), 1);
    let dressed = &f.state.workbench.wardrobe.dressed;
    assert_eq!(dressed.driven.len(), 1, "{:?}", dressed.driven);
    assert_eq!(dressed.driven[0].set, set);
    assert!(!dressed.driven[0].overridden);

    // None holds it back.
    f.state.workbench.wardrobe.dressed.sets = Sets::Off;
    until_the_first_mesh_is(&mut f, "its own model", own);
    assert!(f.state.workbench.wardrobe.dressed.driven.is_empty());

    // One set chosen is in force with the hat taken off the outfit.
    f.state.workbench.wardrobe.dressed.outfit = vec![jacket.clone()];
    f.state.workbench.wardrobe.dressed.sets = Sets::Only(set);
    until_the_first_mesh_is(&mut f, "the chosen set", alternate);
    assert_eq!(f.scene_parts().len(), 1);
    assert_eq!(f.state.workbench.wardrobe.dressed.driven.len(), 1);
}

#[test]
fn the_view_from_the_head_sits_at_the_head_bone_and_wears_the_first_person_alternate() {
    use crate::state::workbench::head_skeleton;
    let Some(models) = staged("ydd", 3) else {
        return;
    };
    let Some(mut f) = fitted_dropping(vec![models[0].clone(), models[1].clone()], false) else {
        return;
    };
    with_an_alternate_for_the_jacket(&mut f, &models[2]);
    let pack = f.state.wardrobe_pack().unwrap().folder.clone();
    let jacket = f.jacket.clone();
    or_wardrobe::edit::update(&pack, |w| {
        let g = w.garments.iter_mut().find(|g| g.id == jacket).unwrap();
        or_wardrobe::firstperson::set(g, Some(1))
    })
    .unwrap();
    f.state.workbench.want_scan();
    f.select(&jacket);
    let ped = f.state.dressed_ped();
    f.state
        .workbench
        .wardrobe
        .dressed
        .show_skeleton(ped, head_skeleton());
    f.shows(1);
    let (own, alternate) = (mesh_of(&models[0]), mesh_of(&models[2]));
    assert_eq!(first_mesh(&f), own);

    let orbit = f
        .state
        .step_viewport_camera(CameraInputs::default())
        .clone();
    f.state.workbench.wardrobe.dressed.first_person = true;
    let head = f
        .state
        .step_viewport_camera(CameraInputs::default())
        .clone();
    // The head bone of the fixture's skeleton stands a metre and six tenths over the root.
    assert!(
        (head.eye() - glam::Vec3::new(0.0, 0.0, 1.6)).length() < 0.05,
        "the eye is at the head bone: {:?}",
        head.eye()
    );
    assert_ne!(head.view(), orbit.view(), "the view is not the orbit view");
    assert!(
        (head.fov_y - 45f32.to_radians()).abs() < 1e-4,
        "the engine's default field of view"
    );
    // Looking forward from the head, the ped's own forward is the y axis.
    let forward = (head.target - head.eye()).normalize();
    assert!(forward.dot(glam::Vec3::Y) > 0.99, "{forward:?}");

    // A drag turns the head and leaves the eye where it was.
    let turned = f
        .state
        .step_viewport_camera(CameraInputs {
            orbit: egui::vec2(120.0, 0.0),
            ..CameraInputs::default()
        })
        .clone();
    assert_ne!(turned.yaw, head.yaw);
    assert!((turned.eye() - head.eye()).length() < 0.05);

    // The garment shows its first person alternate there, and its own model again outside it.
    until_the_first_mesh_is(&mut f, "the first person alternate", alternate);
    f.state.workbench.wardrobe.dressed.first_person = false;
    let back = f
        .state
        .step_viewport_camera(CameraInputs::default())
        .clone();
    assert_eq!(
        back.view(),
        orbit.view(),
        "the orbit camera is where it was left"
    );
    until_the_first_mesh_is(&mut f, "its own model", own);
}

#[test]
fn a_ped_that_wears_heels_stands_on_a_floor_at_the_height_they_raise_it_to() {
    let Some(models) = staged("ydd", 2) else {
        return;
    };
    let Some(mut f) = fitted_dropping(models, false) else {
        return;
    };
    let shoes = f.shoes.clone();
    f.select(&shoes);
    f.shows(1);
    let floor = |f: &Fitted| -> Vec<LineVertex> { crate::render::heel_floor::of(&f.state) };
    assert!(
        floor(&f).is_empty(),
        "shoes with no heels raise nothing, so the grid is the floor"
    );

    f.state.edit_garment(shoes.clone(), Edit::Heels(Some(1.5)));
    let lift = 1.5 * or_wardrobe::dress::METRES_PER_HEEL_UNIT;
    until(&mut f.state, "the heels in the preview", |s| {
        s.workbench.wardrobe.dressed.lift == lift
    });
    let lines = floor(&f);
    assert!(!lines.is_empty());
    assert!(
        lines.iter().all(|v| v.position[2] == lift),
        "the floor is at the height of the heels"
    );

    f.state.workbench.wardrobe.dressed.heels = false;
    until(&mut f.state, "the heels taken off", |s| {
        s.workbench.wardrobe.dressed.lift == 0.0
    });
    assert!(floor(&f).is_empty());
}
