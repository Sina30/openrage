//! What recovers an edit of the Wardrobe on screen: the card of a pack that does not read, the versions of a pack, and the word that another window holds it.
//!
//! ⚠ **Thin by design**, as the panels beside it are: the engine keeps the versions, locks the pack and restores; a
//! button here asks `state` for one of them.

use egui_phosphor::regular as icon;

use super::{action_button, WardrobeUi};
use crate::state::workbench::LastGood;
use crate::state::AppState;
use crate::ui::commands::Action;
use crate::ui::theme;

/// How many versions the garment panel lists.
const MOST_SHOWN: usize = 15;

/// The card a pack that does not read is drawn as: why, and the way back to the last version that did.
pub(super) fn unreadable(
    ui: &mut egui::Ui,
    state: &mut AppState,
    why: &str,
    good: Option<&LastGood>,
) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(
            egui::RichText::new(tr!("{icon} This pack could not be read", icon = icon::WARNING))
                .strong()
                .color(theme::ERROR),
        );
        ui.add(egui::Label::new(egui::RichText::new(why).small().monospace()).wrap());
        ui.add_space(theme::SPACING_TIGHT);
        let said = match good {
            Some(good) => tr!("The newest earlier version that reads is from {when}: {version} ({ago}).", when = crate::state::prefs::ago(good.at), version = good.label, ago = crate::state::front_door::counted(good.garments, "garment")),
            None => tr!("No earlier version of this pack reads, so there is nothing to restore; the file can be mended by hand.").to_string(),
        };
        ui.label(egui::RichText::new(said).color(theme::TEXT_SECONDARY));
        ui.add_space(theme::SPACING_TIGHT);
        ui.horizontal_wrapped(|ui| {
            action_button(ui, state, Action::RestoreLastGood, None, good.is_some());
            action_button(ui, state, Action::RevealPack, Some(tr!("Reveal").into()), false);
        });
        ui.label(
            egui::RichText::new(tr!("What is in wardrobe.json now is kept beside the versions, so a typo in it can still be mended by hand."))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    });
}

/// The word over a pack that another window holds, which this window reads and does not write, and the way to ask for it again.
pub(super) fn held_elsewhere(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(tr!(
                "Another OpenRage window is editing this pack, so it is read-only here."
            ))
            .color(theme::TEXT_SECONDARY),
        );
        if ui.button(tr!("Try again")).clicked() {
            state.try_pack_lock_again();
        }
    });
    ui.add_space(theme::SPACING_TIGHT);
}

/// The menu of the pack's earlier versions, each with the button that makes the whole pack what it was.
pub(super) fn versions_menu(ui: &mut egui::Ui, state: &mut AppState) {
    let mut picked = None;
    let button = ui.menu_button(tr!("Versions"), |ui| {
        state.load_versions();
        ui.set_min_width(300.0);
        if let Some(command) = crate::ui::commands::of(state, Action::ReviewVersions) {
            crate::ui::panels::menu::draw_command(ui, state, &command);
        }
        ui.separator();
        let current = state
            .wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .map(|read| &read.wardrobe);
        let Some(versions) = state.workbench.wardrobe.versions.as_ref() else {
            return;
        };
        if versions.list.is_empty() {
            ui.label(tr!("No earlier version is kept yet"));
            return;
        }
        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                for (version, pack) in &versions.list {
                    ui.horizontal(|ui| {
                        let line = format!(
                            "{} · {}",
                            crate::state::prefs::ago(version.at),
                            version.label
                        );
                        ui.add(egui::Label::new(egui::RichText::new(line).small()).truncate());
                        if Some(pack) == current {
                            ui.label(
                                egui::RichText::new(tr!("now"))
                                    .small()
                                    .color(theme::TEXT_SECONDARY),
                            );
                        } else {
                            let ago = crate::state::prefs::ago(version.at);
                            let restore = ui.small_button(tr!("Restore")).on_hover_text(tr!(
                                "Make the whole pack what it was then. Undo takes it back"
                            ));
                            restore.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    tr!("Restore the pack as it was {ago}", ago = ago),
                                )
                            });
                            if restore.clicked() {
                                picked = Some(version.n);
                            }
                        }
                    });
                }
            });
    });
    button
        .response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tr!("Versions")));
    button.response.on_hover_text(
        tr!("The versions of this pack kept beside it before each change: restore the whole pack, or one garment or texture from its panel. History lists the steps of the project instead"),
    );
    if let Some(n) = picked {
        state.restore_pack_version(n);
        ui.close();
    }
}

/// What the garment panel's list of earlier versions asked for.
pub(super) enum Rollback {
    Garment(String, u64),
    Texture(String, usize, u64),
}

/// The earlier versions in which garment `id` was not as it is, each with the buttons that put it, or one of its textures, back.
pub(super) fn garment_versions(
    ui: &mut egui::Ui,
    state: &AppState,
    shown: &mut WardrobeUi,
    id: &str,
    asked: &mut Option<Rollback>,
) {
    let caret = if shown.versions_open {
        icon::CARET_DOWN
    } else {
        icon::CARET_RIGHT
    };
    if ui
        .selectable_label(
            shown.versions_open,
            tr!("{caret} Earlier versions", caret = caret),
        )
        .on_hover_text(tr!(
            "The versions of the pack kept beside it, as far as this garment was different"
        ))
        .clicked()
    {
        shown.versions_open = !shown.versions_open;
    }
    if !shown.versions_open {
        return;
    }
    let (Some(versions), Some(now)) = (
        state.workbench.wardrobe.versions.as_ref(),
        state
            .wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .and_then(|read| read.wardrobe.garment(id)),
    ) else {
        ui.label(egui::RichText::new(tr!("Reading the versions…")).color(theme::TEXT_SECONDARY));
        return;
    };
    let mut shown_any = false;
    for (version, pack) in versions.list.iter().take(MOST_SHOWN) {
        let Some(then) = pack.garment(id).filter(|then| *then != now) else {
            continue;
        };
        shown_any = true;
        let ago = crate::state::prefs::ago(version.at);
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("{ago} · {}", version.label)).small());
            let all = ui.small_button(tr!("Restore garment"));
            all.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    tr!("Restore this garment as it was {ago}", ago = ago),
                )
            });
            if all.on_hover_text(tr!("Put this garment back as it was then; the others stay as they are. Undo takes it back")).clicked() {
                *asked = Some(Rollback::Garment(id.to_string(), version.n));
            }
            for (i, variation) in then.variations.iter().enumerate() {
                if now.variations.get(i) == Some(variation) {
                    continue;
                }
                let letter = char::from(b'a' + u8::try_from(i).unwrap_or(25).min(25));
                let one = ui.small_button(tr!("Restore texture {letter}", letter = letter));
                if one.on_hover_text(tr!("Put this texture back as it was then; the others stay as they are. Undo takes it back")).clicked() {
                    *asked = Some(Rollback::Texture(id.to_string(), i, version.n));
                }
            }
        });
    }
    if !shown_any {
        ui.label(
            egui::RichText::new(tr!("This garment is as it was in every version kept"))
                .small()
                .color(theme::TEXT_SECONDARY),
        );
    }
}
