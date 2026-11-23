//! The names a garment's flags, mask, audio and hair carry, measured on the ped metadata the game ships.
//!
//! The documents are the metadata corpus (`OPENRAGE_META_CORPUS`, or the sibling `openrage-corpus`); without it the test
//! skips, which is not a pass. Every figure asserted is counted from the documents and every name is the engine's.
//!
//! ⚠ One test reads the corpus once and holds every field to it: reading it takes most of a minute, and a section that
//! fails names itself in its message.

mod common;

use std::collections::BTreeMap;

use common::staged::{self, Staged};
use or_wardrobe::audio;
use or_wardrobe::flags::{self, Flag};
use or_wardrobe::hair;
use or_wardrobe::mask::{self, Property};
use or_wardrobe::{Component, Garment, Skin, Slot};

/// How many drawables state each audio id.
type Uses = BTreeMap<u32, u32>;
/// For each slot, how many drawables it has and the ids its two audio fields state.
type Slots = BTreeMap<u8, (u32, Uses, Uses)>;
/// A row of the preset table: the slot, its drawables, and the ids of its two fields with their uses.
type Row = (u8, u32, Vec<(u32, u32)>, Vec<(u32, u32)>);

const WHAT: &str = "the_behaviour_fields_are_the_bits_and_ids_the_staged_documents_state";

fn bits_of(word: u32) -> impl Iterator<Item = u32> {
    (0..32).filter(move |b| word >> b & 1 != 0)
}

#[test]
fn the_fields_a_garment_names_are_the_ones_the_staged_documents_state() {
    let Some(docs) = staged::load(WHAT) else {
        return;
    };
    eprintln!(
        "{} variation documents, {} creature documents",
        docs.variations.len(),
        docs.creatures.len()
    );
    flags_are_the_bits_the_documents_state(&docs);
    a_prop_is_read_through_the_wide_word(&docs);
    the_mask_bits_follow_what_the_drawables_are(&docs);
    the_hair_is_driven_the_way_the_module_states_it(&docs);
    the_audio_presets_are_the_ids_stated_most(&docs);
    the_default_mask_is_the_race_masks(&docs);
}

fn flags_are_the_bits_the_documents_state(docs: &Staged) {
    // The three places the documents state flags: a component's, a prop's narrow word and a prop's wide one.
    let (mut component, mut narrow, mut wide) = (0u32, 0u32, 0u32);
    let mut uses: BTreeMap<u32, usize> = BTreeMap::new();
    for (_, info) in &docs.variations {
        for i in &info.comp_infos {
            component |= i.flags;
            bits_of(i.flags).for_each(|b| *uses.entry(b).or_default() += 1);
        }
        for p in &info.prop_info.a_prop_meta_data {
            narrow |= u32::from(p.flags);
            wide |= p.prop_flags;
            bits_of(u32::from(p.flags)).for_each(|b| *uses.entry(b).or_default() += 1);
            bits_of(p.prop_flags).for_each(|b| *uses.entry(b).or_default() += 1);
        }
    }
    let stated = component | narrow | wide;
    eprintln!("flag bits stated {stated:#X}; uses by bit {uses:?}");
    assert!(
        flags::MEASURED == stated,
        "flags: the bits a screen names are {:#X} and the documents state {stated:#X}",
        flags::MEASURED
    );
    assert_eq!(
        stated >> Flag::ALL.len(),
        0,
        "a bit past the last the engine names"
    );
    assert_eq!(narrow >> 16, 0, "flags: the narrow word holds 16 bits");
    assert!(
        wide >> 16 != 0,
        "flags: propFlags is where the high bits are"
    );
    assert!(
        component & Flag::NotInCar.mask() != 0,
        "remove in vehicle, on a component"
    );
    assert!(
        wide & Flag::UsePhysicsHat2.mask() != 0,
        "hat physics, on a prop"
    );
    assert!(
        component & Flag::Armoured.mask() != 0,
        "armoured, on a component"
    );
}

