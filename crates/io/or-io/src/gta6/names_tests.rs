use super::*;

fn harvested(body: &[u8], prefix: usize) -> Vec<(String, Option<[u8; 2]>)> {
    let mut found = Vec::new();
    harvest(body, prefix, &mut |h| {
        found.push((h.name.to_string(), h.kind))
    });
    found
}

fn names(found: &[(String, Option<[u8; 2]>)]) -> Vec<&str> {
    found.iter().map(|(name, _)| name.as_str()).collect()
}

/// A made-up binary body: structure bytes around NUL-ended strings.
fn binary_body() -> Vec<u8> {
    let mut body = vec![0u8, 0, 0, 0, 1, 2, 3, 0xFF];
    for part in [
        &b"Zq_Shed_07.#dr\0"[..],
        b"\x01\x02",
        b"zq_crane.#FT\0",
        b"zq:/props/zq_lamp_02.#td\0",
        b"\xFE",
        b"zq_plain_name\0",
        // No NUL ends it: chance bytes, not a string.
        b"zq_cut\x01",
        // A sentence is no name, but the resource it mentions is one.
        b"see zq_door.#dd here\0",
        // Not NUL-ended, and still a resource name.
        b"zq_loose.#bn\x02",
        b"ab\0",
        b"1234\0",
    ] {
        body.extend_from_slice(part);
    }
    body
}

#[test]
fn a_binary_body_gives_its_resource_names_and_its_nul_ended_plain_names() {
    let found = harvested(&binary_body(), HARVEST_PREFIX);
    assert_eq!(
        found,
        [
            ("Zq_Shed_07", Some(*b"dr")),
            ("zq_crane", Some(*b"ft")),
            ("props", None),
            ("zq_lamp_02", Some(*b"td")),
            ("zq_plain_name", None),
            ("zq_door", Some(*b"dd")),
            ("zq_loose", Some(*b"bn")),
        ]
        .map(|(name, kind)| (name.to_string(), kind))
    );
}

#[test]
fn a_text_body_gives_every_token_that_reads_as_a_name() {
    let body = b"<Item>\r\n  <name>zq_tower</name>\n  <filename>zq/zq_tower.#dd</filename>\n  <n>12</n>\n</Item>";
    let found = harvested(body, HARVEST_PREFIX);
    assert_eq!(
        names(&found),
        ["Item", "name", "zq_tower", "name", "filename", "zq_tower", "filename", "Item"]
    );
    assert_eq!(found[5].1, Some(*b"dd"));
    assert!(found.iter().filter(|(_, kind)| kind.is_some()).count() == 1);
}

#[test]
fn only_the_prefix_is_read_and_a_name_it_cuts_is_not_made_up() {
    let body = binary_body();
    let whole = harvested(&body, usize::MAX);
    for prefix in 0..=body.len() {
        let found = harvested(&body, prefix);
        // Each name of a prefix is a name of the whole body, in the same order.
        let mut rest = whole.iter();
        for name in &found {
            assert!(rest.any(|w| w == name), "{prefix}: {name:?} of {found:?}");
        }
    }
    assert!(harvested(&body, 8).is_empty());
    assert!(harvested(&[], HARVEST_PREFIX).is_empty());
    // The ending cut after one character names no kind.
    assert!(harvested(b"\0zq_shed_07.#d", HARVEST_PREFIX).is_empty());
}

#[test]
fn a_name_longer_than_the_bound_and_an_ending_that_is_not_two_characters_are_passed_over() {
    let long = "q".repeat(MAX_NAME + 1);
    let fits = "q".repeat(MAX_NAME);
    let body = format!("{long}.#dr {fits}.#dr zq_a.#drx zq_b.#d zq_c.# zq_d.#dr.zq_e.#ft");
    let found = harvested(body.as_bytes(), HARVEST_PREFIX);
    let kinds: Vec<_> = (found.iter())
        .filter(|(_, kind)| kind.is_some())
        .map(|(name, kind)| (name.as_str(), kind.unwrap()))
        .collect();
    assert_eq!(
        kinds,
        [(fits.as_str(), *b"dr"), ("zq_d", *b"dr"), ("zq_e", *b"ft")]
    );
    assert!(!names(&found).contains(&long.as_str()));
}

#[test]
fn a_name_splits_into_its_stem_and_its_kind() {
    assert_eq!(split_kind("zq_shed.#DR"), ("zq_shed", Some(*b"dr")));
    assert_eq!(split_kind("zq_shed"), ("zq_shed", None));
    assert_eq!(split_kind("zq_shed.#d"), ("zq_shed.#d", None));
    assert_eq!(split_kind(".#dr"), ("", Some(*b"dr")));
}

