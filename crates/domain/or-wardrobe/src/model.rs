//! The model of a pack: garments of freemode peds, their variations, and what the pack waives.
//!
//! It is the file `wardrobe.json` of a clothing module, canonical like every source of OpenRage:
//! keys sorted, two-space indent, LF. A value the engine states by name carries the engine's name
//! (`propMask`), a word that is ours is snake case, and a value left out is the default the engine
//! writes for it, so a garment that states nothing is one of the game's own defaults.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::names::{Ped, Skin, Slot};
use crate::settings::{Replace, Settings};

/// The file a clothing module keeps its model in.
pub const FILE: &str = "wardrobe.json";
pub const FORMAT: &str = "openrage-wardrobe";
pub const SCHEMA: u32 = 4;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    #[error("{0} is not a wardrobe: {1}")]
    Json(String, serde_json::Error),
    #[error("{0} is not a wardrobe: its format is `{1}`, not `{FORMAT}`")]
    Format(String, String),
    #[error(
        "{0} was written by a newer OpenRage (schema {1}) than this one reads (schema {SCHEMA})"
    )]
    Newer(String, u32),
    #[error("{0} is not a wardrobe: the garment id {1:?} is not one plain file name")]
    Id(String, String),
}

/// A pack of clothing for the freemode peds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Wardrobe {
    pub format: String,
    pub schema: u32,
    /// The name of the pack: the folder of its streamed files, the stem of its collections and of its creature metadata.
    pub pack: String,
    /// The collection each ped's garments are in, as the game resolves it; `mp_m_<pack>` and `mp_f_<pack>` where a ped has none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub collections: BTreeMap<Ped, String>,
    pub garments: Vec<Garment>,
    /// The drawables that wear an alternate model while garments of the pack are worn ([`crate::alternate`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternates: Vec<crate::alternate::AlternateSet>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waivers: Vec<Waiver>,
    /// The queries the pack keeps by name, which the list offers as smart groups ([`crate::query`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queries: Vec<crate::query::SavedQuery>,
    /// How the shop metadata is written; the default is OpenRage's.
    #[serde(default, skip_serializing_if = "Shop::is_default")]
    pub shop: Shop,
    /// What the project changed of the rules the pack is held to: the edition it is built for, and for a rule that can change, whether it is on, how serious it is and where its limit stands.
    #[serde(default, skip_serializing_if = "Settings::is_default")]
    pub rules: Settings,
    /// The drawables the last build shipped, read from `wardrobe.lock.json` beside this file and never written with it ([`crate::freeze`]).
    #[serde(skip)]
    #[schemars(skip)]
    pub lock: Option<crate::freeze::Lock>,
}

/// How the pack's shop metadata is written, and what an import kept of a shop's own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shop {
    /// Name each entry that states no id as Durty Cloth Tool names it (`CLO_DCT_TOP_0_0_M`), so a script, a shop table or a package keyed on its names still finds the entries after a move to OpenRage; off, an entry is named for its collection, slot, position and texture.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub durty_hashes: bool,
    /// Entries an import read that point at no garment, kept as they were stated and written back with the rest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unplaced: Vec<Stated>,
    /// Files a resource's manifest registers with the shop that this engine does not read, carried unchanged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub carried: Vec<Carried>,
}

impl Shop {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// What a shop's metadata states of one entry, kept as written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShopEntry {
    /// `uniqueNameHash` exactly as the file wrote it: a name the game hashes, or a literal value, never read as a number because it need not fit 32 bits. Empty for an entry a person made by editing the price or the tags of a texture no file stated, which the build names.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// The file it was read from, as the import saw its path; empty for an entry made here.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub from: String,
    /// Which entry of that file it was, counting from 0 through the components and then the props.
    pub entry: u32,
    /// The `value` of its `cost`, as written; the plan writes 0 where none is stated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<String>,
    /// Every other element the entry states (`forcedComponents`, `restrictionTags`), as XML in the order stated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
}

