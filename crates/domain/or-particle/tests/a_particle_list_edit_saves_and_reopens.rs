//! A corpus `.ypt` laid out as the editor edits it, edited, saved through the
//! owned list and reopened: every event points at its rules' dictionary
//! entries, and each edit reads back.

use or_io::decl::{File, OwnedOrKeyed, Resource, View};
use or_io::gta5::particle::{PtxEventEmitter, PtxFxList};
use or_particle::behaviour::{AgeBehaviour, Behaviour};
use or_particle::list;
use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};

/// Every Legacy corpus `.ypt`, laid out as the editor edits it.
fn corpus(what: &str) -> Option<Vec<PtxFxList>> {
    let files = Corpus::with_root(shared_corpus_root(env!("CARGO_MANIFEST_DIR")), "ypt")
        .target(TARGET_GEN8)
        .find_or_skip(what)?;
    Some(
        files
            .iter()
            .map(|path| {
                let mut list = PtxFxList::open(path).expect("the corpus .ypt opens owned");
                list::canonical(&mut list);
                list
            })
            .collect(),
    )
}

/// The smallest of them.
fn smallest(what: &str) -> Option<PtxFxList> {
    corpus(what)?.into_iter().min_by_key(|l| saved(l).len())
}

fn saved(list: &PtxFxList) -> Vec<u8> {
    let mut list = list.clone();
    list::canonical(&mut list);
    list.to_bytes().expect("the list saves")
}

/// `list` saved and opened again, each event's emitter rule checked to be its dictionary's entry.
fn reopened(list: &PtxFxList) -> PtxFxList {
    let bytes = saved(list);
    assert_eq!(
        unaliased(&bytes),
        0,
        "an event holds an emitter rule of its own"
    );
    let again = PtxFxList::from_bytes(&bytes).expect("the saved list opens");
    for event in list::effect_rules(&again).flat_map(list::events) {
        let named = |held: bool, slot: bool| !held || slot;
        assert!(
            named(
                list::emitter_rule(&again, &event.emitter_rule_name).is_some(),
                !matches!(event.emitter_rule, OwnedOrKeyed::Own(None))
            ) && named(
                list::particle_rule(&again, &event.particle_rule_name).is_some(),
                !matches!(event.particle_rule, OwnedOrKeyed::Own(None))
            ),
            "an event names a rule its list holds and points at none"
        );
    }
    again
}

/// How many events of `bytes` point at an emitter rule its dictionary does not.
fn unaliased(bytes: &[u8]) -> usize {
    let peek = <PtxFxList as Resource>::peek(bytes).expect("the saved list peeks");
    let list = peek.root().expect("its root reads");
    let held: Vec<usize> = match list.emitter_rule_dict().expect("the pointer resolves") {
        Some(dict) => {
            let entries = dict.entries().expect("the run resolves");
            (0..entries.as_slice().len())
                .filter_map(|i| entries.follow(i).expect("an entry resolves"))
                .map(|e| View::offset(&e))
                .collect()
        }
        None => Vec::new(),
    };
    let Some(effects) = list.effect_rule_dict().expect("the pointer resolves") else {
        return 0;
    };
    let effects = effects.entries().expect("the run resolves");
    let mut apart = 0;
    for i in 0..effects.as_slice().len() {
        let Some(effect) = effects.follow(i).expect("an entry resolves") else {
            continue;
        };
        let events = effect.timeline().events().expect("the run resolves");
        for j in 0..events.as_slice().len() {
            let Some(event) = events.follow(j).expect("an event resolves") else {
                continue;
            };
            if let Some(rule) = event.emitter_rule().expect("the pointer resolves") {
                apart += usize::from(!held.contains(&View::offset(&rule)));
            }
        }
    }
    apart
}

/// Every rule a list holds, as the editor reads it.
fn rules(list: &PtxFxList) -> Vec<String> {
    let mut out: Vec<String> = list::particle_rules(list)
        .map(|r| format!("{} {:?}", r.name, list::behaviours(r)))
        .chain(list::emitter_rules(list).map(|r| r.name.clone()))
        .chain(list::effect_rules(list).map(|r| {
            let events: Vec<_> = list::events(r)
                .map(|e| (e.emitter_rule_name.clone(), e.particle_rule_name.clone()))
                .collect();
            format!("{} {events:?} {:?}", r.name, list::evolution_lists(r))
        }))
        .collect();
    out.sort();
    out
}

