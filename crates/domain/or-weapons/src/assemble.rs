//! The assembled weapon: which parts a weapon wears and at which bone, as a pure query over what the pack states.
//!
//! The weapon's own metadata lists its attachment points, each a bone with the components that attach there; the pack's components
//! metadata says what each component is and which model it is drawn with; the pack's model sources are found by that name. Nothing here
//! reads a model or draws one, so it is tested without a renderer, and the preview draws what it answers.

use std::collections::BTreeMap;

use crate::facts::Facts;
use crate::model::Weapon;
use crate::rules::{defined_by, stem};

/// A part a weapon can wear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// The name of the component, as the weapon's metadata attaches it.
    pub component: String,
    /// The bone of the weapon it attaches at.
    pub bone: String,
    /// Whether the weapon wears it unless a choice says otherwise.
    pub default: bool,
    /// The engine's name of the kind of component it is (`CWeaponComponentClipInfo`), none when no metadata of the pack defines it.
    pub kind: Option<&'static str>,
    /// The name of the model the component's metadata gives it, empty for a component with none.
    pub model_name: String,
    /// The source of the pack that is that model, none when the pack holds none by that name.
    pub model: Option<String>,
}

/// The weapon and the parts it can wear.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Assembly {
    /// The source of the weapon's own model, found by the model its metadata gives it, else the only model it lists.
    pub base: Option<String>,
    pub parts: Vec<Part>,
}

/// The model sources a weapon lists, with the name each cooks under in lower case.
fn models<'a>(weapon: &'a Weapon, facts: &'a Facts) -> Vec<(&'a str, String)> {
    weapon
        .sources
        .iter()
        .filter_map(|path| {
            let source = facts.source(path)?;
            source
                .is_model()
                .then(|| (path.as_str(), stem(&source.name)))
        })
        .collect()
}

/// The parts `weapon` can wear and its own model, from what the pack's metadata says.
pub fn assemble(weapon: &Weapon, facts: &Facts) -> Assembly {
    let models = models(weapon, facts);
    let find = |name: &str| {
        models
            .iter()
            .find(|(_, stem)| *stem == name.to_ascii_lowercase())
            .map(|(path, _)| path.to_string())
    };
    let Some(defined) = defined_by(weapon, facts) else {
        return Assembly {
            base: (models.len() == 1).then(|| models[0].0.to_string()),
            parts: Vec::new(),
        };
    };
    let base =
        find(&defined.model).or_else(|| (models.len() == 1).then(|| models[0].0.to_string()));
    let components: BTreeMap<String, &crate::metadata::DefinedComponent> = weapon
        .sources
        .iter()
        .filter_map(|path| facts.source(path)?.defined.as_ref()?.as_ref().ok())
        .flat_map(|defined| &defined.components)
        .map(|c| (c.name.to_ascii_lowercase(), c))
        .collect();
    let mut parts = Vec::new();
    for point in &defined.attach_points {
        for (name, default) in &point.components {
            let component = components.get(&name.to_ascii_lowercase());
            let model_name = component.map(|c| c.model.clone()).unwrap_or_default();
            parts.push(Part {
                component: name.clone(),
                bone: point.bone.clone(),
                default: *default,
                kind: component.map(|c| c.kind),
                model: (!model_name.is_empty())
                    .then(|| find(&model_name))
                    .flatten(),
                model_name,
            });
        }
    }
    Assembly { base, parts }
}

/// What a person picked for each bone: the component to wear there, `None` for nothing.
pub type Choice = BTreeMap<String, Option<String>>;

