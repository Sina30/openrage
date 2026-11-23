//! `openrage-cli awc decode-check`: every GTA VI stream of a container decoded in memory, and only
//! whether it decoded, its samples and its peak printed; no audio is written, and a refusal fails.

use std::process::ExitCode;

use or_io::awc::{AwcFile, AwcKey};
use or_io::container::rpf::{read_rpf9_body, Rpf9BodyKind};

pub fn run(args: &[String]) -> ExitCode {
    let (limit, args) = match super::rpf::split_limit(args) {
        Ok(split) => split,
        Err(e) => return usage(&e),
    };
    let bytes = match args.as_slice() {
        [path] => std::fs::read(path).map_err(|e| format!("read {path}: {e}")),
        [path, offset] => match super::rpf::parse_offset(offset) {
            Ok(offset) => body(path, offset, limit),
            Err(e) => return usage(&e),
        },
        _ => return usage("decode-check needs <file.awc>, or <archive.rpf> <offset>"),
    };
    let awc = bytes.and_then(|b| AwcFile::parse(&b, AwcKey::default()).map_err(|e| e.to_string()));
    match awc {
        Ok(awc) => {
            let lines = check(&awc);
            lines.iter().for_each(|line| println!("{line}"));
            if refused(&lines) {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage(why: &str) -> ExitCode {
    eprintln!("Error: {why}");
    eprintln!("usage: openrage-cli awc decode-check <file.awc>");
    eprintln!("       openrage-cli awc decode-check <archive.rpf> <offset> [--limit <bytes>]");
    ExitCode::from(2)
}

/// The audio container body at `offset` of the GTA VI archive at `path`, read to `limit` bytes.
fn body(path: &str, offset: u64, limit: u64) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open {path}: {e}"))?;
    let (find, bytes) = read_rpf9_body(&file, offset, limit).map_err(|e| e.to_string())?;
    match find.kind {
        Rpf9BodyKind::Awc { .. } => Ok(bytes),
        _ => Err(format!("the body at {offset:#X} is no audio container")),
    }
}

/// A line per Opus stream of `awc`, or per channel of a multi-channel source: whether it decoded,
/// its samples per channel and its peak.
fn check(awc: &AwcFile) -> Vec<String> {
    let mut lines = Vec::new();
    for (at, stream) in awc.streams.iter().enumerate() {
        let groups = match or_media::awc::opus_groups(awc, at) {
            Ok(groups) => groups,
            Err(e) => {
                lines.push(format!("{:08X}  refused  {e}", stream.id));
                continue;
            }
        };
        if groups.is_empty() && stream.coded_format().is_ok_and(|f| f.is_some()) {
            lines.push(match coded(awc, at) {
                Ok(pcm) => format!(
                    "{:08X}  decoded  {} samples  peak {}",
                    stream.id,
                    pcm.len(),
                    peak(&pcm)
                ),
                Err(e) => format!("{:08X}  refused  {e}", stream.id),
            });
            continue;
        }
        for group in groups {
            lines.push(match or_media::opus::decode(&group) {
                Ok(pcm) => format!(
                    "{:08X}  decoded  {} samples  peak {}",
                    group.id,
                    pcm.len() / usize::from(group.channels.max(1)),
                    peak(&pcm)
                ),
                Err(e) => format!("{:08X}  refused  {e}", group.id),
            });
        }
    }
    if lines.is_empty() {
        lines.push("no Opus stream".to_string());
    }
    lines
}

/// Whether a line [`check`] printed says it refused a stream.
fn refused(lines: &[String]) -> bool {
    lines.iter().any(|line| line.contains("  refused  "))
}

/// A mono PCM16, XMA or codec-2 stream decoded through the table, as the app plays it.
fn coded(awc: &AwcFile, at: usize) -> Result<Vec<i16>, or_media::MediaError> {
    let source = or_media::awc::open(awc, AwcKey::default(), at)?;
    source.audio(&or_media::Decoders::new())?.read_all_i16()
}

/// The peak of `pcm` in the scale a container states one: min(2·|s|, 65,535) over every channel.
fn peak(pcm: &[i16]) -> u32 {
    let doubled = pcm.iter().map(|s| 2 * u32::from(s.unsigned_abs()));
    doubled.max().unwrap_or(0).min(u32::from(u16::MAX))
}

/// A container of one mono Opus stream `id` stating `samples`: its format record, its packets in
/// one 2,048-byte packet, a peak chunk and a seek table.
#[cfg(test)]
pub(crate) fn opus_container(id: u32, packets: &[Vec<u8>], samples: u32) -> Vec<u8> {
    let mut data = Vec::new();
    for p in packets {
        data.extend((p.len() as u16).to_le_bytes());
        data.extend(p);
    }
    assert!(data.len() <= 2048, "the packets fill one 2,048-byte packet");
    data.resize(2048, 0);
    let mut record = vec![0u8; 32];
    record[0x00..0x04].copy_from_slice(&samples.to_le_bytes());
    record[0x04..0x08].copy_from_slice(&(-1i32).to_le_bytes());
    record[0x08..0x0A].copy_from_slice(&48_000u16.to_le_bytes());
    record[0x0C..0x10].copy_from_slice(&samples.to_le_bytes());
    (record[0x1C], record[0x1D]) = (11, 1);
    let last = 960 * (packets.len() as u32 - 1);
    let seek: Vec<u8> = [0u32, last].iter().flat_map(|w| w.to_le_bytes()).collect();
    let chunks = [
        (0x76u64, record),
        (0x55, data),
        (0xA3, seek),
        (0x36, vec![0; 4]),
    ];
    single(id, &chunks)
}

/// A container of one stream `id` holding `chunks`, flag bit 1 set.
#[cfg(test)]
fn single(id: u32, chunks: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let tables = 16 + 4 + chunks.len() * 8;
    let mut out = b"ADAT".to_vec();
    let words = [
        0xFF02_0001u32,
        1,
        tables as u32,
        (chunks.len() as u32) << 29 | id,
    ];
    words.iter().for_each(|w| out.extend(w.to_le_bytes()));
    let mut at = tables;
    for (kind, payload) in chunks {
        out.extend((kind << 56 | (payload.len() as u64) << 28 | at as u64).to_le_bytes());
        at += payload.len();
    }
    chunks.iter().for_each(|(_, payload)| out.extend(payload));
    out
}

#[cfg(test)]
mod tests {
    use or_io::awc::opus::OpusStream;
    use or_media::opus::{decode, encode, Settings};

    use super::*;

    #[test]
    fn every_opus_stream_is_decoded_in_memory_and_said_with_its_samples_and_peak() {
        let tone: Vec<i16> = (0..1000).map(|t| ((t * 37) % 4000 - 2000) as i16).collect();
        let settings = Settings {
            bitrate: 64_000,
            voice: false,
        };
        let packets = encode(&tone, 1, settings).expect("it encodes");
        let bytes = opus_container(0x0A01, &packets, 1000);
        let awc = AwcFile::parse(&bytes, AwcKey::default()).expect("the container parses");
        let stream = OpusStream {
            id: 0x0A01,
            channels: 1,
            samples: 1000,
            packets: packets.clone(),
        };
        let pcm = decode(&stream).expect("it decodes");
        let largest = pcm.iter().map(|s| s.unsigned_abs()).max().expect("samples");
        assert!(largest > 0, "the tone decodes to sound");
        let want = format!(
            "00000A01  decoded  1000 samples  peak {}",
            2 * u32::from(largest)
        );
        assert_eq!(check(&awc), [want]);
        assert!(!refused(&check(&awc)));

        // More samples than the packets hold is refused by name, never a panic.
        let bytes = opus_container(0x0A02, &packets, 5000);
        let awc = AwcFile::parse(&bytes, AwcKey::default()).expect("the container parses");
        let lines = check(&awc);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].starts_with("00000A02  refused  "), "{lines:?}");
        assert!(refused(&lines), "a refusal fails the run");
        assert_eq!(peak(&[i16::MIN, 3]), 65_535);
    }

    #[test]
    fn a_pcm16_stream_is_decoded_and_a_codec_2_stream_refused_by_name() {
        let pcm: Vec<i16> = (0..500).map(|t| (t * 13 % 2000 - 1000) as i16).collect();
        let record = |codec: u8, stated: u32| {
            let mut record = vec![0u8; 32];
            record[0x00..0x04].copy_from_slice(&stated.to_le_bytes());
            record[0x08..0x0A].copy_from_slice(&48_000u16.to_le_bytes());
            record[0x0C..0x10].copy_from_slice(&500u32.to_le_bytes());
            (record[0x1C], record[0x1D]) = (codec, 1);
            record
        };
        let data: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        let bytes = single(0x0B01, &[(0x76, record(0, 500)), (0x55, data)]);
        let awc = AwcFile::parse(&bytes, AwcKey::default()).expect("the container parses");
        let want = format!("00000B01  decoded  500 samples  peak {}", peak(&pcm));
        assert_eq!(check(&awc), [want]);

        let spectra = [(0x76, record(2, 2048 * 20)), (0x55, vec![0; 8192 * 20])];
        let awc = AwcFile::parse(&single(0x0B02, &spectra), AwcKey::default());
        let lines = check(&awc.expect("the container parses"));
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].starts_with("00000B02  refused  "), "{lines:?}");
        assert!(lines[0].contains("codec 2"), "{lines:?}");
        assert!(refused(&lines));

        // A silent codec-2 stream as it is laid out: signal blocks, the zero block and the trailer.
        let mut data = vec![0u8; 2 * 8192];
        let mut zero = vec![0u8; 8192];
        (1025..2048).step_by(2).for_each(|f| zero[4 * f + 3] = 0x80);
        (2..19).for_each(|_| data.extend_from_slice(&zero));
        data.extend(0xFFFF_ACE1u32.to_le_bytes());
        data.resize(20 * 8192, 0);
        let spectra = [(0x76, record(2, 2048 * 20)), (0x55, data)];
        let awc = AwcFile::parse(&single(0x0B03, &spectra), AwcKey::default());
        let lines = check(&awc.expect("the container parses"));
        assert_eq!(lines, ["00000B03  decoded  500 samples  peak 0"]);
        assert!(!refused(&lines));
    }
}
