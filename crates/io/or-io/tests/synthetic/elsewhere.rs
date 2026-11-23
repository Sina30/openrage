//! Pointers a file aims at what a pointer elsewhere in the tree owns, on
//! invented owned records: a record a non-ancestor owns, an element of a
//! sibling run, an element of a run of pointers, and a string.

// A counted array of pointers is spelled `Vec<Box<T>>`: the box is the pointer.
#![allow(clippy::vec_box)]

use std::sync::Arc;

use or_io::constants::FIXED_VIRTUAL_BASE;
use or_io::decl::{self, Aligned, Cx, Doc, Owned, Raw, Record, Ref, View};
use or_io::desc::{self, Pack, Value};

#[decl::record(size = 0x10)]
pub struct Rig {
    pub name: String,
    pub bones: u32,
    pub flags: u32,
}

#[decl::record(size = 0x10)]
pub struct Body {
    pub rig: Ref<Option<Box<Rig>>>,
    #[pad_after(4)]
    pub mass: f32,
}

/// Parts whose rig is the main body's, as a fragment's child drawables' skeleton is the common drawable's.
#[decl::record(size = 0x18)]
pub struct Wreck {
    pub main: Box<Body>,
    pub parts: Vec<Box<Body>>,
}

#[decl::record(size = 0x18)]
pub struct Curve {
    pub keys: Vec<u32>,
    pub hash: u32,
    pub blend: u32,
}

#[decl::record(size = 0x10)]
pub struct CurveEntry {
    #[pad_after(4)]
    pub hash: u32,
    pub item: Ref<Box<Curve>>,
}

/// A map whose entries point into the sibling run of curves, as an evolution list's is.
#[decl::record(size = 0x20)]
pub struct Evolutions {
    pub curves: Vec<Curve>,
    pub map: Vec<CurveEntry>,
}

#[decl::record(size = 0x08)]
pub struct Step {
    pub kind: u32,
    pub weight: f32,
}

/// Lists of steps that `all` owns, as a particle rule's behaviour lists are.
#[decl::record(size = 0x30)]
pub struct Rule {
    pub all: Vec<Box<Step>>,
    pub init: Vec<Ref<Box<Step>>>,
    pub draw: Vec<Ref<Box<Step>>>,
}

#[decl::record(size = 0x10)]
pub struct Rules {
    pub rules: Vec<Box<Rule>>,
}

#[decl::record(size = 0x10)]
pub struct Archetype {
    pub filename: Ref<String>,
    #[pad_after(4)]
    pub mass: f32,
}

/// A damaged archetype whose file name is the undamaged one's string.
#[decl::record(size = 0x10)]
pub struct Physics {
    pub archetype: Box<Archetype>,
    pub damaged: Option<Box<Archetype>>,
}

fn pack() -> Arc<Pack> {
    let roots: &[fn(&mut decl::Reach)] = &[
        decl::root::<Raw<Wreck>>,
        decl::root::<Raw<Evolutions>>,
        decl::root::<Raw<Rules>>,
        decl::root::<Raw<Physics>>,
        decl::root::<Raw<Stage>>,
    ];
    Arc::new(decl::describe("synthetic", "gen8", roots).expect("describes"))
}

/// A saved image: its bytes, its virtual size and where its root is.
struct Saved {
    bytes: Aligned,
    virtual_size: usize,
    root: usize,
}

impl Saved {
    fn image(&self) -> desc::Image<'_> {
        desc::Image::new(self.bytes.bytes(), self.virtual_size)
    }

    /// The eight bytes at `address`.
    fn word(&self, address: u64) -> u64 {
        let at = (address - FIXED_VIRTUAL_BASE) as usize;
        u64::from_le_bytes(self.bytes.bytes()[at..at + 8].try_into().unwrap())
    }

    /// These bytes with the eight at `address` set to `word`.
    fn with_word(&self, address: u64, word: u64) -> Saved {
        let at = (address - FIXED_VIRTUAL_BASE) as usize;
        let mut bytes = self.bytes.bytes().to_vec();
        bytes[at..at + 8].copy_from_slice(&word.to_le_bytes());
        Saved {
            bytes: Aligned::from_bytes(&bytes),
            virtual_size: self.virtual_size,
            root: self.root,
        }
    }
}

