//! The dressed ped: which drawable and which texture each slot of a freemode ped wears, with no renderer.
//!
//! The engine decides what is worn and a scene only draws it. A garment wears the model and the
//! texture its pack states for its slot; every component slot nothing selects wears the ped's own
//! drawable 0, named from the ped's variation metadata, so the answer is a list of file names that
//! a test can hold to the pack without opening a window. A prop is worn at its anchor
//! ([`Prop::bone`]) and the ped wears none of its own.

use std::collections::BTreeMap;

use or_io::meta::CPedVariationInfo;

use crate::alternate::{AlternateSet, Asset};
use crate::flags::{self, Flag};
use crate::model::{Garment, Wardrobe};
use crate::names::{Component, Ped, Prop, Skin, Slot};
use crate::{hair, heels};

/// A heel's stated value in metres, as the other clothing tools draw it: a tenth.
///
/// ⚠ A convention of the preview and no measurement: nobody here can run the game to see what a
/// heel's value does (`heels`).
pub const METRES_PER_HEEL_UNIT: f32 = 0.1;

/// A skin tone of the ped's own head, by the word the base head textures end with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    White,
    Black,
    Latino,
    Chinese,
    Pakistani,
    Arab,
}

impl Tone {
    pub const ALL: [Tone; 6] = [
        Tone::White,
        Tone::Black,
        Tone::Latino,
        Tone::Chinese,
        Tone::Pakistani,
        Tone::Arab,
    ];

    /// The word the base head textures of the tone end with.
    pub fn word(self) -> &'static str {
        match self {
            Tone::White => "whi",
            Tone::Black => "bla",
            Tone::Latino => "lat",
            Tone::Chinese => "chi",
            Tone::Pakistani => "pak",
            Tone::Arab => "ara",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tone::White => "White",
            Tone::Black => "Black",
            Tone::Latino => "Latino",
            Tone::Chinese => "Chinese",
            Tone::Pakistani => "Pakistani",
            Tone::Arab => "Arab",
        }
    }

    /// The first base head drawable that wears the tone: the base heads come two to a tone, in the order of `ALL`.
    pub fn head(self) -> u32 {
        let at = Self::ALL
            .iter()
            .position(|t| *t == self)
            .unwrap_or_default();
        2 * u32::try_from(at).unwrap_or_default()
    }

    /// The tone a head's texture states by its `texId`: 1 white, 2 black, 3 Chinese, 4 Latino, 5 Arab and 10 Pakistani.
    ///
    /// ⚠ Measured on the 46 heads of each freemode ped of a Legacy install, whose textures end with the word of their own `texId`.
    pub fn of_tex_id(tex_id: u8) -> Option<Tone> {
        match tex_id {
            1 => Some(Tone::White),
            2 => Some(Tone::Black),
            3 => Some(Tone::Chinese),
            4 => Some(Tone::Latino),
            5 => Some(Tone::Arab),
            10 => Some(Tone::Pakistani),
            _ => None,
        }
    }
}

/// One garment put on, with the texture variation it is worn with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wear {
    pub garment: String,
    pub variation: usize,
}

/// What a person asked of one of the ped's own slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseEdit {
    /// The ped wears nothing in the slot.
    Off,
    /// The ped wears this one of its own drawables in the slot, if it has it.
    Drawable(u32),
}

/// What a person chose to see.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub ped: Ped,
    /// In the order they were put on: a later garment takes the slot of an earlier one.
    pub wear: Vec<Wear>,
    pub tone: Tone,
    /// Raise the ped by the heels a worn garment states.
    pub heels: bool,
    /// The ped as the game has it, without the pack: the A of A/B.
    pub original: bool,
    /// What was asked of the ped's own slots; a slot a garment is worn in is not changed by it.
    pub base: BTreeMap<Component, BaseEdit>,
    /// The alternate model a garment is worn with, by its id and the number after its model's name; a number it has no model for wears the model.
    pub alternates: BTreeMap<String, usize>,
    /// Which sets of alternate variations are in force.
    pub sets: Sets,
    /// The view from the head of the ped: what the game hides there is left out, and a garment wears its first person alternate.
    pub first_person: bool,
}

