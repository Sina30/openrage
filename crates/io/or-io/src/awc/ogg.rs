//! Ogg Opus (RFC 7845) out of a stream's Opus packets, which are written as they are stored.

use super::opus::{OpusStream, OPUS_FRAME};
use super::refuse;
use crate::error::Result;

/// Samples at 48 kHz a decoder discards before the first it plays.
// Not stated by the container: inferred from the decode lining up with the stated peaks best at
// 312, libopus's own encoder delay at 48 kHz.
pub const PRE_SKIP: u16 = 312;

const BOS: u8 = 2;
const EOS: u8 = 4;
/// Lacing values a page holds at most.
const SEGMENTS: usize = 255;

/// The stream as an Ogg Opus file: `OpusHead`, `OpusTags`, then pages of whole packets.
pub fn ogg_opus(stream: &OpusStream) -> Result<Vec<u8>> {
    ogg_opus_tagged(stream, &[])
}

/// [`ogg_opus`] with `comments` as the user comments of its `OpusTags`, each a field and its value.
pub fn ogg_opus_tagged(stream: &OpusStream, comments: &[(String, String)]) -> Result<Vec<u8>> {
    if let Some(p) = stream.packets.iter().find(|p| p.len() / 255 + 1 > SEGMENTS) {
        return Err(refuse(format!(
            "an Opus packet of {} bytes is longer than one Ogg page laces",
            p.len()
        )));
    }
    let mut head = b"OpusHead".to_vec();
    head.extend([1, stream.channels]);
    head.extend(PRE_SKIP.to_le_bytes());
    // Every count the container states is in 48 kHz units; its own rate field is not the decode's.
    head.extend(48_000u32.to_le_bytes());
    head.extend([0, 0, 0]);
    let vendor = b"OpenRage";
    let mut tags = b"OpusTags".to_vec();
    tags.extend((vendor.len() as u32).to_le_bytes());
    tags.extend(vendor);
    tags.extend((comments.len() as u32).to_le_bytes());
    for (field, value) in comments {
        // A field's name is printable ASCII without the `=` that ends it.
        let named =
            !field.is_empty() && (field.bytes()).all(|b| (0x20..=0x7D).contains(&b) && b != b'=');
        if !named {
            return Err(refuse(format!("{field:?} is no comment field's name")));
        }
        tags.extend(((field.len() + 1 + value.len()) as u32).to_le_bytes());
        tags.extend(field.as_bytes());
        tags.push(b'=');
        tags.extend(value.as_bytes());
    }
    if tags.len() / 255 + 1 > SEGMENTS {
        return Err(refuse(format!(
            "comments of {} bytes are longer than one Ogg page laces",
            tags.len()
        )));
    }

    let mut out = Vec::new();
    let mut sequence = 0;
    let mut write = |packets: &[&[u8]], granule: u64, flags: u8| {
        page(&mut out, packets, granule, stream.id, sequence, flags);
        sequence += 1;
    };
    write(&[&head], 0, BOS);
    write(&[&tags], 0, 0);
    // RFC 7845's end trim: the last page's granule is the pre-skip and the samples to play.
    let all = u64::from(OPUS_FRAME) * stream.packets.len() as u64;
    let end = (u64::from(PRE_SKIP) + u64::from(stream.samples)).min(all);
    let (mut pending, mut segments, mut granule): (Vec<&[u8]>, usize, u64) = (Vec::new(), 0, 0);
    let mut flushed = 0;
    for packet in &stream.packets {
        let needs = packet.len() / 255 + 1;
        // A page ends before the packet the trim starts in, so the last page holds all it cuts.
        let trim_starts = granule <= end && end < granule + u64::from(OPUS_FRAME);
        if segments + needs > SEGMENTS || (trim_starts && flushed < granule) {
            write(&pending, granule, 0);
            flushed = granule;
            pending.clear();
            segments = 0;
        }
        pending.push(packet);
        segments += needs;
        granule += u64::from(OPUS_FRAME);
    }
    // A granule below the page before it would leave the last page a negative length.
    write(&pending, end.max(flushed), EOS);
    Ok(out)
}

