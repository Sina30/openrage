//! What stands under and on the 3D view of the dressed ped: the view helpers and the controls of the clip that plays under it (the skeleton, the grid, the view from the head, the light and the camera), and over it the note of the view from the head and the figures of
//! what is drawn.
//!
//! ⚠ **Thin by design.** The skeleton, the grid, the first person view and the clip are the state's; this places the controls where the view they act on is.

use egui_phosphor::regular as icon;

use super::kit::{combo, tip};
use super::tools::{carry_out, small, Play};
use crate::state::workbench::{NO_GAME, SPEEDS};
use crate::state::AppState;
use crate::ui::accessible_name::named;
use crate::ui::theme;

/// What the view from the head is, for the note over the view that says it is on.
const FIRST_PERSON: &str = tr_key!("The camera stands at the head bone with the engine's default field of view, so the ped's own head, hair and teeth are not drawn; a drag turns the head, garments flagged hidden in the first person view are left out and a garment wears its first person alternate. A garment that surrounds the head fills the view unless it is flagged to hide");

/// The view helpers, in a row under the view: the skeleton, the grid, the view from the head and the camera placed on the ped.
pub(super) fn helpers(ui: &mut egui::Ui, state: &mut AppState) {
    let ped = state.dressed_ped();
    let has_skeleton = state.workbench.wardrobe.dressed.skeleton(ped).is_some();
    let has_head = state.workbench.wardrobe.dressed.eyes(ped).is_some();
    let mut frame = false;
    ui.horizontal(|ui| {
        let dressed = &mut state.workbench.wardrobe.dressed;
        ui.add_enabled(has_skeleton, egui::Checkbox::new(&mut dressed.bones, tr!("Bones")))
            .on_hover_text(tr!("The skeleton of the ped, posed by the clip while one plays"))
            .on_disabled_hover_text(tr!("The skeleton is read from the game install, which needs to be chosen in Settings"));
        ui.checkbox(&mut state.prefs.viewport.show_grid, tr!("Grid"))
            .on_hover_text(tr!("The floor grid of the 3D views"));
        let dressed = &mut state.workbench.wardrobe.dressed;
        let response = ui
            .add_enabled(
                has_head,
                egui::Checkbox::new(&mut dressed.first_person, tr!("First person")),
            )
            .on_disabled_hover_text(tr!("The head is placed from the ped's skeleton, which is read from the game install, which needs to be chosen in Settings"));
        tip(response, crate::i18n::tr(FIRST_PERSON));
        super::light::light(ui, state);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let button = ui.button(tr!("{icon} Frame", icon = icon::CROSSHAIR));
            frame = named(button, tr!("Frame the ped"))
                .on_hover_text(tr!("Place the camera on the ped"))
                .clicked();
        });
    });
    if frame {
        state.workbench.wardrobe.dressed.frame_again();
    }
}

/// A box over the view at `rect`, which its content sits in: at the top edge, or at the bottom edge where it is as tall as it was in the last frame.
fn over<R>(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    from_the_bottom: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let room = rect.shrink(8.0);
    // ⚠ A box stood on the bottom edge needs its own height before it is drawn: the last frame's, which a change of content settles in the next.
    let kept = ui
        .id()
        .with(tr!("height of the box over the bottom of the view"));
    let height = ui
        .data(|d| d.get_temp::<f32>(kept))
        .unwrap_or(24.0)
        .min(room.height());
    let room = match from_the_bottom {
        true => egui::Rect::from_min_max(egui::pos2(room.left(), room.bottom() - height), room.max),
        false => room,
    };
    let builder = egui::UiBuilder::new()
        .max_rect(room)
        .layout(egui::Layout::top_down(egui::Align::Min));
    let shown = ui.scope_builder(builder, |ui| {
        egui::Frame::new()
            .fill(theme::overlay_bg())
            .corner_radius(theme::RADIUS_SMALL)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, add)
    });
    if from_the_bottom {
        let drawn = shown.inner.response.rect.height();
        if (drawn - height).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(kept, drawn));
            ui.ctx().request_repaint();
        }
    }
    shown.inner.inner
}

/// The note of the view from the head: that it is on, how to look round and how to leave.
pub(super) fn first_person(ui: &mut egui::Ui, rect: egui::Rect, state: &mut AppState) {
    if !state.workbench.wardrobe.dressed.first_person {
        return;
    }
    let leave = over(ui, rect, false, |ui| {
        ui.horizontal(|ui| {
            ui.label(tr!("First person: drag to look round"))
                .on_hover_text(crate::i18n::tr(FIRST_PERSON));
            let leave = ui.small_button(tr!("Leave (Esc)"));
            named(leave, tr!("Leave first person")).clicked()
        })
        .inner
    });
    if leave {
        state.workbench.wardrobe.dressed.first_person = false;
    }
}

