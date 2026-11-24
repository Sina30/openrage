//! A scanned GTA VI map types body's archetypes from the Map screen: one copied under a name no
//! known archetype holds, one authored from the drawable open, or one taken out, each one step of
//! the body's document, which Save As writes as a loose resource.
//!
//! ⚠ A new name's 64-bit words are written as numbers: the body's string table takes no new text.
//! A 32-bit name and a word that equal the archetype's own name follow the new one, as a drawable
//! archetype's model word must.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use or_io::gta6::hash::{name_hash32, name_hash64};
use or_io::gta6::headerless::Resource;
use or_io::gta6::map::author::{authored, Authored};
use or_io::gta6::map::{self, MapBody};
use or_io::hash::literal_string_hash as lit;
use or_io::meta::binary::hash_index::default_hash_index;
use or_io::meta::instance::HashedString;
use or_io::meta::{MetaInstance, Value};

use super::entity::side;
use super::Source;
use crate::state::workbench::PickKind;
use crate::state::{AppState, DocPayload};

const ARCHETYPES: u32 = lit(b"archetypes");
const NAME: u32 = lit(b"name");
const ASSET_NAME: u32 = lit(b"assetName");
const ASSET_TYPE: u32 = lit(b"assetType");
const PHYSICS_DICTIONARY: u32 = lit(b"physicsDictionary");
const BB_MIN: u32 = lit(b"bbMin");
const BB_MAX: u32 = lit(b"bbMax");
const BS_CENTRE: u32 = lit(b"bsCentre");
const BS_RADIUS: u32 = lit(b"bsRadius");
const LOD_DIST: u32 = lit(b"lodDist");
const FLAGS: u32 = lit(b"flags");
const GUID: u32 = lit(b"guid");
const ID: u32 = lit(b"id");
/// The 64-bit word of the archetype's name, which an entity names it by.
const NAME_WORD: u32 = 0x8BFB_6095;
/// The drawable's word, which a drawable archetype holds equal to its name word.
const DRAWABLE_WORD: u32 = 0x99D5_6B7B;
const C_BASE_ARCHETYPE_DEF: &str = "CBaseArchetypeDef";
/// `ASSET_TYPE_DRAWABLE`.
const DRAWABLE: u64 = 2;

/// A member's name as the tree reader names it.
fn name(hash: u32) -> String {
    default_hash_index().name_or_literal(hash)
}

fn element(v: &Value) -> Option<&MetaInstance> {
    match v {
        Value::Struct(i) | Value::Pointer(Some(i)) => Some(&**i),
        _ => None,
    }
}

fn element_mut(v: &mut Value) -> Option<&mut MetaInstance> {
    match v {
        Value::Struct(i) | Value::Pointer(Some(i)) => Some(&mut **i),
        _ => None,
    }
}

/// The 32-bit hash a name member holds.
fn hash_of(v: &Value) -> Option<u32> {
    match v {
        Value::Hash(h) => Some(h.hash),
        Value::UInt(n) => u32::try_from(*n).ok(),
        _ => None,
    }
}

/// The 64-bit word a word member holds: the number, or the hash of its text.
fn word_of(v: &Value) -> Option<u64> {
    match v {
        Value::UInt(n) => Some(*n),
        Value::String(text) => Some(name_hash64(text.as_bytes())),
        _ => None,
    }
}

/// `n` as the kind of number `stored` holds.
fn number_like(stored: Option<&Value>, n: u64) -> Value {
    match stored {
        Some(Value::Int(_)) => Value::Int(n as i64),
        _ => Value::UInt(n),
    }
}

fn put(inst: &mut MetaInstance, member: u32, value: Value) -> Result<(), String> {
    let slot = inst.get_mut(&name(member)).ok_or_else(|| {
        tr!(
            "The archetype holds no value {member}.",
            member = name(member)
        )
    })?;
    *slot = value;
    Ok(())
}

