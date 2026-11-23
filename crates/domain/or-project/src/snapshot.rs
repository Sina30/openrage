//! The steps of a file kept on disk, one copy of its bytes for each, in a folder a repository never sees.
//!
//! The editor keeps a project's history here (`history` in the project's own folder of `.openrage`, see
//! [`private_dir`]) and the Wardrobe will keep a pack's. A step is the whole file as it stood, so going to
//! any step is a read and no step depends on another; the store knows nothing of what the bytes are. A
//! step may carry bytes of its own, which go when it does.
//!
//! ⚠ **History is best effort.** An index that cannot be read opens as an empty store and a step whose
//! file was deleted is dropped when the store opens, so a damaged folder costs steps and never the project.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use crate::paths::project_folder;
use crate::project::replace_file;

/// How many steps a store keeps; the oldest go first.
pub const KEPT: usize = 200;

/// The folder inside a project's folder that holds what belongs to one machine, never to the repository.
pub const PRIVATE_DIR: &str = ".openrage";

const INDEX: &str = "index.json";
const IGNORE: &str = ".gitignore";

/// What belongs to the project file `project` on this machine: `.openrage/projects/<its name without the extension>` in its folder.
///
/// ⚠ Named for the file, not the folder: two project files in one folder (a Save project as where the
/// dialog starts) would otherwise share one history and one list of files outside the folder.
pub fn private_dir(project: &Path) -> PathBuf {
    let name = project
        .file_stem()
        .unwrap_or(std::ffi::OsStr::new("project"));
    (project_folder(project).join(PRIVATE_DIR))
        .join("projects")
        .join(name)
}

/// Where the history of the project file `project` lives.
pub fn history_dir(project: &Path) -> PathBuf {
    private_dir(project).join("history")
}

/// One step of a store: the number its file is named by, what it was and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub n: u64,
    pub label: String,
    /// Milliseconds since 1970.
    pub at_ms: u64,
}

impl Step {
    /// When the step was made.
    pub fn at(&self) -> SystemTime {
        UNIX_EPOCH + std::time::Duration::from_millis(self.at_ms)
    }
}

/// The steps of one file, oldest first, and the one the file stands at.
#[derive(Debug)]
pub struct Snapshots {
    dir: PathBuf,
    steps: Vec<Step>,
    at: Option<usize>,
    kept: usize,
}

