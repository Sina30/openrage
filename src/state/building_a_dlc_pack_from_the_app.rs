//! A folder packs from its row's menu into the pack `openrage-cli dlc create`
//! builds of it, after a question that shows, before anything is written,
//! what the command packs and refuses and the line `dlclist.xml` needs.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use or_io::container::rpf::{NoOpCryptoProvider, RpfEncryption};

use super::jobs::JobKind;
use super::the_files_view_in_the_centre::{
    accessible, click, click_with, find, frame, leaf_of, listed, settled,
};
use super::{AppState, EditMode, Game, Prefs};
use crate::ui::panels::registry::ids;

const DEADLINE: Duration = Duration::from_secs(60);
const LINE: &str = "<Item>dlcpacks:/mypack/</Item>";

/// A pack's source, `<scratch>/work/mypack_src`: an archive, an archetype file and `more`.
fn staged(scratch: &Path, more: &[(&str, &[u8])]) -> PathBuf {
    let source = scratch.join("work").join("mypack_src");
    let shipped: [(&str, &[u8]); 2] = [
        ("x64/levels/gta5/props/models.rpf", &[7; 3000]),
        ("x64/levels/gta5/props/props.ityp", b"archetypes"),
    ];
    for (rel, bytes) in shipped.iter().chain(more) {
        let path = source.join(rel);
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("a folder");
        std::fs::write(&path, bytes).expect("a file");
    }
    source
}

/// What `openrage-cli dlc create` builds of `source` into `out` as `mypack`, with its defaults.
fn clis_pack(source: &Path, out: &Path) -> Vec<u8> {
    let words: Vec<String> = ["dlc", "create"]
        .map(String::from)
        .into_iter()
        .chain([source, out].map(|path| path.display().to_string()))
        .chain(["--name", "mypack"].map(String::from))
        .collect();
    let _ = or_cli::run(&words);
    std::fs::read(out.join("mypack").join("dlc.rpf")).expect("the command builds the pack")
}

/// `folder` open in Browse, its rows listed in the Files view.
fn browsing(folder: &Path) -> (AppState, egui::Context) {
    let mut state = AppState::from_prefs(Prefs::default());
    state.prefs.explorer = Default::default();
    state.open_folder(folder.to_path_buf());
    let doc = state.active_document.expect("the folder opens");
    let ctx = accessible();
    let until = Instant::now() + DEADLINE;
    while listed(&state, doc).is_empty() && Instant::now() < until {
        state.poll_expansions();
        frame(&ctx, &mut state, Vec::new());
    }
    (state, ctx)
}

/// Every text the screen shows, once it has settled.
fn shown(ctx: &egui::Context, state: &mut AppState) -> Vec<String> {
    (settled(ctx, state).into_iter())
        .map(|(text, _)| text)
        .collect()
}

/// Opens row `name`'s menu in the Files view and clicks Create DLC pack… in it.
fn ask_from_the_menu(ctx: &egui::Context, state: &mut AppState, name: &str) -> Vec<String> {
    let rows = settled(ctx, state);
    let row = find(&rows, leaf_of(state, ids::DOCUMENT), name);
    click_with(ctx, state, row, egui::PointerButton::Secondary);
    let menu = frame(ctx, state, Vec::new());
    click(
        ctx,
        state,
        find(&menu, egui::Rect::EVERYTHING, "Create DLC pack…"),
    );
    shown(ctx, state)
}

/// Types `text` where the caret is, and returns what the screen shows then.
fn typed(ctx: &egui::Context, state: &mut AppState, text: &str) -> Vec<String> {
    frame(ctx, state, vec![egui::Event::Text(text.to_string())]);
    shown(ctx, state)
}