fn build(pack: &Arc<Pack>, tree: &Value) -> Result<Saved, String> {
    let built = or_io::write::build(pack, tree).map_err(|e| e.to_string())?;
    let virtual_size = built.virtual_data.len();
    let mut bytes = built.virtual_data;
    bytes.extend_from_slice(&built.physical_data);
    Ok(Saved {
        bytes: Aligned::from_bytes(&bytes),
        virtual_size,
        root: (built.root - FIXED_VIRTUAL_BASE) as usize,
    })
}

fn tree<T: Owned>(value: &T) -> Value {
    value.to_element(&Cx::new(pack().plans().clone())).unwrap()
}

fn save<T: Owned>(value: &T) -> Result<Saved, String> {
    let pack = pack();
    let tree = value
        .to_element(&Cx::new(pack.plans().clone()))
        .map_err(|e| e.to_string())?;
    build(&pack, &tree)
}

fn open<T: Owned>(saved: &Saved) -> Result<T, String>
where
    Raw<T>: Record,
{
    let doc = Doc::<Raw<T>>::read(pack(), saved.image(), saved.root).map_err(|e| e.to_string())?;
    T::from_element(doc.value()).map_err(|e| e.to_string())
}

/// `saved` read as a tree and laid out again by the canonical writer.
fn tree_save<T: Owned>(saved: &Saved) -> Saved
where
    Raw<T>: Record,
{
    let doc = Doc::<Raw<T>>::read(pack(), saved.image(), saved.root).unwrap();
    build(&pack(), doc.value()).unwrap()
}

fn kids(tree: &mut Value) -> &mut desc::Record {
    match tree {
        Value::Struct(record) => record,
        other => panic!("a struct, not {other:?}"),
    }
}

/// What the live pointer `v` points at.
fn target(v: &mut Value) -> &mut Value {
    match v {
        Value::Ptr {
            target: Some(t), ..
        } => t,
        other => panic!("a live pointer, not {other:?}"),
    }
}

/// The elements of the run the live pointer `v` points at.
fn elements(v: &mut Value) -> &mut Vec<Value> {
    match target(v) {
        Value::Array { elements, .. } => elements,
        other => panic!("an array, not {other:?}"),
    }
}

/// The source the pointer `v` owns its target under, set to `address`.
fn own_at(v: &mut Value, address: u64) {
    match v {
        Value::Ptr {
            source,
            target: Some(_),
        } => *source = address,
        other => panic!("a live pointer, not {other:?}"),
    }
}

fn child(record: &mut Value, at: usize) -> &mut Value {
    kids(record).child_at_mut(at).unwrap()
}

/// `value` saved, opened and saved again: equal to itself, and the second save byte-identical.
fn round_trip<T: Owned + PartialEq + std::fmt::Debug>(value: &T) -> Saved
where
    Raw<T>: Record,
{
    let first = save(value).unwrap();
    let again: T = open(&first).unwrap();
    assert_eq!(&again, value);
    let second = save(&again).unwrap();
    assert_eq!(second.bytes.bytes(), first.bytes.bytes(), "a fixed point");
    first
}

/// `file` opened owned saves as its tree saves, and is a fixed point of its own save.
fn saves_as_tree<T: Owned + PartialEq + std::fmt::Debug>(file: &Saved) -> (T, Saved)
where
    Raw<T>: Record,
{
    let value: T = open(file).unwrap();
    assert_eq!(
        save(&value).unwrap().bytes.bytes(),
        tree_save::<T>(file).bytes.bytes(),
        "the owned save is the tree's"
    );
    let saved = round_trip(&value);
    (value, saved)
}

fn to(path: &str) -> Ref<Box<Step>> {
    Ref::To(path.into())
}

const SHARED: u64 = 0x5000_1000;

fn rig() -> Option<Box<Rig>> {
    Some(Box::new(Rig {
        name: "rig".into(),
        bones: 3,
        flags: 1,
        ..Default::default()
    }))
}

