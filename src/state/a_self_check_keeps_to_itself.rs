//! A self-check plays a stream, starts a rebake, deletes a row and copies one
//! as the app does, and keeps to itself: no speaker sounds, no configuration
//! but the run's own is written, nothing stages in the editor's staging
//! folder, and each toast says what the stand-in did.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use or_io::container::rpf::crypto::ExeFingerprint;
use or_io::container::{Gta5Keys, KeysCache};

use super::every_add_lands_where_the_tree_shows_it::opened;
use super::opening_a_ydd::{finish_load, node_at};
use super::opening_an_awc::three_streams;
use super::the_files_view_in_the_centre::{accessible, click_with, find, frame, leaf_of, settled};
use super::{
    config_root, real_config_root, AppState, DocPayload, DocumentKind, ExtractionResult, Game,
    Prefs,
};
use crate::audio::{AudioSink, NullSink};
use crate::platform::{clipboard, recycle_bin};
use crate::ui::commands::{self, Action, Target};
use crate::ui::panels::registry::ids;

/// The state a self-check starts from: its scratch configuration holds no preferences.
fn a_self_check() -> AppState {
    AppState::from_prefs(Prefs::default())
}

/// `action` run as a click on it does, on `target` where it acts on something.
fn click(state: &mut AppState, action: Action, target: Option<Target>) {
    let command = match target {
        Some(target) => commands::on(state, action, target),
        None => commands::of(state, action),
    }
    .expect("a command");
    assert_eq!(command.enabled, Ok(()), "{action:?} is not offered");
    commands::run(state, &egui::Context::default(), &command);
}

/// The toasts `act` raised.
fn toasts(state: &mut AppState, act: impl FnOnce(&mut AppState)) -> Vec<String> {
    state.toasts.clear();
    act(state);
    state.toasts.iter().map(|t| t.message.clone()).collect()
}

/// When the manifest of the owner's pack of the game's shaders was written, which a bake writes last.
fn owners_pack_manifest() -> Vec<(PathBuf, Option<SystemTime>)> {
    let Some(owner) = real_config_root() else {
        return Vec::new();
    };
    let manifest = owner.join("shader-pack").join("game").join("manifest.json");
    let written = std::fs::metadata(&manifest).and_then(|m| m.modified()).ok();
    vec![(manifest, written)]
}

/// Keys and an install, as a self-check holds them once its `game` step has read them.
fn keys_for(state: &mut AppState, install: &Path) {
    let exe = install.join("GTA5.exe");
    state.prefs.gta_keys.gta5_path = Some(exe.clone());
    state
        .keys
        .spawn("Reading the keys", "Starting", 0, move |_, _| {
            let fingerprint = ExeFingerprint {
                sha1_hex: String::new(),
                size: 0,
            };
            ExtractionResult::Success {
                cache: KeysCache::build(exe, fingerprint, &Gta5Keys::empty()),
                keys: Arc::new(Gta5Keys::empty()),
            }
        });
    let deadline = Instant::now() + Duration::from_secs(30);
    while state.keys.poll().is_none() {
        assert!(Instant::now() < deadline, "the keys never arrived");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(state.keys.has_keys());
}

#[test]
fn a_self_checks_rebake_starts_nothing_and_writes_nothing() {
    let own = config_root().expect("the run's own configuration");
    let owners = owners_pack_manifest();
    let mut state = a_self_check();
    // ⚠ Asked before the rebake: until the bake has a stand-in, a rebake writes where the pack is.
    let pack = state.game_pack.root().to_path_buf();
    assert!(
        pack.starts_with(&own),
        "the game's shader pack is at {}, outside the run's own configuration {}",
        pack.display(),
        own.display()
    );

    let install = tempfile::tempdir().expect("an install");
    keys_for(&mut state, install.path());
    let lines = crate::platform::tests::said(|| click(&mut state, Action::BakeShaders, None));

    assert!(!state.game_pack.is_baking(), "a bake started");
    assert!(!pack.exists(), "a pack was baked into {}", pack.display());
    assert_eq!(
        owners_pack_manifest(),
        owners,
        "the owner's pack of the game's shaders changed"
    );
    let asked = install.path().display().to_string();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("stand-in") && l.contains(&asked)),
        "nothing says the bake's stand-in was asked: {lines:?}"
    );
}