#[test]
fn a_held_name_answers_both_hashes_whatever_its_case() {
    let mut vocabulary = Vocabulary::default();
    assert!(vocabulary.add("Zq_Shed_07", Some(*b"dr")));
    assert!(!vocabulary.add("zq_shed_07", None), "the same name");
    assert!(vocabulary.add("zq_plain_name", None));
    assert_eq!(vocabulary.len(), 2);
    let (hash, word) = (name_hash32(b"zq_shed_07"), name_hash64(b"zq_shed_07"));
    assert_eq!(vocabulary.name32(hash), Some("zq_shed_07"));
    assert_eq!(vocabulary.name64(word), Some("zq_shed_07"));
    assert_eq!(vocabulary.confirmed(hash, word), Some("zq_shed_07"));
    assert_eq!(vocabulary.confirmed(hash, 0), Some("zq_shed_07"));
    assert_eq!(vocabulary.confirmed(hash, word ^ 1), None);
    assert_eq!(vocabulary.confirmed(hash ^ 1, word), None);
    assert_eq!(vocabulary.name32(hash ^ 1), None);
    assert_eq!(
        vocabulary.resource_names().collect::<Vec<_>>(),
        ["zq_shed_07"]
    );

    for refused in [
        "",
        "two words",
        "tab\there",
        "caf\u{e9}",
        &"q".repeat(MAX_NAME + 1),
    ] {
        assert!(!vocabulary.add(refused, None), "{refused:?}");
    }
    assert_eq!(vocabulary.len(), 2);
}

#[test]
fn a_plain_name_takes_the_kind_a_later_body_spells_it_with() {
    let mut vocabulary = Vocabulary::default();
    vocabulary.add("zq_crane", None);
    assert_eq!(vocabulary.counts().plain, 1);
    vocabulary.add("zq_crane", Some(*b"ft"));
    vocabulary.add("zq_crane", Some(*b"dr"));
    let counts = vocabulary.counts();
    assert_eq!((counts.names, counts.plain), (1, 0));
    assert_eq!(counts.kinds, BTreeMap::from([(*b"ft", 1)]));
}

#[test]
fn a_32_bit_hash_two_names_share_names_neither_and_the_64_bit_word_tells_them_apart() {
    let mut vocabulary = Vocabulary::default();
    let mut first: HashMap<u32, String> = HashMap::new();
    // Among names of 32-bit hashes, two share one within some hundred thousand.
    let (a, b) = (0..2_000_000u32)
        .find_map(|k| {
            let name = format!("zq_name_{k}");
            vocabulary.add(&name, None);
            let hash = name_hash32(name.as_bytes());
            first
                .insert(hash, name.clone())
                .map(|earlier| (earlier, name))
        })
        .expect("two of two million names share a 32-bit hash");
    let hash = name_hash32(a.as_bytes());
    assert_eq!(hash, name_hash32(b.as_bytes()));
    assert_eq!(vocabulary.name32(hash), None);
    assert_eq!(vocabulary.confirmed(hash, 0), None);
    for name in [&a, &b] {
        let word = name_hash64(name.as_bytes());
        assert_eq!(vocabulary.confirmed(hash, word), Some(name.as_str()));
    }
    let counts = vocabulary.counts();
    assert_eq!(counts.shared32, 1);
    assert_eq!(counts.hashes32, counts.names - 1);
}

#[test]
fn a_harvest_holds_a_body_s_names_and_two_vocabularies_merge() {
    let mut one = Vocabulary::default();
    assert_eq!(one.harvest(&binary_body(), HARVEST_PREFIX), 7);
    assert_eq!(one.harvest(&binary_body(), HARVEST_PREFIX), 0);
    let counts = one.counts();
    assert_eq!((counts.names, counts.plain), (7, 2));
    assert_eq!(counts.kinds.len(), 5);

    let mut other = Vocabulary::default();
    other.add("zq_crane", None);
    other.add("zq_elsewhere", Some(*b"dr"));
    one.merge(&other);
    assert_eq!(one.len(), 8);
    assert_eq!(
        one.name64(name_hash64(b"zq_elsewhere")),
        Some("zq_elsewhere")
    );
    // The fragment name stays a fragment's, though the other archive held it plain.
    assert_eq!(one.counts().kinds[b"ft"], 1);
}

