//! The edits a screen makes to a pack, so that the screen holds no rule and writes nothing of its own.
//!
//! Dropping files adds the garments their names state and Undo takes them back, [`apply`] changes
//! what one garment states, a [`Remedy`] is the fix of a finding with the sentence that says what it
//! changes, and [`waive`] records the reason a finding stands. Every function either does what it
//! says or returns the reason and changes nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use or_job::{Cancel, Progress};

use sha2::{Digest, Sha256};

use crate::alloc::{allocate, places_of, Placed};
use crate::images;
use crate::import::LeftOut;
use crate::intake::{self, content_digest, Scratch};
use crate::model::{self, Carried, Garment, ShopEntry, Stated, Variation, Waiver, Wardrobe};
use crate::names::{Ped, Skin, Slot};
use crate::redo::{undone, Redo};
use crate::resolve::{resolve, Resolved, Role};
use crate::rules::{name_of, rule_of, Finding};
use crate::shop::{self, Offered, ShopReport};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Model(#[from] model::Error),
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    #[error("the pack has no garment with the id {0}")]
    NoSuchGarment(String),
    #[error("{0}")]
    Refused(String),
    #[error("stopped before it was done, so nothing was changed")]
    Stopped,
}

/// Read the pack in `folder`, change it and write it back; nothing is written when the change fails.
pub fn update<T>(
    folder: &Path,
    change: impl FnOnce(&mut Wardrobe) -> Result<T, Error>,
) -> Result<T, Error> {
    let mut wardrobe = Wardrobe::read(folder)?;
    let done = change(&mut wardrobe)?;
    wardrobe.write(folder)?;
    Ok(done)
}

/// A change made to a pack, with the pack as it stood before and as the change left it, which is all Undo needs to take it back.
#[derive(Debug, Clone, PartialEq)]
pub struct Revision {
    pub before: Wardrobe,
    pub after: Wardrobe,
    /// Files this copied into the module's folder: Undo removes these, unless a garment names them, and nothing else.
    pub copied: Vec<String>,
    /// The folders inside the module's folder that this made.
    pub made: Vec<String>,
}

/// Read the pack in `folder`, change it and write it back as [`update`] does, and keep what it was and what it became; none when the change left the pack as it was.
pub fn revise<T>(
    folder: &Path,
    change: impl FnOnce(&mut Wardrobe) -> Result<T, Error>,
) -> Result<(T, Option<Revision>), Error> {
    let before = Wardrobe::read(folder)?;
    let mut wardrobe = before.clone();
    let done = change(&mut wardrobe)?;
    wardrobe.write(folder)?;
    // ⚠ Read back: the pack Undo compares with is the one the disk holds, not the one this built.
    let after = Wardrobe::read(folder)?;
    let revision = (after != before).then(|| Revision {
        before,
        after,
        copied: Vec::new(),
        made: Vec::new(),
    });
    Ok((done, revision))
}

/// Take back a [`Revision`]: the pack is as it was, and the files the change copied go unless a garment names them; the [`Redo`] makes it again.
///
/// ⚠ Refused when the pack is not as the change left it, because something has changed it since and writing the old one back would lose that.
pub fn undo_revision(folder: &Path, revision: &Revision) -> Result<Redo, Error> {
    if !Wardrobe::read(folder)?.states_the_same_as(&revision.after) {
        return Err(Error::Refused(
            "the pack was changed after that, so taking it back would lose the later changes: take those back first".into(),
        ));
    }
    undone(folder, &revision.copied, &revision.made, |wardrobe| {
        wardrobe.clone_from(&revision.before);
        Ok(())
    })
}

/// Texture `variation` of `garment` if it is the one that names `named`, else why not: a row that was drawn before an edit moved the textures must not act on another one.
fn texture_named<'a>(
    garment: &'a mut Garment,
    variation: usize,
    named: &str,
) -> Result<&'a mut Variation, Error> {
    let held = garment.variations.len();
    let at = garment.variations.get_mut(variation).ok_or_else(|| {
        Error::Refused(format!(
            "the garment has {held} texture(s), not {}",
            variation + 1
        ))
    })?;
    match at.texture.as_deref() == Some(named) {
        true => Ok(at),
        false => Err(Error::Refused(
            "that texture is not the one that was missing any more: look at the problems again"
                .into(),
        )),
    }
}

/// Point texture `variation` of garment `id`, which names the file `named`, at the file `source`, copying it into `folder` unless it is already inside: the way to put right a texture that is not where the pack says.
pub fn locate_texture(
    folder: &Path,
    id: &str,
    variation: usize,
    named: &str,
    source: &Path,
) -> Result<Revision, Error> {
    if !source.exists() {
        return Err(Error::Refused(format!(
            "{} is not a file",
            source.display()
        )));
    }
    located(folder, id, variation, named, |copying| {
        intake::texture(copying, &mut Scratch::default(), source)
    })
}

/// [`locate_texture`] with what `reference` leads to, which stays where it is.
pub fn locate_texture_to(
    folder: &Path,
    id: &str,
    variation: usize,
    named: &str,
    reference: &or_project::reference::Reference,
) -> Result<Revision, Error> {
    located(folder, id, variation, named, |copying| {
        copying.put_reference("textures", reference)
    })
}

/// Point texture `variation` of garment `id`, which names the file `named`, at what `put` places in the pack.
fn located(
    folder: &Path,
    id: &str,
    variation: usize,
    named: &str,
    put: impl FnOnce(&mut Copying) -> Result<String, Error>,
) -> Result<Revision, Error> {
    let before = Wardrobe::read(folder)?;
    let mut wardrobe = before.clone();
    texture_named(garment_mut(&mut wardrobe, id)?, variation, named)?;
    let mut copying = Copying::new(folder);
    let rel = match put(&mut copying) {
        Ok(rel) => rel,
        Err(e) => {
            copying.undo();
            return Err(e);
        }
    };
    texture_named(garment_mut(&mut wardrobe, id)?, variation, named)?.texture = Some(rel);
    if let Err(e) = wardrobe.write(folder) {
        copying.undo();
        return Err(e.into());
    }
    Ok(Revision {
        before,
        after: Wardrobe::read(folder)?,
        copied: copying.copied,
        made: copying.made,
    })
}

/// Take texture `variation` out of garment `id`, which names the file `named`; its file stays, and the textures after it move up one letter.
pub fn remove_texture(
    wardrobe: &mut Wardrobe,
    id: &str,
    variation: usize,
    named: &str,
) -> Result<(), Error> {
    let garment = garment_mut(wardrobe, id)?;
    texture_named(garment, variation, named)?;
    garment.variations.remove(variation);
    Ok(())
}

/// A garment that another place of its slot gave up, at the place it had and the one it has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    /// The garment's id.
    pub garment: String,
    /// What the screens call it.
    pub name: String,
    pub from: u32,
    pub to: u32,
}

/// The garments of `after` whose place is not the one they had in `before`, in the order of their places.
///
/// ⚠ A garment pinned at a place never moves, whatever leaves around it: only those that take the lowest free place do.
pub fn moved(before: &Wardrobe, after: &Wardrobe) -> Vec<Moved> {
    let (was, now) = (allocate(before), allocate(after));
    now.placed
        .iter()
        .filter_map(|p| {
            let garment = &after.garments[p.garment];
            let earlier = was
                .placed
                .iter()
                .find(|q| before.garments[q.garment].id == garment.id)?;
            (earlier.place != p.place).then(|| Moved {
                garment: garment.id.clone(),
                name: name_of(garment),
                from: earlier.place,
                to: p.place,
            })
        })
        .collect()
}