fn gone() -> String {
    tr!("The archetype selected is not in its map types body's tree.").to_string()
}

fn no_plain() -> String {
    tr!("This map types body holds no plain archetype to copy the layout of.").to_string()
}

fn archetype_items(tree: &MetaInstance) -> Option<&Vec<Value>> {
    match tree.get(&name(ARCHETYPES)) {
        Some(Value::Array(items)) => Some(items),
        _ => None,
    }
}

/// The archetype of `items` named `hash`.
fn index_of(items: &[Value], hash: u32) -> Option<usize> {
    let named = |v: &Value| {
        element(v)
            .and_then(|i| i.get(&name(NAME)))
            .and_then(hash_of)
    };
    items.iter().position(|v| named(v) == Some(hash))
}

/// The first `CBaseArchetypeDef` of `items`, of a drawable where `drawable`.
fn first_plain(items: &[Value], drawable: bool) -> Option<usize> {
    items.iter().position(|v| {
        let Some(inst) = element(v) else {
            return false;
        };
        let is_drawable = match inst.get(&name(ASSET_TYPE)) {
            Some(Value::Enum { variant, .. }) => variant == "ASSET_TYPE_DRAWABLE",
            Some(Value::UInt(n)) => *n == DRAWABLE,
            Some(Value::Int(n)) => *n == DRAWABLE as i64,
            _ => false,
        };
        let plain = inst.type_name == C_BASE_ARCHETYPE_DEF
            || inst.type_name == name(lit(C_BASE_ARCHETYPE_DEF.as_bytes()));
        plain && (!drawable || is_drawable)
    })
}

/// A copy of `donor` named `text`, each name and word of it that equals its own name following.
fn renamed(donor: &Value, text: &str) -> Result<Value, String> {
    let mut copy = donor.clone();
    let inst = element_mut(&mut copy).ok_or_else(gone)?;
    let text = text.trim().to_ascii_lowercase();
    let (name32, word) = (name_hash32(text.as_bytes()), name_hash64(text.as_bytes()));
    let own_name = inst.get(&name(NAME)).and_then(hash_of);
    let own_word = inst.get(&name(NAME_WORD)).and_then(word_of);
    for member in [ASSET_NAME, PHYSICS_DICTIONARY] {
        if own_name.is_some() && inst.get(&name(member)).and_then(hash_of) == own_name {
            put(inst, member, Value::Hash(HashedString::from_hash(name32)))?;
        }
    }
    if own_word.is_some() && inst.get(&name(DRAWABLE_WORD)).and_then(word_of) == own_word {
        put(inst, DRAWABLE_WORD, Value::UInt(word))?;
    }
    put(inst, NAME, Value::Hash(HashedString::from_hash(name32)))?;
    put(inst, NAME_WORD, Value::UInt(word))?;
    Ok(copy)
}

/// A drawable archetype `a` named `text`, in the layout of `donor`, with the distance, guid and
/// flags a person gave.
fn authored_from(
    donor: &Value,
    text: &str,
    a: &Authored,
    (lod_dist, guid, flags): (f32, u64, u32),
) -> Result<Value, String> {
    let mut made = renamed(donor, text)?;
    let inst = element_mut(&mut made).ok_or_else(gone)?;
    put(inst, DRAWABLE_WORD, Value::UInt(a.name_word))?;
    put(
        inst,
        ASSET_NAME,
        Value::Hash(HashedString::from_hash(a.name)),
    )?;
    put(inst, BB_MIN, Value::Vec3(a.bb_min))?;
    put(inst, BB_MAX, Value::Vec3(a.bb_max))?;
    put(inst, BS_CENTRE, Value::Vec3(a.bs_centre))?;
    put(inst, BS_RADIUS, Value::Float(f64::from(a.bs_radius)))?;
    put(inst, LOD_DIST, Value::Float(f64::from(lod_dist)))?;
    let stored = inst.get(&name(FLAGS)).cloned();
    put(inst, FLAGS, number_like(stored.as_ref(), u64::from(flags)))?;
    // Every archetype measured holds its guid again as its id.
    let mut given = 0;
    for member in [GUID, ID] {
        if let Some(stored) = inst.get(&name(member)).cloned() {
            put(inst, member, number_like(Some(&stored), guid))?;
            given += 1;
        }
    }
    if given == 0 {
        return Err(tr!(
            "The archetype holds no value {member}.",
            member = name(GUID)
        ));
    }
    Ok(made)
}