/// An entry of a shop file that points at no garment of the pack, with the place it points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Stated {
    pub ped: Ped,
    /// `eCompType` or `eAnchorPoint` as written.
    pub kind: String,
    /// Whether it was in the props of the file; the components otherwise.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prop: bool,
    /// `localDrawableIndex` or `localPropIndex`.
    pub drawable: u32,
    #[serde(rename = "textureIndex")]
    pub texture: u32,
    pub entry: ShopEntry,
}

/// A file the manifest registers with the shop and that is carried unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Carried {
    /// The file, as a path inside the module's folder.
    pub file: String,
    /// The manifest line that registered it: its directive (`data_file`, or a server's own) and its type (`SHOP_PED_APPAREL_SPAK_FILE`).
    pub directive: String,
    pub kind: String,
}

/// One drawable of a slot, or a place held for one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Garment {
    /// Stable for as long as the garment exists and never reused: a rename, a renumbering or an edit keeps it.
    pub id: String,
    /// A name for people; the game never sees it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Words a person files the garment under, for finding it again; the game never sees them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// The one named set a person keeps it in, beside its ped and slot; the game never sees it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// The ped it dresses, none while nobody has said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ped: Option<Ped>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<Slot>,
    /// Where it sits among the drawables of its slot, when somebody decided; it takes the lowest free place otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<u32>,
    #[serde(default, skip_serializing_if = "is_universal")]
    pub skin: Skin,
    /// The `.ydd`, as a path inside the module's folder with `/`; none for a place held, which the game sees as a drawable with no model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The drawables the game shows in place of the model, `<model>_1.ydd` and on, which `numAlternatives` counts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alternates: Vec<String>,
    /// The `.yld`, as a path inside the module's folder: the cloth the game simulates for a component's drawable, shipped under the model's own name. A garment has cloth when one is linked, and `ownsCloth` is written from that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloth: Option<String>,
    /// An import found the metadata stating `ownsCloth` and no cloth file for it. Never set by hand: linking a cloth or unlinking clears it, and while it stands the garment is missing its cloth.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub missing_cloth: bool,
    /// The wind scale the build writes into every cloth of the linked `.yld`, in place of the one the file states; none ships the file as it is ([`crate::wind`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind_scale: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variations: Vec<Variation>,
    #[serde(default, skip_serializing_if = "Meta::is_empty")]
    pub meta: Meta,
    /// The entries it adds to the creature metadata of the pack, such as the expression of a heel.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expressions: Vec<Expression>,
}

fn is_universal(skin: &Skin) -> bool {
    *skin == Skin::Universal
}

/// One texture of a garment.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Variation {
    /// The `.ytd`, as a path inside the module's folder; none for a texture held for a place with no file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<String>,
    /// The word a component texture's name ends with; `uni` or `whi` by the garment's skin where it states none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<String>,
    #[serde(rename = "texId", default, skip_serializing_if = "Option::is_none")]
    pub tex_id: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distribution: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inclusions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusions: Option<u32>,
    #[serde(
        rename = "inclusionId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub inclusion_id: Option<u8>,
    #[serde(
        rename = "exclusionId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub exclusion_id: Option<u8>,
    /// The shop entry an import read for this texture: its id is the one a build writes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shop: Option<ShopEntry>,
    /// An id somebody chose for this texture's shop entry, which wins over the one an import read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shop_id: Option<String>,
}

impl Variation {
    /// The id this texture's shop entry is written with, none where the build names it.
    pub fn stated_shop_id(&self) -> Option<&str> {
        self.shop_id.as_deref().or_else(|| {
            (self.shop.as_ref())
                .map(|e| e.id.as_str())
                .filter(|id| !id.is_empty())
        })
    }
}

macro_rules! meta {
    ($($field:ident: $ty:ty = $name:literal,)*) => {
        /// What the engine's metadata states of a garment beyond its textures, under the engine's names; a value left out is the one the engine writes for a new record.
        #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct Meta {
            $(
                #[serde(rename = $name, default, skip_serializing_if = "Option::is_none")]
                pub $field: Option<$ty>,
            )*
        }

        impl Meta {
            pub fn is_empty(&self) -> bool {
                *self == Self::default()
            }
        }
    };
}

