//! Per-pixel event density, for when there are more keys than pixels.
//!
//! Above a keys-per-pixel threshold the timeline stops drawing keys and paints
//! one mesh whose cost is proportional to pixels: event times are bucketed into
//! one bin per pixel column, blurred so a sparse region does not alias into a
//! dotted line, and normalized. egui rebuilds every widget every frame, and a
//! 600-frame clip across 300 bones is roughly 180,000 glyphs per frame.
//!
//! ⚠ Normalization is against a *smoothed running maximum* rather than this
//! frame's peak: the instantaneous peak makes the whole graph flicker in
//! brightness while panning, because the tallest column keeps changing.
//! Carrying the previous maximum forward with exponential smoothing follows
//! Rerun's `data_density_graph`, at the cost of a one-frame lag.

use egui::{Color32, Mesh, Painter, Rect, Shape};

pub const DENSITY_THRESHOLD: f32 = 1.0;

const BLUR_RADIUS: usize = 2;

const MAX_SMOOTHING: f32 = 0.9;

#[derive(Debug, Default, Clone)]
pub struct DensityGraph {
    bins: Vec<f32>,
    blurred: Vec<f32>,
    running_max: f32,
}

impl DensityGraph {
    pub fn should_aggregate(key_count: usize, pixel_width: f32) -> bool {
        if pixel_width <= 0.0 {
            return false;
        }
        key_count as f32 / pixel_width > DENSITY_THRESHOLD
    }

    fn begin(&mut self, width: usize) {
        self.bins.clear();
        self.bins.resize(width, 0.0);
    }

    fn add(&mut self, x: f32) {
        if self.bins.is_empty() || !x.is_finite() {
            return;
        }
        let last = self.bins.len() - 1;
        if x < -1.0 || x > last as f32 + 1.0 {
            return;
        }
        let floor = x.floor();
        let fraction = x - floor;
        let low = floor as isize;
        if low >= 0 && (low as usize) <= last {
            self.bins[low as usize] += 1.0 - fraction;
        }
        let high = low + 1;
        if high >= 0 && (high as usize) <= last {
            self.bins[high as usize] += fraction;
        }
    }

    fn blur(&mut self) {
        let width = self.bins.len();
        self.blurred.clear();
        self.blurred.resize(width, 0.0);
        if width == 0 {
            return;
        }
        let radius = BLUR_RADIUS.min(width);
        let window = (radius * 2 + 1) as f32;
        for i in 0..width {
            let mut sum = 0.0;
            for offset in 0..=(radius * 2) {
                let index = i as isize + offset as isize - radius as isize;
                if index >= 0 && (index as usize) < width {
                    sum += self.bins[index as usize];
                }
            }
            self.blurred[i] = sum / window;
        }
    }

    fn normalize_against(&mut self) -> Option<f32> {
        let peak = self
            .blurred
            .iter()
            .copied()
            .fold(0.0f32, |acc, v| acc.max(v));
        if peak <= 0.0 && self.running_max <= 0.0 {
            return None;
        }
        self.running_max = if peak > self.running_max {
            peak
        } else {
            self.running_max * MAX_SMOOTHING + peak * (1.0 - MAX_SMOOTHING)
        };
        if self.running_max <= 0.0 {
            None
        } else {
            Some(self.running_max)
        }
    }

    pub fn paint(
        &mut self,
        painter: &Painter,
        rect: Rect,
        screen_positions: impl IntoIterator<Item = f32>,
        colour: Color32,
    ) -> bool {
        let width = rect.width().floor().max(0.0) as usize;
        if width == 0 || rect.height() <= 0.0 {
            return false;
        }
        self.begin(width);
        for x in screen_positions {
            self.add(x - rect.min.x);
        }
        self.blur();
        let Some(max) = self.normalize_against() else {
            return false;
        };

        let mut mesh = Mesh::default();
        for (index, value) in self.blurred.iter().enumerate() {
            let normalized = (value / max).clamp(0.0, 1.0);
            if normalized <= 0.0 {
                continue;
            }
            let height = (rect.height() * normalized).max(1.0);
            let x = rect.min.x + index as f32;
            let column = Rect::from_min_max(
                egui::pos2(x, rect.max.y - height),
                egui::pos2(x + 1.0, rect.max.y),
            );
            let alpha = (60.0 + 195.0 * normalized) as u8;
            mesh.add_colored_rect(column, colour.gamma_multiply(alpha as f32 / 255.0));
        }
        if mesh.is_empty() {
            return false;
        }
        painter.add(Shape::mesh(mesh));
        true
    }

