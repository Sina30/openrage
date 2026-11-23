//! The dressed ped wears what a pack states beyond its components: a prop at the bone of its anchor, drawn at the level chosen and
//! moving with the bone, the alternate model chosen for a garment, and the data the views of vertex colours and tangents hold.
//!
//! ⚠ Built with `AppState::from_prefs` and a clothing project in a scratch folder, whose garments are real models and textures
//! from the corpus, as the dressed ped's other cases are (`the_dressed_ped`).

use super::the_dressed_ped::{
    first_mesh, fitted_dropping, fitted_from, staged, staged_with_levels, until, Fitted,
};

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

#[test]
fn a_hat_is_worn_at_the_bone_of_its_anchor_drawn_at_the_level_chosen_and_moves_with_the_bone() {
    use crate::state::workbench::head_skeleton;
    let Some(models) = staged_with_levels() else {
        return;
    };
    // The jacket takes the first model, and the shoes and the hat the second, which has a lighter medium level.
    let (many, other) = (models[0].clone(), models[1].clone());
    let Some(mut f) = fitted_dropping(vec![other, many.clone()], true) else {
        return;
    };
    let hat = the_hat(&f);
    f.select(&hat);
    // What the model holds where it is stored: the hat is carried from there to the head bone.
    let asset = or_io::open(&many).expect("a staged model opens");
    let stored = crate::render::meshes::meshes_of_asset_at(&asset, 0);
    let stored_z = stored[0].vertices[0].position[2];
    let at_the_head = 1.6;

    // Without the ped's skeleton there is no bone to carry it to, and the preview says so.
    f.state.poll_workbench();
    until(&mut f.state, "the note about the skeleton", |s| {
        s.workbench
            .wardrobe
            .dressed
            .notes
            .iter()
            .any(|n| n.contains("SKEL_Head bone"))
    });
    assert!(
        f.state
            .workbench
            .wardrobe
            .dressed
            .scene()
            .is_none_or(|s| s.parts.is_empty()),
        "a prop with no bone to sit at is not drawn at the origin"
    );

    let ped = f.state.dressed_ped();
    f.state
        .workbench
        .wardrobe
        .dressed
        .show_skeleton(ped, head_skeleton());
    f.shows(1);
    let scene = f.state.workbench.wardrobe.dressed.scene().unwrap();
    let drawn_z = scene.parts[0].geometry[0].vertices[0].position[2];
    assert!(
        (drawn_z - (stored_z + at_the_head)).abs() < 1e-3,
        "carried to the head bone: {drawn_z} against {stored_z} + {at_the_head}"
    );
    assert_eq!(scene.garments(), 1);
    assert!(
        !f.state
            .workbench
            .wardrobe
            .dressed
            .notes
            .iter()
            .any(|n| n.contains("SKEL_Head bone")),
        "{:?}",
        f.state.workbench.wardrobe.dressed.notes
    );

    // The Level tool draws the level asked for, still at the bone.
    let high = scene.drawn();
    f.state.workbench.wardrobe.dressed.lod = 1;
    until(&mut f.state, "the medium level", |s| {
        let d = &s.workbench.wardrobe.dressed;
        !d.is_reading() && d.scene().is_some_and(|scene| scene.drawn() != high)
    });
    let medium = f.state.workbench.wardrobe.dressed.scene().unwrap();
    assert!(medium.drawn().triangles < high.triangles);
    let medium_stored = crate::render::meshes::meshes_of_asset_at(&asset, 1);
    let want = medium_stored[0].vertices[0].position[2] + at_the_head;
    assert!(
        (medium.parts[0].geometry[0].vertices[0].position[2] - want).abs() < 1e-3,
        "the lighter level is carried to the bone too"
    );

    // A pose that lifts the bone a metre lifts the hat with it: it is skinned to that bone.
    assert!(medium.is_skinned_mesh(0, 0));
    let mut skin = vec![glam::Mat4::IDENTITY; 2];
    skin[1] = glam::Mat4::from_translation(glam::Vec3::Z);
    let posed = medium.posed(0, 0, &skin).expect("the hat is posed");
    assert!(
        (posed[0].position[2] - (want + 1.0)).abs() < 1e-3,
        "{} against {}",
        posed[0].position[2],
        want + 1.0
    );
}

