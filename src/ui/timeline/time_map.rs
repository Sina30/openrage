use or_transport::TimeRange;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeMap {
    view: TimeRange,
    screen_min: f32,
    screen_max: f32,
}

// The timeline is a widget shared by the particle and animation timelines, so
// what it offers stays whether both callers use it today or not. ⚠ The allow
// covers the whole block; narrow it to the item if one arrives that the widget
// does not offer.
#[allow(dead_code)]
impl TimeMap {
    pub const MIN_SPAN: f32 = 1e-4;

    pub fn new(view: TimeRange, screen_min: f32, screen_max: f32) -> Self {
        Self {
            view,
            screen_min,
            screen_max: screen_max.max(screen_min),
        }
    }

    pub fn fit(bounds: TimeRange, screen_min: f32, screen_max: f32) -> Self {
        Self::new(bounds, screen_min, screen_max)
    }

    pub fn view(&self) -> TimeRange {
        self.view
    }

    pub fn screen_min(&self) -> f32 {
        self.screen_min
    }

    pub fn screen_max(&self) -> f32 {
        self.screen_max
    }

    pub fn screen_width(&self) -> f32 {
        self.screen_max - self.screen_min
    }

    pub fn pixels_per_second(&self) -> f32 {
        let span = self.view.span();
        if span <= 0.0 || self.screen_width() <= 0.0 {
            0.0
        } else {
            self.screen_width() / span
        }
    }

    pub fn seconds_per_pixel(&self) -> f32 {
        let width = self.screen_width();
        if width <= 0.0 {
            0.0
        } else {
            self.view.span() / width
        }
    }

    pub fn time_to_x(&self, time: f32) -> f32 {
        let span = self.view.span();
        if span <= 0.0 {
            return self.screen_min;
        }
        self.screen_min + (time - self.view.start) / span * self.screen_width()
    }

    pub fn x_to_time(&self, x: f32) -> f32 {
        let width = self.screen_width();
        if width <= 0.0 {
            return self.view.start;
        }
        self.view.start + (x - self.screen_min) / width * self.view.span()
    }

    pub fn is_visible(&self, time: f32) -> bool {
        self.view.contains(time)
    }

    pub fn pan(&self, delta_px: f32) -> Self {
        let seconds = delta_px * self.seconds_per_pixel();
        Self {
            view: TimeRange::new(self.view.start - seconds, self.view.end - seconds),
            ..*self
        }
    }

