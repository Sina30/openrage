//! The interface languages as a person meets them: a switch to French redraws the Project and Wardrobe screens, the menus and the palette at once, and the French screens at the smallest window clip, break and overflow nothing the English ones do not.
//!
//! The language is the thread's own in these tests (`i18n::pin`), so no other test sees it; a test that switches the setting restores it.

use egui::epaint::ClippedShape;
use egui::Shape;

use super::frames_of_the_shell::Shell;
use super::the_front_door_on_screen::a_first_launch;
use super::the_project_screens_on_screen::{a_project_shell, names, shows, until};
use crate::state::front_door::{NewProject, Step, Template};
use crate::state::ThemeChoice;
use crate::ui::commands::{self, Action};
use crate::ui::dock::Workspace;

const SMALLEST: (u32, u32) = (1280, 720);

/// What a screen shows that a person would call a fault: text cut with an ellipsis, a word broken in the middle, text the edge of its panel cuts off, a control past the side of the window (the pages scroll, so one below the window is not a fault).
#[derive(Debug, Default, PartialEq, Eq)]
struct Faults {
    elided: Vec<String>,
    broken: Vec<String>,
    cut_off: Vec<String>,
    outside: Vec<String>,
}

fn is_control(role: egui::accesskit::Role) -> bool {
    use egui::accesskit::Role;
    matches!(
        role,
        Role::Button
            | Role::CheckBox
            | Role::RadioButton
            | Role::ComboBox
            | Role::TextInput
            | Role::SpinButton
            | Role::Switch
    )
}

fn walk(shape: &Shape, clip: egui::Rect, found: &mut Faults) {
    match shape {
        Shape::Vec(all) => all.iter().for_each(|s| walk(s, clip, found)),
        Shape::Text(text) => {
            let galley = &text.galley;
            let at = galley.rect.translate(text.pos.to_vec2());
            if at.max.x > clip.max.x + 1.0 || at.min.x < clip.min.x - 1.0 {
                found.cut_off.push(galley.text().to_string());
            }
            if galley.elided {
                found.elided.push(galley.text().to_string());
            }
            for pair in galley.rows.windows(2) {
                let (above, below) = (pair[0].row.text(), pair[1].row.text());
                let joined = above.chars().last().zip(below.chars().next());
                // A break after a space, a dash or a separator is where a word may end; any other is a word cut in two.
                if !pair[0].ends_with_newline
                    && joined.is_some_and(|(a, b)| {
                        !a.is_whitespace() && !b.is_whitespace() && !"-/\\_·,;:.".contains(a)
                    })
                {
                    found.broken.push(format!("{above}|{below}"));
                }
            }
        }
        _ => {}
    }
}

fn faults(shell: &Shell, shapes: &[ClippedShape]) -> Faults {
    let mut found = Faults::default();
    shapes
        .iter()
        .for_each(|clipped| walk(&clipped.shape, clipped.clip_rect, &mut found));
    let window = egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(SMALLEST.0 as f32, SMALLEST.1 as f32),
    )
    .expand(1.0);
    for widget in shell.described.iter().filter(|w| is_control(w.role)) {
        if (widget.bounds.min.x < window.min.x || widget.bounds.max.x > window.max.x)
            && widget.bounds.width() > 0.0
        {
            found.outside.push(widget.name.clone().unwrap_or_default());
        }
    }
    found
}

/// The faults of `shell` drawn in `code`.
fn drawn_in(shell: &mut Shell, code: &str) -> Faults {
    let _language = crate::i18n::pin(code);
    let shapes = shell.settled_sized(SMALLEST);
    faults(shell, &shapes)
}

/// Puts the setting back to the system's, which a test build says is English.
struct Restored;

impl Drop for Restored {
    fn drop(&mut self) {
        crate::i18n::set_active("en");
    }
}