/// Each button the questions draw, and whether it can be clicked.
fn buttons(ctx: &egui::Context, state: &mut AppState) -> Vec<(String, bool)> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 900.0),
        )),
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| {
        crate::ui::panels::file_prompts::draw(ui.ctx(), state);
    });
    let Some(update) = output.platform_output.accesskit_update else {
        return Vec::new();
    };
    (update.nodes.iter())
        .filter(|(_, node)| node.role() == egui::accesskit::Role::Button)
        .filter_map(|(_, node)| Some((node.label()?.to_string(), !node.is_disabled())))
        .collect()
}

/// What a click on the button labelled `label` put on the clipboard.
fn copied_by(ctx: &egui::Context, state: &mut AppState, label: &str) -> Vec<String> {
    let at = find(&settled(ctx, state), egui::Rect::EVERYTHING, label);
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let mut copied = Vec::new();
    for events in [
        vec![egui::Event::PointerMoved(at), press(true)],
        vec![press(false)],
    ] {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            crate::ui::dock::draw(ui, state, None);
            crate::ui::panels::file_prompts::draw(ui.ctx(), state);
        });
        copied.extend(
            (output.platform_output.commands.into_iter()).filter_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text),
                _ => None,
            }),
        );
    }
    copied
}

fn built_by_the_job(state: &mut AppState) {
    let until = Instant::now() + DEADLINE;
    let building =
        |state: &AppState| (state.running_jobs().iter()).any(|job| job.kind == JobKind::DlcPack);
    while building(state) && Instant::now() < until {
        state.poll_jobs();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_folder_packs_from_its_menu_into_the_pack_the_command_builds_of_it() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let source = staged(scratch.path(), &[]);
    let clis = clis_pack(&source, &scratch.path().join("cli"));

    let (mut state, ctx) = browsing(&scratch.path().join("work"));
    let asked = ask_from_the_menu(&ctx, &mut state, "mypack_src");
    for line in [
        "x64/levels/gta5/props/models.rpf    RPF_FILE",
        "x64/levels/gta5/props/props.ityp    DLC_ITYP_REQUEST",
        "a pack needs a name; it becomes the folder, the device and the hash",
    ] {
        assert!(
            asked.iter().any(|text| text == line),
            "the question does not show `{line}`: {asked:?}"
        );
    }
    assert!(
        buttons(&ctx, &mut state).contains(&("Create…".to_string(), false)),
        "with no name, as the command has none, nothing can be written"
    );

    let named = typed(&ctx, &mut state, "mypack");
    assert!(
        named.iter().any(|text| text == LINE),
        "the question does not show the line dlclist.xml needs: {named:?}"
    );
    assert!(
        !named
            .iter()
            .any(|text| text.starts_with("a pack needs a name")),
        "a name typed answers the refusal: {named:?}"
    );
    assert!(
        buttons(&ctx, &mut state).contains(&("Create…".to_string(), true)),
        "a pack the command builds can be created"
    );
    let by_app = scratch.path().join("app");
    assert!(!by_app.exists(), "something was written before Create…");

    state.create_dlc_pack(by_app.clone());
    built_by_the_job(&mut state);
    let apps = std::fs::read(by_app.join("mypack").join("dlc.rpf")).expect("the app builds it");
    assert!(
        apps == clis,
        "the app built other bytes than the command, of the same folder"
    );

    let built = shown(&ctx, &mut state);
    assert!(
        built.iter().any(|text| text == LINE),
        "the built pack's window does not show its line: {built:?}"
    );
    let offered = buttons(&ctx, &mut state);
    for button in ["Copy line", "Open folder"] {
        assert!(
            offered.contains(&(button.to_string(), true)),
            "the built pack's window offers no {button}: {offered:?}"
        );
    }
    assert_eq!(
        copied_by(&ctx, &mut state, "Copy line"),
        vec![LINE.to_string()],
        "Copy line copies the line"
    );
}

#[test]
fn the_question_shows_what_the_command_refuses_and_offers_nothing_to_write() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let meta: (&str, &[u8]) = ("common/data/weapons.meta", b"<CWeaponInfoBlob />");
    let source = staged(scratch.path(), &[meta]);
    let spec = or_cli::dlc_pack_of_folder(&source, "mypack").expect("the folder lists");
    let never = scratch.path().join("never");
    let writer = or_title::dlc::build_pack(
        &spec,
        &never.join("dlc.rpf"),
        RpfEncryption::Open,
        Arc::new(NoOpCryptoProvider::new()),
    )
    .expect_err("the writer refuses a file whose type it cannot settle");
    let refused: Vec<String> = (writer.to_string().lines().skip(1))
        .map(|line| line.trim().to_string())
        .collect();
    let encrypted = or_cli::create_dlc_pack(&spec, &never, RpfEncryption::Aes, &mut |_| true)
        .expect_err("the command refuses an encrypted pack");
    assert!(!refused.is_empty(), "{writer}");

    let (mut state, ctx) = browsing(&scratch.path().join("work"));
    ask_from_the_menu(&ctx, &mut state, "mypack_src");
    let named = typed(&ctx, &mut state, "mypack");
    for why in &refused {
        assert!(
            named.contains(why),
            "the question does not show the writer's `{why}`: {named:?}"
        );
    }
    let offered = buttons(&ctx, &mut state);
    assert!(
        offered.contains(&("Create…".to_string(), false))
            && offered.contains(&("Cancel".to_string(), true)),
        "a refused pack offers nothing to write: {offered:?}"
    );

    let aes = find(
        &settled(&ctx, &mut state),
        egui::Rect::EVERYTHING,
        "AES (retail)",
    );
    click(&ctx, &mut state, aes);
    let chosen = shown(&ctx, &mut state);
    assert!(
        chosen.contains(&encrypted),
        "an encryption the command refuses is refused in its words: {chosen:?}"
    );
    assert!(!never.exists(), "a refusal wrote something");
}

