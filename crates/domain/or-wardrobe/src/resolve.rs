//! The resolver: what a file name says of the garment it belongs to, and what it does not.
//!
//! A name is read as the game's own files are named, `<ped>_<collection>^<slot>_<number>_<skin>.ydd`
//! for a model and `<slot>_diff_<number>_<letter>_<tint>.ytd` for a texture, with or without the
//! prefix, and after a streaming prefix such as `dlc^`. What a name does not state is [`Unread`], never a guess: a model named `shirt.ydd` has no
//! slot, and the import says so and leaves the choice.

use or_io::authoring::texture_import;

use crate::names::{Component, Ped, Prop, Skin, Slot};

/// What a file is in a pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A `.ydd`, the geometry of a garment.
    Model,
    /// A `.ytd`, or an image that one is made from, one variation of a garment.
    Texture,
    /// A `.yld`, the cloth of a component's model, named as the model is.
    Cloth,
}

/// Something a name does not state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    Role,
    /// The gender, which is the ped.
    Ped,
    Slot,
    Position,
    /// The letter of a texture, which says which variation it is.
    Variant,
}

/// What a file name states.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Resolved {
    pub role: Option<Role>,
    pub ped: Option<Ped>,
    pub slot: Option<Slot>,
    pub position: Option<u32>,
    /// The `u` or `r` of a component model.
    pub skin: Option<Skin>,
    /// The letter of a texture, `a` being 0.
    pub variant: Option<u8>,
    /// The word a component texture ends with: `uni`, `whi` and so on.
    pub tint: Option<String>,
    /// The collection the prefix of the name carries, as the game resolves it.
    pub collection: Option<String>,
    /// The number after the skin of an alternate model: the `1` of `jbib_061_u_1.ydd`.
    pub alternate: Option<u32>,
    /// Words after the ones that mean something.
    pub extra: Vec<String>,
    pub unread: Vec<Unread>,
}

const PROP_WORDS: [&str; 10] = [
    "head", "eyes", "ears", "mouth", "lhand", "rhand", "lwrist", "rwrist", "lhip", "hip",
];

/// Read `path`, a file name or a path with `/` or `\` between names, as a file of a pack.
pub fn resolve(path: &str) -> Resolved {
    let mut found = Resolved::default();
    let mut parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let Some(file) = parts.pop() else {
        found
            .unread
            .extend([Unread::Role, Unread::Ped, Unread::Slot]);
        return found;
    };
    let lower = file.to_ascii_lowercase();
    let (stem, ext) = lower.rsplit_once('.').unwrap_or((&lower, ""));
    found.role = match ext {
        "ydd" => Some(Role::Model),
        "ytd" => Some(Role::Texture),
        "yld" => Some(Role::Cloth),
        // An image is a texture that a compile makes a `.ytd` of, and it is named as a `.ytd` is.
        _ if texture_import::SourceFormat::from_extension(ext).is_some() => Some(Role::Texture),
        _ => None,
    };
    if found.role.is_none() {
        found.unread.push(Unread::Role);
    }

    // `dlc^` and any other streaming prefix is a name in a chain: the garment is the last, and its ped and collection are the one before it.
    let (prefix, rest) = match stem.rsplit_once('^') {
        Some((front, rest)) => (front.rsplit('^').next().unwrap_or(front), rest),
        None => ("", stem),
    };
    let tokens: Vec<&str> = rest.split('_').filter(|t| !t.is_empty()).collect();
    let at = slot_at(&tokens);
    found.slot = at.map(|(_, slot)| slot);
    if found.slot.is_none() {
        found.unread.push(Unread::Slot);
    }

    let (ped, collection) = ped_of(prefix, &parts, found.slot);
    found.ped = ped;
    found.collection = collection;
    if found.ped.is_none() {
        found.unread.push(Unread::Ped);
    }

    let Some((first, slot)) = at else {
        return found;
    };
    let after = first + if slot.is_prop() { 2 } else { 1 };
    let mut words = tokens[after..].iter().copied().peekable();
    let diff = words.next_if_eq(&"diff").is_some();
    found.position = words
        .next_if(|w| !w.is_empty() && w.len() <= 4 && w.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|w| w.parse().ok());
    if found.position.is_none() {
        found.unread.push(Unread::Position);
    }
    // A texture and a model are told apart by the word, and a name that disagrees with its extension is not read as either.
    match (diff, found.role) {
        (true, Some(Role::Model | Role::Cloth)) | (false, Some(Role::Texture)) => {
            found.role = None;
            found.unread.push(Unread::Role);
        }
        _ => {}
    }
    if diff {
        found.variant = words
            .next_if(|w| w.len() == 1 && w.as_bytes()[0].is_ascii_lowercase())
            .map(|w| w.as_bytes()[0] - b'a');
        if found.variant.is_none() {
            found.unread.push(Unread::Variant);
        }
        if !slot.is_prop() {
            found.tint = words.next().map(str::to_string);
        }
    } else {
        if !slot.is_prop() {
            found.skin = words
                .next_if(|w| matches!(*w, "u" | "r"))
                .and_then(|w| w.parse().ok());
        }
        found.alternate = words
            .next_if(|w| w.len() <= 2 && w.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|w| w.parse().ok());
    }
    found.extra = words.map(str::to_string).collect();
    found
}

