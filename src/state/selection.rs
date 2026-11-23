use std::collections::BTreeSet;

use super::document::DocumentId;

/// What is selected: one focused row, and the rows selected alongside it.
///
/// ⚠ **The focus is what every single-row reader sees**: `selected` returns one
/// id, and only [`Selection::selected_ids`] reveals the rest. Only rows in the
/// focus's document can be selected together — a bulk operation across two
/// archives has no single destination.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    current: Option<(DocumentId, u64)>,
    path: Option<or_editor::PackPath>,
    also: BTreeSet<u64>,
    anchor: Option<u64>,
}

impl Selection {
    pub fn select(&mut self, doc: DocumentId, id: u64) {
        self.current = Some((doc, id));
        self.path = None;
        self.also.clear();
        self.anchor = Some(id);
    }

    pub fn select_described(&mut self, doc: DocumentId, path: or_editor::PackPath) {
        self.current = Some((doc, described_id(&path)));
        self.path = Some(path);
        self.also.clear();
        self.anchor = None;
    }

    /// Ctrl-click: add a row to the selection or take it out, without
    /// touching the rest.
    pub fn toggle(&mut self, doc: DocumentId, id: u64) {
        let Some((current_doc, current_id)) = self.current else {
            return self.select(doc, id);
        };
        if current_doc != doc || self.path.is_some() {
            return self.select(doc, id);
        }
        self.anchor = Some(id);
        if current_id == id {
            match self.also.pop_first() {
                Some(next) => self.current = Some((doc, next)),
                None => self.current = None,
            }
        } else if !self.also.remove(&id) {
            self.also.insert(current_id);
            self.current = Some((doc, id));
        }
    }

    /// Shift-click: select every row from the anchor to `id` in `order`, which
    /// is the order the rows are on screen.
    ///
    /// ⚠ **The range is a span of the visible order, not of ids.** Node ids
    /// are allocation order, so a sorted or filtered view would select rows
    /// the user never saw between the two they clicked. An anchor missing from
    /// `order` — scrolled into a collapsed folder, or the folder the listing shows, which it does
    /// not list — spans from the row clicked last, and a selection with neither on screen
    /// starts over rather than guessing where it was.
    pub fn select_range(&mut self, doc: DocumentId, id: u64, order: &[u64]) {
        let (anchor, last) = match (self.current, self.anchor) {
            (Some((d, last)), Some(anchor)) if d == doc && self.path.is_none() => (anchor, last),
            _ => return self.select(doc, id),
        };
        let place = |row: u64| order.iter().position(|&seen| seen == row);
        let (Some(from), Some(to)) = (place(anchor).or_else(|| place(last)), place(id)) else {
            return self.select(doc, id);
        };
        let (lo, hi) = (from.min(to), from.max(to));
        self.also = order[lo..=hi]
            .iter()
            .copied()
            .filter(|&r| r != id)
            .collect();
        self.current = Some((doc, id));
    }

    pub fn described(&self, doc: DocumentId) -> Option<&or_editor::PackPath> {
        match self.current {
            Some((d, _)) if d == doc => self.path.as_ref(),
            _ => None,
        }
    }

    pub fn clear(&mut self, doc: DocumentId) {
        if matches!(self.current, Some((d, _)) if d == doc) {
            self.clear_all();
        }
    }

    pub fn clear_all(&mut self) {
        self.current = None;
        self.path = None;
        self.also.clear();
        self.anchor = None;
    }

    pub fn forget(&mut self, doc: DocumentId) {
        self.clear(doc);
    }

    pub fn selected(&self, doc: DocumentId) -> Option<u64> {
        match self.current {
            Some((d, id)) if d == doc => Some(id),
            _ => None,
        }
    }

