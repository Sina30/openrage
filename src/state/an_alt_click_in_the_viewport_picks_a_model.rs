//! A click on the viewport panel selects the geometry under it, and a click with Alt held selects
//! the model that holds the geometry.

use glam::Vec3;

use super::prefs::Prefs;
use super::the_viewport_picks_what_it_draws::{a_triangle, camera, Holder};
use super::{AppState, DocPayload};
use crate::ui::panels::viewport::select_on_click;

/// A primary click, with `modifiers` held, in the middle of a viewport that fills a screen of its
/// own, looking along -Y at the triangle drawn at the origin.
fn click_in_the_middle(state: &mut AppState, modifiers: egui::Modifiers) {
    let ctx = egui::Context::default();
    let camera = camera(Vec3::new(0.25, 0.0, 0.25), -Vec3::Y, 3.0);
    let middle = std::cell::Cell::new(egui::Pos2::ZERO);
    let mut time = 0.0;
    let mut frame = |events: Vec<egui::Event>, state: &mut AppState| {
        time += 0.05;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 500.0),
            )),
            time: Some(time),
            modifiers,
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            // As the viewport panel lays itself out: one response over the whole panel.
            let (_, response) =
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
            middle.set(response.rect.center());
            select_on_click(&response, state, &camera, (1200, 750));
        });
    };
    frame(Vec::new(), state);
    let at = middle.get();
    frame(vec![egui::Event::PointerMoved(at)], state);
    for pressed in [true, false] {
        let event = egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        };
        frame(vec![event], state);
    }
}

#[test]
fn an_alt_click_in_the_viewport_panel_selects_the_model_and_a_click_the_geometry() {
    let alt = egui::Modifiers {
        alt: true,
        ..egui::Modifiers::NONE
    };
    for holder in Holder::ALL {
        let corners = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let mut state = AppState::from_prefs(Prefs::default());
        let doc = state.insert_document(
            holder.file().into(),
            DocPayload::Resource(holder.asset(a_triangle(corners, None, None))),
            None,
            None,
        );

        click_in_the_middle(&mut state, egui::Modifiers::NONE);
        assert_eq!(
            state.selection.selected(doc),
            Some(holder.geometry(0)),
            "{holder:?}: a click on the triangle did not select its geometry"
        );

        click_in_the_middle(&mut state, alt);
        assert_eq!(
            state.selection.selected(doc),
            Some(holder.model()),
            "{holder:?}: an Alt click in the viewport panel selected the geometry, not its model"
        );
    }
}