#[test]
fn a_vocabulary_reads_back_what_it_wrote_and_passes_over_what_is_no_name() {
    let mut vocabulary = Vocabulary::default();
    vocabulary.harvest(&binary_body(), HARVEST_PREFIX);
    let mut kept = Vec::new();
    vocabulary.write(&mut kept).expect("it writes");
    assert_eq!(
        Vocabulary::read(kept.as_slice()).expect("it reads"),
        vocabulary
    );

    let mut odd = kept.clone();
    odd.extend_from_slice(b"nospace\n?? zq_bad_kind\ndr \n-- zq_added\r\n");
    odd.extend_from_slice(format!("-- {}\n", "q".repeat(4096)).as_bytes());
    let read = Vocabulary::read(odd.as_slice()).expect("it reads");
    assert_eq!(read.len(), vocabulary.len() + 1);
    assert_eq!(read.name32(name_hash32(b"zq_added")), Some("zq_added"));

    assert!(Vocabulary::read(&b"some other file\n-- zq_name\n"[..]).is_err());
    assert!(Vocabulary::read(&b""[..]).is_err());
}

#[test]
fn a_vocabulary_and_an_archive_s_labels_are_kept_in_a_folder_and_served_again() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let root = scratch.path().to_path_buf();
    let archive = root.join("zq_archive.rpf");
    fs::create_dir_all(&root).expect("a scratch folder");
    fs::write(&archive, [0u8; 64]).expect("a made-up archive");

    let mut vocabulary = Vocabulary::default();
    vocabulary.harvest(&binary_body(), HARVEST_PREFIX);
    let file = vocabulary_file(&root);
    assert!(Vocabulary::load(&file).is_empty(), "none is kept yet");
    vocabulary.save(&file).expect("it is kept");
    assert_eq!(Vocabulary::load(&file), vocabulary);

    let labels = Labels::from([
        (0x40, "zq_shed_07.#dr".to_string()),
        (0x1_0000_0000, "zq_crane.#ft".to_string()),
    ]);
    assert!(load_labels(&root, &archive).is_empty(), "none are kept yet");
    let kept = save_labels(&root, &archive, &labels).expect("they are kept");
    assert!(kept.starts_with(&root));
    assert_eq!(load_labels(&root, &archive), labels);
    // The archive changed length: its offsets are another file's.
    fs::write(&archive, [0u8; 65]).expect("the archive grows");
    assert!(load_labels(&root, &archive).is_empty());
    fs::remove_dir_all(&root).expect("the scratch folder goes");
}

#[test]
fn labels_read_back_and_a_line_that_is_no_label_is_passed_over() {
    let labels = Labels::from([(0x10, "zq_a.#dr".to_string()), (0xABC0, "zq_b".to_string())]);
    let mut kept = Vec::new();
    write_labels(&labels, 0x1234, &mut kept).expect("it writes");
    kept.extend_from_slice(b"xyz zq_bad_offset\n20 two words\n30 .#dr\n40\n");
    assert_eq!(
        read_labels(kept.as_slice(), 0x1234).expect("it reads"),
        labels
    );
    assert!(read_labels(kept.as_slice(), 0x1235)
        .expect("it reads")
        .is_empty());
}

fn zlib_find(content: Rpf9Content) -> Rpf9ScanFind {
    Rpf9ScanFind {
        offset: 0x100,
        kind: Rpf9BodyKind::Zlib(Rpf9ZlibRun {
            content,
            chunks: 1,
            stored_pieces: 0,
            first_chunk_len: 16,
            first_chunk_inflated: 64,
            inflated: 64,
            signature: None,
            resource: None,
        }),
        span: 16,
        stored_len: Some(16),
        inflated_len: Some(64),
        container: None,
    }
}

#[test]
fn a_text_body_is_harvested_and_a_run_of_no_known_content_is_not() {
    let body = b"<name>zq_tower.#dr</name>";
    let mut vocabulary = Vocabulary::default();
    let other = zlib_find(Rpf9Content::Other);
    assert!(!harvests(&other));
    assert_eq!(
        harvest_body(&mut vocabulary, &other, body, HARVEST_PREFIX),
        None
    );
    assert!(vocabulary.is_empty());

    let xml = zlib_find(Rpf9Content::Xml);
    assert!(harvests(&xml));
    // A text body states no name of its own.
    assert_eq!(
        harvest_body(&mut vocabulary, &xml, body, HARVEST_PREFIX),
        None
    );
    assert_eq!(
        vocabulary.name32(name_hash32(b"zq_tower")),
        Some("zq_tower")
    );

    let movie = Rpf9ScanFind {
        kind: Rpf9BodyKind::Bink2 {
            revision: b'j',
            length: 64,
        },
        ..xml
    };
    assert!(!harvests(&movie));
}

