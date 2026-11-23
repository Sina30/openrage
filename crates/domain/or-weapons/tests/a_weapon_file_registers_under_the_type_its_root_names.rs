//! The type the game is told a weapon file is follows from the root the file states: over the `content.xml` of every pack of a mounted
//! install, each data file registered as a weapon type states the root `DATA_FILE_TYPES` pairs with it, and every row of the table is
//! one the install registers.
//!
//! Point `OPENRAGE_GAME_ROOT` at an install root. Without one this skips, which is not a pass.
//!
//! ⚠ The packs' registrations are read; the base game's list is the startup content list, whose reader belongs to the command line, so
//! the table is measured on the packs and the base game's `weapons.meta` is held to the same by its root.

mod common;

use std::collections::BTreeSet;

use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};
use or_weapons::{data_file_type, DATA_FILE_TYPES};

const WHAT: &str = "a_weapon_file_registers_under_the_type_its_root_names";

#[test]
fn a_weapon_file_registers_under_the_type_its_root_names() {
    let Some(root) = game_roots().into_iter().next() else {
        skip_install(WHAT, format!("{GAME_ROOT_ENV} names no install"));
        return;
    };
    let Some(game) = common::mount(&root) else {
        skip_install(WHAT, "the install could not be mounted");
        return;
    };
    let ours: BTreeSet<&str> = DATA_FILE_TYPES.iter().map(|(_, ty)| *ty).collect();
    let (mut observed, mut wrong, mut read) = (BTreeSet::new(), Vec::new(), 0usize);
    for pack in &game.packs {
        for entry in &pack.data_files {
            if !ours.contains(entry.file_type.as_str()) {
                continue;
            }
            let Some(bytes) = game.vfs.open(&entry.filename) else {
                continue;
            };
            read += 1;
            let Some(document_root) = common::root_of(&bytes) else {
                wrong.push(format!("{} is no XML document", entry.filename));
                continue;
            };
            if data_file_type(&document_root) != Some(entry.file_type.as_str()) {
                wrong.push(format!(
                    "{} is registered as {} and states the root {document_root}",
                    entry.filename, entry.file_type
                ));
            }
            observed.insert((document_root, entry.file_type.clone()));
        }
    }
    assert!(
        read > 50,
        "the packs register {read} weapon data files, which is too few to be the install's"
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    let table: BTreeSet<(String, String)> = DATA_FILE_TYPES
        .iter()
        .map(|(root, ty)| (root.to_string(), ty.to_string()))
        .collect();
    assert_eq!(
        observed, table,
        "the table pairs each root with the one type the packs register it under, and no other pair is registered"
    );
}
