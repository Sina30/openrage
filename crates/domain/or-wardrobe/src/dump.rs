//! The clothes dump: what a pack states of each of its textures, in the shape the scripts of a server read.
//!
//! One object for each ped, with the collection it is in and an entry for each texture of each
//! garment, components and props apart. The keys and their order are the ones of the dump
//! Durty Cloth Tool writes for a server's resource (`a_dump_of_the_reference_pack_has_the_shape_of_the_one_it_ships`,
//! which reads the reference from `OPENRAGE_REFERENCE_PACK` and skips without it), spelling included:
//! a prop's last key starts with a small `i`.
//!
//! ⚠ **`DrawableId` is the place in the collection.** The reference states the place in the game's
//! list of the ped's drawables, which counts what the game and every pack mounted before this one hold,
//! and nothing a build knows says how many that is on a server. `RelativeCollectionDrawableId` is the
//! place in the collection, and a script that names the collection with it needs no count. Each ped's object
//! ends with [`RELATIVE_TO`], so the file itself says which of the two numbers `DrawableId` is.

use crate::alloc::Allocation;
use crate::layout;
use crate::metadata::durty_category;
use crate::model::Wardrobe;
use crate::names::{Ped, Prop, Slot};
use crate::shop;

/// Where a resource carries the dump as JSON.
pub const JSON: &str = "data/clothes_dump.json";

/// Where a resource carries the same data as a Lua table.
pub const LUA: &str = "data/clothes_dump.lua";

/// The key each ped's object ends with: it says what `DrawableId` counts from, the collection `DlcCollectionName` names, so a script cannot take it for the game's own list of the ped's drawables.
pub const RELATIVE_TO: &str = "DrawableIdIsRelativeTo";

/// What the dump says of one texture of one garment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The word the garment's slot is shown under: `Top`, `PropHead`.
    pub drawable_name: String,
    /// The texture's dictionary, as the build names it, without its extension; empty for a place held.
    pub texture_name: String,
    /// The id the shop metadata states for the texture.
    pub name_hash: String,
    /// `eCompType` of a component, `eAnchorPoint` of a prop.
    pub kind: String,
    /// `ePedVarComp` of a component, `eAnchorPoints` of a prop.
    pub component_id: u32,
    pub drawable_id: u32,
    pub texture_id: u32,
    pub relative: u32,
    /// A place held, which has no model and no texture.
    pub dummy: bool,
}

/// What the dump says of one ped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PedDump {
    pub collection: String,
    pub ped: Ped,
    pub components: Vec<Entry>,
    pub props: Vec<Entry>,
}

/// The dump of a pack.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dump {
    pub peds: Vec<PedDump>,
}

/// The word a component slot is shown under, which is the one a shop names it by.
fn component_word(category: &str) -> String {
    let mut chars = category.chars();
    chars
        .next()
        .map(|first| first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase())
        .unwrap_or_default()
}

/// The word a prop slot is shown under.
///
/// ⚠ Five anchors appear in the reference (`PropEars`, `PropEyes`, `PropHead`, `PropLWrist`, `PropRWrist`); the other four are
/// spelled by the same rule and were not measured.
fn prop_word(prop: Prop) -> &'static str {
    match prop {
        Prop::Head => "PropHead",
        Prop::Eyes => "PropEyes",
        Prop::Ears => "PropEars",
        Prop::Mouth => "PropMouth",
        Prop::LeftHand => "PropLHand",
        Prop::RightHand => "PropRHand",
        Prop::LeftWrist => "PropLWrist",
        Prop::RightWrist => "PropRWrist",
        Prop::Hip => "PropHip",
    }
}

fn word_of(slot: Slot) -> String {
    match slot {
        Slot::Component(c) => component_word(durty_category(c)),
        Slot::Prop(p) => prop_word(p).to_string(),
    }
}

fn number_of(slot: Slot) -> u32 {
    match slot {
        Slot::Component(c) => u32::from(c.id()),
        Slot::Prop(p) => u32::from(p.anchor()),
    }
}

/// The dump of `wardrobe` as `allocation` places it: each ped that has a garment, males first.
pub fn dump(wardrobe: &Wardrobe, allocation: &Allocation) -> Dump {
    let mut peds = Vec::new();
    // One object for each collection, since a script names a drawable by its collection and its number there.
    let collections = Ped::ALL.into_iter().flat_map(|ped| {
        // ⚠ The files of a game collection add no drawable to list.
        let parts = match wardrobe.replaces(ped) {
            Some(_) => Vec::new(),
            None => allocation.parts_of(ped),
        };
        parts.into_iter().map(move |part| (ped, part))
    });
    for (ped, part) in collections {
        let collection = wardrobe.collection_of(ped, part);
        let mut entries: Vec<(bool, Entry)> = Vec::new();
        for placed in (allocation.placed.iter()).filter(|p| p.ped == ped && p.part == part) {
            let garment = &wardrobe.garments[placed.garment];
            // A place held states no texture, and the dump still lists the place once.
            let textures = garment.variations.len().max(1);
            for texture in 0..textures {
                let tint = garment
                    .variations
                    .get(texture)
                    .and_then(|v| v.tint.as_deref())
                    .unwrap_or(garment.skin.tint());
                let name = match garment.is_reserved() {
                    true => String::new(),
                    false => layout::texture_name(
                        ped,
                        &collection,
                        placed.slot,
                        placed.position,
                        texture,
                        tint,
                    )
                    .trim_end_matches(".ytd")
                    .to_string(),
                };
                entries.push((
                    placed.slot.is_prop(),
                    Entry {
                        drawable_name: word_of(placed.slot),
                        texture_name: name,
                        name_hash: shop::id_of(wardrobe, garment, placed, texture),
                        kind: shop::kind_of(placed.slot),
                        component_id: number_of(placed.slot),
                        drawable_id: placed.position,
                        texture_id: u32::try_from(texture).unwrap_or(u32::MAX),
                        relative: placed.position,
                        dummy: garment.is_reserved(),
                    },
                ));
            }
        }
        // The reference lists a ped's components by the word they are shown under, then by place and texture.
        entries.sort_by(|a, b| {
            let key = |e: &Entry| (e.drawable_name.clone(), e.relative, e.texture_id);
            key(&a.1).cmp(&key(&b.1))
        });
        let (props, components): (Vec<_>, Vec<_>) =
            entries.into_iter().partition(|(prop, _)| *prop);
        peds.push(PedDump {
            collection,
            ped,
            components: components.into_iter().map(|(_, e)| e).collect(),
            props: props.into_iter().map(|(_, e)| e).collect(),
        });
    }
    Dump { peds }
}

