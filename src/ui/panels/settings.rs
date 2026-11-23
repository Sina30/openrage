use super::question;
use super::settings_appearance::draw_appearance_page;
use super::settings_editing::draw_editing_page;
use super::settings_materials::draw_materials_page;
use super::settings_shortcuts;
use crate::platform::file_dialogs;
use crate::state::{AppState, EditMode, ExportFixes, Game, Prefs, RpfOutputMode};
use crate::ui::panel_state::PanelState;
use crate::ui::theme;

#[derive(Default)]
pub struct SettingsUiState {
    pub category: SettingsCategory,
    pub query: String,
}

impl PanelState for SettingsUiState {
    const KEY: &'static str = "settings-page";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SettingsCategory {
    #[default]
    Games,
    Editing,
    Appearance,
    Viewport,
    Materials,
    Shortcuts,
    FileAssociations,
    About,
}

impl SettingsCategory {
    pub const ALL: [SettingsCategory; 8] = [
        Self::Games,
        Self::Editing,
        Self::Appearance,
        Self::Viewport,
        Self::Materials,
        Self::Shortcuts,
        Self::FileAssociations,
        Self::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Games => tr!("Games"),
            Self::Editing => tr!("Editing"),
            Self::Appearance => tr!("Appearance"),
            Self::Viewport => tr!("Viewport"),
            Self::Materials => tr!("Materials"),
            Self::Shortcuts => tr!("Keyboard shortcuts"),
            Self::FileAssociations => tr!("File associations"),
            Self::About => tr!("About"),
        }
    }

    pub fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::Games => &[
                "games",
                "game",
                "gta",
                "rdr2",
                "install",
                "folder",
                "active",
                "switch",
                "video",
                "bik",
                "bink",
                "project",
                "launch",
                "reopen",
                "start",
                "keys",
                "key",
                "encryption",
                "encrypt",
                "decrypt",
                "aes",
                "ng",
                "magic.dat",
                "magic",
                "extract",
                "gta5.exe",
                "snapshot",
            ],
            Self::Editing => &[
                "edit",
                "editing",
                "mode",
                "read only",
                "write",
                "base game",
                "mods",
                "overlay",
                "encryption",
                "rpf",
                "open",
                "output",
                "save",
                "export",
                "fixes",
            ],
            Self::Appearance => &[
                "appearance",
                "theme",
                "dark",
                "light",
                "accent",
                "colour",
                "color",
                "scale",
                "zoom",
                "dpi",
                "size",
                "font",
            ],
            Self::Viewport => &[
                "viewport", "view", "grid", "axis", "axes", "overlay", "show",
            ],
            Self::Materials => &[
                "materials",
                "material",
                "bound",
                "bounds",
                "ybn",
                "palette",
                "colour",
                "color",
                "tint",
            ],
            Self::Shortcuts => &[
                "shortcut",
                "shortcuts",
                "keyboard",
                "keybinding",
                "keybindings",
                "binding",
                "bindings",
                "hotkey",
                "hotkeys",
                "rebind",
            ],
            Self::FileAssociations => &[
                "file",
                "association",
                "associations",
                "open with",
                "default",
                "program",
                "double-click",
                "extension",
                "register",
                "shell",
                "explorer",
            ],
            Self::About => &["about", "version", "license", "credits"],
        }
    }

    /// Each option's words as its page draws them, for the search.
    fn options(self) -> Vec<&'static str> {
        match self {
            Self::Games => vec![
                tr!("Reopen the last project at launch"),
                tr!("Active game"),
                tr!("Select folder…"),
                tr!("Change folder…"),
                tr!("Set active"),
                tr!("Forget folder"),
                tr!("Keys"),
                tr!("Select gta5.exe…"),
                tr!("Extract Keys"),
                tr!("Re-extract Keys"),
                tr!("Change gta5.exe…"),
                tr!("Forget"),
                tr!("Advanced"),
                tr!("AES key only"),
                tr!("Your own magic.dat…"),
                tr!("Pre-derive NG encrypt state"),
            ],
            Self::Editing => vec![
                tr!("Edit mode"),
                tr!("Revoke this session's base-game permission"),
                tr!("Read the install's mods overlay first"),
                tr!("When saving and exporting"),
                tr!("Encryption for archives OpenRage saves"),
                tr!("When OpenRage's export fixes would alter a file's bytes"),
            ],
            Self::Appearance => vec![
                tr!("Interface scale"),
                tr!("Reduce motion"),
                tr!("Restore defaults"),
            ],
            Self::Viewport => vec![tr!("Show grid"), tr!("Show world axes")],
            Self::Materials => vec![
                tr!("Shared texture search paths"),
                tr!("Add folder…"),
                tr!("Remove"),
                tr!("Shader definitions"),
                tr!("Bound material colours"),
                tr!("Reload from GTA install"),
            ],
            Self::Shortcuts => vec![tr!("Restore all defaults"), tr!("Edit"), tr!("Reset")],
            Self::FileAssociations => vec![
                tr!("Game formats"),
                tr!("None"),
                tr!("Also make OpenRage the default for these (double-click opens them here)"),
                tr!("Register"),
                tr!("Undo"),
            ],
            Self::About => vec![tr!("Copy version info")],
        }
    }
}

