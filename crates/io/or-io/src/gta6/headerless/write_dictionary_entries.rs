//! A frame filter or animation record dictionary's entries added or removed whole. An entry is
//! added as a copy of another under a new name: its key goes into the ascending key array and its
//! object's pointer to the same place, both arrays, the object, its name and its elements placed
//! at the section's end, and the arrays they replace zeroed. An entry is removed by taking its key
//! and pointer out of the arrays in place and zeroing its object.
//!
//! ⚠ A filter's signature follows from its elements by no rule anyone found, and the same element
//! bytes give the same signature, so a copy carries its source's signature and a record's word at
//! 0x68 with its source's bones. A removed entry's name and elements stay where they lie, unread.

use super::bytes::{sections, Bytes};
use super::write_filter_tracks::{is_filter, is_record, ElementsError};
use super::HeaderlessError;
use crate::container::rpf::{grow_section, Rpf9Section, Rpf9Sections};
use crate::gta6::hash::name_hash32;

/// Where the virtual section is addressed from.
const VIRTUAL_BASE: u64 = 0x5000_0000;
const FILTER_LEN: usize = 0x40;
const RECORD_LEN: usize = 0x70;
const FILTER_STRIDE: usize = 48;
const RECORD_STRIDE: usize = 144;

/// What a dictionary's root states, and every word of it a reader names a pointer.
struct Root {
    codes: Vec<u32>,
    entries: Vec<usize>,
    codes_at: usize,
    entries_at: usize,
    pointers: Vec<usize>,
}

fn refused(why: &'static str) -> ElementsError {
    ElementsError::Refused(why)
}

fn root_at(v: Bytes) -> Result<Root, HeaderlessError> {
    let not = HeaderlessError::NotInPlace;
    let n = usize::from(v.u16("code count", 0x28)?);
    let stated = [0x2A, 0x38, 0x3A].map(|at| v.u16("dictionary count", at));
    for count in stated {
        if usize::from(count?) != n {
            return Err(not(
                "the dictionary's code and entry counts are not each their capacity",
            ));
        }
    }
    if n == 0 {
        return Err(not("the dictionary holds no entry"));
    }
    let codes_at = v.to("codes", 0x20)?;
    let entries_at = v.to("entries", 0x30)?;
    let codes = (v.array("codes", codes_at, n, 4)?.as_chunks::<4>().0.iter())
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    let mut entries = Vec::with_capacity(n);
    let mut pointers = vec![0x08, 0x10, 0x20, 0x30];
    for i in 0..n {
        let slot = entries_at + 8 * i;
        pointers.push(slot);
        let entry = v.to("entry", slot)?;
        if is_filter(v, entry) {
            pointers.extend([entry + 0x18, entry + 0x28, entry + 0x38]);
        } else if is_record(v, entry) {
            pointers.push(entry + 0x20);
            // A name of 15 bytes or fewer lies inline, its length in its last byte.
            if v.u8("animation record name", entry + 0x67)? & 0x80 == 0 {
                pointers.push(entry + 0x58);
            }
        } else {
            return Err(not(
                "an entry of the dictionary is no frame filter or animation record",
            ));
        }
        entries.push(entry);
    }
    Ok(Root {
        codes,
        entries,
        codes_at,
        entries_at,
        pointers,
    })
}

fn root(virtual_section: &[u8]) -> Result<Root, ElementsError> {
    Ok(sections(virtual_section, None, |v, _| root_at(v))?)
}

fn put(v: &mut [u8], at: usize, bytes: &[u8]) {
    v[at..at + bytes.len()].copy_from_slice(bytes);
}

fn pointer(at: usize) -> [u8; 8] {
    (VIRTUAL_BASE + at as u64).to_le_bytes()
}

/// The entry at `at`: whether it is a frame filter, else an animation record, and its elements'
/// bytes.
fn entry_of(v: Bytes, at: usize) -> Result<(bool, Vec<u8>), HeaderlessError> {
    let filter = is_filter(v, at);
    let (count, field, stride) = match filter {
        true => (
            usize::from(v.u16("frame filter tracks", at + 0x30)?),
            0x28,
            FILTER_STRIDE,
        ),
        false => (
            v.u32("animation record bones", at + 0x18)? as usize,
            0x20,
            RECORD_STRIDE,
        ),
    };
    if count == 0 {
        return Ok((filter, Vec::new()));
    }
    let first = v.to("dictionary entry elements", at + field)?;
    Ok((
        filter,
        v.array("dictionary entry elements", first, count, stride)?
            .to_vec(),
    ))
}

/// The key a frame filter (`filter`) or animation record named `name` takes, and its name text.
// `pack:/<stem>.iff` for a filter, its stem hashed; a record's name itself.
pub fn entry_key(name: &str, filter: bool) -> Option<(u32, String)> {
    let printable = name.bytes().all(|b| (33..127).contains(&b));
    if name.is_empty() || name.len() >= usize::from(u16::MAX) - 16 || !printable {
        return None;
    }
    if !filter {
        return Some((name_hash32(name.as_bytes()), name.to_string()));
    }
    let text = match name.starts_with("pack:/") {
        true => name.to_string(),
        false => format!("pack:/{name}.iff"),
    };
    let stem = text.rsplit('/').next()?.split('.').next()?;
    (!stem.is_empty()).then(|| (name_hash32(stem.as_bytes()), text.clone()))
}