    /// Every selected id in `doc`, the focus first.
    pub fn selected_ids(&self, doc: DocumentId) -> Vec<u64> {
        match self.current {
            Some((d, id)) if d == doc => std::iter::once(id)
                .chain(self.also.iter().copied())
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn is_selected(&self, doc: DocumentId, id: u64) -> bool {
        match self.current {
            Some((d, focus)) if d == doc => focus == id || self.also.contains(&id),
            _ => false,
        }
    }

    /// The rows a drag of `id` carries: every row selected when `id` is one of them, else `id` alone.
    pub fn dragged(&self, doc: DocumentId, id: u64) -> Vec<u64> {
        let rows = self.selected_ids(doc);
        if rows.contains(&id) {
            rows
        } else {
            vec![id]
        }
    }

    // The whole selection, where every production caller wants one half of
    // it. Kept because `panel_state`'s runtime-registered panel reads it:
    // a panel with no `AppState` field has no other route to the selection.
    pub fn current(&self) -> Option<(DocumentId, u64)> {
        self.current
    }
}

fn described_id(path: &or_editor::PackPath) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use or_editor::{Hop, PackPath};

    fn path(field: &str, index: usize) -> PackPath {
        PackPath::default()
            .then(Hop::Field(field.into()))
            .then(Hop::Index(index))
    }

    #[test]
    fn selecting_one_kind_clears_the_other() {
        let mut sel = Selection::default();

        sel.select_described(1, path("bounds", 0));
        assert!(sel.described(1).is_some());

        sel.select(1, 0xDEAD_BEEF);
        assert!(
            sel.described(1).is_none(),
            "a domain selection must not leave a stale path behind"
        );
        assert_eq!(sel.selected(1), Some(0xDEAD_BEEF));

        sel.select_described(1, path("bounds", 3));
        assert_eq!(sel.described(1), Some(&path("bounds", 3)));
    }

    #[test]
    fn a_path_identifies_itself() {
        let mut a = Selection::default();
        let mut b = Selection::default();
        a.select_described(1, path("lods", 0));
        b.select_described(1, path("lods", 1));
        assert_ne!(a.current(), b.current());

        b.select_described(1, path("lods", 0));
        assert_eq!(a.current(), b.current());
    }

    #[test]
    fn clearing_drops_both_halves() {
        let mut sel = Selection::default();
        sel.select_described(4, path("lods", 1));
        sel.clear(4);
        assert!(sel.described(4).is_none());
        assert!(sel.selected(4).is_none());
    }

    fn sorted(mut ids: Vec<u64>) -> Vec<u64> {
        ids.sort_unstable();
        ids
    }

    /// The on-screen order deliberately disagrees with id order, so a range
    /// computed over ids selects different rows from one computed over what
    /// the user sees.
    const ON_SCREEN: [u64; 7] = [40, 12, 7, 33, 21, 5, 18];

    #[test]
    fn selecting_a_range_yields_exactly_the_rows_between_the_clicks() {
        let mut sel = Selection::default();
        sel.select(1, 12);
        sel.select_range(1, 21, &ON_SCREEN);

        assert_eq!(sorted(sel.selected_ids(1)), vec![7, 12, 21, 33]);
        assert_eq!(sel.selected(1), Some(21), "the clicked row takes the focus");
        for outside in [40, 5, 18] {
            assert!(
                !sel.is_selected(1, outside),
                "{outside} is outside the span"
            );
        }
    }

    #[test]
    fn a_range_runs_upwards_as_well_and_keeps_its_anchor() {
        let mut sel = Selection::default();
        sel.select(1, 5);
        sel.select_range(1, 7, &ON_SCREEN);
        assert_eq!(sorted(sel.selected_ids(1)), vec![5, 7, 21, 33]);

        // A second shift-click re-spans from the same anchor rather than
        // growing the first span.
        sel.select_range(1, 18, &ON_SCREEN);
        assert_eq!(sorted(sel.selected_ids(1)), vec![5, 18]);
    }

    #[test]
    fn a_range_whose_anchor_is_off_screen_starts_over() {
        let mut sel = Selection::default();
        sel.select(1, 999);
        sel.select_range(1, 33, &ON_SCREEN);
        assert_eq!(sel.selected_ids(1), vec![33]);
    }

    #[test]
    fn a_range_after_the_off_screen_anchor_was_taken_out_spans_from_the_last_row_clicked() {
        // 999 is the folder the listing shows, which the listing does not list
        let mut sel = Selection::default();
        sel.select(1, 999);
        sel.toggle(1, 12);
        sel.toggle(1, 999);
        assert_eq!(sel.selected_ids(1), vec![12]);
        sel.select_range(1, 33, &ON_SCREEN);
        assert_eq!(
            sorted(sel.selected_ids(1)),
            vec![7, 12, 33],
            "a Shift click spans from the row clicked last when the anchor is not on screen"
        );
    }
    #[test]
    fn a_range_never_crosses_documents() {
        let mut sel = Selection::default();
        sel.select(1, 12);
        sel.select_range(2, 21, &ON_SCREEN);
        assert_eq!(sel.selected_ids(2), vec![21]);
        assert!(sel.selected_ids(1).is_empty());
    }

    #[test]
    fn toggling_adds_and_removes_one_row_and_leaves_the_rest() {
        let mut sel = Selection::default();
        sel.select(1, 12);
        sel.toggle(1, 33);
        sel.toggle(1, 5);
        assert_eq!(sorted(sel.selected_ids(1)), vec![5, 12, 33]);
        assert_eq!(sel.selected(1), Some(5));

        sel.toggle(1, 12);
        assert_eq!(sorted(sel.selected_ids(1)), vec![5, 33]);

        sel.toggle(1, 5);
        assert_eq!(sel.selected_ids(1), vec![33], "the focus passes on");
        sel.toggle(1, 33);
        assert!(sel.selected_ids(1).is_empty());
    }

    #[test]
    fn a_plain_click_collapses_a_multi_selection() {
        let mut sel = Selection::default();
        sel.select(1, 12);
        sel.select_range(1, 5, &ON_SCREEN);
        assert!(sel.selected_ids(1).len() > 1);
        sel.select(1, 40);
        assert_eq!(sel.selected_ids(1), vec![40]);
    }
}