/// ⚠ In the modes and with the base-game Yes that would let the gate write
/// there, so only the refusal stands between the pack and the install.
#[test]
fn a_pack_is_never_built_into_a_game_install() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let source = staged(scratch.path(), &[]);
    let install = scratch.path().join("GTAV");
    std::fs::create_dir_all(install.join("mods")).expect("an install");
    std::fs::write(install.join("gta5.exe"), b"an install marker").expect("a marker");
    let mut state = AppState::from_prefs(Prefs::default());
    (state.prefs.game_paths).set(Game::GtaVLegacy, Some(install.clone()));
    state.ask_dlc_pack(source);
    if let Some(asked) = state.dlc_pack.asked.as_mut() {
        asked.spec.name = "mypack".into();
        asked.recheck();
    }

    let dlcpacks = |root: &Path| root.join("update").join("x64").join("dlcpacks");
    for (mode, dest) in [
        (EditMode::Overlay, dlcpacks(&install.join("mods"))),
        (EditMode::Everything, dlcpacks(&install)),
    ] {
        state.prefs.edit_mode = mode;
        state.retail_edit_session_allow = true;
        state.create_dlc_pack(dest.clone());
        built_by_the_job(&mut state);
        assert!(
            state.dlc_pack.asked.is_some(),
            "{mode:?}: a refused build keeps its question, to choose another folder"
        );
        assert!(
            !dest.join("mypack").exists(),
            "{mode:?}: a pack was built into the install, at {}",
            dest.display()
        );
    }

    let outside = scratch.path().join("built");
    state.create_dlc_pack(outside.clone());
    built_by_the_job(&mut state);
    assert!(
        outside.join("mypack").join("dlc.rpf").is_file(),
        "outside the install the pack is built"
    );
}

