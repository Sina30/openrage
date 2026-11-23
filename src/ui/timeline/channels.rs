use std::collections::BTreeSet;

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui};

use super::provider::{ChannelRow, RowId, RowKind};
use crate::ui::theme;

pub const ROW_HEIGHT: f32 = 18.0;

pub const CHANNEL_WIDTH: f32 = 190.0;

const INDENT: f32 = 12.0;

#[derive(Debug, Clone, Default)]
pub struct RowStates {
    hidden: BTreeSet<RowId>,
    muted: BTreeSet<RowId>,
    locked: BTreeSet<RowId>,
    expanded: BTreeSet<RowId>,
    solo: Option<RowId>,
}

impl RowStates {
    pub fn is_hidden(&self, id: RowId) -> bool {
        if let Some(solo) = self.solo {
            return solo != id;
        }
        self.hidden.contains(&id)
    }

    pub fn is_muted(&self, id: RowId) -> bool {
        if let Some(solo) = self.solo {
            return solo != id;
        }
        self.muted.contains(&id)
    }

    pub fn is_locked(&self, id: RowId) -> bool {
        self.locked.contains(&id)
    }

    pub fn is_expanded(&self, id: RowId) -> bool {
        self.expanded.contains(&id)
    }

    pub fn toggle_hidden(&mut self, id: RowId) {
        toggle(&mut self.hidden, id);
    }

    pub fn toggle_muted(&mut self, id: RowId) {
        toggle(&mut self.muted, id);
    }

    pub fn toggle_locked(&mut self, id: RowId) {
        toggle(&mut self.locked, id);
    }

    pub fn toggle_expanded(&mut self, id: RowId) {
        toggle(&mut self.expanded, id);
    }

    pub fn toggle_solo(&mut self, id: RowId) {
        self.solo = if self.solo == Some(id) {
            None
        } else {
            Some(id)
        };
    }

    pub fn reset(&mut self) {
        self.hidden.clear();
        self.muted.clear();
        self.locked.clear();
        self.expanded.clear();
        self.solo = None;
    }
}

fn toggle(set: &mut BTreeSet<RowId>, id: RowId) {
    if !set.remove(&id) {
        set.insert(id);
    }
}

#[derive(Debug, Clone)]
pub struct ChannelFilter {
    pub search: String,
    pub invert: bool,
    pub only_animated: bool,
    pub only_selected: bool,
}

impl Default for ChannelFilter {
    fn default() -> Self {
        Self {
            search: String::new(),
            invert: false,
            only_animated: true,
            only_selected: false,
        }
    }
}

impl ChannelFilter {
    pub fn accepts(&self, row: &ChannelRow, selected: &BTreeSet<RowId>) -> bool {
        if self.only_animated && !row.animated {
            return false;
        }
        if self.only_selected && !selected.is_empty() && !selected.contains(&row.id) {
            return false;
        }
        self.matches_search(&row.label)
    }

    pub fn matches_search(&self, label: &str) -> bool {
        let needle = self.search.trim();
        if needle.is_empty() {
            return true;
        }
        let haystack = label.to_ascii_lowercase();
        let matched = needle
            .split_whitespace()
            .all(|term| haystack.contains(&term.to_ascii_lowercase()));
        matched != self.invert
    }
}

pub fn visible_rows(
    rows: &[ChannelRow],
    filter: &ChannelFilter,
    states: &RowStates,
    selected: &BTreeSet<RowId>,
) -> Vec<usize> {
    let mut out = Vec::new();
    let mut collapsed_below: Option<u8> = None;

    for (index, row) in rows.iter().enumerate() {
        if let Some(depth) = collapsed_below {
            if row.depth > depth {
                continue;
            }
            collapsed_below = None;
        }
        if row.expandable && !states.is_expanded(row.id) {
            collapsed_below = Some(row.depth);
        }
        if filter.accepts(row, selected) {
            out.push(index);
        }
    }
    out
}

pub fn visible_range(
    visible_count: usize,
    scroll_y: f32,
    viewport_height: f32,
) -> std::ops::Range<usize> {
    if visible_count == 0 || viewport_height <= 0.0 {
        return 0..0;
    }
    let first = ((scroll_y / ROW_HEIGHT).floor() as isize - 1).max(0) as usize;
    let count = (viewport_height / ROW_HEIGHT).ceil() as usize + 2;
    let end = (first + count).min(visible_count);
    first.min(end)..end
}

