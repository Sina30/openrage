//! The Wardrobe as a person meets it: an empty pack that is one drop area, rows that are one click target, chips that cover
//! what needs attention, the header that keeps the build with its button, the strip with its problems and ceilings, and
//! words with their plurals, read through AccessKit from whole frames of the shell and clicked where the names are.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::epaint::ClippedShape;

use super::frames_of_the_shell::Shell;
use crate::state::front_door::{NewProject, Template};
use crate::state::workbench::{Fix, Place, Question};
use crate::state::ThemeChoice;
use crate::ui::dock::Workspace;
use crate::ui::theme;

const C: &str = "mp_m_freemode_01_mp_m_demo";

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
    (shell.described.iter())
        .filter_map(|widget| widget.name.clone())
        .collect()
}

fn shows(shell: &Shell, text: &str) -> bool {
    names(shell).iter().any(|name| name.contains(text))
}

fn count(shell: &Shell, text: &str) -> usize {
    names(shell).iter().filter(|n| n.contains(text)).count()
}

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

fn garments(shell: &Shell) -> usize {
    (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .map_or(0, |read| read.wardrobe.garments.len())
}

fn the_pack_is_read(shell: &Shell) -> bool {
    (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .is_some()
}

fn put(dir: &Path, list: &[(&str, &[u8])]) -> Vec<PathBuf> {
    list.iter()
        .map(|(name, body)| {
            let path = dir.join("drop").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, or_source::sample::as_a_game_file(name, body)).unwrap();
            path
        })
        .collect()
}

/// The bounds of the first widget whose name has `text` in it.
fn bounds_containing(shell: &Shell, text: &str) -> egui::Rect {
    (shell.described.iter())
        .find(|w| w.name.as_deref().is_some_and(|n| n.contains(text)))
        .unwrap_or_else(|| panic!("nothing is named `{text}`: {:?}", names(shell)))
        .bounds
}

fn bounds_of(shell: &Shell, name: &str) -> egui::Rect {
    let named = |w: &&crate::ui::frames_of_the_shell::Described| {
        w.name.as_deref().is_some_and(|n| n.starts_with(name))
    };
    // A row's hover tooltip says the row's words as a label, and shows once enough frames have passed under the pointer: the row is the one that is not a label
    (shell.described.iter())
        .filter(named)
        .find(|w| w.role != egui::accesskit::Role::Label)
        .or_else(|| shell.described.iter().find(named))
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)))
        .bounds
}

fn pressed(at: egui::Pos2, button: egui::PointerButton) -> Vec<egui::Event> {
    let event = |pressed| egui::Event::PointerButton {
        pos: at,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    vec![egui::Event::PointerMoved(at), event(true), event(false)]
}

/// A pack of `n` jackets, each with a texture of its own, dropped on the Wardrobe and read.
fn with_jackets(shell: &mut Shell, dir: &Path, n: u32) {
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("the panel is drawn");
    let mut list: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..n {
        list.push((
            format!("{C}^jbib_{i:03}_u.ydd"),
            format!("jacket {i}").into_bytes(),
        ));
        list.push((
            format!("{C}^jbib_diff_{i:03}_a_uni.ytd"),
            format!("texture {i}").into_bytes(),
        ));
    }
    let refs: Vec<(&str, &[u8])> = list
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_slice()))
        .collect();
    shell.drop_files(panel.center(), put(dir, &refs));
    until(shell, "the jackets", |s| garments(s) == n as usize);
}

#[test]
fn an_empty_wardrobe_is_one_drop_area_with_one_sentence_and_one_primary_button() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", the_pack_is_read);
    shell.settled();

    assert_eq!(
        count(&shell, "Drop models, textures and cloth here"),
        1,
        "one sentence: {:?}",
        names(&shell)
    );
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    for text in [
        "Names say the ped",
        "No garments yet",
        "0 garments",
        "(s)",
        "pack `",
    ] {
        assert!(
            !(shell.described.iter())
                .filter(|w| panel.contains_rect(w.bounds))
                .any(|w| w.name.as_deref().is_some_and(|n| n.contains(text))),
            "an empty pack shows `{text}`: {:?}",
            names(&shell)
        );
    }
    // The two buttons are the header's, which is not primary while the pack is empty, and the drop area's, which is.
    assert_eq!(count(&shell, "Add clothes…"), 2, "{:?}", names(&shell));
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    let shapes = shell.draw(Vec::new());
    let primary: Vec<egui::Rect> = accent_buttons(&shapes)
        .into_iter()
        .filter(|rect| panel.contains_rect(*rect) && rect.height() < 40.0)
        .collect();
    assert_eq!(
        primary.len(),
        1,
        "one primary button in the panel: {primary:?}"
    );
    let area = bounds_containing(&shell, "Drop models, textures and cloth here");
    assert!(
        primary[0].min.y > area.min.y,
        "the button is in the drop area, under its sentence"
    );
}

