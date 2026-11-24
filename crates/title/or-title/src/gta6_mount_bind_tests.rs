//! A pack's mode, what its sets carry, and the binding of registered archives to maps by the
//! names of a table, over documents and tables made up here.

use super::tests::{change_set, content, document, map, one_pack, scanned_file, LEVEL};
use super::*;

/// A setup of a level pack whose device is `device`, with no group.
fn device_setup(device: &str) -> Document {
    document(&format!(
        "<SSetupData><deviceName>{device}</deviceName><nameHash>{device}</nameHash>\
         <type>{LEVEL}</type></SSetupData>"
    ))
}

/// A change set `id` that registers each file under its type, with what follows its file list.
fn typed_set(id: &str, files: &[(&str, &str)], after: &str) -> Document {
    let files: String = (files.iter())
        .map(|(name, kind)| {
            format!("<Item><filename>{name}</filename><fileType>{kind}</fileType></Item>")
        })
        .collect();
    document(&format!(
        "<CDataFileMgr__ContentChangeSetDef><id>{id}</id><fileList>{files}</fileList>{after}\
         </CDataFileMgr__ContentChangeSetDef>"
    ))
}

fn entry(path: &str, kind: u8, offset: u64, ciphered: bool) -> Entry {
    Entry {
        word: or_io::gta6::hash::name_hash64(path.as_bytes()),
        name: None,
        offset,
        stored: 16,
        stated: 16,
        kind,
        ciphered,
        chunks: 1,
    }
}

#[test]
fn a_pack_s_mode_is_read_off_its_device() {
    let mode = |device: &str| one_pack(vec![device_setup(device)]).mode;
    assert_eq!(mode("core"), Mode::Base);
    assert_eq!(mode("Story_Made_Up"), Mode::Story);
    assert_eq!(mode("story_made_up_more"), Mode::Story);
    assert_eq!(mode("shared_MINIGAME_one"), Mode::Minigames);
    assert_eq!(mode(""), Mode::Base);
    assert_eq!(
        [Mode::Base.bit(), Mode::Story.bit(), Mode::Minigames.bit()],
        [1, 2, 4]
    );
}

#[test]
fn a_set_carries_its_conditions_and_whether_it_changes_the_map() {
    let pack = one_pack(vec![
        device_setup("core"),
        typed_set(
            "town",
            &[("a.rpf", "RPF_FILE")],
            "<flags>MapChange</flags><executionConditions><activeChangesetConditions />\
             <genericConditions>$made=up</genericConditions></executionConditions>",
        ),
        typed_set("plain", &[], "<flags>Default</flags>"),
    ]);
    assert!(pack.sets[0].map_change());
    assert_eq!(
        pack.sets[0].conditions,
        Some(ExecutionConditions {
            active_changeset_conditions: String::new(),
            generic_conditions: "$made=up".to_string(),
        })
    );
    assert!(!pack.sets[1].map_change());
    assert_eq!(pack.sets[1].conditions, None);
}

#[test]
fn a_set_not_found_whose_file_the_table_lists_under_a_cipher_is_ciphered() {
    let table = Table::of_entries(vec![
        entry("changesets/ccs_closed.meta", KIND_FILE, 0x100, true),
        entry("changesets/ccs_town.meta", KIND_FILE, 0x200, false),
        entry(
            "changesets/ccs_read_all_the_same.meta",
            KIND_FILE,
            0x300,
            true,
        ),
    ]);
    let documents = vec![
        device_setup("core"),
        change_set("town", &[]),
        change_set("read_all_the_same", &[]),
        content(
            &[],
            &[],
            &[
                "$(root)/changesets/ccs_Closed.meta",
                "$(root)/changesets/ccs_absent.meta",
                "$(root)/changesets/ccs_town.meta",
                "$(root)/changesets/ccs_read_all_the_same.meta",
                "$(root)/changesets/ccs_closed.meta",
            ],
        ),
    ];
    let listed = Registry::listed([("one.rpf".into(), documents.clone(), Some(&table))]);
    assert_eq!(listed.packs[0].unread, ["closed", "absent"]);
    assert_eq!(listed.packs[0].ciphered, ["closed"]);
    // With no table nothing says why a set is not there.
    let scanned = Registry::of(vec![("one.rpf".into(), documents)]);
    assert_eq!(scanned.packs[0].unread, ["closed", "absent"]);
    assert_eq!(scanned.packs[0].ciphered, Vec::<String>::new());
}