meta! {
    // CPVDrawblData
    prop_mask: u8 = "propMask",
    num_alternatives: u8 = "numAlternatives",
    // CComponentInfo
    ped_xml_audio_id: String = "pedXml_audioID",
    ped_xml_audio_id2: String = "pedXml_audioID2",
    ped_xml_expression_mods: [f32; 5] = "pedXml_expressionMods",
    flags: u32 = "flags",
    inclusions: u32 = "inclusions",
    exclusions: u32 = "exclusions",
    ped_xml_vfx_comps: u16 = "pedXml_vfxComps",
    ped_xml_flags: u16 = "pedXml_flags",
    // CPedPropMetaData (its `flags` is the `flags` above, in 16 bits)
    audio_id: String = "audioId",
    expression_mods: [f32; 5] = "expressionMods",
    render_flags: u8 = "renderFlags",
    prop_flags: u32 = "propFlags",
    stickyness: u8 = "stickyness",
    // FirstPersonAlternate: the alternate model the first person view streams in for the drawable
    first_person_alternate: u8 = "firstPersonAlternate",
}

/// An entry of the creature metadata a garment adds: the tracks that drive an expression of its drawable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Expression {
    /// `pedCompExpressionIndex` or `pedPropExpressionIndex`.
    pub index: u32,
    pub tracks: Vec<u8>,
    pub ids: Vec<u16>,
    pub types: Vec<u8>,
    pub components: Vec<u8>,
}

/// A rule the pack does not hold itself to, with the reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Waiver {
    pub code: String,
    /// The garment it concerns, by id; every garment where none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub garment: Option<String>,
    pub reason: String,
}

/// The schema of `wardrobe.json`, kept beside those of a source folder's files.
pub fn schema() -> or_source::schemas::Schema {
    or_source::schemas::Schema {
        file: "wardrobe.schema.json",
        binds: &["**/wardrobe.json"],
        schema: or_source::schemas::generate::<Wardrobe>(),
    }
}

impl Wardrobe {
    pub fn new(pack: &str) -> Self {
        Self {
            format: FORMAT.to_string(),
            schema: SCHEMA,
            pack: pack.to_string(),
            collections: BTreeMap::new(),
            garments: Vec::new(),
            alternates: Vec::new(),
            waivers: Vec::new(),
            queries: Vec::new(),
            shop: Shop::default(),
            rules: Settings::default(),
            lock: None,
        }
    }

    /// The collection the garments of `ped` are in: the one the pack states, else `mp_<m|f>_<pack>`.
    pub fn collection(&self, ped: Ped) -> String {
        if let Some(replace) = self.rules.replace.get(&ped) {
            // The game's own: a DLC's name, or none for the ped's base drawables, whose file names carry no collection.
            return replace.dlc.clone().unwrap_or_default();
        }
        self.collections
            .get(&ped)
            .cloned()
            .unwrap_or_else(|| format!("mp_{}_{}", ped.letter(), self.pack))
    }

    /// What the files of `ped` replace of the game, none where the pack adds a collection of its own.
    pub fn replaces(&self, ped: Ped) -> Option<&Replace> {
        self.rules.replace.get(&ped)
    }

    /// The collection that holds part `part` of the slots of `ped`: the one the pack states for part 0, and `<collection>_<n>` for the n-th collection a slot past its cap is cut into.
    pub fn collection_of(&self, ped: Ped, part: u32) -> String {
        match part {
            0 => self.collection(ped),
            n => format!("{}_{n}", self.collection(ped)),
        }
    }

    /// Whether the two packs state the same, the lock aside: that is a file of its own, which a build writes and an edit does not, so an Undo is not refused for a build made since.
    pub fn states_the_same_as(&self, other: &Self) -> bool {
        if self.lock == other.lock {
            return self == other;
        }
        let mut same = self.clone();
        same.lock.clone_from(&other.lock);
        same == *other
    }

    pub fn garment(&self, id: &str) -> Option<&Garment> {
        self.garments.iter().find(|g| g.id == id)
    }

