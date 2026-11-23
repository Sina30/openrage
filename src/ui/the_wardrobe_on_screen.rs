//! The Wardrobe as it is drawn: a clothing project's garments and the drop that fills them, Undo, and the
//! questions Fix and Waive ask, read through AccessKit from whole frames of the shell and clicked where the names are.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::every_control_has_a_name_a_screen_reader_can_speak::mute;
use super::frames_of_the_shell::Shell;
use crate::state::front_door::{NewProject, Template};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;

const C: &str = "mp_m_freemode_01_mp_m_demo";

/// A shell with the project the Clothing pack template makes open on its Wardrobe.
fn a_clothing_shell(dir: &Path) -> Shell {
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("demo".into());
    let file = draft.create().expect("a clothing project").file;
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.settled();
    shell
}

fn names(shell: &Shell) -> Vec<String> {
    shell
        .described
        .iter()
        .filter_map(|widget| widget.name.clone())
        .collect()
}

fn shows(shell: &Shell, text: &str) -> bool {
    names(shell).iter().any(|name| name.contains(text))
}

/// The pack was read, holds no garment, and the frame shows its one drop area.
fn an_empty_pack_is_drawn(shell: &Shell) -> bool {
    (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .is_some_and(|read| read.wardrobe.garments.is_empty() && read.findings.is_empty())
        && shows(shell, "Drop models, textures and cloth here")
}

/// Draw frames, polling what the workers left, until `done`.
fn until(shell: &mut Shell, what: &str, done: impl Fn(&Shell) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        shell.state.poll_workbench();
        shell.draw(Vec::new());
        if done(shell) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{what} never happened: {:?}",
            names(shell)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Click the widget called `name`, after frames that let what a poll just changed settle.
///
/// ⚠ A click lands on what the pass before it registered, and in the pass in which a toast first shows, egui lays its area out as a sizing pass: a 600 by 400 box that senses clicks, over the panel under it.
fn click_on(shell: &mut Shell, name: &str) {
    shell.settled();
    let widget = shell
        .described
        .iter()
        .find(|w| w.name.as_deref() == Some(name))
        .or_else(|| {
            (shell.described.iter()).find(|w| w.name.as_deref().is_some_and(|n| n.contains(name)))
        })
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)));
    let at = widget.bounds.center();
    shell.click(at);
    shell.settled();
}

fn files(dir: &Path, list: &[(&str, &[u8])]) -> Vec<PathBuf> {
    list.iter()
        .map(|(name, body)| {
            let path = dir.join("drop").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, or_source::sample::as_a_game_file(name, body)).unwrap();
            path
        })
        .collect()
}

#[test]
fn a_clothing_project_opens_on_a_wardrobe_that_offers_the_drop_and_names_its_controls() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", an_empty_pack_is_drawn);

    assert_eq!(shell.state.active_workspace, Workspace::Wardrobe);
    for text in [
        "Add clothes…",
        "Drop models, textures and cloth here",
        "Wardrobe",
    ] {
        assert!(shows(&shell, text), "the Wardrobe lacks `{text}`");
    }
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the Wardrobe of an empty pack", &mut found);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn clothes_dropped_where_the_pointer_is_become_garments_and_undo_is_a_click_away() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", an_empty_pack_is_drawn);
    let dropped = files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
            (&format!("{C}^lowr_000_r.ydd"), b"trousers"),
            ("my hat.ydd", b"a hat"),
        ],
    );
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("the panel was drawn");

    shell.drop_files(panel.center(), dropped);

    until(&mut shell, "the garments", |s| {
        shows(s, "Tops") && shows(s, "Legs")
    });
    for text in [
        "Male · Tops · 1 garment",
        "Male · Legs · 1 garment",
        "? Needs a ped or a slot",
        "my hat",
        "? 2 need attention",
        "Undo add (3)",
    ] {
        assert!(
            shows(&shell, text),
            "after the drop: `{text}` is not on screen"
        );
    }
    // The toast says what was done, with its own Undo.
    assert!(
        shell.described.iter().any(|w| {
            w.role == egui::accesskit::Role::Button && w.name.as_deref() == Some("Undo")
        }),
        "{:?}",
        names(&shell)
    );

    click_on(&mut shell, "Undo add (3)");
    // The scan that reads the pack the undo wrote can come while the worker is still moving the dropped files aside, and the sources it finds half moved are rows too.
    until(&mut shell, "the undo", |s| {
        an_empty_pack_is_drawn(s)
            && s.state.wardrobe_is_idle()
            && s.state.workbench.scan_is_current()
    });
    assert!(
        !shows(&shell, "my hat"),
        "the undone drop still shows `my hat` in {:?}",
        names(&shell)
            .into_iter()
            .filter(|name| name.contains("my hat"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_finding_is_a_row_with_fix_and_waive_and_the_question_says_what_the_fix_changes() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", an_empty_pack_is_drawn);
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    let dropped = files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"one jacket"),
            (&format!("{C}^jbib_001_u.ydd"), b"one jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"a"),
            (&format!("{C}^jbib_diff_001_a_uni.ytd"), b"a"),
        ],
    );
    shell.drop_files(panel.center(), dropped);
    until(&mut shell, "the duplicate", |s| {
        shows(s, "Ships the same model as")
    });

    // Problems is along the bottom of the Wardrobe, with the row's two buttons.
    assert!(
        shows(&shell, "Fix…") && shows(&shell, "Waive…"),
        "{:?}",
        names(&shell)
    );
    click_on(&mut shell, "Fix…");
    assert!(
        shows(&shell, "This changes exactly:"),
        "{:?}",
        names(&shell)
    );
    assert!(
        shows(&shell, "from the pack"),
        "it says what it changes: {:?}",
        names(&shell)
    );
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the Fix question", &mut found);
    assert!(found.is_empty(), "{found:?}");

    click_on(&mut shell, "Cancel");
    assert!(!shows(&shell, "This changes exactly:"));
    click_on(&mut shell, "Waive…");
    assert!(
        shows(&shell, "Reason for the waiver"),
        "{:?}",
        names(&shell)
    );
    // The window says what is being decided, at the size of any other sentence, and where the reason goes.
    assert!(shows(&shell, "Waive this warning"), "{:?}", names(&shell));
    assert!(
        shows(
            &shell,
            "The build still goes ahead. Your reason is saved in the pack"
        ),
        "{:?}",
        names(&shell)
    );
    assert!(
        !shows(&shell, "wardrobe.json"),
        "a file name is nothing a person asked about: {:?}",
        names(&shell)
    );
    mute(&shell, "the Waive question", &mut found);
    assert!(found.is_empty(), "{found:?}");
}