fn page(out: &mut Vec<u8>, packets: &[&[u8]], granule: u64, serial: u32, sequence: u32, flags: u8) {
    let start = out.len();
    out.extend(b"OggS");
    out.extend([0, flags]);
    out.extend(granule.to_le_bytes());
    out.extend(serial.to_le_bytes());
    out.extend(sequence.to_le_bytes());
    out.extend([0; 4]);
    let lacing: Vec<u8> = packets
        .iter()
        .flat_map(|p| {
            let full = p.len() / 255;
            std::iter::repeat_n(255, full).chain([(p.len() - full * 255) as u8])
        })
        .collect();
    out.push(lacing.len() as u8);
    out.extend(lacing);
    packets.iter().for_each(|p| out.extend_from_slice(p));
    let crc = crc(&out[start..]);
    out[start + 22..start + 26].copy_from_slice(&crc.to_le_bytes());
}

/// Ogg's CRC-32: polynomial 0x04C11DB7, not reflected, starting from zero, not inverted.
fn crc(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |mut c, b| {
        c ^= u32::from(*b) << 24;
        for _ in 0..8 {
            c = if c & 0x8000_0000 != 0 {
                (c << 1) ^ 0x04C1_1DB7
            } else {
                c << 1
            };
        }
        c
    })
}

#[cfg(test)]
mod tests {
    use super::super::opus::tests::{mono, packet};
    use super::*;

    /// Each page of an Ogg file: its flags, granule, sequence, and the packets it completes.
    fn pages(file: &[u8]) -> Vec<(u8, u64, u32, Vec<Vec<u8>>)> {
        let mut out = Vec::new();
        let mut at = 0;
        while at < file.len() {
            assert_eq!(&file[at..at + 4], b"OggS", "page at {at}");
            assert_eq!(file[at + 4], 0, "version");
            let n = usize::from(file[at + 26]);
            let lacing = &file[at + 27..at + 27 + n];
            let body: usize = lacing.iter().map(|l| usize::from(*l)).sum();
            let end = at + 27 + n + body;
            let mut copy = file[at..end].to_vec();
            copy[22..26].fill(0);
            let stated = u32::from_le_bytes(file[at + 22..at + 26].try_into().expect("four"));
            assert_eq!(crc(&copy), stated, "page at {at}");
            let mut packets = vec![Vec::new()];
            let mut cursor = at + 27 + n;
            for l in lacing {
                let l = usize::from(*l);
                packets
                    .last_mut()
                    .expect("open")
                    .extend(&file[cursor..cursor + l]);
                cursor += l;
                if l < 255 {
                    packets.push(Vec::new());
                }
            }
            assert!(
                packets.pop().expect("open").is_empty(),
                "a packet continues"
            );
            let granule = u64::from_le_bytes(file[at + 6..at + 14].try_into().expect("eight"));
            let sequence = u32::from_le_bytes(file[at + 18..at + 22].try_into().expect("four"));
            out.push((file[at + 5], granule, sequence, packets));
            at = end;
        }
        out
    }

    /// The catalogued check value of this CRC (CRC-32/POSIX before its final inversion).
    #[test]
    fn the_page_checksum_is_oggs_crc() {
        assert_eq!(crc(b"123456789"), !0x765E_7680);
    }

    #[test]
    fn a_stream_writes_out_as_ogg_opus_that_parses_back_page_by_page() {
        let mut packets = mono(700, 9);
        packets.push(packet(0xF8, 600, 1));
        packets.push(packet(0xF8, 255, 2));
        let stream = OpusStream {
            id: 0x0BAD_CAFE,
            channels: 1,
            samples: 960 * 702 - 2000,
            packets: packets.clone(),
        };
        let pages = pages(&ogg_opus(&stream).expect("written"));
        assert!(pages.len() > 4, "{} pages", pages.len());
        let head = &pages[0].3[0];
        assert_eq!(&head[..8], b"OpusHead");
        assert_eq!((head[8], head[9]), (1, 1));
        assert_eq!(u16::from_le_bytes([head[10], head[11]]), PRE_SKIP);
        assert_eq!(&pages[1].3[0][..8], b"OpusTags");
        let mut read = Vec::new();
        for (i, (flags, granule, sequence, packets)) in pages.iter().enumerate() {
            assert_eq!(*sequence, i as u32);
            let want = match i {
                0 => BOS,
                _ if i == pages.len() - 1 => EOS,
                _ => 0,
            };
            assert_eq!(*flags, want, "page {i}");
            if i >= 2 {
                read.extend(packets.iter().cloned());
                if i < pages.len() - 1 {
                    assert_eq!(*granule, 960 * read.len() as u64, "page {i}");
                }
            }
        }
        assert_eq!(read, packets);
        let end = u64::from(PRE_SKIP) + u64::from(stream.samples);
        assert_eq!(pages.last().expect("pages").1, end);
    }