/// Which sets of alternate variations the preview applies.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Sets {
    /// The sets a garment worn causes, as the game applies them.
    #[default]
    Worn,
    /// No set.
    Off,
    /// This set, by its id, as if its source were worn.
    Only(String),
}

impl Choice {
    pub fn new(ped: Ped) -> Self {
        Self {
            ped,
            wear: Vec::new(),
            tone: Tone::White,
            heels: true,
            original: false,
            base: BTreeMap::new(),
            alternates: BTreeMap::new(),
            sets: Sets::default(),
            first_person: false,
        }
    }
}

/// A drawable whose model a set of alternate variations changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Driven {
    /// The drawable that changes: a garment of the pack, or one of the ped's own.
    pub target: Asset,
    /// The number after its model's name.
    pub alt: u8,
    /// The id of the set, and its name.
    pub set: String,
    pub name: String,
    /// The person chose the model of a garment, or the first person view wears its own alternate, which is worn in place of the set's.
    pub overridden: bool,
}

/// The slots a ped wears when nothing is chosen, from the ped's own variation metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BaseOutfit {
    /// Each slot with every drawable of the ped in it, each with the `texId` its first texture states, or none where it states no texture.
    slots: Vec<(Component, Vec<Option<u8>>)>,
}

impl BaseOutfit {
    /// Every component slot the metadata states, with its drawables and the `texId` each one's first texture states.
    pub fn of(info: &CPedVariationInfo) -> Self {
        let slots = Component::ALL
            .into_iter()
            .filter_map(|component| {
                let data = info.avail_comp.get(usize::from(component.id()))?;
                let drawables = &info
                    .a_component_data3
                    .get(usize::from(*data))?
                    .a_drawbl_data3;
                let tex_ids: Vec<Option<u8>> = drawables
                    .iter()
                    .map(|drawable| drawable.a_tex_data.first().map(|t| t.tex_id))
                    .collect();
                (!tex_ids.is_empty()).then_some((component, tex_ids))
            })
            .collect();
        Self { slots }
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The slots the ped wears something in, in the order of [`Component::ALL`].
    pub fn slots(&self) -> impl Iterator<Item = Component> + '_ {
        self.slots.iter().map(|(component, _)| *component)
    }

    /// How many drawables of its own the ped has in `component`.
    pub fn drawables(&self, component: Component) -> usize {
        (self.slots.iter())
            .find(|(slot, _)| *slot == component)
            .map_or(0, |(_, skins)| skins.len())
    }
}

/// Who wears a slot.
#[derive(Debug, Clone, PartialEq)]
pub enum By {
    /// A garment of the pack, by its files as the pack states them.
    Garment {
        id: String,
        model: String,
        texture: Option<String>,
    },
    /// The ped's own drawable, by its file names inside the ped's folder.
    Base {
        model: String,
        texture: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Worn {
    pub slot: Component,
    pub by: By,
}

/// A prop put on, which sits at the bone of its anchor.
#[derive(Debug, Clone, PartialEq)]
pub struct WornProp {
    pub prop: Prop,
    /// Always a garment of the pack: the ped has no prop of its own to wear.
    pub by: By,
}

/// A garment of the choice that is not worn, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Left {
    pub garment: String,
    pub why: String,
}

/// A slot of the ped's own that a worn garment takes off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hidden {
    pub slot: Component,
    /// The garment that hides it.
    pub by: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dressing {
    pub worn: Vec<Worn>,
    /// The props put on, in the order of their anchors.
    pub props: Vec<WornProp>,
    pub left: Vec<Left>,
    /// The slots a hat that states the hide hair scale takes off, whatever would be worn in them.
    pub hidden: Vec<Hidden>,
    /// How high the ped is raised, in metres.
    pub lift: f32,
    /// The drawables a set of alternate variations changes, and by which set.
    pub driven: Vec<Driven>,
    /// What a set asks that the pack cannot show, in sentences.
    pub unmet: Vec<String>,
}

impl Dressing {
    pub fn slot(&self, slot: Component) -> Option<&Worn> {
        self.worn.iter().find(|w| w.slot == slot)
    }