pub use super::settings_window::{
    is_settings_open, open_settings, open_settings_at, set_settings_open, take_raise,
};

pub fn draw_settings_window_body(ctx: &egui::Context, state: &mut AppState) {
    crate::ui::root::with_root_ui(ctx, |ui| {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::BG_L0.get()).inner_margin(12))
            .show_inside(ui, |ui| {
                draw_settings_body(ui, state);
            });
    });
    crate::ui::focus_ring::draw(ctx);
    // ⚠ Every other window and question closes on Escape. After the page, so that a field that takes the key (the search box clearing its text, a shortcut being captured) has it first.
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.egui_wants_keyboard_input() {
        set_settings_open(state, false);
    }
}

pub fn reset_settings_transient(ctx: &egui::Context) {
    settings_shortcuts::reset_transient(ctx);
}

fn draw_settings_body(ui: &mut egui::Ui, state: &mut AppState) {
    ui.set_min_width(560.0);

    let mut settings = state.panels.take::<SettingsUiState>();

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(tr!("Search:"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        let edit = egui::TextEdit::singleline(&mut settings.query)
            .id_salt("settings_search")
            .hint_text(tr!("Type to filter settings…"))
            .desired_width(f32::INFINITY);
        ui.add(edit);
    });
    ui.add_space(theme::SPACING_TIGHT);
    ui.separator();
    ui.add_space(theme::SPACING_TIGHT);

    let query = settings.query.trim().to_string();
    let query_lc = query.to_ascii_lowercase();
    if !category_matches(settings.category, &query_lc, &state.prefs) {
        if let Some(found) = SettingsCategory::ALL
            .into_iter()
            .find(|&category| category_matches(category, &query_lc, &state.prefs))
        {
            settings.category = found;
        }
    }

    egui::Panel::left("settings_rail")
        .resizable(false)
        .exact_size(180.0)
        .frame(egui::Frame::NONE.fill(theme::BG_L1.get()).inner_margin(8))
        .show_inside(ui, |ui| {
            draw_rail(ui, &mut settings, &query_lc, &state.prefs);
        });

    let category = settings.category;
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.inner_margin(12))
        .show_inside(ui, |ui| draw_active_page(ui, state, category, &query_lc));

    state.panels.put(settings);
}