/// A shell with the project `police` open, which links the pack of the project `brand`; the file of `brand` with it.
pub(super) fn a_shell_with_a_linked_pack(dir: &Path) -> (Shell, PathBuf) {
    let mut brand = NewProject::new(Template::Clothing, dir.to_path_buf());
    brand.set_name("brand".into());
    let owner = brand.create().expect("the owner is made").file;
    let pack = owner.parent().unwrap().join("clothing");
    let dropped = files(
        dir,
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
        ],
    );
    or_wardrobe::edit::add_clothes(&pack, &dropped).expect("the owner adds clothes");

    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut police = NewProject::new(Template::Files, work);
    police.set_name("police".into());
    let file = police.create().expect("the linking project is made").file;
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.state.link_content(&pack).expect("the pack is linked");
    shell.state.active_workspace = Workspace::Wardrobe;
    shell.settled();
    (shell, owner)
}

#[test]
fn a_linked_pack_is_read_in_the_wardrobe_with_its_project_named_and_nothing_to_add_or_drop() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, owner) = a_shell_with_a_linked_pack(dir.path());
    until(&mut shell, "the linked pack", |s| {
        shows(s, "Linked from brand.openrage-proj")
    });

    assert!(shows(&shell, "Open project"));
    assert!(
        shows(&shell, "Male · Tops · 1 garment"),
        "the garments are read: {:?}",
        names(&shell)
    );
    assert!(
        !shows(&shell, "Drop models, textures and cloth here"),
        "a linked pack takes no drop"
    );
    let add = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Add clothes…"))
        .expect("the header still names the command");
    assert!(
        add.disabled,
        "Add clothes… runs on a pack another project writes"
    );
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the Wardrobe of a linked pack", &mut found);
    assert!(found.is_empty(), "{found:?}");

    // A drop on the panel is refused with the project that writes the pack, and the pack keeps what it had.
    let held = std::fs::read(
        owner
            .parent()
            .unwrap()
            .join("clothing")
            .join("wardrobe.json"),
    )
    .unwrap();
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    let dropped = files(dir.path(), &[(&format!("{C}^lowr_000_r.ydd"), b"trousers")]);
    shell.drop_files(panel.center(), dropped);
    until(&mut shell, "the refusal", |s| {
        s.state.toasts.iter().any(|t| {
            t.message.contains("brand.openrage-proj") && t.message.contains("only reads it")
        })
    });
    assert_eq!(
        std::fs::read(
            owner
                .parent()
                .unwrap()
                .join("clothing")
                .join("wardrobe.json")
        )
        .unwrap(),
        held
    );

    click_on(&mut shell, "Open project");
    assert!(
        crate::platform::new_window::fake::was_opened(&owner),
        "the project that edits the pack opens in a window of its own"
    );
}

