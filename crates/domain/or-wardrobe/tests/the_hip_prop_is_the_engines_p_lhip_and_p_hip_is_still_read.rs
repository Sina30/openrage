//! The prop anchor 8 is `p_lhip` in the engine's slot names, so it is in the file names of a pack
//! and in `wardrobe.json`. `p_hip`, which OpenRage spelled it before, is still read and never written.

use or_wardrobe::layout::model_name;
use or_wardrobe::{resolve, Ped, Prop, Skin, Slot, Wardrobe};

fn hip() -> Slot {
    Slot::Prop(Prop::Hip)
}

#[test]
fn the_hip_prop_is_spelled_as_the_engine_spells_it() {
    assert_eq!(Prop::Hip.anchor(), 8);
    assert_eq!(Prop::Hip.name(), "p_lhip");
    assert_eq!(hip().to_string(), "p_lhip");
    assert_eq!("p_lhip".parse::<Slot>(), Ok(hip()));
    assert_eq!(
        model_name(Ped::Male, "pack", hip(), 3, Skin::Universal),
        "mp_m_freemode_01_p_pack^p_lhip_003.ydd"
    );
}

#[test]
fn p_hip_is_still_a_slot_and_is_not_one_of_two() {
    assert_eq!("p_hip".parse::<Slot>(), Ok(hip()));
    assert_eq!("P_HIP".parse::<Slot>(), Ok(hip()));
    let names: Vec<&str> = Slot::all().map(Slot::name).collect();
    assert!(
        names.contains(&"p_lhip") && !names.contains(&"p_hip"),
        "{names:?}"
    );
}

#[test]
fn a_file_named_with_either_word_is_the_hip_prop_on_import() {
    for word in ["p_lhip", "p_hip"] {
        let named = format!("mp_m_freemode_01_p_mp_m_pack^{word}_003.ydd");
        let found = resolve(&named);
        assert_eq!(
            (found.ped, found.slot, found.position),
            (Some(Ped::Male), Some(hip()), Some(3)),
            "{named}"
        );
        assert_eq!(found.collection.as_deref(), Some("mp_m_pack"), "{named}");
        assert!(found.unread.is_empty(), "{named}: {found:?}");

        let texture = resolve(&format!("{word}_diff_001_b.ytd"));
        assert_eq!(
            (texture.slot, texture.position, texture.variant),
            (Some(hip()), Some(1), Some(1)),
            "{word}"
        );
    }
}

#[test]
fn a_wardrobe_json_that_says_p_hip_is_read_and_says_p_lhip_when_it_is_saved() {
    let old = r#"{"format":"openrage-wardrobe","schema":2,"pack":"p","garments":[
        {"id":"belt","ped":"mp_m_freemode_01","slot":"p_hip"}]}"#;
    let read = Wardrobe::from_json(old.as_bytes(), "wardrobe.json").expect("p_hip still reads");
    assert_eq!(read.garments[0].slot, Some(hip()));
    let saved = read.to_json();
    assert!(saved.contains("\"slot\": \"p_lhip\""), "{saved}");
    assert!(!saved.contains("p_hip"), "{saved}");
}
