//! The GTA VI archive explorer panel: the build's RPF9 archives down one side, and for the chosen
//! one its deciphered entries — each entry's name64 and, where the recovered-names corpus is set,
//! its path — and its index groups. The data is read through [`Gta6ArchiveExplorer`]; this only
//! draws it. Its state is a child of this panel module, so declaring it costs `state/mod.rs` no line.

mod state;

use state::Gta6ArchiveExplorer;

use crate::state::AppState;
use crate::ui::panel_registry::{Panel, PanelContext, PanelId, PanelRegistry};
use crate::ui::theme;

/// The explorer's panel id, under which the View menu offers it and a saved layout names it.
pub const ID: PanelId = "gta6_archive_tree";

struct Gta6ArchiveTree;

impl Panel for Gta6ArchiveTree {
    fn id(&self) -> PanelId {
        ID
    }

    fn title(&self, _state: &AppState) -> String {
        crate::i18n::tr("Archives").to_string()
    }

    fn draw(&self, ui: &mut egui::Ui, ctx: &mut PanelContext<'_>) {
        draw(ui, ctx.state);
    }
}

static PANEL: Gta6ArchiveTree = Gta6ArchiveTree;

/// Register the explorer into `registry`, so the View menu offers it. Called once as the live panel
/// registry is built, rather than as a builtin, because GTA VI support is read-only and experimental
/// and the panel draws only when a build is set — a dynamic panel is in no default layout's count.
pub fn register_into(registry: &mut PanelRegistry) {
    registry.register(&PANEL);
}

pub fn draw(ui: &mut egui::Ui, state: &mut AppState) {
    // Always paint the heading, so the panel draws something even with no build set (the self-check
    // fails a panel that paints nothing).
    ui.label(egui::RichText::new(crate::i18n::tr("Archives")).strong());

    // The archive names are read once and cached, so listing them is cheap every frame.
    let archives: Vec<String> = state
        .panels
        .get::<Gta6ArchiveExplorer>()
        .ensure_listed()
        .to_vec();

    if archives.is_empty() {
        // No build is set (OPENRAGE_GTA6_BUILD), so there is nothing to list; a weak hint names the
        // variable to set. ⚠ Untranslated: this is a developer-only diagnostic for an experimental,
        // read-only panel, like the open-error strings in its state (see the module header). It also
        // keeps the panel painting content of its own when empty, which the self-check requires.
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(
            egui::RichText::new(format!(
                "Set {} to a GTA VI build folder to list its RPF9 archives.",
                state::BUILD_ENV
            ))
            .weak(),
        );
        return;
    }

    // The archives as a wrapped row of selectable chips; clicking one opens it.
    let mut to_open: Option<String> = None;
    let selected = state.panels.get::<Gta6ArchiveExplorer>().selected.clone();
    ui.horizontal_wrapped(|ui| {
        for name in &archives {
            let is_sel = selected.as_deref() == Some(name.as_str());
            if ui.selectable_label(is_sel, name.as_str()).clicked() {
                to_open = Some(name.clone());
            }
        }
    });
    if let Some(name) = to_open {
        state.panels.get::<Gta6ArchiveExplorer>().open(&name);
    }
    ui.separator();

    let explorer = state.panels.get::<Gta6ArchiveExplorer>();
    if let Some(error) = &explorer.error {
        let color = ui.visuals().error_fg_color;
        ui.colored_label(color, error.as_str());
        return;
    }
    let Some(listing) = &explorer.listing else {
        return;
    };
    ui.monospace(format!(
        "{}: {} of {} entries, index key {:#010X}",
        listing.name,
        listing.entries.len(),
        listing.entry_count,
        listing.key_id
    ));
    ui.add_space(theme::SPACING_TIGHT);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.label(egui::RichText::new(crate::i18n::tr("Groups")).strong());
            for g in &listing.groups {
                ui.monospace(format!(
                    "{:#018X}  ×{:<6} w{:<3} {}",
                    g.name_hash,
                    g.count,
                    g.width,
                    if g.external { "ext" } else { "main" }
                ));
            }
            ui.add_space(theme::SPACING_TIGHT);
            ui.label(egui::RichText::new(crate::i18n::tr("Entries")).strong());
            for e in &listing.entries {
                let size = e
                    .size
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "-".to_string());
                let name = e.name.as_deref().unwrap_or("");
                ui.monospace(format!(
                    "{:>5}  {:#018X}  {:>10}  {name}",
                    e.index, e.name64, size
                ));
            }
        });
}