fn draw_rail(ui: &mut egui::Ui, settings: &mut SettingsUiState, query_lc: &str, prefs: &Prefs) {
    ui.add_space(theme::SPACING_HAIR);
    for category in SettingsCategory::ALL {
        let active = settings.category == category;
        let matches_query = category_matches(category, query_lc, prefs);
        let bg = if active {
            theme::ACCENT.get()
        } else {
            egui::Color32::TRANSPARENT
        };
        let text_color = if active {
            theme::ON_ACCENT.get()
        } else if matches_query {
            theme::TEXT_PRIMARY.get()
        } else {
            theme::TEXT_DISABLED.get()
        };
        // Italic as well as dim: the dim colour is too near the others' to say it alone.
        let mut label = egui::RichText::new(category.label()).color(text_color);
        if !active && !matches_query {
            label = label.italics();
        }
        let button = egui::Button::new(label)
            .fill(bg)
            .stroke(egui::Stroke::NONE)
            .corner_radius(egui::CornerRadius::same(theme::RADIUS_SMALL))
            .min_size(egui::vec2(ui.available_width(), 28.0));
        let response = ui.add(button).on_hover_text(category_hover_hint(category));
        if response.clicked() {
            settings.category = category;
        }
        ui.add_space(theme::SPACING_HAIR);
    }
}

fn draw_active_page(
    ui: &mut egui::Ui,
    state: &mut AppState,
    category: SettingsCategory,
    query_lc: &str,
) {
    if !category_matches(category, query_lc, &state.prefs) {
        empty_search_state(ui, query_lc);
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match category {
            SettingsCategory::Games => super::settings_games::draw_games_page(ui, state),
            SettingsCategory::Editing => draw_editing_page(ui, state),
            SettingsCategory::Appearance => draw_appearance_page(ui, state),
            SettingsCategory::Viewport => draw_viewport_page(ui, state),
            SettingsCategory::Materials => draw_materials_page(ui, state),
            SettingsCategory::Shortcuts => {
                settings_shortcuts::draw_shortcuts_page(ui, state, query_lc)
            }
            SettingsCategory::FileAssociations => {
                super::settings_associations::draw_associations_page(ui, state)
            }
            SettingsCategory::About => draw_about_page(ui, state),
        });
}

fn empty_search_state(ui: &mut egui::Ui, query_lc: &str) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(tr!(
                "No setting matches \"{query_lc}\"",
                query_lc = query_lc
            ))
            .color(theme::TEXT_SECONDARY),
        );
    });
}

/// Whether `category`'s page holds an option the search names; an empty search names every page.
fn category_matches(category: SettingsCategory, query_lc: &str, prefs: &Prefs) -> bool {
    let found = |text: &str| text.to_ascii_lowercase().contains(query_lc);
    if found(category.label())
        || category.keywords().iter().any(|kw| found(kw))
        || category.options().iter().any(|option| found(option))
    {
        return true;
    }
    match category {
        SettingsCategory::Games => Game::all().any(|game| found(game.label())),
        SettingsCategory::Editing => {
            EditMode::ALL.iter().any(|mode| found(mode.label()))
                || RpfOutputMode::ALL.iter().any(|mode| found(mode.label()))
                || ExportFixes::ALL.iter().any(|fixes| found(fixes.label()))
        }
        SettingsCategory::FileAssociations => crate::platform::associations::candidates()
            .iter()
            .any(|c| found(&format!(".{}", c.ext)) || found(&c.noun)),
        SettingsCategory::Shortcuts => crate::ui::commands::COMMANDS
            .iter()
            .any(|spec| settings_shortcuts::matches_query(spec, prefs, query_lc)),
        _ => false,
    }
}

fn category_hover_hint(category: SettingsCategory) -> &'static str {
    match category {
        SettingsCategory::Games => tr!("Active game, per-game install folders and keys"),
        SettingsCategory::Editing => {
            tr!("The edit mode, the mods overlay, and what saving and exporting write")
        }
        SettingsCategory::Appearance => tr!("Theme, accent colour, interface scale and font"),
        SettingsCategory::Viewport => tr!("Viewport overlay toggles (grid, axes)"),
        SettingsCategory::Materials => tr!("Bound mesh material colour palette source"),
        SettingsCategory::Shortcuts => tr!("Search and rebind every editor keyboard shortcut"),
        SettingsCategory::FileAssociations => {
            tr!("Open game files from Explorer in OpenRage, and take that back")
        }
        SettingsCategory::About => tr!("Version, license, and project info"),
    }
}

