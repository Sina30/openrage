//! The alternate variations of a pack: the drawables that wear an alternate model while a garment of the pack is worn.
//!
//! A set names a ped, the drawables that change (each with the alternate it wears) and the garments whose
//! being worn changes them. A hoodie that squashes the hair, a mask or a hat that needs the hair cut away, and a
//! top that must not clip the duffel bag are each such a set: the game's own hair and bags are targets that are
//! outside the pack, and the pack's garments are the sources. `pedalternatevariations.meta` is written from the
//! sets, and an import makes them back from the file, one set for each group of switches the same garments cause.
//!
//! ⚠ An asset that is a garment of the pack is named by its stable id, so a renumbering follows it; one that is
//! not (the game's hair, another pack's top) is named by its collection, slot and number as the file states it.

use std::collections::BTreeMap;

use or_io::meta::instance::HashedString;
use or_io::meta::{
    CAlternateVariations, SAlternateVariationComponent, SAlternateVariationPed,
    SAlternateVariationSwitch,
};
use serde::{Deserialize, Serialize};

use crate::alloc::Allocation;
use crate::edit::Error;
use crate::import::LeftOut;
use crate::metadata::{hashed, text_of};
use crate::model::{new_id, Wardrobe};
use crate::names::{Component, Ped, Prop, Slot};

/// The most switches the game collects for one garment.
pub const MOST_FOR_ONE_ASSET: usize = or_io::meta::typed::MOST_SWITCHES_FOR_ONE_ASSET;

/// The data file type a manifest registers the file with.
pub const FILE_TYPE: &str = "ALTERNATE_VARIATIONS_FILE";

/// A drawable or prop: a garment of the pack, or one the pack does not hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Asset {
    /// A garment of the pack, by its stable id.
    Garment { garment: String },
    /// An asset of another collection, as the file states it.
    Outside(Outside),
}

/// An asset outside the pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Outside {
    /// The collection it is in, none for the game's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dlc: Option<String>,
    pub slot: Slot,
    pub index: u32,
}

impl Asset {
    pub fn garment(id: &str) -> Self {
        Asset::Garment {
            garment: id.to_string(),
        }
    }

    pub fn outside(dlc: Option<&str>, slot: Slot, index: u32) -> Self {
        Asset::Outside(Outside {
            dlc: dlc.map(str::to_string),
            slot,
            index,
        })
    }
}

/// A drawable that wears alternate `alt` of its model while a set's garments are worn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Switch {
    pub target: Asset,
    /// The number after the model's name: `hair_000_u_1.ydd` is alternate 1.
    pub alt: u8,
}

/// Switches and the garments that cause them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AlternateSet {
    /// Stable for as long as the set exists.
    pub id: String,
    /// A name for people; the game never sees it.
    pub name: String,
    pub ped: Ped,
    /// The drawables that change.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub switches: Vec<Switch>,
    /// What changes them while it is worn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Asset>,
}

impl AlternateSet {
    /// Whether wearing garment `id` changes this set's drawables.
    pub fn has_source(&self, id: &str) -> bool {
        self.sources
            .iter()
            .any(|s| matches!(s, Asset::Garment { garment } if garment == id))
    }
}

fn set_mut<'a>(wardrobe: &'a mut Wardrobe, id: &str) -> Result<&'a mut AlternateSet, Error> {
    wardrobe
        .alternates
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| Error::Refused(format!("no set of alternate variations has the id {id}")))
}

/// A new, empty set for `ped`; its id.
pub fn new_set(wardrobe: &mut Wardrobe, ped: Ped, name: &str) -> Result<String, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Refused("a set needs a name".into()));
    }
    let id = new_id();
    wardrobe.alternates.push(AlternateSet {
        id: id.clone(),
        name: name.to_string(),
        ped,
        switches: Vec::new(),
        sources: Vec::new(),
    });
    Ok(id)
}

pub fn rename_set(wardrobe: &mut Wardrobe, id: &str, name: &str) -> Result<(), Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Refused("a set needs a name".into()));
    }
    set_mut(wardrobe, id)?.name = name.to_string();
    Ok(())
}