/// Make `folder` a clothing module holding an empty pack called `pack`; a `wardrobe.json` already there is never replaced.
pub fn create(folder: &Path, pack: &str) -> Result<(), Error> {
    if !crate::names::is_identifier(pack) {
        return Err(not_a_pack_name(pack));
    }
    if folder.join(model::FILE).exists() {
        return Err(Error::Refused(format!(
            "{} already holds a {}",
            folder.display(),
            model::FILE
        )));
    }
    std::fs::create_dir_all(folder).map_err(|e| Error::Io(folder.display().to_string(), e))?;
    Wardrobe::new(pack).write(folder).map_err(Error::from)
}

/// One thing a person says about a garment.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Label(Option<String>),
    Ped(Option<Ped>),
    Slot(Option<Slot>),
    /// Pin it at a place, or let it take the lowest free one.
    Position(Option<u32>),
    Skin(Skin),
    /// The heel value a feet drawable states, or none to take its heels off ([`crate::heels`]).
    Heels(Option<f32>),
    /// How the garment behaves: its flags, its mask, its audio, what a hat does to the hair ([`crate::behaviour`]).
    Behaviour(crate::behaviour::Behaviour),
    /// File it under a word; the same word in another case is the same tag, and a word of nothing is refused.
    AddTag(String),
    /// Take a tag off, whatever its case.
    RemoveTag(String),
    /// Keep it in a named set, or in none.
    Group(Option<String>),
    /// The wind scale the build states in its cloth, or none to ship the cloth as its file is ([`crate::wind`]).
    Wind(Option<f32>),
    /// What the shop entry of one texture states, by the texture's number ([`crate::shop_edit`]).
    Shop(usize, crate::shop_edit::Field),
    /// Take it out of the pack; its files stay where they are.
    Remove,
}

/// Whether two tags are the same word, whatever the case and the space around it.
fn same_word(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

pub fn apply(wardrobe: &mut Wardrobe, id: &str, edit: Edit) -> Result<(), Error> {
    if edit == Edit::Remove {
        let leave = wardrobe.rules.dummies.leave_on_delete;
        return crate::dummies::take_out(wardrobe, id, leave).map(|_| ());
    }
    let at = wardrobe
        .garments
        .iter()
        .position(|g| g.id == id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))?;
    let garment = &mut wardrobe.garments[at];
    match edit {
        Edit::Label(label) => garment.label = label.filter(|l| !l.trim().is_empty()),
        Edit::Ped(ped) => garment.ped = ped,
        Edit::Slot(slot) => garment.slot = slot,
        Edit::Position(position) => garment.position = position,
        Edit::Skin(skin) => crate::mask::follow_skin(garment, skin),
        Edit::Heels(value) => crate::heels::set(garment, value)?,
        Edit::Behaviour(change) => crate::behaviour::apply(garment, change)?,
        Edit::AddTag(tag) => {
            let tag = tag.trim();
            if tag.is_empty() {
                return Err(Error::Refused("a tag needs a word".into()));
            }
            if !garment.tags.iter().any(|t| same_word(t, tag)) {
                garment.tags.push(tag.to_string());
            }
        }
        Edit::RemoveTag(tag) => garment.tags.retain(|t| !same_word(t, &tag)),
        Edit::Group(group) => {
            garment.group = group
                .map(|g| g.trim().to_string())
                .filter(|g| !g.is_empty())
        }
        Edit::Wind(scale) => crate::wind::set(garment, scale)?,
        Edit::Shop(texture, field) => crate::shop_edit::apply(garment, texture, field)?,
        Edit::Remove => unreachable!("a removal is made before the garment is borrowed"),
    }
    Ok(())
}

/// Make `edit` to each of the garments `ids`, all or none: one id the pack does not have leaves every garment as it was.
pub fn apply_all(wardrobe: &mut Wardrobe, ids: &[String], edit: Edit) -> Result<(), Error> {
    if let Some(missing) = ids.iter().find(|id| wardrobe.garment(id).is_none()) {
        return Err(Error::NoSuchGarment(missing.clone()));
    }
    let mut each: Vec<&String> = Vec::new();
    for id in ids {
        if !each.contains(&id) {
            each.push(id);
        }
    }
    for id in each {
        apply(wardrobe, id, edit.clone())?;
    }
    Ok(())
}

/// Which way a garment moves among the garments of its slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toward {
    Earlier,
    Later,
}

/// Swap the place of garment `id` with that of the garment next to it in its slot, the one before it or the one after.
///
/// ⚠ A place that must stay with a garment is a pin. When either of the two is pinned both are pinned at the places they
/// swap to, and when neither is they swap their order in the list and stay as free as they were. Either way the name the
/// game sees changes for both, and the next build's diff says so by their stable ids.
pub fn reorder(wardrobe: &mut Wardrobe, id: &str, toward: Toward) -> Result<(), Error> {
    let index = (wardrobe.garments.iter())
        .position(|g| g.id == id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))?;
    let allocation = allocate(wardrobe);
    let name = name_of(&wardrobe.garments[index]);
    let Some(me) = allocation.of(index).copied() else {
        return Err(Error::Refused(format!(
            "{name} has no place yet: give it a ped and a slot, and a place no pin holds"
        )));
    };
    // `placed` lists a slot by position.
    let slot: Vec<Placed> = allocation.in_slot(me.ped, me.slot).copied().collect();
    let at = slot
        .iter()
        .position(|p| p.garment == index)
        .expect("a placed garment is in its slot");
    let (next, end) = match toward {
        Toward::Earlier => (at.checked_sub(1), "first"),
        Toward::Later => (Some(at + 1), "last"),
    };
    let Some(other) = next.and_then(|n| slot.get(n)) else {
        return Err(Error::Refused(format!(
            "{name} is the {end} of its slot already"
        )));
    };
    let pinned = wardrobe.garments[index].position.is_some()
        || wardrobe.garments[other.garment].position.is_some();
    match pinned {
        true => {
            wardrobe.garments[index].position = Some(other.place);
            wardrobe.garments[other.garment].position = Some(me.place);
        }
        false => wardrobe.garments.swap(index, other.garment),
    }
    Ok(())
}

/// A garment taken out of a pack, whole, with the place it held in the pack's order.
#[derive(Debug, Clone, PartialEq)]
pub struct Removed {
    pub at: usize,
    pub garment: Garment,
    /// The dummy that took its place when the pack leaves one, which putting the garment back lets go.
    pub left: Option<String>,
    /// What it was in the sets of alternate variations, which putting it back restores.
    pub sets: crate::alternate::Forgotten,
}

/// Take garment `id` out of the pack, keeping all it states so that [`put_back`] can return it.
pub fn take_out(wardrobe: &mut Wardrobe, id: &str) -> Result<Removed, Error> {
    let leave = wardrobe.rules.dummies.leave_on_delete;
    let taken = crate::dummies::take_out(wardrobe, id, leave)?;
    Ok(Removed {
        at: taken.at,
        garment: taken.garment,
        left: taken.left,
        sets: taken.sets,
    })
}

