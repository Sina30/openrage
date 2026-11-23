//! A `.ypt` built as an owned value with invented content, for each target:
//! a particle rule with one behaviour its update list names again and one
//! shader variable, an effect rule whose emitter event names that rule by key
//! and owns an emitter rule, and the three dictionaries holding them. Saved,
//! reopened equal, edited and saved to a fixed point; the typed open names the
//! target a file holds. Needs no game file.

use std::path::{Path, PathBuf};

use or_io::decl::{File, Gen8, Gen9, HasOrigin, Hash32, Owned, OwnedOrKeyed, Record, Ref};
use or_io::gta5::drawable::DrawableTarget;
use or_io::gta5::particle::*;
use or_io::gta5::PgDictionary;
use or_io::{Asset, Error};

fn dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("gta5_particle");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn key(name: &str) -> u32 {
    Hash32::of(name).get()
}

fn dictionary<T: Owned<Raw: Record>>(name: &str, entry: T) -> Option<Box<PgDictionary<T>>>
where
    PgDictionary<T>: Default,
{
    Some(Box::new(PgDictionary {
        codes: vec![key(name)],
        entries: vec![Some(Box::new(entry))],
        ..Default::default()
    }))
}

fn particle_rule<T: DrawableTarget>() -> PtxParticleRule<T> {
    let age = PtxBehaviourAny::PtxuAge(PtxuAge {
        base: PtxBehaviour {
            hash_name: BEHAVIOUR_HASH_AGE,
            ..Default::default()
        },
        ..Default::default()
    });
    let tint = PtxShaderVarAny::PtxShaderVarVector(PtxShaderVarVector {
        base: PtxShaderVar {
            hash_name: Hash32::of("tint"),
            r#type: 5,
            ..Default::default()
        },
        ..Default::default()
    });
    PtxParticleRule {
        name: "fire".into(),
        all_behaviours: vec![Ref::Own(Box::new(age))],
        update_behaviours: vec![Ref::To(
            "particle_rule_dict.entries[0].all_behaviours[0]".into(),
        )],
        shader_inst: PtxShaderInst {
            shader_template_name: "ptfx_sprite".into(),
            inst_vars: vec![Some(Box::new(tint))],
            ..Default::default()
        },
        ..Default::default()
    }
}

/// ⚠ The event is the first pointer a file reaches at its emitter rule, so it
/// owns one; the dictionary's entry is a rule of its own that equals it.
fn effects<T: DrawableTarget>() -> PtxFxList<T> {
    let emitter = PtxEmitterRule {
        name: "fire_emit".into(),
        ..Default::default()
    };
    let event = PtxEventEmitter {
        emitter_rule_name: "fire_emit".into(),
        particle_rule_name: "fire".into(),
        emitter_rule: OwnedOrKeyed::Own(Some(Box::new(emitter.clone()))),
        particle_rule: OwnedOrKeyed::Key(key("fire")),
        playback_rate_scalar_max: 1.0,
        ..Default::default()
    };
    let effect = PtxEffectRule {
        name: "fire_fx".into(),
        timeline: PtxTimeLine {
            events: vec![Some(Box::new(event))],
            ..Default::default()
        },
        num_loops: -1,
        ..Default::default()
    };
    PtxFxList {
        name: "fire".into(),
        particle_rule_dict: dictionary("fire", particle_rule()),
        effect_rule_dict: dictionary("fire_fx", effect),
        emitter_rule_dict: dictionary("fire_emit", emitter),
        ..Default::default()
    }
}

/// `value` saved to `path`, reopened as the same type and equal to it, and saved again to the same bytes.
fn round_trip<D>(value: &D, path: &Path) -> D
where
    D: File + PartialEq + std::fmt::Debug,
{
    value.save_as(path).unwrap();
    let reopened = D::open(path).unwrap();
    assert_eq!(&reopened, value);
    let first = std::fs::read(path).unwrap();
    reopened.save().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), first, "a fixed point");
    reopened
}

fn rule(list: &PtxFxList<impl DrawableTarget>) -> &PtxParticleRule<impl DrawableTarget> {
    list.particle_rule_dict.as_ref().unwrap().entries[0]
        .as_ref()
        .unwrap()
}

#[test]
fn a_legacy_particle_list_built_owned_opens_saves_and_edits() {
    let path = dir().join("fire.ypt");
    round_trip(&effects::<Gen8>(), &path);

    let mut list = PtxFxList::open(&path).unwrap();
    let fire = list.particle_rule_dict.as_mut().unwrap().entries[0]
        .as_mut()
        .unwrap();
    assert_eq!(fire.name, "fire");
    assert_eq!(
        fire.update_behaviours[0],
        Ref::To("particle_rule_dict.entries[0].all_behaviours[0]".into())
    );
    fire.name = "fire_big".into();
    list.save().unwrap();

    let again = PtxFxList::open(&path).unwrap();
    assert_eq!(rule(&again).name, "fire_big");
    round_trip(&again, &path);
}

#[test]
fn an_enhanced_particle_list_opens_as_its_target() {
    let path = dir().join("fire_enhanced.ypt");
    round_trip(&effects::<Gen9>(), &path);

    let why = PtxFxList::open(&path).unwrap_err();
    assert!(
        matches!(
            why,
            Error::WrongTarget {
                found: "Enhanced",
                ..
            }
        ),
        "{why:?}"
    );
    match or_io::open(&path).unwrap() {
        Asset::ParticlesGen9(list) => assert_eq!(rule(&list).name, "fire"),
        other => panic!("{:?} opened as another asset", other.origin().path()),
    }
}

#[test]
fn a_legacy_particle_list_opens_through_or_io_open() {
    let path = dir().join("asset.ypt");
    effects::<Gen8>().save_as(&path).unwrap();
    match or_io::open(&path).unwrap() {
        Asset::Particles(list) => assert_eq!(list.name, "fire"),
        other => panic!("{:?} opened as another asset", other.origin().path()),
    }
}
