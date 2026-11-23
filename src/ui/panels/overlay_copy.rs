//! The prompt that copies a shipped archive into the mods overlay, or refreshes a copy the game has outgrown; the copy's progress is in the job list.

use or_title::{Change, RefreshPlan};

use super::question;
use crate::state::overlay_copy::OverlayCopyOffer;
use crate::state::AppState;
use crate::ui::theme;

/// The most lines of a list the prompt draws before it says how many more there are.
const SHOWN: usize = 8;

pub fn draw(ctx: &egui::Context, state: &mut AppState) {
    let Some(offer) = state.overlay_copy.offer.clone() else {
        return;
    };
    let mut answer: Option<bool> = None;
    let title = match offer.replaces {
        Some(_) => tr!("Refresh the mods overlay copy?"),
        None => tr!("Copy to the mods overlay?"),
    };
    question(ctx, title, |ui| {
        ui.set_min_width(420.0);
        match &offer.replaces {
            Some(plan) => refresh_body(ui, &offer, plan),
            None => copy_body(ui, &offer),
        }
        ui.add_space(theme::SPACING_LOOSE);
        ui.horizontal(|ui| {
            let discards = offer.replaces.as_ref().is_some_and(|p| !p.yours.is_empty());
            let go = match (&offer.replaces, discards) {
                (None, _) => tr!("Copy"),
                (Some(_), false) => tr!("Refresh"),
                (Some(_), true) => tr!("Refresh and discard"),
            };
            if ui.button(go).clicked() {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if let Some(accept) = answer {
        state.answer_overlay_copy(accept);
    }
}

fn copy_body(ui: &mut egui::Ui, offer: &OverlayCopyOffer) {
    ui.label(
        egui::RichText::new(tr!(
            "This copies {size} into the overlay.",
            size = or_job::human_bytes(offer.bytes)
        ))
        .strong(),
    );
    ui.add_space(theme::SPACING_TIGHT);
    ui.label(
        egui::RichText::new(offer.source.display().to_string())
            .monospace()
            .color(theme::WARNING),
    );
    ui.label(tr!("to"));
    ui.label(egui::RichText::new(offer.dest.display().to_string()).monospace());
    ui.add_space(theme::SPACING_SNUG);
    ui.label(
        egui::RichText::new(tr!(
            "The shipped archive stays as it is. The overlay's copy is the \
             one OpenRage reads first and the one to edit."
        ))
        .weak(),
    );
}

fn refresh_body(ui: &mut egui::Ui, offer: &OverlayCopyOffer, plan: &RefreshPlan) {
    ui.label(
        egui::RichText::new(tr!(
            "The game was updated since this copy was made. Refreshing replaces it \
             with the patched archive, {size}.",
            size = or_job::human_bytes(offer.bytes)
        ))
        .strong(),
    );
    ui.add_space(theme::SPACING_TIGHT);
    ui.label(egui::RichText::new(offer.dest.display().to_string()).monospace());
    ui.add_space(theme::SPACING_SNUG);
    if plan.yours.is_empty() {
        ui.label(tr!(
            "No change of yours is in the copy: it differs from the patched archive only \
             where the update changed things."
        ));
    } else {
        ui.label(
            egui::RichText::new(tr!(
                "Refreshing discards {count} change(s) of yours in the copy:",
                count = plan.yours.len()
            ))
            .color(theme::WARNING),
        );
        egui::ScrollArea::vertical()
            .max_height(180.0)
            .show(ui, |ui| {
                for discard in &plan.yours {
                    let what = match discard.change {
                        Change::Edited => tr!("Edited"),
                        Change::Added => tr!("Added"),
                        Change::Removed => tr!("Removed"),
                    };
                    ui.label(egui::RichText::new(format!("{what}  {}", discard.path)).monospace());
                }
            });
    }
    if !plan.by_update.is_empty() {
        ui.add_space(theme::SPACING_SNUG);
        let shown: Vec<&str> = plan
            .by_update
            .iter()
            .take(SHOWN)
            .map(String::as_str)
            .collect();
        let more = plan.by_update.len().saturating_sub(SHOWN);
        ui.label(
            egui::RichText::new(tr!(
                "The update changed {changed} file(s) the copy still holds the old version of, and any edit of yours to one is replaced with it: {shown}",
                changed = plan.by_update.len(),
                shown = if more > 0 {
                    tr!("{shown} and {more} more", shown = shown.join(", "), more = more)
                } else {
                    shown.join(", ")
                }
            ))
            .weak(),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use or_title::Discard;

    use super::*;

    /// Every label the prompt draws over a few frames, as AccessKit describes them.
    fn drawn(state: &mut AppState) -> Vec<String> {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 900.0),
            )),
            ..Default::default()
        };
        let mut labels = Vec::new();
        for _ in 0..3 {
            let output = ctx.run_ui(input.clone(), |ui| draw(ui.ctx(), state));
            let nodes = output
                .platform_output
                .accesskit_update
                .map(|update| update.nodes)
                .unwrap_or_default();
            for (_, node) in nodes {
                labels.extend(node.label().or_else(|| node.value()).map(str::to_string));
            }
        }
        labels
    }

    fn refresh_offer(plan: RefreshPlan) -> OverlayCopyOffer {
        OverlayCopyOffer {
            source: PathBuf::from("GTAV/update/update.rpf"),
            dest: PathBuf::from("GTAV/mods/update/update.rpf"),
            bytes: 2048,
            replaces: Some(plan),
        }
    }

    #[test]
    fn a_refresh_that_discards_lists_each_change_before_it_asks() {
        let mut state = AppState::default();
        state.overlay_copy.offer = Some(refresh_offer(RefreshPlan {
            yours: vec![
                Discard::new("data/handling.meta", Change::Edited),
                Discard::new("data/mine.txt", Change::Added),
            ],
            by_update: vec!["data/version.txt".to_string()],
        }));

        let labels = drawn(&mut state);
        let says = |what: &str| labels.iter().any(|label| label.contains(what));
        assert!(says("Refresh the mods overlay copy?"), "{labels:?}");
        assert!(says("discards 2 change(s) of yours"), "{labels:?}");
        assert!(says("Edited  data/handling.meta"), "{labels:?}");
        assert!(says("Added  data/mine.txt"), "{labels:?}");
        assert!(
            says("data/version.txt"),
            "the update's own change is counted apart"
        );
        assert!(
            labels.iter().any(|l| l == "Refresh and discard"),
            "{labels:?}"
        );
    }

    #[test]
    fn a_refresh_with_nothing_of_the_users_to_lose_does_not_say_it_discards() {
        let mut state = AppState::default();
        state.overlay_copy.offer = Some(refresh_offer(RefreshPlan::default()));

        let labels = drawn(&mut state);
        assert!(labels.iter().any(|l| l == "Refresh"), "{labels:?}");
        assert!(
            !labels.iter().any(|l| l.contains("discard")),
            "nothing would be lost, so nothing says it would: {labels:?}"
        );
    }
}