/// Put a garment taken out back where it was; refused while a garment of its id is in the pack.
pub fn put_back(wardrobe: &mut Wardrobe, removed: &Removed) -> Result<(), Error> {
    if wardrobe.garment(&removed.garment.id).is_some() {
        return Err(Error::Refused(
            "that garment is already in the pack".to_string(),
        ));
    }
    // ⚠ The dummy that held the place goes only while it is still a dummy: one somebody gave a model since is theirs.
    if let Some(held) = &removed.left {
        wardrobe
            .garments
            .retain(|g| !(&g.id == held && g.is_reserved()));
    }
    let at = removed.at.min(wardrobe.garments.len());
    wardrobe.garments.insert(at, removed.garment.clone());
    crate::alternate::restore(wardrobe, &removed.sets);
    Ok(())
}

fn not_a_pack_name(name: &str) -> Error {
    Error::Refused(format!(
        "{name} is not a pack name: lower case letters, digits and underscores, starting with a letter"
    ))
}

/// Name the pack `name`, the stem of its collections and the folder of its streamed files.
pub fn rename_pack(wardrobe: &mut Wardrobe, name: &str) -> Result<(), Error> {
    if !crate::names::is_identifier(name) {
        return Err(not_a_pack_name(name));
    }
    wardrobe.pack = name.to_string();
    Ok(())
}

/// Record that finding `code` of `garment` (every garment where none) stands, for `reason`.
pub fn waive(
    wardrobe: &mut Wardrobe,
    code: &str,
    garment: Option<&str>,
    reason: &str,
) -> Result<(), Error> {
    let Some(rule) = rule_of(code) else {
        return Err(Error::Refused(format!("`{code}` is no rule")));
    };
    if !rule.waivable {
        return Err(Error::Refused(format!(
            "`{code}` cannot be waived: no reason mends it"
        )));
    }
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(Error::Refused("a waiver needs a reason".into()));
    }
    if let Some(id) = garment {
        if wardrobe.garment(id).is_none() {
            return Err(Error::NoSuchGarment(id.to_string()));
        }
    }
    let garment = garment.map(String::from);
    match (wardrobe.waivers.iter_mut()).find(|w| w.code == code && w.garment == garment) {
        Some(held) => held.reason = reason.to_string(),
        None => wardrobe.waivers.push(Waiver {
            code: code.to_string(),
            garment,
            reason: reason.to_string(),
        }),
    }
    Ok(())
}

/// What a fix changes.
#[derive(Debug, Clone, PartialEq)]
enum Change {
    Unpin(String),
    /// Pin a garment at a place in its slot.
    PinAt(String, u32),
    RemoveGarment(String),
    StopExpectingCloth(String),
    ForgetCloth(String),
    FollowSkin(String),
    RemoveWaiver(Waiver),
}

/// The fix of a finding: the sentence that says exactly what it changes, and the change.
#[derive(Debug, Clone, PartialEq)]
pub struct Remedy {
    pub code: &'static str,
    /// What the fix does in a few words, as a title and a button say it: `Remove Overcoat copy`.
    pub act: String,
    pub what: String,
    change: Change,
}

/// The fix of `finding` in `wardrobe`, none where the rule has no mechanical remedy or the pack no longer shows the finding.
///
/// ⚠ Only a remedy that needs no choice is offered: a garment with no slot is not given one.
pub fn remedy(wardrobe: &Wardrobe, finding: &Finding) -> Option<Remedy> {
    let garment = finding
        .garment
        .as_deref()
        .and_then(|id| wardrobe.garment(id));
    let (act, what, change) = match finding.code {
        "position-clash" | "slot-full" => {
            let g = garment?;
            let position = g.position?;
            if finding.code == "slot-full"
                && g.ped.is_none_or(|ped| position < places_of(wardrobe, ped))
            {
                return None;
            }
            (
                format!("Unpin {}", name_of(g)),
                format!(
                    "Unpin {} from position {position:03}; it takes the lowest place no pin holds",
                    name_of(g)
                ),
                Change::Unpin(g.id.clone()),
            )
        }
        "renumbered" => {
            let g = garment?;
            let was = wardrobe
                .lock
                .as_ref()?
                .places
                .iter()
                .find(|l| l.id == g.id)?;
            // ⚠ Only where the garment is still in the slot it was built in and the place is free: pinning it into a clash is a choice.
            if (g.ped, g.slot) != (Some(was.ped), Some(was.slot)) {
                return None;
            }
            let place = wardrobe.lock.as_ref()?.place_of(wardrobe, was);
            let held = allocate(wardrobe)
                .in_slot(was.ped, was.slot)
                .any(|p| p.place == place && wardrobe.garments[p.garment].id != g.id);
            if held || place >= places_of(wardrobe, was.ped) {
                return None;
            }
            (
                format!("Pin {}", name_of(g)),
                format!(
                    "Pin {} at {} {:03} of {}, where the last build put it",
                    name_of(g),
                    was.slot,
                    was.position,
                    wardrobe.collection_of(was.ped, was.part)
                ),
                Change::PinAt(g.id.clone(), place),
            )
        }
        "duplicate-content" => {
            let g = garment?;
            (
                format!("Remove {}", name_of(g)),
                format!(
                    "Remove {} from the pack. Its files are not touched. Garments in the same slot that have no fixed position may move up; the next build lists which ones.",
                    name_of(g)
                ),
                Change::RemoveGarment(g.id.clone()),
            )
        }
        "cloth-missing" => {
            let g = garment?;
            if !(g.missing_cloth && g.cloth.is_none()) {
                return None;
            }
            (
                format!("Stop expecting a cloth for {}", name_of(g)),
                format!(
                    "State that {} has no cloth: the pack stops expecting one, and ownsCloth is not written for it",
                    name_of(g)
                ),
                Change::StopExpectingCloth(g.id.clone()),
            )
        }
        "cloth-ignored" => {
            let g = garment?;
            g.cloth.as_ref()?;
            (
                format!("Stop naming the cloth of {}", name_of(g)),
                format!(
                    "Stop naming the cloth file of {}; the file is not touched",
                    name_of(g)
                ),
                Change::ForgetCloth(g.id.clone()),
            )
        }
        "mask-skin" => {
            let g = garment?;
            let asked = crate::mask::form_asked(g);
            // ⚠ The match form is a statement of its own, and taking it off is the person's to decide.
            if asked == 'm' || asked == g.skin.letter() {
                return None;
            }
            (
                format!("Match the mask of {} to its skin", name_of(g)),
                format!(
                    "Make the propMask of {} ask for the {} form of its files, the form its skin ships: the skin bit is {} and every other bit stays",
                    name_of(g),
                    g.skin.letter(),
                    if g.skin == Skin::Race { "set" } else { "taken off" }
                ),
                Change::FollowSkin(g.id.clone()),
            )
        }
        "waiver-without-reason" | "waiver-unknown" => {
            let waiver = wardrobe.waivers.get(finding.waiver?)?;
            (
                "Remove the waiver".to_string(),
                format!(
                    "Remove the waiver of {}",
                    rule_of(&waiver.code).map_or_else(
                        || format!("\"{}\"", waiver.code),
                        |rule| format!("the rule \"{}\"", rule.summary)
                    )
                ),
                Change::RemoveWaiver(waiver.clone()),
            )
        }
        _ => return None,
    };
    Some(Remedy {
        code: rule_of(finding.code)?.code,
        act,
        what,
        change,
    })
}

