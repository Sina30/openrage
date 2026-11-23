//! The heads of the ped's own: each is a race model that wears the tone its own first texture states, and the tone a person chose only picks the head worn when none is asked for.
//!
//! No renderer, no install and no corpus: the ped's outfit is built here from the metadata's records, with the `texId` the freemode peds of an install state for their heads.

use or_io::meta::{CPVComponentData, CPVDrawblData, CPVTextureData, CPedVariationInfo};
use or_wardrobe::dress::{dress, BaseEdit, BaseOutfit, By, Choice, Tone};
use or_wardrobe::{Component, Ped, Wardrobe};

/// The `texId` a head of each tone states, in the order of [`Tone::ALL`]: the heads come two to a tone.
const TEX_IDS: [u8; 6] = [1, 2, 4, 3, 10, 5];

/// A ped with two heads of each tone, in the order of [`Tone::ALL`].
fn base() -> BaseOutfit {
    let mut info = CPedVariationInfo::default();
    info.avail_comp = vec![255; Component::ALL.len()];
    info.avail_comp[usize::from(Component::Head.id())] = 0;
    let mut data = CPVComponentData::default();
    data.a_drawbl_data3 = TEX_IDS
        .iter()
        .flat_map(|id| [*id, *id])
        .map(|tex_id| {
            let mut texture = CPVTextureData::default();
            texture.tex_id = tex_id;
            let mut drawable = CPVDrawblData::default();
            drawable.a_tex_data = vec![texture];
            drawable
        })
        .collect();
    info.a_component_data3.push(data);
    BaseOutfit::of(&info)
}

fn head_asked_for(chosen: Tone, n: u32) -> By {
    let mut choice = Choice::new(Ped::Male);
    choice.tone = chosen;
    choice.base.insert(Component::Head, BaseEdit::Drawable(n));
    let dressing = dress(&Wardrobe::new("demo"), &base(), &choice);
    dressing
        .slot(Component::Head)
        .map(|w| w.by.clone())
        .unwrap_or_else(|| panic!("the head is worn: {dressing:?}"))
}

#[test]
fn every_head_of_the_peds_own_is_a_race_model_whatever_tone_its_texture_states() {
    assert_eq!(base().drawables(Component::Head), 2 * Tone::ALL.len());
    for n in 0..base().drawables(Component::Head) as u32 {
        let By::Base { model, .. } = head_asked_for(Tone::White, n) else {
            panic!("head {n} is the ped's own");
        };
        assert_eq!(model, format!("head_{n:03}_r.ydd"), "head {n}");
    }
}

#[test]
fn a_head_asked_for_wears_the_tone_its_own_texture_states_and_not_the_one_chosen() {
    for chosen in Tone::ALL {
        for n in 0..base().drawables(Component::Head) as u32 {
            let own = Tone::ALL[n as usize / 2];
            let By::Base { texture, .. } = head_asked_for(chosen, n) else {
                panic!("head {n} is the ped's own");
            };
            assert_eq!(
                texture,
                Some(format!("head_diff_{n:03}_a_{}.ytd", own.word())),
                "head {n} with {chosen:?} chosen"
            );
        }
    }
}