fn body(mass: f32, rig: Ref<Option<Box<Rig>>>) -> Box<Body> {
    Box::new(Body {
        rig,
        mass,
        ..Default::default()
    })
}

/// A file whose parts point their rig at the main body's.
fn wreck_file() -> Saved {
    let own = || Ref::Own(rig());
    let wreck = Wreck {
        main: body(1.0, own()),
        parts: vec![body(2.0, own()), body(3.0, own())],
        ..Default::default()
    };
    let mut t = tree(&wreck);
    let rig = std::mem::offset_of!(Raw<Body>, rig);
    let main = child(&mut t, std::mem::offset_of!(Raw<Wreck>, main));
    own_at(child(target(main), rig), SHARED);
    let parts = child(&mut t, std::mem::offset_of!(Raw<Wreck>, parts));
    for part in elements(parts) {
        *child(target(part), rig) = Value::Alias(SHARED);
    }
    build(&pack(), &t).unwrap()
}

/// Where a saved wreck's main rig is, and where each part's rig pointer points.
fn wreck_rigs(saved: &Saved) -> (u64, Vec<u64>) {
    let rig_of = |body: u64| {
        {
            View::<Raw<Body>>::at_address(saved.image(), body)
                .unwrap()
                .rig
        }
        .raw()
    };
    let w = View::<Raw<Wreck>>::at_offset(saved.image(), saved.root).unwrap();
    let parts = { w.parts };
    let part_rigs = (0..parts.count())
        .map(|i| rig_of(saved.word(parts.raw() + 8 * i as u64)))
        .collect();
    (rig_of({ w.main }.raw()), part_rigs)
}

#[test]
fn a_record_a_non_ancestor_owns_opens_as_a_reference_by_path() {
    let (wreck, saved) = saves_as_tree::<Wreck>(&wreck_file());
    assert_eq!(wreck.main.rig, Ref::Own(rig()));
    for part in &wreck.parts {
        assert_eq!(part.rig, Ref::To("main.rig".into()));
    }
    let (main, parts) = wreck_rigs(&saved);
    assert!(main != 0 && parts == [main, main], "{main:#x} {parts:x?}");
}

#[test]
fn a_reference_built_from_scratch_points_where_its_path_leads_or_is_refused() {
    let mut wreck = Wreck {
        main: body(1.0, Ref::Own(rig())),
        parts: vec![
            body(2.0, Ref::To("main.rig".into())),
            body(3.0, Ref::To("parts[0].rig".into())),
        ],
        ..Default::default()
    };
    let (main, parts) = wreck_rigs(&save(&wreck).unwrap());
    assert!(
        main != 0 && parts == [main, main],
        "a reference to a reference leads to their target"
    );

    wreck.parts[1].rig = Ref::Own(rig());
    let (main, parts) = wreck_rigs(&round_trip(&wreck));
    assert!(
        parts[0] == main && parts[1] != main,
        "retargeted to its own"
    );

    wreck.parts[0].rig = Ref::To("parts[7].rig".into());
    let Err(why) = save(&wreck) else {
        panic!("a path that leads nowhere is refused")
    };
    assert!(why.contains("parts[7].rig"), "{why}");

    wreck.parts[0].rig = Ref::To("main.rig".into());
    wreck.main.rig = Ref::Own(None);
    let Err(why) = save(&wreck) else {
        panic!("a reference whose target was removed is refused")
    };
    assert!(why.contains("main.rig"), "{why}");
}

#[test]
fn an_alias_no_pointer_owns_is_refused_at_open() {
    let wreck = Wreck {
        main: body(1.0, Ref::Own(rig())),
        parts: vec![body(2.0, Ref::Own(rig()))],
        ..Default::default()
    };
    let mut t = tree(&wreck);
    let parts = child(&mut t, std::mem::offset_of!(Raw<Wreck>, parts));
    *child(
        target(&mut elements(parts)[0]),
        std::mem::offset_of!(Raw<Body>, rig),
    ) = Value::Alias(0x5000_9990);
    let Err(why) = Wreck::from_element(&t) else {
        panic!("an alias of nothing is refused")
    };
    assert!(
        why.to_string().contains("no pointer in the tree owns"),
        "{why}"
    );
}