#[test]
fn a_self_checks_stream_plays_to_a_silent_sink() {
    let mut state = a_self_check();
    // ⚠ Asked before anything plays: until the stand-in is in, a play sounds
    // on the owner's speakers.
    assert!(
        state.audio.is_silent(),
        "a self-check's audio goes to the output device"
    );
    let dir = tempfile::tempdir().expect("a scratch folder");
    let path = dir.path().join("sounds.awc");
    std::fs::write(&path, three_streams()).expect("the container");
    state.load_file(path);
    let doc = finish_load(&mut state).expect("the .awc opens");
    let stream = state.awc_streams(doc).expect("its streams")[0].stream;

    let lines = crate::platform::tests::said(|| {
        state.awc_play(doc, stream).expect("the stream plays");
    });
    assert_eq!(
        state.audio.playing(),
        Some((doc, stream)),
        "nothing says the stream plays"
    );
    assert!(
        lines.iter().any(|l| l.contains("speaker's stand-in")),
        "the speaker's stand-in does not say what it was asked: {lines:?}"
    );
}

#[test]
fn a_self_checks_delete_says_nothing_was_moved() {
    recycle_bin::fake::leave_in_place();
    let dir = tempfile::tempdir().expect("a scratch folder");
    let folder = dir.path().join("mods");
    std::fs::create_dir(&folder).expect("the folder");
    std::fs::write(folder.join("props.ydr"), b"props").expect("a file");
    let mut state = a_self_check();
    let doc = opened(&mut state, &folder);
    let row = node_at(&mut state, doc, "props.ydr");
    let target = Target::rows(&state, doc, vec![row], false).expect("a row of an open tree");

    let said = toasts(&mut state, |state| {
        click(state, Action::Delete, Some(target));
        state.answer_delete(true);
    });
    assert!(
        folder.join("props.ydr").is_file(),
        "the Recycle Bin's stand-in moved the file"
    );
    assert!(
        said.iter()
            .any(|t| t.contains("stand-in") && t.contains("props.ydr") && t.contains("nothing")),
        "no toast says what the Recycle Bin's stand-in was asked and did: {said:?}"
    );
}

