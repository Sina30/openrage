//! The first person alternate of a drawable: the model the local player's first-person view streams in beside it.
//!
//! The game looks an asset up by the hash of `<ped>_<collection>/<slot>_<number>_<form>`, where the form is
//! `m`, `r` or `u` as the drawable's `propMask` says, and streams the file named like the drawable with the
//! stated alternate after it (`jbib_003_u_2.ydd`). A garment states the number it wears in
//! [`crate::model::Meta`] and keeps the model among its own alternates.
//!
//! ⚠ No staged file states a first person alternate, so the file is written from the engine's own table and read
//! back through the same reader; whether the game streams the model is not measured.

use or_io::meta::instance::HashedString;
use or_io::meta::{FirstPersonAlternate, FirstPersonAlternateData};

use crate::alloc::Allocation;
use crate::edit::Error;
use crate::import::LeftOut;
use crate::mask::{self, Property};
use crate::metadata::hashed;
use crate::model::{Garment, Wardrobe};
use crate::names::{Ped, Slot};

/// The data file type a manifest registers the file with.
pub const FILE_TYPE: &str = "PED_FIRST_PERSON_ALTERNATE_DATA";

/// The name the game hashes for the drawable of `garment` at `position` of `ped`: its collection, slot, number and form of its files.
pub fn asset_name(ped: Ped, collection: &str, garment: &Garment, position: u32) -> Option<String> {
    let Some(Slot::Component(slot)) = garment.slot else {
        return None;
    };
    let form = match (
        mask::has_property(garment, Property::MatchPrev),
        mask::has_property(garment, Property::RaceTex),
    ) {
        (true, _) => 'm',
        (_, true) => 'r',
        _ => 'u',
    };
    Some(format!(
        "{}_{collection}/{}_{position:03}_{form}",
        ped.model(),
        slot.name()
    ))
}

/// State alternate `number` as the first person alternate of the component drawable, or none to take it off.
pub fn set(garment: &mut Garment, number: Option<u8>) -> Result<(), Error> {
    if !matches!(garment.slot, Some(Slot::Component(_))) {
        return Err(Error::Refused(
            "only a component has a first person alternate".into(),
        ));
    }
    if let Some(n) = number {
        if n == 0 || n == u8::MAX {
            return Err(Error::Refused(format!(
                "a first person alternate is a number from 1 to 254, and {n} is not"
            )));
        }
    }
    garment.meta.first_person_alternate = number;
    Ok(())
}

/// What the plan writes of the first person alternates.
#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    pub root: FirstPersonAlternateData,
    pub count: usize,
}

/// The file the pack's garments make: none when no placed garment states one.
pub fn document(wardrobe: &Wardrobe, allocation: &Allocation) -> Option<Written> {
    let mut root = FirstPersonAlternateData::default();
    for placed in &allocation.placed {
        let garment = &wardrobe.garments[placed.garment];
        let Some(number) = garment.meta.first_person_alternate else {
            continue;
        };
        let Some(name) = asset_name(
            placed.ped,
            &wardrobe.collection_of(placed.ped, placed.part),
            garment,
            placed.position,
        ) else {
            continue;
        };
        let mut made = FirstPersonAlternate::default();
        (made.asset_name, made.alternate) = (HashedString::new(name), number);
        root.alternates.push(made);
    }
    let count = root.alternates.len();
    (count > 0).then_some(Written { root, count })
}

/// A first person alternate a garment states that the garment has no model for.
pub fn problems(wardrobe: &Wardrobe) -> Vec<(String, String)> {
    wardrobe
        .garments
        .iter()
        .filter_map(|g| {
            let number = g.meta.first_person_alternate?;
            let who = crate::rules::name_of(g);
            let message = if !matches!(g.slot, Some(Slot::Component(_))) {
                format!("{who} is a prop, and only a component has a first person alternate")
            } else if number == 0 || number == u8::MAX {
                format!("{who} states first person alternate {number}, which is no model's number")
            } else if usize::from(number) > g.alternates.len() {
                format!(
                    "{who} states first person alternate {number} and has {} alternate model(s)",
                    g.alternates.len()
                )
            } else {
                return None;
            };
            Some((g.id.clone(), message))
        })
        .collect()
}

/// What an import made of the first person file it read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Read {
    pub files: Vec<String>,
    pub entries: usize,
    /// The entries that are a garment of the pack's.
    pub matched: usize,
    pub left_out: Vec<LeftOut>,
}