#[test]
fn the_clothing_chip_leads_through_the_dialog_to_a_wardrobe_ready_for_a_drop() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.documents.clear();
    shell.state.active_document = None;
    shell.state.active_workspace = Workspace::Browse;
    shell.settled();

    click_on(&mut shell, "Clothing pack");
    let draft = shell
        .state
        .front_door
        .new_project
        .as_ref()
        .expect("the dialog is open");
    assert_eq!(draft.template, Template::Clothing);
    // The chip chose the template, so the dialog opens on the name and the folder, and says clothing pack.
    assert_eq!(draft.step, crate::state::front_door::Step::Details);
    let draft = shell.state.front_door.new_project.as_mut().expect("open");
    draft.place = dir.path().display().to_string();
    shell.settled();
    shell.draw(vec![egui::Event::Text("harbor".into())]);
    shell.settled();
    for line in [
        "harbor/",
        "sources/",
        "clothing/",
        "wardrobe.json",
        "harbor.openrage-proj",
    ] {
        assert!(
            shows(&shell, line),
            "the tree lacks `{line}`: {:?}",
            names(&shell)
        );
    }
    assert!(
        !dir.path().join("harbor").exists(),
        "the dialog wrote something before Create"
    );

    click_on(&mut shell, "Create");
    // The project is made on a worker, and opens in the frame after it has answered.
    until(&mut shell, "the project", |s| {
        s.state.project_path.is_some()
    });
    assert_eq!(shell.state.active_workspace, Workspace::Wardrobe);
    until(&mut shell, "the empty pack", an_empty_pack_is_drawn);
    assert!(dir.path().join("harbor/clothing/wardrobe.json").is_file());
}

/// The first garment of the clothing the shell shows: its content and its id.
fn first_garment(shell: &Shell) -> (String, String) {
    let module = shell
        .state
        .wardrobe_module_id()
        .expect("a clothing content");
    let id = (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|read| read.wardrobe.garments.first())
        .map(|g| g.id.clone())
        .expect("a garment was read");
    (module, id)
}

#[test]
fn a_second_clothing_project_is_drawn_from_its_own_pack_after_a_first_that_had_no_findings() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut first = NewProject::new(Template::Clothing, dir.path().to_path_buf());
    first.set_name("first".into());
    let first = first.create().expect("the first project").file;
    let dropped = files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
            (&format!("{C}^lowr_000_u.ydd"), b"trousers"),
            (&format!("{C}^lowr_diff_000_a_uni.ytd"), b"blue"),
        ],
    );
    or_wardrobe::edit::add_clothes(&first.parent().unwrap().join("clothing"), &dropped)
        .expect("the first pack is filled");
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(first);
    shell.settled();
    until(&mut shell, "the first pack", |s| {
        shows(s, "Male · Tops · 1 garment")
    });
    assert!(
        shell.state.workbench.findings().is_empty(),
        "the first pack must be clean for this to be the case: {:?}",
        shell.state.workbench.findings()
    );

    shell.state.close_project();
    let mut second = NewProject::new(Template::Clothing, dir.path().to_path_buf());
    second.set_name("second".into());
    let second = second.create().expect("the second project").file;
    shell.state.load_project_from(second);
    shell.settled();

    // The empty pack is drawn as itself, and the first pack's lines are not drawn against it.
    until(&mut shell, "the second pack", an_empty_pack_is_drawn);
    assert!(!shows(&shell, "Male · Tops"), "{:?}", names(&shell));
}

#[test]
fn the_filter_for_garments_that_need_a_ped_is_cleared_once_none_does() {
    use or_wardrobe::edit::Edit;
    use or_wardrobe::{Component, Ped, Slot};

    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", an_empty_pack_is_drawn);
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    let jacket = files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
        ],
    );
    shell.drop_files(panel.center(), jacket);
    until(&mut shell, "the jacket", |s| shows(s, "Tops"));
    // The hat comes alone, so that its texture goes with it and a ped and a slot are all it lacks.
    let hat = files(
        &dir.path().join("hat"),
        &[("my hat.ydd", b"a hat"), ("a straw weave.ytd", b"straw")],
    );
    shell.drop_files(panel.center(), hat);
    until(&mut shell, "the hat", |s| shows(s, "? 1 needs attention"));

    click_on(&mut shell, "? 1 needs attention");
    until(&mut shell, "the filter", |s| !shows(s, "Male · Tops"));
    assert!(shows(&shell, "my hat"), "{:?}", names(&shell));

    // The hat is given its ped and its slot in Garment, and nothing is left that the filter is for.
    let hat = (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|read| read.wardrobe.garments.iter().find(|g| g.ped.is_none()))
        .map(|g| g.id.clone())
        .expect("the hat names neither a ped nor a slot");
    shell
        .state
        .edit_garment(hat.clone(), Edit::Ped(Some(Ped::Male)));
    shell
        .state
        .edit_garment(hat, Edit::Slot(Some(Slot::Component(Component::Jbib))));
    until(&mut shell, "the hat's seat", |s| {
        (s.state.wardrobe_pack())
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|read| {
                (read.wardrobe.garments.iter()).all(|g| g.ped.is_some() && g.slot.is_some())
            })
    });
    shell.settled();

    assert!(
        !shows(&shell, "No garment matches what is on"),
        "the filter outlived the garments it is for: {:?}",
        names(&shell)
    );
    assert!(
        shows(&shell, "Male · Tops · 2 garments"),
        "{:?}",
        names(&shell)
    );
}

