//! A `.ypt` becomes a particles source and compiles back to a list that reads the same.
//!
//! ⚠ The lists are made here from the editor's own calls with small made-up names; the install's files are held to the same by
//! the suite of `or-particles`, which skips without an install.

use std::path::Path;

use or_io::decl::{File, Gen8};
use or_io::gta5::gen8::GrcTexture;
use or_io::gta5::particle::{PtxEventEmitter, PtxFxList};
use or_io::gta5::PgDictionary;
use or_particle::behaviour::{AccelerationBehaviour, Behaviour, ColourBehaviour};
use or_source::asset::Source;
use or_source::game_import::{decompile, Facts};
use or_source::particles::{self, KIND, RULES};
use or_source::sample;
use or_source::textures::Extraction;

/// A list with an effect that fires one emitter and one particle rule, and one texture.
fn a_list() -> PtxFxList {
    let mut list = PtxFxList::<Gen8> {
        name: "fx_test".into(),
        ..Default::default()
    };
    assert!(or_particle::list::add_effect(&mut list, "fx_spark"));
    assert!(or_particle::list::add_emitter_rule(&mut list, "em_spark"));
    assert!(or_particle::list::add_particle_rule(&mut list, "pt_spark"));
    let effect = or_particle::list::effect_rules_mut(&mut list)
        .next()
        .expect("the effect");
    effect.num_loops = 3;
    effect.duration_max = 2.5;
    effect.timeline.events.push(Some(Box::new(PtxEventEmitter {
        emitter_rule_name: "em_spark".into(),
        particle_rule_name: "pt_spark".into(),
        ..Default::default()
    })));
    let particle = or_particle::list::particle_rules_mut(&mut list)
        .next()
        .expect("the particle rule");
    or_particle::list::set_behaviours(
        particle,
        &[
            Behaviour::Acceleration(AccelerationBehaviour::default()),
            Behaviour::Colour(ColourBehaviour::default()),
        ],
    );
    let dictionary =
        PgDictionary::<GrcTexture>::from_bytes(&sample::texture_dictionary("fx_smoke", 40))
            .expect("a sample dictionary reads");
    list.texture_dict = Some(Box::new(dictionary));
    or_particle::list::canonical(&mut list);
    list
}

fn extracted(bytes: &[u8], parent: &Path) -> std::path::PathBuf {
    particles::extract(
        bytes,
        "fx_test",
        parent,
        &Extraction {
            from_game: false,
            virtual_path: None,
        },
    )
    .unwrap_or_else(|problems| panic!("{problems:?}"))
}

#[test]
fn a_list_becomes_a_source_of_rules_and_dds_files_and_compiles_back_to_the_same_list() {
    let list = a_list();
    let bytes = list.to_bytes().expect("the list is valid");
    let dir = tempfile::tempdir().unwrap();
    let folder = extracted(&bytes, dir.path());

    assert!(folder.ends_with("fx_test.particles.src"));
    for file in [
        "asset.json",
        RULES,
        "textures.json",
        "textures/fx_smoke.dds",
    ] {
        assert!(folder.join(file).is_file(), "{file}");
    }
    assert!(
        !folder.join("models.ypt").exists(),
        "a list that draws no model has no models"
    );
    let rules = std::fs::read_to_string(folder.join(RULES)).unwrap();
    assert!(rules.contains("\"num_loops\": 3"), "{rules}");
    assert!(rules.contains("ptxu_Acceleration"), "{rules}");

    let source = Source::open(&folder).unwrap();
    assert_eq!(source.manifest.kind, KIND);
    let compiled = particles::compile(&source).unwrap_or_else(|p| panic!("{p:?}"));
    assert_eq!(compiled.file_name, "fx_test.ypt");
    let mut back = PtxFxList::<Gen8>::from_bytes(&compiled.bytes).unwrap();
    or_particle::list::canonical(&mut back);
    use or_particle::list::{effect_rules, emitter_rules, particle_rules};
    assert!(effect_rules(&back).eq(effect_rules(&list)));
    assert!(emitter_rules(&back).eq(emitter_rules(&list)));
    assert!(particle_rules(&back).eq(particle_rules(&list)));
    let named = |l: &PtxFxList| -> Vec<(String, u16, u16)> {
        l.texture_dict
            .iter()
            .flat_map(|d| d.entries.iter().flatten())
            .map(|t| (t.name.clone(), t.width, t.height))
            .collect()
    };
    assert_eq!(named(&back), named(&list));
    assert_eq!(named(&list).len(), 1);
}