    /// The prop worn at `prop`'s anchor.
    pub fn prop(&self, prop: Prop) -> Option<&WornProp> {
        self.props.iter().find(|w| w.prop == prop)
    }
}

/// What `choice` wears: the garments it names, and the ped's own drawable 0 in every other slot of `base`.
pub fn dress(wardrobe: &Wardrobe, base: &BaseOutfit, choice: &Choice) -> Dressing {
    let mut left = Vec::new();
    let mut put_on: BTreeMap<Component, &Garment> = BTreeMap::new();
    let mut variation: BTreeMap<Component, usize> = BTreeMap::new();
    let mut props_on: BTreeMap<Prop, (&Garment, usize)> = BTreeMap::new();
    if !choice.original {
        for wear in &choice.wear {
            let Some(garment) = wardrobe.garment(&wear.garment) else {
                left.push(Left::of(&wear.garment, "is not in the pack"));
                continue;
            };
            const TAKEN: &str = "is taken off: a garment put on after it takes its slot";
            match worn_slot(garment, choice.ped) {
                Ok(Slot::Component(slot)) => {
                    if let Some(earlier) = put_on.insert(slot, garment) {
                        left.push(Left::of(&earlier.id, TAKEN));
                    }
                    variation.insert(slot, wear.variation);
                }
                Ok(Slot::Prop(prop)) => {
                    if let Some((earlier, _)) = props_on.insert(prop, (garment, wear.variation)) {
                        left.push(Left::of(&earlier.id, TAKEN));
                    }
                }
                Err(why) => left.push(Left::of(&garment.id, why)),
            }
        }
    }
    // ⚠ The hat that is worn, not one a later hat took the place of.
    let hides_hair = (props_on.get(&Prop::Head))
        .filter(|(hat, _)| hair::hides_hair(hat))
        .map(|(hat, _)| hat.id.as_str());
    let mut hidden = Vec::new();
    if let Some(by) = hides_hair {
        hidden.push(Hidden {
            slot: Component::Hair,
            by: by.to_string(),
        });
        if let Some(garment) = put_on.remove(&Component::Hair) {
            left.push(Left::of(
                &garment.id,
                "is hidden by a hat that states the hide hair scale",
            ));
        }
    }
    let mut worn = Vec::new();
    // The number of the ped's own drawable worn in each slot a garment did not take.
    let mut own: BTreeMap<Component, u32> = BTreeMap::new();
    for (component, tex_ids) in &base.slots {
        if hidden.iter().any(|h| h.slot == *component) {
            continue;
        }
        let by = match (put_on.get(component), choice.base.get(component)) {
            (Some(garment), _) => garment_by(
                garment,
                variation.get(component).copied().unwrap_or(0),
                choice,
            ),
            (None, Some(BaseEdit::Off)) => continue,
            (None, edit) => {
                // A drawable the ped does not have is no edit.
                let drawable = match edit {
                    Some(BaseEdit::Drawable(n)) if (*n as usize) < tex_ids.len() => Some(*n),
                    _ => None,
                };
                let tex_id = tex_ids
                    .get(drawable.unwrap_or(0) as usize)
                    .copied()
                    .flatten();
                let (by, number) = base_by(*component, drawable, tex_id, choice.tone);
                own.insert(*component, number);
                by
            }
        };
        worn.push(Worn {
            slot: *component,
            by,
        });
    }
    // A slot the base ped does not state is still worn by a garment put on it.
    for (component, garment) in &put_on {
        if !worn.iter().any(|w| w.slot == *component) {
            worn.push(Worn {
                slot: *component,
                by: garment_by(
                    garment,
                    variation.get(component).copied().unwrap_or(0),
                    choice,
                ),
            });
        }
    }
    worn.sort_by_key(|w| w.slot);
    let lift = match put_on.get(&Component::Feet) {
        Some(feet) if choice.heels => {
            heels::of(feet).map_or(0.0, |value| value * METRES_PER_HEEL_UNIT)
        }
        _ => 0.0,
    };
    let worn_ids: Vec<&str> = (put_on.values().copied())
        .chain(props_on.values().map(|(garment, _)| *garment))
        .map(|garment| garment.id.as_str())
        .collect();
    let mut props: Vec<WornProp> = props_on
        .into_iter()
        .map(|(prop, (garment, variation))| WornProp {
            prop,
            by: garment_by(garment, variation, choice),
        })
        .collect();
    let (driven, unmet) = apply_sets(wardrobe, choice, (&worn_ids, &own), &mut worn);
    if choice.first_person {
        // ⚠ After the sets: what the game hides in its first person view is still worn, and still causes the sets it is a source of.
        let hidden_there = |by: &By| match by {
            By::Garment { id, .. } => wardrobe
                .garment(id)
                .is_some_and(|g| flags::has(g, Flag::HideInFirstPerson)),
            By::Base { .. } => false,
        };
        for by in worn
            .iter()
            .map(|w| &w.by)
            .chain(props.iter().map(|w| &w.by))
        {
            if let (true, By::Garment { id, .. }) = (hidden_there(by), by) {
                left.push(Left::of(
                    id,
                    "is not drawn in the first person view: it is flagged hidden there",
                ));
            }
        }
        // ⚠ A convention of the preview and no measurement: the camera stands inside the ped's own head, hair, beard and teeth, which only hide what it sees.
        let inside = [
            Component::Head,
            Component::Berd,
            Component::Hair,
            Component::Teef,
        ];
        worn.retain(|w| {
            !hidden_there(&w.by) && !(matches!(w.by, By::Base { .. }) && inside.contains(&w.slot))
        });
        props.retain(|w| !hidden_there(&w.by));
    }
    Dressing {
        worn,
        props,
        left,
        hidden,
        lift,
        driven,
        unmet,
    }
}

impl Left {
    fn of(garment: &str, why: &str) -> Self {
        Self {
            garment: garment.to_string(),
            why: why.to_string(),
        }
    }
}

/// The slot `garment` is worn in on `ped`, or why it is not worn.
fn worn_slot(garment: &Garment, ped: Ped) -> Result<Slot, &'static str> {
    match (garment.ped, garment.slot) {
        (Some(wearer), _) if wearer != ped => Err("dresses the other ped"),
        (None, _) => Err("names no ped yet"),
        (_, None) => Err("names no slot yet"),
        (_, Some(_)) if garment.is_reserved() => Err("holds a place and has no model"),
        (_, Some(slot)) => Ok(slot),
    }
}

