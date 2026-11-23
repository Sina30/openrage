//! The seam between the timeline widget and whatever it is showing: particle
//! effects and animation clips both drive it through [`TimelineSource`].
//!
//! Rows come flat — a `Vec` of [`ChannelRow`] with a `depth` field, re-flattened
//! when expansion changes — so the widget paints a contiguous slice and keeps
//! per-row recursion out of a paint loop egui rebuilds every frame.
//!
//! [`TimelineSource::row_events`] takes the visible [`TimeRange`] and writes into
//! a caller-owned [`RowEvents`], so a 600-frame clip with 300 bones never
//! materializes 180,000 events to draw a few hundred pixels. Sources are expected
//! to clip to the view; the widget clips again, but only as a safety net.

use egui::Color32;
use or_transport::TimeRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowId(pub u64);

impl RowId {
    pub const fn new(kind: u8, primary: u32, secondary: u16) -> Self {
        Self(((kind as u64) << 56) | ((primary as u64) << 16) | secondary as u64)
    }

    pub const fn kind(self) -> u8 {
        (self.0 >> 56) as u8
    }

    pub const fn primary(self) -> u32 {
        ((self.0 >> 16) & 0xFFFF_FFFF) as u32
    }

    pub const fn secondary(self) -> u16 {
        (self.0 & 0xFFFF) as u16
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Group,
    Track,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RowBadge {
    pub text: String,
    pub colour: Color32,
    pub tooltip: String,
}

#[derive(Debug, Clone)]
pub struct ChannelRow {
    pub id: RowId,
    pub depth: u8,
    pub label: String,
    pub kind: RowKind,
    pub expandable: bool,
    pub expanded: bool,
    pub animated: bool,
    pub badge: Option<RowBadge>,
    pub colour: Color32,
}

impl ChannelRow {
    pub fn group(id: RowId, label: impl Into<String>) -> Self {
        Self {
            id,
            depth: 0,
            label: label.into(),
            kind: RowKind::Group,
            expandable: false,
            expanded: false,
            animated: true,
            badge: None,
            colour: Color32::from_rgb(0x8a, 0x8a, 0x8a),
        }
    }

    pub fn with_depth(mut self, depth: u8) -> Self {
        self.depth = depth;
        self
    }

    pub fn with_kind(mut self, kind: RowKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_colour(mut self, colour: Color32) -> Self {
        self.colour = colour;
        self
    }

    pub fn with_badge(mut self, badge: Option<RowBadge>) -> Self {
        self.badge = badge;
        self
    }

    pub fn animated(mut self, animated: bool) -> Self {
        self.animated = animated;
        self
    }

    pub fn expandable(mut self, expandable: bool, expanded: bool) -> Self {
        self.expandable = expandable;
        self.expanded = expanded;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyEvent {
    pub time: f32,
    pub shape: KeyShape,
    pub colour: Color32,
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyShape {
    Diamond,
    Square,
    Circle,
    Cross,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpanEvent {
    pub range: TimeRange,
    pub colour: Color32,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RowEvents {
    pub keys: Vec<KeyEvent>,
    pub spans: Vec<SpanEvent>,
    pub total_keys: usize,
}

impl RowEvents {
    pub fn clear(&mut self) {
        self.keys.clear();
        self.spans.clear();
        self.total_keys = 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CurveDomain {
    #[default]
    Time,
    Normalized,
}

#[derive(Debug, Clone, Default)]
pub struct CurveSeries {
    pub label: String,
    pub channels: Vec<CurveChannel>,
    pub handles: Vec<CurveHandle>,
    pub domain: CurveDomain,
    pub domain_label: String,
}

#[derive(Debug, Clone, Default)]
pub struct CurveChannel {
    pub colour: Color32,
    pub samples: Vec<(f32, f32)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveHandle {
    pub channel: usize,
    pub index: usize,
    pub time: f32,
    pub value: f32,
}

/// A curve handle dragged to a new time and value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineEdit {
    pub series: usize,
    pub channel: usize,
    pub index: usize,
    pub time: f32,
    pub value: f32,
}

pub trait TimelineSource {
    fn bounds(&self) -> TimeRange;

    fn rows(&self) -> &[ChannelRow];

    fn row_events(&self, row: usize, view: TimeRange, out: &mut RowEvents);

    fn curves(&self) -> &[CurveSeries] {
        &[]
    }

    /// Whether the source takes edits: it draws curve handles draggable and shows no read-only label.
    fn editable(&self) -> bool {
        false
    }

    fn read_only_reason(&self) -> Option<&str> {
        None
    }

    /// Why playing the source shows nothing, when it does not: the transport greys what only playing uses.
    fn playback_unavailable(&self) -> Option<&str> {
        None
    }

    fn frame_rate(&self) -> Option<f32> {
        None
    }

    fn summary(&self) -> String {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_ids_round_trip_their_parts() {
        let id = RowId::new(3, 0x1234_5678, 0xABCD);
        assert_eq!(id.kind(), 3);
        assert_eq!(id.primary(), 0x1234_5678);
        assert_eq!(id.secondary(), 0xABCD);
    }

    #[test]
    fn different_kinds_with_the_same_indices_are_distinct() {
        assert_ne!(RowId::new(0, 5, 1), RowId::new(1, 5, 1));
        assert_ne!(RowId::new(0, 5, 1), RowId::new(0, 5, 2));
        assert_ne!(RowId::new(0, 5, 1), RowId::new(0, 6, 1));
    }

    #[test]
    fn row_ids_handle_boundary_values() {
        let id = RowId::new(u8::MAX, u32::MAX, u16::MAX);
        assert_eq!(id.kind(), u8::MAX);
        assert_eq!(id.primary(), u32::MAX);
        assert_eq!(id.secondary(), u16::MAX);

        let zero = RowId::new(0, 0, 0);
        assert_eq!(zero.0, 0);
    }

    #[test]
    fn channel_row_builders_compose() {
        let row = ChannelRow::group(RowId::new(1, 2, 3), "SKEL_Head")
            .with_depth(2)
            .with_kind(RowKind::Track)
            .with_colour(Color32::RED)
            .animated(false)
            .expandable(true, false);
        assert_eq!(row.depth, 2);
        assert_eq!(row.kind, RowKind::Track);
        assert_eq!(row.colour, Color32::RED);
        assert!(!row.animated);
        assert!(row.expandable && !row.expanded);
        assert_eq!(row.label, "SKEL_Head");
    }

    #[test]
    fn row_events_clear_resets_the_key_count_too() {
        let mut events = RowEvents {
            keys: vec![KeyEvent {
                time: 0.0,
                shape: KeyShape::Diamond,
                colour: Color32::WHITE,
                selected: false,
            }],
            spans: vec![SpanEvent {
                range: TimeRange::new(0.0, 1.0),
                colour: Color32::WHITE,
                label: None,
            }],
            total_keys: 900,
        };
        assert!(!events.keys.is_empty());
        events.clear();
        assert!(events.keys.is_empty());
        assert!(events.spans.is_empty());
        assert_eq!(events.total_keys, 0);
    }

    struct Empty;
    impl TimelineSource for Empty {
        fn bounds(&self) -> TimeRange {
            TimeRange::new(0.0, 1.0)
        }
        fn rows(&self) -> &[ChannelRow] {
            &[]
        }
        fn row_events(&self, _row: usize, _view: TimeRange, out: &mut RowEvents) {
            out.clear();
        }
    }

    #[test]
    fn trait_defaults_are_read_only_and_curveless() {
        let source = Empty;
        assert!(source.curves().is_empty());
        assert!(!source.editable());
        assert!(source.read_only_reason().is_none());
        assert!(source.playback_unavailable().is_none());
        assert!(source.frame_rate().is_none());
        assert!(source.summary().is_empty());
        assert!(source.rows().is_empty());
    }
}