impl Remedy {
    pub fn apply(&self, wardrobe: &mut Wardrobe) -> Result<(), Error> {
        match &self.change {
            Change::Unpin(id) => apply(wardrobe, id, Edit::Position(None)),
            Change::PinAt(id, place) => apply(wardrobe, id, Edit::Position(Some(*place))),
            Change::RemoveGarment(id) => apply(wardrobe, id, Edit::Remove),
            Change::StopExpectingCloth(id) => {
                garment_mut(wardrobe, id).map(|g| g.missing_cloth = false)
            }
            Change::ForgetCloth(id) => garment_mut(wardrobe, id).map(|g| g.cloth = None),
            Change::FollowSkin(id) => garment_mut(wardrobe, id).map(|g| {
                let skin = g.skin;
                crate::mask::follow_skin(g, skin)
            }),
            Change::RemoveWaiver(waiver) => {
                let at = (wardrobe.waivers.iter())
                    .position(|w| w == waiver)
                    .ok_or_else(|| {
                        Error::Refused("the waiver is not in the pack any more".into())
                    })?;
                wardrobe.waivers.remove(at);
                Ok(())
            }
        }
    }
}

pub(crate) fn garment_mut<'a>(
    wardrobe: &'a mut Wardrobe,
    id: &str,
) -> Result<&'a mut Garment, Error> {
    (wardrobe.garments.iter_mut())
        .find(|g| g.id == id)
        .ok_or_else(|| Error::NoSuchGarment(id.to_string()))
}

/// The most files a dropped folder gives at once: a drop of many thousands of files is read whole, with progress and a way to stop it, and a drop of a whole drive stays a refusal and not a copy.
pub const MAX_FILES: usize = 1_000_000;
const MAX_DEPTH: usize = 8;

/// What adding the dropped files did, which is all Undo needs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Added {
    /// The garments made, by id, in the order of the slot numbers their names state.
    pub garments: Vec<String>,
    /// The dummies the garments took the places of, with where each stood in the list: Undo puts them back.
    pub dummies: Vec<(usize, Garment)>,
    /// Files this copied into the module's folder, as paths inside it: Undo removes these and nothing else.
    pub copied: Vec<String>,
    /// The folders inside the module's folder that this made; Undo removes these, while empty, and no folder that was there before.
    pub made: Vec<String>,
    pub textures: usize,
    pub cloth: usize,
    /// Garments whose ped or slot the names did not state; they are marked with a question mark until somebody says.
    pub unread: usize,
    /// The models of the drop whose garment the pack already held (the same ped and slot, the same model bytes and, where the drop brought textures, the same texture bytes), as the drop showed them: they were not added again.
    pub already_held: Vec<String>,
    pub left_out: Vec<LeftOut>,
    /// What the shop metadata in the drop said: what resolved to a garment's texture, what points at nothing, what was carried.
    pub shop: ShopReport,
    /// What the pack held of the shop before a drop that read shop metadata, which Undo puts back.
    pub shop_before: Option<ShopBefore>,
}

/// What a drop that read shop metadata replaced, so that Undo can put it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopBefore {
    pub unplaced: Vec<Stated>,
    pub carried: Vec<Carried>,
    /// The entry each texture of a garment already in the pack held, by the garment's id and the texture's number, where the files dropped replace it.
    pub entries: Vec<(String, usize, ShopEntry)>,
}

/// A file of a drop, and the path its name is read from: a folder dropped keeps its own name in front, as the folder a pack sits in states the ped.
pub(crate) struct Source {
    pub(crate) path: PathBuf,
    pub(crate) shown: String,
}

/// A model, with what is dropped for it.
struct Draft {
    model: Source,
    read: Resolved,
    alternates: Vec<(u32, Source)>,
    cloth: Option<Source>,
    textures: Vec<(Option<u8>, Resolved, Source)>,
}

type Key = (Option<Ped>, Slot, u32);

/// A garment made from a draft, with the ped, slot and number its name states, which is where a shop entry finds it.
type Made = (Garment, Option<(Ped, Slot, u32)>);

fn key_of(read: &Resolved) -> Option<Key> {
    Some((read.ped, read.slot?, read.position?))
}

fn left(source: &Source, why: impl Into<String>) -> LeftOut {
    LeftOut {
        path: source.shown.clone(),
        why: why.into(),
    }
}

/// What a drop is watched by: how far it has got, and the flag that stops it.
#[derive(Clone, Default)]
pub struct Watch {
    pub progress: Progress,
    pub cancel: Cancel,
}

impl Watch {
    /// Why the work stops, once somebody asked it to.
    pub(crate) fn check(&self) -> Result<(), Error> {
        match self.cancel.is_set() {
            true => Err(Error::Stopped),
            false => Ok(()),
        }
    }
}

/// How many files go by between one look at the flag that stops a drop and the next.
const LOOK_EVERY: usize = 256;

/// Every file of `dropped`, a folder walked as far as the bounds read.
fn gather(
    dropped: &[PathBuf],
    left_out: &mut Vec<LeftOut>,
    watch: &Watch,
) -> Result<Vec<Source>, Error> {
    struct Walk<'a> {
        out: Vec<Source>,
        truncated: bool,
        watch: &'a Watch,
    }
    impl Walk<'_> {
        fn dir(&mut self, shown: String, dir: &Path, depth: usize) -> Result<(), Error> {
            let mut entries: Vec<_> = std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let name = entry.file_name().to_string_lossy().into_owned();
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                // ⚠ A link may lead out of the folder or back into it, and a hidden folder is not a pack's.
                if name.starts_with('.') || kind.is_symlink() {
                    continue;
                }
                // ⚠ A source folder is one file of the drop, a model or a texture, and its own files are not walked.
                if kind.is_dir() && entry.path().join(or_source::asset::MANIFEST).is_file() {
                    self.out.push(Source {
                        path: entry.path(),
                        shown: format!("{shown}/{name}"),
                    });
                } else if kind.is_dir() {
                    match depth < MAX_DEPTH {
                        true => self.dir(format!("{shown}/{name}"), &entry.path(), depth + 1)?,
                        false => self.truncated = true,
                    }
                } else if self.out.len() >= MAX_FILES {
                    self.truncated = true;
                } else {
                    self.out.push(Source {
                        path: entry.path(),
                        shown: format!("{shown}/{name}"),
                    });
                    if self.out.len().is_multiple_of(LOOK_EVERY) {
                        self.watch.check()?;
                        self.watch
                            .progress
                            .now_on(format!("Reading the folders: {} files", self.out.len()));
                    }
                }
            }
            Ok(())
        }
    }
    let mut walk = Walk {
        out: Vec::new(),
        truncated: false,
        watch,
    };
    for path in dropped {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_dir() && path.join(or_source::asset::MANIFEST).is_file() {
            walk.out.push(Source {
                path: path.clone(),
                shown: name,
            });
        } else if path.is_dir() {
            walk.dir(name, path, 0)?;
        } else if path.is_file() {
            walk.out.push(Source {
                path: path.clone(),
                shown: name,
            });
        }
    }
    if walk.truncated {
        left_out.push(LeftOut {
            path: String::new(),
            why: format!("the drop holds more than {MAX_FILES} files or folders deeper than {MAX_DEPTH}; the rest was not read"),
        });
    }
    Ok(walk.out)
}