fn curve(hash: u32, keys: &[u32]) -> Curve {
    Curve {
        keys: keys.to_vec(),
        hash,
        blend: 2,
        ..Default::default()
    }
}

/// A file whose map points its first entry at the run of curves and its second into it, at the second curve.
fn evolutions_file() -> Saved {
    let curves = vec![curve(0x10, &[1, 2]), curve(0x20, &[3, 4, 5])];
    let evolutions = Evolutions {
        map: curves
            .iter()
            .map(|c| CurveEntry {
                hash: c.hash,
                item: Ref::Own(Box::new(c.clone())),
                ..Default::default()
            })
            .collect(),
        curves,
        ..Default::default()
    };
    let mut t = tree(&evolutions);
    own_at(
        child(&mut t, std::mem::offset_of!(Raw<Evolutions>, curves)),
        SHARED,
    );
    let map = child(&mut t, std::mem::offset_of!(Raw<Evolutions>, map));
    *child(
        &mut elements(map)[0],
        std::mem::offset_of!(Raw<CurveEntry>, item),
    ) = Value::Alias(SHARED);
    let built = build(&pack(), &t).unwrap();

    let (items, curves) = evolution_items(&built);
    built.with_word(
        items[1].0,
        curves + std::mem::size_of::<Raw<Curve>>() as u64,
    )
}

/// Where each of a saved map's item pointers sits and points, with the keys it finds there; and where the curves are.
fn evolution_items(saved: &Saved) -> (Vec<(u64, u64, u64)>, u64) {
    let e = View::<Raw<Evolutions>>::at_offset(saved.image(), saved.root).unwrap();
    let map = { e.map };
    let items = (0..map.count() as u64)
        .map(|i| {
            let slot = map.raw()
                + i * std::mem::size_of::<Raw<CurveEntry>>() as u64
                + std::mem::offset_of!(Raw<CurveEntry>, item) as u64;
            let item = saved.word(slot);
            let c = View::<Raw<Curve>>::at_address(saved.image(), item).unwrap();
            (slot, item, { c.keys }.raw())
        })
        .collect();
    (items, { e.curves }.raw())
}

#[test]
fn an_element_of_a_sibling_run_opens_as_a_reference_by_path() {
    let (evolutions, saved) = saves_as_tree::<Evolutions>(&evolutions_file());
    let items: Vec<_> = evolutions.map.iter().map(|m| &m.item).collect();
    assert_eq!(
        items,
        [&Ref::To("curves[0]".into()), &Ref::To("curves[1]".into())]
    );
    let (items, curves) = evolution_items(&saved);
    let second = View::<Raw<Curve>>::at_address(
        saved.image(),
        curves + std::mem::size_of::<Raw<Curve>>() as u64,
    )
    .unwrap();
    assert_eq!(items[0].1, curves, "the first points at the run");
    assert!(
        items[1].1 != curves && items[1].2 == { second.keys }.raw(),
        "the second at a copy of the second curve, whose keys are the curve's"
    );
}

fn step(kind: u32) -> Box<Step> {
    Box::new(Step {
        kind,
        weight: kind as f32 / 2.0,
        ..Default::default()
    })
}

/// Two rules alike, each listing its own steps again in `init` and `draw`.
fn rules_file() -> Saved {
    let rule = || {
        Box::new(Rule {
            all: vec![step(7), step(8), step(9)],
            init: vec![Ref::Own(step(7)), Ref::Own(step(9))],
            draw: vec![Ref::Own(step(8))],
            ..Default::default()
        })
    };
    let rules = Rules {
        rules: vec![rule(), rule()],
        ..Default::default()
    };
    let mut t = tree(&rules);
    let rules = child(&mut t, std::mem::offset_of!(Raw<Rules>, rules));
    for (r, rule) in elements(rules).iter_mut().enumerate() {
        let rule = target(rule);
        let at = |k: usize| SHARED + (r * 0x100 + k * 0x10) as u64;
        for (k, s) in elements(child(rule, std::mem::offset_of!(Raw<Rule>, all)))
            .iter_mut()
            .enumerate()
        {
            own_at(s, at(k));
        }
        let init = elements(child(rule, std::mem::offset_of!(Raw<Rule>, init)));
        init[0] = Value::Alias(at(0));
        init[1] = Value::Alias(at(2));
        elements(child(rule, std::mem::offset_of!(Raw<Rule>, draw)))[0] = Value::Alias(at(1));
    }
    build(&pack(), &t).unwrap()
}

