//! Keys added to a particle effects list's keyframed values: each value that gains keys is given an
//! array of its own past the virtual section's end, its count equal to its capacity, and its
//! pointer aimed there. Nothing already in the section moves, so the words no reader types stay
//! valid, and the array it held stays as bytes nothing points at.

use super::*;
use crate::container::rpf::{grow_section, Rpf9GrowError, Rpf9Section, Rpf9Sections};

/// The word that addresses virtual offset 0.
const VIRTUAL: u64 = 0x5000_0000;
/// A key's bytes: its time's four lanes, then its value's.
const KEY: usize = 0x20;

/// Why keys are not added to a particle effects list.
#[derive(Debug, thiserror::Error)]
pub enum ParticleKeysError {
    #[error(transparent)]
    Read(#[from] HeaderlessError),
    #[error(transparent)]
    Grow(#[from] Rpf9GrowError),
}

/// The keyframed values `edited` gives more keys than `body` holds, written past its end.
// Refused, changing nothing, where a list cannot take them.
pub fn add_particle_keys(
    body: &mut Rpf9Sections,
    edited: &ParticleEffects,
) -> std::result::Result<(), ParticleKeysError> {
    let stored = read_particle_effects(&body.virtual_section, None)?;
    let edited = writer::derived(&stored, edited);
    let wanted = sections(&body.virtual_section, None, |v, _| {
        added(&v, &stored, &edited)
    })?;
    if wanted.is_empty() {
        return Ok(());
    }
    let mut by = 0;
    for (_, k) in &wanted {
        need(
            k.entries.len() <= usize::from(u16::MAX),
            "a keyframed value would hold more keys than its count can state",
        )?;
        by += KEY * k.entries.len();
    }
    let mut grown = body.clone();
    let end = grown.virtual_section.len().next_multiple_of(16);
    grown.virtual_section.resize(end, 0);
    let start = grow_section(&mut grown, &[0x08], Rpf9Section::Virtual, end, by as isize)?;
    // The block map's slots may have grown, which moves what follows them: read the list again.
    let moved = read_particle_effects(&grown.virtual_section, None)?;
    need(
        remover::sizes_cleared(moved.clone()) == remover::sizes_cleared(stored),
        "growing the section changes how what it holds reads, such as its last behaviour",
    )?;
    let places = sections(&grown.virtual_section, None, |v, _| {
        added(&v, &moved, &edited)
    })?;
    let v = &mut grown.virtual_section;
    let mut at = start;
    for (place, k) in &places {
        let keys: Vec<u8> = (k.entries.iter())
            .flat_map(|e| e.time.into_iter().chain(e.value))
            .flat_map(f32::to_le_bytes)
            .collect();
        let n = (k.entries.len() as u16).to_le_bytes();
        put(v, at, &keys)?;
        put(v, place + 8, &(VIRTUAL + at as u64).to_le_bytes())?;
        put(v, place + 0x10, &[n, n].concat())?;
        at += keys.len();
    }
    let mut expected = moved;
    let mut given = edited;
    for (held, wants) in every(&mut expected).into_iter().zip(every(&mut given)) {
        if wants.entries.len() > held.entries.len() {
            *held = wants.clone();
        }
    }
    need(
        read_particle_effects(&grown.virtual_section, None)? == expected,
        "the keys added do not read back as given, or change what else the list holds",
    )?;
    *body = grown;
    Ok(())
}

fn put(v: &mut [u8], at: usize, bytes: &[u8]) -> Result<()> {
    match v.get_mut(at..at + bytes.len()) {
        Some(slot) => {
            slot.copy_from_slice(bytes);
            Ok(())
        }
        None => refuse("a key to write is past the section's end"),
    }
}

/// `(at, edited)` into `out` where `edited` holds more keys than `held`, the value at `at`.
fn more(out: &mut Vec<(usize, Kfp)>, at: usize, held: &Kfp, edited: &Kfp) {
    if edited.entries.len() > held.entries.len() {
        out.push((at, edited.clone()));
    }
}

/// The place of each keyframed value of `list` that `edited` gives more keys, with its keys.
fn added(v: &Bytes, list: &ParticleEffects, edited: &ParticleEffects) -> Result<Vec<(usize, Kfp)>> {
    need(
        list.effects.len() == edited.effects.len()
            && list.emitters.len() == edited.emitters.len()
            && list.behaviours.len() == edited.behaviours.len(),
        "a rule or a behaviour was added or removed, so no keyframed value is matched to its keys",
    )?;
    let mut out = Vec::new();
    for (s, e) in list.effects.iter().zip(&edited.effects) {
        for (i, (sk, ek)) in s.kfps.iter().zip(&e.kfps).enumerate() {
            more(&mut out, s.at as usize + 0xC0 + KFP * i, sk, ek);
        }
    }
    for (s, e) in list.emitters.iter().zip(&edited.emitters) {
        let at = s.at as usize;
        for (i, (sk, ek)) in s.kfps.iter().zip(&e.kfps).enumerate() {
            more(&mut out, at + 0x48 + KFP * i, sk, ek);
        }
        for (kind, (sd, ed)) in s.domains.iter().zip(&e.domains).enumerate() {
            let (Some(sd), Some(ed)) = (sd, ed) else {
                continue;
            };
            let d = v.to("domain", at + 0x30 + 8 * kind)?;
            for (i, (sk, ek)) in sd.kfps.iter().zip(&ed.kfps).enumerate() {
                more(&mut out, d + 8 + KFP * i, sk, ek);
            }
        }
    }
    for (s, e) in list.behaviours.iter().zip(&edited.behaviours) {
        let places = writer::kfp_places(v, s)?;
        need(
            places.len() == s.kfps.len() && e.kfps.len() == s.kfps.len(),
            "a behaviour's keyframed values are not the ones the reader finds",
        )?;
        for ((at, sk), ek) in places.into_iter().zip(&s.kfps).zip(&e.kfps) {
            more(&mut out, at, sk, ek);
        }
    }
    Ok(out)
}

/// Every keyframed value of `list` in one order: the effects', the emitters' with their domains',
/// then the behaviours'.
fn every(list: &mut ParticleEffects) -> Vec<&mut Kfp> {
    let mut out: Vec<&mut Kfp> = Vec::new();
    for e in &mut list.effects {
        out.extend(e.kfps.iter_mut());
    }
    for e in &mut list.emitters {
        out.extend(e.kfps.iter_mut());
        for d in e.domains.iter_mut().flatten() {
            out.extend(d.kfps.iter_mut());
        }
    }
    for b in &mut list.behaviours {
        out.extend(b.kfps.iter_mut());
    }
    out
}