/// A copy of the frame filter's or animation record's entry of `code` added as `name`; its code.
pub fn copy_dictionary_entry(
    body: &mut Rpf9Sections,
    code: u32,
    name: &str,
) -> Result<u32, ElementsError> {
    let held = root(&body.virtual_section)?;
    let i = (held.codes.iter().position(|c| *c == code))
        .ok_or(refused("no entry of the dictionary has that code"))?;
    let (filter, elements) = sections(&body.virtual_section, None, |v, _| {
        entry_of(v, held.entries[i])
    })?;
    let (key, text) = entry_key(name, filter).ok_or(refused(
        "a name is printable ASCII text with no space, shorter than 65,519 bytes",
    ))?;
    if held.codes.contains(&key) {
        return Err(refused(
            "another entry of the dictionary has the key that name hashes to",
        ));
    }
    let n = held.codes.len() + 1;
    if n > usize::from(u16::MAX) {
        return Err(refused(
            "the dictionary would hold more entries than its count states",
        ));
    }
    let object_len = if filter { FILTER_LEN } else { RECORD_LEN };
    let inline = !filter && text.len() <= 15;
    let name_len = if inline {
        0
    } else {
        (text.len() + 1).next_multiple_of(16)
    };
    let codes_len = (4 * n).next_multiple_of(16);
    let entries_len = (8 * n).next_multiple_of(16);
    let by = codes_len + entries_len + object_len + name_len + elements.len();
    let end = body.virtual_section.len().next_multiple_of(16);
    body.virtual_section.resize(end, 0);
    let start = grow_section(body, &held.pointers, Rpf9Section::Virtual, end, by as isize)?;

    // The block map's slots may have grown, which moves what follows them: read the root again.
    let held = root(&body.virtual_section)?;
    let (codes_at, entries_at) = (start, start + codes_len);
    let object_at = entries_at + entries_len;
    let name_at = object_at + object_len;
    let elements_at = name_at + name_len;
    let v = &mut body.virtual_section;
    let source = held.entries[i];
    let object = (v.get(source..source + object_len))
        .ok_or(refused("the entry copied runs past the section"))?
        .to_vec();
    let place = held.codes.partition_point(|c| *c < key);
    let (mut codes, mut entries) = (held.codes.clone(), held.entries.clone());
    codes.insert(place, key);
    entries.insert(place, object_at);
    for (k, (c, e)) in codes.iter().zip(&entries).enumerate() {
        put(v, codes_at + 4 * k, &c.to_le_bytes());
        put(v, entries_at + 8 * k, &pointer(*e));
    }
    put(v, object_at, &object);
    put(v, elements_at, &elements);
    let elements_word = match elements.is_empty() {
        true => [0; 8],
        false => pointer(elements_at),
    };
    let len = text.len() as u16;
    let mut field = [0u8; 16];
    if inline {
        field[..text.len()].copy_from_slice(text.as_bytes());
        field[15] = 0x80 | len as u8;
    } else {
        put(v, name_at, text.as_bytes());
        field[..8].copy_from_slice(&pointer(name_at));
        field[8..10].copy_from_slice(&len.to_le_bytes());
        field[10..12].copy_from_slice(&(len + 1).to_le_bytes());
        field[12..16].copy_from_slice(&1u32.to_le_bytes());
    }
    match filter {
        true => {
            put(v, object_at + 0x18, &field);
            put(v, object_at + 0x28, &elements_word);
        }
        false => {
            put(v, object_at + 0x58, &field);
            put(v, object_at + 0x20, &elements_word);
        }
    }
    // The arrays the root no longer points at hold no word a later growth reads as a pointer.
    v[held.codes_at..held.codes_at + 4 * held.codes.len()].fill(0);
    v[held.entries_at..held.entries_at + 8 * held.entries.len()].fill(0);
    put(v, 0x20, &pointer(codes_at));
    put(v, 0x30, &pointer(entries_at));
    let count = (n as u16).to_le_bytes();
    for at in [0x28, 0x2A, 0x38, 0x3A] {
        put(v, at, &count);
    }
    Ok(key)
}

/// The entry of `code` taken out of its dictionary.
// A dictionary keeps one entry, since an empty one is another form.
pub fn remove_dictionary_entry(body: &mut Rpf9Sections, code: u32) -> Result<(), ElementsError> {
    let held = root(&body.virtual_section)?;
    let i = (held.codes.iter().position(|c| *c == code))
        .ok_or(refused("no entry of the dictionary has that code"))?;
    let n = held.codes.len();
    if n == 1 {
        return Err(refused(
            "a dictionary keeps an entry: one with none is another form",
        ));
    }
    let gone = held.entries[i];
    let filter = sections(&body.virtual_section, None, |v, _| Ok(is_filter(v, gone)))?;
    let object_len = if filter { FILTER_LEN } else { RECORD_LEN };
    if body.virtual_section.len() < gone + object_len {
        return Err(refused("the entry removed runs past the section"));
    }
    let (mut codes, mut entries) = (held.codes, held.entries);
    codes.remove(i);
    entries.remove(i);
    let v = &mut body.virtual_section;
    for (k, (c, e)) in codes.iter().zip(&entries).enumerate() {
        put(v, held.codes_at + 4 * k, &c.to_le_bytes());
        put(v, held.entries_at + 8 * k, &pointer(*e));
    }
    v[held.codes_at + 4 * (n - 1)..held.codes_at + 4 * n].fill(0);
    v[held.entries_at + 8 * (n - 1)..held.entries_at + 8 * n].fill(0);
    if !entries.contains(&gone) {
        v[gone..gone + object_len].fill(0);
    }
    let count = ((n - 1) as u16).to_le_bytes();
    for at in [0x28, 0x2A, 0x38, 0x3A] {
        put(v, at, &count);
    }
    Ok(())
}
