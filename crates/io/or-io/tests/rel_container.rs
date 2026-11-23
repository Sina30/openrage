//! `.rel` over real bytes: every container both installs ship opens, keeps
//! every item, and saves to its fixed point; every item of every `relType`
//! reads typed and writes back to its bytes; and a damaged item table is
//! refused by name rather than read.
//!
//! ⚠ Install-gated: `OPENRAGE_GAME_ROOT` names the installs, `;`-separated.
//! The `.rel` files sit inside archives, some nested, so each install's own
//! executable supplies the keys and the bundled snapshot the NG ones (D18).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::crypto::{extract_keys, Gta5Keys, Gta5RetailCryptoProvider};
use or_io::container::rpf::{RpfArchive, RpfCryptoProvider};
use or_io::rel::category::CategoryItem;
use or_io::rel::config::{ConfigItem, ConfigValue};
use or_io::rel::curve::CurveItem;
use or_io::rel::dynamic_mix::DynamicMixItem;
use or_io::rel::game_object::{
    BeatConstraint, GameObjectItem, MusicAction, MusicTimingConstraint, StopTrackAction,
    TimingConstraints, TrainAudioSettings,
};
use or_io::rel::layout::NameTableOffset;
use or_io::rel::sound::{LoopingSound, Sound, SoundItem};
use or_io::rel::speech::{
    BankNameTable, SpeechItem, VariationGroupInfoData, Voice, VoiceContextInfo,
    VoiceContextInfoVarGroupData, VoiceContextVariationMask,
};
use or_io::rel::synth::SynthItem;
use or_io::rel::{RelFile, RelIndex};
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

/// How deep a `.rpf` inside a `.rpf` is followed, as the install crawl does.
const MAX_NEST_DEPTH: u32 = 8;

fn provider(keys: &Arc<Gta5Keys>) -> Arc<dyn RpfCryptoProvider> {
    Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(keys)))
}

fn walk_archive(
    archive: &Arc<RpfArchive>,
    label: &str,
    depth: u32,
    visit: &mut dyn FnMut(String, Result<Vec<u8>, String>),
) {
    for node in archive.nodes.iter().filter(|n| !n.is_dir()) {
        let name = node.name().to_ascii_lowercase();
        let path = format!("{label}/{}", node.path());
        let i = node.entry_index();
        if name.ends_with(".rpf") && depth < MAX_NEST_DEPTH {
            match archive.open_nested(i) {
                Ok(nested) => walk_archive(&Arc::new(nested), &path, depth + 1, visit),
                Err(e) => visit(path, Err(e.to_string())),
            }
        } else if name.ends_with(".rel") {
            visit(path, archive.read_entry(i).map_err(|e| e.to_string()));
        }
    }
}

/// Every `.rel` under `root`, loose or in an archive at any depth.
fn every_rel(root: &Path, visit: &mut dyn FnMut(String, Result<Vec<u8>, String>)) {
    let exe = ["GTA5_Enhanced.exe", "GTA5.exe"]
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("no game executable under {}", root.display()));
    let keys = Arc::new(extract_keys(&exe, &|_| {}).expect("keys from the install's executable"));
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let label = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let lower = label.to_ascii_lowercase();
            if lower.ends_with(".rel") {
                visit(label, std::fs::read(&path).map_err(|e| e.to_string()));
            } else if lower.ends_with(".rpf") {
                match RpfArchive::open_with(&path, provider(&keys)) {
                    Ok(archive) => walk_archive(&Arc::new(archive), &label, 0, visit),
                    Err(e) => visit(label, Err(e.to_string())),
                }
            }
        }
    }
}

/// One `relType` and index form's share of an install.
#[derive(Default)]
struct PerType {
    files: usize,
    items: usize,
    /// Item type id, the first byte of an item, → items.
    type_ids: BTreeMap<u8, usize>,
}