/// Per saved rule, where each of its `all`, `init` and `draw` pointers points.
fn rule_pointers(saved: &Saved) -> Vec<[Vec<u64>; 3]> {
    let words = |a: or_io::decl::Array<or_io::decl::Ptr<Raw<Step>>>| {
        (0..a.count() as u64)
            .map(|i| saved.word(a.raw() + 8 * i))
            .collect::<Vec<_>>()
    };
    let rs = {
        View::<Raw<Rules>>::at_offset(saved.image(), saved.root)
            .unwrap()
            .rules
    };
    (0..rs.count() as u64)
        .map(|i| {
            let r =
                View::<Raw<Rule>>::at_address(saved.image(), saved.word(rs.raw() + 8 * i)).unwrap();
            [words(r.all), words(r.init), words(r.draw)]
        })
        .collect()
}

#[test]
fn an_element_of_a_run_another_pointer_owns_opens_as_a_reference_by_path() {
    let (rules, saved) = saves_as_tree::<Rules>(&rules_file());
    for (r, rule) in rules.rules.iter().enumerate() {
        let all = |k: usize| to(&format!("rules[{r}].all[{k}]"));
        assert_eq!(rule.init, [all(0), all(2)]);
        assert_eq!(rule.draw, [all(1)]);
    }
    for [all, init, draw] in rule_pointers(&saved) {
        assert_eq!((init, draw), (vec![all[0], all[2]], vec![all[1]]));
    }
}

fn archetype(filename: Ref<String>, mass: f32) -> Box<Archetype> {
    Box::new(Archetype {
        filename,
        mass,
        ..Default::default()
    })
}

/// A file whose damaged archetype's file name is the undamaged one's string.
fn physics_file() -> Saved {
    let hull = || Ref::Own("hull".to_string());
    let physics = Physics {
        archetype: archetype(hull(), 5.0),
        damaged: Some(archetype(hull(), 4.0)),
        ..Default::default()
    };
    let mut t = tree(&physics);
    let filename = std::mem::offset_of!(Raw<Archetype>, filename);
    let undamaged = child(&mut t, std::mem::offset_of!(Raw<Physics>, archetype));
    own_at(child(target(undamaged), filename), SHARED);
    let damaged = child(&mut t, std::mem::offset_of!(Raw<Physics>, damaged));
    *child(target(damaged), filename) = Value::Alias(SHARED);
    build(&pack(), &t).unwrap()
}

#[test]
fn a_string_owned_elsewhere_opens_as_a_reference_by_path() {
    let (physics, saved) = saves_as_tree::<Physics>(&physics_file());
    assert_eq!(physics.archetype.filename, Ref::Own("hull".into()));
    assert_eq!(
        physics.damaged.as_ref().unwrap().filename,
        Ref::To("archetype.filename".into())
    );
    let p = View::<Raw<Physics>>::at_offset(saved.image(), saved.root).unwrap();
    let name = |a: u64| {
        {
            View::<Raw<Archetype>>::at_address(saved.image(), a)
                .unwrap()
                .filename
        }
        .raw()
    };
    let (undamaged, damaged) = (name({ p.archetype }.raw()), name({ p.damaged }.raw()));
    assert!(undamaged != 0 && damaged == undamaged, "one string");
}

