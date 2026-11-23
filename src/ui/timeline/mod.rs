pub mod channels;
pub mod curve;
pub mod density;
pub mod keys;
pub mod provider;
pub mod ruler;
#[cfg(test)]
mod the_marks_of_the_timeline_read_at_3_to_1_on_their_rows;
pub mod time_map;
pub mod transport_bar;

use std::collections::BTreeSet;

use egui::accesskit::Role;
use egui::{Rect, Sense, Ui};
use or_transport::{TimeRange, Transport};

pub use channels::{ChannelFilter, RowStates};
pub use curve::CurveState;
pub use density::DensityGraph;
pub use provider::{
    ChannelRow, CurveChannel, CurveDomain, CurveHandle, CurveSeries, KeyEvent, KeyShape, RowBadge,
    RowEvents, RowId, RowKind, SpanEvent, TimelineEdit, TimelineSource,
};
pub use time_map::TimeMap;
pub use transport_bar::ScrubState;

use crate::ui::{accessible_name, theme};

#[derive(Debug, Clone, Default)]
pub struct TimelineState {
    view: Option<TimeRange>,
    pub filter: ChannelFilter,
    pub rows: RowStates,
    pub curves: CurveState,
    pub scrub: ScrubState,
    density: DensityGraph,
    pub selected_rows: BTreeSet<RowId>,
    pub show_curves: bool,
    events: RowEvents,
    subject: Option<u64>,
}

impl TimelineState {
    /// Drop everything derived from `subject`'s predecessor, if this state was
    /// last used for a different one. Call before drawing.
    ///
    /// ⚠ A `TimelineState` lives on `AppState` for the process lifetime, so
    /// without this guard filters, expansion, curve selection and
    /// `selected_rows` follow the user from one asset to the next; a stale
    /// [`RowId`] silently highlights an unrelated row.
    ///
    /// `subject` is an opaque `u64` rather than a `DocumentId` because one
    /// document holds many clips, whose row sets have nothing in common.
    pub fn ensure_subject(&mut self, subject: u64) {
        if self.subject == Some(subject) {
            return;
        }
        self.subject = Some(subject);
        self.view = None;
        self.rows.reset();
        self.selected_rows.clear();
        self.curves = CurveState::default();
        self.filter = ChannelFilter::default();
    }

    pub fn view(&self, bounds: TimeRange) -> TimeRange {
        self.view.unwrap_or(bounds)
    }
}

#[derive(Debug, Clone, Default)]
pub struct TimelineOutput {
    pub seek_to: Option<f32>,
    pub evaluate: bool,
    pub edits: Vec<TimelineEdit>,
    pub clicked_row: Option<RowId>,
}