#[test]
fn switching_to_french_redraws_the_project_the_wardrobe_the_menus_and_the_palette_at_once() {
    let _restored = Restored;
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    shell.state.active_workspace = Workspace::Project;
    shell.settled_sized(SMALLEST);
    for english in ["Content", "Builds", "Assets", "Problems"] {
        assert!(shows(&shell, english), "{english}: {:?}", names(&shell));
    }
    let new_project = commands::of(&shell.state, Action::NewProject).expect("a command");
    assert_eq!(new_project.label, "Close all");

    until(&mut shell, "the rows read", |s| {
        s.state.workbench.scan_is_current()
    });

    // The Settings page does it, with no pin on this thread: the next frame is French.
    shell.state.set_language(Some("fr".into()));
    assert!(
        !shell.state.workbench.scan_is_current(),
        "the rows are read again, since they say their kind in the language they were read in"
    );
    shell.settled_sized(SMALLEST);
    for french in ["Contenu", "Builds", "Éléments", "Problèmes"] {
        assert!(shows(&shell, french), "{french}: {:?}", names(&shell));
    }
    assert!(
        !shows(&shell, "Content") && !shows(&shell, "Assets"),
        "English is left behind: {:?}",
        names(&shell)
    );
    let new_project = commands::of(&shell.state, Action::NewProject).expect("a command");
    assert_eq!(new_project.label, "Tout fermer");
    assert_eq!(new_project.category, "Fichier");
    assert!(
        commands::all(&shell.state)
            .iter()
            .any(|c| c.label == "Palette de commandes…"),
        "the palette lists the commands in French"
    );

    shell.state.active_workspace = Workspace::Wardrobe;
    shell.settled_sized(SMALLEST);
    assert!(shows(&shell, "Ajouter…"), "{:?}", names(&shell));
    assert!(!shows(&shell, "Add clothes…"));

    shell.state.set_language(Some("en".into()));
    shell.settled_sized(SMALLEST);
    assert!(shows(&shell, "Add clothes…"), "{:?}", names(&shell));
}

#[test]
fn the_setting_is_kept_and_a_launch_applies_it() {
    let _restored = Restored;
    let dir = tempfile::tempdir().unwrap();
    crate::state::prefs::keep_prefs_in(dir.path());
    let mut state = crate::state::AppState::from_prefs(crate::state::Prefs::default());
    state.set_language(Some("fr".into()));
    let saved = crate::state::prefs::load_prefs();
    assert_eq!(saved.language.as_deref(), Some("fr"));
    crate::i18n::set_active("en");
    let _next_launch = crate::state::AppState::from_prefs(saved);
    assert_eq!(
        crate::i18n::active().code,
        "fr",
        "a launch reads the choice"
    );
}

#[test]
fn a_language_no_file_has_is_no_choice_and_the_system_one_is_used() {
    assert_eq!(crate::i18n::pick(Some("tlh"), None), "en");
    assert_eq!(crate::i18n::pick(Some("tlh"), Some("fr")), "fr");
}

