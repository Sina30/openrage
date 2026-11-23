//! Dropping files on the Wardrobe adds the garments their names state, makes each model and texture a source
//! in the module's folder without replacing any, and Undo takes back what the drop did and nothing else.
//!
//! The models and textures are small valid files, since the door decompiles them; a cloth has no source form
//! and is the one file kept as it is.

use std::path::{Path, PathBuf};

use common::{ydd, ytd};
use or_wardrobe::edit::{add_clothes, undo, Added};
use or_wardrobe::{Ped, Skin, Slot, Wardrobe};

mod common;

const C: &str = "mp_m_freemode_01_mp_m_x";

fn module(dir: &Path) -> PathBuf {
    let module = dir.join("clothing");
    std::fs::create_dir_all(&module).unwrap();
    Wardrobe::new("x").write(&module).unwrap();
    module
}

fn put(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let at = root.join(rel);
    std::fs::create_dir_all(at.parent().unwrap()).unwrap();
    std::fs::write(&at, bytes).unwrap();
    at
}

/// The files of the pack, less its private folder (`.openrage`), which keeps the versions and what a Redo waits for.
fn files(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if e.file_name() == ".openrage" {
                continue;
            }
            if e.path().is_dir() {
                walk(base, &e.path(), out);
            } else {
                out.push(
                    e.path()
                        .strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn a_dropped_folder_of_a_pack_adds_a_garment_for_each_model_with_its_textures_and_cloth() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = dir.path().join("downloads").join("my pack");
    put(&drop, &format!("{C}^jbib_001_u.ydd"), &ydd("jacket one"));
    put(&drop, &format!("{C}^jbib_001_u.yld"), b"cloth one");
    put(
        &drop,
        &format!("{C}^jbib_diff_001_b_uni.ytd"),
        &ytd("tex b"),
    );
    put(
        &drop,
        &format!("{C}^jbib_diff_001_a_uni.ytd"),
        &ytd("tex a"),
    );
    put(&drop, &format!("{C}^lowr_000_r.ydd"), &ydd("trousers"));
    put(
        &drop,
        &format!("{C}^lowr_diff_000_a_whi.ytd"),
        &ytd("tex t"),
    );
    put(
        &drop,
        "mp_f_freemode_01_mp_f_x^feet_000_u.ydd",
        &ydd("shoes"),
    );
    put(
        &drop,
        "mp_f_freemode_01_mp_f_x^feet_diff_000_a_bla.ytd",
        &ytd("tex s"),
    );
    put(&drop, "readme.txt", b"not clothing");
    put(&drop, "mp_m_freemode_01_mp_m_x.ymt", b"metadata");

    let added = add_clothes(&module, std::slice::from_ref(&drop)).expect("the drop is added");

    assert_eq!(added.garments.len(), 3);
    assert_eq!((added.textures, added.cloth, added.unread), (4, 1, 0));
    let w = Wardrobe::read(&module).unwrap();
    assert_eq!(w.garments.len(), 3, "the pack on disk has them");
    let by = |ped: Ped, slot: &str| {
        w.garments
            .iter()
            .find(|g| g.ped == Some(ped) && g.slot == Some(slot.parse::<Slot>().unwrap()))
            .unwrap_or_else(|| panic!("no {slot} for {ped}"))
    };
    let jacket = by(Ped::Male, "jbib");
    assert_eq!(
        jacket.position, None,
        "a dropped garment takes the lowest free place"
    );
    assert_eq!(jacket.skin, Skin::Universal);
    assert_eq!(
        jacket.label.as_deref(),
        Some(format!("{C}^jbib_001_u").as_str())
    );
    // The textures go in the order of their letters, not of the order the folder listed them.
    assert_eq!(
        jacket
            .variations
            .iter()
            .map(|v| v.texture.as_deref().unwrap())
            .collect::<Vec<_>>(),
        [
            format!("textures/{C}^jbib_diff_001_a_uni.textures.src"),
            format!("textures/{C}^jbib_diff_001_b_uni.textures.src")
        ]
    );
    assert_eq!(
        jacket.cloth.as_deref(),
        Some(format!("cloth/{C}^jbib_001_u.yld").as_str())
    );
    assert!(
        !jacket.missing_cloth,
        "a cloth dropped with its model is linked"
    );
    let trousers = by(Ped::Male, "lowr");
    assert_eq!(trousers.skin, Skin::Race);
    assert_eq!(
        trousers.variations[0].tint, None,
        "`whi` is the tint of a race garment"
    );
    let shoes = by(Ped::Female, "feet");
    assert_eq!(
        shoes.variations[0].tint.as_deref(),
        Some("bla"),
        "a tint that is not the default is kept"
    );

    // Each model and texture is a source in the module, the cloth is the one file kept as it is, and the drop is untouched.
    assert_eq!(
        jacket.model.as_deref(),
        Some(format!("models/{C}^jbib_001_u.drawables.src").as_str())
    );
    assert!(files(&module).contains(&format!("models/{C}^jbib_001_u.drawables.src/asset.json")));
    assert!(files(&module).contains(&format!("cloth/{C}^jbib_001_u.yld")));
    assert!(
        (files(&module).iter()).all(|f| {
            let compiled = f.ends_with(".ydd") || f.ends_with(".ytd");
            !compiled || f.contains(".src/")
        }),
        "no compiled game file sits in the module outside a source: {:?}",
        files(&module)
    );
    assert_eq!(files(&drop).len(), 10);
    // What is not a model, a texture or a cloth is not taken; metadata is said to be left out and a note is not.
    assert_eq!(
        added
            .left_out
            .iter()
            .map(|l| l.path.as_str())
            .collect::<Vec<_>>(),
        ["my pack/mp_m_freemode_01_mp_m_x.ymt"]
    );
}

#[test]
fn names_that_state_nothing_make_a_garment_marked_unread_and_a_lone_model_keeps_its_textures() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let a = put(dir.path(), "drop/my jacket.ydd", &ydd("jacket"));
    let t1 = put(dir.path(), "drop/red.ytd", &ytd("red"));
    let t2 = put(dir.path(), "drop/blue.ytd", &ytd("blue"));

    let added = add_clothes(&module, &[a, t2, t1]).unwrap();

    assert_eq!(added.garments.len(), 1);
    assert_eq!(added.unread, 1, "nobody said the ped or the slot");
    let w = Wardrobe::read(&module).unwrap();
    let g = &w.garments[0];
    assert_eq!((g.ped, g.slot), (None, None));
    assert_eq!(g.label.as_deref(), Some("my jacket"));
    assert_eq!(
        g.variations
            .iter()
            .map(|v| v.texture.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["textures/blue.textures.src", "textures/red.textures.src"],
        "the only model there is takes the textures that state no place, by name"
    );
}

#[test]
fn with_two_models_a_texture_that_states_no_place_is_left_out_and_said_so() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let drop = vec![
        put(dir.path(), "drop/one.ydd", &ydd("one")),
        put(dir.path(), "drop/two.ydd", &ydd("two")),
        put(dir.path(), "drop/skin.ytd", &ytd("skin")),
    ];

    let added = add_clothes(&module, &drop).unwrap();

    assert_eq!(added.garments.len(), 2);
    assert_eq!(added.left_out.len(), 1);
    assert_eq!(added.left_out[0].path, "skin.ytd");
    assert!(
        added.left_out[0].why.contains("no model"),
        "{:?}",
        added.left_out
    );
}

#[test]
fn undo_takes_back_the_garments_and_the_files_the_drop_made_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    // A garment and a file the module already has.
    let first = put(dir.path(), &format!("a/{C}^jbib_000_u.ydd"), &ydd("first"));
    let first_texture = put(
        dir.path(),
        &format!("a/{C}^jbib_diff_000_a_uni.ytd"),
        &ytd("first texture"),
    );
    let earlier = add_clothes(&module, &[first, first_texture]).unwrap();
    assert_eq!(earlier.garments.len(), 1);
    let before = files(&module);
    let json_before = Wardrobe::read(&module).unwrap().to_json();

    // A second drop: one model whose name is taken by other bytes, and a texture whose name and bytes are the same.
    let second = put(
        dir.path(),
        &format!("b/{C}^jbib_000_u.ydd"),
        &ydd("a different jacket"),
    );
    let same = put(
        dir.path(),
        &format!("b/{C}^jbib_diff_000_a_uni.ytd"),
        &ytd("first texture"),
    );
    let added = add_clothes(&module, &[second, same]).unwrap();

    assert_eq!(added.garments.len(), 1);
    let jackets: Vec<_> = Wardrobe::read(&module)
        .unwrap()
        .garments
        .iter()
        .map(|g| g.model.clone().unwrap())
        .collect();
    assert_eq!(
        jackets,
        [
            format!("models/{C}^jbib_000_u.drawables.src"),
            format!("models/{C}^jbib_000_u-2.drawables.src")
        ],
        "other bytes under a taken name are a source under a free one, and a source never replaces another"
    );
    assert_eq!(Wardrobe::read(&module).unwrap().garments.len(), 2);

    undo(&module, &added).unwrap();

    assert_eq!(
        files(&module),
        before,
        "only the files this drop copied are gone"
    );
    assert_eq!(Wardrobe::read(&module).unwrap().to_json(), json_before);
}

