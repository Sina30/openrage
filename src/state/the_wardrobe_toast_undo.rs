//! The Undo on a drop's or a removal's toast: the older drop's takes back that drop and not the newest, and a removed garment comes back with all it stated.

use or_wardrobe::edit::Edit;

use super::front_door::{NewProject, Template};
use super::the_wardrobe_workspace::{bare, drop_files, folder_of, garments, opened, until, C};
use super::toasts::ToastAction;

#[test]
fn the_undo_of_an_older_drops_toast_takes_back_that_drop_and_not_the_newest() {
    use or_wardrobe::{Component, Slot};
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    until(&mut state, "the scan", |s| s.workbench.is_scanned());
    state.add_clothes(drop_files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
        ],
    ));
    until(&mut state, "the first drop", |s| garments(s) == 1);
    state.add_clothes(drop_files(
        dir.path(),
        &[
            (&format!("{C}^lowr_000_u.ydd"), b"trousers"),
            (&format!("{C}^lowr_diff_000_a_uni.ytd"), b"blue"),
        ],
    ));
    until(&mut state, "the second drop", |s| garments(s) == 2);
    let buttons: Vec<ToastAction> = state.toasts.iter().filter_map(|t| t.action).collect();
    assert_eq!(buttons.len(), 2, "each drop's toast carries an Undo");

    // The toast of the first drop is still up, and its Undo is the first drop's.
    state.run_toast_action(buttons[0]);

    until(&mut state, "the older drop's undo", |s| garments(s) == 1);
    let pack = state.wardrobe_pack().unwrap().read.as_ref().unwrap();
    assert_eq!(
        pack.wardrobe.garments[0].slot,
        Some(Slot::Component(Component::Lowr)),
        "the trousers of the newer drop stay"
    );
    // ⚠ The worker takes the dropped files after the list changes
    let jacket = folder_of(&file, "clothing").join(format!("models/{C}^jbib_000_u.drawables.src"));
    until(&mut state, "the older drop's files taken", |_| {
        !jacket.exists()
    });
    assert!(jacket
        .with_file_name(format!("{C}^lowr_000_u.drawables.src"))
        .join("asset.json")
        .is_file());
}

#[test]
fn a_removed_garment_is_brought_back_by_the_undo_of_its_toast_with_all_it_stated() {
    use or_wardrobe::{Component, Slot, Wardrobe};
    let dir = tempfile::tempdir().unwrap();
    let mut draft = NewProject::new(Template::Clothing, dir.path().to_path_buf());
    draft.set_name("tshirts".into());
    let file = draft.create().expect("the template makes a project").file;
    let pack = folder_of(&file, "clothing");
    let dropped = drop_files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^jbib_000_u.yld"), b"its cloth"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
            (&format!("{C}^lowr_000_u.ydd"), b"trousers"),
        ],
    );
    or_wardrobe::edit::add_clothes(&pack, &dropped).expect("the pack is filled");
    // What a person did to the jacket: pinned it and named it. It has the cloth that was dropped with its model.
    or_wardrobe::edit::update(&pack, |w| {
        let jacket = (w.garments.iter_mut())
            .find(|g| g.slot == Some(Slot::Component(Component::Jbib)))
            .expect("the jacket");
        jacket.position = Some(7);
        jacket.label = Some("my jacket".into());
        Ok(())
    })
    .expect("the jacket is dressed");
    let before = Wardrobe::read(&pack).unwrap();
    let jacket = (before.garments.iter())
        .find(|g| g.slot == Some(Slot::Component(Component::Jbib)))
        .expect("the jacket")
        .clone();
    assert!(jacket.cloth.is_some() && !jacket.missing_cloth);
    let mut state = bare();
    state.load_project_from(file);
    until(&mut state, "the pack", |s| garments(s) == 2);

    state.edit_garment(jacket.id.clone(), Edit::Remove);

    until(&mut state, "the removal", |s| garments(s) == 1);
    let undo = state
        .toasts
        .iter()
        .find_map(|t| t.action)
        .expect("removing a garment offers an Undo in its toast");
    state.run_toast_action(undo);
    until(&mut state, "the undo", |s| garments(s) == 2);
    assert_eq!(
        Wardrobe::read(&pack).unwrap(),
        before,
        "the pin, the name, the metadata and the place in the pack are as they were"
    );
}