pub fn total_height(count: usize) -> f32 {
    count as f32 * ROW_HEIGHT
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RowAction {
    Clicked(RowId),
    ToggledExpanded(RowId),
    ToggledHidden(RowId),
    ToggledMuted(RowId),
    ToggledLocked(RowId),
    ToggledSolo(RowId),
}

pub fn draw_filter_bar(
    ui: &mut Ui,
    filter: &mut ChannelFilter,
    row_count: usize,
    total: usize,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("🔎");
        let response = ui.add(
            egui::TextEdit::singleline(&mut filter.search)
                .hint_text(tr!("Filter channels…"))
                .desired_width(120.0),
        );
        changed |= response.changed();

        if ui
            .selectable_label(filter.invert, "≠")
            .on_hover_text(tr!("Invert the filter"))
            .clicked()
        {
            filter.invert = !filter.invert;
            changed = true;
        }
        if ui
            .selectable_label(filter.only_animated, "◆")
            .on_hover_text(tr!("Only channels with animation"))
            .clicked()
        {
            filter.only_animated = !filter.only_animated;
            changed = true;
        }
        if ui
            .selectable_label(filter.only_selected, "⊙")
            .on_hover_text(tr!("Only the current selection"))
            .clicked()
        {
            filter.only_selected = !filter.only_selected;
            changed = true;
        }

        if row_count != total {
            ui.label(
                egui::RichText::new(format!("{row_count}/{total}"))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        } else {
            ui.label(
                egui::RichText::new(format!("{total} channels"))
                    .small()
                    .color(theme::TEXT_SECONDARY),
            );
        }
    });
    changed
}