pub fn draw<S: TimelineSource + ?Sized>(
    ui: &mut Ui,
    source: &S,
    transport: &mut Transport,
    state: &mut TimelineState,
) -> TimelineOutput {
    let mut output = TimelineOutput {
        evaluate: true,
        ..TimelineOutput::default()
    };

    let bounds = source.bounds();
    if transport.bounds != bounds {
        let had_narrow_range = transport.range != transport.bounds;
        transport.bounds = bounds;
        if !had_narrow_range {
            transport.range = bounds;
        } else {
            transport.set_range(transport.range);
        }
    }
    transport.frame_rate = source.frame_rate();
    let no_playback = source.playback_unavailable();
    if no_playback.is_some() {
        transport.playing = false;
    }

    let summary = source.summary();
    transport_bar::draw(
        ui,
        transport,
        if summary.is_empty() {
            None
        } else {
            Some(summary.as_str())
        },
        no_playback,
    );

    if let Some(reason) = no_playback {
        ui.label(egui::RichText::new(reason).small().color(theme::WARNING));
    }

    if !source.editable() {
        if let Some(reason) = source.read_only_reason() {
            ui.label(
                egui::RichText::new(tr!("Read-only: {reason}", reason = reason))
                    .small()
                    .color(theme::WARNING),
            );
        }
    }

    ui.separator();

    let rows = source.rows();
    let visible = channels::visible_rows(rows, &state.filter, &state.rows, &state.selected_rows);

    let available = ui.available_size();
    let curve_height = if state.show_curves && !source.curves().is_empty() {
        (available.y * 0.4).clamp(60.0, 220.0)
    } else {
        0.0
    };
    let body_height = (available.y - curve_height).max(40.0);

    let (body_rect, body_response) = ui.allocate_exact_size(
        egui::vec2(available.x, body_height),
        Sense::click_and_drag(),
    );
    let body_response = accessible_name::named(body_response, tr!("Timeline"));

    let channel_width = channels::CHANNEL_WIDTH.min(body_rect.width() * 0.45);
    let channel_rect = Rect::from_min_max(
        body_rect.min,
        egui::pos2(body_rect.min.x + channel_width, body_rect.max.y),
    );
    let track_rect = Rect::from_min_max(
        egui::pos2(channel_rect.max.x, body_rect.min.y),
        body_rect.max,
    );
    let ruler_rect = Rect::from_min_max(
        track_rect.min,
        egui::pos2(track_rect.max.x, track_rect.min.y + ruler::RULER_HEIGHT),
    );
    let tracks_rect = Rect::from_min_max(
        egui::pos2(track_rect.min.x, ruler_rect.max.y),
        track_rect.max,
    );

    let view = state.view(bounds);
    let mut map = TimeMap::new(view, track_rect.min.x, track_rect.max.x).clamped_to(bounds);
    map = transport_bar::handle_pan_zoom(ui, &body_response, &map, bounds);
    state.view = Some(map.view());

    ui.painter().rect_filled(body_rect, 0.0, theme::BG_L0);

    {
        let header = Rect::from_min_max(
            channel_rect.min,
            egui::pos2(channel_rect.max.x, ruler_rect.max.y),
        );
        ui.painter().rect_filled(header, 0.0, theme::BG_L1);
        let mut header_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(header.shrink2(egui::vec2(2.0, 1.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        channels::draw_filter_bar(&mut header_ui, &mut state.filter, visible.len(), rows.len());
    }

    let tick_step = ruler::draw(ui.painter(), ruler_rect, &map, source.frame_rate());
    ruler::draw_range_shading(ui.painter(), tracks_rect, &map, transport.range);
    ruler::draw_grid(ui.painter(), tracks_rect, &map, tick_step);

    let scroll_id = ui.id().with("timeline_scroll");
    let scroll_y: f32 = ui
        .ctx()
        .data_mut(|d| d.get_temp::<f32>(scroll_id))
        .unwrap_or(0.0);
    let range = channels::visible_range(visible.len(), scroll_y, tracks_rect.height());

    for (slot, &row_index) in visible[range.clone()].iter().enumerate() {
        let row = &rows[row_index];
        let y = tracks_rect.min.y + (range.start + slot) as f32 * channels::ROW_HEIGHT - scroll_y;
        if y > tracks_rect.max.y {
            break;
        }
        let row_rect = Rect::from_min_size(
            egui::pos2(tracks_rect.min.x, y),
            egui::vec2(tracks_rect.width(), channels::ROW_HEIGHT),
        );
        let label_rect = Rect::from_min_size(
            egui::pos2(channel_rect.min.x, y),
            egui::vec2(channel_rect.width(), channels::ROW_HEIGHT),
        );

        keys::draw_row_background(
            ui.painter(),
            row_rect,
            range.start + slot,
            row.kind == RowKind::Group,
        );

        state.events.clear();
        source.row_events(row_index, map.view(), &mut state.events);
        let colour = channels::event_colour(row, &state.rows);

        keys::draw_spans(ui.painter(), row_rect, &map, &state.events.spans);

        if DensityGraph::should_aggregate(state.events.total_keys, row_rect.width()) {
            let positions: Vec<f32> = state
                .events
                .keys
                .iter()
                .map(|k| map.time_to_x(k.time))
                .collect();
            state
                .density
                .paint(ui.painter(), row_rect, positions, colour);
            accessible_name::described(
                ui.ctx(),
                ui.id().with(("density", row.id)),
                row_rect,
                Role::Image,
                &format!(
                    "{}: {} keys, as a density graph",
                    row.label, state.events.total_keys
                ),
            );
        } else {
            keys::draw_keys(ui.painter(), row_rect, &map, &state.events.keys);
        }

        let is_selected = state.selected_rows.contains(&row.id);
        if let Some(action) =
            channels::draw_row_label(ui, label_rect, row, &state.rows, is_selected)
        {
            if let channels::RowAction::Clicked(id) = action {
                output.clicked_row = Some(id);
                if state.selected_rows.contains(&id) {
                    state.selected_rows.remove(&id);
                } else {
                    state.selected_rows.insert(id);
                }
            }
            channels::apply_action(&mut state.rows, action);
        }
    }

    if body_response.hovered() {
        let delta = ui.input(|i| i.smooth_scroll_delta.y);
        let modifier = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
        if delta != 0.0 && !modifier {
            let max_scroll =
                (channels::total_height(visible.len()) - tracks_rect.height()).max(0.0);
            let next = (scroll_y - delta).clamp(0.0, max_scroll);
            ui.ctx().data_mut(|d| d.insert_temp(scroll_id, next));
        }
    }

    ruler::draw_playhead(ui.painter(), tracks_rect, &map, transport.time);
    ruler::draw_playhead(ui.painter(), ruler_rect, &map, transport.time);

    let scrub = state.scrub.handle(ui, &body_response, &map, bounds);
    if let Some(time) = scrub.seek_to {
        output.seek_to = Some(time);
        output.evaluate = scrub.evaluate;
    }
    if let Some(range) = scrub.set_range {
        transport.set_range(range);
    }
    if scrub.reset_range {
        transport.reset_range();
    }

    // A right click that is no drag, the drag being what pans, offers by name what the small buttons of the bar do.
    body_response.context_menu(|ui| {
        let word = if transport.playing {
            tr!("Pause")
        } else {
            tr!("Play")
        };
        let play = ui.add_enabled(no_playback.is_none(), egui::Button::new(word));
        if play.clicked() {
            transport.toggle();
            ui.close();
        }
        if ui.button(tr!("Go to the start")).clicked() {
            output.seek_to = Some(transport.range.start);
            ui.close();
        }
        if ui.button(tr!("Show the whole length")).clicked() {
            state.view = None;
            transport.reset_range();
            ui.close();
        }
    });

    if curve_height > 0.0 {
        ui.separator();
        let series = source.curves();
        curve::draw_tabs(ui, &mut state.curves, series);
        let (curve_rect, curve_response) = curve::allocate(ui, curve_height - 24.0);
        if let Some(selected) = series.get(state.curves.selected) {
            let draw = curve::CurveDraw {
                rect: curve_rect,
                timeline_map: &map,
                series: selected,
                series_index: state.curves.selected,
                playhead: transport.time,
                editable: source.editable(),
            };
            if let Some(time) = curve::draw_series(
                ui,
                &draw,
                &mut state.curves,
                &curve_response,
                &mut output.edits,
            ) {
                output.seek_to = Some(time);
                output.evaluate = true;
            }
        }
    }

    transport_bar::handle_shortcuts(ui, transport, no_playback.is_none());

    output
}

pub fn draw_placeholder(ui: &mut Ui, message: &str) {
    theme::empty_message(ui, message);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_subject_drops_everything_derived_from_the_last_one() {
        let mut state = TimelineState {
            view: Some(TimeRange::new(1.0, 2.0)),
            ..TimelineState::default()
        };
        state.ensure_subject(1);
        state.view = Some(TimeRange::new(1.0, 2.0));
        state.selected_rows.insert(RowId::new(0, 1, 0));
        state.rows.toggle_hidden(RowId::new(0, 1, 0));
        state.filter.search = "hand".into();
        state.curves.selected = 3;

        state.ensure_subject(2);

        assert!(state.view.is_none());
        assert!(state.selected_rows.is_empty());
        assert!(!state.rows.is_hidden(RowId::new(0, 1, 0)));
        assert!(state.filter.search.is_empty());
        assert_eq!(state.curves.selected, 0);
        assert!(state.filter.only_animated);
    }

    #[test]
    fn the_same_subject_leaves_everything_alone() {
        let mut state = TimelineState::default();
        state.ensure_subject(7);
        state.filter.search = "hand".into();
        state.selected_rows.insert(RowId::new(0, 1, 0));

        for _ in 0..3 {
            state.ensure_subject(7);
        }

        assert_eq!(state.filter.search, "hand");
        assert!(state.selected_rows.contains(&RowId::new(0, 1, 0)));
    }
}