#[test]
fn undoing_the_only_drop_leaves_the_module_as_it_was_found_and_a_file_inside_it_is_not_copied() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let before = files(&module);

    // A model that already sits in the module is where it is; its texture comes from outside.
    let inside = put(&module, &format!("mine/{C}^jbib_000_u.ydd"), &ydd("mine"));
    let outside = put(
        dir.path(),
        &format!("elsewhere/{C}^jbib_diff_000_a_uni.ytd"),
        &ytd("texture"),
    );
    let added = add_clothes(&module, &[inside, outside]).unwrap();

    let w = Wardrobe::read(&module).unwrap();
    assert!(
        w.garments[0]
            .model
            .as_deref()
            .is_some_and(|m| m.starts_with("models/") && m.ends_with(".drawables.src")),
        "a game file inside the module is decompiled too: {:?}",
        w.garments[0].model
    );
    assert!(
        (added.copied.iter()).all(|c| c.contains(".src/")),
        "{:?}",
        added.copied
    );

    undo(&module, &added).unwrap();
    let mut expected = before;
    expected.push(format!("mine/{C}^jbib_000_u.ydd"));
    expected.sort();
    assert_eq!(
        files(&module),
        expected,
        "the file that was inside stays, the copy goes, and so does the folder it made"
    );
    assert!(Wardrobe::read(&module).unwrap().garments.is_empty());
}

