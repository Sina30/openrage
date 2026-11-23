//! The size preview a defragment from the archive menu asks before it writes; its progress is in the job list.

use crate::state::AppState;
use crate::ui::panels::question;
use crate::ui::theme;

/// The size preview that asks before a defragment writes.
pub(super) fn draw_defrag(ctx: &egui::Context, state: &mut AppState) {
    let Some(offer) = &state.defrag.offer else {
        return;
    };
    let (before, after) = (offer.before, offer.after);
    let path = offer.path.display().to_string();
    let mut answer: Option<bool> = None;
    question(ctx, tr!("Defragment?"), |ui| {
        ui.set_min_width(420.0);
        ui.label(egui::RichText::new(path).monospace());
        ui.add_space(theme::SPACING_TIGHT);
        ui.label(tr!(
            "Now {size} ({before} bytes), after {size_2} ({after} bytes).",
            before = before,
            after = after,
            size = or_job::human_bytes(before),
            size_2 = or_job::human_bytes(after)
        ));
        let verdict = match after.cmp(&before) {
            std::cmp::Ordering::Less => egui::RichText::new(tr!(
                "Reclaims {size}.",
                size = or_job::human_bytes(before - after)
            ))
            .strong(),
            std::cmp::Ordering::Equal => egui::RichText::new(tr!("Nothing to reclaim.")).weak(),
            std::cmp::Ordering::Greater => egui::RichText::new(tr!(
                "⚠ The rebuild is larger. That is not slack: an entry's \
                 compression did not survive the round trip."
            ))
            .color(theme::WARNING),
        };
        ui.label(verdict);
        ui.add_space(theme::SPACING_SNUG);
        ui.label(
            egui::RichText::new(tr!(
                "The first write keeps the file as it was beside it as .rpf.bak; \
                 a later one leaves that .bak alone."
            ))
            .weak(),
        );
        ui.add_space(theme::SPACING_LOOSE);
        ui.horizontal(|ui| {
            if ui.button(tr!("Defragment")).clicked() {
                answer = Some(true);
            }
            if ui.button(tr!("Cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if let Some(accept) = answer {
        state.answer_defrag(accept);
    }
}