    pub fn from_json(bytes: &[u8], shown: &str) -> Result<Self, Error> {
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        let mut value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| Error::Json(shown.to_string(), e))?;
        // A file of another format or a newer schema is said so before its shape is asked for.
        if let Some(format) = value["format"].as_str().filter(|f| *f != FORMAT) {
            return Err(Error::Format(shown.to_string(), format.to_string()));
        }
        if let Some(schema) = value["schema"].as_u64().filter(|s| *s > u64::from(SCHEMA)) {
            return Err(Error::Newer(
                shown.to_string(),
                u32::try_from(schema).unwrap_or(u32::MAX),
            ));
        }
        migrate(&mut value);
        let found: Wardrobe =
            serde_json::from_value(value).map_err(|e| Error::Json(shown.to_string(), e))?;
        // The file is text meant to be edited in Git, and an id is a stable name that other files and the record of a build hold.
        if let Some(bad) = found
            .garments
            .iter()
            .find(|g| !or_project::paths::is_component(&g.id))
        {
            return Err(Error::Id(shown.to_string(), bad.id.clone()));
        }
        Ok(found)
    }

    /// The canonical text of the file: keys sorted, two-space indent, LF.
    pub fn to_json(&self) -> String {
        let value = serde_json::to_value(self).expect("a wardrobe is JSON");
        let mut text = serde_json::to_string_pretty(&value).expect("a JSON value serialises");
        text.push('\n');
        text
    }

    /// The wardrobe of the module folder `folder`.
    pub fn read(folder: &Path) -> Result<Self, Error> {
        let path = folder.join(FILE);
        let shown = path.display().to_string();
        let bytes = std::fs::read(&path).map_err(|e| Error::Io(shown.clone(), e))?;
        let mut found = Self::from_json(&bytes, &shown)?;
        // ⚠ A lock that does not read freezes nothing and is said in the log: the next build that goes through writes it again.
        found.lock = crate::freeze::read(folder).unwrap_or_else(|why| {
            tracing::warn!(%why, folder = %folder.display(), "the lock of the pack does not read");
            None
        });
        Ok(found)
    }

    /// Write the file into `folder`, replacing the one there, and keep what was written as a version ([`crate::backup`]).
    pub fn write(&self, folder: &Path) -> Result<(), Error> {
        let path = folder.join(FILE);
        let shown = path.display().to_string();
        let before = std::fs::read(&path).ok();
        let text = self.to_json();
        or_project::content::folder::write_atomic(&path, &text).map_err(|e| Error::Io(shown, e))?;
        // ⚠ A pack that is made has no earlier version to keep, and makes no private folder until the first change after it.
        if before.is_some() {
            crate::backup::keep(folder, before.as_deref(), self, &text);
        }
        Ok(())
    }
}

/// Bring a file of an older schema to the current one, which it is saved as.
///
/// Schema 2 added the keys `alternates` and `firstPersonAlternate` and changed none, so it only
/// takes the new number. Schema 1 kept `ownsCloth` in a garment's metadata. A garment has cloth now
/// when one is linked, so the key goes: a garment that stated it with no cloth file is missing its
/// cloth, a cloth the flag did not turn on is no longer linked, and a waiver of the rule that said
/// so keeps its reason under the rule that replaced it.
fn migrate(value: &mut serde_json::Value) {
    use serde_json::Value;
    let Some(file) = value.as_object_mut() else {
        return;
    };
    let from = file.get("schema").and_then(Value::as_u64);
    if from >= Some(u64::from(SCHEMA)) {
        return;
    }
    // ⚠ The cloth move below is schema 1's: a garment of schema 2 or 3 states no `ownsCloth`, so it would unlink every cloth.
    if from >= Some(2) {
        file.insert("schema".into(), SCHEMA.into());
        return;
    }
    let each = |file: &mut serde_json::Map<String, Value>, key: &str| -> Vec<Value> {
        file.get_mut(key)
            .and_then(Value::as_array_mut)
            .map(std::mem::take)
            .unwrap_or_default()
    };
    let mut garments = each(file, "garments");
    for garment in garments.iter_mut().filter_map(Value::as_object_mut) {
        let owns = (garment.get_mut("meta"))
            .and_then(Value::as_object_mut)
            .and_then(|meta| meta.remove("ownsCloth"));
        match (
            owns == Some(Value::Bool(true)),
            garment.contains_key("cloth"),
        ) {
            (true, false) => {
                garment.insert("missing_cloth".into(), Value::Bool(true));
            }
            // ⚠ The game did not use a cloth whose flag was off or unstated, and a linked cloth is one it uses: the link goes and the file stays where it is.
            (false, true) => {
                garment.remove("cloth");
            }
            _ => {}
        }
    }
    let mut waivers = each(file, "waivers");
    for waiver in waivers.iter_mut().filter_map(Value::as_object_mut) {
        if waiver.get("code").and_then(Value::as_str) == Some("cloth-not-shipped") {
            waiver.insert("code".into(), "cloth-missing".into());
        }
    }
    if file.contains_key("garments") {
        file.insert("garments".into(), Value::Array(garments));
    }
    if file.contains_key("waivers") {
        file.insert("waivers".into(), Value::Array(waivers));
    }
    if file.contains_key("schema") {
        file.insert("schema".into(), SCHEMA.into());
    }
}

