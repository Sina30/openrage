//! What a row's menu, the empty space's menu and Refresh do.

use super::*;

#[test]
fn refresh_reads_a_folder_on_disk_again() {
    let dir = tempfile::tempdir().expect("a folder");
    std::fs::write(dir.path().join("gone.txt"), b"soon gone").expect("a file");
    let mut state = AppState::from_prefs(Prefs::default());
    state.prefs.explorer = Default::default();
    state.open_folder(dir.path().to_path_buf());
    let doc = state.active_document.expect("the folder opens");
    let ctx = egui::Context::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let settle = |state: &mut AppState, want: &[&str]| {
        while listed(state, doc) != want && std::time::Instant::now() < deadline {
            state.poll_expansions();
            frame(&ctx, state, Vec::new());
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(listed(state, doc), want);
    };
    settle(&mut state, &["gone.txt"]);

    std::fs::remove_file(dir.path().join("gone.txt")).expect("removed behind the app's back");
    std::fs::write(dir.path().join("new.txt"), b"arrived").expect("a new file");
    let root = node(&state, doc, "");
    state
        .refresh_folder(doc, root)
        .expect("a folder on disk reads again");
    settle(&mut state, &["new.txt"]);
}

#[test]
fn a_rows_menu_shows_it_on_disk_copies_its_name_and_hash_and_opens_its_properties() {
    let dir = tempfile::tempdir().expect("a folder");
    std::fs::write(dir.path().join("adder.yft"), b"not really").expect("a file");
    let mut state = AppState::from_prefs(Prefs::default());
    state.prefs.explorer = Default::default();
    state.open_folder(dir.path().to_path_buf());
    let doc = state.active_document.expect("the folder opens");
    let ctx = accessible();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while listed(&state, doc).is_empty() && std::time::Instant::now() < deadline {
        state.poll_expansions();
        frame(&ctx, &mut state, Vec::new());
    }
    let shown = settled(&ctx, &mut state);
    let centre = leaf_of(&state, ids::DOCUMENT);
    let row = find(&shown, centre, "adder.yft");
    let open_menu = |state: &mut AppState| {
        click_with(&ctx, state, row, egui::PointerButton::Secondary);
        frame(&ctx, state, Vec::new())
    };

    let menu = open_menu(&mut state);
    for item in ["Open", "Export as…", "Properties"] {
        find(&menu, egui::Rect::EVERYTHING, item);
    }
    let hash = format!("Copy hash  0x{:08X}", or_io::hash::string_hash(b"adder"));
    // What is copied of a row is one row of its menu that opens them.
    let menu = submenu(&ctx, &mut state, &menu, "Copy as");
    click(&ctx, &mut state, find(&menu, egui::Rect::EVERYTHING, &hash));
    let copied = |output: &egui::FullOutput| {
        output
            .platform_output
            .commands
            .iter()
            .find_map(|c| match c {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
    };

    let menu = open_menu(&mut state);
    let menu = submenu(&ctx, &mut state, &menu, "Copy as");
    let at = find(&menu, egui::Rect::EVERYTHING, "Copy name");
    run(
        &ctx,
        &mut state,
        vec![
            egui::Event::PointerMoved(at),
            button(at, egui::PointerButton::Primary, true),
        ],
        egui::Modifiers::NONE,
    );
    let output = run(
        &ctx,
        &mut state,
        vec![button(at, egui::PointerButton::Primary, false)],
        egui::Modifiers::NONE,
    );
    assert_eq!(copied(&output).as_deref(), Some("adder.yft"));

    let menu = open_menu(&mut state);
    let reveal = menu
        .iter()
        .find(|(text, _)| text.starts_with("Reveal in "))
        .map(|(_, at)| *at)
        .expect("a file on disk can be shown in the file manager");
    click(&ctx, &mut state, reveal);
    assert!(crate::platform::shell::fake::was_revealed(
        &dir.path().join("adder.yft")
    ));

    let menu = open_menu(&mut state);
    click(
        &ctx,
        &mut state,
        find(&menu, egui::Rect::EVERYTHING, "Properties"),
    );
    let window = settled(&ctx, &mut state);
    for fact in [
        "Compression",
        "Encryption",
        "Edition",
        "Version",
        "Hash",
        "Path",
    ] {
        find(&window, egui::Rect::EVERYTHING, fact);
    }
    find(
        &window,
        egui::Rect::EVERYTHING,
        &format!("0x{:08X}", or_io::hash::string_hash(b"adder")),
    );
}

#[test]
fn new_folder_asks_for_its_name_in_place() {
    let (mut state, doc) = browsing();
    let ctx = accessible();
    settled(&ctx, &mut state);
    let at = in_the_listing(&state);
    click_with(&ctx, &mut state, at, egui::PointerButton::Secondary);
    let menu = frame(&ctx, &mut state, Vec::new());
    click(
        &ctx,
        &mut state,
        find(&menu, egui::Rect::EVERYTHING, "New folder"),
    );
    let made = node(&state, doc, "New folder");
    assert!(
        ui_state(&mut state).is_renaming_in_centre(doc, made),
        "the new folder's name is up for editing where it shows"
    );
    frame(&ctx, &mut state, Vec::new());
    frame(&ctx, &mut state, vec![egui::Event::Text("maps".into())]);
    let enter = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    frame(&ctx, &mut state, vec![enter]);
    frame(&ctx, &mut state, Vec::new());
    assert_eq!(name_of(&state, doc, made), "maps");
    assert!(listed(&state, doc).contains(&"maps".to_string()));
}

#[test]
fn export_as_writes_the_file_under_the_name_given() {
    let (mut state, doc) = browsing();
    let dir = tempfile::tempdir().expect("a folder");
    let notes = node(&state, doc, "notes.txt");
    let to = dir.path().join("renamed.txt");
    state.export_rpf_node_as(doc, notes, to.clone(), Some(false));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while state.extract_jobs.is_running() && std::time::Instant::now() < deadline {
        state.poll_extract_job();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(std::fs::read(&to).ok().as_deref(), Some(&b"a note"[..]));
    assert!(
        !dir.path().join("notes.txt").exists(),
        "under the name given, not its own"
    );
}

#[test]
fn a_rows_menu_puts_open_and_import_first_as_a_pair_and_the_ways_to_take_a_copy_out_together() {
    let dir = tempfile::tempdir().expect("a folder");
    std::fs::write(dir.path().join("adder.yft"), b"not really").expect("a file");
    let mut state = AppState::from_prefs(Prefs::default());
    state.prefs.explorer = Default::default();
    state.open_folder(dir.path().to_path_buf());
    let doc = state.active_document.expect("the folder opens");
    let ctx = accessible();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while listed(&state, doc).is_empty() && std::time::Instant::now() < deadline {
        state.poll_expansions();
        frame(&ctx, &mut state, Vec::new());
    }
    let shown = settled(&ctx, &mut state);
    let row = find(&shown, leaf_of(&state, ids::DOCUMENT), "adder.yft");
    click_with(&ctx, &mut state, row, egui::PointerButton::Secondary);
    let menu = frame(&ctx, &mut state, Vec::new());

    let row_at = |label: &str| find(&menu, egui::Rect::EVERYTHING, label).y;
    let (open, import) = (row_at("Open"), row_at("Import into project"));
    // ⚠ The two things to do with a game file, first and together: Open changes nothing, Import makes a source.
    for other in ["Reveal in the file manager", "Extract…", "Properties"] {
        assert!(
            open < row_at(other) && import < row_at(other),
            "`{other}` stands above Open or Import: {menu:?}"
        );
    }
    assert!(
        (import - open).abs() < 30.0,
        "Open and Import are a pair: {open} and {import}"
    );
    // The ways to take a copy out stand together under Extract….
    let (extract, export) = (row_at("Extract…"), row_at("Export as…"));
    assert!(
        export > extract && export - extract < 70.0,
        "Export as… is not beside Extract…: {extract} and {export}"
    );
    // The three comparisons are one row, and the menu fits the smallest window with room under it.
    assert!(
        menu.iter().any(|(text, _)| text.starts_with("Compare"))
            && !menu.iter().any(|(text, _)| text == "Select for compare"),
        "Compare is a submenu: {menu:?}"
    );
    assert!(
        !menu
            .iter()
            .any(|(text, _)| text.starts_with("Reveal in Windows")),
        "one name for the file manager, as the Project workspace says it"
    );
}
