//! The Wardrobe's A/B compares the pack as it is with the pack as it was before its latest change, from the versions the Wardrobe keeps,
//! as well as with the ped without the pack.
//!
//! ⚠ Built from real models and textures of the corpus, as the dressed ped's own cases are; a machine without them skips, and says so.

use super::importing_game_files_from_the_app::project;
use super::the_dressed_ped::{fitted_from, staged, until};

#[test]
fn before_wears_the_pack_as_it_was_before_its_latest_change_and_b_wears_it_as_it_is() {
    let Some(models) = staged("ydd", 2) else {
        return;
    };
    let Some(mut f) = fitted_from(models) else {
        return;
    };
    let (jacket, shoes) = (f.jacket.clone(), f.shoes.clone());
    f.select(&jacket);
    {
        let dressed = &mut f.state.workbench.wardrobe.dressed;
        dressed.toggle_in_outfit(&jacket);
        dressed.toggle_in_outfit(&shoes);
        dressed.outfit_on = true;
    }
    f.shows(2);

    // The latest change takes the shoes out of the pack: B wears one garment, and the earlier version still wears both.
    f.state.remove_garments(vec![shoes]);
    until(&mut f.state, "the removal", |s| {
        s.wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|read| read.wardrobe.garments.len() == 1)
    });
    f.shows(1);

    f.state.show_previous_version();
    assert!(f.state.workbench.wardrobe.dressed.previous);
    f.shows(2);

    // B and A as the toolbar sets them: the pack as it is now, and the ped without it.
    f.state.workbench.wardrobe.dressed.previous = false;
    f.shows(1);
    f.state.workbench.wardrobe.dressed.original = true;
    until(&mut f.state, "A", |s| {
        s.workbench
            .wardrobe
            .dressed
            .scene()
            .is_some_and(|scene| scene.parts.is_empty())
    });
}

#[test]
fn without_a_pack_to_compare_before_says_there_is_no_earlier_version() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, _) = project(dir.path());

    state.show_previous_version();

    assert!(!state.workbench.wardrobe.dressed.previous);
    assert!(
        state
            .toasts
            .iter()
            .any(|t| t.message.contains("no earlier version")),
        "{:?}",
        state.toasts
    );
}
