//! Every weapon file of a mounted install opens as a typed root, saves as OpenRage's canonical text, loses nothing the file stated, and saves that text to the same bytes.
//!
//! Point `OPENRAGE_GAME_ROOT` at an install root. Without one this skips, which is not a pass: the staged corpus holds
//! no weapon file, so the install is where the family is measured.
//!
//! ⚠ "Loses nothing" is read off the two texts and not off the typed values: both are flattened to every element the
//! file states, with its place among the items of its parents, its attributes and its text, and the two lists are
//! compared, numbers by value, so a member the family cannot name must still arrive as it was.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use or_io::decl::File;
use or_io::meta::typed::weapon::{
    CWeaponComponentInfoBlob, CWeaponInfoBlob, CWeaponModelInfoInitDataList,
};
use or_io::meta::typed::weapon_types;
use or_io::meta::{MetaDocument, MetaInstance, Value};
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

const WHAT: &str = "the_weapon_metadata_of_the_install_reaches_a_fixed_point";

/// The files that went through one root: how many, and the bytes they came to.
#[derive(Default)]
struct Tally {
    files: usize,
    bytes: usize,
}

/// Open `bytes` as `T`, save, open and save again, and say what went wrong, if anything.
fn through<T: File>(bytes: &[u8]) -> Result<(), String> {
    let first = T::from_bytes(bytes).map_err(|e| format!("does not open: {e}"))?;
    let saved = first
        .to_bytes()
        .map_err(|e| format!("does not save: {e}"))?;
    let lost = common::lost(bytes, &saved);
    if !lost.is_empty() {
        return Err(format!(
            "loses {} element(s) between the file and its canonical text: {}",
            lost.len(),
            lost.iter().take(4).cloned().collect::<Vec<_>>().join("; ")
        ));
    }
    let again = T::from_bytes(&saved)
        .map_err(|e| format!("its canonical text does not open: {e}"))?
        .to_bytes()
        .map_err(|e| format!("its canonical text does not save: {e}"))?;
    if again != saved {
        return Err("saving its canonical text again moves it".to_string());
    }
    Ok(())
}

#[test]
fn every_weapon_file_of_the_install_opens_saves_and_arrives_as_it_was() {
    let Some(root) = game_roots().into_iter().next() else {
        skip_install(WHAT, format!("{GAME_ROOT_ENV} names no install"));
        return;
    };
    let Some(game) = common::mount(&root) else {
        skip_install(WHAT, "the install could not be mounted");
        return;
    };
    let files = common::weapon_files(&game);
    assert!(
        files.len() > 100,
        "the install lists {} weapon files, which is too few to be the install's",
        files.len()
    );
    let (mut tallies, mut wrong): (BTreeMap<&str, Tally>, Vec<String>) = Default::default();
    let mut binary: Vec<String> = Vec::new();
    for (logical, bytes) in &files {
        let result = match common::root_of(bytes).as_deref() {
            Some("CWeaponInfoBlob") => through::<CWeaponInfoBlob>(bytes),
            Some("CWeaponComponentInfoBlob") => through::<CWeaponComponentInfoBlob>(bytes),
            Some("CWeaponModelInfo__InitDataList") => {
                through::<CWeaponModelInfoInitDataList>(bytes)
            }
            // ⚠ A file that is not XML text is a binary document, which the family does not read: none may hide here.
            None => {
                binary.push(logical.clone());
                continue;
            }
            // The animations, loadouts, pickups and shops are not weapons' own: they are references.
            Some(_) => continue,
        };
        let root = common::root_of(bytes).expect("the root was named above");
        let tally = tallies.entry(root_name(&root)).or_default();
        tally.files += 1;
        tally.bytes += bytes.len();
        if let Err(why) = result {
            wrong.push(format!("{logical} {why}"));
        }
    }
    assert!(
        binary.is_empty(),
        "{} weapon files are not XML text, which the family does not read:
  {}",
        binary.len(),
        binary.join(
            "
  "
        )
    );
    assert!(
        wrong.is_empty(),
        "{} of {} weapon files do not arrive as they were:\n  {}",
        wrong.len(),
        tallies.values().map(|t| t.files).sum::<usize>(),
        wrong.join("\n  ")
    );
    // Each class and member the family declares is one the install states: nothing is declared that was not measured.
    let stated = stated_members(&files);
    let mut unmeasured = Vec::new();
    for (class, def) in &weapon_types::declared().structs {
        let seen = stated.get(&or_io::meta::binary::synth::name_hash(class));
        for field in &def.fields {
            if !seen.is_some_and(|members| members.contains(&field.name)) {
                unmeasured.push(format!("{class}::{}", field.name));
            }
        }
    }
    assert!(
        unmeasured.is_empty(),
        "{} declared members are stated nowhere in the install:
  {}",
        unmeasured.len(),
        unmeasured.join(
            "
  "
        )
    );
    for (root, tally) in &tallies {
        assert!(
            tally.files > 0,
            "the install holds no `{root}` file, so the family was not measured on one"
        );
    }
    eprintln!(
        "{}",
        tallies
            .iter()
            .map(|(root, t)| format!("{root}: {} files, {} bytes", t.files, t.bytes))
            .collect::<Vec<_>>()
            .join("; ")
    );
}

/// The root's name as the three roots spell it.
fn root_name(root: &str) -> &'static str {
    match root {
        "CWeaponInfoBlob" => "CWeaponInfoBlob",
        "CWeaponComponentInfoBlob" => "CWeaponComponentInfoBlob",
        _ => "CWeaponModelInfo__InitDataList",
    }
}

/// The members the install states for each class, by the hash of the class's name: what the documents say, read through the family's own declaration.
fn stated_members(files: &[(String, Vec<u8>)]) -> BTreeMap<u32, BTreeSet<String>> {
    fn visit(inst: &MetaInstance, into: &mut BTreeMap<u32, BTreeSet<String>>) {
        let members = into
            .entry(or_io::meta::binary::synth::name_hash(&inst.type_name))
            .or_default();
        members.extend(inst.fields.iter().map(|(name, _)| name.clone()));
        for (_, value) in &inst.fields {
            value_of(value, into);
        }
    }
    fn value_of(value: &Value, into: &mut BTreeMap<u32, BTreeSet<String>>) {
        match value {
            Value::Struct(inst) | Value::Pointer(Some(inst)) => visit(inst, into),
            Value::Array(items) => items.iter().for_each(|item| value_of(item, into)),
            _ => {}
        }
    }
    let library = weapon_types::library(or_io::meta::default_library());
    let mut into = BTreeMap::new();
    for (_, bytes) in files {
        if let Ok(doc) = MetaDocument::parse(&library, bytes.clone()) {
            if weapon_types::is_text_type(&doc.instance.type_name) {
                visit(&doc.instance, &mut into);
            }
        }
    }
    into
}
