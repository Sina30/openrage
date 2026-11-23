//! The assembled weapon: the weapon with the parts its metadata attaches, drawn in the viewport's way.
//!
//! ⚠ **Thin by design.** Which part sits at which bone is `or_weapons::assemble`'s; the files are read on a worker by the state, and this lays out
//! the part pickers, the shading and the view. A part is drawn where its own model puts it: how the game aligns a part's bone to the
//! weapon's is not measured, so the view does not move one.

use or_weapons::assemble::assemble;

use super::WeaponsUi;
use crate::render::viewport::CameraInputs;
use crate::render::RenderBridge;
use crate::state::workbench::ViewMode;
use crate::state::AppState;
use crate::ui::accessible_name::named;
use crate::ui::theme;

/// The least the 3D view is drawn when the dock is far too short for everything.
const VIEW_MIN: f32 = 96.0;

/// The ways the weapon is drawn: its shape, its textures and its edges.
const VIEWS: [ViewMode; 3] = [ViewMode::Solid, ViewMode::Material, ViewMode::Wireframe];

/// What a bone offers: the bone, the component worn there now and the names of the others.
struct Place {
    bone: String,
    worn: Option<String>,
    options: Vec<String>,
}

fn places(state: &AppState) -> Vec<Place> {
    let Some((pack, weapon)) = state.selected_weapon() else {
        return Vec::new();
    };
    let Some(read) = pack.read.as_ref().ok() else {
        return Vec::new();
    };
    let assembly = assemble(weapon, &read.facts);
    let choice = &state.workbench.weapons.choice;
    let worn = or_weapons::assemble::worn(&assembly, choice);
    let mut places: Vec<Place> = Vec::new();
    for part in &assembly.parts {
        match places.iter_mut().find(|p| p.bone == part.bone) {
            Some(place) => place.options.push(part.component.clone()),
            None => places.push(Place {
                bone: part.bone.clone(),
                worn: worn
                    .iter()
                    .find(|w| w.bone == part.bone)
                    .map(|w| w.component.clone()),
                options: vec![part.component.clone()],
            }),
        }
    }
    places
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState, bridge: Option<&mut RenderBridge>) {
    if !state.has_weapons() {
        crate::ui::panels::empty_state::draw(ui, state);
        return;
    }
    let mut shown = state.panels.take::<WeaponsUi>();
    shown.drop_preview = Some((ui.max_rect(), ui.ctx().cumulative_pass_nr()));
    match state.weapon_scene_request() {
        Some(request) => {
            let crypto = state.keys.provider();
            state.workbench.weapons.scene.want(request, crypto);
        }
        None => state.workbench.weapons.scene.clear(),
    }
    state.workbench.weapons.scene.poll();
    if state.workbench.weapons.scene.is_reading() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
    if state.selected_weapon().is_none() {
        ui.add_space(theme::SPACING_DEFAULT);
        ui.label(
            egui::RichText::new(tr!("Select a weapon to see it assembled"))
                .color(theme::TEXT_SECONDARY),
        );
        state.panels.put(shown);
        return;
    }
    tools(ui, state);
    view(ui, state, bridge);
    state.panels.put(shown);
}

/// How the weapon is drawn, and the part worn at each bone.
fn tools(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr!("Shown as")).color(theme::TEXT_SECONDARY));
        for mode in VIEWS {
            let on = state.workbench.weapons.view == mode;
            let response = ui.selectable_label(on, mode.name());
            if named(response, mode.name()).clicked() {
                state.workbench.weapons.view = mode;
            }
        }
    });
    for place in places(state) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(tr!("Part at {bone}", bone = place.bone.as_str()))
                    .color(theme::TEXT_SECONDARY),
            );
            let none = place.worn.is_none();
            let response = ui.selectable_label(none, tr!("None"));
            if named(
                response,
                &tr!("No part at {bone}", bone = place.bone.as_str()),
            )
            .clicked()
            {
                state
                    .workbench
                    .weapons
                    .choice
                    .insert(place.bone.clone(), None);
            }
            for option in &place.options {
                let on = place.worn.as_deref() == Some(option.as_str());
                if ui.selectable_label(on, option).clicked() {
                    state
                        .workbench
                        .weapons
                        .choice
                        .insert(place.bone.clone(), Some(option.clone()));
                }
            }
        });
    }
}

/// The 3D view: orbit by dragging, pan with the middle button or shift, zoom with the wheel.
fn view(ui: &mut egui::Ui, state: &mut AppState, bridge: Option<&mut RenderBridge>) {
    let size = egui::vec2(ui.available_width(), ui.available_height().max(VIEW_MIN));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let response = named(response, tr!("Assembled weapon"));
    let Some(bridge) = bridge else {
        ui.painter().rect_filled(rect, 0.0, theme::BG_L2);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            tr!("Viewport (renderer not initialized)"),
            egui::FontId::default(),
            theme::TEXT_SECONDARY.get(),
        );
        return;
    };
    let mode = state.workbench.weapons.view;
    bridge.set_mode(mode.shading());
    let ppp = ui.ctx().pixels_per_point();
    bridge.set_desired_size(
        (rect.width() * ppp).round() as u32,
        (rect.height() * ppp).round() as u32,
    );
    bridge.refresh_dressed_as(state.workbench.weapons.scene.scene.as_ref(), mode.channel());
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
    let said = match (
        &state.workbench.weapons.scene.error,
        state.workbench.weapons.scene.is_reading(),
    ) {
        (Some(why), _) => Some(why.clone()),
        (None, true) => Some(tr!("Reading the files…").to_string()),
        (None, false) if state.workbench.weapons.scene.scene.is_none() => {
            Some(tr!("The weapon has no model in the pack to draw").to_string())
        }
        _ => None,
    };
    if let Some(said) = said {
        ui.painter().text(
            rect.center_bottom() - egui::vec2(0.0, 12.0),
            egui::Align2::CENTER_BOTTOM,
            said,
            egui::FontId::default(),
            theme::TEXT_SECONDARY.get(),
        );
    }
}