    pub fn zoom_at(&self, anchor_px: f32, factor: f32) -> Self {
        if !(factor.is_finite() && factor > 0.0) {
            return *self;
        }
        let anchor_time = self.x_to_time(anchor_px);
        let span = self.view.span();
        if span <= 0.0 {
            return *self;
        }
        let new_span = (span / factor).max(Self::MIN_SPAN);
        let fraction = if span > 0.0 {
            ((anchor_time - self.view.start) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let start = anchor_time - fraction * new_span;
        Self {
            view: TimeRange::new(start, start + new_span),
            ..*self
        }
    }

    pub fn clamped_to(&self, bounds: TimeRange) -> Self {
        if bounds.is_empty() {
            return *self;
        }
        let span = self.view.span();
        if span >= bounds.span() {
            return Self {
                view: bounds,
                ..*self
            };
        }
        let mut start = self.view.start;
        if start < bounds.start {
            start = bounds.start;
        }
        if start + span > bounds.end {
            start = bounds.end - span;
        }
        Self {
            view: TimeRange::new(start, start + span),
            ..*self
        }
    }

    pub fn zoom_to(&self, range: TimeRange, margin_fraction: f32) -> Self {
        if range.is_empty() {
            return *self;
        }
        let margin = range.span() * margin_fraction.max(0.0);
        Self {
            view: TimeRange::new(range.start - margin, range.end + margin),
            ..*self
        }
    }

    pub fn with_screen(&self, screen_min: f32, screen_max: f32) -> Self {
        Self::new(self.view, screen_min, screen_max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> TimeMap {
        TimeMap::new(TimeRange::new(0.0, 10.0), 0.0, 100.0)
    }

    #[test]
    fn time_and_x_are_inverses() {
        let m = map();
        for t in [0.0, 1.0, 5.5, 10.0] {
            let x = m.time_to_x(t);
            assert!((m.x_to_time(x) - t).abs() < 1e-5, "t = {t}, x = {x}");
        }
        assert_eq!(m.time_to_x(0.0), 0.0);
        assert_eq!(m.time_to_x(10.0), 100.0);
        assert_eq!(m.time_to_x(5.0), 50.0);
    }

    #[test]
    fn scale_helpers_agree_with_the_mapping() {
        let m = map();
        assert!((m.pixels_per_second() - 10.0).abs() < 1e-6);
        assert!((m.seconds_per_pixel() - 0.1).abs() < 1e-6);
        assert_eq!(m.screen_width(), 100.0);
    }

    #[test]
    fn times_outside_the_view_map_outside_the_pixel_range() {
        let m = TimeMap::new(TimeRange::new(2.0, 4.0), 0.0, 100.0);
        assert!(m.time_to_x(1.0) < 0.0);
        assert!(m.time_to_x(5.0) > 100.0);
        assert!(!m.is_visible(1.0));
        assert!(m.is_visible(3.0));
    }

    #[test]
    fn a_pixel_offset_maps_onto_a_shifted_screen_range() {
        let m = TimeMap::new(TimeRange::new(0.0, 10.0), 40.0, 140.0);
        assert_eq!(m.time_to_x(0.0), 40.0);
        assert_eq!(m.time_to_x(10.0), 140.0);
        assert!((m.x_to_time(90.0) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn panning_shifts_the_view_by_the_pixel_delta() {
        let m = map();
        let panned = m.pan(10.0);
        assert!((panned.view().start + 1.0).abs() < 1e-6);
        assert!((panned.view().end - 9.0).abs() < 1e-6);
        assert!((panned.view().span() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn zoom_keeps_the_anchor_time_under_the_anchor_pixel() {
        let m = map();
        for anchor in [0.0, 25.0, 50.0, 100.0] {
            let before = m.x_to_time(anchor);
            for factor in [0.25, 0.5, 2.0, 8.0] {
                let zoomed = m.zoom_at(anchor, factor);
                let after = zoomed.x_to_time(anchor);
                assert!(
                    (after - before).abs() < 1e-4,
                    "anchor {anchor} factor {factor}: {before} -> {after}"
                );
            }
        }
    }

    #[test]
    fn zooming_in_narrows_the_span_by_the_factor() {
        let m = map();
        let zoomed = m.zoom_at(50.0, 2.0);
        assert!((zoomed.view().span() - 5.0).abs() < 1e-5);
        let out = m.zoom_at(50.0, 0.5);
        assert!((out.view().span() - 20.0).abs() < 1e-5);
    }

    #[test]
    fn zoom_stops_at_the_minimum_span() {
        let m = map();
        let zoomed = m.zoom_at(50.0, 1e12);
        assert!(
            zoomed.view().span() >= TimeMap::MIN_SPAN * 0.99,
            "span was {}",
            zoomed.view().span()
        );
        assert!(zoomed.view().span() <= TimeMap::MIN_SPAN * 1.01);
        assert!(zoomed.view().span().is_finite());
    }

    #[test]
    fn a_nonsensical_zoom_factor_is_ignored() {
        let m = map();
        assert_eq!(m.zoom_at(50.0, 0.0), m);
        assert_eq!(m.zoom_at(50.0, -1.0), m);
        assert_eq!(m.zoom_at(50.0, f32::NAN), m);
    }

    #[test]
    fn clamping_slides_the_view_rather_than_shrinking_it() {
        let bounds = TimeRange::new(0.0, 10.0);
        let m = TimeMap::new(TimeRange::new(-3.0, -1.0), 0.0, 100.0).clamped_to(bounds);
        assert!((m.view().start - 0.0).abs() < 1e-6);
        assert!((m.view().span() - 2.0).abs() < 1e-6);

        let m = TimeMap::new(TimeRange::new(11.0, 13.0), 0.0, 100.0).clamped_to(bounds);
        assert!((m.view().end - 10.0).abs() < 1e-6);
        assert!((m.view().span() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn a_view_wider_than_bounds_snaps_to_bounds() {
        let bounds = TimeRange::new(0.0, 10.0);
        let m = TimeMap::new(TimeRange::new(-5.0, 30.0), 0.0, 100.0).clamped_to(bounds);
        assert_eq!(m.view(), bounds);
        let panned = m.pan(500.0).clamped_to(bounds);
        assert_eq!(panned.view(), bounds);
    }

    #[test]
    fn clamping_to_empty_bounds_is_a_no_op() {
        let m = map();
        assert_eq!(m.clamped_to(TimeRange::new(5.0, 5.0)), m);
    }

    #[test]
    fn zoom_to_frames_a_range_with_margin() {
        let m = map();
        let framed = m.zoom_to(TimeRange::new(2.0, 4.0), 0.1);
        assert!((framed.view().start - 1.8).abs() < 1e-5);
        assert!((framed.view().end - 4.2).abs() < 1e-5);
        assert_eq!(m.zoom_to(TimeRange::new(3.0, 3.0), 0.1), m);
    }

    #[test]
    fn resizing_the_panel_keeps_the_visible_time_window() {
        let m = map();
        let resized = m.with_screen(0.0, 400.0);
        assert_eq!(resized.view(), m.view());
        assert!((resized.pixels_per_second() - 40.0).abs() < 1e-6);
    }

    #[test]
    fn degenerate_ranges_do_not_produce_infinities() {
        let m = TimeMap::new(TimeRange::new(0.0, 10.0), 50.0, 50.0);
        assert_eq!(m.pixels_per_second(), 0.0);
        assert_eq!(m.seconds_per_pixel(), 0.0);
        assert!(m.time_to_x(5.0).is_finite());
        assert!(m.x_to_time(50.0).is_finite());

        let m = TimeMap::new(TimeRange::new(3.0, 3.0), 0.0, 100.0);
        assert_eq!(m.pixels_per_second(), 0.0);
        assert_eq!(m.time_to_x(3.0), 0.0);
        assert_eq!(m.x_to_time(50.0), 3.0);
        assert_eq!(m.zoom_at(50.0, 2.0), m);
    }

    #[test]
    fn an_inverted_screen_range_is_normalised() {
        let m = TimeMap::new(TimeRange::new(0.0, 10.0), 100.0, 20.0);
        assert_eq!(m.screen_min(), 100.0);
        assert_eq!(m.screen_max(), 100.0);
        assert_eq!(m.screen_width(), 0.0);
    }

    #[test]
    fn fit_shows_the_whole_asset() {
        let bounds = TimeRange::new(1.0, 6.0);
        let m = TimeMap::fit(bounds, 0.0, 500.0);
        assert_eq!(m.view(), bounds);
        assert!((m.pixels_per_second() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn repeated_pan_and_zoom_stays_stable() {
        let bounds = TimeRange::new(0.0, 10.0);
        let mut m = TimeMap::fit(bounds, 0.0, 100.0);
        for _ in 0..200 {
            m = m.zoom_at(50.0, 1.05).pan(3.0).clamped_to(bounds);
        }
        assert!(m.view().span() > 0.0 && m.view().span().is_finite());
        assert!(m.view().start >= bounds.start - 1e-3);
        assert!(m.view().end <= bounds.end + 1e-3);
    }
}
