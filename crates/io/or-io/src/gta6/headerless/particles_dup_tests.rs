//! Events, emitter rules, particle rules and behaviours copied past the section's end.

use super::*;
use crate::container::rpf::{Rpf9Form, Rpf9Sections};

const EMITTER_COPY: &str = "test_spray_emit_copy";
const PARTICLE_COPY: &str = "test_spray_copy";

/// The body as block-mapped sections whose map counts one virtual block and, where `physical` is
/// not empty, one physical block.
fn mapped(mut v: Vec<u8>, physical: Vec<u8>) -> Rpf9Sections {
    let map = (u64_at(&v, 8) & 0x0FFF_FFFF) as usize;
    v[map + 8] = 1;
    v[map + 9] = u8::from(!physical.is_empty());
    Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: v,
        physical,
    }
}

fn put(v: &mut [u8], at: usize, bytes: &[u8]) {
    v[at..at + bytes.len()].copy_from_slice(bytes);
}

/// The body with GPU buffers at +0x388 and +0x390 of its sprite rule, over a physical section
/// holding their data at 0 and 0x200; the rule's float at +0x1F8 and its copy set to 32.
fn with_buffers() -> (Rpf9Sections, Places) {
    let (mut v, at) = body();
    put(&mut v, at.particle + 0x1F8, &32.0f32.to_le_bytes());
    let mut p = vec![0u8; 0x400];
    put(&mut p, 0, &0x3C00u16.to_le_bytes());
    put(&mut p, 0x200 + 0x18, &32.0f32.to_le_bytes());
    // Each slot's length in words and the words at +0x0C, +0x0E and +0x10 measured there.
    let slots = [
        (0x388, 0x80u32, 8u16, 8u16, 0x980u32),
        (0x390, 8, 16, 4, 0x1600),
    ];
    for (i, (slot, words, a, b, flags)) in slots.into_iter().enumerate() {
        let object = v.len().next_multiple_of(16);
        v.resize(object + 0x50, 0);
        let to = 0x5000_0000 + object as u64;
        put(&mut v, at.particle + slot, &to.to_le_bytes());
        put(&mut v, object + 8, &words.to_le_bytes());
        put(&mut v, object + 0xC, &a.to_le_bytes());
        put(&mut v, object + 0xE, &b.to_le_bytes());
        put(&mut v, object + 0x10, &flags.to_le_bytes());
        let data = 0x6000_0000 + 0x200 * i as u64;
        put(&mut v, object + 0x18, &data.to_le_bytes());
    }
    (mapped(v, p), at)
}

fn refused_unchanged(s: &mut Rpf9Sections, copy: impl Fn(&mut Rpf9Sections) -> Copied) -> String {
    let before = s.clone();
    let refused = copy(s).expect_err("refused");
    assert_eq!(*s, before, "{refused}");
    refused.to_string()
}

type Copied = std::result::Result<(), ParticleKeysError>;

#[test]
fn an_event_duplicated_is_its_effects_last_and_counts_its_rules_once_more() {
    let (v, at) = body();
    let mut s = mapped(v, Vec::new());
    let before = s.virtual_section.clone();
    let old = before.len();
    let effect = read_list(&before).expect("it reads").effects[0].at as usize;
    duplicate_particle_event(&mut s, 0, 0).expect("it copies");

    let list = read_list(&s.virtual_section).expect("it reads");
    let events = &list.effects[0].events;
    assert_eq!(events.len(), 2);
    assert_eq!(events[1], events[0]);
    let held = (u64_at(&s.virtual_section, effect + 0x38) & 0x0FFF_FFFF) as usize;
    assert!(held >= old, "the event list lies past the old end");
    assert_eq!(u32_at(&s.virtual_section, at.emitter + 0x10), 3);
    assert_eq!(u32_at(&s.virtual_section, at.particle + 0x10), 3);
    let mut kept = s.virtual_section[..old].to_vec();
    for (from, len) in [
        (effect + 0x38, 0x10),
        (at.emitter + 0x10, 4),
        (at.particle + 0x10, 4),
    ] {
        kept[from..from + len].copy_from_slice(&before[from..from + len]);
    }
    assert_eq!(kept, before, "nothing else that was there changed");

    // An event whose object breaks a measured rule is not copied.
    let to = (0x5000_0000 + at.particle as u64).to_le_bytes();
    s.virtual_section[at.event + 0x60..at.event + 0x68].copy_from_slice(&to);
    let why = refused_unchanged(&mut s, |s| duplicate_particle_event(s, 0, 0));
    assert!(why.contains("+0x60"), "{why}");
    let why = refused_unchanged(&mut s, |s| duplicate_particle_event(s, 0, 9));
    assert!(why.contains("not one of"), "{why}");
}

