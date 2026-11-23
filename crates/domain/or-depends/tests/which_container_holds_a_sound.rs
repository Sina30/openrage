//! Which `.awc` holds a sound, over a synthetic install: real archives, a
//! nested one holding the container, crawled with no game present.

use std::path::PathBuf;
use std::sync::Arc;

use or_depends::audio::{AudioIndex, Filter, Resolution};
use or_io::awc::AwcKey;
use or_io::container::rpf::crypto::NoOpCryptoProvider;
use or_io::container::rpf::RpfBuilder;
use or_io::rel::sound::{SimpleSound, Sound, SoundItem, WaveRef};
use or_io::rel::{RelFile, RelIndex, RelIndexHash};

fn hash(name: &str) -> u32 {
    or_io::string_hash(name.as_bytes())
}

fn archive(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut builder = RpfBuilder::default();
    for (path, bytes) in files {
        builder
            .insert_with(path, bytes.clone(), false)
            .unwrap_or_else(|e| panic!("add {path}: {e}"));
    }
    builder.to_bytes().expect("save")
}

/// A plain `.awc` holding one chunkless stream per wave name.
fn container(waves: &[&str]) -> Vec<u8> {
    let mut out = b"ADAT".to_vec();
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(waves.len() as u32).to_le_bytes());
    out.extend_from_slice(&(16 + 4 * waves.len() as u32).to_le_bytes());
    for wave in waves {
        out.extend_from_slice(&(hash(wave) & 0x1FFF_FFFF).to_le_bytes());
    }
    out
}

fn simple(bank: &str, wave: &str) -> SoundItem {
    SoundItem::SimpleSound(Box::new(SimpleSound {
        base: Sound::default(),
        wave_ref: WaveRef {
            bank_name: hash(bank),
            wave_name: hash(wave),
        },
        wave_slot_index: 0,
    }))
}

/// A sound file holding `(sound name, item)` pairs.
fn sounds(items: &[(&str, SoundItem)]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut entries = Vec::new();
    for (name, item) in items {
        let body = item.encode().expect("encode");
        entries.push(RelIndexHash {
            hash: hash(name),
            offset: data.len() as u32,
            length: body.len() as u32,
        });
        data.extend_from_slice(&body);
    }
    RelFile {
        rel_type: 54,
        data,
        names: vec!["RESIDENT\\EXPLOSIONS".into()],
        index: RelIndex::Hashed(entries),
        hash_offsets: Vec::new(),
        pack_offsets: Vec::new(),
    }
    .save()
    .expect("save")
}

/// A granular sound whose first channel names the wave and the other five none.
// ⚠ Laid out by hand: a compression mask of 0, the wave slot, six channels of
// bank and wave, then 73 bytes of settings, volumes, parent and clock count.
fn granular(bank: &str, wave: &str) -> SoundItem {
    let mut bytes = vec![19u8];
    bytes.extend_from_slice(&[0u8; 8]);
    bytes.extend_from_slice(&hash(bank).to_le_bytes());
    bytes.extend_from_slice(&hash(wave).to_le_bytes());
    bytes.extend_from_slice(&[0u8; 5 * 8 + 73]);
    let item = SoundItem::decode(&bytes).expect("a granular sound decodes");
    assert!(matches!(item, SoundItem::GranularSound(_)));
    item
}

/// The install's root, and the folder's guard.
fn install(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let resident = archive(&[("explosions.awc", container(&["boom_01", "boom_02"]))]);
    let common = archive(&[
        ("data/placeholder.txt", vec![0u8; 8]),
        (
            "audio/config/sounds.dat54.rel",
            sounds(&[
                ("boom_big", simple("RESIDENT\\EXPLOSIONS", "boom_01")),
                ("boom_lost", simple("RESIDENT\\EXPLOSIONS", "boom_99")),
                ("boom_elsewhere", simple("RESIDENT\\NOWHERE", "boom_01")),
                ("boom_grain", granular("resident/explosions", "boom_02")),
            ]),
        ),
    ]);
    let scratch = tempfile::Builder::new()
        .prefix(&format!("or-depends-audio-{tag}-"))
        .tempdir()
        .expect("mkdir");
    let root = scratch.path().to_path_buf();
    std::fs::write(root.join("common.rpf"), common).expect("write");
    // ⚠ Loose on disk, where Legacy keeps its wave banks and no mount reaches.
    let banks = root.join("x64").join("audio").join("sfx");
    std::fs::create_dir_all(&banks).expect("mkdir");
    std::fs::write(banks.join("RESIDENT.rpf"), resident).expect("write");
    (scratch, root)
}

#[test]
fn a_sound_name_resolves_to_its_container_and_the_items_naming_it() {
    let (_scratch, root) = install("resolve");
    let mut warnings = Vec::new();
    let index = AudioIndex::read(
        &root,
        Arc::new(NoOpCryptoProvider::new()),
        AwcKey::default(),
        &Filter::default(),
        &mut warnings,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(index.refused().is_empty(), "{:?}", index.refused());
    assert_eq!(index.containers().len(), 1);
    assert_eq!(index.references().len(), 4);

    let explosions = "x64/audio/sfx/RESIDENT.rpf/explosions.awc";
    for query in ["boom_big", "boom_01"] {
        let hits = index.find(hash(query), &Filter::default());
        let named: Vec<_> = hits
            .iter()
            .filter_map(|h| h.reference.map(|r| (r.item, r.class)))
            .collect();
        assert!(
            named.contains(&(hash("boom_big"), "SimpleSound")),
            "{query}: {named:?}"
        );
        let held: Vec<&str> = hits
            .iter()
            .filter(|h| h.reference.is_some_and(|r| r.item == hash("boom_big")))
            .flat_map(|h| h.containers.iter().map(|c| c.logical.as_str()))
            .collect();
        assert_eq!(held, [explosions], "{query}");
    }

    let grain = index.find(hash("boom_grain"), &Filter::default());
    assert_eq!(grain.len(), 1);
    assert_eq!(grain[0].reference.map(|r| r.class), Some("GranularSound"));
    assert_eq!(grain[0].containers.len(), 1);

    let by_class = Filter {
        class: Some("granularsound".into()),
        ..Filter::default()
    };
    assert!(index.find(hash("boom_big"), &by_class).is_empty());
    let by_container = Filter {
        container: Some("weapons".into()),
        ..Filter::default()
    };
    assert!(index.find(hash("boom_big"), &by_container).is_empty());
    let by_bank = Filter {
        container: Some("resident/explosions".into()),
        ..Filter::default()
    };
    assert_eq!(index.find(hash("boom_big"), &by_bank).len(), 1);

    let mut tally = [0usize; 3];
    for reference in index.references() {
        tally[match index.resolution(reference) {
            Resolution::Held => 0,
            Resolution::BankWithoutWave => 1,
            Resolution::NoBank => 2,
        }] += 1;
    }
    assert_eq!(tally, [2, 1, 1]);
    assert_eq!(
        index.bank_name(hash("resident/explosions")),
        Some("RESIDENT\\EXPLOSIONS")
    );
}
