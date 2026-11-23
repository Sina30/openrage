//! Normal and specular maps, a texture budget and a folder an editor saves into as the Wardrobe workspace runs them: the maps are listed and then made on the
//! worker, a budget is planned off the UI thread and shown before it is accepted, a picture saved in a bound folder lands in its slot, and each is one
//! step of Undo or none.
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
use or_wardrobe::slots::MapSlot;
use or_wardrobe::{Component, Garment, Ped, Slot, Variation, Wardrobe};

use super::the_wardrobe_workspace::{opened, until};
use super::workbench::{MapsDraft, Question};
use super::AppState;
use crate::ui::commands::{self, Action};

/// A `.ytd` of one texture of `side` by `side`, DXT1, one mip, with a weave in it.
fn ytd(name: &str, side: u32) -> Vec<u8> {
    let mut image = RgbaImage::new(side, side);
    for y in 0..side {
        for x in 0..side {
            let v = (40 + 150 * ((x / 4 + y / 4) % 2) + x % 4 * 10) as u8;
            image.set_pixel(x, y, [v, v / 2 + 20, (x * 3 % 256) as u8, 255]);
        }
    }
    let mut texture = GrcTexture {
        name: name.to_string(),
        resource_type_and_conversion_flags: PROCESSED,
        ..Default::default()
    };
    let options = TextureBuildOptions {
        mip_levels: Some(1),
        ..TextureBuildOptions::default()
    };
    texture
        .set_pixels(GrcTextureFormat::DXT1, &image, &options)
        .unwrap();
    let mut dictionary = PgDictionary::<GrcTexture>::default();
    dictionary.insert(texture);
    dictionary.to_bytes().unwrap()
}

/// The opened project's pack with a jacket for each texture of `side` pixels named, so that a budget has something to cut.
fn a_pack(dir: &Path, names: &[&str], side: u32) -> (AppState, PathBuf) {
    let (mut state, file) = opened(dir);
    let pack = file.parent().unwrap().join("clothing");
    std::fs::create_dir_all(pack.join("textures")).unwrap();
    let mut w = Wardrobe::new("tshirts");
    for (n, name) in names.iter().enumerate() {
        decompile(
            &ytd(name, side),
            &format!("{name}.ytd"),
            &pack.join("textures"),
            &Facts::default(),
        )
        .unwrap();
        let mut g = Garment::new();
        g.ped = Some(Ped::Male);
        g.slot = Some(Slot::Component(Component::Jbib));
        g.position = Some(n as u32);
        g.model = Some(format!("models/m{n}.ydd"));
        g.variations = vec![Variation {
            texture: Some(format!("textures/{name}.textures.src")),
            ..Default::default()
        }];
        w.garments.push(g);
    }
    w.write(&pack).unwrap();
    state.workbench.want_scan();
    let count = names.len();
    until(&mut state, "the pack", |s| {
        s.wardrobe_pack()
            .and_then(|p| p.read.as_ref().ok())
            .is_some_and(|r| r.wardrobe.garments.len() == count)
    });
    (state, pack)
}

fn idle(state: &mut AppState) {
    until(state, "the worker to finish", |s| s.wardrobe_is_idle());
}

fn textures_of(pack: &Path) -> Vec<String> {
    Wardrobe::read(pack)
        .unwrap()
        .garments
        .iter()
        .map(|g| g.variations[0].texture.clone().unwrap())
        .collect()
}

#[test]
fn maps_are_listed_in_a_window_made_on_the_worker_and_one_undo_takes_them_away() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, pack) = a_pack(dir.path(), &["jbib_diff_000_a_uni"], 64);
    let command = commands::of(&state, Action::MakeMaps).expect("a command");
    assert!(command.enabled.is_ok(), "{:?}", command.enabled);

    commands::run(&mut state, &egui::Context::default(), &command);
    let Some(Question::Maps {
        module,
        garments,
        draft,
        lines,
    }) = state.workbench.asking.clone()
    else {
        panic!("the window asks: {:?}", state.workbench.asking);
    };
    assert_eq!(draft, MapsDraft::default());
    let said = lines.join("\n");
    assert!(
        said.contains("normal map jbib_normal_000_a_uni")
            && said.contains("specular map jbib_spec_000_a_uni"),
        "the window lists what it would make: {said}"
    );
    // Taking the specular map off the draft takes it off the list; nothing has been written.
    let narrower = MapsDraft {
        specular: false,
        ..draft
    };
    let fewer = state
        .maps_words(&module, garments.as_deref(), &narrower)
        .join("\n");
    assert!(
        fewer.contains("normal map") && !fewer.contains("specular map"),
        "{fewer}"
    );
    assert_eq!(
        textures_of(&pack),
        ["textures/jbib_diff_000_a_uni.textures.src"]
    );

    state.workbench.asking = None;
    state.make_maps(module, garments, draft);
    idle(&mut state);
    let now = textures_of(&pack);
    assert_eq!(now, ["textures/jbib_diff_000_a_uni-2.textures.src"]);
    assert!(pack
        .join(&now[0])
        .join("textures/jbib_normal_000_a_uni.png")
        .is_file());
    assert!(pack
        .join("textures/jbib_diff_000_a_uni.textures.src")
        .is_dir());
    assert_eq!(state.workbench.wardrobe.undo.len(), 1);

    state.undo_last_change();
    idle(&mut state);
    assert!(!pack.join(&now[0]).exists());
    assert_eq!(
        textures_of(&pack),
        ["textures/jbib_diff_000_a_uni.textures.src"]
    );
}