impl Garment {
    /// A garment with a new id and nothing else decided.
    pub fn new() -> Self {
        Self {
            id: new_id(),
            label: None,
            tags: Vec::new(),
            group: None,
            ped: None,
            slot: None,
            position: None,
            skin: Skin::Universal,
            model: None,
            alternates: Vec::new(),
            cloth: None,
            missing_cloth: false,
            wind_scale: None,
            variations: Vec::new(),
            meta: Meta::default(),
            expressions: Vec::new(),
        }
    }

    /// Whether it holds a place and ships no model, as the game's own gaps do.
    pub fn is_reserved(&self) -> bool {
        self.model.is_none()
    }

    /// Whether it is filed under one of `tags`, whatever the case.
    pub fn carries_a_tag_of(&self, tags: &[String]) -> bool {
        let word = |t: &str| t.trim().to_lowercase();
        (self.tags.iter()).any(|have| tags.iter().any(|want| word(have) == word(want)))
    }

    /// Keep only the place it holds, as a build that leaves it out does: its id, name, ped, slot and position stay, and it ships no model, texture, cloth or metadata.
    pub fn hold_its_place(&mut self) {
        let mut held = Self::new();
        held.id.clone_from(&self.id);
        held.label.clone_from(&self.label);
        held.ped = self.ped;
        held.slot = self.slot;
        held.position = self.position;
        *self = held;
    }
}

impl Default for Garment {
    fn default() -> Self {
        Self::new()
    }
}