/// A made-up model's virtual section that states `name`: a root whose 0x08 points at a block map
/// at 0xE0, whose 0x10 points at a record and whose 0xA8 points at the name; a fragment's next
/// three slots point too, and a drawable's 0x50 does.
fn model_section(name: &str, fragment: bool) -> Vec<u8> {
    let mut v = vec![0u8; 0x100];
    let pointers: &[usize] = match fragment {
        true => &[0x10, 0x18, 0x20, 0x28],
        false => &[0x10, 0x50],
    };
    let records = pointers.iter().map(|at| (*at, 0xC0u64));
    for (at, target) in records.chain([(0x08, 0xE0), (0xA8, 0xD0)]) {
        v[at..at + 8].copy_from_slice(&(0x5000_0000 + target).to_le_bytes());
    }
    // A word that is no pointer, as a fragment's seventh slot holds.
    v[0x30] = u8::from(fragment);
    assert!(name.len() < 0x10, "the name lies before the block map");
    v[0xD0..0xD0 + name.len()].copy_from_slice(name.as_bytes());
    v
}

/// The find of a resource a scan would make of `section`, its whole virtual section.
fn resource_find(section: &[u8]) -> Rpf9ScanFind {
    let len = section.len() as u64;
    let Rpf9BodyKind::Zlib(run) = zlib_find(Rpf9Content::ResourceShaped).kind else {
        unreachable!("a zlib find");
    };
    let resource = crate::container::rpf::Rpf9Resource {
        virtual_blocks: 1,
        physical_blocks: 0,
        blocks: [None; 4],
        virtual_len: len,
        physical_len: 0,
        walk: crate::container::rpf::Rpf9Walk::Whole,
    };
    Rpf9ScanFind {
        kind: Rpf9BodyKind::Zlib(Rpf9ZlibRun {
            signature: crate::container::rpf::Rpf9Signature::of(section, len),
            resource: Some(resource),
            inflated: len,
            ..run
        }),
        ..zlib_find(Rpf9Content::ResourceShaped)
    }
}

/// The role the measured layouts place the resource `find` in.
fn role_of(find: &Rpf9ScanFind) -> Option<Rpf9Role> {
    match find.kind {
        Rpf9BodyKind::Zlib(run) => run.signature.and_then(|s| s.role()),
        _ => None,
    }
}

#[test]
fn a_drawable_and_a_fragment_are_labelled_with_the_name_their_root_states() {
    let mut vocabulary = Vocabulary::default();
    let drawable = model_section("Zq_Shed_07.#dr", false);
    let find = resource_find(&drawable);
    assert_eq!(role_of(&find), Some(Rpf9Role::Drawable));
    assert_eq!(
        harvest_body(&mut vocabulary, &find, &drawable, HARVEST_PREFIX).as_deref(),
        Some("Zq_Shed_07.#dr")
    );
    assert_eq!(
        vocabulary.confirmed(name_hash32(b"zq_shed_07"), name_hash64(b"zq_shed_07")),
        Some("zq_shed_07")
    );
    assert_eq!(vocabulary.counts().kinds[b"dr"], 1);

    // A fragment's root states its name with no ending.
    let fragment = model_section("zq_crane", true);
    let find = resource_find(&fragment);
    assert_eq!(role_of(&find), Some(Rpf9Role::DrawableWithBound));
    assert_eq!(
        harvest_body(&mut vocabulary, &find, &fragment, HARVEST_PREFIX).as_deref(),
        Some("zq_crane")
    );
    assert_eq!(
        vocabulary.name32(name_hash32(b"zq_crane")),
        Some("zq_crane")
    );

    // The bytes held of a body end before the name: no label, and no name made up.
    let mut none = Vocabulary::default();
    let find = resource_find(&drawable);
    assert_eq!(
        harvest_body(&mut none, &find, &drawable[..0xD0], 0xD0),
        None
    );
    assert!(none.is_empty());
    // A resource of another role states no name of its own, and its names are still held.
    let mut other = drawable.clone();
    other[0x50..0x58].fill(0);
    let find = resource_find(&other);
    assert_eq!(role_of(&find), None);
    assert_eq!(harvest_body(&mut none, &find, &other, HARVEST_PREFIX), None);
    assert_eq!(none.counts().kinds[b"dr"], 1);
}

/// A made-up archive at `path`: an RPF9 magic, then each of `bodies` at a page of its own, the
/// offsets given.
fn archive_of(path: &Path, bodies: &[Vec<u8>]) -> Vec<u64> {
    let mut bytes = crate::container::rpf::RPF9_MAGIC.to_le_bytes().to_vec();
    let mut offsets = Vec::new();
    for body in bodies {
        bytes.resize(bytes.len().next_multiple_of(0x1000), 0);
        offsets.push(bytes.len() as u64);
        bytes.extend_from_slice(body);
    }
    bytes.resize(bytes.len().next_multiple_of(0x1000), 0);
    fs::write(path, bytes).expect("a made-up archive");
    offsets
}