fn draw_viewport_page(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading(tr!("Viewport"));
    ui.add_space(theme::SPACING_HAIR);
    ui.label(
        egui::RichText::new(tr!(
            "Overlay toggles for the 3D viewport. These are mirrored into prefs and \
             survive across launches."
        ))
        .small()
        .weak(),
    );
    ui.add_space(theme::SPACING_DEFAULT);

    let mut grid = state.prefs.viewport.show_grid;
    if ui
        .checkbox(&mut grid, tr!("Show grid"))
        .on_hover_text(tr!("Render a 1-unit reference grid at the world origin."))
        .changed()
    {
        state.prefs.viewport.show_grid = grid;
        state.persist_viewport_prefs();
    }

    let mut axis = state.prefs.viewport.show_axis;
    if ui
        .checkbox(&mut axis, tr!("Show world axes"))
        .on_hover_text(tr!("Render the X / Y / Z world-axis gizmo at the origin."))
        .changed()
    {
        state.prefs.viewport.show_axis = axis;
        state.persist_viewport_prefs();
    }
}

fn draw_about_page(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading(tr!("About OpenRage"));
    ui.add_space(theme::SPACING_DEFAULT);

    ui.horizontal(|ui| {
        ui.add_sized(
            egui::vec2(56.0, 56.0),
            crate::ui::branding::app_logo_image(56.0),
        );
        ui.add_space(theme::SPACING_LOOSE);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("OpenRage").size(20.0).strong());
            ui.label(
                egui::RichText::new(crate::i18n::tr(crate::ui::branding::TAGLINE))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
            ui.label(
                egui::RichText::new(tr!(
                    "Version {version}",
                    version = env!("CARGO_PKG_VERSION")
                ))
                .monospace(),
            );
        });
    });

    ui.add_space(theme::SPACING_DEFAULT);
    if let Some(active) = state.prefs.active_game {
        ui.label(
            egui::RichText::new(tr!("Targeting: {active}", active = active.label()))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACING_TIGHT);
    }
    ui.label(
        egui::RichText::new(crate::i18n::tr(crate::ui::branding::DESCRIPTION))
            .small()
            .weak(),
    );
    ui.add_space(theme::SPACING_DEFAULT);

    if ui
        .button(tr!("Copy version info"))
        .on_hover_text(tr!(
            "Copy `OpenRage <version> (target: <game>)` to the clipboard, handy for bug reports."
        ))
        .clicked()
    {
        let info = format!(
            "OpenRage {} (target: {})",
            env!("CARGO_PKG_VERSION"),
            state
                .prefs
                .active_game
                .map(|g| g.label())
                .unwrap_or("unset")
        );
        ui.ctx().copy_text(info);
        state.show_toast(tr!("Version info copied to clipboard"));
    }
    ui.add_space(theme::SPACING_TIGHT);
    ui.label(
        egui::RichText::new(tr!("Third-party licenses: see LICENSE-third-party.md"))
            .small()
            .weak(),
    );
}

