//! The Garment panel edits what a cloth is simulated in and what the shop says of a texture: the wind scale of the linked cloth,
//! and the price, the restriction tags and the forced components of a texture's entry, each a control with a name a screen
//! reader speaks and each one step of Undo, read through AccessKit from whole frames of the shell and driven where the names are.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_io::decl::File;
use or_io::gta5::cloth_dict::*;
use or_io::gta5::shared::{PgDictionary, Vec3V};
use or_wardrobe::shop_edit;

use super::every_control_has_a_name_a_screen_reader_can_speak::mute;
use super::frames_of_the_shell::{key, Shell};
use crate::state::front_door::{NewProject, Template};
use crate::state::ThemeChoice;

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

/// A window tall enough for the whole Garment panel with an entry open, so that no control is below the edge of the screen.
const TALL: (u32, u32) = (1600, 2600);

fn until(shell: &mut Shell, what: &str, done: impl Fn(&Shell) -> bool) {
    until_in(shell, TALL, what, done);
}

fn until_in(shell: &mut Shell, size: (u32, u32), what: &str, done: impl Fn(&Shell) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        shell.state.poll_workbench();
        shell.draw_sized(size, Vec::new());
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

fn click_on(shell: &mut Shell, name: &str) {
    let widget = (shell.described.iter())
        .find(|w| w.name.as_deref() == Some(name))
        .or_else(|| {
            (shell.described.iter()).find(|w| w.name.as_deref().is_some_and(|n| n.contains(name)))
        })
        .unwrap_or_else(|| panic!("nothing is named `{name}`: {:?}", names(shell)));
    let at = widget.bounds.center();
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw_sized(TALL, vec![egui::Event::PointerMoved(at), button(true)]);
    shell.draw_sized(TALL, vec![button(false)]);
    shell.settled_sized(TALL);
}

/// Click the drop-down named `name`, not the label beside it that says the same.
fn click_combo(shell: &mut Shell, name: &str) {
    let widget = (shell.described.iter())
        .find(|w| w.role == egui::accesskit::Role::ComboBox && w.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no drop-down is named `{name}`: {:?}", names(shell)));
    let at = widget.bounds.center();
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw_sized(TALL, vec![egui::Event::PointerMoved(at), button(true)]);
    shell.draw_sized(TALL, vec![button(false)]);
    shell.settled_sized(TALL);
}

/// Click the control named `name`, type `text` and press Enter, as a person does.
fn type_into(shell: &mut Shell, name: &str, text: &str) {
    click_on(shell, name);
    shell.draw_sized(TALL, vec![egui::Event::Text(text.to_string())]);
    shell.draw_sized(TALL, vec![key(egui::Key::Enter)]);
    shell.settled_sized(TALL);
}

fn the_pack(shell: &Shell) -> &or_wardrobe::Wardrobe {
    &(shell.state.wardrobe_pack())
        .and_then(|p| p.read.as_ref().ok())
        .expect("the pack was read")
        .wardrobe
}

/// A cloth dictionary of one cloth that states `scale` as its wind.
fn a_yld(scale: f32) -> Vec<u8> {
    let level = PhVerletCloth {
        cloth_data: PhClothData {
            vertex_positions: vec![Vec3V::default(); 3],
            ..Default::default()
        },
        edge_data: vec![PhEdgeData {
            vert_indices: [0, 1],
            edge_length2: 1.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut controller = CharacterClothController {
        bone_index_map: vec![7, 8],
        bone_id_map: vec![0x1234, 0x5678],
        wind_scale: scale,
        ..Default::default()
    };
    controller.base.cloth_0 = Some(Box::new(level));
    PgDictionary {
        codes: vec![0x1234_5678],
        entries: vec![Some(Box::new(CharacterCloth {
            cloth_controller: Some(Box::new(controller)),
            ..Default::default()
        }))],
        ..Default::default()
    }
    .to_bytes()
    .expect("a cloth dictionary saves")
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

/// A shell with a jacket of two textures in the pack and the jacket selected.
fn with_a_jacket_selected(dir: &Path) -> (Shell, String) {
    let mut shell = a_clothing_shell(dir);
    until_in(&mut shell, crate::app::EDITOR_SIZE, "the pack", |s| {
        s.state.wardrobe_pack().is_some_and(|p| p.read.is_ok())
    });
    let panel = crate::ui::panels::wardrobe::drop_rect(&shell.state).expect("drawn");
    shell.drop_files(
        panel.center(),
        files(
            dir,
            &[
                (&format!("{C}^jbib_000_u.ydd"), b"a jacket"),
                (&format!("{C}^jbib_diff_000_a_uni.ytd"), b"red"),
                (&format!("{C}^jbib_diff_000_b_uni.ytd"), b"blue"),
            ],
        ),
    );
    until_in(&mut shell, crate::app::EDITOR_SIZE, "the garment", |s| {
        shows(s, "Tops")
    });
    let module = shell.state.wardrobe_module_id().unwrap();
    let id = the_pack(&shell).garments[0].id.clone();
    shell.state.show_garment(&module, id.clone());
    shell.settled_sized(TALL);
    (shell, id)
}

#[test]
fn the_wind_of_a_linked_cloth_is_set_from_the_cloth_section_and_the_file_s_own_is_one_click_away() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, id) = with_a_jacket_selected(dir.path());

    // With no cloth linked there is nothing to state a wind in.
    assert!(!shows(&shell, "Wind scale of the cloth"));

    let cloth = dir.path().join("elsewhere").join("jacket.yld");
    std::fs::create_dir_all(cloth.parent().unwrap()).unwrap();
    std::fs::write(&cloth, a_yld(1.0)).unwrap();
    crate::platform::file_dialogs::answer_next(cloth.clone());
    click_on(&mut shell, "Link cloth…");
    until(&mut shell, "the picker", |s| shows(s, "Choose the cloth"));
    click_on(&mut shell, "Browse…");
    until(&mut shell, "the question", |s| {
        shows(s, "Only reference it")
    });
    click_on(&mut shell, "Import into the project");
    until(&mut shell, "the link", |s| {
        shows(s, "Linked: cloth/jacket.yld") && shows(s, "Wind scale of the cloth")
    });
    assert!(
        shows(&shell, "The file states 1; the build ships it as it is"),
        "{:?}",
        names(&shell)
    );
    assert!(!shows(&shell, "Use the wind scale the cloth file states"));

    // Typing a number states it for the build: one step of Undo, and the file linked is not touched.
    type_into(&mut shell, "Wind scale of the cloth", "2.5");
    until(&mut shell, "the wind", |s| {
        the_pack(s).garment(&id).unwrap().wind_scale == Some(2.5)
    });
    until(&mut shell, "the sentence", |s| {
        shows(s, "The build states 2.5 in the cloth; the file states 1")
    });
    assert_eq!(
        or_wardrobe::wind::stated_by(
            &shell.state.wardrobe_pack().unwrap().folder,
            "cloth/jacket.yld"
        )
        .unwrap(),
        [1.0],
        "the linked file is never touched"
    );
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the Cloth section with a wind stated", &mut found);
    assert!(found.is_empty(), "{found:?}");

    // The file's own is one click away, and Undo takes each step back.
    click_on(&mut shell, "Use the wind scale the cloth file states");
    until(&mut shell, "the file's own wind", |s| {
        the_pack(s).garment(&id).unwrap().wind_scale.is_none()
    });
    shell.state.undo_last_change();
    until(&mut shell, "the wind again", |s| {
        the_pack(s).garment(&id).unwrap().wind_scale == Some(2.5)
    });
}

#[test]
fn a_texture_s_shop_entry_is_edited_from_the_shop_section_and_each_edit_is_a_step_of_undo() {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, id) = with_a_jacket_selected(dir.path());

    // The entry is folded under a header that says what it states, and says the build's own while it states nothing.
    assert!(
        shows(&shell, "Entry of texture a: the build's own"),
        "{:?}",
        names(&shell)
    );
    click_on(&mut shell, "Entry of texture a");
    // The elements the entry has no field for are folded away until they are asked for.
    assert!(!shows(&shell, "Name of a new element of texture a"));
    click_on(&mut shell, "Other elements");
    for control in [
        "Price of texture a",
        "New restriction tag of texture a",
        "Add a forced component to texture a",
        "Name of a new element of texture a",
        "XML of a new element of texture a",
    ] {
        assert!(shows(&shell, control), "`{control}`: {:?}", names(&shell));
    }
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the Shop section with an entry open", &mut found);
    assert!(found.is_empty(), "{found:?}");

    let entry = |shell: &Shell, texture: usize| {
        the_pack(shell).garment(&id).unwrap().variations[texture]
            .shop
            .clone()
    };
    type_into(&mut shell, "Price of texture a", "250");
    until(&mut shell, "the price", |s| {
        entry(s, 0).is_some_and(|e| e.cost.as_deref() == Some("250"))
    });
    assert!(
        entry(&shell, 1).is_none(),
        "the other texture states nothing"
    );
    until(&mut shell, "the header", |s| {
        shows(s, "Entry of texture a: price 250")
    });

    type_into(&mut shell, "New restriction tag of texture a", "HAT");
    until(&mut shell, "the tag", |s| {
        entry(s, 0).is_some_and(|e| shop_edit::tags(&e) == ["HAT"])
    });
    until(&mut shell, "the tag to be listed", |s| {
        shows(s, "Take the restriction tag HAT off texture a")
    });

    click_on(&mut shell, "Add a forced component to texture a");
    until(&mut shell, "the forced component", |s| {
        entry(s, 0).is_some_and(|e| shop_edit::forced(&e).is_some_and(|f| f.len() == 1))
    });
    until(&mut shell, "its controls", |s| {
        shows(s, "Number of forced component 1 of texture a")
            && shows(s, "Remove forced component 1 of texture a")
    });

    // An element no staged file states is typed as the XML it is written in.
    type_into(&mut shell, "Name of a new element of texture a", "lockHash");
    type_into(
        &mut shell,
        "XML of a new element of texture a",
        "<lockHash>LOCK_ME</lockHash>",
    );
    click_on(&mut shell, "Add the element to texture a");
    // Undo takes the newest step there is: the scan that shows the element can come before the worker's answer, which makes its step.
    until(&mut shell, "the element", |s| {
        entry(s, 0).is_some_and(|e| shop_edit::others(&e).len() == 1)
            && s.state.workbench.wardrobe.is_idle()
    });
    mute(
        &shell,
        "the Shop section with an entry that states things",
        &mut found,
    );
    assert!(found.is_empty(), "{found:?}");

    // Each is a step: Undo takes the element, the forced component, the tag and the price back in turn.
    for gone in ["element", "forced", "tag", "price"] {
        shell.state.undo_last_change();
        until(&mut shell, gone, |s| s.state.workbench.wardrobe.is_idle());
    }
    until(&mut shell, "the pack read again", |s| entry(s, 0).is_none());
}

#[test]
fn the_preview_offers_a_light_the_tangent_and_vertex_colour_views_and_the_model_of_an_alternate() {
    use crate::state::workbench::{light::Light, ViewMode};
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, id) = with_a_jacket_selected(dir.path());

    // The Draw tool lists the two views of vertex data the preview had not, each with a name a screen reader speaks.
    click_combo(&mut shell, "Draw");
    for view in ["Tangents", "Vertex colours", "Normals", "UVs"] {
        assert!(
            shows(&shell, view),
            "Draw lacks `{view}`: {:?}",
            names(&shell)
        );
    }
    click_on(&mut shell, "Vertex colours");
    assert_eq!(
        shell.state.workbench.wardrobe.dressed.view,
        ViewMode::VertexColours
    );
    click_combo(&mut shell, "Draw");
    click_on(&mut shell, "Tangents");
    assert_eq!(
        shell.state.workbench.wardrobe.dressed.view,
        ViewMode::Tangents
    );

    // The Light menu: direction, height and strength, the sun at the time of day, and the way back to the light the viewport began with.
    assert!(shell.state.workbench.wardrobe.dressed.light.is_the_first());
    click_on(&mut shell, "Light");
    for control in [
        "Light direction",
        "Light height",
        "Light strength",
        "Light from the sun at the time of day",
    ] {
        assert!(
            shows(&shell, control),
            "Light lacks `{control}`: {:?}",
            names(&shell)
        );
    }
    let reset = |shell: &Shell| {
        (shell.described.iter())
            .find(|w| w.name.as_deref() == Some("Reset light"))
            .expect("the button is drawn")
            .disabled
    };
    assert!(
        reset(&shell),
        "nothing was changed, so there is nothing to put back"
    );
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the Light menu", &mut found);
    assert!(found.is_empty(), "{found:?}");

    shell.state.workbench.wardrobe.dressed.light = Light {
        strength: 1.5,
        ..Light::default()
    };
    shell.settled_sized(TALL);
    assert!(
        !reset(&shell),
        "a light that is not the first one can be put back"
    );
    click_on(&mut shell, "Reset light");
    assert!(shell.state.workbench.wardrobe.dressed.light.is_the_first());

    click_on(&mut shell, "Light");
    click_on(&mut shell, "Light from the sun at the time of day");
    assert!(shell.state.workbench.wardrobe.dressed.light.sun);

    // A garment with an alternate offers which model the ped wears it with; one with none offers nothing.
    let has_the_model_choice = |shell: &Shell| {
        (shell.described.iter()).any(|w| {
            w.role == egui::accesskit::Role::ComboBox && w.name.as_deref() == Some("Model")
        })
    };
    assert!(!has_the_model_choice(&shell), "{:?}", names(&shell));
    let folder = shell.state.wardrobe_pack().unwrap().folder.clone();
    or_wardrobe::edit::update(&folder, |w| {
        w.garments[0].alternates = vec!["models/jacket_1.ydd".into()];
        Ok(())
    })
    .unwrap();
    shell.state.workbench.want_scan();
    until(&mut shell, "the alternate", |s| {
        !the_pack(s).garment(&id).unwrap().alternates.is_empty() && has_the_model_choice(s)
    });
    click_combo(&mut shell, "Model");
    for model in ["Main model", "Alternate 1"] {
        assert!(shows(&shell, model), "`{model}`: {:?}", names(&shell));
    }
    click_on(&mut shell, "Alternate 1");
    assert_eq!(
        shell.state.workbench.wardrobe.dressed.alternate.get(&id),
        Some(&1)
    );
}

#[test]
fn the_wardrobe_labels_its_two_ways_in_add_clothes_imports_into_the_project_and_open_looks_where_it_is(
) {
    let _serial = crate::ui::theme::serial();
    let dir = tempfile::tempdir().unwrap();
    let (mut shell, id) = with_a_jacket_selected(dir.path());

    // Add clothes… says it imports and decompiles to the project, in the words of the Project panel's own Import.
    let hover = |action| {
        crate::ui::commands::of(&shell.state, action)
            .and_then(|c| c.hover.clone())
            .unwrap_or_default()
    };
    for action in [
        crate::ui::commands::Action::AddClothes,
        crate::ui::commands::Action::AddClothesFolder,
    ] {
        let said = hover(action);
        assert!(
            said.contains("Imports into the project")
                && said.contains("decompiled into a source of the project")
                && said.contains("where Open looks at a file where it is"),
            "{action:?}: {said}"
        );
    }
    let project_panels =
        crate::ui::commands::of(&shell.state, crate::ui::commands::Action::ImportGameFiles)
            .and_then(|c| c.hover.clone())
            .unwrap_or_default();
    assert!(
        project_panels.contains("Decompile"),
        "the Project panel's Import uses the word the Wardrobe's labels use: {project_panels}"
    );

    // The row's menu offers the other way in by its name, and Enter on the row does the same.
    let row = (shell.described.iter())
        .find(|w| {
            w.name.as_deref().is_some_and(|n| n.contains("jbib_000_u"))
                && w.role != egui::accesskit::Role::Label
        })
        .unwrap_or_else(|| panic!("the row: {:?}", names(&shell)))
        .bounds
        .center();
    let secondary = |pressed| egui::Event::PointerButton {
        pos: row,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.draw_sized(TALL, vec![egui::Event::PointerMoved(row), secondary(true)]);
    shell.draw_sized(TALL, vec![secondary(false)]);
    shell.settled_sized(TALL);
    assert!(shows(&shell, "Open model in place"), "{:?}", names(&shell));
    let mut found = std::collections::BTreeMap::new();
    mute(&shell, "the menu of a garment", &mut found);
    assert!(found.is_empty(), "{found:?}");
    click_on(&mut shell, "Open model in place");
    assert_eq!(
        shell.state.active_workspace,
        crate::ui::dock::Workspace::Model,
        "the model is looked at where it is, in the Model workspace"
    );
    let _ = id;
}