/// The parts worn under `choice`: at each bone the one picked, else the one the weapon wears by default.
pub fn worn<'a>(assembly: &'a Assembly, choice: &Choice) -> Vec<&'a Part> {
    let mut bones: Vec<&str> = Vec::new();
    for part in &assembly.parts {
        if !bones.contains(&part.bone.as_str()) {
            bones.push(&part.bone);
        }
    }
    bones
        .into_iter()
        .filter_map(|bone| {
            let at = || assembly.parts.iter().filter(move |p| p.bone == bone);
            match choice.get(bone) {
                Some(Some(picked)) => at().find(|p| p.component.eq_ignore_ascii_case(picked)),
                Some(None) => None,
                None => at().find(|p| p.default),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{Fact, SourceFact};
    use crate::metadata::{AttachPoint, Defined, DefinedComponent, DefinedWeapon};

    fn source(kind: &str, name: &str, ext: &str, defined: Option<Defined>) -> Fact {
        Fact::Source(Box::new(SourceFact {
            kind: kind.into(),
            name: name.into(),
            extension: ext.into(),
            bytes: 1,
            game: Vec::new(),
            digest: None,
            defined: defined.map(Ok),
        }))
    }

    fn pack() -> (Weapon, Facts) {
        let weapons = Defined {
            root: "CWeaponInfoBlob".into(),
            weapons: vec![DefinedWeapon {
                name: "WEAPON_X".into(),
                model: "w_pi_x".into(),
                audio: String::new(),
                slot: String::new(),
                group: String::new(),
                attach_points: vec![
                    AttachPoint {
                        bone: "WAPClip".into(),
                        components: vec![("CLIP_01".into(), true), ("CLIP_02".into(), false)],
                    },
                    AttachPoint {
                        bone: "WAPFlsh".into(),
                        components: vec![("LIGHT".into(), false)],
                    },
                ],
            }],
            ..Defined::default()
        };
        let component = |name: &str, model: &str, kind| DefinedComponent {
            name: name.into(),
            model: model.into(),
            attach_bone: String::new(),
            kind,
        };
        let components = Defined {
            root: "CWeaponComponentInfoBlob".into(),
            components: vec![
                component("CLIP_01", "w_pi_x_mag1", "CWeaponComponentClipInfo"),
                component("CLIP_02", "w_pi_x_mag2", "CWeaponComponentClipInfo"),
            ],
            ..Defined::default()
        };
        let mut facts = Facts::default();
        facts.insert(
            "w_pi_x.drawable.src",
            source("drawable", "w_pi_x", "ydr", None),
        );
        facts.insert(
            "w_pi_x_mag1.drawable.src",
            source("drawable", "W_PI_X_MAG1", "ydr", None),
        );
        facts.insert(
            "x.metadata.src",
            source("metadata", "x", "meta", Some(weapons)),
        );
        facts.insert(
            "c.metadata.src",
            source("metadata", "c", "meta", Some(components)),
        );
        let mut weapon = Weapon::new("weapon_x");
        weapon.sources = [
            "w_pi_x.drawable.src",
            "w_pi_x_mag1.drawable.src",
            "x.metadata.src",
            "c.metadata.src",
        ]
        .map(String::from)
        .to_vec();
        (weapon, facts)
    }

    #[test]
    fn a_weapon_wears_the_default_of_each_bone_and_a_pick_replaces_it() {
        let (weapon, facts) = pack();
        let assembly = assemble(&weapon, &facts);
        assert_eq!(assembly.base.as_deref(), Some("w_pi_x.drawable.src"));
        let clip = &assembly.parts[0];
        assert_eq!(
            (clip.component.as_str(), clip.bone.as_str()),
            ("CLIP_01", "WAPClip")
        );
        assert_eq!(clip.kind, Some("CWeaponComponentClipInfo"));
        // The model is found by the name the component's metadata gives it, whatever its case.
        assert_eq!(clip.model.as_deref(), Some("w_pi_x_mag1.drawable.src"));
        // The second clip's model is not in the pack, and a component nothing defines has no kind.
        assert_eq!(assembly.parts[1].model, None);
        assert_eq!(assembly.parts[1].model_name, "w_pi_x_mag2");
        assert_eq!(assembly.parts[2].kind, None);

        let names = |choice: &Choice| -> Vec<String> {
            worn(&assembly, choice)
                .into_iter()
                .map(|p| p.component.clone())
                .collect()
        };
        assert_eq!(names(&Choice::new()), ["CLIP_01"]);
        let mut choice = Choice::new();
        choice.insert("WAPClip".into(), Some("clip_02".into()));
        choice.insert("WAPFlsh".into(), Some("LIGHT".into()));
        assert_eq!(names(&choice), ["CLIP_02", "LIGHT"]);
        choice.insert("WAPClip".into(), None);
        assert_eq!(names(&choice), ["LIGHT"]);
    }

    #[test]
    fn a_weapon_nothing_defines_wears_nothing_and_its_only_model_is_its_own() {
        let (mut weapon, facts) = pack();
        weapon.name = "WEAPON_ELSE".into();
        weapon.sources = vec!["w_pi_x.drawable.src".into()];
        let assembly = assemble(&weapon, &facts);
        assert_eq!(assembly.base.as_deref(), Some("w_pi_x.drawable.src"));
        assert!(assembly.parts.is_empty());
    }
}