#[test]
fn every_shipped_rel_reads_and_saves_to_its_fixed_point() {
    let what = "every shipped .rel to its fixed point";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return;
    }
    for root in roots {
        let (mut seen, mut identical) = (0usize, 0usize);
        let mut failed: Vec<String> = Vec::new();
        let mut census: BTreeMap<(u32, &'static str), PerType> = BTreeMap::new();
        every_rel(&root, &mut |label, bytes| {
            let bytes = match bytes {
                Ok(bytes) => bytes,
                // A nested archive that will not open is a `.rel` this run did
                // not see, so it fails rather than shrinking the census.
                Err(e) => return failed.push(format!("{label}: {e}")),
            };
            seen += 1;
            let file = match RelFile::open(&bytes) {
                Ok(file) => file,
                Err(e) => return failed.push(format!("{label}: open: {e}")),
            };
            let saved = match file.save() {
                Ok(saved) => saved,
                Err(e) => return failed.push(format!("{label}: save: {e}")),
            };
            let again = match RelFile::open(&saved) {
                Ok(again) => again,
                Err(e) => return failed.push(format!("{label}: reopen: {e}")),
            };
            if again.save().ok().as_deref() != Some(&saved[..]) {
                return failed.push(format!("{label}: the second save differs from the first"));
            }
            let (items, kept) = (file.items().unwrap(), again.items().unwrap());
            if items != kept {
                return failed.push(format!("{label}: the saved file's items differ"));
            }
            if saved == bytes {
                identical += 1;
            }
            let form = match file.index {
                RelIndex::Hashed(_) => "hashed",
                RelIndex::Named(_) => "named",
            };
            let per = census.entry((file.rel_type, form)).or_default();
            per.files += 1;
            per.items += items.len();
            for item in &items {
                *per.type_ids.entry(item.type_id()).or_default() += 1;
            }
        });
        println!(
            "{}: {seen} .rel, {identical} saved byte-identical",
            root.display()
        );
        for ((rel_type, form), per) in &census {
            println!(
                "  relType {rel_type}, {form} index: {} files, {} items, item type ids {:?}",
                per.files, per.items, per.type_ids
            );
        }
        assert!(
            failed.is_empty(),
            "{}: {} of {seen} failed:\n{}",
            root.display(),
            failed.len(),
            failed.join("\n")
        );
        assert!(seen > 0, "{}: no .rel found", root.display());
        assert_eq!(identical, seen, "{}", root.display());
    }
}

/// Each typed item's encoding, in index order, for a `relType` 4 file, and its kind's name.
fn typed_rel_type_4(file: &RelFile) -> Result<Vec<(Vec<u8>, &'static str)>, String> {
    let encoded = |bytes: or_io::Result<Vec<u8>>| bytes.map_err(|e| e.to_string());
    match file.index {
        RelIndex::Hashed(_) => file
            .speech_items()
            .map_err(|e| e.to_string())?
            .iter()
            .map(|item| Ok((encoded(item.encode())?, speech_kind(item))))
            .collect(),
        RelIndex::Named(_) => file
            .config_items()
            .map_err(|e| e.to_string())?
            .iter()
            .map(|item| Ok((encoded(item.encode())?, config_kind(&item.value))))
            .collect(),
    }
}

fn speech_kind(item: &SpeechItem) -> &'static str {
    match item {
        SpeechItem::VariationGroupInfoData(_) => "VariationGroupInfoData",
        SpeechItem::VoiceContextInfo(_) => "VoiceContextInfo",
        SpeechItem::VoiceContextInfoVarGroupData(_) => "VoiceContextInfoVarGroupData",
        SpeechItem::Voice(_) => "Voice",
        SpeechItem::BankNameTable(_) => "BankNameTable",
        SpeechItem::VoiceContextVariationMask(_) => "VoiceContextVariationMask",
    }
}

fn config_kind(value: &ConfigValue) -> &'static str {
    match value {
        ConfigValue::Int(_) => "Int",
        ConfigValue::UnsignedInt(_) => "UnsignedInt",
        ConfigValue::Float(_) => "Float",
        ConfigValue::String(_) => "String",
        ConfigValue::Vector3 { .. } => "Vector3",
        ConfigValue::VariableList(_) => "VariableList",
        ConfigValue::Slot(_) => "Slot",
        ConfigValue::WaveSlots(_) => "WaveSlots",
        ConfigValue::ErSettings(_) => "ERSettings",
    }
}

/// The spans of a file's indexed items in its data block, in index order.
fn spans(file: &RelFile) -> Vec<(usize, usize)> {
    match &file.index {
        RelIndex::Hashed(entries) => entries
            .iter()
            .map(|e| (e.offset as usize, e.length as usize))
            .collect(),
        RelIndex::Named(entries) => entries
            .iter()
            .map(|e| (e.offset as usize, e.length as usize))
            .collect(),
    }
}