pub use or_project::content::folder::new_id;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::Component;

    fn jacket() -> Garment {
        let mut g = Garment::new();
        g.label = Some("jacket".into());
        g.ped = Some(Ped::Male);
        g.slot = Some(Slot::Component(Component::Jbib));
        g.position = Some(12);
        g.model = Some("models/jacket.ydd".into());
        g.variations = vec![
            Variation {
                texture: Some("textures/jacket_a.ytd".into()),
                ..Variation::default()
            },
            Variation {
                texture: Some("textures/jacket_b.ytd".into()),
                tex_id: Some(1),
                ..Variation::default()
            },
        ];
        g.meta.prop_mask = Some(17);
        g.meta.ped_xml_expression_mods = Some([0.0, 0.0, 0.185, 0.0, 0.0]);
        g
    }

    #[test]
    fn a_wardrobe_is_written_canonically_and_reads_back_equal() {
        let mut w = Wardrobe::new("unity_clothes");
        w.collections.insert(Ped::Female, "mp_f_unity".into());
        w.garments.push(jacket());
        w.waivers.push(Waiver {
            code: "duplicate-content".into(),
            garment: Some(w.garments[0].id.clone()),
            reason: "the same mesh on purpose".into(),
        });
        let text = w.to_json();
        assert!(text.ends_with("}\n") && !text.contains('\r'));
        let keys: Vec<&str> = text
            .lines()
            .filter(|l| l.starts_with("  \""))
            .map(|l| l.trim().split('"').nth(1).unwrap())
            .collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted, "keys sorted:\n{text}");
        assert!(
            text.contains("\"propMask\": 17") && text.contains("\"texId\": 1"),
            "{text}"
        );
        assert_eq!(Wardrobe::from_json(text.as_bytes(), "x").unwrap(), w);
        assert_eq!(
            Wardrobe::from_json(text.as_bytes(), "x").unwrap().to_json(),
            text
        );
    }

    #[test]
    fn a_value_left_out_is_not_written_and_a_misspelt_key_is_refused() {
        let mut g = Garment::new();
        g.model = Some("a.ydd".into());
        let text = serde_json::to_string(&g).unwrap();
        assert_eq!(
            text.matches(':').count(),
            2,
            "only the id and the model: {text}"
        );

        let bad = r#"{"id":"x","propmask":1}"#;
        assert!(serde_json::from_str::<Garment>(bad).is_err());
        let bad = r#"{"id":"x","meta":{"propmask":1}}"#;
        assert!(serde_json::from_str::<Garment>(bad).is_err());
    }

    #[test]
    fn a_collection_is_the_one_stated_or_the_pack_with_the_peds_letter() {
        let mut w = Wardrobe::new("unity_clothes");
        assert_eq!(w.collection(Ped::Male), "mp_m_unity_clothes");
        assert_eq!(w.collection(Ped::Female), "mp_f_unity_clothes");
        w.collections
            .insert(Ped::Male, "male_freemode_beach".into());
        assert_eq!(w.collection(Ped::Male), "male_freemode_beach");
    }

    #[test]
    fn a_file_of_another_format_or_a_newer_schema_is_not_read() {
        let other = br#"{"format":"x","schema":1,"pack":"p","garments":[]}"#;
        assert!(matches!(
            Wardrobe::from_json(other, "f"),
            Err(Error::Format(..))
        ));
        let newer = format!(
            r#"{{"format":"{FORMAT}","schema":{},"pack":"p","garments":[]}}"#,
            SCHEMA + 1
        );
        assert!(matches!(
            Wardrobe::from_json(newer.as_bytes(), "f"),
            Err(Error::Newer(..))
        ));
    }

    #[test]
    fn a_file_of_schema_1_is_read_with_its_cloth_moved_to_what_replaced_ownscloth_and_is_saved_as_the_current_schema(
    ) {
        let old = r#"{"format":"openrage-wardrobe","schema":1,"pack":"p","garments":[
            {"id":"stated_no_file","model":"a.ydd","meta":{"ownsCloth":true}},
            {"id":"stated_and_shipped","model":"b.ydd","cloth":"b.yld","meta":{"ownsCloth":true}},
            {"id":"file_but_flag_off","model":"c.ydd","cloth":"c.yld","meta":{"ownsCloth":false}},
            {"id":"file_but_flag_unstated","model":"d.ydd","cloth":"d.yld"},
            {"id":"no_cloth","model":"e.ydd","meta":{"propMask":3}}
        ],"waivers":[{"code":"cloth-not-shipped","reason":"added by hand later"}]}"#;
        let w = Wardrobe::from_json(old.as_bytes(), "wardrobe.json").expect("schema 1 still reads");
        let garment = |id: &str| w.garment(id).expect(id);

        assert_eq!(w.schema, SCHEMA);
        assert!(
            garment("stated_no_file").missing_cloth && garment("stated_no_file").cloth.is_none()
        );
        assert_eq!(
            garment("stated_and_shipped").cloth.as_deref(),
            Some("b.yld")
        );
        assert!(!garment("stated_and_shipped").missing_cloth);
        // ⚠ The game did not use a cloth whose flag was off or unstated, and a linked cloth is one it uses: the link goes, the file stays where it is.
        for id in ["file_but_flag_off", "file_but_flag_unstated", "no_cloth"] {
            assert!(
                garment(id).cloth.is_none() && !garment(id).missing_cloth,
                "{id}"
            );
        }
        assert_eq!(garment("no_cloth").meta.prop_mask, Some(3));
        assert_eq!(w.waivers[0].code, "cloth-missing");
        let saved = w.to_json();
        assert!(!saved.contains("ownsCloth") && !saved.contains("cloth-not-shipped"));
        assert!(saved.contains(&format!("\"schema\": {SCHEMA}")), "{saved}");
    }

    #[test]
    fn what_a_file_writes_is_schema_4_because_a_reader_of_3_would_read_it_and_drop_its_saved_queries(
    ) {
        // Schema 3 is the last number an OpenRage that knows nothing of `queries` reads.
        assert_eq!(SCHEMA, 4);
        assert!(Wardrobe::new("p").to_json().contains("\"schema\": 4"));
    }

    #[test]
    fn a_file_of_schema_3_is_read_whole_and_is_saved_as_schema_4() {
        let old = r#"{"format":"openrage-wardrobe","schema":3,"pack":"p",
            "garments":[{"id":"coat","model":"a.ydd","cloth":"a.yld"}]}"#;
        let w = Wardrobe::from_json(old.as_bytes(), "wardrobe.json").expect("schema 3 still reads");
        assert_eq!(w.schema, SCHEMA);
        // ⚠ Schema 3 states no `ownsCloth` either, so the move that schema 1 needs would drop this link.
        assert_eq!(w.garment("coat").unwrap().cloth.as_deref(), Some("a.yld"));
        assert!(w.queries.is_empty());
        assert!(w.to_json().contains("\"schema\": 4"));
    }

    #[test]
    fn a_file_of_schema_2_is_read_as_it_was_written_and_is_saved_as_the_current_schema() {
        let old = r#"{"format":"openrage-wardrobe","schema":2,"pack":"p",
            "alternates":[{"id":"set","name":"Set","ped":"mp_m_freemode_01"}],
            "garments":[
            {"id":"coat","model":"a.ydd","cloth":"a.yld","meta":{"firstPersonAlternate":2}},
            {"id":"gap"}
        ],"waivers":[{"code":"cloth-missing","reason":"kept"}]}"#;
        let w = Wardrobe::from_json(old.as_bytes(), "wardrobe.json").expect("schema 2 still reads");

        assert_eq!(w.schema, SCHEMA);
        assert_eq!(w.alternates.len(), 1);
        let coat = w.garment("coat").expect("coat");
        assert_eq!(coat.meta.first_person_alternate, Some(2));
        // ⚠ Schema 2 states no `ownsCloth`, so the move that schema 1 needs would drop this link.
        assert_eq!(coat.cloth.as_deref(), Some("a.yld"));
        assert_eq!(w.waivers[0].code, "cloth-missing");
        let saved = w.to_json();
        assert!(saved.contains("\"schema\": 4"), "{saved}");
        assert_eq!(
            Wardrobe::from_json(saved.as_bytes(), "f").expect("again"),
            w
        );
    }

    #[test]
    fn a_file_of_a_schema_this_build_does_not_know_is_refused_with_a_sentence_that_says_which() {
        let newer = r#"{"format":"openrage-wardrobe","schema":5,"pack":"p","garments":[]}"#;
        let refused = Wardrobe::from_json(newer.as_bytes(), "wardrobe.json")
            .expect_err("a newer schema is not read")
            .to_string();
        assert_eq!(
            refused,
            "wardrobe.json was written by a newer OpenRage (schema 5) than this one reads (schema 4)"
        );
    }

    #[test]
    fn a_file_whose_garment_has_an_id_that_is_no_plain_file_name_is_not_read() {
        let file = |id: &str| {
            format!(
                r#"{{"format":"{FORMAT}","schema":{SCHEMA},"pack":"p","garments":[{{"id":"{id}"}}]}}"#
            )
        };
        for bad in ["", "a/b", "a\\\\b", "..", "a?", "tail.", "a\\u0000b"] {
            let refused = Wardrobe::from_json(file(bad).as_bytes(), "wardrobe.json")
                .expect_err(&format!("{bad:?} was read"))
                .to_string();
            assert!(
                refused.contains("wardrobe.json") && refused.contains("plain file name"),
                "{refused}"
            );
        }
        // Wide characters are a file name on every host, so the file is read.
        assert!(Wardrobe::from_json(file("aéééé").as_bytes(), "wardrobe.json").is_ok());
    }

    #[test]
    fn an_id_is_one_plain_file_name_and_is_new_every_time() {
        let (a, b) = (new_id(), new_id());
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    }
}