#[test]
fn an_emitter_rule_duplicated_reads_as_its_source_and_takes_its_own_edits() {
    let (v, at) = body();
    let mut s = mapped(v, Vec::new());
    let old = s.virtual_section.len();
    duplicate_particle_emitter(&mut s, 0, EMITTER_COPY).expect("it copies");

    let mut list = read_list(&s.virtual_section).expect("it reads");
    assert_eq!(list.emitters.len(), 2);
    let copy = (list.emitters.iter().position(|e| e.name == EMITTER_COPY)).expect("the copy");
    let source = (list.emitters.iter().position(|e| e.name == EMITTER)).expect("the source");
    let (c, src) = (&list.emitters[copy], &list.emitters[source]);
    assert!(c.at as usize >= old, "the copy lies past the old end");
    assert_eq!(c.code, hash(EMITTER_COPY));
    let renamed = EmitterRule {
        at: src.at,
        code: src.code,
        name: src.name.clone(),
        ..c.clone()
    };
    assert_eq!(&renamed, src);
    assert_eq!(u32_at(&s.virtual_section, c.at as usize + 0x10), 1);
    assert_eq!(u32_at(&s.virtual_section, at.emitter + 0x10), 2);
    // The event still names the source.
    let fired = &list.effects[0].events[0];
    assert_eq!(list.emitters[fired.emitter].name, EMITTER);
    assert_eq!(list.mesh_hashes.len(), 1);

    let held = list.emitters[source].clone();
    list.emitters[copy].kfps[0].entries[0].value[0] = 99.0;
    let domain = list.emitters[copy].domains[0].as_mut();
    domain.expect("its creation domain").kfps[0].entries[0].value[0] = 7.0;
    write_particle_effects(&mut s.virtual_section, &list).expect("it writes");
    let back = read_list(&s.virtual_section).expect("it reads again");
    assert_eq!(back.emitters[copy].kfps[0].entries[0].value[0], 99.0);
    assert_eq!(back.emitters[source], held, "the source keeps its keys");

    let why = refused_unchanged(&mut s, |s| duplicate_particle_emitter(s, 0, EMITTER));
    assert!(why.contains("dictionary"), "{why}");
    refused_unchanged(&mut s, |s| duplicate_particle_emitter(s, 5, "elsewhere"));
    refused_unchanged(&mut s, |s| duplicate_particle_emitter(s, 0, "a\tname"));
}

