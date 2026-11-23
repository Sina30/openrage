use std::time::{Duration, Instant};

use or_io::gta6::hash::name_hash64;

use super::*;
use crate::state::gta6_edit::tests::dictionary_file;
use crate::state::gta6_resource::texture_name;
use crate::state::opening_a_ttpf::{drawable_keyed, drawn};
use crate::state::texture_resolution;

#[test]
fn the_index_keys_each_texture_that_reads_by_the_body_that_holds_it_and_stops_when_asked() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (body, _) = dictionary_file(dir.path());
    // A path no archive is behind reads as no body, and holds no key.
    let bodies = vec![dir.path().join("0x40.rsc"), body.clone()];
    let progress = or_job::Progress::new();
    let built = index(bodies.clone(), &or_job::Cancel::new(), &progress).expect("not stopped");
    assert_eq!(built.bodies, [body]);
    assert_eq!(
        built.keys,
        HashMap::from([(name_hash64(b"zq_paint"), 0)]),
        "the one texture, by its name's key"
    );
    let stopped = or_job::Cancel::new();
    stopped.set();
    assert!(index(bodies, &stopped, &progress).is_none());
}

#[test]
fn a_drawable_s_key_opens_the_scanned_dictionary_that_holds_it_beside_the_drawable() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (body, _) = dictionary_file(dir.path());
    let key = name_hash64(b"zq_paint");
    let built = index(
        vec![body.clone()],
        &or_job::Cancel::new(),
        &or_job::Progress::new(),
    );
    let mut state = AppState::new();
    let (doc, _) = drawn(&mut state, dir.path(), drawable_keyed(key));
    let name = texture_name(key);
    let ask = |state: &mut AppState| {
        state.texture_report = texture_resolution::report(state, None, [(None, name.clone())], 1);
        state.try_open_texture_dictionary()
    };
    assert!(!ask(&mut state), "no index, no dictionary");
    assert!(texture_resolution::resolve(&state, None, None, &name).is_none());

    *state
        .texture_packs
        .dictionaries
        .built
        .lock()
        .expect("unlocked") = built;
    let until = Instant::now() + Duration::from_secs(30);
    while !ask(&mut state) {
        assert!(Instant::now() < until, "the dictionary never opened");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        state.active_document,
        Some(doc),
        "the drawable stays active"
    );
    let opened = (state.documents.iter())
        .find(|d| d.path.as_deref() == Some(body.as_path()))
        .expect("the dictionary that holds the key is open");
    assert!(crate::state::gta6_edit::is_scanned_dictionary(opened));
    let found = texture_resolution::resolve(&state, None, None, &name)
        .expect("the drawable's key resolves to the dictionary opened for it");
    assert_eq!(found.origin.document, opened.id);
    assert!(!ask(&mut state), "a key held opens nothing more");
    let dictionaries = (state.documents.iter()).filter(|d| d.path.as_deref() == Some(&*body));
    assert_eq!(dictionaries.count(), 1);
}