pub fn remove_set(wardrobe: &mut Wardrobe, id: &str) -> Result<(), Error> {
    let at = wardrobe
        .alternates
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| Error::Refused(format!("no set of alternate variations has the id {id}")))?;
    wardrobe.alternates.remove(at);
    Ok(())
}

fn check_asset(
    wardrobe: &Wardrobe,
    ped: Ped,
    asset: &Asset,
    component_only: bool,
) -> Result<(), Error> {
    match asset {
        Asset::Garment { garment } => {
            let g = wardrobe
                .garment(garment)
                .ok_or_else(|| Error::NoSuchGarment(garment.clone()))?;
            if g.ped != Some(ped) {
                return Err(Error::Refused(format!(
                    "that garment is not of {ped}, whose set this is"
                )));
            }
            if component_only && g.slot.is_none_or(Slot::is_prop) {
                return Err(Error::Refused(
                    "a drawable that changes is a component, never a prop".into(),
                ));
            }
        }
        Asset::Outside(o) => {
            if component_only && o.slot.is_prop() {
                return Err(Error::Refused(
                    "a drawable that changes is a component, never a prop".into(),
                ));
            }
            if o.index > u32::from(u8::MAX) {
                return Err(Error::Refused(format!(
                    "drawable {} is past the 255 the game counts",
                    o.index
                )));
            }
        }
    }
    Ok(())
}

/// Make `target` wear alternate `alt` while the set's garments are worn.
pub fn add_switch(wardrobe: &mut Wardrobe, set: &str, target: Asset, alt: u8) -> Result<(), Error> {
    if alt == 0 {
        return Err(Error::Refused(
            "alternate 0 is the drawable itself: the first alternate is 1".into(),
        ));
    }
    let ped = set_mut(wardrobe, set)?.ped;
    check_asset(wardrobe, ped, &target, true)?;
    let held = set_mut(wardrobe, set)?;
    if held
        .switches
        .iter()
        .any(|s| s.target == target && s.alt == alt)
    {
        return Err(Error::Refused("the set already has that switch".into()));
    }
    held.switches.push(Switch { target, alt });
    Ok(())
}

pub fn remove_switch(wardrobe: &mut Wardrobe, set: &str, at: usize) -> Result<(), Error> {
    let held = set_mut(wardrobe, set)?;
    if at >= held.switches.len() {
        return Err(Error::Refused(format!("the set has no switch {at}")));
    }
    held.switches.remove(at);
    Ok(())
}

/// Let wearing `source` change the set's drawables.
pub fn join(wardrobe: &mut Wardrobe, set: &str, source: Asset) -> Result<(), Error> {
    let ped = set_mut(wardrobe, set)?.ped;
    check_asset(wardrobe, ped, &source, false)?;
    let held = set_mut(wardrobe, set)?;
    if !held.sources.contains(&source) {
        held.sources.push(source);
    }
    Ok(())
}

/// Stop wearing `source` changing the set's drawables.
pub fn leave(wardrobe: &mut Wardrobe, set: &str, source: &Asset) -> Result<(), Error> {
    set_mut(wardrobe, set)?.sources.retain(|s| s != source);
    Ok(())
}

/// What taking a garment out took out of the sets, so that putting it back can put that back too.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Forgotten {
    sources: Vec<(String, usize, Asset)>,
    switches: Vec<(String, usize, Switch)>,
}

/// Take garment `id` out of every set, as a source and as a drawable that changes; a set left with nothing to do is kept.
pub fn forget(wardrobe: &mut Wardrobe, id: &str) -> Forgotten {
    let is = |a: &Asset| matches!(a, Asset::Garment { garment } if garment == id);
    let mut forgotten = Forgotten::default();
    for set in &mut wardrobe.alternates {
        let mut at = 0;
        set.sources.retain(|s| {
            at += 1;
            let keep = !is(s);
            if !keep {
                forgotten.sources.push((set.id.clone(), at - 1, s.clone()));
            }
            keep
        });
        let mut at = 0;
        set.switches.retain(|s| {
            at += 1;
            let keep = !is(&s.target);
            if !keep {
                forgotten.switches.push((set.id.clone(), at - 1, s.clone()));
            }
            keep
        });
    }
    forgotten
}