/// A click target the row paints itself, which AccessKit describes as a button called `name`.
fn button(ui: &mut Ui, rect: Rect, name: &str) -> egui::Response {
    let response = ui.allocate_rect(rect, Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    response
}

pub fn draw_row_label(
    ui: &mut Ui,
    rect: Rect,
    row: &ChannelRow,
    states: &RowStates,
    is_selected: bool,
) -> Option<RowAction> {
    let painter = ui.painter().clone();
    if is_selected {
        painter.rect_filled(rect, 0.0, theme::SELECTION_BG);
    } else if row.depth == 0 {
        painter.rect_filled(rect, 0.0, theme::BG_L2);
    }

    let mut action = None;
    let mut x = rect.min.x + 2.0 + row.depth as f32 * INDENT;

    if row.expandable {
        let glyph_rect =
            Rect::from_min_size(egui::pos2(x, rect.min.y), egui::vec2(12.0, rect.height()));
        let expanded = states.is_expanded(row.id);
        painter.text(
            glyph_rect.center(),
            Align2::CENTER_CENTER,
            if expanded { "▾" } else { "▸" },
            FontId::proportional(theme::SMALL_TEXT),
            theme::TEXT_SECONDARY.get(),
        );
        let name = format!(
            "{} {}",
            if expanded {
                tr!("Collapse")
            } else {
                tr!("Expand")
            },
            row.label
        );
        if button(ui, glyph_rect, &name).clicked() {
            action = Some(RowAction::ToggledExpanded(row.id));
        }
        x += 12.0;
    } else {
        x += 12.0;
    }

    // ⚠ A hidden or muted channel's label cannot be dimmed and stay legible, so a line through it says so.
    let dimmed = states.is_hidden(row.id) || states.is_muted(row.id);
    let colour = if row.kind == RowKind::Group && !dimmed {
        theme::TEXT_PRIMARY.get()
    } else {
        theme::TEXT_SECONDARY.get()
    };
    let font = if row.kind == RowKind::Group {
        FontId::proportional(11.0)
    } else {
        FontId::monospace(10.0)
    };

    const TOGGLES: f32 = 42.0;
    let label_width = (rect.max.x - TOGGLES - x).max(0.0);
    let label_rect = Rect::from_min_size(
        egui::pos2(x, rect.min.y),
        egui::vec2(label_width, rect.height()),
    );
    let galley = label_galley(&painter, &row.label, font, colour, label_width);
    let drawn = Align2::LEFT_CENTER.anchor_size(
        egui::pos2(label_rect.min.x, label_rect.center().y),
        galley.size(),
    );
    let label_end = drawn.max.x;
    painter.galley(drawn.min, galley, colour);
    if dimmed {
        painter.line_segment(
            [drawn.left_center(), drawn.right_center()],
            Stroke::new(1.0_f32, colour),
        );
    }

    if let Some(badge) = &row.badge {
        let badge_x = label_end + 6.0;
        if badge_x < label_rect.max.x {
            // ⚠ The badge's colour tells the encodings apart and is a pastel that does not read on every panel; its
            // text is that colour moved until it does, and it is clipped so that it never runs under the toggles.
            painter.with_clip_rect(label_rect).text(
                egui::pos2(badge_x, label_rect.center().y),
                Align2::LEFT_CENTER,
                &badge.text,
                FontId::monospace(theme::SMALL_TEXT),
                theme::legible(badge.colour, theme::AA),
            );
        }
    }

    let label_response = button(ui, label_rect, &row.label);
    if label_response.clicked() {
        action = Some(RowAction::Clicked(row.id));
    }
    if let Some(badge) = &row.badge {
        label_response.on_hover_text(&badge.tooltip);
    }

    let mut toggle_x = rect.max.x - TOGGLES;
    for (glyph, active, kind, verb, tooltip) in [
        (
            "👁",
            states.is_hidden(row.id),
            0u8,
            tr!("Hide"),
            tr!("Hide (ctrl-click to solo)"),
        ),
        ("🔇", states.is_muted(row.id), 1, tr!("Mute"), tr!("Mute")),
        ("🔒", states.is_locked(row.id), 2, tr!("Lock"), tr!("Lock")),
    ] {
        let glyph_rect = Rect::from_min_size(
            egui::pos2(toggle_x, rect.min.y),
            egui::vec2(14.0, rect.height()),
        );
        // A toggle that is on has a box round it, so that colour is not all that says so.
        if active {
            let plate = glyph_rect.shrink2(egui::vec2(0.0, 2.0));
            painter.rect_filled(plate, 3.0, theme::SELECTION_BG);
            painter.rect_stroke(
                plate,
                3.0,
                Stroke::new(1.0_f32, theme::ACCENT_TEXT),
                egui::StrokeKind::Inside,
            );
        }
        painter.text(
            glyph_rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(theme::SMALL_TEXT),
            if active {
                theme::ACCENT_TEXT.get()
            } else {
                theme::TEXT_SECONDARY.get()
            },
        );
        let response = button(ui, glyph_rect, &format!("{verb} {}", row.label));
        if response.clicked() {
            let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
            action = Some(match (kind, ctrl) {
                (0, true) => RowAction::ToggledSolo(row.id),
                (0, false) => RowAction::ToggledHidden(row.id),
                (1, _) => RowAction::ToggledMuted(row.id),
                _ => RowAction::ToggledLocked(row.id),
            });
        }
        response.on_hover_text(tooltip);
        toggle_x += 14.0;
    }

    action
}

pub fn apply_action(states: &mut RowStates, action: RowAction) {
    match action {
        RowAction::ToggledExpanded(id) => states.toggle_expanded(id),
        RowAction::ToggledHidden(id) => states.toggle_hidden(id),
        RowAction::ToggledMuted(id) => states.toggle_muted(id),
        RowAction::ToggledLocked(id) => states.toggle_locked(id),
        RowAction::ToggledSolo(id) => states.toggle_solo(id),
        RowAction::Clicked(_) => {}
    }
}

/// `text` laid out in `font` on one row, cut with an ellipsis where it passes `width`.
// ⚠ Measured with the font it is painted in: a fixed width per character undercounts a
// monospace font, and the label then runs under the toggles.
fn label_galley(
    painter: &egui::Painter,
    text: &str,
    font: FontId,
    colour: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(font, colour),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    painter.layout_job(job)
}

pub fn event_colour(row: &ChannelRow, states: &RowStates) -> Color32 {
    let colour = theme::graphic(row.colour);
    if states.is_muted(row.id) || states.is_hidden(row.id) {
        colour.gamma_multiply(0.3)
    } else {
        colour
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u32, depth: u8, label: &str, animated: bool) -> ChannelRow {
        ChannelRow::group(RowId::new(0, id, 0), label)
            .with_depth(depth)
            .animated(animated)
    }

    #[test]
    fn search_splits_on_whitespace_and_requires_every_term() {
        let mut filter = ChannelFilter {
            search: "l hand".to_string(),
            ..ChannelFilter::default()
        };
        assert!(filter.matches_search("SKEL_L_Hand"));
        assert!(filter.matches_search("hand_left"));
        assert!(!filter.matches_search("SKEL_R_Foot"));
        filter.search = "hand l".to_string();
        assert!(filter.matches_search("SKEL_L_Hand"));
    }

    #[test]
    fn search_is_case_insensitive_and_empty_matches_everything() {
        let mut filter = ChannelFilter {
            search: "SKEL".to_string(),
            ..ChannelFilter::default()
        };
        assert!(filter.matches_search("skel_pelvis"));
        filter.search = "   ".to_string();
        assert!(filter.matches_search("anything"));
    }

    #[test]
    fn inverting_the_search_flips_the_result() {
        let mut filter = ChannelFilter {
            search: "hand".to_string(),
            invert: true,
            ..ChannelFilter::default()
        };
        assert!(!filter.matches_search("SKEL_L_Hand"));
        assert!(filter.matches_search("SKEL_R_Foot"));
        filter.search = String::new();
        assert!(filter.matches_search("SKEL_L_Hand"));
    }

    #[test]
    fn only_animated_is_on_by_default_and_hides_static_rows() {
        let filter = ChannelFilter::default();
        assert!(filter.only_animated);
        let selected = BTreeSet::new();
        assert!(filter.accepts(&row(0, 0, "moving", true), &selected));
        assert!(!filter.accepts(&row(1, 0, "static", false), &selected));
    }

    #[test]
    fn only_selected_is_ignored_when_nothing_is_selected() {
        let filter = ChannelFilter {
            only_selected: true,
            ..ChannelFilter::default()
        };
        let empty = BTreeSet::new();
        assert!(filter.accepts(&row(0, 0, "bone", true), &empty));

        let mut selected = BTreeSet::new();
        selected.insert(RowId::new(0, 5, 0));
        assert!(!filter.accepts(&row(0, 0, "bone", true), &selected));
        assert!(filter.accepts(&row(5, 0, "bone", true), &selected));
    }

    #[test]
    fn collapsed_groups_hide_their_descendants() {
        let rows = vec![
            row(0, 0, "bone_a", true).expandable(true, false),
            row(1, 1, "translation", true),
            row(2, 1, "rotation", true),
            row(3, 0, "bone_b", true),
        ];
        let filter = ChannelFilter::default();
        let mut states = RowStates::default();
        let selected = BTreeSet::new();

        let visible = visible_rows(&rows, &filter, &states, &selected);
        assert_eq!(visible, vec![0, 3]);

        states.toggle_expanded(rows[0].id);
        let visible = visible_rows(&rows, &filter, &states, &selected);
        assert_eq!(visible, vec![0, 1, 2, 3]);
    }

    #[test]
    fn nested_collapse_hides_grandchildren() {
        let rows = vec![
            row(0, 0, "bone", true).expandable(true, false),
            row(1, 1, "translation", true).expandable(true, false),
            row(2, 2, "x", true),
            row(3, 0, "other", true),
        ];
        let filter = ChannelFilter::default();
        let mut states = RowStates::default();
        let selected = BTreeSet::new();

        states.toggle_expanded(rows[0].id);
        let visible = visible_rows(&rows, &filter, &states, &selected);
        assert_eq!(visible, vec![0, 1, 3]);

        states.toggle_expanded(rows[1].id);
        let visible = visible_rows(&rows, &filter, &states, &selected);
        assert_eq!(visible, vec![0, 1, 2, 3]);
    }

    #[test]
    fn the_virtual_range_covers_the_viewport_with_padding() {
        let range = visible_range(100, 20.0 * ROW_HEIGHT, 10.0 * ROW_HEIGHT);
        assert!(
            range.start <= 20,
            "start {} should include row 20",
            range.start
        );
        assert!(range.end >= 30, "end {} should include row 30", range.end);
        assert!(range.len() < 20, "range {range:?} is not virtualized");
    }

    #[test]
    fn the_virtual_range_is_clamped_to_the_row_count() {
        let range = visible_range(5, 0.0, 1000.0);
        assert_eq!(range, 0..5);
        let range = visible_range(5, 10_000.0, 100.0);
        assert!(range.start <= range.end);
        assert!(range.end <= 5);
    }

    #[test]
    fn the_virtual_range_of_nothing_is_empty() {
        assert_eq!(visible_range(0, 0.0, 100.0), 0..0);
        assert_eq!(visible_range(10, 0.0, 0.0), 0..0);
    }

    #[test]
    fn painted_row_count_is_independent_of_list_length() {
        let viewport = 20.0 * ROW_HEIGHT;
        let small = visible_range(50, 0.0, viewport).len();
        let huge = visible_range(50_000, 0.0, viewport).len();
        assert_eq!(small, huge);
    }

    #[test]
    fn toggles_flip_and_survive_independently() {
        let mut states = RowStates::default();
        let a = RowId::new(0, 1, 0);
        let b = RowId::new(0, 2, 0);

        states.toggle_hidden(a);
        assert!(states.is_hidden(a));
        assert!(!states.is_hidden(b));
        states.toggle_hidden(a);
        assert!(!states.is_hidden(a));

        states.toggle_muted(a);
        states.toggle_locked(b);
        assert!(states.is_muted(a));
        assert!(!states.is_muted(b));
        assert!(states.is_locked(b));
        assert!(!states.is_locked(a));
    }

    #[test]
    fn solo_hides_everything_else_and_overrides_hidden() {
        let mut states = RowStates::default();
        let a = RowId::new(0, 1, 0);
        let b = RowId::new(0, 2, 0);

        states.toggle_hidden(a);
        states.toggle_solo(a);
        assert!(!states.is_hidden(a));
        assert!(states.is_hidden(b));
        assert!(!states.is_muted(a));
        assert!(states.is_muted(b));

        states.toggle_solo(a);
        assert!(states.is_hidden(a));
    }

    #[test]
    fn soloing_a_second_row_moves_the_solo() {
        let mut states = RowStates::default();
        let a = RowId::new(0, 1, 0);
        let b = RowId::new(0, 2, 0);
        states.toggle_solo(a);
        states.toggle_solo(b);
        assert!(states.is_hidden(a));
        assert!(!states.is_hidden(b));
    }

    #[test]
    fn actions_apply_to_the_matching_state() {
        let mut states = RowStates::default();
        let id = RowId::new(0, 7, 0);
        let other = RowId::new(0, 8, 0);
        apply_action(&mut states, RowAction::ToggledExpanded(id));
        assert!(states.is_expanded(id));
        apply_action(&mut states, RowAction::ToggledSolo(id));
        assert!(!states.is_hidden(id));
        assert!(states.is_hidden(other));
        apply_action(&mut states, RowAction::Clicked(id));
        assert!(!states.is_hidden(id));
        assert!(states.is_hidden(other));
    }

    #[test]
    fn a_label_is_cut_to_its_width_in_the_font_it_is_painted_in() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter();
            let label = "moverAbsoluteTranslation · hash_0123ABCD = 1.000, 2.000, 3.000";
            for font in [FontId::monospace(10.0), FontId::proportional(11.0)] {
                for width in [40.0, 90.0, 150.0] {
                    let galley = label_galley(painter, label, font.clone(), Color32::WHITE, width);
                    assert!(
                        galley.size().x <= width + 0.5,
                        "{font:?} at {width}: {}",
                        galley.size().x
                    );
                    assert!(galley.elided);
                }
                let whole = label_galley(painter, "SKEL_Head", font, Color32::WHITE, 1000.0);
                assert!(!whole.elided);
            }
        });
    }

    #[test]
    fn muted_rows_paint_their_events_dimmed() {
        let mut states = RowStates::default();
        let r = row(1, 0, "bone", true).with_colour(Color32::from_rgb(200, 100, 50));
        let bright = event_colour(&r, &states);
        states.toggle_muted(r.id);
        let dim = event_colour(&r, &states);
        assert_ne!(bright, dim);
    }
}