#[test]
fn a_hat_with_an_alternate_is_worn_with_the_model_chosen_for_it() {
    let Some(models) = staged("ydd", 3) else {
        return;
    };
    let Some(mut f) = fitted_dropping(vec![models[0].clone(), models[1].clone()], true) else {
        return;
    };
    // The jacket gets an alternate: a third model, named after its own.
    let pack = f.state.wardrobe_pack().unwrap().folder.clone();
    let alternate = pack.join("models").join("jacket_alt.ydd");
    std::fs::create_dir_all(alternate.parent().unwrap()).unwrap();
    std::fs::copy(&models[2], &alternate).unwrap();
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
    f.select(&jacket);
    f.shows(1);
    let own = first_mesh(&f);

    f.state
        .workbench
        .wardrobe
        .dressed
        .alternate
        .insert(jacket.clone(), 1);
    until(&mut f.state, "the alternate model", |s| {
        let d = &s.workbench.wardrobe.dressed;
        !d.is_reading()
            && d.scene().is_some_and(|scene| {
                scene
                    .parts
                    .first()
                    .and_then(|p| p.geometry.first())
                    .is_some_and(|m| (m.vertices.len(), m.indices.len()) != own)
            })
    });
    let asset = or_io::open(&models[2]).unwrap();
    let want = &crate::render::meshes::meshes_of_asset_at(&asset, 0)[0];
    assert_eq!(first_mesh(&f), (want.vertices.len(), want.indices.len()));

    f.state.workbench.wardrobe.dressed.alternate.clear();
    f.shows(1);
    until(&mut f.state, "its own model again", |s| {
        let d = &s.workbench.wardrobe.dressed;
        !d.is_reading()
            && d.scene().is_some_and(|scene| {
                scene
                    .parts
                    .first()
                    .and_then(|p| p.geometry.first())
                    .is_some_and(|m| (m.vertices.len(), m.indices.len()) == own)
            })
    });
}

#[test]
fn the_vertex_colour_and_tangent_views_hold_the_colours_of_the_meshes_that_have_the_channel() {
    use crate::render::debug_channel::DebugChannel;
    let Some(models) = staged("ydd", 2) else {
        return;
    };
    let asset = or_io::open(&models[0]).expect("a staged model opens");
    let Some(mut f) = fitted_from(models) else {
        return;
    };
    let jacket = f.jacket.clone();
    f.select(&jacket);
    f.shows(1);

    // The model as the document viewport reads it, with each channel asked for in turn.
    let plain = crate::render::meshes::meshes_of_asset_in(&asset, 0, None);
    let wanted = |channel: DebugChannel, mesh: usize| -> Option<Vec<[f32; 4]>> {
        plain[mesh].channels.contains(&channel).then(|| {
            let drawn = crate::render::meshes::meshes_of_asset_in(&asset, 0, Some(channel));
            drawn[mesh].vertices.iter().map(|v| v.colour).collect()
        })
    };
    let scene = f.state.workbench.wardrobe.dressed.scene().unwrap();
    let (mut with_colour, mut with_tangent) = (0, 0);
    for mesh in 0..plain.len() {
        let aux = scene
            .aux(0, mesh)
            .expect("every mesh of the scene has its data");
        assert_eq!(
            aux.colour,
            wanted(DebugChannel::Colour(0), mesh),
            "colour of mesh {mesh}"
        );
        assert_eq!(
            aux.tangent,
            wanted(DebugChannel::Tangent(0), mesh),
            "tangent of mesh {mesh}"
        );
        with_colour += usize::from(aux.colour.is_some());
        with_tangent += usize::from(aux.tangent.is_some());
        if let Some(colours) = &aux.colour {
            assert_eq!(colours.len(), scene.parts[0].geometry[mesh].vertices.len());
        }
    }
    assert!(
        with_colour + with_tangent > 0,
        "the staged model has neither a vertex colour nor a tangent, so this holds nothing to the views"
    );
}

#[test]
fn the_ux_design_says_a_prop_is_worn_at_its_bone_and_nowhere_that_it_is_only_listed() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs/direction/Workbench_UX_Design.md");
    let doc = std::fs::read_to_string(path).expect("the UX design");
    for stale in [
        "listed and not drawn",
        "nothing this build reads says which bone",
    ] {
        assert!(
            !doc.contains(stale),
            "the design still says {stale:?} of a prop the preview wears"
        );
    }
    let ped = (doc.lines())
        .find(|line| line.starts_with("- **The dressed ped.**"))
        .expect("the dressed ped bullet");
    assert!(
        ped.contains("worn at `Prop::bone`"),
        "the bullet on the dressed ped does not say a prop is worn at `Prop::bone`"
    );
}