fn select() -> String {
    tr!("Select an archetype on the Map screen first").to_string()
}

impl AppState {
    /// The map types document an archetype edit goes to, and the name of the archetype it starts
    /// from: the archetype of the entity selected on the scanned map shown, where its body is
    /// open; with no scanned map shown, the active document where it is a map types body.
    pub(crate) fn scanned_archetype_target(&self) -> Option<(PathBuf, Option<u32>)> {
        if let Some(shown) = self.scanned_map() {
            let Some(PickKind::Entity(index)) =
                self.workbench.maps.selected.as_ref().map(|p| p.kind)
            else {
                return None;
            };
            return match shown.archetypes.get(index).and_then(Option::as_ref) {
                Some((Source::Open(path), a)) => Some((path.clone(), Some(a.name))),
                _ => None,
            };
        }
        let path = self.active_document()?.path.clone()?;
        match super::opened_map(&path) {
            Some(Ok(body)) if matches!(*body, MapBody::Types(_)) => Some((path, None)),
            _ => None,
        }
    }

    /// The name and root box of the GTA VI drawable open, the first of several.
    pub(crate) fn open_gta6_drawable(&self) -> Option<(String, [f32; 3], [f32; 3])> {
        self.documents.iter().find_map(|d| match &d.payload {
            DocPayload::Gta6Drawable(read) => match &**read {
                Resource::Drawable(d) => Some((d.name.clone(), d.box_min, d.box_max)),
                _ => None,
            },
            _ => None,
        })
    }

    /// Copy the archetype [`Self::scanned_archetype_target`] names, else the first plain one of its
    /// body, under a name no known archetype holds, as one step.
    pub(crate) fn add_scanned_archetype(&mut self) {
        if let Err(why) = self.try_add_scanned_archetype() {
            self.show_error_toast(why);
        }
    }

    /// Take the archetype [`Self::scanned_archetype_target`] names out of its body, as one step.
    pub(crate) fn remove_scanned_archetype(&mut self) {
        if let Err(why) = self.try_remove_scanned_archetype() {
            self.show_error_toast(why);
        }
    }

    /// Declare in the body [`Self::scanned_archetype_target`] names an archetype of the GTA VI
    /// drawable open, named `text`, with `lodDist`, `guid` and the flags given, as one step.
    pub(crate) fn author_scanned_archetype(
        &mut self,
        text: &str,
        lod_dist: f32,
        guid: u64,
        flags: u32,
    ) -> Result<(), String> {
        let (path, _) = self.scanned_archetype_target().ok_or_else(select)?;
        let (_, min, max) = self.open_gta6_drawable().ok_or_else(|| {
            tr!("No GTA VI drawable is open to make an archetype of.").to_string()
        })?;
        if !lod_dist.is_finite() || lod_dist < 0.0 {
            return Err(tr!("lodDist, guid and flags take numbers").to_string());
        }
        let a = authored(text, min, max).map_err(|e| e.to_string())?;
        let text = text.trim().to_ascii_lowercase();
        if self.held_archetype_names().contains(&a.name) {
            return Err(tr!(
                "The name {name} is held by another archetype.",
                name = text.as_str()
            ));
        }
        let mut tree = self.types_tree(&path)?;
        let items = archetype_items(&tree).ok_or_else(no_plain)?;
        let donor = (first_plain(items, true).and_then(|at| items.get(at))).ok_or_else(no_plain)?;
        let made = authored_from(donor, &text, &a, (lod_dist, guid, flags))?;
        if let Some(Value::Array(items)) = tree.get_mut(&name(ARCHETYPES)) {
            items.push(made);
        }
        let label = tr!("Added the archetype {name}", name = text.as_str());
        self.set_types_tree(&path, tree, label)
    }