/// Put back what [`forget`] took out, where it was.
pub fn restore(wardrobe: &mut Wardrobe, forgotten: &Forgotten) {
    for (set, at, source) in &forgotten.sources {
        if let Some(held) = wardrobe.alternates.iter_mut().find(|s| s.id == *set) {
            held.sources
                .insert((*at).min(held.sources.len()), source.clone());
        }
    }
    for (set, at, switch) in &forgotten.switches {
        if let Some(held) = wardrobe.alternates.iter_mut().find(|s| s.id == *set) {
            held.switches
                .insert((*at).min(held.switches.len()), switch.clone());
        }
    }
}

/// The sets garment `id` is a source of.
pub fn sets_of<'a>(wardrobe: &'a Wardrobe, id: &str) -> Vec<&'a AlternateSet> {
    wardrobe
        .alternates
        .iter()
        .filter(|s| s.has_source(id))
        .collect()
}

/// Where each garment the allocation placed stands: its ped, slot, position in its collection and collection, by stable id.
fn places<'a>(
    wardrobe: &'a Wardrobe,
    allocation: &Allocation,
) -> BTreeMap<&'a str, (Ped, Slot, u32, u32)> {
    allocation
        .placed
        .iter()
        .map(|p| {
            (
                wardrobe.garments[p.garment].id.as_str(),
                (p.ped, p.slot, p.position, p.part),
            )
        })
        .collect()
}

/// One asset as the file spells it, or the reason it cannot be: `(component, anchor, index, collection)`.
fn spelled(
    wardrobe: &Wardrobe,
    places: &BTreeMap<&str, (Ped, Slot, u32, u32)>,
    ped: Ped,
    asset: &Asset,
) -> Result<(Slot, u8, HashedString), String> {
    match asset {
        Asset::Garment { garment } => {
            let (own, slot, position, part) = places
                .get(garment.as_str())
                .copied()
                .ok_or_else(|| format!("garment {garment} is not placed in the pack"))?;
            if own != ped {
                return Err(format!("garment {garment} is not of {ped}"));
            }
            let index = u8::try_from(position)
                .map_err(|_| format!("position {position} is past the 255 the game counts"))?;
            Ok((slot, index, hashed(&wardrobe.collection_of(ped, part))))
        }
        Asset::Outside(o) => {
            let index = u8::try_from(o.index)
                .map_err(|_| format!("drawable {} is past the 255 the game counts", o.index))?;
            let dlc = o.dlc.as_deref().map_or_else(HashedString::default, hashed);
            Ok((o.slot, index, dlc))
        }
    }
}

/// What the plan writes of the alternate variations.
#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    pub root: CAlternateVariations,
    pub switches: usize,
    /// What was left out, and why.
    pub notes: Vec<String>,
}