/// Keep one picture at most for each variation a name states, and none where a `.ytd` states it: the `.ytd` wins, else the first picture, and each other picture is left out with the file that holds the variation.
///
/// ⚠ A folder that keeps a picture beside each texture under its name states every variation twice, and a `.ytd` of the same variation is not touched: this removes only pictures.
fn one_texture_per_variation(draft: &mut Draft, left_out: &mut Vec<LeftOut>) {
    let mut holder: BTreeMap<u8, (PathBuf, String)> = BTreeMap::new();
    for (letter, _, source) in &draft.textures {
        if let (Some(letter), false) = (letter, images::is_image(&source.path)) {
            (holder.entry(*letter)).or_insert_with(|| (source.path.clone(), source.shown.clone()));
        }
    }
    draft.textures.retain(|(letter, _, source)| {
        let Some(letter) = letter.filter(|_| images::is_image(&source.path)) else {
            return true;
        };
        let (path, shown) =
            (holder.entry(letter)).or_insert_with(|| (source.path.clone(), source.shown.clone()));
        let kept = *path == source.path;
        if !kept {
            left_out.push(left(
                source,
                format!(
                    "variation {} of this garment is already `{}`, and a variation is one texture",
                    char::from(b'a' + letter),
                    shown.rsplit('/').next().unwrap_or(shown)
                ),
            ));
        }
        kept
    });
}

/// Add the garments the files of `dropped` state, copying them into `folder`.
///
/// The names are read as the game's own are (`resolve`): a model makes a garment, the textures and
/// cloth of its place go with it, and what a name does not state is left unset and counted in
/// [`Added::unread`], never guessed. An image named as a texture is made into the `.ytd` of its variation. A file already in `folder` is referenced where it is; a copy
/// never replaces a file there, and takes a free name when the name is taken by other bytes.
pub fn add_clothes(folder: &Path, dropped: &[PathBuf]) -> Result<Added, Error> {
    add_clothes_watched(folder, dropped, &Watch::default())
}

/// [`add_clothes`], telling `watch.progress` how far it has got and stopping once `watch.cancel` is set.
///
/// ⚠ All or none: a drop that is stopped leaves the pack and its folder as they were.
pub fn add_clothes_watched(
    folder: &Path,
    dropped: &[PathBuf],
    watch: &Watch,
) -> Result<Added, Error> {
    let mut wardrobe = Wardrobe::read(folder)?;
    let mut added = Added::default();
    let mut left_out = Vec::new();
    let sources = gather(dropped, &mut left_out, watch)?;
    // The shop's own files are found among everything dropped, whatever their names say.
    let offered: Vec<Offered> = sources
        .iter()
        .map(|s| Offered {
            shown: s.shown.clone(),
            path: s.path.clone(),
        })
        .collect();
    let found = shop::discover(&offered);

    let total = sources.len() as u64;
    let mut drafts: Vec<Draft> = Vec::new();
    let mut places: BTreeSet<Key> = BTreeSet::new();
    let mut textures: Vec<(Resolved, Source)> = Vec::new();
    let mut cloths: Vec<(Resolved, Source)> = Vec::new();
    let mut alternates: Vec<(Resolved, Source)> = Vec::new();
    for (n, source) in sources.into_iter().enumerate() {
        if n.is_multiple_of(LOOK_EVERY) {
            watch.check()?;
            watch.progress.reached(n as u64, total);
            watch.progress.now_on("Reading the names");
        }
        // A source folder is read by the name of the game file it becomes, which is how a game file states its place.
        let (ext, shown) = match (
            source.path.is_dir(),
            crate::facts::source_kind(&source.shown),
        ) {
            (true, Some(kind)) if kind == or_source::models::DRAWABLES => (
                "ydd".to_string(),
                source.shown.replace(".drawables.src", ".ydd"),
            ),
            (true, Some(_)) => (
                "ytd".to_string(),
                source.shown.replace(".textures.src", ".ytd"),
            ),
            (true, None) => {
                left_out.push(left(
                    &source,
                    "a source of another kind: a garment takes a `.drawables.src` for its model and a `.textures.src` for its texture",
                ));
                continue;
            }
            (false, _) => (
                source
                    .path
                    .extension()
                    .map(|e| e.to_string_lossy().to_ascii_lowercase())
                    .unwrap_or_default(),
                source.shown.clone(),
            ),
        };
        let image = images::is_image(&source.path);
        match ext.as_str() {
            "ydd" | "ytd" | "yld" => {}
            "ymt" | "yft" => {
                left_out.push(left(&source, format!("a .{ext} file: a build writes the metadata, and a freemode component has no fragment")));
                continue;
            }
            _ if image => {}
            _ => continue,
        }
        let name = source
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        if !name.as_deref().is_some_and(or_project::paths::is_component) {
            left_out.push(left(
                &source,
                "its name cannot be a file name on every machine",
            ));
            continue;
        }
        if !image && !source.path.is_dir() {
            if let Some(why) = crate::import::protected(&source.path) {
                left_out.push(left(&source, why));
                continue;
            }
        }
        let read = resolve(&shown);
        match read.role {
            Some(Role::Model) if !read.extra.is_empty() => left_out.push(left(
                &source,
                format!(
                    "a model with `{}` after its name, which nothing here builds",
                    read.extra.join("_")
                ),
            )),
            Some(Role::Model) if read.alternate.is_some() => alternates.push((read, source)),
            Some(Role::Model) => {
                if let Some(key) = key_of(&read) {
                    if !places.insert(key) {
                        left_out.push(left(&source, "a second model for one place"));
                        continue;
                    }
                }
                drafts.push(Draft {
                    model: source,
                    read,
                    alternates: Vec::new(),
                    cloth: None,
                    textures: Vec::new(),
                });
            }
            Some(Role::Cloth) => cloths.push((read, source)),
            // ⚠ An image is any picture, so it is a texture only where its name says whose: a `.ytd` that states no place goes with the only model, a picture of the pack does not.
            Some(Role::Texture) if image && key_of(&read).is_none() => {
                left_out.push(left(&source, crate::import::STRAY_PICTURE))
            }
            Some(Role::Texture) => textures.push((read, source)),
            None => left_out.push(left(
                &source,
                "its name does not state what kind of file it is",
            )),
        }
    }

    // A file goes with the model that has its place; one that states no place goes with the only model there is.
    let sole = (drafts.len() == 1).then_some(0);
    let index: BTreeMap<Key, usize> = (drafts.iter().enumerate())
        .filter_map(|(i, d)| Some((key_of(&d.read)?, i)))
        .collect();
    let owner = |read: &Resolved| -> Option<usize> {
        match key_of(read) {
            Some(key) => index.get(&key).copied(),
            None => sole,
        }
    };
    for (read, source) in alternates {
        match owner(&read) {
            Some(i) => drafts[i]
                .alternates
                .push((read.alternate.unwrap_or_default(), source)),
            None => left_out.push(left(&source, "an alternate with no model dropped with it")),
        }
    }
    for (read, source) in cloths {
        match owner(&read) {
            None => left_out.push(left(&source, "a cloth with no model dropped with it")),
            Some(i) if drafts[i].read.slot.is_some_and(Slot::is_prop) => {
                left_out.push(left(&source, "a prop owns no cloth"))
            }
            Some(i) if drafts[i].cloth.is_some() => {
                left_out.push(left(&source, "a second cloth for one model"))
            }
            Some(i) => drafts[i].cloth = Some(source),
        }
    }
    for (read, source) in textures {
        match owner(&read) {
            Some(i) => drafts[i].textures.push((read.variant, read, source)),
            None => left_out.push(left(&source, "a texture with no model dropped with it")),
        }
    }
    for draft in &mut drafts {
        one_texture_per_variation(draft, &mut left_out);
    }
    // A garment the pack already holds is not added again, and neither are the files that came with it: the same type, the same model bytes and, where the drop brings textures, the same texture bytes.
    let mut held = HeldModels::of(folder, &wardrobe);
    let mut scratch = Scratch::default();
    let mut fresh = Vec::new();
    for (n, draft) in drafts.into_iter().enumerate() {
        if n.is_multiple_of(LOOK_EVERY) {
            watch.check()?;
            watch.progress.now_on("Looking for what the pack holds");
        }
        let same = held.same_model(draft.read.ped, draft.read.slot, &draft.model.path);
        let again = match same.is_empty() {
            true => false,
            false if draft.textures.is_empty() => true,
            false => {
                let wanted: Option<Vec<String>> = (draft.textures.iter())
                    .map(|(_, _, s)| content_digest(&s.path))
                    .collect();
                wanted.is_some_and(|mut wanted| {
                    wanted.sort_unstable();
                    held.has_textures(&same, &wanted)
                })
            }
        };
        match again {
            true => added.already_held.push(draft.model.shown.clone()),
            false => fresh.push(draft),
        }
    }
    let mut drafts = fresh;
    // The order of the slot numbers the names state, so a pack dropped whole keeps its relative order.
    drafts.sort_by_key(|d| {
        (
            d.read.ped,
            d.read.slot,
            d.read.position,
            d.model.shown.clone(),
        )
    });

    let mut copying = Copying::new(folder);
    let mut made: Vec<Made> = Vec::new();
    if let Err(e) = assemble(
        &mut copying,
        drafts,
        &mut added,
        &mut made,
        &mut scratch,
        &mut left_out,
        watch,
    ) {
        copying.undo();
        return Err(e);
    }
    let reads_shop = !found.shops.is_empty() || !found.carried.is_empty();
    if made.is_empty() && !reads_shop {
        added.left_out = left_out;
        return Ok(added);
    }
    added.garments = made.iter().map(|(g, _)| g.id.clone()).collect();
    let (made, keys): (Vec<Garment>, Vec<_>) = made.into_iter().unzip();
    wardrobe.garments.extend(made);
    if wardrobe.rules.dummies.replace_on_add {
        added.dummies = crate::dummies::replace_on_add(&mut wardrobe, &added.garments);
    }
    // The garments made are the tail of the list: the dummies they took the places of stood before them.
    let first_new = wardrobe.garments.len() - keys.len();
    if reads_shop {
        let shown = (found, &offered[..]);
        if let Err(e) = read_shop(
            &mut wardrobe,
            &mut copying,
            &mut added,
            shown,
            first_new,
            &keys,
        ) {
            copying.undo();
            return Err(e);
        }
    }
    if let Err(e) = watch.check().and_then(|()| Ok(wardrobe.write(folder)?)) {
        copying.undo();
        return Err(e);
    }
    added.copied = copying.copied;
    added.made = copying.made;
    added.left_out = left_out;
    Ok(added)
}

