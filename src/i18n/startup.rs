//! A launch reads the languages folder before anything asks for a text, even when saved preferences exist.
//!
//! The list of languages is read once per process, so the check runs a launch in a process of its own.

use crate::state::prefs::{keep_prefs_in, save_prefs, Prefs};
use crate::state::test_scratch::run_with_a_temporary_folder_of_its_own;
use crate::state::AppState;

const LAUNCH: &str = "OPENRAGE_I18N_STARTUP_LAUNCH";

#[test]
fn a_language_file_in_the_folder_is_listed_by_a_launch_that_has_saved_preferences() {
    if std::env::var_os(LAUNCH).is_none() {
        let module = module_path!().split_once("::").map_or("", |(_, rest)| rest);
        let test = format!(
            "{module}::a_language_file_in_the_folder_is_listed_by_a_launch_that_has_saved_preferences"
        );
        let (ran, left) = run_with_a_temporary_folder_of_its_own(&[test], &[(LAUNCH, "1")]);
        let said = String::from_utf8_lossy(&ran.stdout);
        assert!(
            ran.status.success() && said.contains("1 passed"),
            "the launch did not list the language of the folder: {said}{}",
            String::from_utf8_lossy(&ran.stderr)
        );
        assert!(left.is_empty(), "the launch left {left:?} behind");
        return;
    }

    let config = tempfile::tempdir().expect("a configuration folder");
    keep_prefs_in(config.path());
    let languages = config.path().join("languages");
    std::fs::create_dir(&languages).expect("the languages folder");
    std::fs::write(
        languages.join("xx.po"),
        "msgid \"\"\nmsgstr \"Language-Name: Xish\\n\"\n\nmsgid \"Dark\"\nmsgstr \"Xark\"\n",
    )
    .expect("a language file");
    std::fs::write(languages.join("fr.po"), "msgid \"Dark\"\nmsgstr \"Nuit\"\n")
        .expect("a file over the built-in French");
    // The preferences a person who has used the editor has: they hold a theme, which loading them reads.
    save_prefs(&Prefs::default());
    assert!(
        std::fs::read_to_string(config.path().join("prefs.json"))
            .expect("saved preferences")
            .contains("\"theme\""),
        "the preferences hold a theme"
    );

    let _launch = AppState::new();

    let codes: Vec<&str> = super::languages().iter().map(|c| c.code.as_str()).collect();
    assert_eq!(
        codes,
        ["en", "de", "es", "fr", "it", "ja", "ko", "pt", "ru", "zh", "xx"],
        "the folder's language is listed"
    );
    assert_eq!(
        super::languages()[3].translate("Dark"),
        Some("Nuit"),
        "the folder's file lies over the built-in French"
    );
}
