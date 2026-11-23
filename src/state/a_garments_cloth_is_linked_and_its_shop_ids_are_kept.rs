//! What the Wardrobe does with a garment's cloth and a shop's own ids: a cloth is linked to the garment
//! selected and unlinked, shop metadata dropped with the clothes becomes the ids of their textures, and
//! each of these is a change Undo takes back.
//!
//! ⚠ Built with `AppState::from_prefs` and a project in a scratch folder, and a worker for each scan and
//! edit that a case waits on. The file dialog is the stand-in the build answers with the path a case names.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use or_wardrobe::Garment;

use super::front_door::{NewProject, Template};
use super::workbench::{Choice, Question};
use super::{AppState, Prefs};
use crate::ui::commands::{self, Action};

const C: &str = "mp_m_freemode_01_mp_m_t";

fn write(path: &Path, body: &[u8]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    std::fs::write(path, or_source::sample::as_a_game_file(&name, body)).unwrap();
    path.to_path_buf()
}

/// A clothing project made the way the template makes it, opened in a state.
fn opened(dir: &Path) -> AppState {
    let mut draft = NewProject::new(Template::Clothing, dir.to_path_buf());
    draft.set_name("tshirts".into());
    let created = draft.create().expect("the template makes a project");
    let mut state = AppState::from_prefs(Prefs::default());
    state.documents.clear();
    state.active_document = None;
    state.load_project_from(created.file);
    state
}

