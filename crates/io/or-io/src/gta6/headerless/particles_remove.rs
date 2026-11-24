//! Rules, events and behaviours taken out of a particle effects list in place: each array that
//! loses an element is written shorter where it stands, what was taken out stays as bytes nothing
//! points at, and every word derived from what remains is derived again.

use std::collections::{BTreeMap, BTreeSet};

use super::*;

/// The classes every rule lists: `ptxu_Age`, `ptxu_Velocity`, `ptxu_Size` and `ptxu_Colour`.
const EVERY_RULE: u32 = (1 << 1) | (1 << 4) | (1 << 7) | (1 << 19);
/// The root's pointers to the effect, emitter and particle rule dictionaries.
const DICTIONARIES: [usize; 3] = [0x28, 0x30, 0x38];
/// The root's set of the drawables' hashes and its set of the mesh domains' model hashes.
const DRAWABLE_SET: usize = 0x40;
const MESH_SET: usize = 0x58;
/// The word that addresses virtual offset 0.
const VIRTUAL: u64 = 0x5000_0000;

/// What to take out of a list, each by its place in the list as read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParticleRemovals {
    /// Indices into the list's effect rules.
    pub effects: BTreeSet<usize>,
    /// Indices into the list's emitter rules.
    pub emitters: BTreeSet<usize>,
    /// Indices into the list's particle rules.
    pub particles: BTreeSet<usize>,
    /// An effect rule's index and an event's place in its timeline.
    pub events: BTreeSet<(usize, usize)>,
    /// A particle rule's index and a behaviour's place in its list.
    pub behaviours: BTreeSet<(usize, usize)>,
}

impl ParticleRemovals {
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
            && self.emitters.is_empty()
            && self.particles.is_empty()
            && self.events.is_empty()
            && self.behaviours.is_empty()
    }
}

/// Bytes to write, each at its section offset, in order.
type Patches = Vec<(usize, Vec<u8>)>;

/// `removals` taken out of the list in `virtual_section` in place, or refused, writing nothing.
pub fn remove_from_particle_effects(
    virtual_section: &mut [u8],
    removals: &ParticleRemovals,
) -> Result<()> {
    let stored = read_particle_effects(virtual_section, None)?;
    let expected = after(&stored, removals)?;
    let patches = sections(virtual_section, None, |v, _| {
        plan(&v, &stored, &expected, removals)
    })?;
    let mut out = virtual_section.to_vec();
    for (at, bytes) in patches {
        match out.get_mut(at..at + bytes.len()) {
            Some(slot) => slot.copy_from_slice(&bytes),
            None => return refuse("a word to write is past the section's end"),
        }
    }
    let read = read_particle_effects(&out, None)?;
    need(
        sizes_cleared(read) == sizes_cleared(expected),
        "taking it out changes how what remains reads, such as the behaviour before a removed one",
    )?;
    virtual_section.copy_from_slice(&out);
    Ok(())
}

/// `list` with no behaviour's size, which a removed neighbour changes.
pub(super) fn sizes_cleared(mut list: ParticleEffects) -> ParticleEffects {
    for b in &mut list.behaviours {
        b.size = 0;
    }
    list
}

/// Each index of `len` to its index once `gone` is out, `None` for one of `gone`.
fn renumber(len: usize, gone: &BTreeSet<usize>) -> Vec<Option<usize>> {
    let mut next = 0;
    (0..len)
        .map(|i| {
            if gone.contains(&i) {
                return None;
            }
            next += 1;
            Some(next - 1)
        })
        .collect()
}