/// The model `garment` states for the first person view, where it states one it holds a model for.
fn first_person_model(garment: &Garment) -> Option<&String> {
    let number = usize::from(garment.meta.first_person_alternate?);
    garment.alternates.get(number.checked_sub(1)?)
}

/// What `garment` wears: the model, or the alternate model the choice asks for where the garment has one of that number, and the texture of `variation`.
///
/// The first person view wears the garment's first person alternate in place of any other choice of model.
fn garment_by(garment: &Garment, variation: usize, choice: &Choice) -> By {
    let alternate = choice.alternates.get(&garment.id).copied().unwrap_or(0);
    let model = (choice
        .first_person
        .then(|| first_person_model(garment))
        .flatten())
    .or_else(|| (alternate.checked_sub(1)).and_then(|n| garment.alternates.get(n)))
    .or(garment.model.as_ref());
    By::Garment {
        id: garment.id.clone(),
        model: model.cloned().unwrap_or_default(),
        texture: garment
            .variations
            .get(variation)
            .and_then(|v| v.texture.clone()),
    }
}

/// The ped's own drawable `drawable` of `component`, by the names of its files and its number: a race drawable wears the base word on its body and, on its head, the tone its own texture states.
///
/// With none asked for it is drawable 0, and a race head the first one of the person's `tone`.
///
/// ⚠ A texture of any `texId` but 0 is a race one; the heads state one `texId` for each tone.
fn base_by(
    component: Component,
    drawable: Option<u32>,
    tex_id: Option<u8>,
    tone: Tone,
) -> (By, u32) {
    let name = component.name();
    let skin = tex_id.map(|id| {
        if id == Skin::Universal.tex_id() {
            Skin::Universal
        } else {
            Skin::Race
        }
    });
    let (drawable, word) = match (component, skin, drawable) {
        (Component::Head, Some(Skin::Race), None) => (tone.head(), tone.word()),
        (Component::Head, Some(Skin::Race), Some(n)) => {
            let own = tex_id.and_then(Tone::of_tex_id);
            (n, own.map_or(Skin::Race.tint(), Tone::word))
        }
        (_, Some(skin), n) => (n.unwrap_or(0), skin.tint()),
        (_, None, n) => (n.unwrap_or(0), Skin::Universal.tint()),
    };
    let letter = skin.unwrap_or_default().letter();
    let by = By::Base {
        model: format!("{name}_{drawable:03}_{letter}.ydd"),
        texture: skin.map(|_| format!("{name}_diff_{drawable:03}_a_{word}.ytd")),
    };
    (by, drawable)
}

