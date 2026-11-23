//! The scanned Map screen's edits of what the map holds: the commands that add and take out an
//! entity or an archetype, and the form for an archetype authored from the GTA VI drawable open.

use crate::state::AppState;
use crate::ui::accessible_name::named;
use crate::ui::commands::Action;
use crate::ui::theme;

/// The buttons of the commands that add and take out an entity or an archetype.
pub(super) fn buttons(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal_wrapped(|ui| {
        for action in [
            Action::AddEntity,
            Action::RemoveEntity,
            Action::AddArchetype,
            Action::RemoveArchetype,
        ] {
            super::action_button(ui, state, action, false);
        }
    });
}

/// What the form holds between frames, as typed.
#[derive(Debug, Clone)]
struct Form {
    name: String,
    lod_dist: String,
    guid: String,
    flags: String,
}

/// A number typed in decimal, or in hexadecimal after `0x`.
fn number(text: &str) -> Option<u64> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// The values typed, once each is a number of its kind: the name, `lodDist`, `guid` and the flags.
fn asked(form: &Form) -> Option<(String, f32, u64, u32)> {
    let lod_dist =
        (form.lod_dist.trim().parse::<f32>().ok()).filter(|v| v.is_finite() && *v >= 0.0)?;
    let flags = number(&form.flags).and_then(|f| u32::try_from(f).ok())?;
    Some((form.name.clone(), lod_dist, number(&form.guid)?, flags))
}

/// The form, where the entity selected has its archetype in an open map types body; what it asks
/// once Add is clicked.
pub(super) fn draw(ui: &mut egui::Ui, state: &AppState) -> Option<(String, f32, u64, u32)> {
    state.scanned_archetype_target()?;
    let id = egui::Id::new("gta6_archetype_from_a_drawable");
    let mut wanted = None;
    egui::CollapsingHeader::new(tr!("New archetype from a drawable"))
        .id_salt(id)
        .show(ui, |ui| {
            let Some((drawable, _, _)) = state.open_gta6_drawable() else {
                let open = tr!("No GTA VI drawable is open to make an archetype of.");
                ui.label(egui::RichText::new(open).color(theme::TEXT_SECONDARY));
                return;
            };
            let mut form = (ui.data_mut(|d| d.get_temp::<Form>(id))).unwrap_or(Form {
                name: drawable,
                lod_dist: "100".to_string(),
                guid: "0".to_string(),
                flags: "0".to_string(),
            });
            for (label, text) in [
                (tr!("Name"), &mut form.name),
                ("lodDist", &mut form.lod_dist),
                ("guid", &mut form.guid),
                ("flags", &mut form.flags),
            ] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(label).color(theme::TEXT_SECONDARY));
                    named(ui.text_edit_singleline(text), label);
                });
            }
            match asked(&form) {
                Some(values) => {
                    let add = tr!("Add the archetype");
                    let hover = tr!("Declares in the map types body of this entity's archetype an archetype of the drawable open, its names from the text, its box the drawable's, a seed to check, and its sphere the box's. Undo takes it back");
                    if named(ui.button(add), add).on_hover_text(hover).clicked() {
                        wanted = Some(values);
                    }
                }
                None => {
                    let numbers = tr!("lodDist, guid and flags take numbers");
                    ui.label(egui::RichText::new(numbers).color(theme::TEXT_SECONDARY));
                }
            }
            ui.data_mut(|d| d.insert_temp(id, form));
        });
    wanted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(lod_dist: &str, guid: &str, flags: &str) -> Form {
        Form {
            name: "prop_bench".to_string(),
            lod_dist: lod_dist.to_string(),
            guid: guid.to_string(),
            flags: flags.to_string(),
        }
    }

    #[test]
    fn the_form_asks_once_each_field_holds_a_number_of_its_kind() {
        let want: Option<(String, f32, u64, u32)> =
            Some(("prop_bench".to_string(), 120.5, 0xAB, 32));
        assert_eq!(asked(&form(" 120.5", "0xAB", "32")), want);
        assert_eq!(asked(&form("-1", "1", "0")), None, "a distance below 0");
        assert_eq!(asked(&form("NaN", "1", "0")), None);
        assert_eq!(asked(&form("1", "x", "0")), None);
        assert_eq!(
            asked(&form("1", "1", "0x100000000")),
            None,
            "flags past 32 bits"
        );
    }
}
