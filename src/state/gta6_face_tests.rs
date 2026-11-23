use or_io::gta6::headerless::{Bone, Model};

use super::*;

/// A skeleton of `names`, each bone the child of the one before, its bind rotation `w` long in w.
fn skeleton(names: &[&str], w: f32) -> Skeleton {
    let bones = (names.iter().enumerate())
        .map(|(i, name)| Bone {
            name: (*name).to_string(),
            hash: name_hash32(name.as_bytes()),
            parent: i.checked_sub(1).map(|p| p as u16),
            rotation: [0.0, 0.0, 0.0, w],
            translation: [i as f32, 0.5, -0.25],
            scale: [1.0; 3],
            inverse_bind: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0; 3]],
        })
        .collect();
    Skeleton { bones }
}

fn face(second: &str) -> Skeleton {
    skeleton(&["FACIAL_C_FacialRoot", second], 1.0)
}

/// A dictionary lookup holding `entries`, each a drawable name less `.#dr` and its face skeleton.
fn holding(entries: Vec<(&'static str, Skeleton)>) -> impl Fn(u32) -> Option<Skeleton> {
    move |code| {
        (entries.iter())
            .find(|(name, _)| name_hash32(name.as_bytes()) == code)
            .map(|(_, s)| s.clone())
    }
}

#[test]
fn a_full_head_finds_the_skeleton_of_its_eyes_full_entry() {
    let wanted = face("FB_Jaw_A");
    let find = holding(vec![
        ("test_head_a_full", face("FB_Jaw_B")),
        ("test_head_a_eyes_full", wanted.clone()),
    ]);
    let found = face_skeleton("Test_Head_A_Full.#dr", None, find);
    assert_eq!(found, Ok(wanted));
}

#[test]
fn a_head_with_no_full_finds_its_name_with_eyes_full() {
    let wanted = face("FB_Nose_C");
    let find = holding(vec![("test_head_b_eyes_full", wanted.clone())]);
    assert_eq!(face_skeleton("test_head_b.#dr", None, find), Ok(wanted));
}

#[test]
fn the_dictionary_entry_wins_over_an_own_face_skeleton_and_an_own_one_serves_without_it() {
    let (entry, own) = (face("FB_Entry"), face("FB_Own"));
    let find = holding(vec![("test_head_c_eyes_full", entry.clone())]);
    assert_eq!(
        face_skeleton("test_head_c_full.#dr", Some(&own), find),
        Ok(entry)
    );
    let none = holding(Vec::new());
    assert_eq!(
        face_skeleton("test_head_c_full.#dr", Some(&own), none),
        Ok(own)
    );
}

#[test]
fn an_own_skeleton_that_is_no_face_is_not_taken_and_the_entries_looked_for_are_said() {
    let body = skeleton(&["SKEL_ROOT", "SKEL_Head"], 1.0);
    let found = face_skeleton("test_head_d_full.#dr", Some(&body), holding(Vec::new()));
    let looked_for = ["test_head_d_eyes_full", "test_head_d_full_eyes"];
    assert_eq!(found, Err(looked_for.map(String::from).to_vec()));
}

#[test]
fn a_head_with_no_eyes_full_entry_finds_its_eyes_entry() {
    let wanted = face("FB_Brow_D");
    let entries = || {
        holding(vec![
            ("test_head_e_eyes", wanted.clone()),
            ("test_head_f_eyes_full", face("FB_E")),
        ])
    };
    let found = face_skeleton("test_head_e.#dr", None, entries());
    assert_eq!(found, Ok(wanted.clone()));
    let found = face_skeleton("test_head_g.#dr", None, entries());
    let looked_for = ["test_head_g_eyes_full", "test_head_g_eyes"];
    assert_eq!(found, Err(looked_for.map(String::from).to_vec()));
}

#[test]
fn a_face_skeleton_has_the_face_root_first_and_unit_rotations() {
    assert!(is_face(&face("FB_F")));
    assert!(!is_face(&skeleton(&["FB_F", "FACIAL_C_FacialRoot"], 1.0)));
    assert!(is_face(&skeleton(
        &["FACIAL_C_FacialRoot"],
        (1.0 + UNIT_SLACK / 4.0).sqrt()
    )));
    assert!(!is_face(&skeleton(
        &["FACIAL_C_FacialRoot"],
        (1.0 + 4.0 * UNIT_SLACK).sqrt()
    )));
    assert!(!is_face(&Skeleton { bones: Vec::new() }));
}

#[test]
fn the_palette_bones_are_every_hash_palette_entry_once_rising() {
    let model = |palette| Model {
        geometries: Vec::new(),
        palette,
        matrix_index: 0,
    };
    let drawable = Drawable {
        name: "h_full.#dr".into(),
        box_min: [0.0; 3],
        box_max: [1.0; 3],
        shaders: Vec::new(),
        levels: [
            Some(vec![model(Ok(Palette::Hashes(vec![0x30, 0x10, 0x20])))]),
            None,
            Some(vec![
                model(Ok(Palette::Own)),
                model(Ok(Palette::Hashes(vec![0x20, 0x05]))),
            ]),
            None,
        ],
        lod_distances: [0.0; 4],
        bound: None,
        embedded: None,
        extra_skeletons: Vec::new(),
    };
    assert_eq!(palette_bones(&drawable), vec![0x05, 0x10, 0x20, 0x30]);
}

/// A set reading two facial controls, of 1 and 2 components, and one track of the unnamed word.
fn expression_section() -> Vec<u8> {
    let put = |bytes: &mut Vec<u8>, at: usize, word: &[u8]| {
        bytes[at..at + word.len()].copy_from_slice(word);
    };
    let pointer = |to: u64| (0x5000_0000 + to).to_le_bytes();
    let mut bytes = vec![0; 0x400];
    put(&mut bytes, 0x08, &pointer(0x3F0));
    put(
        &mut bytes,
        0x10,
        &[0x31, 0x32, 0x33, 0x34, 0x31, 0x32, 0x33, 0x34],
    );
    let records = [
        (0x18, 2u32, 0x1C0),
        (0x28, 3, 0x1D0),
        (0x38, 2, 0x1D8),
        (0x98, 1, 0x1E0),
        (0xA8, 1, 0x1E8),
        (0xB8, 1, 0x1F0),
    ];
    for (at, n, to) in records {
        put(&mut bytes, at, &n.to_le_bytes());
        put(&mut bytes, at + 4, &n.to_le_bytes());
        put(&mut bytes, at + 8, &pointer(to));
    }
    put(&mut bytes, 0x4C, &1u32.to_le_bytes());
    put(&mut bytes, 0x50, &1u32.to_le_bytes());
    let facial = name_hash32(b"facialTranslation");
    for (k, id) in [0xA1u32, 0xA2].into_iter().enumerate() {
        put(&mut bytes, 0x1C0 + 8 * k, &id.to_le_bytes());
        put(&mut bytes, 0x1C4 + 8 * k, &facial.to_le_bytes());
    }
    put(&mut bytes, 0x1D0, &[0, 0, 1, 0, 2, 0]);
    put(&mut bytes, 0x1D8, &[1, 2]);
    put(&mut bytes, 0x1E0, &0xC1u32.to_le_bytes());
    put(&mut bytes, 0x1E4, &0x8332_88FBu32.to_le_bytes());
    put(&mut bytes, 0x1E8, &[3, 0]);
    put(&mut bytes, 0x1F0, &[5]);
    bytes
}

#[test]
fn an_expression_set_opens_as_its_groups_tracks_a_line_each() {
    let _serial = crate::ui::theme::serial();
    let set = read_expressions(&expression_section()).expect("the set reads");
    let text = facts(&set);
    let lines: Vec<&str> = text.lines().collect();
    let at = |line: &str| lines.iter().position(|l| *l == line);
    let reads = at("What it reads, in 4 slots").expect("the inputs' heading");
    let writes = at("What it writes, in 0 slots").expect("the outputs' heading");
    assert!(reads < writes, "{text}");
    let first = at("Facial controls it reads (2)").expect("group A");
    assert!(lines[first + 1].starts_with("  hash_000000A1 "), "{text}");
    assert!(
        lines[first + 2].ends_with(": 2 components in slots 1, 2"),
        "{text}"
    );
    let control = at("Tracks of an unnamed word it reads (1)").expect("group C");
    assert!(lines[control + 1].ends_with(": byte 5, slot 3"), "{text}");
    // A group the set leaves empty has no heading.
    assert_eq!(at("Bones it reads (0)"), None);
}

#[test]
fn an_expression_set_is_learned_as_its_set_and_a_body_the_reader_refuses_as_lines() {
    let _serial = crate::ui::theme::serial();
    let mut section = expression_section();
    let len = section.len() as u64;
    let signature = Rpf9Signature::of(&section, len).expect("a head");
    assert_eq!(signature.to_string(), EXPRESSIONS_SIGNATURE);
    let path = Path::new("learned/0x10.rsc");
    let Some(Learned::Expressions(set)) = learn(path, signature, &section) else {
        panic!("the set is not learned as one");
    };
    let set = *set;
    assert_eq!(Some(set.clone()), read_expressions(&section).ok());
    assert_eq!(expression_set(path), Some(set));

    // The key stated a second time differs, so the reader refuses the body and it opens as lines.
    section[0x14] ^= 1;
    let refused = Path::new("refused/0x10.rsc");
    let Some(Learned::Facts(text)) = learn(refused, signature, &section) else {
        panic!("a refused set is not its lines");
    };
    assert!(text.contains("does not read this body"), "{text}");
    assert!(!is_expression_set(refused));
}
