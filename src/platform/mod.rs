pub mod associations;
pub mod clipboard;
pub mod drag_drop;
pub mod file_dialogs;
pub mod locale;
pub mod new_window;
pub mod recycle_bin;
#[cfg(any(target_os = "linux", test))]
pub mod runtime_libraries;
pub mod shell;

use std::sync::atomic::{AtomicBool, Ordering};

static STANDING_IN: AtomicBool = AtomicBool::new(false);

/// Send every call past the app to its stand-in from now on, as a self-check does before anything else.
pub fn stand_in_for_the_desktop() {
    STANDING_IN.store(true, Ordering::Relaxed);
}

/// Whether a call past the app goes to its stand-in, which records what it was asked: always in a test build, and in a self-check.
pub fn standing_in() -> bool {
    cfg!(test) || STANDING_IN.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    /// What `body` logged, one message each.
    pub(crate) fn said(body: impl FnOnce()) -> Vec<String> {
        let records = crate::diagnostics::captured(body).snapshot().records;
        records.iter().map(|r| r.message.clone()).collect()
    }

    #[test]
    fn a_self_check_stands_in_for_the_desktop_before_anything_else() {
        let refused = crate::self_check::Options {
            shots: Some(Default::default()),
            ..Default::default()
        };
        assert_eq!(
            crate::self_check::run(refused),
            1,
            "a run refused at its first check"
        );
        assert!(
            STANDING_IN.load(Ordering::Relaxed),
            "a self-check got past its first check with the desktop's own calls"
        );
    }

    /// ⚠ Read from the source rather than run: a test that reached the desktop would open a window on it.
    #[test]
    fn nothing_outside_the_platform_layer_reaches_past_the_app() {
        const PAST_THE_APP: [&str; 6] = [
            "rfd::",
            "arboard::",
            "explorer.exe",
            "xdg-open",
            "SHFileOperation",
            "ShellExecute",
        ];
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let (mut folders, mut read, mut reaching) = (vec![src.clone()], 0, Vec::new());
        while let Some(folder) = folders.pop() {
            for path in std::fs::read_dir(&folder)
                .expect("src lists")
                .flatten()
                .map(|e| e.path())
            {
                if path.is_dir() && path != src.join("platform") {
                    folders.push(path);
                } else if let Ok(text) = std::fs::read_to_string(&path) {
                    read += 1;
                    for call in PAST_THE_APP.iter().filter(|call| text.contains(*call)) {
                        reaching.push(format!("  {}: {call}", path.display()));
                    }
                }
            }
        }
        assert!(read > 100, "only {read} files read: the walk is wrong");
        assert!(
            reaching.is_empty(),
            "these reach past the app without going through src/platform, whose stand-ins a \
             self-check uses:\n{}",
            reaching.join("\n")
        );
    }

    #[test]
    fn every_stand_in_says_what_it_was_asked() {
        let scratch = tempfile::tempdir().expect("a scratch folder");
        let file = scratch.path().join("asked.ytd");
        std::fs::write(&file, b"asked").expect("a file");
        let bin = scratch.path().join("bin");
        std::fs::create_dir(&bin).expect("a bin");
        recycle_bin::fake::use_bin(&bin);

        let lines = said(|| {
            shell::reveal(&file);
            shell::open_folder(&bin);
            clipboard::offer_files(std::slice::from_ref(&file)).expect("the clipboard's stand-in");
            recycle_bin::recycle(&file).expect("the Recycle Bin's stand-in");
        });
        for (asked, what) in [
            ("reveal", &file),
            ("open", &bin),
            ("clipboard", &file),
            ("Recycle Bin", &file),
        ] {
            let what = what.display().to_string();
            assert!(
                lines.iter().any(|l| l.contains(asked) && l.contains(&what)),
                "nothing says the {asked} stand-in was asked for {what}: {lines:?}"
            );
        }
    }

    #[test]
    fn a_file_dialog_answers_what_a_script_named_and_otherwise_cancels() {
        let named = PathBuf::from("C:/scratch/answer.glb");
        let shown = |_| -> Option<PathBuf> { panic!("the platform's own dialog was shown") };
        let mut answers = Vec::new();
        let lines = said(|| {
            file_dialogs::answer_next(named.clone());
            answers.push(file_dialogs::ask("Replace from glTF", shown));
            answers.push(file_dialogs::ask("Replace from glTF", shown));
        });
        assert_eq!(
            answers,
            [Some(named), None],
            "the named file, then a cancel"
        );
        assert_eq!(
            lines
                .iter()
                .filter(|l| l.contains("Replace from glTF"))
                .count(),
            2,
            "each dialog says it was asked: {lines:?}"
        );
    }
}