#[test]
fn nothing_recognised_adds_nothing_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let note = put(dir.path(), "drop/notes.txt", b"hello");
    let json = std::fs::read(module.join("wardrobe.json")).unwrap();

    let added: Added = add_clothes(&module, &[note]).unwrap();

    assert!(added.garments.is_empty() && added.copied.is_empty());
    assert_eq!(std::fs::read(module.join("wardrobe.json")).unwrap(), json);
}

#[test]
fn a_folder_with_no_wardrobe_is_not_dropped_into() {
    let dir = tempfile::tempdir().unwrap();
    let drop = put(dir.path(), &format!("drop/{C}^jbib_000_u.ydd"), &ydd("x"));
    let error = add_clothes(&dir.path().join("nowhere"), &[drop]).unwrap_err();
    assert!(error.to_string().contains("wardrobe.json"), "{error}");
}

#[test]
fn a_module_is_made_with_an_empty_pack_and_a_pack_already_there_is_never_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("clothing");

    or_wardrobe::edit::create(&folder, "unity").unwrap();
    let w = Wardrobe::read(&folder).unwrap();
    assert_eq!((w.pack.as_str(), w.garments.len()), ("unity", 0));

    let kept = std::fs::read(folder.join("wardrobe.json")).unwrap();
    let error = or_wardrobe::edit::create(&folder, "other").unwrap_err();
    assert!(error.to_string().contains("already holds"), "{error}");
    assert_eq!(std::fs::read(folder.join("wardrobe.json")).unwrap(), kept);

    let error = or_wardrobe::edit::create(&dir.path().join("x"), "Not A Name").unwrap_err();
    assert!(error.to_string().contains("not a pack name"), "{error}");
    assert!(!dir.path().join("x").exists());
}