/// The file the pack's sets make: none when no set has a switch that something causes.
pub fn document(wardrobe: &Wardrobe, allocation: &Allocation) -> Option<Written> {
    let places = places(wardrobe, allocation);
    let mut notes = Vec::new();
    let mut root = CAlternateVariations::default();
    for ped in Ped::ALL {
        let mut switches: Vec<SAlternateVariationSwitch> = Vec::new();
        for set in wardrobe.alternates.iter().filter(|s| s.ped == ped) {
            let mut sources: Vec<SAlternateVariationComponent> = Vec::new();
            for asset in &set.sources {
                match spelled(wardrobe, &places, ped, asset) {
                    Ok((slot, index, dlc)) => {
                        let mut s = SAlternateVariationComponent::default();
                        (s.index, s.dlc_name_hash) = (index, dlc);
                        match slot {
                            Slot::Component(c) => s.component = c.id(),
                            Slot::Prop(p) => s.anchor = p.anchor(),
                        }
                        let same = |o: &SAlternateVariationComponent| {
                            (o.component, o.anchor, o.index, o.dlc_name_hash.hash)
                                == (s.component, s.anchor, s.index, s.dlc_name_hash.hash)
                        };
                        if !sources.iter().any(same) {
                            sources.push(s);
                        }
                    }
                    Err(why) => {
                        notes.push(format!("the set {}: a source is left out, {why}", set.name))
                    }
                }
            }
            if sources.is_empty() {
                if !set.switches.is_empty() {
                    notes.push(format!(
                        "the set {} writes nothing: no garment of it is placed in the pack",
                        set.name
                    ));
                }
                continue;
            }
            for switch in &set.switches {
                let (slot, index, dlc) = match spelled(wardrobe, &places, ped, &switch.target) {
                    Ok(found) => found,
                    Err(why) => {
                        notes.push(format!("the set {}: a switch is left out, {why}", set.name));
                        continue;
                    }
                };
                let Slot::Component(component) = slot else {
                    notes.push(format!(
                        "the set {}: a switch is left out, a prop never changes",
                        set.name
                    ));
                    continue;
                };
                let held = switches.iter_mut().find(|s| {
                    (s.component, s.index, s.alt, s.dlc_name_hash.hash)
                        == (component.id(), index, switch.alt, dlc.hash)
                });
                match held {
                    Some(held) => {
                        for source in &sources {
                            let known = held.source_assets.iter().any(|o| {
                                (o.component, o.anchor, o.index, o.dlc_name_hash.hash)
                                    == (
                                        source.component,
                                        source.anchor,
                                        source.index,
                                        source.dlc_name_hash.hash,
                                    )
                            });
                            if !known {
                                held.source_assets.push(source.clone());
                            }
                        }
                    }
                    None => {
                        let mut made = SAlternateVariationSwitch::default();
                        (made.component, made.index, made.alt) =
                            (component.id(), index, switch.alt);
                        (made.dlc_name_hash, made.source_assets) = (dlc, sources.clone());
                        switches.push(made);
                    }
                }
            }
        }
        if !switches.is_empty() {
            let mut made = SAlternateVariationPed::default();
            (made.name, made.switches) = (HashedString::new(ped.model()), switches);
            root.peds.push(made);
        }
    }
    let count = root.peds.iter().map(|p| p.switches.len()).sum();
    (count > 0).then_some(Written {
        root,
        switches: count,
        notes,
    })
}

/// What the plan says of sets that write nothing, for a build that leaves the file out or has no source to write.
pub fn notes(wardrobe: &Wardrobe, allocation: &Allocation) -> Vec<String> {
    let places = places(wardrobe, allocation);
    wardrobe
        .alternates
        .iter()
        .filter(|set| !set.switches.is_empty())
        .filter(|set| {
            !set.sources
                .iter()
                .any(|s| spelled(wardrobe, &places, set.ped, s).is_ok())
        })
        .map(|set| {
            format!(
                "the set {} writes nothing: no garment of it is placed in the pack",
                set.name
            )
        })
        .collect()
}

/// Something wrong with a set, for the rules to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub code: &'static str,
    /// The garment it is about, where it is about one.
    pub garment: Option<String>,
    pub message: String,
}

/// The alternates a garment of the pack states: `numAlternatives` where it states it, else the models it holds.
fn alternates_of(wardrobe: &Wardrobe, id: &str) -> usize {
    wardrobe.garment(id).map_or(0, |g| {
        g.meta
            .num_alternatives
            .map_or(g.alternates.len(), usize::from)
    })
}

