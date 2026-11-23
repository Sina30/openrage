//! A texture place the Wardrobe holds: made, filled with a file, put after the textures and taken back, each one step of Undo.
//!
//! ⚠ Built with `AppState::from_prefs` and a clothing project in a scratch folder, whose garments are real models and textures from
//! the corpus (`the_dressed_ped`); a machine without them skips, and says so.

use super::the_dressed_ped::{fitted_from, staged, until, Fitted};

/// The file each texture of the jacket names, none where it holds a place.
fn textures(f: &Fitted) -> Vec<Option<String>> {
    (f.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|read| read.wardrobe.garment(&f.jacket))
        .map(|g| g.variations.iter().map(|v| v.texture.clone()).collect())
        .unwrap_or_default()
}

fn until_the_jacket_has(f: &mut Fitted, what: &str, want: impl Fn(&[Option<String>]) -> bool) {
    let id = f.jacket.clone();
    until(&mut f.state, what, |s| {
        s.wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .and_then(|read| read.wardrobe.garment(&id))
            .is_some_and(|g| {
                let held: Vec<Option<String>> =
                    g.variations.iter().map(|v| v.texture.clone()).collect();
                want(&held)
            })
    });
}

#[test]
fn a_texture_place_is_held_filled_and_put_last_as_a_step_each_and_undo_takes_each_back() {
    let Some(models) = staged("ydd", 2) else {
        return;
    };
    let Some(spare) = staged("ytd", 4) else {
        return;
    };
    let Some(mut f) = fitted_from(models) else {
        return;
    };
    let jacket = f.jacket.clone();
    f.select(&jacket);
    let first = textures(&f);
    assert_eq!(first.len(), 2, "the fixture's jacket has two textures");

    // Two places are held, one step each.
    f.state.hold_texture_place(jacket.clone());
    until_the_jacket_has(&mut f, "the first place", |t| t.len() == 3);
    f.state.hold_texture_place(jacket.clone());
    until_the_jacket_has(&mut f, "the second place", |t| t.len() == 4);
    let held = textures(&f);
    assert_eq!(held[..2], first[..]);
    assert_eq!(held[2..], [None, None], "a place ships no file");

    // The first place is filled: it keeps its letter, and the one after it is still held.
    f.state.replace_texture(jacket.clone(), 2, spare[3].clone());
    until_the_jacket_has(&mut f, "the filled place", |t| {
        t.len() == 4 && t[2].is_some()
    });
    let filled = textures(&f);
    assert_eq!(filled[3], None);
    assert_eq!(
        filled[..2],
        first[..],
        "the textures before it are as they were"
    );

    // One Undo holds it again, and no more.
    f.state.undo_last_change();
    until_the_jacket_has(&mut f, "the place held again", |t| {
        t.len() == 4 && t[2].is_none()
    });
    assert_eq!(textures(&f), held);

    // The last place is filled while the one before it is held.
    f.state.replace_texture(jacket.clone(), 3, spare[3].clone());
    until_the_jacket_has(&mut f, "the last place filled", |t| {
        t.len() == 4 && t[3].is_some()
    });
    // A place between two textures goes after them.
    let between = textures(&f);
    assert_eq!((between[2].is_none(), between[3].is_some()), (true, true));
    f.state.sort_texture_places(jacket.clone());
    until_the_jacket_has(&mut f, "the places last", |t| {
        t.len() == 4 && t[2].is_some() && t[3].is_none()
    });
    assert_eq!(textures(&f)[2], between[3]);
    f.state.undo_last_change();
    until_the_jacket_has(&mut f, "the order back", |t| {
        t[2].is_none() && t[3].is_some()
    });
}