#[test]
fn a_budget_is_planned_off_the_ui_thread_shown_side_by_side_and_accepted_as_one_step() {
    let dir = tempfile::tempdir().unwrap();
    // Three textures of 256 by 256, DXT1 with one mip: 32 KB each, so a budget of 60 KB has to cut one.
    let names = [
        "jbib_diff_000_a_uni",
        "jbib_diff_001_a_uni",
        "jbib_diff_002_a_uni",
    ];
    let (mut state, pack) = a_pack(dir.path(), &names, 256);
    let before: Vec<String> = textures_of(&pack);

    let command = commands::of(&state, Action::BudgetTextures).expect("a command");
    commands::run(&mut state, &egui::Context::default(), &command);
    let Some(Question::Budget { module, garments }) = state.workbench.asking.clone() else {
        panic!("the window asks: {:?}", state.workbench.asking);
    };
    state.workbench.wardrobe.textures.target_mb = 0.06;
    state.plan_budget(module, garments);
    assert!(state.is_planning_a_budget());
    until(&mut state, "the plan", |s| {
        s.workbench.wardrobe.textures.plan.is_some()
    });
    let shown = state.workbench.wardrobe.textures.plan.clone().unwrap();
    assert!(
        shown.plan.met && shown.plan.textures() > 0,
        "{:?}",
        shown.plan
    );
    assert!(shown.plan.after <= 62_915, "{:?}", shown.plan);
    assert_eq!(textures_of(&pack), before, "a plan writes nothing");

    // The first choice is shown beside what it was: two pictures of one size.
    until(&mut state, "the pictures", |s| {
        s.workbench.wardrobe.textures.pictures.is_some()
    });
    let (_, both) = state.workbench.wardrobe.textures.pictures.clone().unwrap();
    assert_eq!(
        (both[0].width, both[0].height),
        (both[1].width, both[1].height)
    );
    assert!(both[0].width <= 256);

    state.accept_budget();
    idle(&mut state);
    let after = textures_of(&pack);
    assert!(after.iter().zip(&before).any(|(a, b)| a != b), "{after:?}");
    assert_eq!(state.workbench.wardrobe.undo.len(), 1);
    state.undo_last_change();
    idle(&mut state);
    assert_eq!(textures_of(&pack), before);
}

#[test]
fn a_picture_saved_in_a_bound_folder_lands_in_the_slot_of_the_texture() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, pack) = a_pack(dir.path(), &["jbib_diff_000_a_uni"], 64);
    let art = dir.path().join("photoshop");
    std::fs::create_dir_all(&art).unwrap();
    let module = state.wardrobe_module_id().unwrap();
    let garment = Wardrobe::read(&pack).unwrap().garments[0].id.clone();

    state.bind_watch_folder(&module, garment, 0, MapSlot::Normal, art.clone());
    until(&mut state, "the binding", |s| {
        !s.workbench.wardrobe.textures.bindings().is_empty()
    });
    let mut picture = RgbaImage::new(64, 64);
    for y in 0..64 {
        for x in 0..64 {
            picture.set_pixel(x, y, [128, 128, 255, 255]);
        }
    }
    or_io::codec::texture::save_png(art.join("saved.png"), &picture).unwrap();

    let put =
        pack.join("textures/jbib_diff_000_a_uni.textures.src/textures/jbib_normal_000_a_uni.png");
    until(&mut state, "the picture to be put in", |_| put.is_file());
    assert!(
        art.join("saved.png").is_file(),
        "the editor's file is left where it is"
    );
    assert!(
        state
            .workbench
            .history()
            .iter()
            .any(|e| e.text.contains("into the normal")),
        "{:?}",
        state.workbench.history()
    );
}

#[test]
fn a_save_the_source_could_not_take_is_put_in_when_the_folder_next_changes_and_not_before() {
    let dir = tempfile::tempdir().unwrap();
    let (mut state, pack) = a_pack(dir.path(), &["jbib_diff_000_a_uni"], 64);
    let art = dir.path().join("photoshop");
    std::fs::create_dir_all(&art).unwrap();
    let module = state.wardrobe_module_id().unwrap();
    let garment = Wardrobe::read(&pack).unwrap().garments[0].id.clone();
    state.bind_watch_folder(&module, garment, 0, MapSlot::Normal, art.clone());
    until(&mut state, "the binding", |s| {
        !s.workbench.wardrobe.textures.bindings().is_empty()
    });

    // The source's rules do not read for the moment, as when another program holds them half written.
    let source = pack.join("textures/jbib_diff_000_a_uni.textures.src");
    let rules = source.join("textures.json");
    let kept = std::fs::read_to_string(&rules).unwrap();
    std::fs::write(&rules, "{").unwrap();
    or_io::codec::texture::save_png(art.join("saved.png"), &RgbaImage::new(64, 64)).unwrap();
    until(&mut state, "the refusal to be told", |s| {
        s.toasts
            .iter()
            .any(|t| t.message.contains("was not put in"))
    });

    // Nothing was put in, so the same save is tried again when the folder changes, whatever the change is.
    std::fs::write(&rules, kept).unwrap();
    let put = source.join("textures/jbib_normal_000_a_uni.png");
    assert!(!put.exists());
    std::fs::write(art.join("notes.txt"), b"not a picture").unwrap();
    until(&mut state, "the picture to be put in", |_| put.is_file());
}