#[test]
fn every_shipped_rel_type_4_item_reads_typed_and_writes_back_its_bytes() {
    let what = "every shipped relType 4 item, typed";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return;
    }
    for root in roots {
        let (mut files, mut items) = (0usize, 0usize);
        let mut failed: Vec<String> = Vec::new();
        let mut census: BTreeMap<&'static str, usize> = BTreeMap::new();
        every_rel(&root, &mut |label, bytes| {
            let bytes = match bytes {
                Ok(bytes) => bytes,
                Err(e) => return failed.push(format!("{label}: {e}")),
            };
            let mut file = match RelFile::open(&bytes) {
                Ok(file) if file.rel_type == 4 => file,
                Ok(_) => return,
                Err(e) => return failed.push(format!("{label}: open: {e}")),
            };
            files += 1;
            let typed = match typed_rel_type_4(&file) {
                Ok(typed) => typed,
                Err(e) => return failed.push(format!("{label}: {e}")),
            };
            // Every item's bytes are cleared and only its typed form writes
            // them back, so a field the typed form drops shows in the save.
            let spans = spans(&file);
            for &(offset, length) in &spans {
                file.data[offset..offset + length].fill(0);
            }
            for ((offset, length), (encoded, kind)) in spans.into_iter().zip(typed) {
                if encoded.len() != length {
                    return failed.push(format!(
                        "{label}: a {kind} at {offset:#x} writes {} bytes, not {length}",
                        encoded.len()
                    ));
                }
                file.data[offset..offset + length].copy_from_slice(&encoded);
                *census.entry(kind).or_default() += 1;
                items += 1;
            }
            match file.save() {
                Ok(saved) if saved == bytes => {}
                Ok(_) => failed.push(format!("{label}: the typed items save other bytes")),
                Err(e) => failed.push(format!("{label}: save: {e}")),
            }
        });
        println!(
            "{}: {files} relType 4 files, {items} items typed",
            root.display()
        );
        for (kind, count) in &census {
            println!("  {kind}: {count}");
        }
        assert!(
            failed.is_empty(),
            "{}: {} failed:\n{}",
            root.display(),
            failed.len(),
            failed.join("\n")
        );
        assert!(items > 0, "{}: no relType 4 item found", root.display());
    }
}

/// An item of any `rel_type` but 4, typed and encoded, or why it stays bytes.
fn typed_item(rel_type: u32, body: &[u8]) -> Result<(Vec<u8>, &'static str), String> {
    macro_rules! typed {
        ($items:ty) => {
            <$items>::decode(body).and_then(|item| Ok((item.encode()?, item.type_name())))
        };
    }
    let typed = match rel_type {
        10 => typed!(SynthItem),
        15 => typed!(DynamicMixItem),
        16 => typed!(CurveItem),
        22 => typed!(CategoryItem),
        54 => typed!(SoundItem),
        149..=151 => typed!(GameObjectItem),
        _ => return Err(format!("relType {rel_type} has no typed form")),
    };
    typed.map_err(|e| e.to_string())
}