/// The text of every galley the frame painted.
fn painted_text(shapes: &[ClippedShape]) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
            _ => {}
        }
    }
    let mut found = Vec::new();
    shapes
        .iter()
        .for_each(|clipped| walk(&clipped.shape, &mut found));
    found
}

/// The rectangles drawn filled with the accent, which is what a primary button is.
fn accent_buttons(shapes: &[ClippedShape]) -> Vec<egui::Rect> {
    fn walk(shape: &egui::Shape, out: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            egui::Shape::Rect(rect) if rect.fill == theme::ACCENT.get() => out.push(rect.rect),
            _ => {}
        }
    }
    let mut found = Vec::new();
    shapes
        .iter()
        .for_each(|clipped| walk(&clipped.shape, &mut found));
    found
}

#[test]
fn a_garments_row_is_one_click_target_and_a_right_click_selects_it_as_a_click_does() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", the_pack_is_read);
    with_jackets(&mut shell, dir.path(), 2);
    shell.settled();
    let id_at = |shell: &Shell, i: usize| {
        (shell.state.wardrobe_pack())
            .and_then(|p| p.read.as_ref().ok())
            .map(|read| read.wardrobe.garments[i].id.clone())
            .unwrap()
    };
    assert_eq!(shell.state.workbench.wardrobe.selected, None);

    // The far right of the row, over what it says about the textures, which is no label's own.
    let second = bounds_of(&shell, "001, place 001");
    shell.draw(pressed(
        egui::pos2(second.right() - 4.0, second.center().y),
        egui::PointerButton::Secondary,
    ));
    shell.settled();
    assert_eq!(
        shell
            .state
            .workbench
            .wardrobe
            .selected
            .as_ref()
            .map(|(_, id)| id.clone()),
        Some(id_at(&shell, 1)),
        "a right click on the row selects it"
    );

    // The far left, over the dot and the place, with a plain click.
    let first = bounds_of(&shell, "000, place 000");
    shell.click(egui::pos2(first.left() + 3.0, first.center().y));
    shell.settled();
    assert_eq!(
        shell
            .state
            .workbench
            .wardrobe
            .selected
            .as_ref()
            .map(|(_, id)| id.clone()),
        Some(id_at(&shell, 0)),
        "a click on the margin of the row selects it"
    );
}

#[test]
fn rows_are_named_by_what_the_pack_does_not_repeat_and_tops_come_first() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", the_pack_is_read);
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    let dropped = put(
        dir.path(),
        &[
            (&format!("{C}^lowr_000_u.ydd"), b"trousers"),
            (&format!("{C}^lowr_diff_000_a_uni.ytd"), b"denim"),
            (&format!("{C}^feet_000_u.ydd"), b"boots"),
            (&format!("{C}^feet_diff_000_a_uni.ytd"), b"leather"),
            (&format!("{C}^jbib_000_u.ydd"), b"jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
        ],
    );
    shell.drop_files(panel.center(), dropped);
    until(&mut shell, "the garments", |s| garments(s) == 3);
    shell.settled();

    for row in ["jbib, place 000", "lowr, place 000", "feet, place 000"] {
        assert!(shows(&shell, row), "no row `{row}`: {:?}", names(&shell));
    }
    assert!(
        !shows(&shell, "mp_m_freemode_01_mp_m_demo^jbib_000_u, place"),
        "a row does not say what every row says"
    );
    let top = |heading: &str| bounds_of(&shell, heading).min.y;
    assert!(
        top("Male · Tops") < top("Male · Legs") && top("Male · Legs") < top("Male · Shoes"),
        "Tops, then the others as the game numbers them: {:?}",
        names(&shell)
    );
}