#[test]
fn a_reference_to_a_removed_run_element_is_refused_and_later_ones_follow_theirs() {
    let (rules, _) = saves_as_tree::<Rules>(&rules_file());

    let mut removed = rules.clone();
    removed.rules[1].all.remove(1);
    removed.rules[1].all.push(step(10));
    let Err(why) = save(&removed) else {
        panic!("a reference to a removed element is refused")
    };
    assert!(why.contains("rules[1].all[1]"), "{why}");

    let mut edited = rules.clone();
    edited.rules[1].all.remove(0);
    edited.rules[1].init.remove(0);
    let saved = save(&edited).unwrap();
    let [all, init, draw] = &rule_pointers(&saved)[1];
    assert_eq!(
        (init, draw),
        (&vec![all[1]], &vec![all[0]]),
        "each reference follows its step"
    );
    let reopened: Rules = open(&saved).unwrap();
    assert_eq!(reopened.rules[1].init, [to("rules[1].all[1]")]);
    assert_eq!(reopened.rules[1].draw, [to("rules[1].all[0]")]);

    let mut fewer = rules;
    fewer.rules.remove(0);
    let saved = save(&fewer).unwrap();
    let [all, init, draw] = &rule_pointers(&saved)[0];
    assert_eq!(
        (init, draw),
        (&vec![all[0], all[2]], &vec![all[1]]),
        "references follow their rule"
    );
}

/// A file whose two map entries both point at the second curve.
fn twice_file() -> Saved {
    let curves = vec![curve(0x10, &[1, 2]), curve(0x20, &[3, 4, 5])];
    let evolutions = Evolutions {
        map: (0..2)
            .map(|_| CurveEntry {
                hash: 0x20,
                item: Ref::Own(Box::new(curves[1].clone())),
                ..Default::default()
            })
            .collect(),
        curves,
        ..Default::default()
    };
    let built = save(&evolutions).unwrap();
    let (items, curves) = evolution_items(&built);
    let second = curves + std::mem::size_of::<Raw<Curve>>() as u64;
    built
        .with_word(items[0].0, second)
        .with_word(items[1].0, second)
}