impl Snapshots {
    /// The store in `dir`; empty when there is none or it cannot be read. Nothing is created.
    pub fn open(dir: &Path) -> Self {
        let mut store = Self {
            dir: dir.to_path_buf(),
            steps: Vec::new(),
            at: None,
            kept: KEPT,
        };
        let Ok(text) = std::fs::read(dir.join(INDEX)) else {
            return store;
        };
        let Ok(Value::Object(index)) = serde_json::from_slice::<Value>(&text) else {
            return store;
        };
        let standing = index.get("at").and_then(Value::as_u64);
        for entry in index
            .get("steps")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let step = Step {
                n: entry.get("n").and_then(Value::as_u64).unwrap_or(u64::MAX),
                label: entry
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                at_ms: entry.get("ms").and_then(Value::as_u64).unwrap_or(0),
            };
            if step.n != u64::MAX && store.file(step.n).is_file() {
                store.steps.push(step);
            }
        }
        store.at = standing
            .and_then(|n| store.steps.iter().position(|step| step.n == n))
            .or_else(|| store.steps.len().checked_sub(1));
        store
    }

    /// The store keeps at most `kept` steps (at least one) instead of [`KEPT`]: a pack's versions are whole files that can be large.
    pub fn keeping(mut self, kept: usize) -> Self {
        self.kept = kept.max(1);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Every step, oldest first.
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// The place in [`Self::steps`] of the step the file stands at.
    pub fn position(&self) -> Option<usize> {
        self.at
    }

    /// The step the file stands at.
    pub fn current(&self) -> Option<&Step> {
        self.steps.get(self.at?)
    }

    /// The bytes of the step at `position`.
    pub fn read(&self, position: usize) -> io::Result<Vec<u8>> {
        let step = self.steps.get(position).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("no step {position}"))
        })?;
        std::fs::read(self.file(step.n))
    }

    /// Keep `bytes` as a new step called `label` and stand at it; the steps after the one stood at, which were undone, are dropped. Returns its number.
    pub fn push(&mut self, label: &str, bytes: &[u8]) -> io::Result<u64> {
        std::fs::create_dir_all(&self.dir)?;
        // ⚠ The folder ignores itself, so a repository that did not list `.openrage` still never sees it.
        let ignore = self.dir.join(IGNORE);
        if !ignore.exists() {
            std::fs::write(ignore, b"*\n")?;
        }
        let keep = self.at.map_or(0, |at| at + 1);
        // ⚠ Dropped first: the step after the one stood at has the number the new step takes.
        let dropped: Vec<Step> = self.steps.drain(keep..).collect();
        for step in dropped {
            self.forget(step.n);
        }
        let n = self.steps.last().map_or(0, |last| last.n + 1);
        // ⚠ A step whose file was deleted was dropped when the store opened, and its bytes may be left under the number this one takes.
        let _ = std::fs::remove_file(self.attached(n));
        replace_file(&self.file(n), bytes)?;
        let at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        self.steps.push(Step {
            n,
            label: label.to_string(),
            at_ms,
        });
        while self.steps.len() > self.kept {
            let oldest = self.steps.remove(0);
            self.forget(oldest.n);
        }
        self.at = Some(self.steps.len() - 1);
        self.write_index()?;
        Ok(n)
    }

    /// Keep `bytes` with the step numbered `n`, which the step has until it is dropped.
    pub fn attach(&self, n: u64, bytes: &[u8]) -> io::Result<()> {
        replace_file(&self.attached(n), bytes)
    }

    /// What was attached to the step numbered `n`.
    pub fn attachment(&self, n: u64) -> Option<Vec<u8>> {
        std::fs::read(self.attached(n)).ok()
    }

    /// Rewrite the bytes of the step stood at, for a file that was written since the step was kept.
    pub fn replace(&mut self, bytes: &[u8]) -> io::Result<()> {
        let step = self
            .current()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no step"))?;
        replace_file(&self.file(step.n), bytes)
    }

    /// Stand at the step at `position`, which keeps the steps after it to go forward to.
    pub fn go(&mut self, position: usize) -> io::Result<()> {
        if position >= self.steps.len() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no step {position}"),
            ));
        }
        self.at = Some(position);
        self.write_index()
    }

    fn file(&self, n: u64) -> PathBuf {
        self.dir.join(format!("{n:06}.snapshot"))
    }

    /// The folder that keeps a copy of what the step numbered `n` made, which Redo puts back; it goes with the step.
    pub fn aside(&self, n: u64) -> PathBuf {
        self.dir.join(format!("{n:06}.made"))
    }

    fn attached(&self, n: u64) -> PathBuf {
        self.dir.join(format!("{n:06}.attached"))
    }

    fn forget(&self, n: u64) {
        let _ = std::fs::remove_file(self.file(n));
        let _ = std::fs::remove_file(self.attached(n));
        let _ = std::fs::remove_dir_all(self.aside(n));
    }

    fn write_index(&self) -> io::Result<()> {
        let steps: Vec<Value> = (self.steps.iter())
            .map(|step| json!({ "n": step.n, "label": step.label, "ms": step.at_ms }))
            .collect();
        let mut index = Map::new();
        if let Some(step) = self.current() {
            index.insert("at".into(), step.n.into());
        }
        index.insert("steps".into(), Value::Array(steps));
        let bytes = serde_json::to_vec_pretty(&Value::Object(index)).map_err(io::Error::other)?;
        replace_file(&self.dir.join(INDEX), &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(dir: &Path) -> PathBuf {
        dir.join("harbor.openrage-proj")
    }

    fn store(dir: &Path) -> Snapshots {
        Snapshots::open(&history_dir(&project(dir)))
    }

    #[test]
    fn two_project_files_in_one_folder_have_folders_of_their_own() {
        let dir = tempfile::tempdir().unwrap();
        let (first, second) = (
            project(dir.path()),
            dir.path().join("harbor_v2.openrage-proj"),
        );
        assert_ne!(private_dir(&first), private_dir(&second));
        assert_ne!(history_dir(&first), history_dir(&second));
        assert!(history_dir(&first).starts_with(private_dir(&first)));
        assert!(private_dir(&first).starts_with(dir.path().join(PRIVATE_DIR)));
    }

    #[test]
    fn what_is_attached_to_a_step_outlasts_the_store_and_goes_when_the_step_does() {
        let dir = tempfile::tempdir().unwrap();
        let mut steps = store(dir.path());
        steps.push("a", b"1").unwrap();
        let b = steps.push("b", b"2").unwrap();
        steps.attach(b, b"made").unwrap();

        assert_eq!(store(dir.path()).attachment(b).unwrap(), b"made");
        assert_eq!(
            store(dir.path()).attachment(0),
            None,
            "no other step has it"
        );

        steps.go(0).unwrap();
        steps.push("c", b"3").unwrap();
        assert_eq!(
            steps.attachment(b),
            None,
            "the step c took the number of b and must not be given what b carried"
        );
    }

    #[test]
    fn a_step_never_inherits_what_was_left_under_its_number() {
        let dir = tempfile::tempdir().unwrap();
        let mut steps = store(dir.path());
        steps.push("a", b"1").unwrap();
        let b = steps.push("b", b"2").unwrap();
        steps.attach(b, b"made").unwrap();
        // The step's copy is deleted by hand: the store drops it when it opens and leaves the attachment.
        std::fs::remove_file(history_dir(&project(dir.path())).join(format!("{b:06}.snapshot")))
            .unwrap();

        let mut again = store(dir.path());
        assert_eq!(again.steps().len(), 1);
        assert_eq!(again.push("c", b"3").unwrap(), b);
        assert_eq!(again.attachment(b), None);
    }

    #[test]
    fn what_is_attached_to_the_oldest_step_goes_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut steps = store(dir.path());
        let first = steps.push("first", b"0").unwrap();
        steps.attach(first, b"made").unwrap();
        for i in 1..=KEPT {
            steps.push(&format!("step {i}"), b"x").unwrap();
        }
        assert_eq!(steps.steps()[0].label, "step 1");
        assert!(
            !history_dir(&project(dir.path()))
                .join(format!("{first:06}.attached"))
                .exists(),
            "the oldest step was trimmed and its bytes stayed"
        );
    }

    #[test]
    fn a_step_is_the_bytes_it_was_given_and_survives_the_store_being_opened_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut steps = store(dir.path());
        assert!(steps.is_empty());
        steps.push("Opened", b"one").unwrap();
        steps.push("Added a build", b"two").unwrap();

        let again = store(dir.path());
        let labels: Vec<&str> = again.steps().iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["Opened", "Added a build"]);
        assert_eq!(again.position(), Some(1));
        assert_eq!(again.read(0).unwrap(), b"one");
        assert_eq!(again.read(1).unwrap(), b"two");
    }

    #[test]
    fn going_back_keeps_the_later_steps_to_go_forward_to_and_a_new_step_drops_them() {
        let dir = tempfile::tempdir().unwrap();
        let mut steps = store(dir.path());
        for (label, bytes) in [("a", "1"), ("b", "2"), ("c", "3")] {
            steps.push(label, bytes.as_bytes()).unwrap();
        }
        steps.go(0).unwrap();
        assert_eq!(store(dir.path()).position(), Some(0), "the place is kept");
        assert_eq!(store(dir.path()).steps().len(), 3, "and the steps after it");

        steps.push("d", b"4").unwrap();
        let labels: Vec<&str> = steps.steps().iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            ["a", "d"],
            "what was undone is gone once something new is done"
        );
        assert_eq!(steps.read(1).unwrap(), b"4");
        let files = std::fs::read_dir(history_dir(&project(dir.path())))
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "snapshot"))
            .count();
        assert_eq!(files, 2, "the dropped steps' files are removed");
    }

    #[test]
    fn a_step_can_be_rewritten_for_a_file_that_was_written_since() {
        let dir = tempfile::tempdir().unwrap();
        let mut steps = store(dir.path());
        steps.push("a", b"1").unwrap();
        steps.push("b", b"2").unwrap();
        steps.go(0).unwrap();

        steps.replace(b"one").unwrap();
        assert_eq!(steps.read(0).unwrap(), b"one", "the step stood at");
        assert_eq!(steps.read(1).unwrap(), b"2", "and no other");
        assert!(
            Snapshots::open(&history_dir(&project(&dir.path().join("none"))))
                .replace(b"x")
                .is_err()
        );
    }
    #[test]
    fn the_folder_ignores_itself() {
        let dir = tempfile::tempdir().unwrap();
        let mut steps = store(dir.path());
        steps.push("Opened", b"x").unwrap();
        let ignore = std::fs::read(history_dir(&project(dir.path())).join(".gitignore")).unwrap();
        assert_eq!(ignore, b"*\n");
    }

    #[test]
    fn only_the_newest_steps_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let mut steps = store(dir.path());
        for i in 0..KEPT + 5 {
            steps
                .push(&format!("step {i}"), format!("{i}").as_bytes())
                .unwrap();
        }
        assert_eq!(steps.steps().len(), KEPT);
        assert_eq!(steps.steps()[0].label, "step 5");
        assert_eq!(steps.read(0).unwrap(), b"5");
        let files = std::fs::read_dir(history_dir(&project(dir.path())))
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "snapshot"))
            .count();
        assert_eq!(files, KEPT);
    }

    #[test]
    fn a_store_that_keeps_fewer_steps_drops_the_oldest_first_and_never_keeps_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut steps = store(dir.path()).keeping(3);
        for i in 0..5 {
            steps.push(&format!("s{i}"), b"x").unwrap();
        }
        let labels: Vec<&str> = steps.steps().iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["s2", "s3", "s4"]);
        let mut one = store(dir.path()).keeping(0);
        one.push("only", b"y").unwrap();
        assert_eq!(one.steps().len(), 1);
    }

    #[test]
    fn a_damaged_index_opens_empty_and_a_deleted_step_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let mut steps = store(dir.path());
        steps.push("a", b"1").unwrap();
        steps.push("b", b"2").unwrap();
        let history = history_dir(&project(dir.path()));

        std::fs::remove_file(history.join("000000.snapshot")).unwrap();
        let labels: Vec<String> = (store(dir.path()).steps().iter())
            .map(|s| s.label.clone())
            .collect();
        assert_eq!(labels, ["b"]);

        std::fs::write(history.join("index.json"), b"{ not json").unwrap();
        let mut broken = store(dir.path());
        assert!(
            broken.is_empty(),
            "a history that cannot be read is no history"
        );
        broken.push("again", b"3").unwrap();
        assert_eq!(store(dir.path()).steps().len(), 1);
    }
}