/// Everything wrong with the sets of the pack.
pub fn problems(wardrobe: &Wardrobe, allocation: &Allocation) -> Vec<Problem> {
    let places = places(wardrobe, allocation);
    let mut found = Vec::new();
    for set in &wardrobe.alternates {
        let who = format!("The set {}", set.name);
        for asset in set
            .sources
            .iter()
            .chain(set.switches.iter().map(|s| &s.target))
        {
            if let Err(why) = spelled(wardrobe, &places, set.ped, asset) {
                let garment = match asset {
                    Asset::Garment { garment } => Some(garment.clone()),
                    Asset::Outside(_) => None,
                };
                found.push(Problem {
                    code: "alternate-reference",
                    garment,
                    message: format!("{who} names a garment it cannot use: {why}"),
                });
            }
        }
        for switch in &set.switches {
            if switch.alt == 0 {
                found.push(Problem {
                    code: "alternate-alt",
                    garment: None,
                    message: format!(
                        "{who} has a switch to alternate 0, which is the drawable itself"
                    ),
                });
            }
            if let Asset::Garment { garment } = &switch.target {
                let held = alternates_of(wardrobe, garment);
                if wardrobe.garment(garment).is_some() && usize::from(switch.alt) > held {
                    found.push(Problem {
                        code: "alternate-alt",
                        garment: Some(garment.clone()),
                        message: format!(
                            "{who} makes a drawable wear alternate {}, and it has {held}",
                            switch.alt
                        ),
                    });
                }
            }
        }
    }
    if let Some(written) = document(wardrobe, allocation) {
        for ped in &written.root.peds {
            let mut counts: BTreeMap<(u8, u8, u8, u32), usize> = BTreeMap::new();
            for asset in ped.switches.iter().flat_map(|s| &s.source_assets) {
                *counts
                    .entry((
                        asset.component,
                        asset.anchor,
                        asset.index,
                        asset.dlc_name_hash.hash,
                    ))
                    .or_default() += 1;
            }
            if let Some((_, most)) = counts.iter().max_by_key(|(_, n)| **n) {
                if *most > MOST_FOR_ONE_ASSET {
                    found.push(Problem {
                        code: "alternate-limit",
                        garment: None,
                        message: format!(
                            "A garment of {} changes {most} drawables, and the game holds at most {MOST_FOR_ONE_ASSET} for one",
                            text_of(&ped.name)
                        ),
                    });
                }
            }
        }
    }
    found
}

/// What an import made of the alternate variations it read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Read {
    /// The files read.
    pub files: Vec<String>,
    pub sets: usize,
    pub switches: usize,
    /// Sources that are garments of the pack, and sources that are not.
    pub inside: usize,
    pub outside: usize,
    /// What the pack could not take.
    pub left_out: Vec<LeftOut>,
}

fn slot_of(component: u8, anchor: u8) -> Option<Slot> {
    match (component, anchor) {
        (u8::MAX, u8::MAX) => None,
        (u8::MAX, anchor) => Prop::from_anchor(anchor).map(Slot::Prop),
        (component, _) => Component::from_id(component).map(Slot::Component),
    }
}

/// The switches that one list of sources causes, as an import finds them.
struct Group {
    /// The sources as the file spells them: component, anchor, number and collection hash.
    key: Vec<(u8, u8, u8, u32)>,
    sources: Vec<Asset>,
    switches: Vec<Switch>,
}