#[test]
fn the_chip_for_what_needs_attention_covers_errors_and_duplicates_and_the_duplicate_chip_lists_the_copies(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", the_pack_is_read);
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    // A jacket, a copy of it, and trousers with no texture, which is an error.
    let dropped = put(
        dir.path(),
        &[
            (&format!("{C}^jbib_000_u.ydd"), b"the same jacket"),
            (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
            (&format!("{C}^jbib_001_u.ydd"), b"the same jacket"),
            (&format!("{C}^jbib_diff_001_a_uni.ytd"), b"red"),
            (&format!("{C}^lowr_000_u.ydd"), b"trousers"),
        ],
    );
    shell.drop_files(panel.center(), dropped);
    until(&mut shell, "the chips", |s| shows(s, "1 duplicate"));
    shell.settled();

    assert!(shows(&shell, "? 2 need attention"), "{:?}", names(&shell));
    assert!(
        shows(&shell, "3 garments · 2 textures"),
        "{:?}",
        names(&shell)
    );
    click_on(&mut shell, "1 duplicate");
    assert!(shows(&shell, "jbib_001, place 001"));
    assert!(
        !shows(&shell, "jbib_000, place 000"),
        "only the copy is listed"
    );
    assert!(!shows(&shell, "lowr_000, place 000"), "{:?}", names(&shell));

    click_on(&mut shell, "1 duplicate");
    click_on(&mut shell, "? 2 need attention");
    assert!(shows(&shell, "jbib_001, place 001") && shows(&shell, "lowr_000, place 000"));
    assert!(
        !shows(&shell, "jbib_000, place 000"),
        "the jacket that is fine is not listed: {:?}",
        names(&shell)
    );
}

fn click_on(shell: &mut Shell, name: &str) {
    let at = bounds_of(shell, name).center();
    shell.click(at);
    shell.settled();
}

#[test]
fn the_window_has_one_build_button_in_the_menu_bar_and_the_chooser_stays_under_the_list() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", the_pack_is_read);
    for size in [(1280, 720), (1024, 640)] {
        shell.settled_sized(size);
        let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
        let chooser = bounds_of(&shell, "Build the Build button makes");
        assert!(
            panel.contains_rect(chooser),
            "at {size:?} the chooser is in the panel {panel:?}, where a click reaches it: {chooser:?}"
        );
        // The button that builds starts with the hammer; the tab called Build is the title of a panel and builds nothing.
        let builds: Vec<egui::Rect> = (shell.described.iter())
            .filter(|w| w.role == egui::accesskit::Role::Button)
            .filter(|w| {
                (w.name.as_deref())
                    .is_some_and(|name| name.starts_with(egui_phosphor::regular::HAMMER))
            })
            .map(|w| w.bounds)
            .collect();
        assert_eq!(
            builds.len(),
            1,
            "at {size:?} the window has {} Build buttons: {builds:?}",
            builds.len()
        );
        assert!(
            builds[0].max.y < chooser.min.y && builds[0].max.y < 40.0,
            "at {size:?} the Build button is not in the menu bar: {:?}",
            builds[0]
        );
        let status_bar = size.1 as f32 - 24.0;
        assert!(
            !(shell.described.iter()).any(|w| {
                w.bounds.min.y >= status_bar
                    && w.name.as_deref().is_some_and(|name| name.contains("Build"))
            }),
            "at {size:?} the status bar still names a Build"
        );
    }
}

#[test]
fn clothing_no_build_ships_offers_add_a_build_as_a_button_and_not_as_a_sentence_to_follow() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", the_pack_is_read);
    shell.state.project_carried.container.outputs.clear();
    shell.settled();

    assert!(shows(&shell, "No build ships this clothing"));
    assert!(!shows(&shell, "Project ▸ Add build"), "{:?}", names(&shell));
    let button = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some("Add a build"))
        .unwrap_or_else(|| panic!("no Add a build button: {:?}", names(&shell)));
    assert_eq!(button.role, egui::accesskit::Role::Button);
    click_on(&mut shell, "Add a build");
    assert!(
        matches!(
            shell.state.workbench.asking,
            Some(Question::Builds {
                adding: Some(_),
                ..
            })
        ),
        "{:?}",
        shell.state.workbench.asking
    );
}