    fn try_add_scanned_archetype(&mut self) -> Result<(), String> {
        let (path, from) = self.scanned_archetype_target().ok_or_else(select)?;
        let held = self.held_archetype_names();
        let text = (1u32..)
            .map(|n| match n {
                1 => "prop_new".to_string(),
                n => format!("prop_new_{n}"),
            })
            .find(|t| !held.contains(&name_hash32(t.as_bytes())))
            .unwrap_or_default();
        let mut tree = self.types_tree(&path)?;
        let items = archetype_items(&tree).ok_or_else(no_plain)?;
        let at = match from {
            Some(hash) => index_of(items, hash).ok_or_else(gone)?,
            None => first_plain(items, false).ok_or_else(no_plain)?,
        };
        let copy = renamed(items.get(at).ok_or_else(gone)?, &text)?;
        if let Some(Value::Array(items)) = tree.get_mut(&name(ARCHETYPES)) {
            items.push(copy);
        }
        let label = tr!("Added the archetype {name}", name = text.as_str());
        self.set_types_tree(&path, tree, label)
    }

    fn try_remove_scanned_archetype(&mut self) -> Result<(), String> {
        let Some((path, Some(hash))) = self.scanned_archetype_target() else {
            return Err(select());
        };
        let mut tree = self.types_tree(&path)?;
        let at =
            (archetype_items(&tree).and_then(|items| index_of(items, hash))).ok_or_else(gone)?;
        if let Some(Value::Array(items)) = tree.get_mut(&name(ARCHETYPES)) {
            items.remove(at);
        }
        let label = tr!("Removed archetype {index}", index = at);
        self.set_types_tree(&path, tree, label)
    }

    /// The 32-bit names of every archetype an open or found map types body holds.
    fn held_archetype_names(&self) -> HashSet<u32> {
        let mut held = HashSet::new();
        self.each_known_archetype(|a| {
            held.insert(a.name);
        });
        held
    }

    /// The tree of the map types document at `path`.
    fn types_tree(&self, path: &Path) -> Result<MetaInstance, String> {
        let doc = (self.meta_document_at(path)).and_then(|doc| self.document(doc));
        match doc.map(|d| &d.payload) {
            Some(DocPayload::Meta(meta)) => (meta.parsed.as_ref())
                .map(|p| p.instance.clone())
                .ok_or_else(|| tr!("The scanned map's metadata did not read.").to_string()),
            _ => Err(tr!("The scanned map's document is closed.").to_string()),
        }
    }