#[test]
fn a_particle_rule_duplicated_has_its_own_behaviours_and_gpu_buffers() {
    let (mut s, at) = with_buffers();
    let (old, physical) = (s.virtual_section.clone(), s.physical.clone());
    duplicate_particle_rule(&mut s, 0, PARTICLE_COPY).expect("it copies");

    let mut list = read_list(&s.virtual_section).expect("it reads");
    assert_eq!(list.particles.len(), 2);
    let copy = (list.particles.iter().position(|p| p.name == PARTICLE_COPY)).expect("the copy");
    let source = (list.particles.iter().position(|p| p.name == PARTICLE)).expect("the source");
    let (c, src) = (&list.particles[copy], &list.particles[source]);
    assert!(c.at as usize >= old.len(), "the copy lies past the old end");
    assert_eq!(c.code, hash(PARTICLE_COPY));
    assert_eq!(list.behaviours.len(), 4, "its own behaviours");
    let renamed = ParticleRule {
        at: src.at,
        code: src.code,
        name: src.name.clone(),
        behaviours: src.behaviours.clone(),
        ..c.clone()
    };
    assert_eq!(&renamed, src);
    for (&a, &b) in c.behaviours.iter().zip(&src.behaviours) {
        let (a, b) = (&list.behaviours[a], &list.behaviours[b]);
        assert_ne!(a.at, b.at);
        assert_eq!(a.class, b.class);
        assert_eq!(a.class_bit, b.class_bit);
        assert_eq!(a.kfps, b.kfps);
    }
    assert_eq!(u32_at(&s.virtual_section, c.at as usize + 0x10), 1);
    let fired = &list.effects[0].events[0];
    assert_eq!(list.particles[fired.particle].name, PARTICLE);

    // Each buffer's data is its own, 256-aligned past the old physical end, and holds its source's.
    let theirs = read_particle_gpu_buffers(&s.virtual_section, Some(&s.physical[..]), c)
        .expect("the copy's buffers read");
    let ours = read_particle_gpu_buffers(&s.virtual_section, Some(&s.physical[..]), src)
        .expect("the source's buffers read");
    let starts: Vec<u64> = theirs.iter().map(|b| b.data_at).collect();
    assert_eq!(starts, [0x400, 0x600]);
    let data = |g: &GpuBuffer| &s.physical[g.data_at as usize..(g.data_at + g.len) as usize];
    for (a, b) in ours.iter().zip(&theirs) {
        assert_eq!(data(a), data(b));
    }
    assert_eq!(&s.physical[..physical.len()], &physical[..]);
    // What was there changed only in the particle dictionary's heads.
    let mut kept = s.virtual_section[..old.len()].to_vec();
    let d = (u64_at(&old, 0x38) & 0x0FFF_FFFF) as usize;
    kept[d + 0x20..d + 0x40].copy_from_slice(&old[d + 0x20..d + 0x40]);
    assert_eq!(kept, old);

    // An edit of the copy's behaviour leaves its source's as it was.
    let size_of = |list: &ParticleEffects, rule: usize| list.particles[rule].behaviours[1];
    let (mine, theirs) = (size_of(&list, copy), size_of(&list, source));
    let held = list.behaviours[theirs].clone();
    list.behaviours[mine].kfps[0].entries[0].value[0] = 9.0;
    write_particle_effects(&mut s.virtual_section, &list).expect("it writes");
    let back = read_list(&s.virtual_section).expect("it reads again");
    assert_eq!(back.behaviours[mine].kfps[0].entries[0].value[0], 9.0);
    assert_eq!(back.behaviours[theirs], held);
    assert_eq!(at.particle, back.particles[source].at as usize);
}

#[test]
fn a_rule_whose_gpu_buffers_do_not_read_is_not_duplicated() {
    // The plain body's sprite rule points at no GPU buffer.
    let (v, _) = body();
    let mut s = mapped(v, Vec::new());
    refused_unchanged(&mut s, |s| duplicate_particle_rule(s, 0, PARTICLE_COPY));
    let (mut s, _) = with_buffers();
    let why = refused_unchanged(&mut s, |s| duplicate_particle_rule(s, 0, PARTICLE));
    assert!(why.contains("dictionary"), "{why}");
    refused_unchanged(&mut s, |s| duplicate_particle_rule(s, 3, "elsewhere"));
}

#[test]
fn a_behaviour_copied_into_a_rule_takes_its_class_bits_place() {
    let (mut s, _) = with_buffers();
    duplicate_particle_rule(&mut s, 0, PARTICLE_COPY).expect("it copies");
    let list = read_list(&s.virtual_section).expect("it reads");
    let copy = (list.particles.iter().position(|p| p.name == PARTICLE_COPY)).expect("the copy");
    let source = (list.particles.iter().position(|p| p.name == PARTICLE)).expect("the source");
    // The copy made to list its second behaviour only, ptxu_Size's.
    let rule = list.particles[copy].at as usize;
    let v = &mut s.virtual_section;
    let listed = (u64_at(v, rule + 0x188) & 0x0FFF_FFFF) as usize;
    let second = v[listed + 8..listed + 0x10].to_vec();
    put(v, listed, &second);
    put(v, rule + 0x188 + 8, &[1, 0, 1, 0]);
    class_words(v, rule, 1 << 7);
    let list = read_list(&s.virtual_section).expect("one behaviour");
    assert_eq!(list.particles[copy].behaviours.len(), 1);
    let age = list.particles[source].behaviours[0];
    let held = list.behaviours.len();

    copy_particle_behaviour(&mut s, age, copy).expect("it copies");
    let list = read_list(&s.virtual_section).expect("it reads");
    let into = &list.particles[copy];
    assert_eq!(into.class_bits, AGE_AND_SIZE);
    assert_eq!(into.behaviours.len(), 2);
    assert_eq!(into.behaviours[0], held, "the copy takes bit 1's place");
    let (a, b) = (&list.behaviours[held], &list.behaviours[age]);
    assert_ne!(a.at, b.at);
    assert_eq!((a.class_bit, &a.fields), (b.class_bit, &b.fields));
    assert_eq!(a.size, b.size, "the copy keeps its source's bytes");
    let masks = [0x198, 0x1A0, 0x1A8].map(|o| u32_at(&s.virtual_section, rule + o));
    assert_eq!(masks, [AGE_AND_SIZE, 0, 0]);

    // A rule lists one behaviour of each class bit.
    let why = refused_unchanged(&mut s, |s| copy_particle_behaviour(s, age, source));
    assert!(why.contains("already"), "{why}");
    refused_unchanged(&mut s, |s| copy_particle_behaviour(s, 99, copy));
}

