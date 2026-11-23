//! The Materials page of the settings window: bound material colours, the
//! shader definition database, and the ordered texture search paths.

use crate::platform::file_dialogs;
use crate::state::{AppState, MaterialPaletteStatus, ShaderDefsStatus};
use crate::ui::theme;

/// The ordered folders searched for a `.ytd` nothing open holds.
///
/// ⚠ The order is the feature, so the list is reorderable and never sorted
/// for display. Two folders holding a dictionary of the same name is the
/// normal case — a mod's and the base game's — and the first match wins.
fn draw_texture_paths_section(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading(tr!("Shared texture search paths"));
    ui.add_space(theme::SPACING_HAIR);
    ui.label(
        egui::RichText::new(tr!(
            "Searched in order when a drawable names a texture no open dictionary \
             holds, after a dictionary travelling in the same archive. The first \
             match wins, so put the folder you want to override with at the top."
        ))
        .small()
        .weak(),
    );
    ui.add_space(theme::SPACING_TIGHT);

    let mut remove: Option<usize> = None;
    let mut swap: Option<(usize, usize)> = None;
    let count = state.prefs.texture_search_paths.len();

    for (index, path) in state.prefs.texture_search_paths.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{}.", index + 1))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
            if ui
                .add_enabled(index > 0, egui::Button::new("↑").small())
                .clicked()
            {
                swap = Some((index, index - 1));
            }
            if ui
                .add_enabled(index + 1 < count, egui::Button::new("↓").small())
                .clicked()
            {
                swap = Some((index, index + 1));
            }
            if ui.small_button(tr!("Remove")).clicked() {
                remove = Some(index);
            }
            let missing = !path.is_dir();
            let text = egui::RichText::new(path.display().to_string()).small();
            let text = if missing {
                text.color(theme::WARNING)
            } else {
                text
            };
            let label = ui.label(text);
            if missing {
                label.on_hover_text(tr!("This folder is not there now. It stays in the list."));
            }
        });
    }

    if count == 0 {
        ui.label(
            egui::RichText::new(tr!(
                "No search paths. Only open dictionaries are consulted."
            ))
            .small()
            .color(theme::TEXT_SECONDARY),
        );
    }

    ui.add_space(theme::SPACING_TIGHT);
    if ui.button(tr!("Add folder…")).clicked() {
        if let Some(folder) = file_dialogs::pick_open_folder() {
            if !state.prefs.texture_search_paths.contains(&folder) {
                state.prefs.texture_search_paths.push(folder);
                crate::state::save_prefs(&state.prefs);
            }
        }
    }

    if let Some((a, b)) = swap {
        state.prefs.texture_search_paths.swap(a, b);
        crate::state::save_prefs(&state.prefs);
    }
    if let Some(index) = remove {
        state.prefs.texture_search_paths.remove(index);
        crate::state::save_prefs(&state.prefs);
    }
    if swap.is_some() || remove.is_some() {
        // ⚠ A changed order can change which dictionary answers, and the
        // refusal to retry is remembered per document.
        state.attempted_texture_search_paths.clear();
    }
}

/// What the editor can say about a material's parameters, and what it cannot.
fn draw_shader_definitions_section(ui: &mut egui::Ui, state: &AppState) {
    ui.heading(tr!("Shader definitions"));
    ui.add_space(theme::SPACING_HAIR);
    ui.label(
        egui::RichText::new(tr!(
            "What a material's parameters are — their names, types and defaults — read \
             from the shader effects and `.sps` presets your install ships. Without it a \
             parameter is a bare hash."
        ))
        .small()
        .weak(),
    );
    ui.add_space(theme::SPACING_TIGHT);

    let (colour, text) = match &state.shader_defs.status {
        ShaderDefsStatus::Absent => (
            theme::WARNING,
            tr!("None loaded; material parameters show as hashes").to_string(),
        ),
        ShaderDefsStatus::Loading { source } => {
            (theme::INFO, tr!("Reading {source}…", source = source))
        }
        ShaderDefsStatus::Loaded {
            source,
            shaders,
            parameters,
        } => (
            theme::SUCCESS,
            tr!(
                "{shaders} shaders, {parameters} parameters from {source}",
                shaders = shaders,
                parameters = parameters,
                source = source
            ),
        ),
        ShaderDefsStatus::Failed { source, .. } => (
            theme::WARNING,
            tr!("Load from {source} failed", source = source),
        ),
    };
    ui.colored_label(colour, text);
    if let Some(message) = state.shader_defs.status.failure_message() {
        ui.add_space(theme::SPACING_HAIR);
        ui.colored_label(theme::ERROR, tr!("Error: {message}", message = message));
    }
}

pub(super) fn draw_materials_page(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading(tr!("Bound material colours"));
    ui.add_space(theme::SPACING_HAIR);
    ui.label(
        egui::RichText::new(tr!(
            "Per-material tints used when rendering YBN bound meshes. Pulled from \
             your local `common.rpf/data/materials/materials.dat` + `materialfx.dat` \
             when keys are loaded; until then every material is a neutral grey."
        ))
        .small()
        .weak(),
    );
    ui.add_space(theme::SPACING_TIGHT);

    let palette = state.materials_palette.palette();
    let entry_count = palette.len();
    let (badge_color, badge_text) = match &state.materials_palette.status {
        MaterialPaletteStatus::NotLoaded => (
            theme::WARNING,
            tr!("Source: not loaded — no install read yet").to_string(),
        ),
        MaterialPaletteStatus::Loading { gta_root } => (
            theme::INFO,
            tr!("Loading from {gta_root}…", gta_root = gta_root.display()),
        ),
        MaterialPaletteStatus::Runtime { gta_root } => (
            theme::SUCCESS,
            tr!(
                "Source: {gta_root} ({entry_count} entries)",
                gta_root = gta_root.display(),
                entry_count = entry_count
            ),
        ),
        MaterialPaletteStatus::Failed { gta_root, .. } => (
            theme::WARNING,
            tr!(
                "Using {entry_count} entries; last load from {gta_root} failed",
                entry_count = entry_count,
                gta_root = gta_root.display()
            ),
        ),
    };
    ui.colored_label(badge_color, badge_text);

    if let Some(message) = state.materials_palette.status.failure_message() {
        ui.add_space(theme::SPACING_HAIR);
        ui.colored_label(theme::ERROR, tr!("Error: {message}", message = message));
    }

    ui.add_space(theme::SPACING_SNUG);
    let can_reload = state.prefs.gta_keys.gta5_path.is_some() && state.keys.has_keys();
    let reload_button = egui::Button::new(tr!("Reload from GTA install"));
    let response = ui
        .add_enabled(
            can_reload && !state.materials_palette.is_loading(),
            reload_button,
        )
        .on_hover_text(tr!(
            "Re-extract `materials.dat` + `materialfx.dat` from `common.rpf` \
             using the configured GTA install + extracted keys. Useful after a \
             game patch."
        ));
    if response.clicked() {
        state.reload_material_palette();
    }
    if !can_reload {
        ui.add_space(theme::SPACING_HAIR);
        ui.label(
            egui::RichText::new(tr!(
                "Configure gta5.exe and extract keys above to enable runtime palette loading."
            ))
            .small()
            .weak(),
        );
    }

    ui.add_space(theme::SPACING_RELAXED);
    ui.separator();
    ui.add_space(theme::SPACING_DEFAULT);
    draw_shader_definitions_section(ui, state);
    ui.add_space(theme::SPACING_RELAXED);
    ui.separator();
    ui.add_space(theme::SPACING_DEFAULT);
    draw_texture_paths_section(ui, state);
}
