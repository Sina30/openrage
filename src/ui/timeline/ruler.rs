use egui::{Align2, Color32, FontId, Painter, Rect, Shape, Stroke};
use or_transport::{TimeRange, Transport};

use super::time_map::TimeMap;
use crate::ui::theme;

pub const RULER_HEIGHT: f32 = 22.0;

const MIN_LABEL_SPACING: f32 = 64.0;

const MIN_MINOR_SPACING: f32 = 8.0;

pub fn tick_step(seconds_per_pixel: f32, min_spacing: f32) -> f32 {
    if !(seconds_per_pixel.is_finite() && seconds_per_pixel > 0.0) {
        return 1.0;
    }
    let target = seconds_per_pixel * min_spacing;
    for exponent in -4..=6 {
        let decade = 10f32.powi(exponent);
        for multiplier in [1.0, 2.0, 5.0] {
            let step = decade * multiplier;
            if step >= target {
                return step;
            }
        }
    }
    target.max(1e-4)
}

pub fn frame_tick_step(seconds_per_pixel: f32, frame_rate: f32, min_spacing: f32) -> f32 {
    if !(frame_rate.is_finite() && frame_rate > 0.0) {
        return tick_step(seconds_per_pixel, min_spacing);
    }
    let frames_per_pixel = seconds_per_pixel * frame_rate;
    let target_frames = (frames_per_pixel * min_spacing).max(1.0);
    for exponent in 0..=6 {
        let decade = 10f32.powi(exponent);
        for multiplier in [1.0, 2.0, 5.0] {
            let frames = decade * multiplier;
            if frames >= target_frames {
                return frames / frame_rate;
            }
        }
    }
    target_frames / frame_rate
}

pub fn format_tick(time: f32, step: f32, frame_rate: Option<f32>) -> String {
    if let Some(fps) = frame_rate.filter(|f| *f > 0.0) {
        let frames = time * fps;
        if (step * fps - (step * fps).round()).abs() < 1e-3 {
            return format!("{}", frames.round() as i64);
        }
    }
    if step >= 1.0 {
        format!("{:.0}s", time)
    } else if step >= 0.1 {
        format!("{:.1}s", time)
    } else if step >= 0.01 {
        format!("{:.2}s", time)
    } else {
        format!("{:.3}s", time)
    }
}

pub fn format_time(transport: &Transport) -> String {
    let seconds = transport.time;
    let percent = transport.ratio() * 100.0;
    match (transport.frame(), transport.frame_count()) {
        (Some(frame), Some(total)) if total >= 1.0 => format!(
            "{seconds:.3}s · {percent:.0}% · frame {:.0}/{:.0}",
            frame, total
        ),
        _ => format!("{seconds:.3}s · {percent:.0}%"),
    }
}

pub fn ticks_in(view: TimeRange, step: f32) -> impl Iterator<Item = f32> {
    let step = if step > 0.0 { step } else { 1.0 };
    let first = (view.start / step).ceil() * step;
    let count = if view.span() > 0.0 {
        ((view.span() / step).floor() as usize).saturating_add(2)
    } else {
        0
    };
    (0..count)
        .map(move |i| first + i as f32 * step)
        .take_while(move |t| *t <= view.end + step * 1e-3)
}

pub fn draw(painter: &Painter, rect: Rect, map: &TimeMap, frame_rate: Option<f32>) -> f32 {
    painter.rect_filled(rect, 0.0, theme::BG_L1);

    let seconds_per_pixel = map.seconds_per_pixel();
    let major = match frame_rate {
        Some(fps) => frame_tick_step(seconds_per_pixel, fps, MIN_LABEL_SPACING),
        None => tick_step(seconds_per_pixel, MIN_LABEL_SPACING),
    };
    let minor = match frame_rate {
        Some(fps) => frame_tick_step(seconds_per_pixel, fps, MIN_MINOR_SPACING),
        None => tick_step(seconds_per_pixel, MIN_MINOR_SPACING),
    };

    if minor < major {
        for time in ticks_in(map.view(), minor) {
            let x = map.time_to_x(time);
            painter.line_segment(
                [egui::pos2(x, rect.max.y - 4.0), egui::pos2(x, rect.max.y)],
                Stroke::new(1.0_f32, theme::BORDER.gamma_multiply(0.5)),
            );
        }
    }

    for time in ticks_in(map.view(), major) {
        let x = map.time_to_x(time);
        painter.line_segment(
            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
            Stroke::new(1.0_f32, theme::BORDER),
        );
        painter.text(
            egui::pos2(x + 3.0, rect.min.y + 1.0),
            Align2::LEFT_TOP,
            format_tick(time, major, frame_rate),
            FontId::monospace(10.0),
            theme::TEXT_SECONDARY.get(),
        );
    }

    major
}

