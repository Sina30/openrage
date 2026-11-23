//! A source taken from the game says whether the game's file is still the one it was taken from.

use std::cell::Cell;

use or_source::asset::{sha256_hex, AssetManifest, How, Origin};
use or_source::origin_check::{check, Installed, Verdict, CHANGED};

/// An edit of the origin a case starts from.
type Edit = Box<dyn FnOnce(&mut Origin)>;

const PATH: &str = "x64v.rpf/models/lamp.ydr";

fn taken_from(original: &[u8], edit: impl FnOnce(&mut Origin)) -> AssetManifest {
    let mut origin = Origin {
        how: How::Extracted,
        from_game: true,
        virtual_path: Some(PATH.to_string()),
        game_build: None,
        sha256: Some(sha256_hex(original)),
        cooked_sha256: None,
        extra: Default::default(),
    };
    edit(&mut origin);
    AssetManifest::new("drawable", "lamp", "gen8", origin).unwrap()
}

/// Checks `manifest` against an install that holds `held`, and says what the check asked it for.
fn against(manifest: &AssetManifest, held: Installed) -> (Verdict, Vec<String>) {
    let asked = Cell::new(Vec::new());
    let verdict = check(manifest, |path| {
        let mut so_far = asked.take();
        so_far.push(path.to_string());
        asked.set(so_far);
        held
    });
    (verdict, asked.take())
}

#[test]
fn the_file_the_source_was_taken_from_passes_and_the_install_was_asked_for_that_path_once() {
    let original = b"the original bytes".to_vec();
    let manifest = taken_from(&original, |_| {});

    let (verdict, asked) = against(&manifest, Installed::Found(original));

    assert_eq!(verdict, Verdict::Unchanged);
    assert_eq!(asked, [PATH]);
}

#[test]
fn a_file_whose_bytes_were_mutated_is_flagged_with_both_hashes_and_the_words_of_the_row() {
    let original = b"the original bytes".to_vec();
    let manifest = taken_from(&original, |_| {});
    let mut mutated = original.clone();
    mutated[3] ^= 1;

    let (verdict, _) = against(&manifest, Installed::Found(mutated.clone()));

    assert_eq!(
        verdict,
        Verdict::Changed {
            recorded: sha256_hex(&original),
            now: sha256_hex(&mutated),
        }
    );
    assert!(verdict.is_changed());
    assert!(
        verdict.sentence().starts_with(CHANGED),
        "{}",
        verdict.sentence()
    );
    assert_eq!(CHANGED, "the game's original changed");
}

#[test]
fn a_file_the_install_no_longer_holds_is_said_to_be_missing_and_is_not_a_change() {
    let manifest = taken_from(b"x", |_| {});

    let (verdict, _) = against(&manifest, Installed::Missing);

    assert_eq!(
        verdict,
        Verdict::Missing {
            path: PATH.to_string()
        }
    );
    assert!(!verdict.is_changed());
    assert!(verdict.sentence().contains(PATH));
}

#[test]
fn an_install_that_cannot_be_read_compares_nothing_and_says_why() {
    let manifest = taken_from(b"x", |_| {});

    let (verdict, _) = against(&manifest, Installed::Unreadable("keys are missing".into()));

    let Verdict::Unchecked(why) = verdict else {
        panic!("{verdict:?}");
    };
    assert!(why.contains("keys are missing"), "{why}");
}

#[test]
fn a_source_with_nothing_to_compare_never_touches_the_install() {
    let cases: [(&str, Edit); 3] = [
        ("not the game's", Box::new(|o| o.from_game = false)),
        ("no place in the game", Box::new(|o| o.virtual_path = None)),
        ("no hash", Box::new(|o| o.sha256 = None)),
    ];
    for (what, edit) in cases {
        let manifest = taken_from(b"x", edit);

        let (verdict, asked) = against(&manifest, Installed::Found(b"x".to_vec()));

        assert!(
            matches!(verdict, Verdict::Unchecked(_)),
            "{what}: {verdict:?}"
        );
        assert!(asked.is_empty(), "{what}: the install was read");
    }
}