#[test]
fn compiling_an_unchanged_source_twice_gives_the_same_bytes_and_a_second_import_gives_the_same_source(
) {
    let bytes = a_list().to_bytes().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let first = extracted(&bytes, dir.path());
    let compiled = particles::compile(&Source::open(&first).unwrap()).unwrap();
    assert_eq!(
        particles::compile(&Source::open(&first).unwrap()).unwrap(),
        compiled
    );

    let again = tempfile::tempdir().unwrap();
    let second = extracted(&compiled.bytes, again.path());
    for file in [RULES, "textures.json", "textures/fx_smoke.dds"] {
        assert_eq!(
            std::fs::read(first.join(file)).unwrap(),
            std::fs::read(second.join(file)).unwrap(),
            "{file} is not a fixed point of import and compile"
        );
    }
}

#[test]
fn an_edit_of_the_rules_changes_that_rule_of_the_compiled_list_and_no_other() {
    let list = a_list();
    let dir = tempfile::tempdir().unwrap();
    let folder = extracted(&list.to_bytes().unwrap(), dir.path());
    let text = std::fs::read_to_string(folder.join(RULES)).unwrap();
    std::fs::write(
        folder.join(RULES),
        text.replace("\"num_loops\": 3", "\"num_loops\": 7"),
    )
    .unwrap();

    let compiled = particles::compile(&Source::open(&folder).unwrap()).unwrap();
    let back = PtxFxList::<Gen8>::from_bytes(&compiled.bytes).unwrap();
    let effect = or_particle::list::effect_rule(&back, "fx_spark").unwrap();
    assert_eq!(effect.num_loops, 7);
    assert_eq!(effect.duration_max, 2.5);
    assert_eq!(
        or_particle::list::particle_rules(&back).count(),
        1,
        "the other rules are as they were"
    );
}

#[test]
fn a_member_the_text_does_not_know_a_wrong_value_and_a_class_with_no_layout_are_refused_with_their_place(
) {
    let dir = tempfile::tempdir().unwrap();
    let folder = extracted(&a_list().to_bytes().unwrap(), dir.path());
    let original = std::fs::read_to_string(folder.join(RULES)).unwrap();
    for (from, to, says) in [
        (
            "\"num_loops\": 3",
            "\"num_loops\": 3, \"loops\": 1",
            "has no member `loops`",
        ),
        (
            "\"num_loops\": 3",
            "\"num_loops\": \"three\"",
            "effect_rules[0].num_loops is not a whole number",
        ),
        ("\"ptxu_Acceleration\"", "\"ptxu_Nothing\"", "has no layout"),
    ] {
        std::fs::write(folder.join(RULES), original.replace(from, to)).unwrap();
        let problems = particles::compile(&Source::open(&folder).unwrap()).unwrap_err();
        let said = problems
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        assert!(said.contains(says), "{to}: {said}");
        assert!(said.contains(RULES), "{said}");
    }
}

#[test]
fn the_door_decompiles_a_ypt_to_a_particles_source() {
    let bytes = a_list().to_bytes().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let made = decompile(
        &bytes,
        "fx_test.ypt",
        dir.path(),
        &Facts {
            from_game: true,
            virtual_path: Some("x64/ptfx.rpf/fx_test.ypt".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let source = Source::open(&made).unwrap();
    assert_eq!(source.manifest.kind, KIND);
    assert!(source.manifest.origin.from_game);
    assert!(
        source.manifest.origin.cooked_sha256.is_some(),
        "an import of the game's file records what its cook made"
    );
    // The recook of a saved file inside it makes the `.ypt` in the shadow.
    let shadow = tempfile::tempdir().unwrap();
    let cooked = or_source::cook::cook_source(&made, shadow.path()).unwrap();
    assert!(cooked.ends_with("fx_test.ypt"));
}