/// The slot a name's words state and where its first word is, reading from the right so that a collection that has a slot's word in it is not the slot.
fn slot_at(tokens: &[&str]) -> Option<(usize, Slot)> {
    (0..tokens.len()).rev().find_map(|i| {
        if tokens[i] == "p" {
            let prop = tokens
                .get(i + 1)
                .and_then(|w| Prop::from_name(&format!("p_{w}")))?;
            return Some((i, Slot::Prop(prop)));
        }
        let after_p = i > 0 && tokens[i - 1] == "p" && PROP_WORDS.contains(&tokens[i]);
        let component = Component::ALL.into_iter().find(|c| c.name() == tokens[i])?;
        (!after_p).then_some((i, Slot::Component(component)))
    })
}

/// The ped a name's prefix or folders state, and the collection its prefix carries.
fn ped_of(prefix: &str, folders: &[&str], slot: Option<Slot>) -> (Option<Ped>, Option<String>) {
    for ped in Ped::ALL {
        if let Some(rest) = prefix.strip_prefix(ped.model()) {
            let collection = rest.strip_prefix('_').map(|c| match slot {
                Some(Slot::Prop(_)) => c.strip_prefix("p_").unwrap_or(c),
                _ => c,
            });
            return (
                Some(ped),
                collection.filter(|c| !c.is_empty()).map(str::to_string),
            );
        }
    }
    for ped in Ped::ALL {
        if prefix.starts_with(&format!("mp_{}_", ped.letter())) {
            return (Some(ped), Some(prefix.to_string()));
        }
    }
    let by_folder = folders.iter().rev().find_map(|folder| {
        let folder = folder.to_ascii_lowercase();
        Ped::ALL.into_iter().find(|ped| {
            folder == "male" && *ped == Ped::Male
                || folder == "female" && *ped == Ped::Female
                || folder.contains(ped.model())
        })
    });
    (by_folder, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(name: &str) -> Option<Slot> {
        Some(name.parse().unwrap())
    }

    #[test]
    fn a_component_model_states_its_ped_collection_slot_position_and_skin() {
        let r = resolve("mp_m_freemode_01_mp_m_unity_clothes^jbib_012_u.ydd");
        assert_eq!(r.role, Some(Role::Model));
        assert_eq!(r.ped, Some(Ped::Male));
        assert_eq!(r.collection.as_deref(), Some("mp_m_unity_clothes"));
        assert_eq!(
            (r.slot, r.position, r.skin),
            (slot("jbib"), Some(12), Some(Skin::Universal))
        );
        assert!(r.unread.is_empty() && r.extra.is_empty(), "{r:?}");

        let r = resolve("stream/pack/mp_f_freemode_01_mp_f_unity_clothes/lowr/mp_f_freemode_01_mp_f_unity_clothes^lowr_067_r.ydd");
        assert_eq!(
            (r.ped, r.slot, r.position, r.skin),
            (Some(Ped::Female), slot("lowr"), Some(67), Some(Skin::Race))
        );
    }

    #[test]
    fn a_component_texture_states_its_variation_and_tint() {
        let r = resolve("mp_m_freemode_01_mp_m_unity_clothes^lowr_diff_067_c_whi.ytd");
        assert_eq!(r.role, Some(Role::Texture));
        assert_eq!(
            (r.slot, r.position, r.variant),
            (slot("lowr"), Some(67), Some(2))
        );
        assert_eq!(r.tint.as_deref(), Some("whi"));
        assert!(r.unread.is_empty(), "{r:?}");
    }

    #[test]
    fn a_prop_is_read_by_its_two_words_and_its_collection_loses_the_p() {
        let r = resolve("mp_m_freemode_01_p_mp_m_unity_clothes^p_head_005.ydd");
        assert_eq!(
            (r.ped, r.slot, r.position),
            (Some(Ped::Male), slot("p_head"), Some(5))
        );
        assert_eq!(r.collection.as_deref(), Some("mp_m_unity_clothes"));
        assert_eq!(r.skin, None, "a prop has no skin");

        let r = resolve("mp_m_freemode_01_p_mp_m_unity_clothes^p_lwrist_diff_001_b.ytd");
        assert_eq!(
            (r.slot, r.position, r.variant),
            (slot("p_lwrist"), Some(1), Some(1))
        );
        assert_eq!(r.tint, None);
        assert!(r.unread.is_empty(), "{r:?}");
    }

    #[test]
    fn the_head_of_a_component_and_the_head_of_a_prop_are_different_slots() {
        assert_eq!(resolve("head_000_u.ydd").slot, slot("head"));
        assert_eq!(resolve("p_head_000.ydd").slot, slot("p_head"));
        assert_eq!(resolve("hair_000_u.ydd").slot, slot("hair"));
    }

    #[test]
    fn a_name_without_a_prefix_takes_its_ped_from_the_folders_and_marks_what_it_cannot_read() {
        let r = resolve("female/jbib_012_u.ydd");
        assert_eq!(
            (r.ped, r.slot, r.position),
            (Some(Ped::Female), slot("jbib"), Some(12))
        );
        assert_eq!(r.collection, None);

        let r = resolve("jbib_012_u.ydd");
        assert_eq!(r.ped, None);
        assert_eq!(r.unread, [Unread::Ped]);

        let r = resolve("shirt.ydd");
        assert_eq!((r.ped, r.slot, r.position), (None, None, None));
        assert_eq!(r.unread, [Unread::Slot, Unread::Ped]);

        let r = resolve("mp_m_freemode_01^surprise_000_u.ydd");
        assert_eq!((r.ped, r.slot), (Some(Ped::Male), None));
        assert_eq!(r.unread, [Unread::Slot]);

        assert_eq!(
            resolve("notes.txt").unread,
            [Unread::Role, Unread::Slot, Unread::Ped]
        );
    }

    #[test]
    fn a_name_that_has_a_slots_word_in_its_collection_is_read_from_the_right() {
        let r = resolve("mp_m_handy_hair_pack_jbib_003_u.ydd");
        assert_eq!((r.slot, r.position), (slot("jbib"), Some(3)));
    }

    #[test]
    fn an_alternate_is_the_number_after_the_skin_and_what_follows_that_is_kept_not_dropped() {
        let r = resolve("mp_m_freemode_01_mp_m_x^jbib_061_u_1.ydd");
        assert_eq!(
            (r.position, r.skin, r.alternate),
            (Some(61), Some(Skin::Universal), Some(1))
        );
        assert!(r.extra.is_empty());
        let r = resolve("mp_m_freemode_01_p_mp_m_x^p_head_007_2.ydd");
        assert_eq!((r.position, r.alternate), (Some(7), Some(2)));
        let r = resolve("mp_m_freemode_01_mp_m_x^jbib_061_u_hi.ydd");
        assert_eq!(r.alternate, None);
        assert_eq!(r.extra, ["hi"]);
    }

    #[test]
    fn case_is_ignored_and_a_name_that_disagrees_with_its_extension_is_not_read_as_either() {
        let r = resolve("MP_M_FREEMODE_01_X^JBIB_000_U.YDD");
        assert_eq!(
            (r.ped, r.slot, r.skin, r.role),
            (
                Some(Ped::Male),
                slot("jbib"),
                Some(Skin::Universal),
                Some(Role::Model)
            )
        );

        let r = resolve("jbib_diff_000_a_uni.ydd");
        assert_eq!(r.role, None);
        assert!(r.unread.contains(&Unread::Role));
        assert_eq!(resolve("jbib_000_u.ytd").role, None);
    }

    #[test]
    fn a_cloth_file_is_named_as_its_model_is_and_a_texture_word_makes_it_nothing() {
        let r = resolve("mp_m_freemode_01_mp_m_unity_clothes^jbib_012_u.yld");
        assert_eq!(r.role, Some(Role::Cloth));
        assert_eq!(
            (r.ped, r.slot, r.position, r.skin),
            (
                Some(Ped::Male),
                slot("jbib"),
                Some(12),
                Some(Skin::Universal)
            )
        );
        assert!(r.unread.is_empty() && r.extra.is_empty(), "{r:?}");

        let r = resolve("jbib_diff_000_a_uni.yld");
        assert_eq!(r.role, None);
        assert!(r.unread.contains(&Unread::Role));
    }

    #[test]
    fn a_texture_without_its_letter_is_a_variation_nobody_can_place() {
        let r = resolve("mp_m_freemode_01_x^jbib_diff_007.ytd");
        assert_eq!((r.position, r.variant), (Some(7), None));
        assert_eq!(r.unread, [Unread::Variant]);
    }

    #[test]
    fn a_dlc_streaming_prefix_is_read_through_and_the_name_after_it_states_the_garment() {
        let r = resolve("dlc^mp_m_freemode_01_mp_m_unity_clothes^jbib_012_u.ydd");
        assert_eq!(
            (r.ped, r.slot, r.position, r.skin),
            (
                Some(Ped::Male),
                slot("jbib"),
                Some(12),
                Some(Skin::Universal)
            )
        );
        assert_eq!(r.collection.as_deref(), Some("mp_m_unity_clothes"));
        assert!(r.unread.is_empty() && r.extra.is_empty(), "{r:?}");

        let r = resolve("DLC^mp_f_freemode_01_p_mp_f_unity_clothes^p_head_diff_003_b.ytd");
        assert_eq!(
            (r.ped, r.slot, r.position, r.variant),
            (Some(Ped::Female), slot("p_head"), Some(3), Some(1))
        );
        assert_eq!(r.collection.as_deref(), Some("mp_f_unity_clothes"));

        // A prefix with no ped in it leaves the ped to the folder, as a bare name does.
        let r = resolve("female/dlc^lowr_004_r.ydd");
        assert_eq!(
            (r.ped, r.slot, r.position),
            (Some(Ped::Female), slot("lowr"), Some(4))
        );
        assert_eq!(r.collection, None);

        // The garment is the last name of a chain, whatever the chain is.
        let r = resolve("dlc^other^mp_m_freemode_01_mp_m_x^feet_001_u.ydd");
        assert_eq!((r.ped, r.slot), (Some(Ped::Male), slot("feet")));
        assert_eq!(r.collection.as_deref(), Some("mp_m_x"));
    }

    #[test]
    fn an_image_is_a_texture_by_its_extension_in_every_format_a_texture_compiles_from() {
        for extension in or_io::authoring::texture_import::SourceFormat::EXTENSIONS {
            let r = resolve(&format!(
                "mp_m_freemode_01_mp_m_x^jbib_diff_001_c_uni.{extension}"
            ));
            assert_eq!(r.role, Some(Role::Texture), "{extension}");
            assert_eq!(
                (r.slot, r.position, r.variant),
                (slot("jbib"), Some(1), Some(2)),
                "{extension}"
            );
            assert_eq!(r.tint.as_deref(), Some("uni"), "{extension}");
            assert!(r.unread.is_empty(), "{extension}: {r:?}");
        }
        // The word that tells a texture from a model still decides, whatever the extension is.
        assert_eq!(resolve("jbib_001_u.png").role, None);
        assert_eq!(resolve("notes.txt").role, None);
    }
}
