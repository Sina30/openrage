//! A particle list's rules as the document `particles.json`: the list's name and its effect, emitter and particle rules, each in the
//! order of its dictionary, which is the order of the hash of its name.
//!
//! The textures and the models of a list are not here: a source holds the textures as DDS files and the models as a list of their own.

use std::collections::BTreeSet;

use or_io::decl::{Owned, Ref};
use or_io::gta5::drawable::DrawableTarget;
use or_io::gta5::particle::{
    AtBinaryMapDataPair, PtxEffectRule, PtxEmitterRule, PtxEvolutionList, PtxFxList,
    PtxParticleRule,
};
use or_io::gta5::PgDictionary;
use or_io::string_hash_str;
use serde_json::{Map, Value};

use super::text::{Fault, Json, Trail};

/// The members of the document.
const MEMBERS: [&str; 4] = ["name", "effect_rules", "emitter_rules", "particle_rules"];

/// Why a list cannot be written as a document, one sentence for each thing in the way.
pub fn refusals<T: DrawableTarget>(list: &PtxFxList<T>) -> Vec<String> {
    fn keyed<R: Owned>(
        what: &str,
        dict: &Option<Box<PgDictionary<R>>>,
        name: fn(&R) -> &str,
    ) -> Vec<String>
    where
        or_io::decl::Raw<R>: or_io::decl::Record,
    {
        let Some(dict) = dict else { return Vec::new() };
        if dict.codes.len() != dict.entries.len() {
            return vec![format!(
                "its {what} dictionary holds {} hashes and {} entries",
                dict.codes.len(),
                dict.entries.len()
            )];
        }
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        for (code, entry) in dict.codes.iter().zip(&dict.entries) {
            let Some(rule) = entry else {
                out.push(format!("its {what} dictionary holds an empty entry"));
                continue;
            };
            if string_hash_str(name(rule)) != *code {
                out.push(format!(
                    "the {what} `{}` is filed under a hash that is not its name's",
                    name(rule)
                ));
            }
            if !seen.insert(*code) {
                out.push(format!(
                    "two {what}s are filed under the hash of `{}`",
                    name(rule)
                ));
            }
        }
        out
    }
    let mut out = keyed(
        "effect rule",
        &list.effect_rule_dict,
        |r: &PtxEffectRule<T>| &r.name,
    );
    out.extend(keyed(
        "emitter rule",
        &list.emitter_rule_dict,
        |r: &PtxEmitterRule| &r.name,
    ));
    out.extend(keyed(
        "particle rule",
        &list.particle_rule_dict,
        |r: &PtxParticleRule<T>| &r.name,
    ));
    out
}

/// The document of `list`'s rules.
pub fn write<T: DrawableTarget>(list: &PtxFxList<T>) -> Value {
    let mut document = Map::new();
    document.insert("name".into(), Value::String(list.name.clone()));
    document.insert(
        "effect_rules".into(),
        Value::Array(
            or_particle::list::effect_rules(list)
                .map(Json::write)
                .collect(),
        ),
    );
    document.insert(
        "emitter_rules".into(),
        Value::Array(
            or_particle::list::emitter_rules(list)
                .map(Json::write)
                .collect(),
        ),
    );
    document.insert(
        "particle_rules".into(),
        Value::Array(
            or_particle::list::particle_rules(list)
                .map(Json::write)
                .collect(),
        ),
    );
    Value::Object(document)
}

/// The rules of the list a document states, laid out as the editor lays them out; its textures and models are for the caller to add.
pub fn read<T: DrawableTarget>(document: &Value) -> Result<PtxFxList<T>, Fault> {
    let mut at = Trail::default();
    let o = at.object(document)?;
    at.only(o, &MEMBERS)?;
    let mut list = PtxFxList::<T>::default();
    if let Some(name) = o.get("name") {
        at.field("name", |at| list.name.read(name, at))?;
    }
    list.effect_rule_dict = rules("effect_rules", o, &mut at, |r: &PtxEffectRule<T>| &r.name)?;
    list.emitter_rule_dict = rules("emitter_rules", o, &mut at, |r: &PtxEmitterRule| &r.name)?;
    list.particle_rule_dict = rules("particle_rules", o, &mut at, |r: &PtxParticleRule<T>| {
        &r.name
    })?;
    ask_for_the_maps(&mut list);
    or_particle::list::canonical(&mut list);
    Ok(list)
}

/// Marks the map of every evolution list that has values as one to rebuild.
///
/// ⚠ The layout keeps a map of a list for as long as the values it names are, which an empty map of a list built from a document
/// does not name, so the list is given a map that names nothing and the layout writes it anew.
fn ask_for_the_maps<T: DrawableTarget>(list: &mut PtxFxList<T>) {
    let stale = |l: &mut PtxEvolutionList| {
        if !l.evolved_keyframe_props.is_empty() {
            l.evolved_keyframe_prop_map.data = vec![AtBinaryMapDataPair {
                key: 0,
                data: Ref::To("stale".into()),
                ..Default::default()
            }];
        }
    };
    for rule in or_particle::list::effect_rules_mut(list) {
        if let Some(l) = rule.evolution_list.as_deref_mut() {
            stale(l);
        }
        for event in or_particle::list::events_mut(rule) {
            if let Some(l) = event.base.evolution_list.as_deref_mut() {
                stale(l);
            }
        }
    }
}

/// The dictionary of the rules the member `key` of the document lists, none where it lists none.
fn rules<R: Owned + Json>(
    key: &str,
    o: &Map<String, Value>,
    at: &mut Trail,
    name: fn(&R) -> &str,
) -> Result<Option<Box<PgDictionary<R>>>, Fault>
where
    or_io::decl::Raw<R>: or_io::decl::Record,
    PgDictionary<R>: Default,
{
    let Some(listed) = o.get(key) else {
        return Ok(None);
    };
    let mut held: Vec<R> = Vec::new();
    at.field(key, |at| held.read(listed, at))?;
    if held.is_empty() {
        return Ok(None);
    }
    let mut seen = BTreeSet::new();
    let mut codes = Vec::with_capacity(held.len());
    let mut entries = Vec::with_capacity(held.len());
    for (i, rule) in held.into_iter().enumerate() {
        let code = string_hash_str(name(&rule));
        if !seen.insert(code) {
            return Err(at.field(key, |at| {
                at.index(i, |at| {
                    at.fault(format!(
                        "is named `{}`, and another rule of the list has a name with the same hash",
                        name(&rule)
                    ))
                })
            }));
        }
        codes.push(code);
        entries.push(Some(Box::new(rule)));
    }
    Ok(Some(Box::new(PgDictionary {
        codes,
        entries,
        ..Default::default()
    })))
}