#[test]
fn undo_leaves_a_folder_that_was_there_before_the_drop_and_removes_the_ones_the_drop_made() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    // `models` is there and empty before the drop; `textures` is not.
    std::fs::create_dir_all(module.join("models")).unwrap();
    let model = put(
        dir.path(),
        &format!("drop/{C}^jbib_000_u.ydd"),
        &ydd("jacket"),
    );
    let texture = put(
        dir.path(),
        &format!("drop/{C}^jbib_diff_000_a_uni.ytd"),
        &ytd("red"),
    );
    let added = add_clothes(&module, &[model, texture]).unwrap();
    assert_eq!(added.garments.len(), 1);

    undo(&module, &added).unwrap();

    assert!(
        module.join("models").is_dir(),
        "a folder the drop found is not its to remove"
    );
    assert!(
        !module.join("textures").exists(),
        "a folder the drop made goes with it"
    );
}

#[test]
fn undoing_an_older_drop_keeps_a_file_a_newer_garment_names() {
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let older = add_clothes(
        &module,
        &[
            put(dir.path(), &format!("a/{C}^jbib_000_u.ydd"), &ydd("jacket")),
            put(dir.path(), "a/shared.ytd", &ytd("one texture")),
        ],
    )
    .unwrap();
    // The same texture under the same name for another jacket: the newer garment names the file the older drop copied.
    let newer = add_clothes(
        &module,
        &[
            put(
                dir.path(),
                &format!("b/{C}^jbib_001_u.ydd"),
                &ydd("another jacket"),
            ),
            put(dir.path(), "b/shared.ytd", &ytd("one texture")),
        ],
    )
    .unwrap();
    assert_eq!(newer.garments.len(), 1, "{newer:?}");
    assert!(
        !newer.copied.iter().any(|c| c.starts_with("textures/")),
        "the texture is the source already there, not a second one: {:?}",
        newer.copied
    );
    assert_eq!(Wardrobe::read(&module).unwrap().garments.len(), 2);

    undo(&module, &older).unwrap();

    let w = Wardrobe::read(&module).unwrap();
    assert_eq!(w.garments.len(), 1);
    let kept = w.garments[0].variations[0].texture.as_deref().unwrap();
    assert!(
        module.join(kept).join("asset.json").is_file(),
        "{kept} is the texture of a garment that is still in the pack"
    );
}

#[test]
fn a_garment_taken_out_is_put_back_whole_and_in_its_place() {
    use or_wardrobe::edit::{put_back, take_out};
    let dir = tempfile::tempdir().unwrap();
    let module = module(dir.path());
    let dropped = [
        put(dir.path(), &format!("d/{C}^jbib_000_u.ydd"), &ydd("jacket")),
        put(dir.path(), &format!("d/{C}^jbib_000_u.yld"), b"cloth"),
        put(
            dir.path(),
            &format!("d/{C}^jbib_001_u.ydd"),
            &ydd("another"),
        ),
        put(
            dir.path(),
            &format!("d/{C}^lowr_000_u.ydd"),
            &ydd("trousers"),
        ),
    ];
    add_clothes(&module, &dropped).unwrap();
    let mut w = Wardrobe::read(&module).unwrap();
    w.garments[0].position = Some(7);
    w.garments[0].label = Some("my jacket".into());
    let before = w.clone();
    let id = w.garments[0].id.clone();

    let taken = take_out(&mut w, &id).unwrap();
    assert_eq!(w.garments.len(), 2);
    assert!(w.garment(&id).is_none());

    put_back(&mut w, &taken).unwrap();
    assert_eq!(
        w, before,
        "the pin, the name, the cloth and the order are as they were"
    );

    let error = put_back(&mut w, &taken).unwrap_err();
    assert!(error.to_string().contains("already"), "{error}");
    assert_eq!(w, before, "a garment is not put back twice");
    assert!(take_out(&mut w, "nobody").is_err());
}

#[test]
fn a_pack_is_renamed_only_to_a_name_a_collection_can_carry() {
    let mut w = Wardrobe::new("x");

    or_wardrobe::edit::rename_pack(&mut w, "uniforms").unwrap();
    assert_eq!(w.pack, "uniforms");

    for bad in ["", "Not A Name", "9lives", "uniformes_é", "_x"] {
        let error = or_wardrobe::edit::rename_pack(&mut w, bad).unwrap_err();
        assert!(
            error.to_string().contains("not a pack name"),
            "{bad}: {error}"
        );
        assert_eq!(w.pack, "uniforms", "a refused name changes nothing");
    }
}