fn entries_of(folder: &Path) -> BTreeSet<String> {
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

/// What `folder` gained since `before` under this process's id, which a staging folder is named for.
///
/// ⚠ Not every entry named for the id: Windows hands a dead process's id to a later one, and the
/// editor's own staging folder keeps the entries of every run it ever had.
fn staged_by_this_process_since(folder: &Path, before: &BTreeSet<String>) -> BTreeSet<String> {
    let ours = format!("{}-", std::process::id());
    let mut gained = entries_of(folder);
    gained.retain(|name| name.starts_with(&ours) && !before.contains(name));
    gained
}

#[test]
fn an_entry_a_dead_process_left_under_this_ones_id_is_not_what_this_one_staged() {
    let folder = tempfile::tempdir().expect("a staging folder");
    let id = std::process::id();
    let stale = format!("{id}-1");
    std::fs::create_dir(folder.path().join(&stale)).expect("a stale entry");
    let before = entries_of(folder.path());
    for name in [format!("{id}-2"), format!("{}-3", id + 4)] {
        std::fs::create_dir(folder.path().join(name)).expect("an entry");
    }

    let gained = staged_by_this_process_since(folder.path(), &before);
    assert_eq!(
        gained.into_iter().collect::<Vec<_>>(),
        [format!("{id}-2")],
        "the stale entry {stale} or another process's was counted"
    );
}

#[test]
fn a_self_checks_copy_stages_in_its_own_folder() {
    let own = config_root().expect("the run's own configuration");
    let shared = std::env::temp_dir().join("openrage-clipboard");
    let before = entries_of(&shared);
    let mut state = a_self_check();
    let doc = state.new_empty_document(DocumentKind::Rpf);
    let Some(DocPayload::Rpf(rpf)) = state.document_mut(doc).map(|d| &mut d.payload) else {
        panic!("an empty archive");
    };
    let root = rpf.session.root_id();
    let row = (rpf.session.add_file_copy(root, "a.ytd", vec![5; 9])).expect("a row");
    let target = Target::rows(&state, doc, vec![row], false).expect("a row of an open tree");

    let said = toasts(&mut state, |state| click(state, Action::Copy, Some(target)));
    let staged = clipboard::fake::offered();
    assert!(
        !staged.is_empty() && staged.iter().all(|p| p.starts_with(&own)),
        "the copy staged at {staged:?}, outside the run's own folder {}",
        own.display()
    );
    let in_shared = staged_by_this_process_since(&shared, &before);
    assert!(
        in_shared.is_empty(),
        "the copy staged in the editor's {}: {in_shared:?}",
        shared.display()
    );
    assert!(
        said.iter()
            .any(|t| t.contains("stand-in") && t.contains("a.ytd")),
        "no toast says the clipboard's stand-in holds the copy: {said:?}"
    );
}

#[test]
fn a_self_checks_first_launch_bake_asks_the_stand_in_once() {
    let owners = owners_pack_manifest();
    let mut state = a_self_check();
    let install = tempfile::tempdir().expect("an install");
    keys_for(&mut state, install.path());

    // ⚠ Twice, as the editor's every redraw asks.
    let lines = crate::platform::tests::said(|| {
        state.maybe_auto_bake_game_pack();
        state.maybe_auto_bake_game_pack();
    });

    let asked = install.path().display().to_string();
    let asks = lines
        .iter()
        .filter(|l| l.contains("stand-in") && l.contains(&asked))
        .count();
    assert_eq!(
        asks, 1,
        "the bake's stand-in was asked {asks} times: {lines:?}"
    );
    assert!(!state.game_pack.is_baking(), "a bake started");
    assert_eq!(
        owners_pack_manifest(),
        owners,
        "the owner's pack of the game's shaders changed"
    );
}

#[test]
fn a_silent_sink_keeps_only_the_stream_that_is_sounding() {
    let mut sink = NullSink::default();
    let heard = Arc::clone(&sink.heard);
    sink.play(32000, 1, vec![1; 4000]).expect("a first stream");
    sink.play(48000, 2, vec![2; 10]).expect("a second stream");
    assert_eq!(
        *heard.lock().expect("the sink's record"),
        vec![(48000, 2, vec![2; 10])],
        "the sink kept the stream it was told to replace"
    );

    sink.stop();
    assert!(
        heard.lock().expect("the sink's record").is_empty(),
        "the sink kept a stream that stopped"
    );
}

#[test]
fn a_scripts_open_of_a_folder_waits_for_its_rows_so_a_right_click_at_once_finds_the_menu() {
    let dir = tempfile::tempdir().expect("a folder");
    std::fs::write(dir.path().join("adder.yft"), b"not really").expect("a file");
    let mut state = a_self_check();
    state.prefs.explorer = Default::default();
    let ctx = accessible();

    // The frames a script draws poll as the editor's do; nothing here polls but them.
    crate::self_check::shots::open_and_wait(&mut state, dir.path(), |state| {
        state.poll_load_result();
        state.poll_expansions();
        frame(&ctx, state, Vec::new());
        Ok(())
    })
    .expect("the folder opens");

    let shown = settled(&ctx, &mut state);
    let row = find(&shown, leaf_of(&state, ids::DOCUMENT), "adder.yft");
    click_with(&ctx, &mut state, row, egui::PointerButton::Secondary);
    let menu = frame(&ctx, &mut state, Vec::new());
    for item in ["Open", "Properties", "Extract…"] {
        find(&menu, egui::Rect::EVERYTHING, item);
    }
}

#[test]
fn a_scripts_game_step_waits_for_the_installs_rows_so_a_right_click_at_once_finds_the_menu() {
    let install = tempfile::tempdir().expect("an install");
    std::fs::write(install.path().join("adder.yft"), b"not really").expect("a file");
    let mut state = a_self_check();
    let ctx = accessible();

    // The frames a script draws poll as the editor's do; nothing here polls but them.
    crate::self_check::shots::activate_game_and_wait(
        &mut state,
        Game::GtaVLegacy,
        install.path(),
        |state| {
            state.poll_load_result();
            state.poll_expansions();
            frame(&ctx, state, Vec::new());
            Ok(())
        },
    )
    .expect("the install opens");

    let shown = settled(&ctx, &mut state);
    let row = find(&shown, leaf_of(&state, ids::DOCUMENT), "adder.yft");
    click_with(&ctx, &mut state, row, egui::PointerButton::Secondary);
    let menu = frame(&ctx, &mut state, Vec::new());
    for item in ["Open", "Properties", "Extract…"] {
        find(&menu, egui::Rect::EVERYTHING, item);
    }
}