/// The model a ped's own `model` is replaced by for alternate `alt`: the number after its name, as the game names the file.
fn alternate_of(model: &str, alt: u8) -> String {
    match model.rsplit_once('.') {
        Some((name, extension)) => format!("{name}_{alt}.{extension}"),
        None => format!("{model}_{alt}"),
    }
}

/// The sets of alternate variations in force for what `choice` wears, applied to `worn`; the drawables they change, and what a set asks that the pack cannot show.
///
/// `(worn, own)` is the ids of the garments worn, and the number of the ped's own drawable worn in each slot.
fn apply_sets(
    wardrobe: &Wardrobe,
    choice: &Choice,
    (worn_ids, own): (&[&str], &BTreeMap<Component, u32>),
    worn: &mut [Worn],
) -> (Vec<Driven>, Vec<String>) {
    let (mut driven, mut unmet) = (Vec::new(), Vec::new());
    if choice.original {
        return (driven, unmet);
    }
    let is_worn = |asset: &Asset| match asset {
        Asset::Garment { garment } => worn_ids.contains(&garment.as_str()),
        // ⚠ The ped has no prop of its own, and another collection's drawables are not read here.
        Asset::Outside(o) => match (&o.dlc, o.slot) {
            (None, Slot::Component(c)) => own.get(&c) == Some(&o.index),
            _ => false,
        },
    };
    let in_force = |set: &&AlternateSet| {
        set.ped == choice.ped
            && match &choice.sets {
                Sets::Off => false,
                Sets::Only(id) => &set.id == id,
                Sets::Worn => set.sources.iter().any(is_worn),
            }
    };
    let mut changed: Vec<&Asset> = Vec::new();
    for set in wardrobe.alternates.iter().filter(in_force) {
        for switch in &set.switches {
            // The first set to name a drawable decides it.
            if changed.contains(&&switch.target) {
                continue;
            }
            let made = |overridden| Driven {
                target: switch.target.clone(),
                alt: switch.alt,
                set: set.id.clone(),
                name: set.name.clone(),
                overridden,
            };
            match &switch.target {
                Asset::Garment { garment: id } => {
                    let Some(slot) = worn
                        .iter_mut()
                        .find(|w| matches!(&w.by, By::Garment { id: of, .. } if of == id))
                    else {
                        continue;
                    };
                    let Some(g) = wardrobe.garment(id) else {
                        continue;
                    };
                    // The person's own choice of model, and the first person view's, are worn in place of a set's.
                    let chosen = choice.alternates.contains_key(id)
                        || (choice.first_person && first_person_model(g).is_some());
                    if chosen {
                        driven.push(made(true));
                    } else if let (Some(file), By::Garment { model, .. }) = (
                        usize::from(switch.alt)
                            .checked_sub(1)
                            .and_then(|n| g.alternates.get(n)),
                        &mut slot.by,
                    ) {
                        *model = file.clone();
                        driven.push(made(false));
                    } else {
                        unmet.push(format!(
                            "The set {} makes {} wear alternate {}, and it holds {} alternate model(s)",
                            set.name,
                            crate::rules::name_of(g),
                            switch.alt,
                            g.alternates.len()
                        ));
                        continue;
                    }
                }
                Asset::Outside(o) => {
                    let Slot::Component(c) = o.slot else {
                        continue;
                    };
                    if o.dlc.is_some() || own.get(&c) != Some(&o.index) {
                        continue;
                    }
                    let Some(Worn {
                        by: By::Base { model, .. },
                        ..
                    }) = worn.iter_mut().find(|w| w.slot == c)
                    else {
                        continue;
                    };
                    *model = alternate_of(model, switch.alt);
                    driven.push(made(false));
                }
            }
            changed.push(&switch.target);
        }
    }
    (driven, unmet)
}
