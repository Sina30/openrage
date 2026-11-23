use super::super::{read, Resource};
use super::*;
use crate::container::rpf::{Rpf9Role, Rpf9Signature};

fn hash(name: &str) -> u32 {
    name_hash32(name.as_bytes())
}

/// A virtual section laid out record by record, each 16-aligned.
#[derive(Default)]
struct Body {
    v: Vec<u8>,
}

impl Body {
    fn alloc(&mut self, len: usize) -> usize {
        let at = self.v.len().next_multiple_of(16);
        self.v.resize(at + len, 0);
        at
    }
    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.v[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn u16(&mut self, at: usize, x: u16) {
        self.put(at, &x.to_le_bytes());
    }
    fn u32(&mut self, at: usize, x: u32) {
        self.put(at, &x.to_le_bytes());
    }
    fn u64(&mut self, at: usize, x: u64) {
        self.put(at, &x.to_le_bytes());
    }
    fn f32(&mut self, at: usize, x: f32) {
        self.put(at, &x.to_le_bytes());
    }
    fn ptr(&mut self, at: usize, to: usize) {
        self.u64(at, 0x5000_0000 + to as u64);
    }
    fn text(&mut self, s: &str) -> usize {
        let at = self.alloc(s.len() + 1);
        self.put(at, s.as_bytes());
        at
    }
    fn array(&mut self, at: usize, to: usize, count: u16, capacity: u16) {
        self.ptr(at, to);
        self.u16(at + 8, count);
        self.u16(at + 10, capacity);
    }
    fn kfp(&mut self, at: usize, id: u32, base: u32, entries: &[([f32; 4], [f32; 4])]) {
        self.u32(at + 0x28, id);
        self.u32(at + 0x2C, base);
        if entries.is_empty() {
            return;
        }
        let e = self.alloc(0x20 * entries.len());
        for (i, (time, value)) in entries.iter().enumerate() {
            for lane in 0..4 {
                self.f32(e + 0x20 * i + 4 * lane, time[lane]);
                self.f32(e + 0x20 * i + 0x10 + 4 * lane, value[lane]);
            }
        }
        let n = entries.len() as u16;
        self.array(at + 8, e, n, n);
    }
    fn kfp_list(&mut self, at: usize, kfps: &[usize], capacity: u16) {
        let list = self.alloc(8 * kfps.len());
        for (i, k) in kfps.iter().enumerate() {
            self.ptr(list + 8 * i, *k);
        }
        self.array(at, list, kfps.len() as u16, capacity);
    }
    /// A dictionary at the root's `slot` of one entry keyed by `name`.
    fn dictionary(&mut self, slot: usize, name: &str, entry: usize) {
        let d = self.alloc(0x40);
        self.u64(d + 0x18, 1);
        let codes = self.alloc(4);
        self.u32(codes, hash(name));
        let entries = self.alloc(8);
        self.ptr(entries, entry);
        self.array(d + 0x20, codes, 1, 1);
        self.array(d + 0x30, entries, 1, 1);
        self.ptr(slot, d);
    }
    /// A set or map of `keys` in 11 buckets, each node's data the value given.
    fn atmap(&mut self, at: usize, keys: &[(u32, Option<usize>)]) {
        let buckets = self.alloc(8 * 11);
        for (key, data) in keys {
            let node = self.alloc(0x20);
            self.u32(node, *key);
            if let Some(d) = data {
                self.ptr(node + 8, *d);
            }
            self.ptr(buckets + 8 * (*key as usize % 11), node);
        }
        self.ptr(at, buckets);
        self.u32(at + 8, 11);
        self.u32(at + 0x10, keys.len() as u32);
    }
    fn domain(&mut self, kind: usize, shape: u32, model: Option<&str>) -> usize {
        let d = self.alloc(if model.is_some() { 0x1C8 } else { 0x1B0 });
        self.u32(d + 0x188, kind as u32);
        self.u32(d + 0x18C, shape);
        self.f32(d + 0x194, 2.1);
        let ks: Vec<usize> = (0..6).map(|i| d + 8 + 0x40 * i).collect();
        for (i, k) in ks.iter().enumerate() {
            let position = [0.5 + kind as f32, 1.5, 2.5, 0.0];
            let entries = match i {
                0 => vec![([0.0; 4], position)],
                _ => vec![],
            };
            self.kfp(*k, DOMAIN_KFPS[kind][i], DOMAIN_KFPS[0][i], &entries);
        }
        self.kfp_list(d + 0x198, &[ks[0], ks[1], ks[3], ks[2], ks[4], ks[5]], 16);
        if let Some(model) = model {
            let name = self.text(model);
            self.ptr(d + 0x1B0, name);
            self.u32(d + 0x1C0, hash(model));
        }
        d
    }
}

const EFFECT: &str = "ent_test_spray";
const EMITTER: &str = "test_spray_emit";
const PARTICLE: &str = "test_spray";
const MESH: &str = "ptfx_mesh_test_sphere";
const EVOLUTION: &str = "strength";
const SPAN: [f32; 4] = [1.0, 1.0, 0.0, 0.0];
/// The class word of the body's rule: bits 1 and 7.
const AGE_AND_SIZE: u32 = (1 << 1) | (1 << 7);

/// The class word `bits` and its three masks written over the rule at `rule`.
fn class_words(v: &mut [u8], rule: usize, bits: u32) {
    let words = [
        (0x180, bits),
        (0x198, bits & !((1 << 18) | (7 << 27))),
        (0x1A0, bits & (1 << 19)),
        (0x1A8, bits & 0x0398_0001),
    ];
    for (o, w) in words {
        v[rule + o..rule + o + 4].copy_from_slice(&w.to_le_bytes());
    }
}
/// Shader words by the names of their variable and value, a switch's value empty; the body's
/// rule holds the first three.
const WORDS: [(&str, &str); 8] = [
    ("lightingmode", "diffuseonly"),
    ("projectionmode", "none"),
    ("diffusemode", "rgba"),
    ("soft", ""),
    ("uvdistortion", ""),
    ("refract", ""),
    ("screenspace", ""),
    ("soft", ""),
];

/// Where a test changes the body it is given.
struct Places {
    particle: usize,
    registry: usize,
    shader_words: usize,
    event: usize,
    emitter: usize,
}

/// A list of one effect, one emitter rule with a mesh creation domain and a sphere target, and one
/// particle rule listing a behaviour of no named value and a `ptxu_Size`.
fn body() -> (Vec<u8>, Places) {
    let mut b = Body::default();
    let root = b.alloc(0x70);
    let map = b.alloc(0x10);
    b.ptr(root + 8, map);
    let name = b.text("test_list");
    b.ptr(root + 0x10, name);

    let r = b.alloc(0x20);
    let count: usize = REGISTRY.iter().map(|(_, n)| n).sum();
    let registry = b.alloc(0x10 * count);
    let mut i = 0;
    for (class, n) in REGISTRY {
        for _ in 0..n {
            b.u32(registry + 0x10 * i + 8, 0x0A00_0100 + 3 * i as u32);
            b.u32(registry + 0x10 * i + 0xC, hash(class));
            i += 1;
        }
    }
    b.array(r + 8, registry, count as u16, count as u16);
    b.ptr(root + 0x20, r);

    let particle = b.alloc(0x430);
    let none = b.text("");
    for s in [0x20, 0x90, 0x100] {
        b.ptr(particle + s + 0x50, none);
    }
    b.u32(particle + 0x10, 2);
    b.u32(particle + 0x170, 3);
    b.u32(particle + 0x174, 7);
    let particle_name = b.text(PARTICLE);
    b.ptr(particle + 0x178, particle_name);
    let unknown = b.alloc(0x10);
    let size = b.alloc(0x118);
    let listed = b.alloc(16);
    b.ptr(listed, unknown);
    b.ptr(listed + 8, size);
    b.array(particle + 0x188, listed, 2, 2);
    // Age's class bit and Size's, one per behaviour listed, and the one mask they reach.
    b.u32(particle + 0x180, AGE_AND_SIZE);
    b.u32(particle + 0x198, AGE_AND_SIZE);
    let whd = [1.0, 2.0, 3.0, 0.0];
    let whd_min = hash("ptxu_Size:m_whdMinKFP");
    b.kfp(size + 8, whd_min, whd_min, &[([0.0; 4], whd)]);
    b.kfp(size + 0x48, hash("ptxu_Size:m_whdMaxKFP"), 0x7710_3929, &[]);
    b.u64(particle + 0x2F0, 0x1122_3344_5566_7788);
    b.u64(particle + 0x2F0 + 8 * 12, 0x0102_0304_0506_0708);
    b.u32(particle + 0x368, 1);
    let words = b.alloc(8 * WORDS.len());
    for (w, (variable, value)) in WORDS.iter().enumerate() {
        let value = if value.is_empty() { 0 } else { hash(value) };
        b.u32(words + 8 * w, hash(variable));
        b.u32(words + 8 * w + 4, value);
    }
    b.array(particle + 0x370, words, 3, 3);
    b.u32(particle + 0x3A8, 0x3FEE_018B);
    b.put(particle + 0x410, &[2, 0]);

    let emitter = b.alloc(0x300);
    b.u32(emitter + 0x10, 2);
    b.u32(emitter + 0x18, EMITTER_VERSION);
    b.u32(emitter + 0x1C, hash(EMITTER));
    let emitter_name = b.text(EMITTER);
    b.ptr(emitter + 0x20, emitter_name);
    let creation = b.domain(0, 4, Some(MESH));
    let target = b.domain(1, 1, None);
    b.ptr(emitter + 0x30, creation);
    b.ptr(emitter + 0x38, target);
    let ks: Vec<usize> = (0..10).map(|i| emitter + 0x48 + 0x40 * i).collect();
    for (i, k) in ks.iter().enumerate() {
        let entries = match i {
            0 => vec![
                ([0.0; 4], [10.0, 10.0, 0.0, 0.0]),
                (SPAN, [20.0, 30.0, 0.0, 0.0]),
            ],
            _ => vec![],
        };
        b.kfp(*k, EMITTER_KFPS[i], EMITTER_KFPS[i], &entries);
    }
    b.kfp_list(emitter + 0x2C8, &ks, 10);
    b.u32(emitter + 0x2F0, 1);

    let effect = b.alloc(0x240);
    b.u32(effect + 0x10, 1);
    b.u32(effect + 0x18, EFFECT_VERSION);
    let effect_name = b.text(EFFECT);
    b.ptr(effect + 0x20, effect_name);
    b.ptr(effect + 0x28, root);
    b.u32(effect + 0x50, -1i32 as u32);
    b.put(effect + 0x54, &[1, 2, 0, 1]);
    for (i, x) in [0.0, 0.25, 1.5, 2.5, 0.75, 1.25].into_iter().enumerate() {
        b.f32(effect + 0x70 + 4 * i, x);
    }
    b.f32(effect + 0xA4, 20.0);
    let ks: Vec<usize> = (0..5).map(|i| effect + 0xC0 + 0x40 * i).collect();
    for (i, k) in ks.iter().enumerate() {
        b.kfp(*k, EFFECT_KFPS[i], EFFECT_KFPS[i], &[]);
    }
    b.kfp_list(effect + 0x200, &ks, 16);
    b.u32(effect + 0x23C, ZOOM_LEVEL);
    let event = b.alloc(0x88);
    b.f32(event + 0x10, 0.1);
    b.f32(event + 0x14, 0.9);
    b.ptr(event + 0x40, emitter_name);
    b.ptr(event + 0x48, particle_name);
    b.ptr(event + 0x50, emitter);
    b.ptr(event + 0x58, particle);
    for (i, x) in [1.0, 2.0, 3.0, 4.0].into_iter().enumerate() {
        b.f32(event + 0x68 + 4 * i, x);
    }
    b.u32(event + 0x78, 0xFF80_4020);
    b.u32(event + 0x7C, 0xFFFF_FFFF);
    let events = b.alloc(8);
    b.ptr(events, event);
    b.array(effect + 0x38, events, 1, 4);

    let list = b.alloc(0x38);
    let evolutions = b.alloc(0x18);
    let evolution_name = b.text(EVOLUTION);
    b.ptr(evolutions, evolution_name);
    b.u32(evolutions + 8, hash(EVOLUTION));
    b.f32(evolutions + 0xC, 0.5);
    b.v[evolutions + 0x10] = 1;
    b.array(list, evolutions, 1, 1);
    let props = b.alloc(0x18);
    b.u32(props + 0x10, EFFECT_KFPS[0]);
    b.v[props + 0x14] = 2;
    b.array(list + 0x10, props, 1, 1);
    b.atmap(list + 0x20, &[(EFFECT_KFPS[0], Some(props))]);
    b.ptr(effect + 0x48, list);

    b.dictionary(root + 0x28, EFFECT, effect);
    b.dictionary(root + 0x30, EMITTER, emitter);
    b.dictionary(root + 0x38, PARTICLE, particle);
    b.atmap(root + 0x58, &[(hash(MESH), None)]);
    let places = Places {
        particle,
        registry,
        shader_words: particle + 0x370,
        event,
        emitter,
    };
    (b.v, places)
}

fn read_list(v: &[u8]) -> Result<ParticleEffects> {
    read_particle_effects(v, None)
}

fn is_refused(read: Result<ParticleEffects>) -> bool {
    matches!(read, Err(HeaderlessError::Particles(_)))
}

#[test]
fn a_particle_list_is_placed_by_its_signature_and_read_through_the_shared_dispatch() {
    let (v, _) = body();
    let signature = Rpf9Signature::of(&v, v.len() as u64).expect("a signature");
    assert_eq!(signature.to_string(), "0PP0PPPP");
    assert_eq!(signature.role(), Some(Rpf9Role::ParticleEffects));
    let Ok(Resource::ParticleEffects(list)) = read(signature, &v, None) else {
        panic!("read as a particle list");
    };
    assert_eq!(list.name, "test_list");
    // A root whose tables point inside it is not this layout.
    let mut inside = v.clone();
    inside[0x20..0x28].copy_from_slice(&0x5000_0040u64.to_le_bytes());
    let signature = Rpf9Signature::of(&inside, inside.len() as u64).expect("a signature");
    assert_eq!(signature.role(), None);
}

#[test]
fn a_particle_list_reads_its_rules_events_domains_and_keyframes() {
    let (v, _) = body();
    let list = read_list(&v).expect("it reads");
    assert_eq!(list.registry.len(), 29);
    let [effect] = &list.effects[..] else {
        panic!("one effect");
    };
    assert_eq!((effect.name.as_str(), effect.code), (EFFECT, hash(EFFECT)));
    assert_eq!(effect.num_loops, -1);
    assert_eq!(effect.flags, [1, 2, 0, 1]);
    assert_eq!(effect.times, [0.0, 0.25, 1.5, 2.5, 0.75, 1.25]);
    assert_eq!(effect.culling[1], 20.0);
    let ids: Vec<u32> = effect.kfps.iter().map(|k| k.id).collect();
    assert_eq!(ids, EFFECT_KFPS);
    let [event] = &effect.events[..] else {
        panic!("one event");
    };
    assert_eq!((event.start_ratio, event.end_ratio), (0.1, 0.9));
    assert_eq!((event.emitter, event.particle), (0, 0));
    assert_eq!(event.playback_rate_scalar, [1.0, 2.0]);
    assert_eq!(event.zoom_scalar, [3.0, 4.0]);
    assert_eq!(event.colour_tint, [0xFF80_4020, 0xFFFF_FFFF]);
    let evolutions = effect.evolution_list.as_ref().expect("an evolution list");
    assert_eq!(evolutions.evolutions[0].name, EVOLUTION);
    assert_eq!(evolutions.evolutions[0].override_value, 0.5);
    assert!(evolutions.evolutions[0].is_overridden);
    assert_eq!(evolutions.props[0].id, EFFECT_KFPS[0]);
    assert_eq!(evolutions.props[0].blend_mode, 2);

    let [emitter] = &list.emitters[..] else {
        panic!("one emitter");
    };
    assert_eq!(emitter.name, EMITTER);
    assert_eq!(emitter.flags, 1);
    let rate = &emitter.kfps[0];
    assert_eq!(rate.entries.len(), 2);
    assert_eq!(rate.entries[1].time, SPAN);
    assert_eq!(rate.entries[1].value, [20.0, 30.0, 0.0, 0.0]);
    let [Some(creation), Some(target), None] = &emitter.domains else {
        panic!("a creation and a target domain");
    };
    assert_eq!(creation.shape, 4);
    assert_eq!(creation.model, Some((MESH.to_string(), hash(MESH))));
    assert_eq!(target.shape, 1);
    assert_eq!(target.kfps[0].entries[0].value, [1.5, 1.5, 2.5, 0.0]);
    assert_eq!(target.kfps[0].base_id, DOMAIN_KFPS[0][0]);
    assert_eq!(list.mesh_hashes, BTreeSet::from([hash(MESH)]));
    assert!(list.drawable_hashes.is_empty());
}

#[test]
fn a_particle_rule_lists_its_behaviours_by_class_or_by_size() {
    let (v, _) = body();
    let list = read_list(&v).expect("it reads");
    let [rule] = &list.particles[..] else {
        panic!("one particle rule");
    };
    assert_eq!(rule.name, PARTICLE);
    assert_eq!(rule.tex_frame_id, [3, 7]);
    assert_eq!(rule.texture_keys[0], 0x1122_3344_5566_7788);
    assert_eq!(rule.texture_keys[12], 0x0102_0304_0506_0708);
    assert_eq!(rule.shader_words[2], (hash("diffusemode"), hash("rgba")));
    assert_eq!(rule.shader_hash, 0x3FEE_018B);
    assert_eq!((rule.sort_type, rule.draw_type), (2, 0));
    assert!(rule
        .spawners
        .iter()
        .all(|s| s.effect.is_none() && s.name.is_empty()));
    let classes: Vec<_> = (rule.behaviours.iter())
        .map(|&b| (list.behaviours[b].class, list.behaviours[b].kfps.len()))
        .collect();
    assert_eq!(classes, [(None, 0), (Some("ptxu_Size"), 2)]);
    let unknown = &list.behaviours[rule.behaviours[0]];
    assert_eq!(unknown.size, 0x10);
    let size = &list.behaviours[rule.behaviours[1]];
    assert_eq!(size.kfps[0].entries[0].value, [1.0, 2.0, 3.0, 0.0]);
    // The second value's base id differs from its id, which is read and not refused.
    assert_eq!(size.kfps[1].id, hash("ptxu_Size:m_whdMaxKFP"));
    assert_eq!(size.kfps[1].base_id, 0x7710_3929);
}

#[test]
fn a_shader_word_count_above_five_reads() {
    let (mut v, at) = body();
    v[at.shader_words + 8..at.shader_words + 12].copy_from_slice(&[8, 0, 8, 0]);
    let list = read_list(&v).expect("eight words read");
    assert_eq!(list.particles[0].shader_words.len(), 8);
    assert_eq!(list.particles[0].shader_words[7], (hash("soft"), 0));
}

#[test]
fn a_particle_list_is_refused_where_a_measured_rule_breaks() {
    let (v, at) = body();
    assert!(read_list(&v).is_ok());
    let refused = |change: &dyn Fn(&mut Vec<u8>)| {
        let mut w = v.clone();
        change(&mut w);
        is_refused(read_list(&w))
    };
    // A reference count other than 1 and the events naming the rule.
    assert!(refused(&|w| w[at.particle + 0x10] = 7));
    // The model set emptied while a mesh domain names a model.
    assert!(refused(&|w| w[0x58..0x70].fill(0)));
    // A registry class hash of another class.
    let wind = hash("ptxu_Wind").to_le_bytes();
    assert!(refused(
        &|w| w[at.registry + 0xC..at.registry + 0x10].copy_from_slice(&wind)
    ));
    // An event whose particle rule is the emitter rule.
    let emitter = (0x5000_0000 + at.emitter as u64).to_le_bytes();
    assert!(refused(
        &|w| w[at.event + 0x58..at.event + 0x60].copy_from_slice(&emitter)
    ));
    // A draw type past trail.
    assert!(refused(&|w| w[at.particle + 0x411] = 3));
    // A trail rule with no trail behaviour.
    assert!(refused(&|w| w[at.particle + 0x411] = 2));
}

#[test]
fn a_rules_class_word_gives_each_behaviour_its_bit_in_the_order_it_lists_them() {
    let (v, _) = body();
    let list = read_list(&v).expect("it reads");
    let rule = &list.particles[0];
    assert_eq!(rule.class_bits, AGE_AND_SIZE);
    let bits: Vec<Option<u8>> = (rule.behaviours.iter())
        .map(|&b| list.behaviours[b].class_bit)
        .collect();
    assert_eq!(bits, [Some(1), Some(7)]);
    assert_eq!(
        list.behaviours[rule.behaviours[0]].fields,
        BehaviourFields::Age,
        "bit 1 is ptxu_Age"
    );
    assert_eq!(
        class_of_bit(7),
        Some((Some("ptxu_Size"), ClassNamedBy::Kfp))
    );
}

#[test]
fn a_class_word_that_breaks_a_measured_rule_is_refused() {
    let (v, at) = body();
    let refused = |change: &dyn Fn(&mut Vec<u8>)| {
        let mut w = v.clone();
        change(&mut w);
        is_refused(read_list(&w))
    };
    let rule = at.particle;
    // One bit for two behaviours, and ptxu_Size at Rotation's bit.
    assert!(refused(&|w| class_words(w, rule, 1 << 1)));
    assert!(refused(&|w| class_words(w, rule, (1 << 1) | (1 << 5))));
    // A bit no rule sets, a model bit on a sprite rule.
    assert!(refused(&|w| class_words(w, rule, (1 << 1) | (1 << 13))));
    assert!(refused(&|w| class_words(w, rule, (1 << 1) | (1 << 29))));
    // A mask that is not the word's, and a pad word set.
    assert!(refused(&|w| w[rule + 0x198] = 0));
    assert!(refused(&|w| w[rule + 0x1A4] = 1));
    // A third spawner that starts an effect in a rule with no ptxu_ZCull.
    let effect = read_list(&v).expect("it reads").effects[0].at;
    let to = (0x5000_0000 + effect).to_le_bytes();
    assert!(refused(
        &|w| w[rule + 0x148..rule + 0x150].copy_from_slice(&to)
    ));
    // The same at the first spawner reads.
    let mut first = v.clone();
    first[rule + 0x68..rule + 0x70].copy_from_slice(&to);
    assert!(read_list(&first).is_ok());
}

#[test]
fn a_map_holds_the_smallest_slot_count_of_its_series_above_its_key_count() {
    let (v, _) = body();
    let mut w = v.clone();
    w[0x58 + 8..0x58 + 0xC].copy_from_slice(&29u32.to_le_bytes());
    assert!(is_refused(read_list(&w)), "29 slots for one key");
}

#[test]
fn a_truncated_particle_list_is_an_error_not_a_panic() {
    let (v, _) = body();
    for len in [0, 0x40, 0x70, v.len() / 2, v.len() - 1] {
        assert!(read_list(&v[..len]).is_err(), "{len}");
    }
}

#[test]
fn a_particle_list_written_unchanged_is_byte_identical() {
    let (mut v, _) = body();
    let before = v.clone();
    let list = read_list(&v).expect("it reads");
    write_particle_effects(&mut v, &list).expect("it writes");
    assert_eq!(v, before);
}

#[test]
fn a_particle_list_writes_its_values_in_place_and_reads_them_back() {
    let (mut v, _) = body();
    let size = v.len();
    let mut list = read_list(&v).expect("it reads");
    let effect = &mut list.effects[0];
    effect.times[2] = 3.5;
    effect.flags[0] = 0;
    effect.culling[1] = 25.0;
    effect.events[0].start_ratio = 0.2;
    effect.events[0].colour_tint[1] = 0x8040_2010;
    let emitter = &mut list.emitters[0];
    emitter.flags = 5;
    emitter.kfps[0].entries[1].value = [25.0, 35.0, 0.0, 0.0];
    let target = emitter.domains[1].as_mut().expect("a target domain");
    target.flags = [0, 1, 1, 0];
    target.kfps[0].entries[0].value = [6.0, 7.0, 8.0, 0.0];
    let rule = &mut list.particles[0];
    rule.tex_frame_id = [4, 9];
    rule.sort_type = 3;
    rule.spawners[1].trigger_info = 0.75;
    rule.spawners[0].scalars_max.zoom = 120.0;
    let behaviour = rule.behaviours[1];
    list.behaviours[behaviour].kfps[0].entries[0].value = [4.0, 5.0, 6.0, 0.0];
    write_particle_effects(&mut v, &list).expect("it writes");
    assert_eq!(v.len(), size);
    assert_eq!(read_list(&v).expect("it reads again"), list);
}

#[test]
fn a_particle_list_edit_that_would_move_or_is_not_written_is_refused_and_writes_nothing() {
    let (v, _) = body();
    let list = read_list(&v).expect("it reads");
    let refused = |change: &dyn Fn(&mut ParticleEffects)| {
        let mut edited = list.clone();
        change(&mut edited);
        let mut w = v.clone();
        let refused = matches!(
            write_particle_effects(&mut w, &edited),
            Err(HeaderlessError::Particles(_))
        );
        refused && w == v
    };
    // A key added to a keyframed value.
    assert!(refused(&|l| {
        let key = l.emitters[0].kfps[0].entries[1];
        l.emitters[0].kfps[0].entries.push(key);
    }));
    // A rule renamed, an event removed, a domain's shape changed.
    assert!(refused(&|l| l.particles[0].name = "another".into()));
    assert!(refused(&|l| l.effects[0].events.clear()));
    assert!(refused(&|l| {
        if let Some(d) = l.emitters[0].domains[1].as_mut() {
            d.shape = 2;
        }
    }));
    // A key's time outside [0, 1], which the reader refuses.
    assert!(refused(&|l| l.emitters[0].kfps[0].entries[1].time[0] = -1.0));
}

#[test]
fn a_rule_an_event_or_a_behaviour_added_or_removed_is_refused_naming_what_is_not_stated() {
    let (v, _) = body();
    let list = read_list(&v).expect("it reads");
    let why = |change: &dyn Fn(&mut ParticleEffects)| {
        let mut edited = list.clone();
        change(&mut edited);
        let mut w = v.clone();
        let refused = write_particle_effects(&mut w, &edited).expect_err("refused");
        assert_eq!(w, v, "{refused}");
        refused.to_string()
    };
    let rule = why(&|l| {
        let copy = l.particles[0].clone();
        l.particles.push(copy);
    });
    assert!(rule.contains("a rule was added or removed"), "{rule}");
    let behaviour = why(&|l| {
        l.behaviours.pop();
    });
    assert!(behaviour.contains("no stated class"), "{behaviour}");
    let event = why(&|l| {
        let copy = l.effects[0].events[0].clone();
        l.effects[0].events.push(copy);
    });
    assert!(event.contains("+0x60"), "{event}");
}

#[path = "particles_open_tests.rs"]
mod open;