/// `section` as the loose resource a scan finds.
fn resource_file(section: Vec<u8>) -> Vec<u8> {
    use crate::container::rpf::{write_rpf9_resource, Rpf9Form, Rpf9Sections};
    let sections = Rpf9Sections {
        form: Rpf9Form::BlockMapped,
        virtual_section: section,
        physical: Vec::new(),
    };
    write_rpf9_resource(&sections).expect("a resource")
}

/// `text` as one zlib stream under the head a scan finds.
fn zlib_file(text: &[u8]) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(text).expect("it deflates");
    z.finish().expect("it ends")
}

#[test]
fn an_archive_is_harvested_in_one_pass_whatever_its_ranges_and_workers() {
    let scratch = tempfile::tempdir().expect("a scratch folder");
    let root = scratch.path().to_path_buf();
    fs::create_dir_all(&root).expect("a scratch folder");
    let archive = root.join("zq_models.rpf");
    // A text body of some 40 KiB, its last name far past its first.
    let mut text = b"<?xml version=\"1.0\"?>\n<Zq>\n<a>zq_first_name</a>\n".to_vec();
    while text.len() < 40 << 10 {
        text.extend_from_slice(format!("<n>{}</n>\n", text.len() * 7919).as_bytes());
    }
    text.extend_from_slice(b"<a>zq_last_name.#td</a>\n</Zq>\n");
    let offsets = archive_of(
        &archive,
        &[
            resource_file(model_section("zq_hut_03.#dr", false)),
            zlib_file(&text),
            resource_file(model_section("zq_mast", true)),
        ],
    );
    let wanted = Labels::from([
        (offsets[0], "zq_hut_03.#dr".to_string()),
        (offsets[2], "zq_mast".to_string()),
    ]);
    let never = || false;
    let said = std::sync::Mutex::new(Vec::new());
    let reached = |done: u64, len: u64| said.lock().expect("unlocked").push((done, len));
    let len = fs::metadata(&archive).expect("there").len();
    let mut first: Option<Vocabulary> = None;
    // One range and one worker, then ranges that cut the text body, on one worker and on several.
    for (range, workers) in [(u64::MAX, 1), (0x1000, 1), (0x1000, 4), (0x3000, 3)] {
        let gave = harvest_ranges(&archive, HARVEST_PREFIX, range, workers, &never, &reached)
            .expect("it is harvested");
        let how = format!("{range:#X} bytes a range, {workers} workers");
        assert!(!gave.stopped, "{how}");
        assert_eq!(gave.labels, wanted, "{how}");
        for name in ["zq_hut_03", "zq_mast", "zq_first_name", "zq_last_name"] {
            let held = gave.vocabulary.name32(name_hash32(name.as_bytes()));
            assert_eq!(held, Some(name), "{how}");
        }
        if workers == 1 {
            // One worker reads each body once, each range from where the last one's scan ended.
            assert_eq!(gave.bodies, 3, "{how}");
            let whole = first.get_or_insert_with(|| gave.vocabulary.clone());
            assert_eq!(gave.vocabulary, *whole, "{how}");
        }
        let mut said = said.lock().expect("unlocked");
        assert_eq!(said.iter().map(|(done, _)| *done).max(), Some(len), "{how}");
        assert!(said.iter().all(|(_, of)| *of == len), "{how}");
        said.clear();
    }

    // No more of a body is held than the bytes kept: the first name, and not the last.
    let gave = harvest_ranges(&archive, 4096, u64::MAX, 1, &never, &reached).expect("harvested");
    let held = |name: &[u8]| gave.vocabulary.name32(name_hash32(name)).is_some();
    assert!(held(b"zq_first_name") && !held(b"zq_last_name"));
    assert_eq!(gave.labels, wanted);
    // A model whose name lies past the bytes kept is read for names and states none.
    let gave = harvest_ranges(&archive, 0xC0, u64::MAX, 1, &never, &reached).expect("harvested");
    assert!(gave.labels.is_empty() && !gave.stopped);

    let stopped = harvest_ranges(&archive, HARVEST_PREFIX, 0x1000, 2, &|| true, &reached)
        .expect("it returns");
    assert!(stopped.stopped && stopped.vocabulary.is_empty() && stopped.labels.is_empty());
    assert!(harvest_archive(&root.join("zq_absent.rpf"), 1, 1, &never, &reached).is_err());
    fs::remove_dir_all(&root).expect("the scratch folder goes");
}