/// The garments the pack holds, found by what their files hold and not by what they are called.
struct HeldModels {
    garments: Vec<Held>,
    /// The garments whose model is a file of this size, the cheap thing two different files seldom share.
    by_size: std::collections::HashMap<u64, Vec<usize>>,
    /// The garments whose model is a source folder, which is found by the file it was made from.
    sources: Vec<usize>,
    digests: std::collections::HashMap<PathBuf, Option<String>>,
}

/// A garment of the pack whose model the pack has on disk: its type and the files it ships.
struct Held {
    ped: Option<Ped>,
    slot: Option<Slot>,
    model: PathBuf,
    textures: Vec<PathBuf>,
}

impl HeldModels {
    fn of(folder: &Path, wardrobe: &Wardrobe) -> Self {
        let place = |rel: &str| folder.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let mut garments = Vec::new();
        let mut by_size: std::collections::HashMap<u64, Vec<usize>> = Default::default();
        let mut sources = Vec::new();
        for g in &wardrobe.garments {
            let Some(model) = g.model.as_deref().map(place) else {
                continue;
            };
            let Ok(meta) = std::fs::metadata(&model) else {
                continue;
            };
            match meta.is_dir() {
                true => sources.push(garments.len()),
                false => by_size.entry(meta.len()).or_default().push(garments.len()),
            }
            garments.push(Held {
                ped: g.ped,
                slot: g.slot,
                model,
                textures: (g.variations.iter())
                    .filter_map(|v| v.texture.as_deref())
                    .map(place)
                    .collect(),
            });
        }
        Self {
            garments,
            by_size,
            sources,
            digests: Default::default(),
        }
    }

    fn digest(&mut self, path: &Path) -> Option<String> {
        self.digests
            .entry(path.to_path_buf())
            .or_insert_with(|| content_digest(path))
            .clone()
    }

    /// The garments of the pack of this type whose model has the bytes of the file at `path`.
    fn same_model(&mut self, ped: Option<Ped>, slot: Option<Slot>, path: &Path) -> Vec<usize> {
        let Ok(meta) = std::fs::metadata(path) else {
            return Vec::new();
        };
        // A dropped source folder has no size to compare, so every garment of its type is: its digest is that of the file it came from.
        let sized: Vec<usize> = match meta.is_dir() {
            true => (0..self.garments.len()).collect(),
            false => (self.by_size.get(&meta.len()).into_iter().flatten().copied())
                .chain(self.sources.iter().copied())
                .collect(),
        };
        let same_size: Vec<usize> = sized
            .into_iter()
            .filter(|&i| (self.garments[i].ped, self.garments[i].slot) == (ped, slot))
            .collect();
        if same_size.is_empty() {
            return same_size;
        }
        let Some(wanted) = content_digest(path) else {
            return Vec::new();
        };
        same_size
            .into_iter()
            .filter(|&i| {
                let model = self.garments[i].model.clone();
                self.digest(&model).as_deref() == Some(wanted.as_str())
            })
            .collect()
    }

    /// Whether one of the garments `candidates` ships exactly the textures whose digests, sorted, are `wanted`.
    fn has_textures(&mut self, candidates: &[usize], wanted: &[String]) -> bool {
        candidates.iter().any(|&i| {
            let paths = self.garments[i].textures.clone();
            let held: Option<Vec<String>> = paths.iter().map(|p| self.digest(p)).collect();
            held.is_some_and(|mut held| {
                held.sort_unstable();
                held == wanted
            })
        })
    }
}

/// Resolve the shop metadata of a drop against the pack, and copy in the files a manifest registered with the shop.
fn read_shop(
    wardrobe: &mut Wardrobe,
    copying: &mut Copying,
    added: &mut Added,
    (found, offered): (shop::Found, &[Offered]),
    first_new: usize,
    keys: &[Option<(Ped, Slot, u32)>],
) -> Result<(), Error> {
    let from: Vec<&str> = found.shops.iter().map(|f| f.shown.as_str()).collect();
    let mut before = ShopBefore {
        unplaced: wardrobe.shop.unplaced.clone(),
        carried: wardrobe.shop.carried.clone(),
        entries: Vec::new(),
    };
    for g in &wardrobe.garments[..first_new] {
        for (t, v) in g.variations.iter().enumerate() {
            if let Some(entry) = v.shop.as_ref().filter(|e| from.contains(&e.from.as_str())) {
                before.entries.push((g.id.clone(), t, entry.clone()));
            }
        }
    }
    let allocation = allocate(wardrobe);
    let at = |_: &Wardrobe, i: usize| {
        if let Some(Some(key)) = i.checked_sub(first_new).and_then(|n| keys.get(n)) {
            return Some(*key);
        }
        let p = allocation.of(i)?;
        Some((p.ped, p.slot, p.place))
    };
    added.shop = shop::apply(wardrobe, &found.shops, &at);
    added.shop.unread.extend(found.unread);
    for (shown, line) in found.carried {
        let Some(o) = offered.iter().find(|o| o.shown == shown) else {
            continue;
        };
        let name = o.path.file_name().map(|n| n.to_string_lossy().into_owned());
        if !name.as_deref().is_some_and(or_project::paths::is_component) {
            added.shop.unread.push(LeftOut {
                path: shown,
                why: "its name cannot be a file name on every machine".into(),
            });
            continue;
        }
        let source = Source {
            path: o.path.clone(),
            shown: shown.clone(),
        };
        let carried = Carried {
            file: copying.put("shop", &source)?,
            directive: line.directive,
            kind: line.kind,
        };
        added.shop.carried.push(LeftOut {
            path: shown,
            why: format!(
                "registered as {} by `{}`, carried unchanged as `{}`",
                carried.kind, carried.directive, carried.file
            ),
        });
        if !wardrobe.shop.carried.contains(&carried) {
            wardrobe.shop.carried.push(carried);
        }
    }
    added.shop_before = Some(before);
    Ok(())
}