#[test]
fn a_garment_selected_in_the_wardrobe_is_drawn_with_a_name_on_every_control_that_edits_it() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", an_empty_pack_is_drawn);
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    let dropped = files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
        ],
    );
    shell.drop_files(panel.center(), dropped);
    until(&mut shell, "the garment", |s| shows(s, "Tops"));

    let (module, id) = first_garment(&shell);
    shell.state.show_garment(&module, id);
    shell.settled();

    for control in [
        "Garment name",
        "Garment ped",
        "Garment slot",
        "Garment skin",
        "Pin at",
        "Position",
        "Remove from the pack",
    ] {
        assert!(
            shows(&shell, control),
            "Garment lacks `{control}`: {:?}",
            names(&shell)
        );
    }
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the Wardrobe with a garment selected", &mut found);
    assert!(found.is_empty(), "{found:?}");
}

const SHOP: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ShopPedApparel>\n  <pedName>mp_m_freemode_01</pedName>\n  <dlcName>demo</dlcName>\n  <pedComponents>\n    <Item>\n      <cost value=\"250\"/>\n      <uniqueNameHash>7F3A9B21C0D4</uniqueNameHash>\n      <localDrawableIndex value=\"0\"/>\n      <eCompType>PV_COMP_JBIB</eCompType>\n      <textureIndex value=\"0\"/>\n    </Item>\n  </pedComponents>\n  <pedProps/>\n</ShopPedApparel>\n";

#[test]
fn the_cloth_and_shop_sections_of_a_garment_are_drawn_with_names_and_link_and_unlink_are_clicks() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", an_empty_pack_is_drawn);
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    let dropped = files(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
            ("shop.meta", SHOP.as_bytes()),
        ],
    );
    shell.drop_files(panel.center(), dropped);
    until(&mut shell, "the garment and its shop id", |s| {
        shows(s, "Tops")
            && s.state
                .workbench
                .history()
                .iter()
                .any(|e| e.text.contains("became the ids"))
    });
    let (module, id) = first_garment(&shell);
    shell.state.show_garment(&module, id);
    shell.settled();

    // A garment with no cloth says so; its shop id is the one the file stated, and may be given another.
    for text in [
        "No cloth",
        "Link cloth…",
        "Unlink cloth",
        "7F3A9B21C0D4",
        "read from shop.meta, entry 1, cost 250",
        "Shop id of texture a",
    ] {
        assert!(
            shows(&shell, text),
            "Garment lacks `{text}`: {:?}",
            names(&shell)
        );
    }
    let unlink = (shell.described.iter()).find(|w| w.name.as_deref() == Some("Unlink cloth"));
    assert!(
        unlink.expect("the button is drawn").disabled,
        "a garment with no cloth has nothing to unlink"
    );
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the Cloth and Shop sections", &mut found);
    assert!(found.is_empty(), "{found:?}");

    let cloth = dir.path().join("elsewhere").join("jacket.yld");
    std::fs::create_dir_all(cloth.parent().unwrap()).unwrap();
    std::fs::write(&cloth, b"cloth").unwrap();
    // Link cloth… opens the picker; Browse… takes a file of elsewhere, which asks before it is imported.
    crate::platform::file_dialogs::answer_next(cloth);
    click_on(&mut shell, "Link cloth…");
    until(&mut shell, "the picker", |s| shows(s, "Choose the cloth"));
    click_on(&mut shell, "Browse…");
    until(&mut shell, "the question", |s| {
        shows(s, "Only reference it")
    });
    click_on(&mut shell, "Import into the project");
    until(&mut shell, "the link", |s| {
        shows(s, "Linked: cloth/jacket.yld")
    });
    assert!(shows(&shell, "Undo cloth"), "{:?}", names(&shell));
    mute(&shell, "the Garment with a cloth linked", &mut found);
    assert!(found.is_empty(), "{found:?}");

    click_on(&mut shell, "Unlink cloth");
    until(&mut shell, "the unlink", |s| shows(s, "No cloth"));
}
