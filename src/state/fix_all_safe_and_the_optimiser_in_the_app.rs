//! Fix all safe, the repair of a model and the texture optimiser as the Wardrobe workspace runs them: on the worker, with the
//! report in the Log, one step of Undo each, stoppable, and refused for a pack that is not this window's to write.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder: no install, no keys, and a worker for each edit.

use std::path::{Path, PathBuf};

use or_io::authoring::TextureBuildOptions;
use or_io::codec::texture::{GrcTextureFormat, RgbaImage};
use or_io::decl::File;
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::texture_dict::gen8::PROCESSED;
use or_io::gta5::PgDictionary;
use or_source::game_import::{decompile, Facts};
use or_wardrobe::{Component, Garment, Ped, Slot, Variation, Wardrobe};

use super::the_wardrobe_workspace::{opened, until};
use super::workbench::{Cut, Draft, Question};
use super::AppState;
use crate::ui::commands::{self, Action};

/// A `.ytd` of one texture of `width` by `height` that stores one mip.
pub(super) fn ytd(name: &str, width: u32, height: u32) -> Vec<u8> {
    let mut image = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            image.set_pixel(x, y, [(x * 3 % 256) as u8, (y * 5 % 256) as u8, 90, 255]);
        }
    }
    let mut texture = GrcTexture {
        name: name.to_string(),
        resource_type_and_conversion_flags: PROCESSED,
        ..Default::default()
    };
    let options = TextureBuildOptions {
        mip_levels: Some(1),
        allow_npot: true,
        ..TextureBuildOptions::default()
    };
    texture
        .set_pixels(GrcTextureFormat::DXT1, &image, &options)
        .unwrap();
    let mut dictionary = PgDictionary::<GrcTexture>::default();
    dictionary.insert(texture);
    dictionary.to_bytes().unwrap()
}

/// The opened project's pack, filled with two jackets pinned at one place, the first with a texture of 100 by 60 that stores one mip.
fn a_pack_to_fix(dir: &Path) -> (AppState, PathBuf) {
    let (mut state, file) = opened(dir);
    let pack = file.parent().unwrap().join("clothing");
    std::fs::create_dir_all(pack.join("textures")).unwrap();
    for (name, w, h) in [("odd", 100, 60), ("fine", 64, 64)] {
        decompile(
            &ytd(name, w, h),
            &format!("{name}.ytd"),
            &pack.join("textures"),
            &Facts::default(),
        )
        .unwrap();
    }
    let mut w = Wardrobe::new("tshirts");
    for (model, texture) in [("a", "odd"), ("b", "fine")] {
        let mut g = Garment::new();
        g.ped = Some(Ped::Male);
        g.slot = Some(Slot::Component(Component::Jbib));
        g.position = Some(3);
        g.model = Some(format!("models/{model}.ydd"));
        g.variations = vec![Variation {
            texture: Some(format!("textures/{texture}.textures.src")),
            ..Default::default()
        }];
        w.garments.push(g);
    }
    w.write(&pack).unwrap();
    state.workbench.want_scan();
    until(&mut state, "the pack", |s| {
        s.wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|r| r.wardrobe.garments.len() == 2)
    });
    (state, pack)
}

fn idle(state: &mut AppState) {
    until(state, "the worker to finish", |s| s.wardrobe_is_idle());
}

fn logged(state: &AppState, text: &str) -> bool {
    state
        .workbench
        .history()
        .iter()
        .any(|e| e.text.contains(text))
}

#[test]
fn fix_all_safe_runs_on_the_worker_says_each_step_in_the_log_and_is_one_step_of_undo() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, pack) = a_pack_to_fix(dir.path());
    let command = commands::of(&state, Action::FixAllSafe).expect("a command");
    assert!(command.enabled.is_ok(), "{:?}", command.enabled);

    commands::run(&mut state, &egui::Context::default(), &command);
    idle(&mut state);

    let fixed = Wardrobe::read(&pack).unwrap();
    assert_eq!(fixed.garments[1].position, None);
    assert_eq!(
        fixed.garments[0].variations[0].texture.as_deref(),
        Some("textures/odd-2.textures.src")
    );
    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        1,
        "one step for the pipeline"
    );
    for line in [
        "Repair: nothing to do",
        "Textures: 2 change(s)",
        "Order: 1 change(s)",
    ] {
        assert!(logged(&state, line), "{line} is not in the Log");
    }

    // A second press finds nothing: no step of Undo, no folder.
    commands::run(&mut state, &egui::Context::default(), &command);
    idle(&mut state);
    assert_eq!(state.workbench.wardrobe.undo.len(), 1);
    assert!(!pack.join("textures/odd-3.textures.src").exists());

    state.undo_last_change();
    idle(&mut state);
    assert_eq!(Wardrobe::read(&pack).unwrap().garments[1].position, Some(3));
    assert!(!pack.join("textures/odd-2.textures.src").exists());
}