/// A base pack and a story pack that each register archives, and the tables of their two archives.
fn two_packs() -> (Registry, Table, Table) {
    let rpf = "RPF_FILE";
    let registry = Registry::of(vec![
        (
            "root/common.rpf".into(),
            vec![
                device_setup("core"),
                typed_set(
                    "town",
                    &[
                        ("platform:/levels/town/DOCKS_metadata.rpf", rpf),
                        ("platform:/levels/town/docks.rpf", rpf),
                        ("platform:/levels/town/shared_metadata.rpf", rpf),
                        ("platform:/levels/town/gone.rpf", rpf),
                        ("platform:/levels/town/notes.meta", "TEXT_FILE"),
                        ("platform:/levels/props/crate.ityp", PERMANENT_TYPES),
                        ("platform:/levels/props/lost.ityp", PERMANENT_TYPES),
                    ],
                    "",
                ),
            ],
        ),
        (
            "root/pack/tale/tale.rpf".into(),
            vec![
                device_setup("story_tale"),
                typed_set(
                    "tale_town",
                    &[
                        ("$(target)/levels/town/pier_metadata.rpf", rpf),
                        ("$(target)/levels/town/shared_metadata.rpf", rpf),
                    ],
                    "",
                ),
            ],
        ),
    ]);
    let levels = Table::of_entries(vec![
        entry("levels/town/docks_metadata", 0, 0x1000, false),
        entry("levels/town/docks", 0, 0x2000, false),
        entry("levels/props/crate_metadata", 0, 0x3000, false),
        entry("levels/town/shared_metadata", 0, 0x5000, false),
    ]);
    let tale = Table::of_entries(vec![
        entry("levels/town/pier_metadata", 0, 0x1000, false),
        entry("levels/town/shared_metadata", 0, 0x7000, false),
    ]);
    (registry, levels, tale)
}

#[test]
fn a_registered_archive_is_bound_by_its_name_in_the_table_of_its_pack_s_folder_first() {
    let (registry, levels, tale) = two_packs();
    let tables = [
        Listed {
            archive: 0,
            path: Path::new("root/levels.rpf"),
            table: &levels,
        },
        Listed {
            archive: 1,
            path: Path::new("root/pack/tale/tale.rpf"),
            table: &tale,
        },
    ];
    let at = |archive, container| MapFact {
        archive,
        container,
        ..map(0, None, "a_map", "")
    };
    let maps = [
        at(0, Some(0x1000)),
        at(0, Some(0x2000)),
        at(0, Some(0x5000)),
        at(1, Some(0x1000)),
        at(1, Some(0x7000)),
        at(0, None),
        at(0, Some(0x9000)),
        at(0, Some(0x1000)),
    ];
    let binding = bind_by_table(&registry.packs, &tables, &maps);
    assert_eq!(binding.modes, [1, 1, 1, 2, 2, 0, 0, 1]);
    assert_eq!(
        (
            binding.named_archives,
            binding.listed_archives,
            binding.bound_archives
        ),
        (6, 5, 5)
    );
    // A permanent map-types file lies in the archive named after it, and one with none is counted.
    assert_eq!(binding.permanent, [(0, 0x3000)]);
    assert_eq!(binding.permanent_named, 2);
    assert_eq!(world_archives(&registry.packs, &tables), [0, 1]);
    // With the tale's table alone, the base pack's shared archive is found there.
    let alone = bind_by_table(&registry.packs, &tables[1..], &maps);
    assert_eq!(alone.modes, [0, 0, 0, 2, 3, 0, 0, 0]);
    assert_eq!(world_archives(&registry.packs, &tables[1..]), [1]);
    assert_eq!(bind_by_table(&registry.packs, &[], &maps).modes, [0; 8]);
}