fn until(state: &mut AppState, what: &str, done: impl Fn(&AppState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        state.poll_workbench();
        if done(state) {
            return;
        }
        assert!(Instant::now() < deadline, "{what} never happened");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The garments of the content shown, as the last scan read them.
fn garments(state: &AppState) -> Vec<Garment> {
    state
        .wardrobe_pack()
        .and_then(|p| p.read.as_ref().ok())
        .map(|read| read.wardrobe.garments.clone())
        .unwrap_or_default()
}

/// A pack of one jacket with one texture, the jacket selected.
fn a_pack_with_a_jacket(dir: &Path) -> AppState {
    let mut state = opened(dir);
    until(&mut state, "the first scan", |s| s.workbench.is_scanned());
    state.add_clothes(vec![
        write(
            &dir.join("drop").join(format!("{C}^jbib_000_u.ydd")),
            &or_source::sample::drawable_dictionary("jacket", 6, 0.0),
        ),
        write(
            &dir.join("drop")
                .join(format!("{C}^jbib_diff_000_a_uni.ytd")),
            &or_source::sample::texture_dictionary("tex", 3),
        ),
    ]);
    until(&mut state, "the drop", |s| garments(s).len() == 1);
    let module = state.wardrobe_module_id().unwrap();
    let id = garments(&state)[0].id.clone();
    state.select_garment(&module, Some(id));
    state
}

fn run(state: &mut AppState, action: Action) {
    commands::run_action(state, &egui::Context::default(), action);
}

#[test]
fn a_cloth_is_linked_to_the_garment_selected_and_unlinked_and_undo_takes_each_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = a_pack_with_a_jacket(dir.path());
    assert_eq!(
        commands::of(&state, Action::UnlinkCloth).unwrap().enabled,
        Err("This garment has no cloth")
    );
    assert!(commands::of(&state, Action::LinkCloth)
        .unwrap()
        .enabled
        .is_ok());

    let cloth = write(&dir.path().join("elsewhere").join("jacket.yld"), b"cloth");
    // The command opens the picker; a file of another place asks whether to import it or only to reference it.
    run(&mut state, Action::LinkCloth);
    let Some(Question::Pick(picker)) = state.workbench.asking.clone() else {
        panic!("the command opens the picker: {:?}", state.workbench.asking);
    };
    state.choose(picker.wanting, Choice::Outside(cloth));
    assert!(
        matches!(
            state.workbench.asking,
            Some(Question::ImportOrReference { .. })
        ),
        "{:?}",
        state.workbench.asking
    );
    state.answer_import_or_reference(true);
    until(&mut state, "the link", |s| garments(s)[0].cloth.is_some());
    let linked = garments(&state)[0].cloth.clone().unwrap();
    assert_eq!(linked, "cloth/jacket.yld");
    let pack = state.wardrobe_pack().unwrap().folder.clone();
    assert!(
        pack.join(&linked).is_file(),
        "the file is copied into the pack"
    );
    assert!(
        commands::of(&state, Action::UnlinkCloth)
            .unwrap()
            .enabled
            .is_ok(),
        "a garment with a cloth can be unlinked"
    );
    assert_eq!(state.workbench.wardrobe.undo_words(), "Undo cloth");

    run(&mut state, Action::UnlinkCloth);
    until(&mut state, "the unlink", |s| garments(s)[0].cloth.is_none());
    assert!(pack.join(&linked).is_file(), "unlinking leaves the file");

    // Undo puts the link back, and the Undo before it takes the file the link copied.
    state.undo_last_change();
    until(&mut state, "the undo of the unlink", |s| {
        garments(s)[0].cloth.is_some()
    });
    state.undo_last_change();
    until(&mut state, "the undo of the link", |s| {
        garments(s)[0].cloth.is_none()
    });
    assert!(!pack.join(&linked).exists());
    assert_eq!(
        state.workbench.wardrobe.undo_words(),
        "Undo add (1)",
        "only the drop that made the garment is left to take back"
    );
}

#[test]
fn a_cloth_cannot_be_linked_to_a_prop_or_with_no_garment_selected_and_the_reason_is_said() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = a_pack_with_a_jacket(dir.path());
    let module = state.wardrobe_module_id().unwrap();
    state.select_garment(&module, None);
    assert_eq!(
        commands::of(&state, Action::LinkCloth).unwrap().enabled,
        Err("Select a garment in the Wardrobe first")
    );

    // A prop dropped beside the jacket.
    state.add_clothes(vec![
        write(
            &dir.path()
                .join("drop2")
                .join("mp_m_freemode_01_p_mp_m_t^p_head_000.ydd"),
            b"hat",
        ),
        write(
            &dir.path()
                .join("drop2")
                .join("mp_m_freemode_01_p_mp_m_t^p_head_diff_000_a.ytd"),
            b"tex",
        ),
    ]);
    until(&mut state, "the prop", |s| garments(s).len() == 2);
    let prop = garments(&state)
        .into_iter()
        .find(|g| g.slot.is_some_and(or_wardrobe::Slot::is_prop))
        .expect("the hat");
    state.select_garment(&module, Some(prop.id));
    assert_eq!(
        commands::of(&state, Action::LinkCloth).unwrap().enabled,
        Err("A prop owns no cloth: the game reads none for it")
    );
}

const SHOP: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<ShopPedApparel>\r\n  <pedName>mp_m_freemode_01</pedName>\r\n  <dlcName>t</dlcName>\r\n  <pedComponents>\r\n    <Item>\r\n      <cost value=\"250\"/>\r\n      <uniqueNameHash>7F3A9B21C0D4</uniqueNameHash>\r\n      <localDrawableIndex value=\"0\"/>\r\n      <eCompType>PV_COMP_JBIB</eCompType>\r\n      <textureIndex value=\"0\"/>\r\n    </Item>\r\n    <Item>\r\n      <cost value=\"0\"/>\r\n      <uniqueNameHash>RETIRED_COAT</uniqueNameHash>\r\n      <localDrawableIndex value=\"4\"/>\r\n      <eCompType>PV_COMP_JBIB</eCompType>\r\n      <textureIndex value=\"0\"/>\r\n    </Item>\r\n  </pedComponents>\r\n  <pedProps/>\r\n</ShopPedApparel>\r\n";

