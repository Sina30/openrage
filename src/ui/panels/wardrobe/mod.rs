//! The Wardrobe workspace's panels: the garments of a pack, which a drop on the panel fills, and the garment selected.
//!
//! ⚠ **Thin by design.** Every rule, every fix and every write is `or-wardrobe`'s and runs through
//! `state.workbench.wardrobe`; a panel reads what the last scan left and asks for a change.

mod behaviour;
mod chips;
mod cloth_wind;
mod entry_fields;
mod garment;
mod header;
pub use header::bar as draw_bar;
mod keys;
mod kit;
mod light;
mod list;
mod menus;
mod mesh;
mod overlay;
mod parts;
mod preview;
mod query_bar;
mod query_help;
mod quick;
mod recover;
mod resolve;
mod rows;
mod strip;
mod texture_places;
mod tiles;
mod tools;
mod views;
mod watched;
mod wear;
mod words;

pub use garment::draw as draw_garment;
pub(crate) use header::pack_name;
pub use list::draw as draw_list;
use or_wardrobe::Ped;
pub use preview::draw as draw_preview;
pub use words::{ped_words, slot_words};

use crate::state::AppState;
use crate::ui::commands::{self, Action};
use crate::ui::panel_state::PanelState;

/// The chip that narrows the list to some of its garments.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    #[default]
    All,
    /// A ped or a slot the names left unsaid, an error, or a duplicate.
    Attention,
    /// A model another garment ships byte for byte.
    Duplicates,
}

/// What the panels keep between frames.
#[derive(Default)]
pub struct WardrobeUi {
    /// The text the garment list is filtered by.
    pub filter: String,
    /// Only the garments of this ped.
    pub ped: Option<Ped>,
    /// Only the garments a chip picks out.
    pub chip: Chip,
    /// How many garments each saved query of the pack selects, counted once for each scan.
    smart: Option<query_bar::Smart>,
    /// The panel's rectangle in the pass it was last drawn, which is where a drop is the Wardrobe's.
    drop: Option<(egui::Rect, u64)>,
    /// The same for the dressed ped, which a drop of clothes lands on as well.
    drop_preview: Option<(egui::Rect, u64)>,
    /// The tile of each variation of the garment selected as drawn in the pass it was last drawn, with the garment and the variation.
    variants: (u64, Vec<(egui::Rect, String, usize)>),
    /// The tile pictures, by the id of the pixels they were made from.
    tiles: std::collections::HashMap<u64, egui::TextureHandle>,
    cache: Option<list::Cached>,
    /// The label being typed for a garment, kept until the field is left.
    label: Option<(String, String)>,
    /// The name being typed for a pack, with the name the pack has, kept until the field is left.
    pack: Option<(String, String)>,
    /// The heel value being set: the garment and the number, kept until the field is let go.
    heel: Option<(String, f32)>,
    /// The wind scale of a cloth being set, and what its file states.
    wind: cloth_wind::Ui,
    /// What the controls of a shop entry keep: text typed and not let go.
    entry: entry_fields::Ui,
    /// What the behaviour controls keep: a number typed, a set being made.
    behaviour: behaviour::Ui,
    /// The shop id being typed: the garment, the texture and the text, kept until the field is left.
    shop_id: Option<(String, usize, String)>,
    /// The tint being typed: the garment, the texture and the text, kept until the field is left.
    tint: Option<(String, usize, String)>,
    /// The garment panel lists the earlier versions in which the garment was not as it is.
    versions_open: bool,
    /// What the list scrolls to and which of its lines it drew.
    view: list::View,
    /// Every problem of the garment selected is listed, not only its worst.
    problems_open: bool,
    /// The tag being typed, kept until it is added.
    tag: String,
    /// The group being typed: the garment and the text, kept until the field is left.
    group: Option<(String, String)>,
    /// The Garment panel puts the keyboard in the name the next time it draws it.
    focus_name: bool,
    /// The filter has the keyboard the next time it is drawn: a term was put in it.
    focus_filter: bool,
    /// The dialog that says what garments with an unreadable name are, while it is open.
    resolving: Option<resolve::Dialog>,
}

impl PanelState for WardrobeUi {
    const KEY: &'static str = "wardrobe";
}

/// The Wardrobe panel's rectangle as drawn this frame, none when it was not drawn.
pub fn drop_rect(state: &AppState) -> Option<egui::Rect> {
    state
        .panels
        .peek::<WardrobeUi>()?
        .drop
        .map(|(rect, _)| rect)
}

/// Whether a drop at `at` lands on the Wardrobe panel or the dressed ped, as drawn in this pass.
pub fn accepts_drop_at(ctx: &egui::Context, state: &AppState, at: egui::Pos2) -> bool {
    let pass = ctx.cumulative_pass_nr();
    state.panels.peek::<WardrobeUi>().is_some_and(|ui| {
        [ui.drop, ui.drop_preview]
            .into_iter()
            .flatten()
            .any(|(rect, drawn)| drawn == pass && rect.contains(at))
    })
}

/// The garment and the variation whose tile is under `at`, as drawn in this pass.
pub fn variant_at(
    ctx: &egui::Context,
    state: &AppState,
    at: egui::Pos2,
) -> Option<(String, usize)> {
    let shown = state.panels.peek::<WardrobeUi>()?;
    let (pass, tiles) = &shown.variants;
    if *pass != ctx.cumulative_pass_nr() {
        return None;
    }
    (tiles.iter())
        .find(|(rect, ..)| rect.contains(at))
        .map(|(_, garment, variation)| (garment.clone(), *variation))
}

/// The rectangle of the tile `garment` and `variation` were last drawn in.
pub fn variant_rect(state: &AppState, garment: &str, variation: usize) -> Option<egui::Rect> {
    let shown = state.panels.peek::<WardrobeUi>()?;
    (shown.variants.1.iter())
        .find(|(_, g, v)| g == garment && *v == variation)
        .map(|(rect, ..)| *rect)
}

/// `action` as a button: its label, greyed with the reason when it cannot run; true once it ran.
pub(super) fn action_button(
    ui: &mut egui::Ui,
    state: &mut AppState,
    action: Action,
    label: Option<String>,
    filled: bool,
) -> bool {
    let Some(command) = commands::of(state, action) else {
        return false;
    };
    let text = label.unwrap_or_else(|| command.label.clone());
    let button = if filled {
        egui::Button::new(egui::RichText::new(text).color(crate::ui::theme::ON_ACCENT.get()))
            .fill(crate::ui::theme::ACCENT.get())
    } else {
        egui::Button::new(text)
    };
    let response = ui.add_enabled(command.enabled.is_ok(), button);
    if commands::hints(response, &command).clicked() {
        commands::run(state, ui.ctx(), &command);
        return true;
    }
    false
}