/// Make a garment of each draft, copying its files in; a failure leaves the garments made so far in `made` and the copies in `copying`.
fn assemble(
    copying: &mut Copying,
    drafts: Vec<Draft>,
    added: &mut Added,
    made: &mut Vec<Made>,
    scratch: &mut Scratch,
    left_out: &mut Vec<LeftOut>,
    watch: &Watch,
) -> Result<(), Error> {
    let total = drafts.len() as u64;
    for (n, mut draft) in drafts.into_iter().enumerate() {
        watch.check()?;
        watch.progress.reached(n as u64, total);
        watch.progress.now_on(draft.model.shown.clone());
        let mut g = Garment::new();
        g.ped = draft.read.ped;
        g.slot = draft.read.slot;
        g.skin = draft.read.skin.unwrap_or(Skin::Universal);
        g.label = Some(stem_of(&draft.model.shown));
        // ⚠ Every game file becomes a source at the door: a file that cannot is left out with the reason, and a model that cannot leaves its garment out.
        match intake::model(copying, scratch, &draft.model.path) {
            Ok(rel) => g.model = Some(rel),
            Err(Error::Refused(why)) => {
                left_out.push(left(&draft.model, why));
                left_out.extend(draft.textures.iter().map(|(_, _, s)| {
                    left(
                        s,
                        "its model could not be turned into a source, so its garment was not added",
                    )
                }));
                continue;
            }
            Err(e) => return Err(e),
        }
        draft.alternates.sort_by_key(|(n, s)| (*n, s.shown.clone()));
        for (_, alternate) in &draft.alternates {
            match intake::model(copying, scratch, &alternate.path) {
                Ok(rel) => g.alternates.push(rel),
                Err(Error::Refused(why)) => left_out.push(left(alternate, why)),
                Err(e) => return Err(e),
            }
        }
        if let Some(cloth) = &draft.cloth {
            g.cloth = Some(copying.put("cloth", cloth)?);
            added.cloth += 1;
        }
        draft
            .textures
            .sort_by_key(|(letter, _, s)| (letter.is_none(), *letter, s.shown.clone()));
        for (_, read, texture) in &draft.textures {
            let rel = match intake::texture(copying, scratch, &texture.path) {
                Ok(rel) => rel,
                Err(Error::Refused(why)) => {
                    left_out.push(left(texture, why));
                    continue;
                }
                Err(e) => return Err(e),
            };
            g.variations.push(Variation {
                texture: Some(rel),
                tint: read.tint.clone().filter(|t| *t != g.skin.tint()),
                ..Variation::default()
            });
            added.textures += 1;
        }
        if g.ped.is_none() || g.slot.is_none() {
            added.unread += 1;
        }
        let r = &draft.read;
        let stated = r
            .ped
            .zip(r.slot)
            .zip(r.position)
            .map(|((p, s), n)| (p, s, n));
        made.push((g, stated));
    }
    watch.progress.reached(total, total);
    Ok(())
}

/// Take back what [`add_clothes`] did: its garments leave the pack and the files it copied go; every other file stays. The [`Redo`] makes the drop again.
///
/// ⚠ A copied file that a garment still in the pack names stays too, so taking back an older drop never breaks a newer garment.
pub fn undo(folder: &Path, added: &Added) -> Result<Redo, Error> {
    undone(folder, &added.copied, &added.made, |wardrobe| {
        wardrobe
            .garments
            .retain(|g| !added.garments.contains(&g.id));
        for (at, held) in added.dummies.iter().rev() {
            wardrobe
                .garments
                .insert((*at).min(wardrobe.garments.len()), held.clone());
        }
        if let Some(before) = &added.shop_before {
            let from: Vec<&str> = added.shop.files.iter().map(String::as_str).collect();
            for v in wardrobe.garments.iter_mut().flat_map(|g| &mut g.variations) {
                if v.shop
                    .as_ref()
                    .is_some_and(|e| from.contains(&e.from.as_str()))
                {
                    v.shop = None;
                }
            }
            for (id, t, entry) in &before.entries {
                let held = (wardrobe.garments.iter_mut().find(|g| &g.id == id))
                    .and_then(|g| g.variations.get_mut(*t));
                if let Some(v) = held {
                    v.shop = Some(entry.clone());
                }
            }
            wardrobe.shop.unplaced.clone_from(&before.unplaced);
            wardrobe.shop.carried.clone_from(&before.carried);
        }
        Ok(())
    })
}

/// What a garment states of its cloth: the file linked, and whether an import found it missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClothState {
    pub cloth: Option<String>,
    pub missing: bool,
}

/// What linking or unlinking a cloth did, which is all Undo needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    /// The garment, by id.
    pub garment: String,
    /// What it stated of its cloth before.
    pub previous: ClothState,
    /// Files this copied into the module's folder: Undo removes these and nothing else.
    pub copied: Vec<String>,
    /// The folders inside the module's folder that this made.
    pub made: Vec<String>,
}

/// Link the cloth file `source` to garment `id`, copying it into `folder` unless it is already inside.
///
/// A cloth belongs to one drawable, because the game finds it by the model's name, so it is the
/// garment's child: the build ships it under the model's name and writes `ownsCloth` from it. A
/// prop owns no cloth, and a place held has no model to name it after, so neither takes one.
///
/// ⚠ A cloth has no source form, so it is the one file of a drop that is kept compiled; it can instead be referenced
/// ([`link_cloth_to`]), in the project, in another project or by its id alone.
pub fn link_cloth(folder: &Path, id: &str, source: &Path) -> Result<Linked, Error> {
    let name = source.file_name().map(|n| n.to_string_lossy().into_owned());
    linked(folder, id, |copying| {
        if !name.as_deref().is_some_and(or_project::paths::is_component) {
            return Err(Error::Refused(
                "its name cannot be a file name on every machine".into(),
            ));
        }
        if !source
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("yld"))
        {
            return Err(Error::Refused("a cloth is a .yld file".into()));
        }
        if !source.is_file() {
            return Err(Error::Refused(format!(
                "{} is not a file",
                source.display()
            )));
        }
        let source = Source {
            path: source.to_path_buf(),
            shown: name.unwrap_or_default(),
        };
        copying.put("cloth", &source)
    })
}

/// Link to garment `id` the cloth that `reference` leads to, which stays where it is: a file of the project, of another project, or an id alone.
pub fn link_cloth_to(
    folder: &Path,
    id: &str,
    reference: &or_project::reference::Reference,
) -> Result<Linked, Error> {
    linked(folder, id, |copying| {
        copying.put_reference("cloth", reference)
    })
}