#[test]
fn two_references_to_one_element_past_the_first_share_one_copy() {
    let (evolutions, saved) = saves_as_tree::<Evolutions>(&twice_file());
    let items: Vec<_> = evolutions.map.iter().map(|m| &m.item).collect();
    assert_eq!(
        items,
        [&Ref::To("curves[1]".into()), &Ref::To("curves[1]".into())]
    );
    let (items, _) = evolution_items(&saved);
    assert_eq!(items[0].1, items[1].1, "one copy, {items:x?}");

    let scratch = Evolutions {
        curves: evolutions.curves.clone(),
        map: (0..2)
            .map(|_| CurveEntry {
                hash: 0x20,
                item: Ref::To("curves[1]".into()),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let (items, _) = evolution_items(&round_trip(&scratch));
    assert_eq!(items[0].1, items[1].1, "one copy, {items:x?}");
}

#[test]
fn a_copied_element_keeps_its_counted_array_count() {
    let (_, saved) = saves_as_tree::<Evolutions>(&evolutions_file());
    let (items, _) = evolution_items(&saved);
    let copy = View::<Raw<Curve>>::at_address(saved.image(), items[1].1).unwrap();
    let keys = { copy.keys };
    assert_eq!(
        (keys.count(), keys.capacity()),
        (3, 3),
        "the copy states the second curve's keys"
    );
}

/// The `all` run of saved rule `r`: its count and capacity.
fn all_header(saved: &Saved, r: usize) -> (usize, usize) {
    let rs = {
        View::<Raw<Rules>>::at_offset(saved.image(), saved.root)
            .unwrap()
            .rules
    };
    let rule =
        View::<Raw<Rule>>::at_address(saved.image(), saved.word(rs.raw() + 8 * r as u64)).unwrap();
    let all = { rule.all };
    (all.count(), all.capacity())
}

#[test]
fn an_aliased_counted_array_states_its_owners_count_and_capacity() {
    let rules = Rules {
        rules: (0..2)
            .map(|_| {
                Box::new(Rule {
                    all: vec![step(1), step(2), step(3)],
                    ..Default::default()
                })
            })
            .collect(),
        ..Default::default()
    };
    // The alias comes after its owner, then before it.
    for (owner, alias) in [(0, 1), (1, 0)] {
        let mut t = tree(&rules);
        let list = elements(child(&mut t, std::mem::offset_of!(Raw<Rules>, rules)));
        let all = std::mem::offset_of!(Raw<Rule>, all);
        let run = child(target(&mut list[owner]), all);
        own_at(run, SHARED);
        match target(run) {
            Value::Array { capacity, .. } => *capacity = Some(5),
            other => panic!("an array, not {other:?}"),
        }
        *child(target(&mut list[alias]), all) = Value::Alias(SHARED);
        let saved = build(&pack(), &t).unwrap();
        assert_eq!(all_header(&saved, owner), (3, 5));
        assert_eq!(
            all_header(&saved, alias),
            (3, 5),
            "the alias states its owner's"
        );
    }
}

#[decl::record(size = 0x10)]
pub struct Shelf {
    pub steps: Vec<Box<Step>>,
}

/// Steps `all` and `shelf` own, which `lead` and `spare` read by value and `pick` and `spot` name; an owner comes before its copy, so a read makes it the owner.
#[decl::record(size = 0x38)]
pub struct Stage {
    pub all: Vec<Box<Step>>,
    pub lead: Option<Box<Step>>,
    pub pick: Ref<Option<Box<Step>>>,
    pub shelf: Option<Box<Shelf>>,
    pub spare: Option<Box<Shelf>>,
    pub spot: Ref<Option<Box<Step>>>,
}

fn shelf() -> Option<Box<Shelf>> {
    Some(Box::new(Shelf {
        steps: vec![step(4), step(5), step(6)],
        ..Default::default()
    }))
}

/// A stage whose `lead` and `pick` point at `all[1]`, whose `spare` is its `shelf`, and whose `spot` points at the shelf's second step.
fn stage_file() -> Saved {
    let stage = Stage {
        all: vec![step(7), step(8), step(9)],
        lead: Some(step(8)),
        pick: Ref::Own(Some(step(8))),
        shelf: shelf(),
        spare: shelf(),
        spot: Ref::Own(Some(step(5))),
        ..Default::default()
    };
    let mut t = tree(&stage);
    let at = |k: usize| SHARED + 0x10 * k as u64;
    let all = child(&mut t, std::mem::offset_of!(Raw<Stage>, all));
    for (k, s) in elements(all).iter_mut().enumerate() {
        own_at(s, at(k));
    }
    let shelf = child(&mut t, std::mem::offset_of!(Raw<Stage>, shelf));
    own_at(shelf, at(0x10));
    let steps = child(target(shelf), std::mem::offset_of!(Raw<Shelf>, steps));
    for (k, s) in elements(steps).iter_mut().enumerate() {
        own_at(s, at(0x20 + k));
    }
    for (field, address) in [
        (std::mem::offset_of!(Raw<Stage>, lead), at(1)),
        (std::mem::offset_of!(Raw<Stage>, pick), at(1)),
        (std::mem::offset_of!(Raw<Stage>, spare), at(0x10)),
        (std::mem::offset_of!(Raw<Stage>, spot), at(0x21)),
    ] {
        *child(&mut t, field) = Value::Alias(address);
    }
    build(&pack(), &t).unwrap()
}

#[test]
fn a_reference_to_a_removed_owner_is_refused_rather_than_moved_onto_its_copy() {
    let (stage, _) = saves_as_tree::<Stage>(&stage_file());
    assert_eq!((&stage.lead, &stage.spare), (&Some(step(8)), &shelf()));
    assert_eq!(stage.pick, Ref::To("all[1]".into()));
    assert_eq!(stage.spot, Ref::To("shelf.steps[1]".into()));

    let mut removed = stage.clone();
    removed.all.remove(1);
    let Err(why) = save(&removed) else {
        panic!("a reference to an owner a copy survives is refused")
    };
    assert!(why.contains("all[1]"), "{why}");

    let mut removed = stage;
    removed.shelf.as_mut().unwrap().steps.remove(1);
    let Err(why) = save(&removed) else {
        panic!("a reference to an owner a copy of its record survives is refused")
    };
    assert!(why.contains("shelf.steps[1]"), "{why}");
}
