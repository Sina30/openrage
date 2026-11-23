//! A watch on the folders of the project's clothing packs: a change in a pack's files asks for a rescan at once, in place of the fixed cadence.
//!
//! ⚠ **The fixed cadence stays as the fallback.** A watcher that cannot start, or a folder it cannot
//! watch, leaves `covers` false, and the Wardrobe is read again every `RESCAN_AFTER` as it was before.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use super::RESCAN_AFTER;
use crate::ui::dock::Workspace;

/// How long the files of a pack stay quiet before it is read again: a save, a copy or a checkout is many writes.
const QUIET: Duration = Duration::from_millis(300);

pub struct PackWatch {
    events: Receiver<notify::Result<notify::Event>>,
    watcher: Option<RecommendedWatcher>,
    /// The folders the watcher took.
    watched: HashSet<PathBuf>,
    /// The folders the last `follow` asked for.
    asked: HashSet<PathBuf>,
    /// When the last change was seen, until it has been quiet long enough to be reported.
    changed_at: Option<Instant>,
}

impl PackWatch {
    pub fn new() -> Self {
        let (sender, events) = channel();
        let watcher = RecommendedWatcher::new(
            move |event| {
                let _ = sender.send(event);
            },
            notify::Config::default(),
        )
        .map_err(|e| tracing::warn!(error = %e, "no file watcher, so the Wardrobe is read again on a clock"))
        .ok();
        Self {
            events,
            watcher,
            watched: HashSet::new(),
            asked: HashSet::new(),
            changed_at: None,
        }
    }

    /// A watch whose watcher could not start, as one is on a machine that has none.
    #[cfg(test)]
    pub fn unavailable() -> Self {
        let mut watch = Self::new();
        watch.watcher = None;
        watch
    }

    /// Watch exactly `folders`, each with everything in it: a folder no longer asked for is let go, and one that could not be watched is tried again at each call.
    pub fn follow(&mut self, folders: &[PathBuf]) {
        let wanted: HashSet<PathBuf> = folders.iter().cloned().collect();
        if wanted == self.asked && self.covers() {
            return;
        }
        let first_time = wanted != self.asked;
        let gone: Vec<PathBuf> = self.watched.difference(&wanted).cloned().collect();
        let new: Vec<PathBuf> = wanted.difference(&self.watched).cloned().collect();
        if let Some(watcher) = self.watcher.as_mut() {
            for folder in gone {
                let _ = watcher.unwatch(&folder);
                self.watched.remove(&folder);
            }
            for folder in new {
                match watcher.watch(&folder, RecursiveMode::Recursive) {
                    Ok(()) => {
                        self.watched.insert(folder);
                    }
                    Err(e) if first_time => {
                        tracing::warn!(folder = %folder.display(), error = %e, "a pack's folder is not watched");
                    }
                    Err(_) => {}
                }
            }
        }
        self.asked = wanted;
    }

    /// Whether every folder asked for is watched, so a change to any of them is heard and the clock is not needed.
    pub fn covers(&self) -> bool {
        self.watcher.is_some()
            && !self.asked.is_empty()
            && self.asked.iter().all(|f| self.watched.contains(f))
    }

    /// Whether a change has been seen and the files have been quiet since; true once for a change.
    pub fn changed(&mut self, now: Instant) -> bool {
        for event in self.events.try_iter().flatten() {
            // ⚠ What the Wardrobe keeps beside a pack (versions, originals, the lock) is no change of the pack: writing it must not ask for the scan that wrote it.
            let private =
                |path: &std::path::Path| path.components().any(|c| c.as_os_str() == ".openrage");
            if matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) && !event.paths.iter().all(|path| private(path))
            {
                self.changed_at = Some(now);
            }
        }
        match self.changed_at {
            Some(at) if now.saturating_duration_since(at) >= QUIET => {
                self.changed_at = None;
                true
            }
            _ => false,
        }
    }
}

