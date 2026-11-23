//! The textures of a garment: one is added from a file, moved to another letter, tinted and taken out, each as an edit
//! Undo takes back, and a garment's files are written out under the names the game gives them, which a drop reads again.

mod common;

use std::path::Path;

use common::{garment, put_bytes, JBIB};
use or_project::Origin;
use or_wardrobe::edit::{add_clothes, undo_revision};
use or_wardrobe::{export, inspect, layout, plan, redo, variations, Depth, Ped, Skin, Wardrobe};

fn read(folder: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(folder.join(rel)).unwrap()
}

/// A male jacket pinned at 12 with a model, an alternate, a cloth and two textures, the second tinted and flagged.
fn jacket(folder: &Path) -> (Wardrobe, String) {
    put_bytes(folder, "models/jbib.ydd", &common::ydd("model"));
    put_bytes(folder, "models/jbib_1.ydd", &common::ydd("alternate"));
    put_bytes(folder, "cloth/jbib.yld", b"cloth");
    put_bytes(folder, "textures/a.ytd", &common::ytd("texture a"));
    put_bytes(folder, "textures/b.ytd", &common::ytd("texture b"));
    let mut g = garment(
        Ped::Male,
        JBIB,
        Some(12),
        "models/jbib.ydd",
        &["textures/a.ytd", "textures/b.ytd"],
    );
    g.alternates = vec!["models/jbib_1.ydd".into()];
    g.cloth = Some("cloth/jbib.yld".into());
    g.variations[1].tint = Some("bla".into());
    g.variations[1].tex_id = Some(1);
    let id = g.id.clone();
    let mut w = Wardrobe::new("demo");
    w.garments.push(g);
    w.write(folder).unwrap();
    (w, id)
}

/// The file each shipped texture of the plan is copied from, by the name it ships under.
fn shipped(folder: &Path, w: &Wardrobe) -> Vec<(String, String)> {
    let facts = inspect(folder, w, Depth::Stat);
    let mut found: Vec<(String, String)> = plan(w, &facts, "pack", folder)
        .contribution
        .files
        .into_iter()
        .filter(|f| f.path.ends_with(".ytd"))
        .map(|f| match f.origin {
            Origin::Copy { from, .. } => (
                f.path.rsplit('/').next().unwrap().to_string(),
                from.file_name().unwrap().to_string_lossy().into_owned(),
            ),
            other => panic!("{other:?}"),
        })
        .collect();
    found.sort();
    found
}

#[test]
fn a_texture_is_added_from_a_file_and_undo_and_redo_take_it_out_and_put_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let (before, id) = jacket(at);
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("c.ytd"), common::ytd("texture c")).unwrap();

    let change = variations::add(at, &id, &outside.path().join("c.ytd")).unwrap();

    let w = Wardrobe::read(at).unwrap();
    assert_eq!(w.garments[0].variations.len(), 3);
    let rel = w.garments[0].variations[2].texture.clone().unwrap();
    assert!(
        rel.ends_with("c.textures.src") && at.join(&rel).join("asset.json").is_file(),
        "a texture added from a .ytd is its source: {rel}"
    );
    assert_eq!(
        w.garments[0].variations[1],
        before.garments[0].variations[1]
    );

    let redo_over = undo_revision(at, &change).unwrap();
    assert_eq!(Wardrobe::read(at).unwrap(), before);
    assert!(!at.join(&rel).exists());
    redo(at, &redo_over).unwrap();
    assert_eq!(Wardrobe::read(at).unwrap(), w);
    assert!(at.join(&rel).join("asset.json").is_file());

    for (what, source) in [("a model", "x.ydd"), ("nothing", "gone.ytd")] {
        std::fs::write(outside.path().join("x.ydd"), common::ydd("x")).unwrap();
        assert!(
            variations::add(at, &id, &outside.path().join(source)).is_err(),
            "{what} is no texture"
        );
    }
    assert!(variations::add(at, "nobody", &outside.path().join("c.ytd")).is_err());
}