#[test]
fn the_optimiser_asks_what_to_do_then_cuts_the_size_and_one_undo_takes_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, pack) = a_pack_to_fix(dir.path());
    let id = state.workbench.packs()[0]
        .read
        .as_ref()
        .unwrap()
        .wardrobe
        .garments[1]
        .id
        .clone();
    state.select_garment(&state.wardrobe_module_id().unwrap(), Some(id));

    state.ask_to_optimise();
    let Some(Question::Optimise {
        module,
        garments,
        draft,
    }) = state.workbench.asking.clone()
    else {
        panic!("the window asks: {:?}", state.workbench.asking);
    };
    assert_eq!(
        garments.as_ref().map(Vec::len),
        Some(1),
        "the garment picked"
    );
    assert_eq!(
        draft,
        Draft::default(),
        "what Fix all safe does, until the person says more"
    );
    assert!(draft.asks());

    let draft = Draft {
        cut: Cut::Side(32),
        ..draft
    };
    state.workbench.asking = None;
    state.optimise_textures(module, garments, draft);
    idle(&mut state);

    let after = Wardrobe::read(&pack).unwrap();
    assert_eq!(
        after.garments[1].variations[0].texture.as_deref(),
        Some("textures/fine-2.textures.src")
    );
    assert_eq!(
        after.garments[0].variations[0].texture.as_deref(),
        Some("textures/odd.textures.src"),
        "the garment not picked is not touched"
    );
    assert!(
        logged(&state, "becomes 32 by 32"),
        "{:?}",
        state.workbench.history()
    );
    state.undo_last_change();
    idle(&mut state);
    assert!(!pack.join("textures/fine-2.textures.src").exists());
}

#[test]
fn the_repair_of_a_protected_model_is_refused_by_its_name_and_is_no_step_of_undo() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, pack) = a_pack_to_fix(dir.path());
    std::fs::create_dir_all(pack.join("models")).unwrap();
    std::fs::write(pack.join("models/a.ydd"), b"FXAP\0\0\0\0sealed").unwrap();
    let first = state.workbench.packs()[0]
        .read
        .as_ref()
        .unwrap()
        .wardrobe
        .garments[0]
        .id
        .clone();
    state.select_garment(&state.wardrobe_module_id().unwrap(), Some(first));
    let command = commands::of(&state, Action::RepairModel).expect("a command");
    assert!(command.enabled.is_ok(), "{:?}", command.enabled);

    commands::run(&mut state, &egui::Context::default(), &command);
    idle(&mut state);

    assert!(
        logged(&state, "models/a.ydd cannot be repaired") && logged(&state, "FXAP"),
        "{:?}",
        state.workbench.history()
    );
    assert!(
        state.workbench.wardrobe.undo.is_empty(),
        "a refusal is no step"
    );
}

/// A drawable dictionary whose levels switch at the 9998 of a ped component's, which is where the doctor makes levels.
fn ped_component(name: &str) -> Vec<u8> {
    use or_io::decl::Gen8;
    use or_io::gta5::GtaDrawable;
    let mut dictionary = PgDictionary::<GtaDrawable<Gen8>>::from_bytes(
        &or_source::sample::drawable_dictionary(name, 90, 0.0),
    )
    .unwrap();
    for drawable in dictionary.entries.iter_mut().flatten() {
        drawable.base.lod_group.lod_distances = [9998.0; 4];
    }
    dictionary.to_bytes().unwrap()
}

#[test]
fn also_adding_missing_levels_is_off_until_ticked_and_then_one_more_step_of_the_same_undo() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, pack) = a_pack_to_fix(dir.path());
    std::fs::write(pack.join("models/a.ydd"), ped_component("a")).unwrap_or_else(|_| {
        std::fs::create_dir_all(pack.join("models")).unwrap();
        std::fs::write(pack.join("models/a.ydd"), ped_component("a")).unwrap();
    });
    assert!(!state.workbench.wardrobe.mesh.fix_levels, "off by default");
    let command = commands::of(&state, Action::FixAllSafe).expect("a command");

    commands::run(&mut state, &egui::Context::default(), &command);
    idle(&mut state);
    assert_eq!(
        Wardrobe::read(&pack).unwrap().garments[0].model.as_deref(),
        Some("models/a.ydd"),
        "unticked, no model is touched"
    );
    assert!(
        !logged(&state, "Levels:"),
        "{:?}",
        state.workbench.history()
    );
    assert_eq!(state.workbench.wardrobe.undo.len(), 1);

    state.workbench.wardrobe.mesh.fix_levels = true;
    commands::run(&mut state, &egui::Context::default(), &command);
    idle(&mut state);
    let model = Wardrobe::read(&pack).unwrap().garments[0].model.clone();
    assert!(
        model
            .as_deref()
            .is_some_and(|m| m.ends_with(".drawables.src")),
        "{model:?}"
    );
    assert!(
        logged(&state, "Levels: 1 change(s)"),
        "{:?}",
        state.workbench.history()
    );
    assert_eq!(
        state.workbench.wardrobe.undo.len(),
        2,
        "one step for the pipeline with its levels"
    );

    state.undo_last_change();
    idle(&mut state);
    assert_eq!(
        Wardrobe::read(&pack).unwrap().garments[0].model.as_deref(),
        Some("models/a.ydd")
    );
}