    /// The map types document at `path` given `tree`, once it writes and reads back as one, as
    /// one step.
    fn set_types_tree(
        &mut self,
        path: &Path,
        tree: MetaInstance,
        label: String,
    ) -> Result<(), String> {
        let closed = || tr!("The scanned map's document is closed.").to_string();
        let not_read = || tr!("The scanned map's metadata did not read.").to_string();
        let doc = self.meta_document_at(path).ok_or_else(closed)?;
        let Some(DocPayload::Meta(meta)) = self.document(doc).map(|d| &d.payload) else {
            return Err(closed());
        };
        let before = meta.headerless_sections().ok_or_else(not_read)??;
        let mut edited = meta.clone();
        if let Some(parsed) = edited.parsed.as_mut() {
            parsed.instance = tree;
        }
        let after = side(edited.headerless_sections().ok_or_else(not_read)??)?;
        match map::open(&after.sections.virtual_section, &after.sections.physical) {
            Ok(MapBody::Types(_)) => {}
            Ok(MapBody::Data(_)) => {
                return Err(tr!("The edited map types body does not read back as one.").to_string())
            }
            Err(e) => return Err(e.to_string()),
        }
        self.make_map_step(doc, path, before, after, label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DONOR: &str = "prop_donor";

    /// A `class` archetype named `text`, of asset type `asset`, whose physics dictionary is
    /// `physics`, with a guid and an id where `ids`.
    fn archetype(class: &str, text: &str, asset: u64, physics: u32, ids: bool) -> Value {
        let mut inst = MetaInstance::new(class);
        let named = Value::Hash(HashedString::from_hash(name_hash32(text.as_bytes())));
        let word = name_hash64(text.as_bytes());
        inst.set(name(NAME_WORD), Value::String(text.to_string()));
        inst.set(
            name(DRAWABLE_WORD),
            Value::UInt(if asset == DRAWABLE { word } else { 0 }),
        );
        inst.set(name(LOD_DIST), Value::Float(80.0));
        inst.set(name(BB_MIN), Value::Vec3([-1.0; 3]));
        inst.set(name(BB_MAX), Value::Vec3([1.0; 3]));
        inst.set(name(BS_CENTRE), Value::Vec3([0.0; 3]));
        inst.set(name(BS_RADIUS), Value::Float(1.7));
        inst.set(name(FLAGS), Value::UInt(0x20));
        inst.set(name(NAME), named.clone());
        inst.set(
            name(PHYSICS_DICTIONARY),
            Value::Hash(HashedString::from_hash(physics)),
        );
        inst.set(name(ASSET_TYPE), Value::UInt(asset));
        inst.set(name(ASSET_NAME), named);
        if ids {
            inst.set(name(GUID), Value::UInt(0x77));
            inst.set(name(ID), Value::UInt(0x77));
        }
        Value::Pointer(Some(Box::new(inst)))
    }

    fn member(v: &Value, member: u32) -> Option<Value> {
        element(v).and_then(|i| i.get(&name(member))).cloned()
    }

    fn hashed(text: &str) -> Value {
        Value::Hash(HashedString::from_hash(name_hash32(text.as_bytes())))
    }

    #[test]
    fn a_copy_takes_the_new_name_and_the_names_and_words_that_followed_the_old_one() {
        let donor = archetype(C_BASE_ARCHETYPE_DEF, DONOR, DRAWABLE, 0, true);
        let copy = renamed(&donor, " Prop_New ").expect("renamed");
        let word = name_hash64(b"prop_new");
        assert_eq!(member(&copy, NAME), Some(hashed("prop_new")));
        assert_eq!(member(&copy, NAME_WORD), Some(Value::UInt(word)));
        // The drawable word and the asset name followed the name; a physics dictionary of 0 did not.
        assert_eq!(member(&copy, DRAWABLE_WORD), Some(Value::UInt(word)));
        assert_eq!(member(&copy, ASSET_NAME), Some(hashed("prop_new")));
        assert_eq!(
            member(&copy, PHYSICS_DICTIONARY),
            member(&donor, PHYSICS_DICTIONARY)
        );
        assert_eq!(member(&copy, BB_MAX), member(&donor, BB_MAX));

        // A fragment archetype's drawable word is 0, which no name follows.
        let fragment = archetype(
            C_BASE_ARCHETYPE_DEF,
            DONOR,
            1,
            name_hash32(DONOR.as_bytes()),
            true,
        );
        let copy = renamed(&fragment, "prop_new").expect("renamed");
        assert_eq!(member(&copy, DRAWABLE_WORD), Some(Value::UInt(0)));
        assert_eq!(member(&copy, PHYSICS_DICTIONARY), Some(hashed("prop_new")));
        assert!(
            renamed(&Value::UInt(1), "prop_new").is_err(),
            "no archetype"
        );
    }

    #[test]
    fn the_first_plain_archetype_is_found_by_class_and_asset_type_and_one_by_its_name() {
        let items = [
            archetype("CMloArchetypeDef", "a", 0, 0, true),
            archetype(C_BASE_ARCHETYPE_DEF, "b", 1, 0, true),
            archetype(C_BASE_ARCHETYPE_DEF, "c", DRAWABLE, 0, true),
        ];
        assert_eq!(first_plain(&items, false), Some(1));
        assert_eq!(first_plain(&items, true), Some(2));
        assert_eq!(first_plain(&items[..2], true), None);
        assert_eq!(index_of(&items, name_hash32(b"c")), Some(2));
        assert_eq!(index_of(&items, name_hash32(b"d")), None);
    }

    #[test]
    fn an_authored_archetype_takes_the_drawable_s_box_its_sphere_and_the_values_given() {
        let donor = archetype(C_BASE_ARCHETYPE_DEF, DONOR, DRAWABLE, 0, true);
        let a = authored("prop_bench", [-2.0, -1.0, 0.0], [2.0, 1.0, 2.0]).expect("authored");
        let made = authored_from(&donor, "prop_bench", &a, (150.0, 0xABC, 0x40)).expect("made");
        assert_eq!(member(&made, NAME), Some(hashed("prop_bench")));
        assert_eq!(member(&made, NAME_WORD), Some(Value::UInt(a.name_word)));
        assert_eq!(member(&made, DRAWABLE_WORD), Some(Value::UInt(a.name_word)));
        assert_eq!(member(&made, BB_MIN), Some(Value::Vec3([-2.0, -1.0, 0.0])));
        assert_eq!(member(&made, BS_CENTRE), Some(Value::Vec3([0.0, 0.0, 1.0])));
        let radius = f64::from(a.bs_radius);
        assert_eq!(member(&made, BS_RADIUS), Some(Value::Float(radius)));
        assert!((radius - 6f64.sqrt()).abs() < 1e-6, "{radius}");
        assert_eq!(member(&made, LOD_DIST), Some(Value::Float(150.0)));
        assert_eq!(member(&made, FLAGS), Some(Value::UInt(0x40)));
        assert_eq!(member(&made, GUID), Some(Value::UInt(0xABC)));
        assert_eq!(member(&made, ID), Some(Value::UInt(0xABC)));

        let no_ids = archetype(C_BASE_ARCHETYPE_DEF, DONOR, DRAWABLE, 0, false);
        let refused = authored_from(&no_ids, "prop_bench", &a, (150.0, 1, 0)).unwrap_err();
        assert!(refused.contains("guid"), "{refused}");
    }

    #[test]
    fn a_map_types_body_shown_alone_is_the_target_with_no_archetype_selected() {
        use or_io::gta6::map::MapTypes;

        let _serial = crate::ui::theme::serial();
        let dir = tempfile::tempdir().expect("a scratch directory");
        let path = dir.path().join("0x50.rbm");
        let types = MapTypes {
            name: 9,
            archetypes: Vec::new(),
        };
        super::super::keep_read(&path, MapBody::Types(types));
        let mut state = AppState::new();
        assert_eq!(state.scanned_archetype_target(), None);
        let meta = DocPayload::Meta(crate::state::document::MetaDocument::empty());
        let doc = state.insert_document(path.clone(), meta, None, None);
        state.active_document = Some(doc);
        state.refresh_scanned_map();
        assert_eq!(state.scanned_archetype_target(), Some((path, None)));
        // Nothing to take out, and an empty tree holds no plain archetype to copy.
        state.toasts.clear();
        state.remove_scanned_archetype();
        state.add_scanned_archetype();
        let said = crate::state::opening_an_rpf9_archive::said;
        assert!(said(
            &state,
            crate::state::ToastType::Error,
            &["Select an archetype"]
        ));
        assert!(said(
            &state,
            crate::state::ToastType::Error,
            &["no plain archetype"]
        ));
        assert!(!state.document(doc).expect("open").history.can_undo());
    }
}