/// The note over the top of the view while no game install is chosen: the ped is not drawn, and this says why and is the way to choose one.
pub(super) fn no_game(ui: &mut egui::Ui, rect: egui::Rect, state: &mut AppState) {
    if state.prefs.active_game_root().is_some() {
        return;
    }
    let choose = over(ui, rect, false, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::i18n::tr(NO_GAME));
            ui.button(tr!("Choose a game install…"))
                .on_hover_text(tr!(
                    "The ped and its own clothes are drawn from the game install"
                ))
                .clicked()
        })
        .inner
    });
    if choose {
        crate::ui::panels::settings::open_settings_at(
            state,
            crate::ui::panels::settings::SettingsCategory::Games,
        );
    }
}

/// Escape leaves the view from the head while the pointer or the keyboard is on the view; true when it did.
pub(super) fn escape_leaves(ui: &egui::Ui, view: &egui::Response, state: &mut AppState) -> bool {
    let dressed = &mut state.workbench.wardrobe.dressed;
    let here = view.hovered() || view.has_focus();
    // ⚠ Not while a window or a menu is open: Escape closes that first.
    let covered = state.workbench.asking.is_some() || egui::Popup::is_any_open(ui.ctx());
    if !dressed.first_person || !here || covered {
        return false;
    }
    let pressed = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    if pressed {
        dressed.first_person = false;
    }
    pressed
}

/// What the view draws, in its bottom left corner.
pub(super) fn bottom(ui: &mut egui::Ui, rect: egui::Rect, state: &mut AppState) {
    let drawn = state
        .workbench
        .wardrobe
        .dressed
        .scene()
        .map(|scene| scene.drawn())
        .filter(|d| d.triangles > 0);
    let drives = (state.workbench.wardrobe.dressed.motion.as_ref()).map(|m| {
        tr!(
            "the clip drives {resolved} of {clip_tags} bones",
            resolved = m.report.resolved,
            clip_tags = m.report.clip_tags()
        )
    });
    let figures = drawn.map(|d| {
        tr!(
            "{triangles} triangles, {vertices} vertices",
            triangles = d.triangles,
            vertices = d.vertices
        )
    });
    let line = match (figures, drives) {
        (Some(figures), Some(drives)) => format!("{figures}; {drives}"),
        (Some(only), None) | (None, Some(only)) => only,
        (None, None) => return,
    };
    over(ui, rect, true, |ui| {
        small(ui, line).on_hover_text(tr!(
            "What the 3D view draws now: the ped's own drawables and the garments"
        ));
    });
}

/// Whether a clip is chosen or being read, so that its controls stand under the view.
pub(super) fn is_playing(state: &AppState) -> bool {
    let dressed = &state.workbench.wardrobe.dressed;
    dressed.motion.is_some() || dressed.is_animating()
}

/// The controls of the clip that plays, in a row under the view: over it they covered the legs and the shoes, which are what a garment is checked on.
pub(super) fn transport_row(ui: &mut egui::Ui, state: &mut AppState) {
    if !is_playing(state) {
        return;
    }
    let mut asked = None;
    ui.horizontal_wrapped(|ui| transport(ui, state, &mut asked));
    carry_out(state, asked);
}

/// Play or pause, loop, speed and the choice among the clips of the dictionary.
fn transport(ui: &mut egui::Ui, state: &mut AppState, asked: &mut Option<Play>) {
    let dressed = &mut state.workbench.wardrobe.dressed;
    let Some(motion) = &mut dressed.motion else {
        small(ui, tr!("Reading the clip…"));
        return;
    };
    let (glyph, words) = match motion.transport.playing {
        true => (icon::PAUSE, tr!("Pause")),
        false => (icon::PLAY, tr!("Play")),
    };
    if named(ui.button(glyph), words)
        .on_hover_text(words)
        .clicked()
    {
        motion.toggle();
    }
    small(ui, motion.label.clone());
    let mut looping = motion.transport.is_looping();
    if ui
        .checkbox(&mut looping, tr!("Loop"))
        .on_hover_text(tr!("Play the clip again from its start whenever it ends"))
        .changed()
    {
        motion.transport.set_looping(looping);
    }
    let rate = motion.transport.rate;
    combo(ui, tr!("Speed"), &format!("{rate}x"), |ui| {
        for speed in SPEEDS {
            ui.selectable_value(&mut motion.transport.rate, speed, format!("{speed}x"));
        }
    });
    if dressed.clips.len() > 1 {
        let shown = (dressed.clips.iter())
            .find(|(key, _)| *key == motion.key)
            .map_or(tr!("Clip"), |(_, name)| name.as_str());
        combo(ui, tr!("Clip"), shown, |ui| {
            for (key, name) in &dressed.clips {
                if ui.selectable_label(*key == motion.key, name).clicked() {
                    *asked = Some(Play::Clip(*key));
                }
            }
        });
    }
}