pub fn draw_grid(painter: &Painter, rect: Rect, map: &TimeMap, step: f32) {
    for time in ticks_in(map.view(), step) {
        let x = map.time_to_x(time);
        if x < rect.min.x || x > rect.max.x {
            continue;
        }
        painter.line_segment(
            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
            Stroke::new(1.0_f32, theme::BORDER.gamma_multiply(0.25)),
        );
    }
}

pub fn draw_playhead(painter: &Painter, rect: Rect, map: &TimeMap, time: f32) {
    if !map.is_visible(time) {
        return;
    }
    let x = map.time_to_x(time);
    painter.line_segment(
        [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
        Stroke::new(2.0_f32, theme::graphic(PLAYHEAD)),
    );
    const HANDLE: f32 = 8.0;
    painter.add(Shape::convex_polygon(
        vec![
            egui::pos2(x - HANDLE / 2.0, rect.min.y),
            egui::pos2(x + HANDLE / 2.0, rect.min.y),
            egui::pos2(x, rect.min.y + HANDLE),
        ],
        theme::graphic(PLAYHEAD),
        Stroke::NONE,
    ));
}

pub fn draw_range_shading(painter: &Painter, rect: Rect, map: &TimeMap, range: TimeRange) {
    let dim = Color32::from_black_alpha(90);
    let start_x = map.time_to_x(range.start).clamp(rect.min.x, rect.max.x);
    let end_x = map.time_to_x(range.end).clamp(rect.min.x, rect.max.x);
    if start_x > rect.min.x {
        painter.rect_filled(
            Rect::from_min_max(rect.min, egui::pos2(start_x, rect.max.y)),
            0.0,
            dim,
        );
    }
    if end_x < rect.max.x {
        painter.rect_filled(
            Rect::from_min_max(egui::pos2(end_x, rect.min.y), rect.max),
            0.0,
            dim,
        );
    }
}

pub const PLAYHEAD: Color32 = Color32::from_rgb(0xE5, 0x3E, 0x3E);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_steps_come_from_the_one_two_five_ladder() {
        for spp in [1e-4, 1e-3, 0.01, 0.05, 0.2, 1.0, 7.5] {
            let step = tick_step(spp, 64.0);
            let decade = 10f32.powf(step.log10().floor());
            let multiplier = step / decade;
            assert!(
                [1.0, 2.0, 5.0]
                    .iter()
                    .any(|m| (multiplier - m).abs() < 1e-3),
                "spp {spp} gave step {step} (multiplier {multiplier})"
            );
        }
    }

    #[test]
    fn tick_step_clears_the_minimum_spacing() {
        for spp in [1e-4, 1e-3, 0.01, 0.1, 1.0] {
            let step = tick_step(spp, 64.0);
            let spacing_px = step / spp;
            assert!(
                spacing_px >= 64.0,
                "spp {spp}: step {step} gives only {spacing_px} px"
            );
        }
    }

    #[test]
    fn zooming_in_shrinks_the_tick_step() {
        let wide = tick_step(0.1, 64.0);
        let narrow = tick_step(0.001, 64.0);
        assert!(narrow < wide, "{narrow} should be finer than {wide}");
    }

    #[test]
    fn degenerate_scales_give_a_usable_step() {
        assert_eq!(tick_step(0.0, 64.0), 1.0);
        assert_eq!(tick_step(-1.0, 64.0), 1.0);
        assert_eq!(tick_step(f32::NAN, 64.0), 1.0);
        assert!(tick_step(f32::INFINITY, 64.0) > 0.0);
    }

    #[test]
    fn frame_ticks_land_on_whole_frames() {
        let fps = 30.0;
        for spp in [0.001, 0.01, 0.05, 0.2] {
            let step = frame_tick_step(spp, fps, 64.0);
            let frames = step * fps;
            assert!(
                (frames - frames.round()).abs() < 1e-3,
                "spp {spp} gave {frames} frames per tick"
            );
            assert!(frames >= 1.0);
        }
    }

    #[test]
    fn frame_ticks_fall_back_without_a_frame_rate() {
        assert_eq!(frame_tick_step(0.01, 0.0, 64.0), tick_step(0.01, 64.0));
        assert_eq!(frame_tick_step(0.01, f32::NAN, 64.0), tick_step(0.01, 64.0));
    }

    #[test]
    fn tick_labels_use_precision_matched_to_the_step() {
        assert_eq!(format_tick(10.0, 5.0, None), "10s");
        assert_eq!(format_tick(1.5, 0.5, None), "1.5s");
        assert_eq!(format_tick(1.25, 0.05, None), "1.25s");
        assert_eq!(format_tick(1.125, 0.005, None), "1.125s");
    }

    #[test]
    fn whole_frame_steps_label_as_frame_numbers() {
        assert_eq!(format_tick(1.0, 1.0 / 30.0, Some(30.0)), "30");
        assert_eq!(format_tick(0.0, 1.0 / 30.0, Some(30.0)), "0");
        assert!(format_tick(1.0, 0.05, Some(30.0)).ends_with('s'));
    }

    #[test]
    fn ticks_align_to_multiples_of_the_step() {
        let view = TimeRange::new(1.3, 4.7);
        let ticks: Vec<f32> = ticks_in(view, 1.0).collect();
        assert_eq!(ticks, vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn ticks_stay_put_as_the_view_slides() {
        let a: Vec<f32> = ticks_in(TimeRange::new(0.0, 5.0), 1.0).collect();
        let b: Vec<f32> = ticks_in(TimeRange::new(0.1, 5.1), 1.0).collect();
        for t in [1.0, 2.0, 3.0, 4.0, 5.0] {
            assert!(a.contains(&t), "{t} missing from {a:?}");
            assert!(b.contains(&t), "{t} missing from {b:?}");
        }
    }

    #[test]
    fn ticks_never_run_past_the_view() {
        for (start, end, step) in [(0.0, 10.0, 1.0), (0.0, 0.35, 0.1), (-2.0, 2.0, 0.5)] {
            let view = TimeRange::new(start, end);
            for tick in ticks_in(view, step) {
                assert!(
                    tick >= start - step && tick <= end + step,
                    "tick {tick} outside {start}..{end}"
                );
            }
        }
    }

    #[test]
    fn an_empty_view_yields_no_ticks() {
        assert_eq!(ticks_in(TimeRange::new(3.0, 3.0), 1.0).count(), 0);
    }

    #[test]
    fn a_zero_step_does_not_hang() {
        let ticks: Vec<f32> = ticks_in(TimeRange::new(0.0, 3.0), 0.0).collect();
        assert_eq!(ticks, vec![0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn time_readout_includes_seconds_percent_and_frame() {
        let mut transport = Transport {
            frame_rate: Some(30.0),
            ..Transport::with_duration(2.0)
        };
        transport.seek(1.0);
        let text = format_time(&transport);
        assert!(text.contains("1.000s"), "{text}");
        assert!(text.contains("50%"), "{text}");
        assert!(text.contains("frame 30/60"), "{text}");
    }

    #[test]
    fn time_readout_omits_the_frame_when_unknown() {
        let mut transport = Transport::with_duration(2.0);
        transport.seek(0.5);
        let text = format_time(&transport);
        assert!(text.contains("0.500s"), "{text}");
        assert!(text.contains("25%"), "{text}");
        assert!(!text.contains("frame"), "{text}");
    }

    #[test]
    fn a_zero_length_asset_reports_zero_percent_rather_than_nan() {
        let transport = Transport::with_duration(0.0);
        let text = format_time(&transport);
        assert!(!text.contains("NaN"), "{text}");
        assert!(text.contains("0%"), "{text}");
    }
}