impl super::Workbench {
    /// Watch the folders of the packs the last scan read, each time one lands: a folder that could not be watched is tried again then.
    pub(super) fn follow_packs(&mut self) {
        let folders: Vec<PathBuf> = self.packs().iter().map(|p| p.folder.clone()).collect();
        self.pack_watch
            .get_or_insert_with(PackWatch::new)
            .follow(&folders);
    }
}

impl Default for PackWatch {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether the clock alone asks for the disk to be read again: on the Project and Wardrobe workspaces once `RESCAN_AFTER` has passed, except for the Wardrobe while its packs are watched.
pub(super) fn due_by_the_clock(
    workspace: Workspace,
    watching: bool,
    since_the_last: Option<Duration>,
) -> bool {
    let on_screen = workspace.needs_a_project();
    let covered = workspace.is_task_screen() && watching;
    on_screen && !covered && since_the_last.is_none_or(|since| since >= RESCAN_AFTER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::front_door::{NewProject, Template};
    use crate::state::{AppState, Prefs};

    fn bare() -> AppState {
        let mut state = AppState::from_prefs(Prefs::default());
        state.documents.clear();
        state.active_document = None;
        state
    }

    fn within(what: &str, seconds: u64, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        while !done() {
            assert!(Instant::now() < deadline, "{what} never happened");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_change_in_a_watched_folder_is_reported_once_it_has_been_quiet() {
        let dir = tempfile::tempdir().unwrap();
        let mut watch = PackWatch::new();
        watch.follow(&[dir.path().to_path_buf()]);
        assert!(watch.covers());
        assert!(!watch.changed(Instant::now()), "nothing has changed yet");

        std::fs::create_dir_all(dir.path().join("stream/x")).unwrap();
        std::fs::write(dir.path().join("stream/x/a.ydd"), b"model").unwrap();
        within("the report of a file written in a subfolder", 10, || {
            watch.changed(Instant::now())
        });
        assert!(!watch.changed(Instant::now()), "a change is reported once");
    }

    #[test]
    fn what_is_kept_beside_a_pack_in_its_private_folder_is_no_change_of_the_pack() {
        let dir = tempfile::tempdir().unwrap();
        let mut watch = PackWatch::new();
        watch.follow(&[dir.path().to_path_buf()]);
        assert!(watch.covers());
        // ⚠ A Mac can report a folder as made a moment after it was: that is heard out before the writes this test is about.
        if cfg!(target_os = "macos") {
            std::thread::sleep(Duration::from_millis(900));
            let before = Instant::now();
            watch.changed(before);
            watch.changed(before + QUIET + Duration::from_millis(50));
        }

        std::fs::create_dir_all(dir.path().join(".openrage/versions")).unwrap();
        std::fs::write(dir.path().join(".openrage/versions/000000.snapshot"), b"v").unwrap();
        std::fs::write(dir.path().join(".openrage/lock"), b"").unwrap();
        std::thread::sleep(Duration::from_millis(900));
        // The first call hears the events, and the second is a quiet long enough to report what it heard.
        let heard = Instant::now();
        assert!(!watch.changed(heard));
        assert!(
            !watch.changed(heard + QUIET + Duration::from_millis(50)),
            "a version kept is not a change of the pack"
        );

        std::fs::write(dir.path().join("a.ydd"), b"model").unwrap();
        within("the report of a file of the pack", 10, || {
            watch.changed(Instant::now())
        });
    }

    #[test]
    fn a_folder_that_cannot_be_watched_or_a_watcher_that_cannot_start_leaves_the_clock() {
        let dir = tempfile::tempdir().unwrap();
        let mut watch = PackWatch::new();
        watch.follow(&[dir.path().to_path_buf(), dir.path().join("not-there")]);
        assert!(!watch.covers(), "one of the two cannot be watched");
        watch.follow(&[dir.path().to_path_buf()]);
        assert!(watch.covers(), "asked for the one it can");
        watch.follow(&[]);
        assert!(!watch.covers(), "with no folder there is nothing covered");

        let mut none = PackWatch::unavailable();
        none.follow(&[dir.path().to_path_buf()]);
        assert!(!none.covers());
        std::fs::write(dir.path().join("a.ydd"), b"x").unwrap();
        std::thread::sleep(Duration::from_millis(400));
        assert!(!none.changed(Instant::now()), "it hears nothing");
    }

    #[test]
    fn the_clock_asks_for_a_scan_on_the_project_and_the_wardrobe_except_for_a_wardrobe_that_is_watched(
    ) {
        let late = Some(RESCAN_AFTER);
        let early = Some(RESCAN_AFTER / 2);
        for (workspace, watching, since, due) in [
            (Workspace::Wardrobe, false, late, true),
            (Workspace::Wardrobe, false, early, false),
            (Workspace::Wardrobe, true, late, false),
            (Workspace::Wardrobe, true, None, false),
            (Workspace::Wardrobe, false, None, true),
            // The Project workspace lists files outside the packs, which nothing watches.
            (Workspace::Project, true, late, true),
            (Workspace::Project, true, early, false),
            (Workspace::Model, false, late, false),
        ] {
            assert_eq!(
                due_by_the_clock(workspace, watching, since),
                due,
                "{workspace:?}, watched {watching}, {since:?}"
            );
        }
    }

    /// The seconds from a person's edit of a pack's files to the Wardrobe reading it, with the pack watched.
    fn seconds_to_see_an_outside_edit(mut state: AppState) -> f32 {
        within("the first scan", 30, || {
            state.poll_workbench();
            state.workbench.is_scanned() && state.wardrobe_pack().is_some()
        });
        within("the watch on the pack", 30, || {
            state.poll_workbench();
            state
                .workbench
                .pack_watch
                .as_ref()
                .is_some_and(PackWatch::covers)
        });
        // Let the scans the watcher's setup caused finish, so that the clock is not what the next one comes from.
        let settle = Instant::now() + Duration::from_millis(800);
        while Instant::now() < settle {
            state.poll_workbench();
            std::thread::sleep(Duration::from_millis(5));
        }
        let folder = state.wardrobe_pack().unwrap().folder.clone();
        let drop = folder.join("..").join("drop");
        std::fs::create_dir_all(&drop).unwrap();
        std::fs::write(
            drop.join("mp_m_freemode_01_mp_m_t^jbib_000_u.ydd"),
            or_source::sample::drawables_of("jacket"),
        )
        .unwrap();
        let started = Instant::now();
        // Somebody else adds clothes to the pack: the app is not the one writing.
        or_wardrobe::edit::add_clothes(&folder, &[drop]).expect("the pack is edited");
        within("the Wardrobe reading the garment", 30, || {
            state.poll_workbench();
            state
                .wardrobe_pack()
                .and_then(|p| p.read.as_ref().ok())
                .is_some_and(|read| read.wardrobe.garments.len() == 1)
        });
        started.elapsed().as_secs_f32()
    }

    #[test]
    fn an_edit_made_outside_the_app_to_a_pack_is_seen_by_the_watcher_before_the_clock_is_due() {
        let dir = tempfile::tempdir().unwrap();
        let mut draft = NewProject::new(Template::Clothing, dir.path().to_path_buf());
        draft.set_name("tshirts".into());
        let created = draft.create().expect("the template makes a project");
        let mut state = bare();
        state.load_project_from(created.file.clone());
        assert_eq!(state.active_workspace, Workspace::Wardrobe);

        let took = seconds_to_see_an_outside_edit(state);
        assert!(
            took < RESCAN_AFTER.as_secs_f32() - 1.0,
            "the Wardrobe took {took} s to see it, which is the clock's pace and not the watcher's"
        );
    }
}