#[test]
fn the_french_screens_clip_break_and_overflow_nothing_the_english_ones_do_not_at_the_smallest_window(
) {
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    let mut screens = Vec::new();
    for (name, workspace) in [
        ("the Project workspace", Workspace::Project),
        ("the Wardrobe", Workspace::Wardrobe),
    ] {
        shell.state.active_workspace = workspace;
        screens.push((name, drawn_in(&mut shell, "en"), drawn_in(&mut shell, "fr")));
    }
    // The Garment panel is the Wardrobe's widest column of controls: it is drawn once a garment is picked.
    let (module, id) = {
        let pack = shell.state.workbench.packs().first().expect("a pack");
        let read = pack.read.as_ref().expect("a pack that reads");
        (pack.module.clone(), read.wardrobe.garments[0].id.clone())
    };
    shell.state.pick_garments(&module, vec![id]);
    screens.push((
        "the Wardrobe with a garment picked",
        drawn_in(&mut shell, "en"),
        drawn_in(&mut shell, "fr"),
    ));
    drop(shell);
    // The Weapons screen, with no weapon picked and with one that has components.
    let weapons_dir = tempfile::tempdir().unwrap();
    let mut weapons = crate::ui::the_weapons_screen_on_screen::a_weapons_shell(weapons_dir.path());
    screens.push((
        "the Weapons screen",
        drawn_in(&mut weapons, "en"),
        drawn_in(&mut weapons, "fr"),
    ));
    let (module, id) = {
        let pack = weapons.state.weapons_pack().expect("a weapon pack");
        let read = pack.read.as_ref().expect("a pack that reads");
        (pack.module.clone(), read.model.weapons[0].id.clone())
    };
    weapons.state.select_weapon(&module, Some(id));
    screens.push((
        "the Weapons screen with a weapon picked",
        drawn_in(&mut weapons, "en"),
        drawn_in(&mut weapons, "fr"),
    ));
    drop(weapons);
    // The Map screen, with nothing picked, with an entity picked (the Selection is its widest column of fields) and with an archetype.
    let maps_dir = tempfile::tempdir().unwrap();
    let mut maps = crate::ui::the_maps_screen_on_screen::a_maps_shell(maps_dir.path());
    screens.push((
        "the Map screen",
        drawn_in(&mut maps, "en"),
        drawn_in(&mut maps, "fr"),
    ));
    let (module, set) = {
        let (pack, set) = maps.state.shown_set().expect("a set");
        (pack.module.clone(), set.id.clone())
    };
    for (name, source, kind) in [
        (
            "the Map screen with an entity picked",
            "harbour.metadata.src",
            crate::state::workbench::PickKind::Entity(0),
        ),
        (
            "the Map screen with an archetype picked",
            "harbour_types.metadata.src",
            crate::state::workbench::PickKind::Archetype(0),
        ),
    ] {
        maps.state.select_in_maps(
            &module,
            &set,
            Some(crate::state::workbench::Pick {
                source: source.to_string(),
                kind,
            }),
        );
        screens.push((name, drawn_in(&mut maps, "en"), drawn_in(&mut maps, "fr")));
    }
    drop(maps);
    // The Particle screen, with a pack of one file and the arrangement for a project, and the pack with nothing in it.
    let particles_dir = tempfile::tempdir().unwrap();
    let mut particles =
        crate::ui::the_particle_screen_on_screen::a_particles_shell(particles_dir.path());
    screens.push((
        "the Particle screen",
        drawn_in(&mut particles, "en"),
        drawn_in(&mut particles, "fr"),
    ));
    drop(particles);
    let empty_dir = tempfile::tempdir().unwrap();
    let mut empty = a_first_launch();
    empty
        .state
        .load_project_from(crate::ui::the_front_door_on_screen::a_project(
            empty_dir.path(),
            "sparks",
            Template::Particles,
        ));
    empty.state.active_workspace = crate::ui::dock::Workspace::Particle;
    screens.push((
        "the Particle screen of an empty pack",
        drawn_in(&mut empty, "en"),
        drawn_in(&mut empty, "fr"),
    ));
    drop(empty);
    let mut start = a_first_launch();
    screens.push((
        "the start page",
        drawn_in(&mut start, "en"),
        drawn_in(&mut start, "fr"),
    ));
    // ⚠ `Action::NewProject` is the New Session command, which draws the start page again: the dialog is opened on its own.
    for (name, template, step, button) in [
        (
            "the New project dialog's gallery",
            Template::Empty,
            Step::Gallery,
            "Next",
        ),
        (
            "the details of an empty project",
            Template::Empty,
            Step::Details,
            "Create",
        ),
        (
            "the details of a files project",
            Template::Files,
            Step::Details,
            "Create",
        ),
        (
            "the details of a clothing pack",
            Template::Clothing,
            Step::Details,
            "Create",
        ),
        (
            "the details of a weapon pack",
            Template::Weapons,
            Step::Details,
            "Create",
        ),
        (
            "the details of a particle pack",
            Template::Particles,
            Step::Details,
            "Create",
        ),
        (
            "the details of a project from a folder",
            Template::Adopt,
            Step::Details,
            "Make project",
        ),
    ] {
        start.state.start_new_project_on(template, step);
        let english = drawn_in(&mut start, "en");
        assert!(
            shows(&start, button),
            "{name} is not drawn: {:?}",
            names(&start)
        );
        // A dialog opened again: what the first one worded in English is kept for a moment.
        start.state.start_new_project_on(template, step);
        let french = drawn_in(&mut start, "fr");
        let said = {
            let _french = crate::i18n::pin("fr");
            crate::i18n::tr(button)
        };
        assert!(
            shows(&start, said),
            "{name} is not drawn in French: {:?}",
            names(&start)
        );
        screens.push((name, english, french));
    }

    let mut found = Vec::new();
    for (name, english, french) in screens {
        for (what, in_english, in_french) in [
            ("an ellipsis", &english.elided, &french.elided),
            ("a word broken in two", &english.broken, &french.broken),
            (
                "text cut off by the edge of its panel",
                &english.cut_off,
                &french.cut_off,
            ),
            (
                "a control past the side of the window",
                &english.outside,
                &french.outside,
            ),
        ] {
            // What English has cut too is the person's own text (a project's name), which no language changes.
            let new: Vec<_> = in_french
                .iter()
                .filter(|t| !in_english.contains(t))
                .collect();
            if !new.is_empty() {
                found.push(format!(
                    "{name} in French has {what} that English has not: {new:?}"
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "{}",
        found.join(
            "
"
        )
    );
}

/// A project with a clothing pack that holds no garment: the Wardrobe is one drop area.
fn an_empty_wardrobe(dir: &std::path::Path) -> Shell {
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("empty".into());
    let file = draft.create().expect("a clothing project").file;
    let mut shell = Shell::new(ThemeChoice::Dark);
    shell.state.load_project_from(file);
    shell.state.active_workspace = Workspace::Wardrobe;
    shell.settled();
    until(&mut shell, "the pack reads", |s| {
        (s.state.workbench.packs().first()).is_some_and(|p| p.read.is_ok())
    });
    shell
}

#[test]
fn the_empty_wardrobe_and_the_level_of_detail_are_said_in_french() {
    let dir = tempfile::tempdir().unwrap();
    let mut empty = an_empty_wardrobe(dir.path());
    let drop_here = "Drop models, textures and cloth here, or folders of them";
    {
        let _english = crate::i18n::pin("en");
        empty.settled_sized(SMALLEST);
        assert!(shows(&empty, drop_here), "{:?}", names(&empty));
    }
    let french = {
        let _french = crate::i18n::pin("fr");
        empty.settled_sized(SMALLEST);
        crate::i18n::tr("Drop models, textures and cloth here, or folders of them")
    };
    assert_ne!(french, drop_here, "French has a sentence for it");
    assert!(shows(&empty, french), "{:?}", names(&empty));
    assert!(!shows(&empty, drop_here), "{:?}", names(&empty));

    // The level the combo shows is painted, not named, so it is read from the shapes.
    let dir = tempfile::tempdir().unwrap();
    let mut shell = a_project_shell(dir.path(), ThemeChoice::Dark);
    shell.state.active_workspace = Workspace::Wardrobe;
    let level = |shell: &mut Shell, code: &str| {
        let _language = crate::i18n::pin(code);
        painted(&shell.settled_sized(SMALLEST))
    };
    let in_english = level(&mut shell, "en");
    assert!(in_english.iter().any(|t| t == "High"), "{in_english:?}");
    let in_french = level(&mut shell, "fr");
    assert!(in_french.iter().any(|t| t == "Haut"), "{in_french:?}");
    assert!(!in_french.iter().any(|t| t == "High"), "{in_french:?}");
}

/// Every text the shapes paint.
fn painted(shapes: &[ClippedShape]) -> Vec<String> {
    fn walk(shape: &Shape, out: &mut Vec<String>) {
        match shape {
            Shape::Text(text) => out.push(text.galley.text().to_string()),
            Shape::Vec(all) => all.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes
        .iter()
        .for_each(|clipped| walk(&clipped.shape, &mut out));
    out
}