#[test]
fn every_corpus_particle_list_saves_each_rule_once_and_reads_back() {
    let what = "every_corpus_particle_list_saves_each_rule_once_and_reads_back";
    let Some(lists) = corpus(what) else {
        return;
    };
    for list in &lists {
        let again = reopened(list);
        assert_eq!(rules(&again), rules(list), "{}", list.name);
    }
}

#[test]
fn a_keyframe_edit_reads_back() {
    let what = "a_keyframe_edit_reads_back";
    let Some(mut list) = smallest(what) else {
        return;
    };
    let rule = list::emitter_rules_mut(&mut list)
        .find(|r| !r.particle_life_kfp.keyframe.entries.is_empty())
        .expect("an emitter rule keys its particle life");
    let name = rule.name.clone();
    rule.particle_life_kfp.keyframe.entries[0].value.x += 0.25;
    let edited = rule.particle_life_kfp.keyframe.entries[0].value.x;

    let again = reopened(&list);
    let rule = list::emitter_rule(&again, &name).expect("the rule is still there");
    assert_eq!(rule.particle_life_kfp.keyframe.entries[0].value.x, edited);
}

#[test]
fn a_renamed_rule_is_found_by_its_new_name_from_every_event() {
    let what = "a_renamed_rule_is_found_by_its_new_name_from_every_event";
    let Some(mut list) = smallest(what) else {
        return;
    };
    let name = list::particle_rules(&list)
        .map(|r| r.name.clone())
        .find(|n| list::particle_references(&list, n) > 0)
        .expect("an event names a particle rule");
    let uses = list::particle_references(&list, &name);
    assert!(list::rename_particle_rule(
        &mut list,
        &name,
        "renamed_by_the_editor"
    ));

    let again = reopened(&list);
    assert_eq!(
        list::particle_references(&again, "renamed_by_the_editor"),
        uses
    );
    assert!(list::particle_rule(&again, "renamed_by_the_editor").is_some());
}

#[test]
fn an_added_behaviour_is_listed_where_its_class_runs() {
    let what = "an_added_behaviour_is_listed_where_its_class_runs";
    let Some(mut list) = smallest(what) else {
        return;
    };
    let rule = list::particle_rules_mut(&mut list)
        .next()
        .expect("a particle rule");
    let name = rule.name.clone();
    let mut behaviours = list::behaviours(rule);
    behaviours.push(Behaviour::Age(AgeBehaviour::default()));
    list::set_behaviours(rule, &behaviours);

    let again = reopened(&list);
    let rule = list::particle_rule(&again, &name).expect("the rule reads");
    assert_eq!(rule.all_behaviours.len(), behaviours.len());
    assert!(matches!(
        list::behaviours(rule).last(),
        Some(Behaviour::Age(_))
    ));
    assert!(
        !rule.init_behaviours.is_empty() && !rule.update_behaviours.is_empty(),
        "an age behaviour runs at init and at update"
    );
}

#[test]
fn rules_the_editor_adds_and_binds_save_and_reopen() {
    let what = "rules_the_editor_adds_and_binds_save_and_reopen";
    let Some(mut list) = smallest(what) else {
        return;
    };
    assert!(list::add_effect(&mut list, "editor_effect"));
    assert!(list::add_emitter_rule(&mut list, "editor_emitter"));
    assert!(list::add_particle_rule(&mut list, "editor_particle"));
    let effect = list::effect_rules_mut(&mut list)
        .find(|r| r.name == "editor_effect")
        .expect("the added effect");
    effect.timeline.events.push(Some(Box::new(PtxEventEmitter {
        emitter_rule_name: "editor_emitter".into(),
        particle_rule_name: "editor_particle".into(),
        ..Default::default()
    })));

    let again = reopened(&list);
    assert!(list::emitter_rule(&again, "editor_emitter").is_some());
    assert_eq!(list::particle_references(&again, "editor_particle"), 1);
    let effect = list::effect_rule(&again, "editor_effect").expect("the added effect reads");
    assert!(
        !matches!(
            list::events(effect).next().map(|e| &e.emitter_rule),
            None | Some(OwnedOrKeyed::Own(None))
        ),
        "the added event's emitter rule is not linked"
    );
}

#[test]
fn a_deleted_rule_leaves_its_dictionary() {
    let what = "a_deleted_rule_leaves_its_dictionary";
    let Some(mut list) = smallest(what) else {
        return;
    };
    let name = list::particle_rules(&list)
        .next()
        .expect("a particle rule")
        .name
        .clone();
    let count = list::particle_rules(&list).count();
    assert!(list::remove_particle_rule(&mut list, &name));

    let again = reopened(&list);
    assert_eq!(list::particle_rules(&again).count(), count - 1);
    assert!(list::particle_rule(&again, &name).is_none());
}