#[test]
fn a_mode_s_game_shapes_are_the_archives_its_pack_registers_under_that_folder() {
    let rpf = "RPF_FILE";
    let registry = Registry::of(vec![
        (
            "root/common.rpf".into(),
            vec![
                device_setup("core"),
                typed_set(
                    "town",
                    &[
                        ("platform:/levels/town/GameShapes/towncommon.rpf", rpf),
                        ("platform:/levels/town/gameshapes/towncommon.rpf", rpf),
                        ("platform:/levels/town/docks_metadata.rpf", rpf),
                        ("platform:/levels/town/gameshapes/notes.meta", "TEXT_FILE"),
                        ("platform:/levels/town/gameshapes/absent.rpf", rpf),
                    ],
                    "",
                ),
            ],
        ),
        (
            "root/pack/tale/tale.rpf".into(),
            vec![
                device_setup("story_tale"),
                typed_set(
                    "tale_shapes",
                    &[("$(target)/levels/town/gameshapes/townstory.rpf", rpf)],
                    "",
                ),
            ],
        ),
    ]);
    let levels = Table::of_entries(vec![
        entry("levels/town/gameshapes/towncommon", 0, 0x1000, false),
        entry("levels/town/docks_metadata", 0, 0x2000, false),
    ]);
    let tale = Table::of_entries(vec![entry(
        "levels/town/gameshapes/townstory",
        0,
        0x3000,
        false,
    )]);
    let tables = [
        Listed {
            archive: 0,
            path: Path::new("root/levels.rpf"),
            table: &levels,
        },
        Listed {
            archive: 1,
            path: Path::new("root/pack/tale/tale.rpf"),
            table: &tale,
        },
    ];
    let mut found: Vec<(Mode, u32, u64)> = (game_shapes(&registry.packs, &tables).into_iter())
        .map(|(mode, archive, entry)| (mode, archive, entry.offset))
        .collect();
    found.sort_by_key(|(_, archive, _)| *archive);
    // One archive named twice is found once.
    assert_eq!(found, [(Mode::Base, 0, 0x1000), (Mode::Story, 1, 0x3000)]);
    assert!(game_shapes(&registry.packs, &[]).is_empty());
}

#[test]
fn a_map_is_held_back_by_no_registration_then_its_flag_then_holding_nothing() {
    let with = |flags, holds| MapFact {
        flags,
        holds,
        ..map(0, None, "pier", "")
    };
    assert_eq!(held_back(true, &with(0, true)), None);
    assert_eq!(
        held_back(false, &with(0, true)),
        Some(HeldBack::NotRegistered)
    );
    assert_eq!(
        held_back(false, &with(1, false)),
        Some(HeldBack::NotRegistered)
    );
    assert_eq!(
        held_back(true, &with(3, false)),
        Some(HeldBack::ScriptManaged)
    );
    assert_eq!(held_back(true, &with(2, false)), Some(HeldBack::Empty));
}

#[test]
fn the_documents_of_a_table_s_clear_deflated_files_are_read_where_it_puts_them() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let (path, set_at) = scanned_file(dir.path());
    let table = Table::of_entries(vec![
        entry("setup2.xml", KIND_FILE, 0, false),
        entry("changesets/ccs_town.meta", KIND_FILE, set_at, false),
        entry("changesets/ccs_closed.meta", KIND_FILE, set_at, true),
        entry("town/docks_metadata", table::KIND_ARCHIVE, set_at, false),
        Entry {
            chunks: 0,
            ..entry("stored/as_it_is.bk2", KIND_FILE, 0, false)
        },
        Entry {
            stated: BODY_LIMIT + 1,
            ..entry("far/too_long.xml", KIND_FILE, 0, false)
        },
    ]);
    let found = read_documents_by_table(&path, &table, &|| false).expect("read");
    let offsets: Vec<u64> = found.iter().map(|f| f.offset).collect();
    assert_eq!(offsets, [0, set_at]);
    assert!(matches!(&found[0].document, Document::Setup(setup) if setup.name_hash == "one"));
    assert!(matches!(&found[1].document, Document::ChangeSet(set) if set.id == "town"));
    let stopped = read_documents_by_table(&path, &table, &|| true).expect_err("stopped");
    assert_eq!(stopped.kind(), io::ErrorKind::Interrupted);
}
