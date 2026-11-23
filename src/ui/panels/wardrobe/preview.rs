//! The dressed ped: the ped in the middle of the Wardrobe with what it wears, the tools that change how it is seen, and the texture variants under it.
//!
//! ⚠ **Thin by design.** What is worn is `or_wardrobe::dress`'s, the files are read by the state, and
//! a texture is made from a dropped image by the engine; this lays out the tools, the viewport and the tiles.
//!
//! The tools stand in rows that keep their places (who wears it, which version is shown, how it is drawn, the parts and the motion), the view takes
//! what is left, and under it the view helpers, the notes and the tiles; the view is never drawn over them.

pub(super) use super::tiles::tile_texture;

use super::{overlay, tiles, tools, wear, WardrobeUi};
use crate::render::viewport::CameraInputs;
use crate::render::RenderBridge;
use crate::state::AppState;
use crate::ui::accessible_name::named;
use crate::ui::theme;

/// The least the 3D view is drawn when the dock is far too short for everything: drawn smaller than this it would be no view.
const VIEW_MIN: f32 = 96.0;

/// What the view is given before the tiles take their full size: with less than this, the tiles are one compact row.
const VIEW_COMFORTABLE: f32 = 220.0;

/// The height of the helpers, the notes and a row of full tiles, which the view leaves room for.
const STRIP_FULL: f32 = 156.0;

/// The row of the clip that plays, which the strip holds while one does.
const TRANSPORT_ROW: f32 = 28.0;

pub fn draw(ui: &mut egui::Ui, state: &mut AppState, bridge: Option<&mut RenderBridge>) {
    if !state.has_clothing() {
        crate::ui::panels::empty_state::draw(ui, state);
        return;
    }
    let mut shown = state.panels.take::<WardrobeUi>();
    shown.drop_preview = Some((ui.max_rect(), ui.ctx().cumulative_pass_nr()));
    let dressed = &state.workbench.wardrobe.dressed;
    if dressed.is_reading() || dressed.is_cataloguing() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
    if dressed.is_animating() {
        ui.ctx().request_repaint();
    }
    wear::wear(ui, state);
    wear::show(ui, state);
    tools::draw(ui, state);
    let playing = overlay::is_playing(state);
    let strip = STRIP_FULL + if playing { TRANSPORT_ROW } else { 0.0 };
    let compact = ui.available_height() < strip + VIEW_COMFORTABLE;
    egui::Panel::bottom(ui.id().with("dressed_strip"))
        .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(8, 3)))
        .show_inside(ui, |ui| {
            overlay::helpers(ui, state);
            overlay::transport_row(ui, state);
            tiles::notes(ui, state);
            tiles::variants(ui, state, &mut shown, compact);
        });
    view(ui, state, bridge);
    state.panels.put(shown);
}

/// The 3D view: orbit by dragging, pan with the middle button or shift, zoom with the wheel.
fn view(ui: &mut egui::Ui, state: &mut AppState, bridge: Option<&mut RenderBridge>) {
    let size = egui::vec2(ui.available_width(), ui.available_height().max(VIEW_MIN));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let response = named(response, tr!("Dressed ped"));
    match bridge {
        Some(bridge) => draw_scene(ui, state, bridge, rect, &response),
        None => {
            ui.painter().rect_filled(rect, 0.0, theme::BG_L2);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                tr!("Viewport (renderer not initialized)"),
                egui::FontId::default(),
                theme::TEXT_SECONDARY.get(),
            );
        }
    }
    overlay::escape_leaves(ui, &response, state);
    overlay::first_person(ui, rect, state);
    overlay::no_game(ui, rect, state);
    overlay::bottom(ui, rect, state);
}

fn draw_scene(
    ui: &mut egui::Ui,
    state: &mut AppState,
    bridge: &mut RenderBridge,
    rect: egui::Rect,
    response: &egui::Response,
) {
    // The textures are the point of a dressed ped, so it starts in Material; the view is the person's after that.
    let (shading, channel) = {
        let dressed = &state.workbench.wardrobe.dressed;
        (dressed.view.shading(), dressed.view.channel())
    };
    bridge.set_mode(shading);
    let ppp = ui.ctx().pixels_per_point();
    bridge.set_desired_size(
        (rect.width() * ppp).round() as u32,
        (rect.height() * ppp).round() as u32,
    );
    let dressed = &state.workbench.wardrobe.dressed;
    bridge.refresh_dressed_as(dressed.scene(), channel);
    bridge.refresh_dressed_game(dressed.scene());
    bridge.pose_dressed_game(dressed.motion.as_ref().map(|m| (m.revision(), m.skin())));
    if let Some(scene) = dressed.scene() {
        let pose = dressed.motion.as_ref().map(|m| (m.revision(), m.skin()));
        bridge.pose_dressed(scene, pose);
    }
    // ⚠ After the ped is posed: the wireframe is drawn from the vertices the pose wrote.
    bridge.refresh_lines(state);

    let mut inputs = CameraInputs::default();
    if response.dragged_by(egui::PointerButton::Middle)
        || (response.dragged_by(egui::PointerButton::Primary) && ui.input(|i| i.modifiers.shift))
    {
        inputs.pan = response.drag_delta();
    } else if response.dragged_by(egui::PointerButton::Primary) {
        inputs.orbit = response.drag_delta();
    }
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        if scroll.y != 0.0 {
            inputs.zoom = (scroll.y / 80.0).clamp(-2.0, 2.0);
        }
    }
    bridge.push_inputs(inputs);

    match bridge.texture_id() {
        Some(texture) => {
            ui.painter().image(
                texture,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            ui.painter().rect_filled(rect, 0.0, theme::BG_L2);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                tr!("Initializing viewport…"),
                egui::FontId::default(),
                theme::TEXT_SECONDARY.get(),
            );
        }
    }
    ui.painter().rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0_f32, theme::BORDER),
        egui::StrokeKind::Inside,
    );
    if !state.dressed_is_ready() {
        let words = if state.workbench.wardrobe.dressed.is_reading() {
            tr!("Reading the files…")
        } else {
            tr!("Select a garment to see it worn")
        };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            words,
            egui::FontId::proportional(14.0),
            theme::TEXT_SECONDARY.get(),
        );
    }
}