/// The list `stored` reads as once `r` is out, refused where what is kept names what is not.
fn after(stored: &ParticleEffects, r: &ParticleRemovals) -> Result<ParticleEffects> {
    let within = |set: &BTreeSet<usize>, len: usize| set.iter().all(|&i| i < len);
    need(
        within(&r.effects, stored.effects.len())
            && within(&r.emitters, stored.emitters.len())
            && within(&r.particles, stored.particles.len()),
        "a rule to remove is not one of the list",
    )?;
    for &(e, i) in &r.events {
        need(
            stored.effects.get(e).is_some_and(|e| i < e.events.len()),
            "an event to remove is not one of the list",
        )?;
    }
    for &(p, i) in &r.behaviours {
        need(
            stored
                .particles
                .get(p)
                .is_some_and(|p| i < p.behaviours.len()),
            "a behaviour to remove is not one a rule lists",
        )?;
    }
    let emitter_at = renumber(stored.emitters.len(), &r.emitters);
    let particle_at = renumber(stored.particles.len(), &r.particles);
    let effect_at = renumber(stored.effects.len(), &r.effects);
    let mut effects = Vec::new();
    for (e, rule) in stored.effects.iter().enumerate() {
        if r.effects.contains(&e) {
            continue;
        }
        let mut rule = rule.clone();
        let mut events = Vec::new();
        for (i, ev) in rule.events.iter().enumerate() {
            if r.events.contains(&(e, i)) {
                continue;
            }
            let emitter = emitter_at.get(ev.emitter).copied().flatten();
            let particle = particle_at.get(ev.particle).copied().flatten();
            let (Some(emitter), Some(particle)) = (emitter, particle) else {
                return refuse("an event names a rule removed: remove the event too");
            };
            events.push(Event {
                emitter,
                particle,
                ..ev.clone()
            });
        }
        rule.events = events;
        effects.push(rule);
    }
    let emitters: Vec<EmitterRule> = (stored.emitters.iter().enumerate())
        .filter(|(i, _)| !r.emitters.contains(i))
        .map(|(_, rule)| rule.clone())
        .collect();
    let mut particles = Vec::new();
    let mut listed = BTreeSet::new();
    for (p, rule) in stored.particles.iter().enumerate() {
        if r.particles.contains(&p) {
            continue;
        }
        let mut rule = rule.clone();
        for s in &mut rule.spawners {
            if let Some(e) = s.effect {
                s.effect = effect_at.get(e).copied().flatten();
                need(
                    s.effect.is_some(),
                    "a particle rule's spawner starts an effect removed",
                )?;
            }
        }
        let bits: Vec<u8> = (0..32u8)
            .filter(|&b| (rule.class_bits >> b) & 1 == 1)
            .collect();
        let mut kept = Vec::new();
        for (place, &b) in rule.behaviours.iter().enumerate() {
            if !r.behaviours.contains(&(p, place)) {
                kept.push(b);
                continue;
            }
            let Some(&bit) = bits.get(place) else {
                return refuse(
                    "a rule's class word does not set one bit for each behaviour it lists",
                );
            };
            need(
                (EVERY_RULE >> bit) & 1 == 0,
                "every rule lists ptxu_Age, ptxu_Velocity, ptxu_Size and ptxu_Colour, so none of them is removed",
            )?;
            rule.class_bits &= !(1u32 << bit);
        }
        listed.extend(kept.iter().copied());
        rule.behaviours = kept;
        particles.push(rule);
    }
    let order: Vec<usize> = listed.into_iter().collect();
    let behaviours: Vec<Behaviour> = (order.iter())
        .filter_map(|&b| stored.behaviours.get(b).cloned())
        .collect();
    need(
        behaviours.len() == order.len(),
        "a rule lists a behaviour the list does not hold",
    )?;
    for rule in &mut particles {
        rule.behaviours = (rule.behaviours.iter())
            .map(|b| order.binary_search(b).unwrap_or_default())
            .collect();
    }
    let drawable_hashes = (particles.iter())
        .flat_map(|p| p.drawables.iter().map(|d| d.hash))
        .collect();
    let mesh_hashes = (emitters.iter())
        .flat_map(|e| e.domains.iter().flatten())
        .filter_map(|d| d.model.as_ref())
        .filter(|(model, _)| !model.is_empty())
        .map(|(model, _)| name_hash32(model.as_bytes()))
        .collect();
    Ok(ParticleEffects {
        name: stored.name.clone(),
        registry: stored.registry.clone(),
        effects,
        emitters,
        particles,
        behaviours,
        drawable_hashes,
        mesh_hashes,
    })
}

fn word(out: &mut Patches, at: usize, w: u32) {
    out.push((at, w.to_le_bytes().to_vec()));
}

/// The writes that take `r` out of `stored`, which then reads as `after` but for sizes.
fn plan(
    v: &Bytes,
    stored: &ParticleEffects,
    after: &ParticleEffects,
    r: &ParticleRemovals,
) -> Result<Patches> {
    let mut out = Patches::new();
    for (slot, gone) in DICTIONARIES
        .into_iter()
        .zip([&r.effects, &r.emitters, &r.particles])
    {
        if gone.is_empty() {
            continue;
        }
        let d = v.to("dictionary", slot)?;
        shorten(v, d + 0x20, 4, gone, &mut out)?;
        shorten(v, d + 0x30, 8, gone, &mut out)?;
    }
    for (e, rule) in stored.effects.iter().enumerate() {
        let gone: BTreeSet<usize> = (r.events.iter())
            .filter(|(i, _)| *i == e)
            .map(|(_, j)| *j)
            .collect();
        if !gone.is_empty() && !r.effects.contains(&e) {
            events(v, rule.at as usize + 0x38, &gone, &mut out)?;
        }
    }
    let fired: Vec<&Event> = after.effects.iter().flat_map(|e| &e.events).collect();
    for (i, rule) in after.emitters.iter().enumerate() {
        let n = fired.iter().filter(|ev| ev.emitter == i).count();
        word(&mut out, rule.at as usize + 0x10, 1 + n as u32);
    }
    for (i, rule) in after.particles.iter().enumerate() {
        let n = fired.iter().filter(|ev| ev.particle == i).count();
        word(&mut out, rule.at as usize + 0x10, 1 + n as u32);
    }
    for (p, rule) in stored.particles.iter().enumerate() {
        let gone: BTreeSet<usize> = (r.behaviours.iter())
            .filter(|(i, _)| *i == p)
            .map(|(_, j)| *j)
            .collect();
        if gone.is_empty() || r.particles.contains(&p) {
            continue;
        }
        let at = rule.at as usize;
        shorten(v, at + 0x188, 8, &gone, &mut out)?;
        let Some(kept) = after.particles.iter().find(|q| q.at == rule.at) else {
            return refuse("a rule whose behaviour is removed is not in the list");
        };
        for (o, w) in super::bits::class_words(kept.class_bits) {
            word(&mut out, at + o, w);
        }
    }
    if after.drawable_hashes != stored.drawable_hashes {
        set(v, DRAWABLE_SET, &after.drawable_hashes, &mut out)?;
    }
    if after.mesh_hashes != stored.mesh_hashes {
        set(v, MESH_SET, &after.mesh_hashes, &mut out)?;
    }
    Ok(out)
}