#[test]
fn every_other_shipped_rel_item_reads_typed_and_writes_back_its_bytes() {
    let what = "every shipped item of every relType but 4, typed";
    let roots = game_roots();
    if roots.is_empty() {
        skip_install(what, format_args!("no install root; set {GAME_ROOT_ENV}"));
        return;
    }
    for root in roots {
        let mut failed: Vec<String> = Vec::new();
        let mut census: BTreeMap<(u32, &'static str), usize> = BTreeMap::new();
        every_rel(&root, &mut |label, bytes| {
            let bytes = match bytes {
                Ok(bytes) => bytes,
                Err(e) => return failed.push(format!("{label}: {e}")),
            };
            let mut file = match RelFile::open(&bytes) {
                Ok(file) if file.rel_type != 4 => file,
                Ok(_) => return,
                Err(e) => return failed.push(format!("{label}: open: {e}")),
            };
            let mut typed = Vec::new();
            for (offset, length) in spans(&file) {
                let body = &file.data[offset..offset + length];
                match typed_item(file.rel_type, body) {
                    Ok((encoded, kind)) if encoded.len() == length => {
                        typed.push((offset, encoded, kind))
                    }
                    Ok((encoded, kind)) => failed.push(format!(
                        "{label}: a {kind} at {offset:#x} writes {} bytes, not {length}",
                        encoded.len()
                    )),
                    Err(e) => failed.push(format!(
                        "{label}: class id {} at {offset:#x}, {length} bytes: {e}",
                        body[0]
                    )),
                }
            }
            // Every typed item's bytes are cleared and only its typed form
            // writes them back, so a field the typed form drops shows in the save.
            for (offset, encoded, _) in &typed {
                file.data[*offset..offset + encoded.len()].fill(0);
            }
            for (offset, encoded, kind) in typed {
                file.data[offset..offset + encoded.len()].copy_from_slice(&encoded);
                *census.entry((file.rel_type, kind)).or_default() += 1;
            }
            match file.save() {
                Ok(saved) if saved == bytes => {}
                Ok(_) => failed.push(format!("{label}: the typed items save other bytes")),
                Err(e) => failed.push(format!("{label}: save: {e}")),
            }
        });
        let typed: usize = census.values().sum();
        println!(
            "{}: {typed} items of every relType but 4 typed",
            root.display()
        );
        for ((rel_type, kind), count) in &census {
            println!("  relType {rel_type} {kind}: {count}");
        }
        assert!(
            failed.is_empty(),
            "{}: {} failed:\n{}",
            root.display(),
            failed.len(),
            failed.join("\n")
        );
        assert!(
            typed > 0,
            "{}: no item of a relType but 4 found",
            root.display()
        );
    }
}

fn u32s(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// A container laid out by hand: two items, a name table of one, an index
/// keyed by hash, one hash offset and one pack offset.
fn hand_built() -> Vec<u8> {
    let data = u32s(&[0x0055_0000, 0x0000_0103, 0x1111_1111, 0x0000_0002]);
    let mut out = u32s(&[10, data.len() as u32]);
    out.extend_from_slice(&data);
    // Name table: its length, the count, one offset, one name.
    out.extend(u32s(&[4 + 4 + 4, 1, 0]));
    out.extend_from_slice(b"abc\0");
    // Index: two items, keyed by hash.
    out.extend(u32s(&[2, 0xAAAA_0001, 4, 8, 0xAAAA_0002, 12, 4]));
    // Hash table, then pack table, each offset counted from the file's start:
    // 16 is 8 bytes into the data block.
    out.extend(u32s(&[1, 16, 1, 16]));
    out
}

/// The byte offset of the second index entry's length word.
fn second_entry_length_at(bytes: &[u8]) -> usize {
    bytes.len() - 4 * 4 - 4
}

#[test]
fn a_hand_built_container_reads_as_laid_out_and_saves_unchanged() {
    let bytes = hand_built();
    let file = RelFile::open(&bytes).unwrap();
    assert_eq!(file.rel_type, 10);
    assert_eq!(file.names, ["abc"]);
    assert_eq!(file.hash_offsets, [8]);
    assert_eq!(file.pack_offsets, [8]);
    let items = file.items().unwrap();
    let ids: Vec<u8> = items.iter().map(|item| item.type_id()).collect();
    assert_eq!(ids, [3, 2]);
    assert_eq!(items[0].body(), &bytes[12..20]);
    assert_eq!(file.save().unwrap(), bytes);
}

#[test]
fn a_named_index_round_trips() {
    let data = u32s(&[0x0055_0000, 0x0000_0003]);
    let mut bytes = u32s(&[4, data.len() as u32]);
    bytes.extend_from_slice(&data);
    bytes.extend(u32s(&[4, 0]));
    // Index: one item keyed by a name; the word after the count is the
    // names' bytes, length prefixes included.
    bytes.extend(u32s(&[1, 1 + 5]));
    bytes.push(5);
    bytes.extend_from_slice(b"SLOTS");
    bytes.extend(u32s(&[4, 4, 0, 0]));
    let file = RelFile::open(&bytes).unwrap();
    let RelIndex::Named(entries) = &file.index else {
        panic!("read as {:?}", file.index)
    };
    assert_eq!(entries[0].name, "SLOTS");
    assert_eq!(file.items().unwrap()[0].type_id(), 3);
    assert_eq!(file.save().unwrap(), bytes);
}

#[test]
fn a_corrupted_item_table_is_refused_by_name() {
    let mut bytes = hand_built();
    let at = second_entry_length_at(&bytes);
    bytes[at..at + 4].copy_from_slice(&0x100u32.to_le_bytes());
    let why = RelFile::open(&bytes).unwrap_err().to_string();
    assert!(why.contains("item table"), "{why}");

    let mut empty = hand_built();
    empty[at..at + 4].copy_from_slice(&0u32.to_le_bytes());
    let why = RelFile::open(&empty).unwrap_err().to_string();
    assert!(why.contains("item table"), "{why}");
}

#[test]
fn a_speech_item_is_typed_by_what_points_at_it_before_its_length() {
    // A mask, a variation group and a context info of the lengths the group
    // shares with them, the group's reference, a voice and a bank name table.
    let mut data = vec![0xEE; 4];
    data.extend_from_slice(&[0x01, 0x00, 0x02, 0xAA, 0xBB, 0x02, 0x05, 0x00]);
    data.extend(u32s(&[0x20]));
    data.push(0x07);
    data.extend(u32s(&[4, 0xDEAD_BEEF]));
    let mut bytes = u32s(&[4, data.len() as u32]);
    bytes.extend_from_slice(&data);
    bytes.extend(u32s(&[4, 0]));
    let spans = [
        (0x10, 4, 2),
        (0x20, 6, 3),
        (0x30, 9, 3),
        (0x40, 12, 4),
        (0x50, 16, 1),
        (0x60, 17, 8),
    ];
    bytes.extend(u32s(&[spans.len() as u32]));
    for (hash, offset, length) in spans {
        bytes.extend(u32s(&[hash, offset, length]));
    }
    // The hash table marks the reference and the pack table the bank name,
    // each counted from the file's start.
    bytes.extend(u32s(&[1, 8 + 12, 1, 8 + 17 + 4]));

    let file = RelFile::open(&bytes).unwrap();
    let typed = file.speech_items().unwrap();
    assert_eq!(
        typed,
        [
            SpeechItem::VoiceContextVariationMask(VoiceContextVariationMask { variation_mask: 1 }),
            SpeechItem::VariationGroupInfoData(VariationGroupInfoData {
                variation_data: vec![0xAA, 0xBB]
            }),
            SpeechItem::VoiceContextInfo(VoiceContextInfo {
                num_variations: 2,
                bank_name_table_index: 5
            }),
            SpeechItem::VoiceContextInfoVarGroupData(VoiceContextInfoVarGroupData { r#ref: 0x20 }),
            SpeechItem::Voice(Voice { pain_type: 7 }),
            SpeechItem::BankNameTable(BankNameTable {
                name_table_offset: 0,
                bank_name: 0xDEAD_BEEF
            }),
        ]
    );
    for (item, typed) in file.items().unwrap().iter().zip(&typed) {
        assert_eq!(typed.encode().unwrap(), item.body());
    }
}

#[test]
fn a_configuration_item_reads_by_its_class_id_and_refuses_one_it_has_no_type_for() {
    let int = u32s(&[0x3B00, 0xAAAA_AAAA, (-2i32) as u32]);
    let item = ConfigItem::decode(&int).unwrap();
    assert_eq!(item.name_table_offset, 0x3B);
    assert_eq!(item.flags, 0xAAAA_AAAA);
    assert_eq!(item.value, ConfigValue::Int(-2));
    assert_eq!(item.encode().unwrap(), int);

    let vector4 = u32s(&[6, 0, 0, 0, 0, 0, 0, 0]);
    let why = ConfigItem::decode(&vector4).unwrap_err().to_string();
    assert!(why.contains("class id 6"), "{why}");
}

#[test]
fn a_sound_states_only_the_header_fields_its_compression_mask_sets() {
    // Class id 1, a mask with only bit 2 (volume) set, the volume, then the
    // looping sound's own fields, packed.
    let mut bytes = vec![1];
    bytes.extend(4u32.to_le_bytes());
    bytes.extend((-100i16).to_le_bytes());
    bytes.extend((-1i16).to_le_bytes());
    bytes.extend([0, 0, 0, 0]);
    bytes.extend(u32s(&[0xDEAD_BEEF, 0]));
    let item = SoundItem::decode(&bytes).unwrap();
    let expected = SoundItem::LoopingSound(Box::new(LoopingSound {
        base: Sound {
            volume: Some(-100),
            ..Sound::default()
        },
        loop_count: -1,
        loop_count_variance: 0,
        loop_point: 0,
        sound_ref: 0xDEAD_BEEF,
        loop_count_variable: 0,
    }));
    assert_eq!(item, expected);
    assert_eq!(item.encode().unwrap(), bytes);

    let mut past = bytes.clone();
    past[1..5].copy_from_slice(&(1u32 << 30).to_le_bytes());
    let why = SoundItem::decode(&past).unwrap_err().to_string();
    assert!(why.contains("compression mask"), "{why}");
}

#[test]
fn a_game_object_aligns_its_fields_after_its_base_and_keeps_a_full_array() {
    // Class id 64 with name table offset 5; the base's flags, `constrain` and
    // three bytes of alignment, a count of 1 before both stored constraints,
    // the delay, then the derived `fade_time`.
    let mut bytes = u32s(&[64 | 5 << 8, 0xAAAA_AAAA, 3, 1, 0x1111_1111, 0, 0]);
    bytes.extend(5000i32.to_le_bytes());
    let item = GameObjectItem::decode(&bytes).unwrap();
    let expected = GameObjectItem::StopTrackAction(Box::new(StopTrackAction {
        base: MusicAction {
            name_table_offset: NameTableOffset(5),
            flags: 0xAAAA_AAAA,
            constrain: 3,
            num_timing_constraints: 1,
            timing_constraints: [
                TimingConstraints {
                    constraint: 0x1111_1111,
                },
                TimingConstraints { constraint: 0 },
            ],
            delay_time: 0.0,
        },
        fade_time: 5000,
    }));
    assert_eq!(item, expected);
    assert_eq!(item.encode().unwrap(), bytes);

    // A derived type pads its end to its own fields' alignment, not its base's.
    let beat = [69, 0, 0, 0, 0xAA, 0xAA];
    let item = GameObjectItem::decode(&beat).unwrap();
    assert_eq!(
        item,
        GameObjectItem::BeatConstraint(Box::new(BeatConstraint {
            base: MusicTimingConstraint {
                name_table_offset: NameTableOffset(0)
            },
            valid_sixteenths: 0xAAAA,
        }))
    );
    assert_eq!(item.encode().unwrap(), beat);

    let mut padded = bytes.clone();
    padded[9] = 1;
    let why = GameObjectItem::decode(&padded).unwrap_err().to_string();
    assert!(why.contains("padding"), "{why}");
}

#[test]
fn an_older_shorter_game_object_leaves_its_newer_fields_out() {
    // Class id 53 and 38 words: a TrainAudioSettings that stops before
    // `shockwave_intensity_scale`, as the base Legacy game data's do.
    let mut short = vec![0u8; 4 * 39];
    short[0] = 53;
    let GameObjectItem::TrainAudioSettings(mut item) = GameObjectItem::decode(&short).unwrap()
    else {
        panic!("class id 53 is a TrainAudioSettings");
    };
    assert_eq!(item.shockwave_intensity_scale, None);
    assert_eq!(item.shockwave_radius_scale, None);
    let encode = |item: &TrainAudioSettings| {
        GameObjectItem::TrainAudioSettings(Box::new(item.clone())).encode()
    };
    assert_eq!(encode(&item).unwrap(), short);

    item.shockwave_intensity_scale = Some(1.0);
    item.shockwave_radius_scale = Some(2.0);
    let mut full = short.clone();
    full.extend(u32s(&[1f32.to_bits(), 2f32.to_bits()]));
    assert_eq!(encode(&item).unwrap(), full);

    item.shockwave_intensity_scale = None;
    let why = encode(&item).unwrap_err().to_string();
    assert!(why.contains("newer field"), "{why}");
}

#[test]
fn a_compiled_synth_sizes_its_program_three_words_before_it() {
    // Class id 3; flags; one buffer, no registers; one output of four stored
    // slots; a 4-byte program, one state block and a cost of 2; then two
    // operations, one constant and no input variables.
    let mut bytes = u32s(&[3, 0xAAAA_AAAA, 1, 0, 1, 0, 4, 1, 2, 0xFFFF_4811, 1]);
    bytes.extend(1f32.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    let item = SynthItem::decode(&bytes).unwrap();
    let SynthItem::CompiledSynth(synth) = &item else {
        panic!("class id 3 is a CompiledSynth");
    };
    assert_eq!(synth.operations, [0x4811, 0xFFFF]);
    assert_eq!(synth.cost_estimate, 2);
    assert_eq!(item.encode().unwrap(), bytes);

    let mut odd = bytes.clone();
    odd[24] = 3;
    let why = SynthItem::decode(&odd).unwrap_err().to_string();
    assert!(why.contains("whole operations"), "{why}");
}

#[test]
fn an_equal_power_curve_pads_its_end_to_its_base() {
    // Class id 5, flags, the input range, `flip`, then three bytes of padding.
    let mut bytes = u32s(&[5, 0xAAAA_AAAA, 0, 1f32.to_bits()]);
    bytes.extend([3, 0, 0, 0]);
    let item = CurveItem::decode(&bytes).unwrap();
    assert_eq!(item.type_name(), "EqualPower");
    assert_eq!(item.encode().unwrap(), bytes);
    assert!(CurveItem::decode(&bytes[..17]).is_err());
}