/// Link the cloth that `put` places in the pack to garment `id`, and write the pack.
fn linked(
    folder: &Path,
    id: &str,
    put: impl FnOnce(&mut Copying) -> Result<String, Error>,
) -> Result<Linked, Error> {
    let mut wardrobe = Wardrobe::read(folder)?;
    let garment = garment_mut(&mut wardrobe, id)?;
    if garment.slot.is_some_and(Slot::is_prop) {
        return Err(Error::Refused("a prop owns no cloth".into()));
    }
    if garment.model.is_none() {
        return Err(Error::Refused(
            "a place held has no model for the cloth to be named after".into(),
        ));
    }
    let mut copying = Copying::new(folder);
    let rel = match put(&mut copying) {
        Ok(rel) => rel,
        Err(e) => {
            copying.undo();
            return Err(e);
        }
    };
    let garment = garment_mut(&mut wardrobe, id)?;
    let previous = ClothState {
        cloth: garment.cloth.replace(rel),
        missing: std::mem::take(&mut garment.missing_cloth),
    };
    if let Err(e) = wardrobe.write(folder) {
        copying.undo();
        return Err(e.into());
    }
    Ok(Linked {
        garment: id.to_string(),
        previous,
        copied: copying.copied,
        made: copying.made,
    })
}

/// Unlink the cloth of garment `id`: the build writes no `ownsCloth` for it and ships no cloth. The file is not touched.
pub fn unlink_cloth(folder: &Path, id: &str) -> Result<Linked, Error> {
    update(folder, |wardrobe| {
        let garment = garment_mut(wardrobe, id)?;
        if garment.cloth.is_none() && !garment.missing_cloth {
            return Err(Error::Refused("that garment has no cloth".into()));
        }
        Ok(Linked {
            garment: id.to_string(),
            previous: ClothState {
                cloth: garment.cloth.take(),
                missing: std::mem::take(&mut garment.missing_cloth),
            },
            copied: Vec::new(),
            made: Vec::new(),
        })
    })
}

/// Put a garment [`take_out`] put out back where it was; the [`Redo`] takes it out again.
pub fn undo_removal(folder: &Path, removed: &Removed) -> Result<Redo, Error> {
    undone(folder, &[], &[], |wardrobe| put_back(wardrobe, removed))
}

/// Take back [`link_cloth`] or [`unlink_cloth`]: the garment states its cloth as it did, and the file a link copied goes unless a garment names it; the [`Redo`] makes it again.
pub fn undo_cloth(folder: &Path, linked: &Linked) -> Result<Redo, Error> {
    undone(folder, &linked.copied, &linked.made, |wardrobe| {
        let garment = garment_mut(wardrobe, &linked.garment)?;
        garment.cloth.clone_from(&linked.previous.cloth);
        garment.missing_cloth = linked.previous.missing;
        Ok(())
    })
}

/// The longest id a person may give a shop entry.
pub const MAX_SHOP_ID: usize = 128;

/// Give texture `texture` of garment `id` the shop id `shop_id` in place of the one an import read or the build names, or take that choice back with none. It returns what the choice was before.
pub fn set_shop_id(
    wardrobe: &mut Wardrobe,
    id: &str,
    texture: usize,
    shop_id: Option<String>,
) -> Result<Option<String>, Error> {
    let shop_id = shop_id
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty());
    if let Some(text) = &shop_id {
        if text.chars().count() > MAX_SHOP_ID || text.chars().any(char::is_control) {
            return Err(Error::Refused(format!(
                "a shop id is one line of at most {MAX_SHOP_ID} characters"
            )));
        }
    }
    let garment = garment_mut(wardrobe, id)?;
    let held = garment.variations.len();
    let variation = garment.variations.get_mut(texture).ok_or_else(|| {
        Error::Refused(format!(
            "the garment has {held} texture(s), not {}",
            texture + 1
        ))
    })?;
    Ok(std::mem::replace(&mut variation.shop_id, shop_id))
}

/// The files of the module a garment names.
pub(crate) fn files_of(garment: &Garment) -> impl Iterator<Item = String> + '_ {
    (garment.model.iter())
        .chain(&garment.alternates)
        .chain(&garment.cloth)
        .chain(garment.variations.iter().filter_map(|v| v.texture.as_ref()))
        .cloned()
}

/// The name of a file without its folders and its extension.
fn stem_of(shown: &str) -> String {
    let name = shown.rsplit('/').next().unwrap_or(shown);
    name.rsplit_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_string()
}

/// The copying of an add: what it put in the module's folder, so that a failure or an Undo removes that and nothing else.
pub(crate) struct Copying<'a> {
    pub(crate) folder: &'a Path,
    pub(crate) copied: Vec<String>,
    /// The folders it created: a folder that was there already is not its to remove.
    pub(crate) made: Vec<String>,
}

impl<'a> Copying<'a> {
    pub(crate) fn new(folder: &'a Path) -> Self {
        Self {
            folder,
            copied: Vec::new(),
            made: Vec::new(),
        }
    }

    /// The path inside the folder `source` is at afterwards: where it already is, or its copy in `dir`.
    pub(crate) fn put(&mut self, dir: &str, source: &Source) -> Result<String, Error> {
        if let (Ok(root), Ok(file)) = (self.folder.canonicalize(), source.path.canonicalize()) {
            if let Ok(inside) = file.strip_prefix(&root) {
                return Ok(inside.to_string_lossy().replace('\\', "/"));
            }
        }
        let shown = source.path.display().to_string();
        let name = source
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| Error::Refused(format!("{shown} has no file name")))?;
        let wanted = digest_of(&source.path).map_err(|e| Error::Io(shown.clone(), e))?;
        let (stem, ext) = name.rsplit_once('.').unwrap_or((&name, ""));
        for n in 1.. {
            let candidate = match n {
                1 => name.clone(),
                _ if ext.is_empty() => format!("{stem}-{n}"),
                _ => format!("{stem}-{n}.{ext}"),
            };
            let rel = format!("{dir}/{candidate}");
            let to = self.folder.join(dir).join(&candidate);
            if to.exists() {
                // The same bytes are the same file: it is used as it is, and Undo leaves it.
                if digest_of(&to).is_ok_and(|held| held == wanted) {
                    return Ok(rel);
                }
                continue;
            }
            if !self.folder.join(dir).exists() && !self.made.iter().any(|m| m == dir) {
                self.made.push(dir.to_string());
            }
            std::fs::create_dir_all(self.folder.join(dir))
                .map_err(|e| Error::Io(self.folder.join(dir).display().to_string(), e))?;
            let part = self.folder.join(dir).join(format!(".{candidate}.part"));
            let done = std::fs::copy(&source.path, &part).and_then(|_| std::fs::rename(&part, &to));
            if let Err(e) = done {
                let _ = std::fs::remove_file(&part);
                return Err(Error::Io(shown, e));
            }
            self.copied.push(rel.clone());
            return Ok(rel);
        }
        unreachable!("a free name is found")
    }

    /// Remove the copies, and the folders this made that they leave empty.
    pub(crate) fn undo(&mut self) {
        for rel in self.copied.drain(..) {
            let _ = std::fs::remove_file(self.folder.join(&rel));
        }
        // The last made goes first, so that a folder made inside another is out of the way of it.
        for dir in self.made.drain(..).rev() {
            // Fails, as it should, while a file is left in it.
            let _ = std::fs::remove_dir(self.folder.join(dir));
        }
    }
}

pub(crate) fn digest_of(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
