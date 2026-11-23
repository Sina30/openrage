//! Rows of one height inside a scroll area that holds other things too: each stands where a full
//! list would put it, and only the ones in view are drawn.

/// The widest any row has been, kept so the area's width does not follow the rows in view.
#[derive(Clone, Copy, Default)]
struct Widest {
    count: usize,
    width: f32,
}

/// The rows of `count` that `clip` shows, for rows `pitch` apart from `top` down.
fn in_view(count: usize, top: f32, pitch: f32, clip: egui::Rangef) -> std::ops::Range<usize> {
    let row =
        |y: f32, round: fn(f32) -> f32| (round((y - top) / pitch).max(0.0) as usize).min(count);
    let first = row(clip.min, f32::floor);
    first..row(clip.max, f32::ceil).max(first)
}

/// Draws rows `0..count` from the cursor down, calling `row` only for those in view; `reveal` is a row to scroll to.
///
/// ⚠ A row is one line: `row` draws left to right in a strip as tall as a button, and what it
/// draws taller overlaps the next.
pub fn show(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    count: usize,
    reveal: Option<usize>,
    mut row: impl FnMut(&mut egui::Ui, usize),
) {
    if count == 0 {
        return;
    }
    let height = ui.spacing().interact_size.y;
    let gap = ui.spacing().item_spacing.y;
    let pitch = height + gap;
    let top = ui.cursor().min;
    let right = ui.max_rect().right().max(top.x + 1.0);
    let strip = |index: usize| {
        let y = top.y + index as f32 * pitch;
        egui::Rect::from_min_max(egui::pos2(top.x, y), egui::pos2(right, y + height))
    };
    if let Some(index) = reveal.filter(|index| *index < count) {
        ui.scroll_to_rect(strip(index), Some(egui::Align::Center));
    }

    let id = ui.id().with(("virtual_rows", id_salt));
    let mut widest = ui
        .data(|d| d.get_temp::<Widest>(id))
        .filter(|kept| kept.count == count)
        .map_or(0.0, |kept| kept.width);
    for index in in_view(count, top.y, pitch, ui.clip_rect().y_range()) {
        let builder = egui::UiBuilder::new()
            .id_salt(("virtual_row", index))
            .max_rect(strip(index))
            .layout(egui::Layout::left_to_right(egui::Align::Center));
        let mut strip_ui = ui.new_child(builder);
        row(&mut strip_ui, index);
        widest = widest.max(strip_ui.min_rect().right() - top.x);
    }
    ui.data_mut(|d| {
        d.insert_temp(
            id,
            Widest {
                count,
                width: widest,
            },
        );
    });
    let size = egui::vec2(widest, count as f32 * pitch - gap);
    ui.allocate_rect(egui::Rect::from_min_size(top, size), egui::Sense::hover());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_rows_in_view_are_drawn_and_the_list_keeps_its_whole_height() {
        let ctx = egui::Context::default();
        let mut drawn = Vec::new();
        let mut content = 0.0;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 200.0),
                )),
                ..Default::default()
            },
            |ui| {
                let pitch = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                let out = egui::ScrollArea::vertical()
                    .scroll_offset(egui::vec2(0.0, 500.0 * pitch))
                    .show(ui, |ui| {
                        show(ui, "rows", 10_000, None, |ui, index| {
                            drawn.push(index);
                            ui.label(index.to_string());
                        });
                    });
                content = out.content_size.y / pitch;
            },
        );
        assert_eq!(drawn.first(), Some(&500), "{drawn:?}");
        assert!(drawn.len() < 20, "{} rows drawn", drawn.len());
        assert!(drawn.windows(2).all(|pair| pair[1] == pair[0] + 1));
        assert!((content - 10_000.0).abs() < 1.0, "{content} rows tall");
    }

    #[test]
    fn the_rows_in_view_are_none_past_the_end_and_all_of_a_short_list() {
        let clip = |min, max| egui::Rangef::new(min, max);
        assert_eq!(in_view(3, 0.0, 10.0, clip(0.0, 500.0)), 0..3);
        assert_eq!(in_view(3, 0.0, 10.0, clip(900.0, 950.0)), 3..3);
        assert_eq!(in_view(100, 0.0, 10.0, clip(-50.0, 25.0)), 0..3);
        assert_eq!(in_view(100, 100.0, 10.0, clip(0.0, 50.0)), 0..0);
    }
}