/// Make sets of the switches `root` states for the two freemode peds, one for each group of switches the same assets cause.
pub fn apply(wardrobe: &mut Wardrobe, root: &CAlternateVariations, shown: &str, read: &mut Read) {
    read.files.push(shown.to_string());
    let held: BTreeMap<(Ped, Slot, u32), String> = wardrobe
        .garments
        .iter()
        .filter_map(|g| Some(((g.ped?, g.slot?, g.position?), g.id.clone())))
        .collect();
    for ped_switches in &root.peds {
        let Some(ped) = Ped::from_model(&text_of(&ped_switches.name)) else {
            read.left_out.push(LeftOut {
                path: shown.to_string(),
                why: format!(
                    "switches of {}, which is not one of the two freemode peds",
                    text_of(&ped_switches.name)
                ),
            });
            continue;
        };
        let own = hashed(&wardrobe.collection(ped)).hash;
        let asset =
            |slot: Slot, index: u8, dlc: &HashedString, inside: &mut usize, outside: &mut usize| {
                if dlc.hash == own {
                    if let Some(id) = held.get(&(ped, slot, u32::from(index))) {
                        *inside += 1;
                        return Asset::garment(id);
                    }
                }
                *outside += 1;
                Asset::outside(
                    (dlc.hash != 0).then(|| text_of(dlc)).as_deref(),
                    slot,
                    u32::from(index),
                )
            };
        // Groups in the order they first appear, keyed by the sources a switch lists.
        let mut groups: Vec<Group> = Vec::new();
        for switch in &ped_switches.switches {
            let Some(target_slot) = Component::from_id(switch.component).map(Slot::Component)
            else {
                read.left_out.push(LeftOut {
                    path: shown.to_string(),
                    why: format!(
                        "a switch of component {}, which no slot has",
                        switch.component
                    ),
                });
                continue;
            };
            let key: Vec<(u8, u8, u8, u32)> = switch
                .source_assets
                .iter()
                .map(|a| (a.component, a.anchor, a.index, a.dlc_name_hash.hash))
                .collect();
            let at = match groups.iter().position(|g| g.key == key) {
                Some(at) => at,
                None => {
                    let mut sources = Vec::new();
                    for a in &switch.source_assets {
                        let Some(slot) = slot_of(a.component, a.anchor) else {
                            read.left_out.push(LeftOut {
                                path: shown.to_string(),
                                why: "a source that is neither a component nor a prop".into(),
                            });
                            continue;
                        };
                        let found = asset(
                            slot,
                            a.index,
                            &a.dlc_name_hash,
                            &mut read.inside,
                            &mut read.outside,
                        );
                        if !sources.contains(&found) {
                            sources.push(found);
                        }
                    }
                    groups.push(Group {
                        key,
                        sources,
                        switches: Vec::new(),
                    });
                    groups.len() - 1
                }
            };
            let (mut inside, mut outside) = (0, 0);
            let target = asset(
                target_slot,
                switch.index,
                &switch.dlc_name_hash,
                &mut inside,
                &mut outside,
            );
            let found = Switch {
                target,
                alt: switch.alt,
            };
            if !groups[at].switches.contains(&found) {
                groups[at].switches.push(found);
                read.switches += 1;
            }
        }
        for (
            n,
            Group {
                sources, switches, ..
            },
        ) in groups.into_iter().enumerate()
        {
            read.sets += 1;
            wardrobe.alternates.push(AlternateSet {
                id: new_id(),
                name: format!("{} alternates {}", ped.letter().to_ascii_uppercase(), n + 1),
                ped,
                switches,
                sources,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::allocate;
    use crate::model::Garment;

    fn garment(ped: Ped, slot: Slot, position: u32) -> Garment {
        let mut g = Garment::new();
        g.ped = Some(ped);
        g.slot = Some(slot);
        g.position = Some(position);
        g.model = Some(format!("{slot}_{position}.ydd"));
        g
    }

    const JBIB: Slot = Slot::Component(Component::Jbib);
    const HAIR: Slot = Slot::Component(Component::Hair);
    const HAT: Slot = Slot::Prop(Prop::Head);

    fn pack() -> (Wardrobe, String, String, String) {
        let mut w = Wardrobe::new("unity");
        let hoodie = garment(Ped::Male, JBIB, 3);
        let hat = garment(Ped::Male, HAT, 1);
        let hair = garment(Ped::Male, HAIR, 0);
        let ids = (hoodie.id.clone(), hat.id.clone(), hair.id.clone());
        w.garments.extend([hoodie, hat, hair]);
        (w, ids.0, ids.1, ids.2)
    }

    #[test]
    fn a_set_writes_a_switch_for_each_drawable_and_the_assets_that_cause_it() {
        let (mut w, hoodie, hat, hair) = pack();
        let set = new_set(&mut w, Ped::Male, "hair under a hood").unwrap();
        add_switch(
            &mut w,
            &set,
            Asset::outside(Some("mp_m_gunrunning_hair_01"), HAIR, 4),
            3,
        )
        .unwrap();
        add_switch(&mut w, &set, Asset::outside(None, HAIR, 7), 1).unwrap();
        add_switch(&mut w, &set, Asset::garment(&hair), 2).unwrap();
        join(&mut w, &set, Asset::garment(&hoodie)).unwrap();
        join(&mut w, &set, Asset::garment(&hat)).unwrap();
        let written = document(&w, &allocate(&w)).expect("a set with a source writes");
        assert_eq!(written.switches, 3);
        let ped = &written.root.peds[0];
        assert_eq!(ped.name, "mp_m_freemode_01");
        let first = &ped.switches[0];
        assert_eq!((first.component, first.index, first.alt), (2, 4, 3));
        assert_eq!(first.dlc_name_hash, "mp_m_gunrunning_hair_01");
        assert_eq!(first.source_assets.len(), 2);
        // The hoodie is a component of the pack's collection at its position, the hat a prop at its anchor.
        assert_eq!(
            (
                first.source_assets[0].component,
                first.source_assets[0].anchor,
                first.source_assets[0].index
            ),
            (11, 255, 3)
        );
        assert_eq!(first.source_assets[0].dlc_name_hash, "mp_m_unity");
        assert_eq!(
            (
                first.source_assets[1].component,
                first.source_assets[1].anchor,
                first.source_assets[1].index
            ),
            (255, 0, 1)
        );
        assert_eq!(
            ped.switches[1].dlc_name_hash.hash, 0,
            "the game's own hair states no collection"
        );
        // A drawable of the pack changes by its place.
        assert_eq!((ped.switches[2].component, ped.switches[2].index), (2, 0));
        assert_eq!(ped.switches[2].dlc_name_hash, "mp_m_unity");
        // What it writes is a file: it saves, and reads back as it was.
        use or_io::decl::File;
        let bytes = written.root.to_bytes().expect("it saves");
        assert!(CAlternateVariations::from_bytes(&bytes).expect("it reopens") == written.root);
    }

    #[test]
    fn a_set_with_no_source_in_the_pack_writes_nothing_and_says_so() {
        let (mut w, hoodie, ..) = pack();
        let set = new_set(&mut w, Ped::Male, "empty").unwrap();
        add_switch(&mut w, &set, Asset::outside(None, HAIR, 7), 1).unwrap();
        assert!(document(&w, &allocate(&w)).is_none());
        join(&mut w, &set, Asset::garment(&hoodie)).unwrap();
        w.garments.retain(|g| g.id != hoodie);
        assert!(document(&w, &allocate(&w)).is_none());
    }

    #[test]
    fn two_sets_that_switch_one_drawable_to_one_alternate_share_a_switch() {
        let (mut w, hoodie, hat, _) = pack();
        for (name, source) in [("a", &hoodie), ("b", &hat)] {
            let set = new_set(&mut w, Ped::Male, name).unwrap();
            add_switch(&mut w, &set, Asset::outside(None, HAIR, 7), 1).unwrap();
            join(&mut w, &set, Asset::garment(source)).unwrap();
        }
        let written = document(&w, &allocate(&w)).unwrap();
        assert_eq!(written.switches, 1);
        assert_eq!(written.root.peds[0].switches[0].source_assets.len(), 2);
    }

    #[test]
    fn a_set_refuses_what_the_game_would_misread() {
        let (mut w, hoodie, hat, _) = pack();
        let set = new_set(&mut w, Ped::Male, "x").unwrap();
        assert!(
            add_switch(&mut w, &set, Asset::outside(None, HAIR, 1), 0).is_err(),
            "alternate 0"
        );
        assert!(
            add_switch(&mut w, &set, Asset::garment(&hat), 1).is_err(),
            "a prop never changes"
        );
        assert!(add_switch(&mut w, &set, Asset::outside(None, HAT, 1), 1).is_err());
        assert!(add_switch(&mut w, &set, Asset::outside(None, HAIR, 300), 1).is_err());
        add_switch(&mut w, &set, Asset::outside(None, HAIR, 1), 1).unwrap();
        assert!(
            add_switch(&mut w, &set, Asset::outside(None, HAIR, 1), 1).is_err(),
            "twice"
        );
        assert!(join(&mut w, &set, Asset::garment("nobody")).is_err());
        let mut female = garment(Ped::Female, JBIB, 0);
        let female_id = female.id.clone();
        female.model = Some("f.ydd".into());
        w.garments.push(female);
        assert!(
            join(&mut w, &set, Asset::garment(&female_id)).is_err(),
            "the other ped's"
        );
        join(&mut w, &set, Asset::garment(&hoodie)).unwrap();
        join(&mut w, &set, Asset::garment(&hoodie)).unwrap();
        assert_eq!(w.alternates[0].sources.len(), 1);
    }

    #[test]
    fn taking_a_garment_out_takes_it_out_of_the_sets_and_a_set_names_those_it_is_a_source_of() {
        let (mut w, hoodie, hat, hair) = pack();
        let set = new_set(&mut w, Ped::Male, "x").unwrap();
        add_switch(&mut w, &set, Asset::garment(&hair), 1).unwrap();
        join(&mut w, &set, Asset::garment(&hoodie)).unwrap();
        join(&mut w, &set, Asset::garment(&hat)).unwrap();
        assert_eq!(sets_of(&w, &hoodie).len(), 1);
        assert!(sets_of(&w, &hair).is_empty());
        let before = w.clone();
        let first = forget(&mut w, &hoodie);
        let second = forget(&mut w, &hair);
        assert_eq!(w.alternates[0].sources, vec![Asset::garment(&hat)]);
        assert!(w.alternates[0].switches.is_empty());
        // Putting them back puts them back where they were.
        restore(&mut w, &second);
        restore(&mut w, &first);
        assert!(w == before, "the sets are as they were");
    }

    #[test]
    fn the_rules_name_a_dead_reference_an_alternate_the_drawable_lacks_and_too_many_switches() {
        let (mut w, hoodie, _, hair) = pack();
        let set = new_set(&mut w, Ped::Male, "x").unwrap();
        add_switch(&mut w, &set, Asset::garment(&hair), 2).unwrap();
        join(&mut w, &set, Asset::garment(&hoodie)).unwrap();
        let codes = |w: &Wardrobe| -> Vec<&'static str> {
            problems(w, &allocate(w)).iter().map(|p| p.code).collect()
        };
        assert_eq!(codes(&w), ["alternate-alt"], "the hair has no alternates");
        w.garments[2].alternates = vec!["a.ydd".into(), "b.ydd".into()];
        assert!(codes(&w).is_empty());
        w.alternates[0].sources.push(Asset::garment("gone"));
        assert_eq!(codes(&w), ["alternate-reference"]);
        w.alternates[0].sources.pop();
        // One garment that causes more switches than the game holds for it.
        for i in 0..=MOST_FOR_ONE_ASSET {
            add_switch(
                &mut w,
                &set,
                Asset::outside(None, HAIR, 10 + u32::try_from(i % 200).unwrap()),
                1 + u8::try_from(i / 200).unwrap(),
            )
            .unwrap();
        }
        assert!(codes(&w).contains(&"alternate-limit"), "{:?}", codes(&w));
    }

    #[test]
    fn an_import_makes_a_set_of_each_group_of_switches_the_same_assets_cause() {
        let (w, hoodie, ..) = pack();
        let mut w = w;
        let set = new_set(&mut w, Ped::Male, "x").unwrap();
        add_switch(&mut w, &set, Asset::outside(Some("mp_m_a"), HAIR, 4), 1).unwrap();
        add_switch(&mut w, &set, Asset::outside(None, HAIR, 5), 3).unwrap();
        join(&mut w, &set, Asset::garment(&hoodie)).unwrap();
        join(&mut w, &set, Asset::outside(Some("mp_m_other"), JBIB, 2)).unwrap();
        let written = document(&w, &allocate(&w)).unwrap();

        let mut back = Wardrobe::new("unity");
        back.garments = w.garments.clone();
        let mut read = Read::default();
        apply(&mut back, &written.root, "data/x.meta", &mut read);
        assert_eq!(
            (read.sets, read.switches, read.inside, read.outside),
            (1, 2, 1, 1)
        );
        let again = &back.alternates[0];
        assert_eq!(again.switches, w.alternates[0].switches);
        assert_eq!(again.sources, w.alternates[0].sources);
        assert_eq!(
            document(&back, &allocate(&back)).unwrap().root,
            written.root
        );
    }
}