/// The keys of an entry, in the order the dump writes them, with the value each takes.
fn fields(entry: &Entry, prop: bool) -> Vec<(&'static str, Field)> {
    let text = |s: &str| Field::Text(s.to_string());
    vec![
        ("DrawableName", text(&entry.drawable_name)),
        ("TextureName", text(&entry.texture_name)),
        ("NameHash", text(&entry.name_hash)),
        (
            if prop { "AnchorPoint" } else { "ComponentType" },
            text(&entry.kind),
        ),
        ("ComponentId", Field::Number(entry.component_id)),
        ("DrawableId", Field::Number(entry.drawable_id)),
        ("TextureId", Field::Number(entry.texture_id)),
        (
            "RelativeCollectionDrawableId",
            Field::Number(entry.relative),
        ),
        (
            if prop {
                "isDummyPlaceholder"
            } else {
                "IsDummyPlaceholder"
            },
            Field::Flag(entry.dummy),
        ),
    ]
}

enum Field {
    Text(String),
    Number(u32),
    Flag(bool),
}

impl Dump {
    /// The dump as JSON, indented by two spaces and ended with a line feed.
    pub fn json(&self) -> String {
        let mut out = String::from("[");
        for (n, ped) in self.peds.iter().enumerate() {
            out.push_str(if n == 0 { "\n  {\n" } else { ",\n  {\n" });
            out.push_str(&format!(
                "    \"DlcCollectionName\": {},\n    \"PedName\": {},\n",
                json_text(&ped.collection),
                json_text(ped.ped.model())
            ));
            json_list(&mut out, "ComponentVariations", &ped.components, false);
            out.push_str(",\n");
            json_list(&mut out, "Props", &ped.props, true);
            out.push_str(&format!(
                ",\n    \"{RELATIVE_TO}\": \"DlcCollectionName\"\n  }}"
            ));
        }
        out.push_str(if self.peds.is_empty() { "]\n" } else { "\n]\n" });
        out
    }

    /// The same data as a Lua table that a script gets with `load` or `dofile`, keys spelled as the JSON spells them.
    pub fn lua(&self) -> String {
        let mut out = String::from(
            "-- The clothes of the pack, one table for each ped. DrawableId is the place in the collection; use it with DlcCollectionName.\nreturn {\n",
        );
        for ped in &self.peds {
            out.push_str("  {\n");
            out.push_str(&format!(
                "    DlcCollectionName = {},\n    PedName = {},\n",
                lua_text(&ped.collection),
                lua_text(ped.ped.model())
            ));
            lua_list(&mut out, "ComponentVariations", &ped.components, false);
            lua_list(&mut out, "Props", &ped.props, true);
            out.push_str(&format!(
                "    {RELATIVE_TO} = \"DlcCollectionName\",\n  }},\n"
            ));
        }
        out.push_str("}\n");
        out
    }
}

fn json_text(text: &str) -> String {
    serde_json::to_string(text).expect("a string is JSON")
}

fn json_list(out: &mut String, key: &str, entries: &[Entry], prop: bool) {
    if entries.is_empty() {
        out.push_str(&format!("    \"{key}\": []"));
        return;
    }
    out.push_str(&format!("    \"{key}\": [\n"));
    for (n, entry) in entries.iter().enumerate() {
        out.push_str("      {\n");
        let fields = fields(entry, prop);
        for (m, (name, value)) in fields.iter().enumerate() {
            let value = match value {
                Field::Text(text) => json_text(text),
                Field::Number(n) => n.to_string(),
                Field::Flag(on) => on.to_string(),
            };
            let comma = if m + 1 < fields.len() { "," } else { "" };
            out.push_str(&format!("        \"{name}\": {value}{comma}\n"));
        }
        out.push_str(if n + 1 < entries.len() {
            "      },\n"
        } else {
            "      }\n"
        });
    }
    out.push_str("    ]");
}

/// A Lua string in double quotes: a backslash, a quote and a control character escaped, anything else as it is.
fn lua_text(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => out.push_str(&format!("\\{:03}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn lua_list(out: &mut String, key: &str, entries: &[Entry], prop: bool) {
    if entries.is_empty() {
        out.push_str(&format!("    {key} = {{}},\n"));
        return;
    }
    out.push_str(&format!("    {key} = {{\n"));
    for entry in entries {
        out.push_str("      {\n");
        for (name, value) in fields(entry, prop) {
            let value = match value {
                Field::Text(text) => lua_text(&text),
                Field::Number(n) => n.to_string(),
                Field::Flag(on) => on.to_string(),
            };
            out.push_str(&format!("        {name} = {value},\n"));
        }
        out.push_str("      },\n");
    }
    out.push_str("    },\n");
}