/// Give each garment the first person alternate the file states for it.
pub fn apply(
    wardrobe: &mut Wardrobe,
    root: &FirstPersonAlternateData,
    shown: &str,
    read: &mut Read,
) {
    read.files.push(shown.to_string());
    let names: Vec<(u32, usize)> = wardrobe
        .garments
        .iter()
        .enumerate()
        .filter_map(|(at, g)| {
            let (ped, position) = (g.ped?, g.position?);
            let name = asset_name(ped, &wardrobe.collection(ped), g, position)?;
            Some((hashed(&name).hash, at))
        })
        .collect();
    for entry in &root.alternates {
        read.entries += 1;
        match names
            .iter()
            .find(|(hash, _)| *hash == entry.asset_name.hash)
        {
            Some((_, at)) => {
                wardrobe.garments[*at].meta.first_person_alternate = Some(entry.alternate);
                read.matched += 1;
            }
            None => read.left_out.push(LeftOut {
                path: shown.to_string(),
                why: format!(
                    "a first person alternate for {}, which is no drawable of the pack",
                    crate::metadata::text_of(&entry.asset_name)
                ),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::allocate;
    use crate::names::Component;

    fn mask_garment(position: u32) -> Garment {
        let mut g = Garment::new();
        g.ped = Some(Ped::Male);
        g.slot = Some(Slot::Component(Component::Berd));
        g.position = Some(position);
        g.model = Some("mask.ydd".into());
        g.alternates = vec!["mask_1.ydd".into(), "mask_2.ydd".into()];
        g
    }

    #[test]
    fn the_asset_is_named_by_ped_collection_slot_number_and_the_form_the_mask_asks_for() {
        let mut g = mask_garment(7);
        assert_eq!(
            asset_name(Ped::Male, "mp_m_unity", &g, 7).as_deref(),
            Some("mp_m_freemode_01_mp_m_unity/berd_007_u")
        );
        g.skin = crate::names::Skin::Race;
        assert_eq!(
            asset_name(Ped::Male, "mp_m_unity", &g, 7).as_deref(),
            Some("mp_m_freemode_01_mp_m_unity/berd_007_r")
        );
        g.slot = Some(Slot::Prop(crate::names::Prop::Head));
        assert_eq!(asset_name(Ped::Male, "mp_m_unity", &g, 7), None);
    }

    #[test]
    fn a_mask_with_a_first_person_alternate_writes_the_entry_and_reads_back_equal() {
        let mut w = Wardrobe::new("unity");
        let mut g = mask_garment(0);
        set(&mut g, Some(2)).unwrap();
        w.garments.push(g);
        let written = document(&w, &allocate(&w)).expect("one entry");
        assert_eq!(written.count, 1);
        assert_eq!(written.root.alternates[0].alternate, 2);
        use or_io::decl::File;
        let bytes = written.root.to_bytes().expect("it saves");
        let back = FirstPersonAlternateData::from_bytes(&bytes).expect("it reopens");
        let mut again = Wardrobe::new("unity");
        again.garments.push(mask_garment(0));
        let mut read = Read::default();
        apply(&mut again, &back, "data/fp.meta", &mut read);
        assert_eq!((read.entries, read.matched), (1, 1));
        assert_eq!(again.garments[0].meta.first_person_alternate, Some(2));
    }

    #[test]
    fn a_number_the_garment_has_no_model_for_and_a_prop_are_findings_and_zero_is_refused() {
        let mut g = mask_garment(0);
        assert!(set(&mut g, Some(0)).is_err());
        assert!(set(&mut g, Some(255)).is_err());
        set(&mut g, Some(3)).unwrap();
        let mut w = Wardrobe::new("unity");
        w.garments.push(g);
        let found = problems(&w);
        assert_eq!(found.len(), 1);
        assert!(found[0].1.contains("has 2 alternate model(s)"));
        w.garments[0].alternates.push("mask_3.ydd".into());
        assert!(problems(&w).is_empty());
        let mut hat = Garment::new();
        hat.slot = Some(Slot::Prop(crate::names::Prop::Head));
        assert!(set(&mut hat, Some(1)).is_err());
    }

    #[test]
    fn an_entry_for_an_asset_the_pack_does_not_hold_is_left_out_and_said() {
        let mut w = Wardrobe::new("unity");
        w.garments.push(mask_garment(0));
        let mut root = FirstPersonAlternateData::default();
        let mut entry = FirstPersonAlternate::default();
        (entry.asset_name, entry.alternate) =
            (HashedString::new("mp_m_freemode_01_other/berd_000_u"), 1);
        root.alternates.push(entry);
        let mut read = Read::default();
        apply(&mut w, &root, "data/fp.meta", &mut read);
        assert_eq!(read.matched, 0);
        assert_eq!(read.left_out.len(), 1);
        assert_eq!(w.garments[0].meta.first_person_alternate, None);
    }
}