    #[cfg(test)]
    fn blurred(&self) -> &[f32] {
        &self.blurred
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregation_engages_only_above_one_key_per_pixel() {
        assert!(!DensityGraph::should_aggregate(100, 200.0));
        assert!(!DensityGraph::should_aggregate(200, 200.0));
        assert!(DensityGraph::should_aggregate(201, 200.0));
        assert!(!DensityGraph::should_aggregate(10_000, 0.0));
    }

    #[test]
    fn events_land_in_their_pixel_column() {
        let mut graph = DensityGraph::default();
        graph.begin(10);
        graph.add(3.0);
        graph.add(3.0);
        assert!((graph.bins[3] - 2.0).abs() < 1e-6);
        assert_eq!(graph.bins[4], 0.0);
    }

    #[test]
    fn fractional_positions_split_between_neighbours() {
        let mut graph = DensityGraph::default();
        graph.begin(10);
        graph.add(3.25);
        assert!((graph.bins[3] - 0.75).abs() < 1e-6);
        assert!((graph.bins[4] - 0.25).abs() < 1e-6);
        let total: f32 = graph.bins.iter().sum();
        assert!((total - 1.0).abs() < 1e-6);
    }

    #[test]
    fn out_of_range_and_nonfinite_positions_are_dropped() {
        let mut graph = DensityGraph::default();
        graph.begin(10);
        graph.add(-50.0);
        graph.add(500.0);
        graph.add(f32::NAN);
        graph.add(f32::INFINITY);
        assert_eq!(graph.bins.iter().sum::<f32>(), 0.0);
    }

    #[test]
    fn positions_just_off_the_edge_still_contribute() {
        let mut graph = DensityGraph::default();
        graph.begin(10);
        graph.add(-0.5);
        assert!(graph.bins[0] > 0.0);
        graph.begin(10);
        graph.add(9.5);
        assert!(graph.bins[9] > 0.0);
    }

    #[test]
    fn adding_to_an_empty_histogram_is_harmless() {
        let mut graph = DensityGraph::default();
        graph.add(1.0);
        assert!(graph.bins.is_empty());
    }

    #[test]
    fn blur_spreads_a_single_spike_over_its_neighbours() {
        let mut graph = DensityGraph::default();
        graph.begin(11);
        graph.add(5.0);
        graph.blur();
        let blurred = graph.blurred();
        assert!(blurred[5] > 0.0 && blurred[5] < 1.0);
        assert!(blurred[4] > 0.0);
        assert!(blurred[6] > 0.0);
        assert_eq!(blurred[0], 0.0);
        assert_eq!(blurred[10], 0.0);
    }

    #[test]
    fn blur_conserves_total_mass_away_from_the_edges() {
        let mut graph = DensityGraph::default();
        graph.begin(21);
        graph.add(10.0);
        graph.blur();
        let total: f32 = graph.blurred().iter().sum();
        assert!((total - 1.0).abs() < 1e-5, "total was {total}");
    }

    #[test]
    fn blur_of_an_empty_histogram_is_empty() {
        let mut graph = DensityGraph::default();
        graph.begin(0);
        graph.blur();
        assert!(graph.blurred().is_empty());
        assert!(graph.normalize_against().is_none());
    }

    #[test]
    fn the_normalising_max_rises_immediately() {
        let mut graph = DensityGraph::default();
        graph.begin(5);
        for _ in 0..10 {
            graph.add(2.0);
        }
        graph.blur();
        let first = graph.normalize_against().expect("some density");
        assert!((first - graph.blurred()[2]).abs() < 1e-6);

        graph.begin(5);
        for _ in 0..100 {
            graph.add(2.0);
        }
        graph.blur();
        let second = graph.normalize_against().expect("some density");
        assert!(second > first * 5.0, "{first} -> {second}");
    }

    #[test]
    fn the_normalising_max_falls_slowly() {
        let mut graph = DensityGraph::default();
        graph.begin(5);
        for _ in 0..100 {
            graph.add(2.0);
        }
        graph.blur();
        let peak = graph.normalize_against().expect("density");

        graph.begin(5);
        graph.add(2.0);
        graph.blur();
        let after = graph.normalize_against().expect("density");
        assert!(after < peak);
        assert!(after > peak * 0.5, "{peak} -> {after} decayed too fast");
    }

    #[test]
    fn normalisation_reports_nothing_when_there_is_no_data_at_all() {
        let mut graph = DensityGraph::default();
        graph.begin(10);
        graph.blur();
        assert!(graph.normalize_against().is_none());
        assert_eq!(graph.running_max, 0.0);
    }

    #[test]
    fn histogram_size_tracks_pixels_not_events() {
        let mut graph = DensityGraph::default();
        graph.begin(80);
        for i in 0..100_000 {
            graph.add((i % 80) as f32);
        }
        assert_eq!(graph.bins.len(), 80);
        graph.blur();
        assert_eq!(graph.blurred().len(), 80);
    }

    #[test]
    fn buffers_are_reused_across_frames() {
        let mut graph = DensityGraph::default();
        graph.begin(50);
        graph.add(10.0);
        graph.blur();
        let capacity = graph.bins.capacity();
        for _ in 0..10 {
            graph.begin(50);
            graph.add(10.0);
            graph.blur();
        }
        assert_eq!(graph.bins.len(), 50);
        assert!(graph.bins.capacity() >= capacity);
    }
}