/// Right-clicks the document row of `name` in the Files tree, which has its icon before the name where the address bar's step has none, and clicks Create DLC pack… in its menu.
fn ask_from_the_documents_own_row(ctx: &egui::Context, state: &mut AppState, name: &str) {
    let icon = |c: char| ('\u{e000}'..='\u{f8ff}').contains(&c);
    let rows = settled(ctx, state);
    let tree = leaf_of(state, ids::FILES);
    let row = (rows.iter())
        .find(|(text, at)| {
            text.starts_with(icon) && text.trim_start_matches(icon) == name && tree.contains(*at)
        })
        .map(|(_, at)| *at)
        .unwrap_or_else(|| panic!("no row of `{name}` in {tree:?}: {rows:?}"));
    click_with(ctx, state, row, egui::PointerButton::Secondary);
    let menu = frame(ctx, state, Vec::new());
    click(
        ctx,
        state,
        find(&menu, egui::Rect::EVERYTHING, "Create DLC pack…"),
    );
}

#[test]
fn a_folder_opened_with_open_folder_packs_from_its_own_row() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let source = staged(scratch.path(), &[]);

    let (mut state, ctx) = browsing(&source);
    ask_from_the_documents_own_row(&ctx, &mut state, "mypack_src");
    let asked = shown(&ctx, &mut state);
    assert!(
        asked
            .iter()
            .any(|text| text == "x64/levels/gta5/props/models.rpf    RPF_FILE"),
        "the question does not show what the folder opened packs: {asked:?}"
    );
    assert_eq!(
        state
            .dlc_pack
            .asked
            .as_ref()
            .map(|asked| asked.folder.clone()),
        Some(source),
        "the pack is not built of the folder opened"
    );
}

/// One frame of the dock and the command palette over it, with `events` as the user's input.
fn palette_frame(ctx: &egui::Context, state: &mut AppState, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1600.0, 900.0),
        )),
        events,
        ..Default::default()
    };
    let _ = ctx.run_ui(input, |ui| {
        crate::ui::dock::draw(ui, state, None);
        crate::ui::panels::command_palette::draw(ui.ctx(), state);
        crate::ui::panels::file_prompts::draw(ui.ctx(), state);
    });
}

#[test]
fn the_palette_packs_the_folder_opened_when_no_row_is_selected() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let source = staged(scratch.path(), &[]);

    let (mut state, ctx) = browsing(&source);
    state.selection.clear_all();
    assert!(
        state
            .selection
            .selected_ids(state.active_document.expect("open"))
            .is_empty(),
        "a row is selected, so nothing is tested"
    );
    crate::ui::panels::command_palette::open(&mut state);
    palette_frame(&ctx, &mut state, Vec::new());
    palette_frame(
        &ctx,
        &mut state,
        vec![egui::Event::Text("create dlc pack".to_string())],
    );
    let enter = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: Some(egui::Key::Enter),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    palette_frame(&ctx, &mut state, vec![enter]);

    assert_eq!(
        state
            .dlc_pack
            .asked
            .as_ref()
            .map(|asked| asked.folder.clone()),
        Some(source),
        "the palette's Create DLC pack… did not pack the folder opened"
    );
    let asked = shown(&ctx, &mut state);
    assert!(
        asked
            .iter()
            .any(|text| text == "x64/levels/gta5/props/models.rpf    RPF_FILE"),
        "the question does not show what the folder opened packs: {asked:?}"
    );
}

#[test]
fn the_dlc_packs_folder_picker_says_what_it_picks_a_folder_for() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    staged(scratch.path(), &[]);

    let (mut state, ctx) = browsing(&scratch.path().join("work"));
    ask_from_the_menu(&ctx, &mut state, "mypack_src");
    typed(&ctx, &mut state, "mypack");
    let create = find(
        &settled(&ctx, &mut state),
        egui::Rect::EVERYTHING,
        "Create…",
    );
    crate::platform::file_dialogs::answer_next(scratch.path().join("app"));
    let said = crate::platform::tests::said(|| click(&ctx, &mut state, create));
    built_by_the_job(&mut state);
    assert!(
        said.iter()
            .any(|line| line.contains("the dialog \"Create DLC Pack Into Folder\"")),
        "the folder is asked for as a DLC pack's: {said:?}"
    );
}