fn a_prop_is_read_through_the_wide_word(docs: &Staged) {
    // The game reads `propFlags` when it is not zero: the staged props that state both are the reason `word` follows it.
    let mut both = 0usize;
    for (_, info) in &docs.variations {
        for p in &info.prop_info.a_prop_meta_data {
            if p.flags != 0 && p.prop_flags != 0 {
                both += 1;
                let mut g = Garment::new();
                g.slot = Some(Slot::Prop(or_wardrobe::Prop::Head));
                g.meta.flags = Some(u32::from(p.flags));
                g.meta.prop_flags = Some(p.prop_flags);
                assert_eq!(flags::word(&g), p.prop_flags, "wide word");
            }
        }
    }
    eprintln!("{both} staged props state both words");
    assert!(both > 0, "wide word: no staged prop states both");
}

fn the_mask_bits_follow_what_the_drawables_are(docs: &Staged) {
    let mut by_slot: BTreeMap<u8, (usize, usize, usize)> = BTreeMap::new();
    let (mut seen, mut render) = (0u8, 0u8);
    for (_, info) in &docs.variations {
        for (slot, index) in info.avail_comp.iter().enumerate() {
            if *index == u8::MAX {
                continue;
            }
            for d in &info.a_component_data3[usize::from(*index)].a_drawbl_data3 {
                seen |= d.prop_mask;
                let entry = by_slot.entry(slot as u8).or_default();
                entry.0 += 1;
                entry.1 += usize::from(d.prop_mask & Property::RaceTex.mask() != 0);
                entry.2 += usize::from(d.prop_mask & Property::Cutout.mask() != 0);
            }
        }
        for p in &info.prop_info.a_prop_meta_data {
            render |= p.render_flags;
        }
    }
    eprintln!("propMask bits stated {seen:#010b}; (drawables, race, cutout) by slot {by_slot:?}");
    assert_eq!(
        mask::PROPERTIES_MEASURED,
        seen,
        "mask: every bit of propMask is stated"
    );
    assert_eq!(
        mask::RENDER_MEASURED,
        render,
        "mask: the decal pass is stated by no prop"
    );
    let share = |slot: Component, pick: fn(&(usize, usize, usize)) -> usize| {
        let row = by_slot[&slot.id()];
        pick(&row) as f64 / row.0 as f64
    };
    // The meaning each bit is named for: hair is drawn in the cutout pass, and the slots that carry skin take the race textures
    // (the head nearly always, the body about two in three), where clothes and props almost never do.
    assert!(
        share(Component::Hair, |s| s.2) > 0.9,
        "mask: hair is cutout"
    );
    assert!(
        share(Component::Head, |s| s.1) > 0.9,
        "mask: the head follows the skin"
    );
    for slot in [Component::Uppr, Component::Lowr] {
        assert!(
            share(slot, |s| s.1) > 0.5,
            "mask: {slot:?} follows the skin"
        );
    }
    for slot in [
        Component::Berd,
        Component::Accs,
        Component::Jbib,
        Component::Teef,
    ] {
        assert!(
            share(slot, |s| s.1) < 0.1,
            "mask: {slot:?} does not follow the skin"
        );
    }
}

