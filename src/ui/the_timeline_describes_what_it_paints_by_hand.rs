//! The timeline paints a row of more keys than pixels as a density graph, and
//! the handles of a curve, with no widget behind them: AccessKit describes
//! each only because the timeline says what it is.

use egui::accesskit::Role;

use super::frames_of_the_shell::{Described, Shell, DENSE_KEYS, HANDLES};
use crate::state::ThemeChoice;
use crate::ui::theme;

fn named<'a>(
    shell: &'a Shell,
    role: Role,
    part: &'a str,
) -> impl Iterator<Item = &'a Described> + 'a {
    (shell.described.iter())
        .filter(move |w| w.role == role && w.name.as_deref().is_some_and(|n| n.contains(part)))
}

#[test]
fn a_row_drawn_as_a_density_graph_is_described_with_its_label_and_its_key_count() {
    let _serial = theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.an_editable_clips_timeline();
    let found: Vec<_> = named(&shell, Role::Image, "SKEL_Dense").collect();
    assert_eq!(
        found.len(),
        1,
        "{} nodes describe the row of {DENSE_KEYS} keys",
        found.len()
    );
    let name = found[0].name.clone().unwrap_or_default();
    assert!(name.contains(&DENSE_KEYS.to_string()), "{name:?}");
    assert!(
        found[0].bounds.width() > 200.0 && found[0].bounds.height() > 8.0,
        "the description covers {:?}, which is not the row",
        found[0].bounds
    );
}

#[test]
fn every_handle_of_a_curve_is_described_inside_the_curve_view_and_a_read_only_clip_draws_none() {
    let _serial = theme::serial();
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.an_editable_clips_timeline();
    let view = named(&shell, Role::Unknown, "Curve view")
        .map(|w| w.bounds)
        .next()
        .expect("the curve view is named");
    let handles: Vec<_> = named(&shell, Role::Button, "Translation channel").collect();
    assert_eq!(handles.len(), HANDLES, "{handles:?}");
    for handle in handles {
        assert!(
            view.expand(8.0).contains_rect(handle.bounds) && handle.bounds.width() >= 8.0,
            "{:?} is described at {:?}, away from the curve view {view:?} or too small to hit",
            handle.name,
            handle.bounds
        );
    }

    shell.a_clips_timeline();
    let drawn = named(&shell, Role::Button, "Translation channel").count();
    assert_eq!(
        drawn, 0,
        "a read-only clip draws no handles, yet {drawn} are described"
    );
}