    /// The last page's granule trims the decode to the samples the container states, and is
    /// never below the page before it, even when the trim spans more than one packet.
    #[test]
    fn the_last_page_ends_the_decode_at_the_stated_samples() {
        // Three of these fill a page's 255 lacing values.
        let packets: Vec<_> = (0..7).map(|i| packet(0xF8, 255 * 84, i)).collect();
        let samples = 960 * 7 - 1500 - u32::from(PRE_SKIP);
        let stream = OpusStream {
            id: 1,
            channels: 1,
            samples,
            packets,
        };
        let pages = pages(&ogg_opus(&stream).expect("written"));
        let granules: Vec<u64> = pages.iter().map(|p| p.1).collect();
        assert!(granules.is_sorted(), "{granules:?}");
        assert_eq!(granules.last(), Some(&(u64::from(samples) + 312)));
    }

    #[test]
    fn comments_are_written_into_the_tags_packet_and_a_bad_field_is_refused() {
        let stream = OpusStream {
            id: 7,
            channels: 1,
            samples: 960 * 3,
            packets: mono(3, 1),
        };
        let comment = |f: &str, v: &str| (f.to_string(), v.to_string());
        let comments = [
            comment("TITLE", "A_LINE_01"),
            comment("SUBTITLE", "Bonjour, là"),
        ];
        let tagged = pages(&ogg_opus_tagged(&stream, &comments).expect("written"));
        let tags = &tagged[1].3[0];
        let word = |at: usize| u32::from_le_bytes(tags[at..at + 4].try_into().expect("four"));
        let mut at = 12 + word(8) as usize;
        assert_eq!(word(at), 2);
        at += 4;
        let mut read = Vec::new();
        for _ in 0..2 {
            let len = word(at) as usize;
            read.push(String::from_utf8(tags[at + 4..at + 4 + len].to_vec()).expect("UTF-8"));
            at += 4 + len;
        }
        assert_eq!(read, ["TITLE=A_LINE_01", "SUBTITLE=Bonjour, là"]);
        assert_eq!(at, tags.len());
        // The audio pages are those of the file without comments.
        let plain = pages(&ogg_opus(&stream).expect("written"));
        assert_eq!(tagged[2..], plain[2..]);

        for field in ["", "A=B", "É"] {
            let refused = ogg_opus_tagged(&stream, &[comment(field, "x")]);
            assert!(refused.is_err(), "{field:?}");
        }
        let long = [comment("SUBTITLE", &"x".repeat(70_000))];
        assert!(ogg_opus_tagged(&stream, &long).is_err());
    }

    #[test]
    fn a_packet_longer_than_one_page_laces_is_refused() {
        let stream = OpusStream {
            id: 1,
            channels: 1,
            samples: 960,
            packets: vec![
                packet(0xF8, 3, 0),
                packet(0xF8, 70_000, 1),
                packet(0xF8, 3, 2),
            ],
        };
        let why = format!("{}", ogg_opus(&stream).expect_err("refused"));
        assert!(why.contains("70000 bytes"), "{why}");
        let mut most = stream.clone();
        most.packets[1].truncate(255 * 255 - 1);
        let read: Vec<_> = pages(&ogg_opus(&most).expect("written"))
            .into_iter()
            .skip(2)
            .flat_map(|p| p.3)
            .collect();
        assert_eq!(read, most.packets);
    }
}