#[test]
fn moving_a_texture_renames_the_letters_it_passes_and_its_tint_and_flags_go_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let (mut w, id) = jacket(at);
    assert_eq!(
        shipped(at, &w),
        [
            (
                "mp_m_freemode_01_mp_m_demo^jbib_diff_012_a_uni.ytd".into(),
                "a.ytd".into()
            ),
            (
                "mp_m_freemode_01_mp_m_demo^jbib_diff_012_b_bla.ytd".into(),
                "b.ytd".into()
            ),
        ]
    );

    // A row drawn before the textures moved names the file it was drawn with; another file is refused.
    let refused = variations::move_to(&mut w, &id, 0, Some("textures/b.ytd"), 1)
        .unwrap_err()
        .to_string();
    assert!(refused.contains("not the texture"), "{refused}");
    assert!(variations::move_to(&mut w, &id, 0, Some("textures/a.ytd"), 2).is_err());

    variations::move_to(&mut w, &id, 0, Some("textures/a.ytd"), 1).unwrap();

    let g = &w.garments[0];
    assert_eq!(g.variations[0].texture.as_deref(), Some("textures/b.ytd"));
    assert_eq!(g.variations[0].tint.as_deref(), Some("bla"));
    assert_eq!(g.variations[0].tex_id, Some(1));
    assert_eq!(
        shipped(at, &w),
        [
            (
                "mp_m_freemode_01_mp_m_demo^jbib_diff_012_a_bla.ytd".into(),
                "b.ytd".into()
            ),
            (
                "mp_m_freemode_01_mp_m_demo^jbib_diff_012_b_uni.ytd".into(),
                "a.ytd".into()
            ),
        ],
        "b is lettered a now, with the tint it had"
    );
}

#[test]
fn a_tint_is_one_word_and_the_skins_own_word_is_not_stored() {
    let dir = tempfile::tempdir().unwrap();
    let (mut w, id) = jacket(dir.path());

    variations::tint(&mut w, &id, 0, Some("textures/a.ytd"), Some(" WHI ")).unwrap();
    assert_eq!(w.garments[0].variations[0].tint.as_deref(), Some("whi"));
    variations::tint(&mut w, &id, 0, Some("textures/a.ytd"), Some("uni")).unwrap();
    assert_eq!(
        w.garments[0].variations[0].tint, None,
        "uni is what a universal skin writes already"
    );
    variations::tint(&mut w, &id, 1, Some("textures/b.ytd"), None).unwrap();
    assert_eq!(w.garments[0].variations[1].tint, None);

    let held = w.clone();
    for bad in ["a_b", "two words", "../x", "waytoolongatintword"] {
        let refused = variations::tint(&mut w, &id, 0, Some("textures/a.ytd"), Some(bad))
            .unwrap_err()
            .to_string();
        assert!(refused.contains("one word"), "{bad}: {refused}");
    }
    assert!(variations::tint(&mut w, &id, 0, Some("textures/b.ytd"), Some("red")).is_err());
    assert_eq!(w, held);
    w.garments[0].skin = Skin::Race;
    variations::tint(&mut w, &id, 0, Some("textures/a.ytd"), Some("whi")).unwrap();
    assert_eq!(
        w.garments[0].variations[0].tint, None,
        "whi is the other skin's word"
    );
}

#[test]
fn a_texture_taken_out_leaves_its_file_and_the_ones_after_it_move_up_a_letter() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let (mut w, id) = jacket(at);

    variations::remove(&mut w, &id, 0, Some("textures/a.ytd")).unwrap();

    assert_eq!(w.garments[0].variations.len(), 1);
    assert_eq!(
        w.garments[0].variations[0].texture.as_deref(),
        Some("textures/b.ytd")
    );
    assert!(at.join("textures/a.ytd").is_file(), "the file stays");
    assert!(variations::remove(&mut w, &id, 0, Some("textures/a.ytd")).is_err());
}