/// Curve 0 of a rule's five: two keys ramping to 10 as half floats, the other four with no key.
fn sprite_curves() -> ParticleCurves {
    let mut c: ParticleCurves = Default::default();
    c[0].times = vec![0, 0x7299];
    c[0].values = vec![[0.0, 0.0], [10.0, 0.0]];
    c
}

/// Lay `curves` into the sprite rule at `particle`: its five kinds, curve 0's key arrays past the
/// virtual section's end, and the +0x388 tables' physical data (at 0) baked from them.
fn install_curves(s: &mut Rpf9Sections, particle: usize, curves: &ParticleCurves) {
    // Mirrors particles_tables.rs: curves start at +0x248, each 0x18 bytes, kinds 3, 2, 7, 6, 6.
    const CURVES: usize = 0x248;
    const CURVE: usize = 0x18;
    const KINDS: [u32; 5] = [3, 2, 7, 6, 6];
    let v = &mut s.virtual_section;
    for (k, kind) in KINDS.iter().enumerate() {
        put(v, particle + CURVES + CURVE * k + 0x14, &kind.to_le_bytes());
    }
    let c = &curves[0];
    let times = v.len().next_multiple_of(16);
    v.resize(times + 0x20, 0);
    for (j, t) in c.times.iter().enumerate() {
        put(v, times + 2 * j, &t.to_le_bytes());
    }
    let values = v.len().next_multiple_of(16);
    v.resize(values + 2 * c.values.len(), 0);
    for (j, x) in c.values.iter().enumerate() {
        put(
            v,
            values + 2 * j,
            &crate::half::f32_to_half(x[0]).to_le_bytes(),
        );
    }
    put(
        v,
        particle + CURVES,
        &(0x5000_0000 + times as u64).to_le_bytes(),
    );
    put(
        v,
        particle + CURVES + 8,
        &(0x5000_0000 + values as u64).to_le_bytes(),
    );
    put(
        v,
        particle + CURVES + 0x10,
        &(c.times.len() as u32).to_le_bytes(),
    );
    for (t, table) in bake_particle_tables(curves).iter().enumerate() {
        for (r, row) in table.iter().enumerate() {
            for (l, h) in row.iter().enumerate() {
                put(&mut s.physical, 0x100 * t + 8 * r + 2 * l, &h.to_le_bytes());
            }
        }
    }
}

#[test]
fn a_duplicated_rules_curves_are_its_own_and_an_edit_leaves_the_source() {
    let (mut s, at) = with_buffers();
    let curves = sprite_curves();
    install_curves(&mut s, at.particle, &curves);
    let named = |s: &Rpf9Sections, name: &str| {
        let list = read_list(&s.virtual_section).expect("it reads");
        (list.particles.iter().find(|p| p.name == name).cloned()).expect("the rule")
    };
    let read = |s: &Rpf9Sections, rule: &ParticleRule| {
        read_particle_curves(&s.virtual_section, &s.physical, rule)
    };
    assert_eq!(
        read(&s, &named(&s, PARTICLE)).expect("source curves"),
        curves
    );

    duplicate_particle_rule(&mut s, 0, PARTICLE_COPY).expect("it copies");
    let (src, copy) = (named(&s, PARTICLE), named(&s, PARTICLE_COPY));
    assert_eq!(read(&s, &copy).expect("the copy's curves"), curves);

    // Edit the copy's first curve; the source re-reads unchanged, the copy reads the edit.
    let mut edited = curves.clone();
    edited[0].values[1][0] = 2.0;
    write_particle_curves(&mut s.virtual_section, &mut s.physical, &copy, &edited).expect("writes");
    assert_eq!(read(&s, &copy).expect("the copy reads its edit"), edited);
    assert_eq!(read(&s, &src).expect("the source still reads"), curves);
}

#[path = "particles_event_object_tests.rs"]
mod event_object;
