//! Legacy `.awc` ADPCM over the install: a shipped stream's decode,
//! re-encoded by [`codec::encode_adpcm`], decodes back within ADPCM's error
//! and lays out as the shipped stream does.
//!
//! ⚠ Install-gated: `OPENRAGE_GAME_ROOT` names the installs, `;`-separated,
//! and only a Legacy one is read. The `.awc` files sit inside archives, so the
//! install's own executable supplies the archive and AWC keys.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::awc::{codec, Accumulator, AwcFile, AwcKey, Codec};
use or_io::container::rpf::crypto::{extract_keys, Gta5RetailCryptoProvider};
use or_io::container::rpf::RpfArchive;
use or_test_support::{game_roots, skip_install, GAME_ROOT_ENV};

/// The sample: every single-channel ADPCM stream in three sound-effect
/// archives, 1,273 streams on the Legacy install.
const SAMPLE: [&str; 3] = [
    "x64/audio/sfx/ANIMALS_NEAR.rpf",
    "x64/audio/sfx/PAIN.rpf",
    "x64/audio/sfx/ONESHOT_AMBIENCE.rpf",
];

/// Signal-to-error ratio, in dB, no stream's re-encode falls below. Over the
/// sample the worst measures 18.0 and the median 41.3.
const STREAM_FLOOR_DB: f64 = 15.0;

fn samples(pcm: &[u8]) -> impl Iterator<Item = f64> + '_ {
    pcm.as_chunks::<2>()
        .0
        .iter()
        .map(|b| f64::from(i16::from_le_bytes([b[0], b[1]])))
}

fn snr_db(want: &[u8], got: &[u8]) -> f64 {
    let signal: f64 = samples(want).map(|s| s * s).sum();
    let error: f64 = samples(want)
        .zip(samples(got))
        .map(|(a, b)| (a - b) * (a - b))
        .sum();
    10.0 * (signal.max(1.0) / error.max(1.0)).log10()
}

/// One stream: its label, its shipped decode, its re-encode's decode, and the
/// shipped payload's length against the re-encode's.
type Reencoded = (String, Vec<u8>, Vec<u8>, usize, usize);

fn reencoded(root: &Path) -> Vec<Reencoded> {
    let keys = Arc::new(
        extract_keys(root.join("GTA5.exe"), &|_| {}).expect("keys from the install's executable"),
    );
    let key = AwcKey(keys.awc_key);
    let mut out = Vec::new();
    for archive in SAMPLE {
        let path = root.join(archive);
        let opened = RpfArchive::open_with(
            &path,
            Arc::new(Gta5RetailCryptoProvider::new(Arc::clone(&keys))),
        )
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for node in opened.nodes.iter().filter(|n| !n.is_dir()) {
            if !node.name().to_ascii_lowercase().ends_with(".awc") {
                continue;
            }
            let label = format!("{archive}/{}", node.path());
            let bytes = opened
                .read_entry(node.entry_index())
                .unwrap_or_else(|e| panic!("{label}: {e}"));
            let awc = AwcFile::parse(&bytes, key).unwrap_or_else(|e| panic!("{label}: {e}"));
            for stream in &awc.streams {
                let Some(format) = stream.format().filter(|f| f.codec == Codec::ImaAdpcm) else {
                    continue;
                };
                let n = format.samples as usize;
                let data = awc
                    .stream_data(stream, key)
                    .unwrap_or_else(|| panic!("{label} {:08x}: not descrambled", stream.id));
                let pcm = codec::decode_adpcm(&data, n, Accumulator::Clamped);
                let encoded = codec::encode_adpcm(&pcm);
                let back = codec::decode_adpcm(&encoded, n, Accumulator::Clamped);
                assert_eq!(
                    back,
                    codec::decode_adpcm(&encoded, n, Accumulator::Unclamped),
                    "{label} {:08x}: the re-encode depends on the accumulator model",
                    stream.id
                );
                out.push((
                    format!("{label} {:08x} ({n} samples)", stream.id),
                    pcm,
                    back,
                    data.len(),
                    encoded.len(),
                ));
            }
        }
    }
    out
}

/// ⚠ **Within ADPCM's error, not exact**: the game's encoder chooses each
/// block's header afresh, so a re-encode that carries its state across
/// blocks cannot land on the same samples (`encode_adpcm`'s note). An
/// even-length stream, whose last byte holds two samples, re-encodes to the
/// shipped payload's length; an odd one keeps the last sample the game's
/// encoder dropped, so it may run one word longer.
#[test]
fn a_shipped_adpcm_stream_reencodes_within_adpcm_error() {
    let what = "Legacy ADPCM streams re-encoded";
    let roots: Vec<PathBuf> = game_roots()
        .into_iter()
        .filter(|root| root.join("GTA5.exe").is_file() && !root.join("GTA5_Enhanced.exe").is_file())
        .collect();
    if roots.is_empty() {
        skip_install(
            what,
            format_args!("no Legacy install root; set {GAME_ROOT_ENV}"),
        );
        return;
    }
    for root in roots {
        let streams = reencoded(&root);
        assert!(!streams.is_empty(), "{}: no ADPCM stream", root.display());
        let mut worst = (f64::MAX, String::new());
        let mut all = Vec::with_capacity(streams.len());
        for (label, pcm, back, shipped, ours) in &streams {
            let snr = snr_db(pcm, back);
            all.push(snr);
            if snr < worst.0 {
                worst = (snr, label.clone());
            }
            if (pcm.len() / 2).is_multiple_of(2) {
                assert_eq!(ours, shipped, "{label}: the re-encode's length");
            } else {
                assert!(
                    (*shipped..=shipped + 4).contains(ours),
                    "{label}: {ours} bytes re-encoded against {shipped} shipped"
                );
            }
        }
        all.sort_by(f64::total_cmp);
        eprintln!(
            "{}: {} stream(s) re-encoded; the median at {:.1} dB, the worst at {:.1} dB, {}",
            root.display(),
            streams.len(),
            all[all.len() / 2],
            worst.0,
            worst.1
        );
        assert!(
            worst.0 >= STREAM_FLOOR_DB,
            "{} re-encodes at {:.1} dB, under {STREAM_FLOOR_DB}",
            worst.1,
            worst.0
        );
    }
}
