//! Which rows of a GTA VI scan's list are shown, how many are of each kind, the row the list goes
//! to and the rows an extraction takes.

use std::collections::{BTreeMap, BTreeSet};

use or_io::container::rpf::Rpf9Family;

use super::rpf9_scan::Row;

/// A family of rows the list's toggles show or hide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RowKind {
    Model,
    /// Of drawables or of textures: a scan does not read far enough to tell which.
    Dictionary,
    Collision,
    Navigation,
    Animation,
    Metadata,
    Text,
    Audio,
    Movie,
    Shaders,
    Archive,
    /// A resource of a family with no row kind of its own, or of none the layouts place.
    Resource,
    Unknown,
    Missing,
}

impl RowKind {
    pub const ALL: [Self; 14] = [
        Self::Model,
        Self::Dictionary,
        Self::Collision,
        Self::Navigation,
        Self::Animation,
        Self::Metadata,
        Self::Text,
        Self::Audio,
        Self::Movie,
        Self::Shaders,
        Self::Archive,
        Self::Resource,
        Self::Unknown,
        Self::Missing,
    ];

    pub fn of(row: &Row) -> Self {
        let Row::Body(find) = row else {
            return Self::Missing;
        };
        match Rpf9Family::of(find) {
            Rpf9Family::Models => Self::Model,
            Rpf9Family::Dictionaries => Self::Dictionary,
            Rpf9Family::Collision => Self::Collision,
            Rpf9Family::Navigation => Self::Navigation,
            Rpf9Family::Animations => Self::Animation,
            Rpf9Family::Metadata => Self::Metadata,
            Rpf9Family::Text => Self::Text,
            Rpf9Family::Audio => Self::Audio,
            Rpf9Family::Movies => Self::Movie,
            Rpf9Family::Shaders => Self::Shaders,
            Rpf9Family::Archives => Self::Archive,
            Rpf9Family::Other => Self::Resource,
            Rpf9Family::Unknown => Self::Unknown,
        }
    }
}

/// The most rows a change of the filter examines in one frame, so that a list of hundreds of
/// thousands is filtered over a few frames and never in one.
const FILTER_BUDGET: usize = 50_000;

/// Which rows of a scan's list are shown: those of the kinds toggled on, every kind when none is,
/// whose words hold the text; and the row the list goes to where the text states an offset.
#[derive(Debug, Default)]
pub struct ListFilter {
    /// What the Find field holds: words rows are kept by, or an offset the list goes to.
    pub text: String,
    pub kinds: BTreeSet<RowKind>,
    /// The row the last jump landed on, which the list marks.
    pub jumped: Option<usize>,
    /// Whether the list scrolls to the jumped row on its next frame.
    pub scroll: bool,
    /// The offset the text last sent the list to.
    sought: Option<u64>,
    /// How many of the first `counted` rows are of each kind.
    counts: BTreeMap<RowKind, usize>,
    counted: usize,
    /// The rows shown, of the first `examined`, for the text and kinds `built` holds.
    shown: Vec<usize>,
    examined: usize,
    built: (String, BTreeSet<RowKind>),
}

impl ListFilter {
    /// The offset the text states, written in hexadecimal after `0x`.
    pub fn offset(&self) -> Option<u64> {
        let text = self.text.trim();
        let hex = text
            .strip_prefix("0x")
            .or_else(|| text.strip_prefix("0X"))?;
        u64::from_str_radix(hex, 16).ok()
    }

    /// The words rows are kept by: an offset keeps every row and sends the list to its own.
    fn words(&self) -> &str {
        match self.offset() {
            Some(_) => "",
            None => self.text.trim(),
        }
    }

    pub fn is_off(&self) -> bool {
        self.words().is_empty() && self.kinds.is_empty()
    }

    /// Send the list to the row at the offset the text states, once for each offset it states.
    pub fn seek(&mut self, rows: &[Row]) {
        let offset = self.offset();
        if offset == self.sought {
            return;
        }
        self.sought = offset;
        if let Some(offset) = offset {
            let first = (!rows.is_empty()).then_some(0);
            self.jumped = row_at_offset(rows, offset).or(first);
            self.scroll = true;
        }
    }

    /// How many of `rows` are of each kind, of those counted so far: at most [`FILTER_BUDGET`]
    /// more a call, as the filter examines them.
    pub fn counts(&mut self, rows: &[Row]) -> &BTreeMap<RowKind, usize> {
        let upto = rows.len().min(self.counted + FILTER_BUDGET);
        for row in rows.iter().take(upto).skip(self.counted) {
            *self.counts.entry(RowKind::of(row)).or_default() += 1;
        }
        self.counted = upto;
        &self.counts
    }

    /// Whether rows of a list of `rows` are still to be counted by kind.
    pub fn counting(&self, rows: usize) -> bool {
        self.counted < rows
    }

    /// The rows of `rows` shown, in order, or `None` for every row: the rows added since the last
    /// call are examined, all of them when the filter changed, at most [`FILTER_BUDGET`] a call.
    pub fn shown(&mut self, rows: &[Row], words: impl Fn(&Row) -> String) -> Option<&[usize]> {
        if self.is_off() {
            return None;
        }
        let key = (self.words().to_lowercase(), self.kinds.clone());
        if key != self.built {
            self.shown.clear();
            (self.examined, self.built) = (0, key);
        }
        let (text, kinds) = &self.built;
        let upto = rows.len().min(self.examined + FILTER_BUDGET);
        for (i, row) in rows.iter().enumerate().take(upto).skip(self.examined) {
            let kind = kinds.is_empty() || kinds.contains(&RowKind::of(row));
            if kind && (text.is_empty() || words(row).to_lowercase().contains(text.as_str())) {
                self.shown.push(i);
            }
        }
        self.examined = upto;
        Some(&self.shown)
    }

    /// Whether rows of a list of `rows` are still to be examined.
    pub fn filtering(&self, rows: usize) -> bool {
        !self.is_off() && self.examined < rows
    }
}

/// The rows the Extract button takes of a list of `rows`: those selected, else those shown.
pub fn rows_to_extract(
    selection: &BTreeSet<usize>,
    shown: Option<&[usize]>,
    rows: usize,
) -> Vec<usize> {
    match (selection.is_empty(), shown) {
        (false, _) => selection.iter().copied().collect(),
        (true, Some(shown)) => shown.to_vec(),
        (true, None) => (0..rows).collect(),
    }
}

/// The rows a Shift+click takes: every row shown from `anchor` to `to`, both included.
pub fn rows_between(anchor: usize, to: usize, shown: Option<&[usize]>) -> Vec<usize> {
    let (low, high) = (anchor.min(to), anchor.max(to));
    match shown {
        Some(shown) => {
            let from = shown.partition_point(|&row| row < low);
            let until = shown.partition_point(|&row| row <= high);
            shown[from..until].to_vec()
        }
        None => (low..=high).collect(),
    }
}

/// The last row of `rows` that starts at or before `offset`; `None` when every row starts after it.
pub fn row_at_offset(rows: &[Row], offset: u64) -> Option<usize> {
    let start = |row: &Row| match row {
        Row::Body(find) => find.offset,
        Row::Missing { from, .. } => *from,
    };
    rows.partition_point(|r| start(r) <= offset).checked_sub(1)
}