/// The array at `at` of `stride`-byte elements written without those at `gone`, where it
/// stands; a capacity equal to the count stays equal to it.
fn shorten(
    v: &Bytes,
    at: usize,
    stride: usize,
    gone: &BTreeSet<usize>,
    out: &mut Patches,
) -> Result<()> {
    const WHAT: &str = "array";
    let (to, count, capacity) = array(v, WHAT, at, false)?;
    let held = v.get(WHAT, to, stride * count)?;
    let kept: Vec<u8> = (held.chunks_exact(stride).enumerate())
        .filter(|(i, _)| !gone.contains(i))
        .flat_map(|(_, e)| e.iter().copied())
        .collect();
    let n = kept.len() / stride;
    if !kept.is_empty() {
        out.push((to, kept));
    }
    let capacity = if capacity == count { n } else { capacity };
    out.push((at + 8, (n as u16).to_le_bytes().to_vec()));
    out.push((at + 10, (capacity as u16).to_le_bytes().to_vec()));
    Ok(())
}

/// The event array at `at` written without the events at `gone`, each kept event numbered by its
/// new place.
fn events(v: &Bytes, at: usize, gone: &BTreeSet<usize>, out: &mut Patches) -> Result<()> {
    let (to, count, _) = array(v, "event", at, false)?;
    let kept = (0..count).filter(|i| !gone.contains(i));
    for (place, i) in kept.enumerate() {
        if place != i {
            let e = v.to("event", to + 8 * i)?;
            word(out, e + 8, place as u32);
        }
    }
    shorten(v, at, 8, gone, out)
}

/// The set at `at` holding only `keys`, some of what it holds, its kept nodes chained again in
/// the smallest slot count above their number.
fn set(v: &Bytes, at: usize, keys: &BTreeSet<u32>, out: &mut Patches) -> Result<()> {
    const WHAT: &str = "set";
    let (slots, used) = (
        v.u32(WHAT, at + 8)? as usize,
        v.u32(WHAT, at + 0x10)? as usize,
    );
    let Some(buckets) = v.ptr(WHAT, at)? else {
        return need(keys.is_empty(), "a set to keep keys in has no buckets");
    };
    v.get(WHAT, buckets, 8 * slots)?;
    let mut nodes = BTreeMap::new();
    let mut hops = 0;
    for slot in 0..slots {
        let mut node = v.ptr(WHAT, buckets + 8 * slot)?;
        while let Some(n) = node {
            hops += 1;
            need(hops <= used, "a map's chains are longer than its key count")?;
            nodes.insert(v.u32(WHAT, n)?, n);
            node = v.ptr(WHAT, n + 0x10)?;
        }
    }
    need(
        keys.iter().all(|k| nodes.contains_key(k)),
        "a set would gain a key, which needs room past the section's end",
    )?;
    out.push((buckets, vec![0; 8 * slots]));
    if keys.is_empty() {
        out.push((at, vec![0; 8]));
        word(out, at + 8, 0);
        word(out, at + 0x10, 0);
        return Ok(());
    }
    let Some(slots) = MAP_SLOTS.into_iter().find(|&n| n > keys.len()) else {
        return refuse("a set holds more keys than its slot counts reach");
    };
    let mut heads = vec![0u64; slots];
    for (key, node) in keys.iter().filter_map(|k| Some((*k, *nodes.get(k)?))) {
        let Some(head) = heads.get_mut(key as usize % slots) else {
            continue;
        };
        out.push((node + 0x10, head.to_le_bytes().to_vec()));
        *head = VIRTUAL + node as u64;
    }
    let heads: Vec<u8> = heads.iter().flat_map(|h| h.to_le_bytes()).collect();
    out.push((buckets, heads));
    word(out, at + 8, slots as u32);
    word(out, at + 0x10, keys.len() as u32);
    Ok(())
}