#[test]
fn the_pack_is_named_where_a_content_is_chosen_and_a_count_of_one_is_singular() {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", the_pack_is_read);
    with_jackets(&mut shell, dir.path(), 1);
    shell.settled();
    assert!(
        shows(&shell, "1 garment · 1 texture"),
        "{:?}",
        names(&shell)
    );

    shell.state.add_clothing_content();
    until(&mut shell, "the second pack", |s| {
        s.state.clothing_modules().len() == 2 && s.state.wardrobe_pack().is_some()
    });
    shell.settled();
    // A combo box's chosen text is painted and not described, so it is read from what was painted.
    let shapes = shell.draw(Vec::new());
    let said = painted_text(&shapes);
    assert!(
        said.iter().any(|n| n.contains("Pack: demo_2")),
        "the chooser says the pack, not the id of the content: {said:?}"
    );
    assert!(
        !said.iter().any(|n| n.contains("Clothing: clothing")),
        "{said:?}"
    );
}

#[test]
fn the_strip_says_how_many_problems_the_clothing_has_and_the_figures_that_have_a_ceiling_on_one_line(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", the_pack_is_read);
    {
        let output = &mut shell.state.project_carried.container.outputs[0];
        output.limits.insert("clothing_drawables".into(), 128);
        output.max_files = Some(500);
    }
    with_jackets(&mut shell, dir.path(), 2);
    until(&mut shell, "the plan", |s| shows(s, "Drawables: 2 of 128"));

    // The line holds the problems, the figures a build declares a ceiling for and the memory, never a figure with no ceiling to meet.
    assert!(shows(&shell, "No problems"), "{:?}", names(&shell));
    assert!(shows(&shell, "Estimated memory: "), "{:?}", names(&shell));
    assert!(
        names(&shell)
            .iter()
            .any(|n| n.contains("Files: ") && n.contains(" of 500")),
        "the plan's own ceiling on files: {:?}",
        names(&shell)
    );
    for quiet in ["Component drawables", "Prop drawables", "Heaviest outfit"] {
        assert!(
            !shows(&shell, quiet),
            "`{quiet}` has no ceiling and waits in the menu: {:?}",
            names(&shell)
        );
    }
    // One unit for every byte count, and no engine name.
    assert!(
        !shows(&shell, " MB") && !shows(&shell, "clothing_"),
        "{:?}",
        names(&shell)
    );

    // The drawables are told apart, the component slots' and the props', and the heaviest outfit from the whole pack, in the menu of every figure.
    click_on(&mut shell, "All figures");
    for figure in [
        "Component drawables: 2",
        "Prop drawables: 0",
        "Heaviest outfit: ",
        "Textures: 2",
        "Size: ",
    ] {
        assert!(shows(&shell, figure), "`{figure}`: {:?}", names(&shell));
    }
    // A figure of the plan is no sentence of the log: it names no 'clothing' and no underscore.
    assert!(!shows(&shell, "Clothing drawables"), "{:?}", names(&shell));
    shell.draw(vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    shell.settled();

    // A garment with no texture is an error, and the strip counts it.
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    shell.drop_files(
        panel.center(),
        put(dir.path(), &[(&format!("{C}^lowr_000_u.ydd"), b"trousers")]),
    );
    until(&mut shell, "the error", |s| shows(s, "1 error"));
    assert!(!shows(&shell, "No problems"));
}

#[test]
fn the_command_that_gives_a_project_a_pack_and_the_one_that_fills_it_have_labels_of_their_own() {
    use crate::ui::commands::{self, Action};
    let dir = tempfile::tempdir().unwrap();
    let shell = a_clothing_shell(dir.path());
    let label = |action| commands::of(&shell.state, action).map(|c| c.label.clone());
    let (pack, clothes) = (label(Action::AddClothing), label(Action::AddClothes));
    assert_eq!(pack.as_deref(), Some("Add a clothing pack…"));
    assert_eq!(clothes.as_deref(), Some("Add clothes…"));
    assert_ne!(pack, clothes);
}

#[test]
fn a_garment_panel_lays_each_problem_out_as_a_sentence_then_its_buttons_and_a_texture_takes_one_line(
) {
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", the_pack_is_read);
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    shell.drop_files(
        panel.center(),
        put(
            dir.path(),
            &[
                (&format!("{C}^jbib_000_u.ydd"), b"jacket"),
                (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
                (&format!("{C}^jbib_diff_000_b_uni.ytd"), b"blue"),
            ],
        ),
    );
    until(&mut shell, "the jacket", |s| garments(s) == 1);
    let (module, id) = {
        let read = shell
            .state
            .wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .unwrap();
        (
            shell.state.wardrobe_module_id().unwrap(),
            read.wardrobe.garments[0].id.clone(),
        )
    };
    // The second texture is not where the pack says: Locate and Remove.
    let folder = dir.path().join("demo").join("clothing");
    let gone = (shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .and_then(|read| read.wardrobe.garments[0].variations[1].texture.clone())
        .unwrap();
    let gone = folder.join(&gone);
    if gone.is_dir() {
        std::fs::remove_dir_all(&gone).unwrap();
    } else {
        std::fs::remove_file(&gone).unwrap();
    }
    shell.state.workbench.want_scan();
    shell.state.show_garment(&module, id);
    until(&mut shell, "the finding", |s| {
        (s.state.workbench.findings().iter()).any(|f| {
            f.fixes
                .iter()
                .any(|fix| matches!(fix, Fix::FindFile { .. }))
        })
    });

    for size in [(1600, 900), (1024, 640)] {
        shell.settled_sized(size);
        let right = f32::from(u16::try_from(size.0).unwrap()) * 0.7;
        // The Garment panel is the one on the right.
        let in_garment = |name: &str| {
            (shell.described.iter())
                .filter(|w| w.bounds.min.x > right)
                .find(|w| w.name.as_deref().is_some_and(|n| n.contains(name)))
                .map(|w| w.bounds)
        };
        let sentence = in_garment("a texture that is not there")
            .unwrap_or_else(|| panic!("no sentence at {size:?}: {:?}", names(&shell)));
        // What is wrong with the garment is the first thing under its name, before its place: it is what needs attention.
        let (title, problems) = (
            in_garment("Garment name").expect("the name is the title"),
            in_garment("Problems (1)").expect("the problems"),
        );
        let position = in_garment("Position").expect("the position");
        assert!(
            title.max.y <= problems.min.y + 1.0 && problems.max.y <= position.min.y,
            "at {size:?}: {title:?}, then {problems:?}, then {position:?}"
        );
        assert!(
            in_garment("Stable id").is_none(),
            "the id is in the tip of the name, not a line of the first screenful: {:?}",
            names(&shell)
        );
        // The labels of the place are beside their fields, in a column of their own.
        let ped = in_garment("Garment ped").expect("the ped");
        assert!(ped.min.x > right + 40.0, "{ped:?}");
        let locate = in_garment("Locate…").expect("Locate…");
        let remove = in_garment("Remove this texture").expect("Remove this texture");
        assert!(
            locate.min.y >= sentence.max.y - 1.0 && remove.min.y >= sentence.max.y - 1.0,
            "at {size:?} the buttons are under the sentence: {sentence:?} then {locate:?} and {remove:?}"
        );
        for letter in ["a  mp_m_", "b  mp_m_"] {
            let line = in_garment(letter).unwrap_or_else(|| {
                panic!("no texture row `{letter}` at {size:?}: {:?}", names(&shell))
            });
            assert!(
                line.height() < 20.0,
                "at {size:?} a texture row is one line, not {line:?}"
            );
        }
    }
    assert!(matches!(
        shell
            .state
            .workbench
            .findings()
            .iter()
            .find_map(|f| match &f.place {
                Place::Garment { id: Some(_), .. } if f.text.contains("not there") =>
                    Some(f.fixes.len()),
                _ => None,
            }),
        Some(2)
    ));
}

#[test]
fn the_wardrobe_of_a_project_says_nothing_of_read_only() {
    // The edit mode's banner is the Project wave's to word (P14): over a project's own files, which a pack is, it does not say Read only, and nothing the Wardrobe draws does either.
    let _serial = theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_clothing_shell(dir.path());
    until(&mut shell, "the pack", the_pack_is_read);
    with_jackets(&mut shell, dir.path(), 1);
    for size in [(1600, 900), (1280, 720), (1024, 640)] {
        shell.settled_sized(size);
        assert_eq!(shell.state.active_workspace, Workspace::Wardrobe);
        assert!(
            !shows(&shell, "Read only"),
            "at {size:?} the Wardrobe says Read only: {:?}",
            names(&shell)
        );
    }
}