#[test]
fn the_files_of_a_garment_are_written_under_the_names_the_game_gives_them_and_a_drop_reads_them_again(
) {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let (_, id) = jacket(at);
    let out = tempfile::tempdir().unwrap();

    let done = export::garments(at, std::slice::from_ref(&id), out.path()).unwrap();

    let stem = layout::stem(Ped::Male, "mp_m_demo", false);
    let model = format!("{stem}^jbib_012_u.ydd");
    let mut expected = vec![
        model.clone(),
        format!("{stem}^jbib_012_u_1.ydd"),
        format!("{stem}^jbib_012_u.yld"),
        format!("{stem}^jbib_diff_012_a_uni.ytd"),
        format!("{stem}^jbib_diff_012_b_bla.ytd"),
    ];
    expected.sort();
    let mut written = done.written.clone();
    written.sort();
    assert_eq!(written, expected);
    assert!(done.left_out.is_empty(), "{:?}", done.left_out);
    assert_eq!(read(out.path(), &model), common::ydd("model"));
    assert_eq!(
        read(out.path(), &format!("{stem}^jbib_diff_012_b_bla.ytd")),
        common::ytd("texture b")
    );

    // Again into the same folder: the same bytes are there already, which is no refusal. Other bytes are never overwritten.
    assert!(export::garments(at, std::slice::from_ref(&id), out.path())
        .unwrap()
        .left_out
        .is_empty());
    std::fs::write(out.path().join(&model), b"someone else's").unwrap();
    let again = export::garments(at, std::slice::from_ref(&id), out.path()).unwrap();
    assert_eq!(again.left_out.len(), 1);
    assert!(again.left_out[0].why.contains("not overwritten"));
    assert_eq!(read(out.path(), &model), b"someone else's");
    std::fs::write(out.path().join(&model), common::ydd("model")).unwrap();

    // The folder dropped on another pack makes the garment again: its place, its alternate, its cloth, its textures and its tint.
    let other = tempfile::tempdir().unwrap();
    Wardrobe::new("other").write(other.path()).unwrap();
    let added = add_clothes(other.path(), &[out.path().to_path_buf()]).unwrap();
    assert_eq!(added.garments.len(), 1, "{:?}", added.left_out);
    let w = Wardrobe::read(other.path()).unwrap();
    let again = &w.garments[0];
    assert_eq!(
        (
            again.ped,
            again.slot,
            again.alternates.len(),
            again.cloth.is_some()
        ),
        (Some(Ped::Male), Some(JBIB), 1, true)
    );
    assert_eq!(again.variations.len(), 2);
    assert_eq!(again.variations[1].tint.as_deref(), Some("bla"));
}

#[test]
fn one_texture_is_written_alone_and_a_garment_with_no_place_is_listed_and_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path();
    let (mut w, id) = jacket(at);
    let mut nowhere = garment(Ped::Male, JBIB, None, "models/jbib.ydd", &[]);
    nowhere.slot = None;
    nowhere.label = Some("nowhere".into());
    let nowhere_id = nowhere.id.clone();
    w.garments.push(nowhere);
    w.write(at).unwrap();
    let out = tempfile::tempdir().unwrap();

    let one = export::variation(at, &id, 1, out.path()).unwrap();
    assert_eq!(
        one.written,
        [format!(
            "{}^jbib_diff_012_b_bla.ytd",
            layout::stem(Ped::Male, "mp_m_demo", false)
        )]
    );

    let lost = export::garments(at, &[nowhere_id], out.path()).unwrap();
    assert!(lost.written.is_empty());
    assert!(
        lost.left_out[0].why.contains("no place"),
        "{:?}",
        lost.left_out
    );
    assert!(export::garments(at, &["nobody".into()], out.path()).is_err());
    assert!(export::garments(at, &[], out.path()).is_err());
}

#[test]
fn a_path_that_leaves_the_pack_is_left_out_of_an_export_and_nothing_outside_the_pack_is_read() {
    for named in ["../outside.ydd", "..", ".", ""] {
        let dir = tempfile::tempdir().unwrap();
        let at = dir.path().join("pack");
        put_bytes(&at, "models/a.ydd", b"inside");
        put_bytes(dir.path(), "outside.ydd", b"outside");
        let mut g = garment(Ped::Male, JBIB, Some(12), named, &[named]);
        g.alternates = vec![named.into()];
        g.cloth = Some(named.into());
        let id = g.id.clone();
        let mut w = Wardrobe::new("demo");
        w.garments.push(g);
        w.write(&at).unwrap();
        let out = tempfile::tempdir().unwrap();

        let done = export::garments(&at, &[id], out.path()).unwrap();

        assert!(done.written.is_empty(), "{named:?}: {:?}", done.written);
        assert_eq!(
            std::fs::read_dir(out.path()).unwrap().count(),
            0,
            "{named:?}: nothing was written"
        );
        assert_eq!(done.left_out.len(), 4, "{named:?}: {:?}", done.left_out);
        assert!(
            done.left_out
                .iter()
                .all(|l| l.why.contains("not a file inside the pack")),
            "{named:?}: {:?}",
            done.left_out
        );
    }
}
