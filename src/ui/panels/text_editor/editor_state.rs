use std::num::NonZeroUsize;
use std::ops::Range;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::LayoutJob;
use egui::{FontId, Galley, TextFormat};
use lru::LruCache;
use ropey::Rope;

use super::fold_worker::FoldWorker;
use super::folds::FoldTree;
use super::go_to_line::GoToLine;
use super::highlighters::{highlight_line, Language};
use super::replace_worker::ReplaceWorker;
use super::search_worker::SearchWorker;
use crate::ui::theme;

pub const GUTTER_PAD: f32 = 8.0;
pub const CARET_WIDTH: f32 = 1.5;

pub const GALLEY_CACHE_CAP: usize = 1024;

pub const UNDO_HISTORY_CAP: usize = 1024;

pub const EDIT_COALESCE_WINDOW: Duration = Duration::from_millis(750);

pub const FIND_DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, Default)]
pub struct CaretState {
    pub cursor: usize,
    pub anchor: usize,
    pub scroll_y: f32,
    /// How far the lines are scrolled sideways, in points.
    pub scroll_x: f32,
    pub desired_col: Option<usize>,
}

impl CaretState {
    pub fn selection(&self) -> Option<Range<usize>> {
        if self.cursor == self.anchor {
            None
        } else {
            let lo = self.cursor.min(self.anchor);
            let hi = self.cursor.max(self.anchor);
            Some(lo..hi)
        }
    }

    pub fn collapse(&mut self) {
        self.anchor = self.cursor;
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    text: Arc<str>,
    font_size: u32,
    language: Language,
    appearance: u64,
}

pub struct GalleyCache {
    inner: LruCache<CacheKey, Arc<Galley>>,
}

impl GalleyCache {
    pub fn new() -> Self {
        Self {
            inner: LruCache::new(NonZeroUsize::new(GALLEY_CACHE_CAP).unwrap()),
        }
    }