pub fn draw_keys_prompt_modal(ctx: &egui::Context, state: &mut AppState) {
    if !state.keys.show_prompt {
        return;
    }
    let pending_count = state.keys.pending_open.len();
    let mut dismissed = false;
    let mut extract_clicked = false;
    question(ctx, tr!("Encrypted archive"), |ui| {
        if pending_count == 0 {
            ui.label(tr!("Open an encrypted archive to see this prompt."));
        } else {
            ui.label(format!(
                "{} encrypted archive{} waiting on the GTA V keys.",
                pending_count,
                if pending_count == 1 { "" } else { "s" }
            ));
        }
        ui.add_space(theme::SPACING_DEFAULT);
        ui.label(tr!(
            "This archive is encrypted with Rockstar's keys. OpenRage needs \
             them to read its list of files and the files themselves."
        ));
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(
            egui::RichText::new(tr!(
                "Pick your gta5.exe to extract the keys — usually a few \
                 seconds. NG keys come from your own magic.dat beside the keys \
                 cache, else from the key snapshot bundled with OpenRage. The \
                 deferred archives will load automatically when extraction completes."
            ))
            .small()
            .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACING_RELAXED);
        ui.horizontal(|ui| {
            if ui.button(tr!("Select gta5.exe…")).clicked() {
                if let Some(path) = file_dialogs::pick_gta5_exe() {
                    state.start_key_extraction(path);
                    extract_clicked = true;
                }
            }
            if ui.button(tr!("Cancel")).clicked() {
                let _ = state.keys.drain_pending();
                dismissed = true;
            }
        });
    });
    if dismissed || extract_clicked {
        state.keys.show_prompt = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::Role;

    /// The first line of each button, box and choice one frame of `category`'s page draws.
    fn options_on(category: SettingsCategory, state: &mut AppState) -> Vec<String> {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut options = Vec::new();
        // ⚠ A collapsing header's first frame only measures it.
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 4000.0),
                )),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| draw_active_page(ui, state, category, ""));
            options = output
                .platform_output
                .accesskit_update
                .map(|update| {
                    update
                        .nodes
                        .iter()
                        .filter(|(_, node)| {
                            matches!(
                                node.role(),
                                Role::Button | Role::CheckBox | Role::RadioButton | Role::Link
                            )
                        })
                        .filter_map(|(_, node)| node.label()?.lines().next().map(str::trim))
                        .filter(|label| label.chars().any(char::is_alphanumeric))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
        }
        options
    }

    #[test]
    fn the_settings_search_finds_every_option() {
        let mut state = AppState::default();
        let mut missed = Vec::new();
        for category in SettingsCategory::ALL {
            let options = options_on(category, &mut state);
            assert!(!options.is_empty(), "{} drew no option", category.label());
            for option in options {
                if !category_matches(category, &option.to_ascii_lowercase(), &state.prefs) {
                    missed.push(format!("{option} ({})", category.label()));
                }
            }
        }
        assert!(
            missed.is_empty(),
            "searching for these finds nothing: {missed:#?}"
        );
    }

    #[test]
    fn typing_an_options_name_shows_the_page_that_holds_it() {
        let mut state = AppState::default();
        let page = state.panels.get::<SettingsUiState>();
        page.category = SettingsCategory::Games;
        page.query = "world axes".to_string();
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                draw_settings_body(ui, &mut state)
            });
        }
        assert_eq!(
            state.panels.get::<SettingsUiState>().category,
            SettingsCategory::Viewport
        );
    }

    #[test]
    fn a_page_the_search_misses_is_in_italics_as_well_as_dim() {
        let mut state = AppState::default();
        let page = state.panels.get::<SettingsUiState>();
        page.category = SettingsCategory::Viewport;
        page.query = "world axes".to_string();
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            };
            shapes = ctx
                .run_ui(input, |ui| draw_settings_body(ui, &mut state))
                .shapes;
        }
        fn italic(shape: &egui::Shape, text: &str, out: &mut Vec<bool>) {
            match shape {
                egui::Shape::Text(t) if t.galley.text() == text => {
                    out.push(t.galley.job.sections.iter().all(|s| s.format.italics));
                }
                egui::Shape::Vec(inner) => inner.iter().for_each(|s| italic(s, text, out)),
                _ => {}
            }
        }
        let italic_of = |text: &str| {
            let mut out = Vec::new();
            shapes.iter().for_each(|c| italic(&c.shape, text, &mut out));
            out
        };
        assert_eq!(italic_of("Games"), [true], "the rail's page that misses");
        assert_eq!(italic_of("Editing"), [true], "the rail's page that misses");
        let found = italic_of("Viewport");
        assert!(
            !found.is_empty() && !found.contains(&true),
            "the page that matches stays upright: {found:?}"
        );
    }
}