#[test]
fn shop_metadata_dropped_with_clothes_becomes_the_ids_of_their_textures_is_said_in_the_log_and_undo_takes_it_back(
) {
    let dir = tempfile::tempdir().unwrap();
    let mut state = opened(dir.path());
    until(&mut state, "the first scan", |s| s.workbench.is_scanned());
    let drop = dir.path().join("drop");
    state.add_clothes(vec![
        write(&drop.join(format!("{C}^jbib_000_u.ydd")), b"jacket"),
        write(&drop.join(format!("{C}^jbib_diff_000_a_uni.ytd")), b"tex"),
        write(&drop.join("shop.meta"), SHOP.as_bytes()),
    ]);
    until(&mut state, "the drop", |s| garments(s).len() == 1);
    until(&mut state, "the shop ids in the scan", |s| {
        garments(s)[0].variations[0].shop.is_some()
    });

    let jacket = &garments(&state)[0];
    let entry = jacket.variations[0].shop.clone().unwrap();
    assert_eq!(
        (entry.id.as_str(), entry.cost.as_deref()),
        ("7F3A9B21C0D4", Some("250"))
    );
    let said: Vec<&str> = state
        .workbench
        .history()
        .iter()
        .map(|e| e.text.as_str())
        .collect();
    assert!(
        said.iter()
            .any(|t| t.contains("1 of 2 entries became the ids of textures")),
        "{said:?}"
    );
    assert!(
        said.iter().any(|t| t.contains("RETIRED_COAT")),
        "the entry that points at no garment is listed in the Log: {said:?}"
    );
    // The entry that points at nothing is a finding of the pack, which can be set aside with a reason.
    until(&mut state, "the finding", |s| {
        s.workbench
            .findings()
            .iter()
            .any(|f| f.text.contains("no garment of the pack"))
    });

    state.undo_last_change();
    until(&mut state, "the undo", |s| garments(s).is_empty());
    let pack = state.wardrobe_pack().unwrap().read.as_ref().unwrap();
    assert!(pack.wardrobe.shop.unplaced.is_empty());
}

#[test]
fn a_shop_id_chosen_for_a_texture_is_kept_and_taking_the_choice_back_restores_the_imported_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = opened(dir.path());
    until(&mut state, "the first scan", |s| s.workbench.is_scanned());
    let drop = dir.path().join("drop");
    state.add_clothes(vec![
        write(&drop.join(format!("{C}^jbib_000_u.ydd")), b"jacket"),
        write(&drop.join(format!("{C}^jbib_diff_000_a_uni.ytd")), b"tex"),
        write(&drop.join("shop.meta"), SHOP.as_bytes()),
    ]);
    until(&mut state, "the shop ids", |s| {
        garments(s)
            .first()
            .is_some_and(|g| g.variations[0].shop.is_some())
    });
    let id = garments(&state)[0].id.clone();

    state.set_shop_id(id.clone(), 0, Some("  MY_OWN_ID ".into()));
    until(&mut state, "the choice", |s| {
        garments(s)[0].variations[0].shop_id.is_some()
    });
    let jacket = &garments(&state)[0].variations[0];
    assert_eq!(jacket.shop_id.as_deref(), Some("MY_OWN_ID"));
    assert_eq!(
        jacket.shop.as_ref().unwrap().id,
        "7F3A9B21C0D4",
        "what the import read is kept to go back to"
    );

    state.set_shop_id(id.clone(), 0, None);
    until(&mut state, "the choice taken back", |s| {
        garments(s)[0].variations[0].shop_id.is_none()
    });

    // An id is one short line: a refused one says so and changes nothing.
    state.set_shop_id(id, 0, Some("two\nlines".into()));
    until(&mut state, "the refusal", |s| {
        s.toasts.iter().any(|t| t.message.contains("one line"))
    });
    assert!(garments(&state)[0].variations[0].shop_id.is_none());
}
