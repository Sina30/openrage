//! What the Wardrobe takes in beyond a tidy pack of game-named files: pictures named as textures, names that say no ped or
//! slot (said for the selection in one step), and a drop of thousands of files that tells how far it has got and stops.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder, as the Wardrobe's other cases are: no install, no
//! keys, and a worker for each edit that a case waits on.

use std::path::{Path, PathBuf};

use or_wardrobe::{Component, Ped, Slot, Wardrobe};

use super::the_wardrobe_workspace::{drop_files, garments, opened, until, write, C};
use super::AppState;

fn idle(state: &mut AppState, what: &str) {
    until(state, what, |s| s.workbench.wardrobe.is_idle());
}

fn on_disk(project: &Path) -> Wardrobe {
    Wardrobe::read(&project.parent().unwrap().join("clothing")).expect("a pack on disk")
}

#[test]
fn a_picture_dropped_on_the_wardrobe_becomes_the_ytd_of_a_variation_and_undo_takes_it_out() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    let model = drop_files(dir.path(), &[(&format!("{C}^jbib_000_u.ydd"), b"a jacket")]).remove(0);
    let picture = dir
        .path()
        .join("drop")
        .join(format!("{C}^jbib_diff_000_a_uni.png"));
    image::RgbaImage::from_fn(16, 16, |x, y| {
        image::Rgba([x as u8 * 16, y as u8 * 16, 90, 255])
    })
    .save(&picture)
    .unwrap();

    state.add_clothes(vec![model, picture]);
    until(&mut state, "the drop", |s| garments(s) == 1);
    idle(&mut state, "the drop");

    let pack = on_disk(&file);
    let texture = pack.garments[0].variations[0].texture.clone().unwrap();
    assert!(
        texture.ends_with("jbib_diff_000_a_uni.textures.src"),
        "{texture}"
    );
    let folder = file.parent().unwrap().join("clothing");
    let cooked = or_source::cook::cook_source(&folder.join(&texture), &dir.path().join("scratch"));
    assert!(
        cooked.is_ok_and(|ytd| or_io::open(ytd).is_ok()),
        "the source cooks to a file the reader opens"
    );
    assert!(
        state
            .toasts
            .iter()
            .any(|t| t.message.contains("1 texture") && !t.message.contains("left out")),
        "{:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );

    state.undo_last_change();
    until(&mut state, "the undo", |s| garments(s) == 0);
    idle(&mut state, "the undo");
    assert!(!folder.join(&texture).exists());
}

#[test]
fn three_garments_whose_names_say_nothing_are_given_their_ped_and_slot_in_one_step_and_one_undo_takes_it_back(
) {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.add_clothes(drop_files(
        dir.path(),
        &[
            ("blue shirt.ydd", b"one"),
            ("red shirt.ydd", b"two"),
            ("old shirt.ydd", b"three"),
        ],
    ));
    until(&mut state, "the drop", |s| garments(s) == 3);
    idle(&mut state, "the drop");
    let before = on_disk(&file);
    assert!(
        before
            .garments
            .iter()
            .all(|g| g.ped.is_none() && g.slot.is_none()),
        "no name says what these are"
    );
    let ids: Vec<String> = before.garments.iter().map(|g| g.id.clone()).collect();
    let steps = state.workbench.wardrobe.undo.len();

    let tops = Slot::Component(Component::Jbib);
    state.resolve_garments(ids.clone(), Some(Ped::Male), Some(tops));
    until(&mut state, "the answer", |s| {
        s.workbench.wardrobe.undo.len() == steps + 1
    });
    idle(&mut state, "the answer");

    let after = on_disk(&file);
    assert!(
        after
            .garments
            .iter()
            .all(|g| g.ped == Some(Ped::Male) && g.slot == Some(tops)),
        "{:?}",
        after
            .garments
            .iter()
            .map(|g| (g.ped, g.slot))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        steps + 1,
        "the ped and the slot of all three are one step"
    );

    state.undo_last_change();
    until(&mut state, "the undo", |s| {
        s.workbench.wardrobe.undo.len() == steps
    });
    idle(&mut state, "the undo");
    assert_eq!(
        on_disk(&file),
        before,
        "all three are as the drop left them"
    );
}

#[test]
fn the_answer_to_names_that_say_nothing_leaves_alone_what_it_is_not_given() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    state.add_clothes(drop_files(
        dir.path(),
        &[
            ("blue shirt.ydd", b"one"),
            (&format!("{C}^lowr_000_u.ydd"), b"two"),
        ],
    ));
    until(&mut state, "the drop", |s| garments(s) == 2);
    idle(&mut state, "the drop");
    let ids: Vec<String> = on_disk(&file)
        .garments
        .iter()
        .map(|g| g.id.clone())
        .collect();

    // Only the ped: the slot the second name stated stays, and the first has none still.
    state.resolve_garments(ids, Some(Ped::Female), None);
    idle(&mut state, "the answer");
    until(&mut state, "the answer", |_| {
        on_disk(&file)
            .garments
            .iter()
            .all(|g| g.ped == Some(Ped::Female))
    });
    let after = on_disk(&file);
    let slots: Vec<Option<Slot>> = after.garments.iter().map(|g| g.slot).collect();
    assert!(slots.contains(&Some(Slot::Component(Component::Lowr))));
    assert!(slots.contains(&None), "{slots:?}");
}

/// `count` jackets, a model and a texture each, in a folder of the drop.
fn library(dir: &Path, count: usize) -> Vec<PathBuf> {
    let drop = dir.join("library");
    for n in 0..count {
        write(
            &drop.join(format!("{C}^jbib_{n:04}_u.ydd")),
            format!("jacket {n}").as_bytes(),
        );
        write(
            &drop.join(format!("{C}^jbib_diff_{n:04}_a_uni.ytd")),
            format!("texture {n}").as_bytes(),
        );
    }
    vec![drop]
}

#[test]
fn a_big_drop_says_how_far_it_has_got_and_stop_leaves_the_pack_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, file) = opened(dir.path());
    let before = on_disk(&file);
    state.add_clothes(library(dir.path(), 1_500));

    // The first poll starts the work; from then on the header has words for it and the work can be stopped.
    state.poll_workbench();
    let words = state
        .workbench
        .wardrobe
        .progress_words()
        .expect("the drop is at work");
    assert!(words.starts_with("Adding clothes"), "{words}");
    assert!(state.workbench.wardrobe.stoppable());
    state.workbench.wardrobe.stop();
    assert!(!state.workbench.wardrobe.stoppable(), "asked once");

    idle(&mut state, "the stop");
    assert_eq!(garments(&state), 0);
    assert_eq!(on_disk(&file), before, "the pack is as it was");
    assert!(
        state.toasts.iter().any(|t| t.message.contains("Stopped")),
        "{:?}",
        state.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        0,
        "nothing to take back"
    );
}