    pub fn get_or_layout(
        &mut self,
        ctx: &egui::Context,
        text: &str,
        font: &FontId,
        language: Language,
    ) -> Arc<Galley> {
        let key = CacheKey {
            text: Arc::<str>::from(text),
            font_size: (font.size * 100.0) as u32,
            language,
            appearance: theme::generation(),
        };
        if let Some(g) = self.inner.get(&key) {
            return g.clone();
        }
        let mut job = LayoutJob::default();
        let base = TextFormat {
            font_id: font.clone(),
            color: theme::TEXT_PRIMARY.get(),
            ..Default::default()
        };
        for token in highlight_line(language, text) {
            let mut fmt = base.clone();
            fmt.color = token.color;
            job.append(token.text, 0.0, fmt);
        }
        let galley = ctx.fonts_mut(|f| f.layout_job(job));
        self.inner.put(key.clone(), galley.clone());
        galley
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

#[derive(Debug, Clone)]
pub struct Edit {
    pub char_pos: usize,
    pub removed: String,
    pub inserted: String,
    pub cursor_before: usize,
    pub cursor_after: usize,
    pub timestamp: Instant,
    pub coalescable: bool,
}

pub enum HistoryEntry {
    Edit(Edit),
    Snapshot {
        before: Arc<Rope>,
        after: Arc<Rope>,
        cursor_before: usize,
        cursor_after: usize,
    },
}

#[derive(Default)]
pub struct EditorHistory {
    pub undo: Vec<HistoryEntry>,
    pub redo: Vec<HistoryEntry>,
}

impl EditorHistory {
    pub fn push_edit(&mut self, edit: Edit) {
        if let Some(HistoryEntry::Edit(last)) = self.undo.last_mut() {
            if last.coalescable
                && edit.coalescable
                && last.removed.is_empty()
                && edit.removed.is_empty()
                && last.char_pos + last.inserted.chars().count() == edit.char_pos
                && edit.timestamp.duration_since(last.timestamp) < EDIT_COALESCE_WINDOW
            {
                last.inserted.push_str(&edit.inserted);
                last.cursor_after = edit.cursor_after;
                last.timestamp = edit.timestamp;
                self.redo.clear();
                return;
            }
        }
        self.undo.push(HistoryEntry::Edit(edit));
        if self.undo.len() > UNDO_HISTORY_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn push_snapshot(
        &mut self,
        before: Arc<Rope>,
        after: Arc<Rope>,
        cursor_before: usize,
        cursor_after: usize,
    ) {
        self.undo.push(HistoryEntry::Snapshot {
            before,
            after,
            cursor_before,
            cursor_after,
        });
        if self.undo.len() > UNDO_HISTORY_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, rope: &mut Rope) -> Option<(usize, Option<Rope>)> {
        let entry = self.undo.pop()?;
        match entry {
            HistoryEntry::Edit(edit) => {
                let inserted_chars = edit.inserted.chars().count();
                if inserted_chars > 0 {
                    rope.remove(edit.char_pos..edit.char_pos + inserted_chars);
                }
                if !edit.removed.is_empty() {
                    rope.insert(edit.char_pos, &edit.removed);
                }
                let cursor = edit.cursor_before;
                self.redo.push(HistoryEntry::Edit(edit));
                Some((cursor, None))
            }
            HistoryEntry::Snapshot {
                before,
                after,
                cursor_before,
                cursor_after,
            } => {
                let new_rope = (*before).clone();
                let cursor = cursor_before;
                self.redo.push(HistoryEntry::Snapshot {
                    before,
                    after,
                    cursor_before,
                    cursor_after,
                });
                Some((cursor, Some(new_rope)))
            }
        }
    }

    pub fn redo(&mut self, rope: &mut Rope) -> Option<(usize, Option<Rope>)> {
        let entry = self.redo.pop()?;
        match entry {
            HistoryEntry::Edit(edit) => {
                let removed_chars = edit.removed.chars().count();
                if removed_chars > 0 {
                    rope.remove(edit.char_pos..edit.char_pos + removed_chars);
                }
                if !edit.inserted.is_empty() {
                    rope.insert(edit.char_pos, &edit.inserted);
                }
                let cursor = edit.cursor_after;
                self.undo.push(HistoryEntry::Edit(edit));
                Some((cursor, None))
            }
            HistoryEntry::Snapshot {
                before,
                after,
                cursor_before,
                cursor_after,
            } => {
                let new_rope = (*after).clone();
                let cursor = cursor_after;
                self.undo.push(HistoryEntry::Snapshot {
                    before,
                    after,
                    cursor_before,
                    cursor_after,
                });
                Some((cursor, Some(new_rope)))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindMode {
    Find,
    Replace,
}

pub struct FindState {
    pub visible: bool,
    pub mode: FindMode,
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub in_selection: bool,
    pub has_selection: bool,
    pub matches: Vec<Range<usize>>,
    pub active: Option<usize>,
    pub gen: u64,
    pub cancel: Arc<AtomicBool>,
    pub searching: bool,
    pub truncated: bool,
    pub last_input: Instant,
    pub matched_at_revision: u64,
    pub error: Option<String>,
    pub last_scope: Range<usize>,
    pub last_dispatched_query: String,
    pub last_dispatched_flags: u32,
    pub want_focus_query: bool,
}

impl Default for FindState {
    fn default() -> Self {
        Self {
            visible: false,
            mode: FindMode::Find,
            query: String::new(),
            replacement: String::new(),
            case_sensitive: false,
            whole_word: false,
            regex: false,
            in_selection: false,
            has_selection: false,
            matches: Vec::new(),
            active: None,
            gen: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            searching: false,
            truncated: false,
            last_input: Instant::now(),
            matched_at_revision: 0,
            error: None,
            last_scope: 0..0,
            last_dispatched_query: String::new(),
            last_dispatched_flags: u32::MAX,
            want_focus_query: false,
        }
    }
}

impl FindState {
    pub fn flag_bits(&self) -> u32 {
        (self.case_sensitive as u32)
            | ((self.whole_word as u32) << 1)
            | ((self.regex as u32) << 2)
            | ((self.in_selection as u32) << 3)
    }

    pub fn invalidate_results(&mut self) {
        self.matches.clear();
        self.active = None;
        self.truncated = false;
        self.searching = false;
    }

    /// Whether the search has run for the query and options now in the bar, over the text as it stands at `rope_revision`.
    pub fn searched(&self, rope_revision: u64) -> bool {
        self.query == self.last_dispatched_query
            && self.flag_bits() == self.last_dispatched_flags
            && self.matched_at_revision == rope_revision
    }
}

/// The editor's find, replace and go-to-line commands this frame, as the user bound them.
#[derive(Debug, Clone, Default)]
pub struct FindKeys {
    pub find: bool,
    pub replace: bool,
    pub go_to_line: bool,
    /// The replace binding the find bar names.
    pub replace_hint: Option<String>,
}

pub enum FoldComputeState {
    Idle,
    Pending { gen: u64 },
    Ready(FoldTree),
}

pub struct FoldState {
    pub language: Language,
    pub compute: FoldComputeState,
    pub worker: Option<FoldWorker>,
    pub gen_counter: u64,
    pub computed_for_revision: u64,
}

impl FoldState {
    pub fn new() -> Self {
        Self {
            language: Language::Plain,
            compute: FoldComputeState::Idle,
            worker: None,
            gen_counter: 0,
            computed_for_revision: u64::MAX,
        }
    }

    pub fn tree(&self) -> Option<&FoldTree> {
        match &self.compute {
            FoldComputeState::Ready(t) => Some(t),
            _ => None,
        }
    }

    pub fn tree_mut(&mut self) -> Option<&mut FoldTree> {
        match &mut self.compute {
            FoldComputeState::Ready(t) => Some(t),
            _ => None,
        }
    }
}

pub struct EditorState {
    pub caret: CaretState,
    pub history: EditorHistory,
    pub cache: GalleyCache,
    pub revision: u64,
    pub scrolled_at_revision: u64,
    pub rope_revision: u64,
    pub find: FindState,
    /// Set before each paint, from the app's bindings.
    pub find_keys: FindKeys,
    /// The Go to Line field, while it is open.
    pub go_to: Option<GoToLine>,
    pub search_worker: Option<SearchWorker>,
    pub replace_worker: Option<ReplaceWorker>,
    pub folds: FoldState,
    pub last_language: Language,
    /// The step Edit ▸ Undo or Redo asks of the next paint: true to undo, false to redo.
    pub asked_history: Option<bool>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            caret: CaretState::default(),
            history: EditorHistory::default(),
            cache: GalleyCache::new(),
            revision: 0,
            scrolled_at_revision: 0,
            rope_revision: 0,
            find: FindState::default(),
            find_keys: FindKeys::default(),
            go_to: None,
            search_worker: None,
            replace_worker: None,
            folds: FoldState::new(),
            last_language: Language::Plain,
            asked_history: None,
        }
    }
}

impl EditorState {
    pub fn id(doc_id: crate::state::DocumentId) -> egui::Id {
        egui::Id::new(("text_editor_state_v3", doc_id))
    }

    pub fn search_worker(&mut self) -> &SearchWorker {
        if self.search_worker.is_none() {
            self.search_worker = Some(SearchWorker::spawn());
        }
        self.search_worker.as_ref().unwrap()
    }

    pub fn replace_worker(&mut self) -> &ReplaceWorker {
        if self.replace_worker.is_none() {
            self.replace_worker = Some(ReplaceWorker::spawn());
        }
        self.replace_worker.as_ref().unwrap()
    }
}

pub type EditorStateHandle = Arc<std::sync::Mutex<EditorState>>;

pub fn load_state(ctx: &egui::Context, doc_id: crate::state::DocumentId) -> EditorStateHandle {
    let id = EditorState::id(doc_id);
    if let Some(handle) = ctx.memory(|m| m.data.get_temp::<EditorStateHandle>(id)) {
        return handle;
    }
    let handle: EditorStateHandle = Arc::new(std::sync::Mutex::new(EditorState::default()));
    ctx.memory_mut(|m| m.data.insert_temp(id, handle.clone()));
    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AppearancePrefs, ThemeChoice};

    fn colours(galley: &Galley) -> Vec<egui::Color32> {
        galley.job.sections.iter().map(|s| s.format.color).collect()
    }

    /// ⚠ Falsified by taking `appearance` out of `CacheKey`: the second
    /// layout is then the cached dark one, drawn on a light background.
    #[test]
    fn a_theme_change_lays_cached_text_out_again() {
        let _palette = theme::serial();
        let ctx = egui::Context::default();
        let font = FontId::monospace(13.0);
        let mut cache = GalleyCache::new();

        theme::apply_theme(&ctx, &AppearancePrefs::default());
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        let dark = cache.get_or_layout(&ctx, "<Item Name=\"x\"/>", &font, Language::Xml);
        let again = cache.get_or_layout(&ctx, "<Item Name=\"x\"/>", &font, Language::Xml);
        assert!(
            Arc::ptr_eq(&dark, &again),
            "an unchanged theme should be served from the cache"
        );

        let light = AppearancePrefs {
            theme: ThemeChoice::Light,
            ..AppearancePrefs::default()
        };
        theme::apply_theme(&ctx, &light);
        let relaid = cache.get_or_layout(&ctx, "<Item Name=\"x\"/>", &font, Language::Xml);
        theme::apply_theme(&ctx, &AppearancePrefs::default());

        assert_ne!(
            colours(&dark),
            colours(&relaid),
            "the cache served the old theme's colours"
        );
    }
}