fn the_hair_is_driven_the_way_the_module_states_it(docs: &Staged) {
    let (mut scale, mut height) = (0usize, 0usize);
    for (name, creature) in &docs.creatures {
        for e in &creature.ped_prop_expressions {
            if e.ped_prop_id == 0 && e.ped_prop_expression_index == hair::SCALE_EXPRESSION {
                scale += 1;
                assert_eq!(e.ids, vec![hair::SCALE_BONE], "hair scale in {name}");
                assert!(
                    e.tracks == [33] && e.components == [1],
                    "hair scale in {name}"
                );
            }
        }
        for e in &creature.ped_comp_expressions {
            if e.ped_comp_id == u32::from(Component::Hair.id())
                && e.ped_comp_expression_index == hair::HEIGHT_EXPRESSION
            {
                height += 1;
                assert_eq!(e.ids, vec![hair::HEIGHT_BONE], "hair height in {name}");
            }
        }
    }
    eprintln!("{scale} hair scale entries, {height} hair height entries");
    assert!(scale > 0 && height > 0, "hair: nothing staged drives it");
    // The entry the module writes is the one almost every staged entry is.
    let mut hat = Garment::new();
    hat.slot = Some(Slot::Prop(or_wardrobe::Prop::Head));
    hair::set_scale(&mut hat, Some(-0.5)).unwrap();
    let written = &hat.expressions[0];
    let same = docs
        .creatures
        .iter()
        .flat_map(|(_, c)| &c.ped_prop_expressions)
        .filter(|e| e.ped_prop_id == 0 && e.ped_prop_expression_index == 0)
        .filter(|e| {
            e.tracks == written.tracks
                && e.ids == written.ids
                && e.types == written.types
                && e.components == written.components
        })
        .count();
    assert!(
        same * 100 >= scale * 95,
        "hair: {same} of {scale} entries are the one written"
    );
    // The values the documents give a hat and a hair stay inside what the module accepts.
    let (mut lowest, mut highest) = (f32::MAX, f32::MIN);
    let (mut hats, mut hiding) = (0usize, 0usize);
    for (_, info) in &docs.variations {
        for p in &info.prop_info.a_prop_meta_data {
            let v = p.expression_mods[0];
            if p.anchor_id == 0 && v.abs() > 1e-6 {
                (lowest, highest) = (lowest.min(v), highest.max(v));
                hats += 1;
                hiding += usize::from(v <= hair::HIDES);
            }
        }
    }
    eprintln!(
        "hat hair scale from {lowest} to {highest}; {hiding} of {hats} hats state {} or less",
        hair::HIDES
    );
    assert!(
        hiding > 0,
        "hair: no staged hat states the hide hair preset"
    );
    assert!(highest < 0.0, "hair: a hat's scale squashes the hair");
    assert!(lowest >= -hair::LARGEST, "hair: a staged scale is refused");
    let tallest = docs
        .variations
        .iter()
        .flat_map(|(_, info)| &info.comp_infos)
        .filter(|i| i.ped_xml_comp_idx == Component::Hair.id())
        .map(|i| i.ped_xml_expression_mods[2])
        .fold(0f32, f32::max);
    eprintln!("tallest hair {tallest}");
    assert!(tallest <= hair::LARGEST, "hair: a staged height is refused");
}

fn the_audio_presets_are_the_ids_stated_most(docs: &Staged) {
    let none = or_wardrobe::metadata::hashed("NONE").hash;
    let mut by_slot: Slots = BTreeMap::new();
    for (_, info) in &docs.variations {
        for i in &info.comp_infos {
            let entry = by_slot.entry(i.ped_xml_comp_idx).or_default();
            entry.0 += 1;
            *entry.1.entry(i.ped_xml_audio_id.hash).or_default() += 1;
            *entry.2.entry(i.ped_xml_audio_id2.hash).or_default() += 1;
        }
    }
    let top = |ids: &Uses, drawables: u32| -> Vec<(u32, u32)> {
        let mut list: Vec<(u32, u32)> = ids
            .iter()
            .filter(|(h, n)| **h != none && **h != 0 && **n >= 20.max(drawables / 100))
            .map(|(h, n)| (*h, *n))
            .collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        list.truncate(8);
        list
    };
    let mut computed = Vec::new();
    for (slot, (drawables, first, second)) in &by_slot {
        let (first, second) = (top(first, *drawables), top(second, *drawables));
        if first.is_empty() && second.is_empty() {
            continue;
        }
        computed.push((*slot, *drawables, first, second));
    }
    let table: Vec<Row> = audio::PRESETS
        .iter()
        .map(|p| {
            (
                p.slot.id(),
                p.drawables,
                p.first.to_vec(),
                p.second.to_vec(),
            )
        })
        .collect();
    assert!(
        table == computed,
        "audio: the table is not the documents' own"
    );
}

fn the_default_mask_is_the_race_masks(docs: &Staged) {
    // The most common mask of the upper body is the one a garment of the race skin is given when it states none.
    let mut counts: BTreeMap<u8, usize> = BTreeMap::new();
    let slot = usize::from(Component::Uppr.id());
    for (_, info) in &docs.variations {
        if let Some(index) = info.avail_comp.get(slot).filter(|i| **i != u8::MAX) {
            for d in &info.a_component_data3[usize::from(*index)].a_drawbl_data3 {
                *counts.entry(d.prop_mask).or_default() += 1;
            }
        }
    }
    let (common, _) = counts.iter().max_by_key(|(_, n)| **n).unwrap();
    assert_eq!(
        *common,
        mask::default_for(Skin::Race),
        "mask: the common upper-body mask"
    );
}
