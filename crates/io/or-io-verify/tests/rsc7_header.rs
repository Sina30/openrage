//! The RSC7 header through its declaration, over every RSC7 file of the
//! corpus whatever its format: the typed read against the header's own
//! words, the envelope's reading, the page sizes the flags state, and the
//! header written back.

mod declared;

use declared::{corpus_root, name};
use or_io::container::resource::{DatResourceFileHeader, ResourceEnvelope, Rsc7};
use or_io::decl::{Aligned, Owned, Plain, Raw, Rx, View};
use or_io::desc::{Image, Value};
use or_test_support::Corpus;

/// Every format whose files are RSC7, declared or not: the header is the
/// container's, not the format's.
const EXTENSIONS: &[&str] = &[
    "ybd", "ybn", "ycd", "ydd", "ydr", "yed", "yfd", "yft", "yld", "ynd", "ynv", "ypt", "ysc",
    "ytd",
];

/// Every RSC7 file with its first sixteen bytes: the header is all but one
/// test reads, and holding every whole file cost about 560 MB a test.
fn corpus() -> Option<Vec<(std::path::PathBuf, Vec<u8>)>> {
    use std::io::Read;
    let root = corpus_root()?;
    let mut out = Vec::new();
    for ext in EXTENSIONS {
        let Some(paths) = Corpus::with_root(root.clone(), ext)
            .all_targets()
            .find_or_skip("the RSC7 header")
        else {
            continue;
        };
        for path in paths {
            let mut head = Vec::with_capacity(0x10);
            std::fs::File::open(&path)
                .and_then(|f| f.take(0x10).read_to_end(&mut head))
                .expect("a corpus file reads");
            if head.starts_with(b"RSC7") {
                out.push((path, head));
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn typed(bytes: &[u8]) -> Raw<DatResourceFileHeader> {
    let aligned = Aligned::from_bytes(&bytes[..0x10]);
    *View::get(
        &View::<Raw<DatResourceFileHeader>>::at_offset(Image::new(aligned.bytes(), 0x10), 0)
            .unwrap(),
    )
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[test]
fn every_rsc7_header_reads_typed_as_the_envelope_reads() {
    let Some(files) = corpus() else {
        return;
    };
    let (mut same, mut nibbles, mut failures) = (0usize, 0usize, Vec::new());
    let tree = Value::Bytes(Vec::new());
    let rx = Rx::new(&tree);
    for (path, bytes) in &files {
        let header = typed(bytes);
        let words = [
            word(bytes, 0),
            word(bytes, 4),
            word(bytes, 8),
            word(bytes, 12),
        ];
        let peeked = Rsc7::peek(bytes).expect("an RSC7 resource");
        let parsed = DatResourceFileHeader::parse(bytes).expect("parses");
        if [
            header.magic,
            header.version,
            header.info.r#virtual,
            header.info.physical,
        ] == words
            && DatResourceFileHeader::from_raw(&header, &[], 0, &rx).ok() == Some(parsed)
            && header.version == peeked.version()
        {
            same += 1;
        } else {
            failures.push(format!("{}: {header:?} against {words:x?}", name(path)));
        }
        let split = ((header.info.r#virtual >> 28) << 4) | (header.info.physical >> 28);
        nibbles += usize::from(split == header.version & 0xff);
    }
    println!(
        "{} RSC7 files: {same} headers read typed as their words and the envelope read them; \
         {nibbles} carry the version again in their flag words' top nibbles",
        files.len()
    );
    failures.truncate(20);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(same, files.len());
}

#[test]
fn every_rsc7_header_writes_back_to_its_own_bytes() {
    let Some(files) = corpus() else {
        return;
    };
    let mut failures = Vec::new();
    for (path, bytes) in &files {
        let mut out = Vec::new();
        DatResourceFileHeader::parse(bytes)
            .unwrap()
            .write_to(&mut out);
        if out != bytes[..0x10] {
            failures.push(name(path));
        }
    }
    println!(
        "{} RSC7 headers: {} write back to their own sixteen bytes",
        files.len(),
        files.len() - failures.len()
    );
    assert!(failures.is_empty(), "{failures:?}");
}

/// What the flag words state the two heaps to be, against what decompresses.
///
/// ⚠ Not an oracle for the declaration: the envelope sizes the heaps through
/// the same header, so a swapped flag pair moves both sides. The typed read
/// against the file's own words is what catches that.
#[test]
fn every_rsc7_header_states_the_size_its_pages_decompress_to() {
    let Some(files) = corpus() else {
        return;
    };
    let (mut exact, mut failures) = (0usize, Vec::new());
    for (path, _) in &files {
        let bytes = std::fs::read(path).expect("a corpus file reads");
        let header = DatResourceFileHeader::parse(&bytes).unwrap();
        let opened = Rsc7::open(&bytes).unwrap();
        let stated = (
            header.virtual_page_size() as usize,
            header.physical_page_size() as usize,
        );
        let virtual_size = opened.virtual_size as usize;
        let held = (virtual_size, opened.data.len() - virtual_size);
        if stated == held && opened.version == header.version {
            exact += 1;
        } else {
            failures.push(format!(
                "{}: the flags state {stated:?} bytes and {held:?} decompress; the envelope \
                 reads version {} and the header states {}",
                name(path),
                opened.version,
                header.version
            ));
        }
    }
    println!(
        "{} RSC7 files: {exact} decompress to exactly the two heap sizes their flags state",
        files.len()
    );
    failures.truncate(20);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_header_without_the_magic_is_refused() {
    let mut bytes = [0u8; 0x10];
    bytes[..4].copy_from_slice(b"RSC8");
    let why = DatResourceFileHeader::parse(&bytes)
        .expect_err("no magic")
        .to_string();
    println!("{why}");
    assert!(DatResourceFileHeader::parse(&bytes[..0x0f]).is_err());
}

#[test]
fn the_header_describes_itself_at_the_offsets_it_is_read_at() {
    use std::mem::{offset_of, size_of};
    type Header = Raw<DatResourceFileHeader>;
    type Info = Raw<or_io::container::resource::DatResourceInfo>;
    let pack = or_io::decl::describe("gta5", "gen8", &[or_io::decl::root::<Header>])
        .expect("a description");
    let fields = |name: &str| -> (usize, Vec<(String, usize)>) {
        let ty = pack.types.iter().find(|t| t.name == name).unwrap();
        let got = ty
            .fields
            .iter()
            .map(|f| (f.name.clone(), f.offset))
            .collect();
        (ty.size, got)
    };
    let header = fields("DatResourceFileHeader");
    let info = fields("DatResourceInfo");
    println!("{header:?} {info:?}");
    assert_eq!(
        header,
        (
            size_of::<Header>(),
            vec![
                ("magic".to_string(), offset_of!(Header, magic)),
                ("version".to_string(), offset_of!(Header, version)),
                ("info".to_string(), offset_of!(Header, info)),
            ]
        )
    );
    assert_eq!(
        info,
        (
            size_of::<Info>(),
            vec![
                ("virtual".to_string(), offset_of!(Info, r#virtual)),
                ("physical".to_string(), offset_of!(Info, physical)),
            ]
        )
    );
    let bytes = b"RSC7\x05\0\0\0\x01\0\0\0\x02\0\0\0";
    assert_eq!(Plain::as_bytes(&typed(bytes)), bytes);
}

#[test]
#[ignore = "a timing, meaningful only in a release build; run by name"]
fn what_an_rsc7_header_read_costs() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    let Some(files) = corpus() else {
        return;
    };
    let heads: Vec<Aligned> = files
        .iter()
        .map(|(_, b)| Aligned::from_bytes(&b[..0x10]))
        .collect();
    let best = |f: &mut dyn FnMut() -> u64| {
        let mut best = Duration::MAX;
        for _ in 0..5 {
            let start = Instant::now();
            let mut sink = 0u64;
            for _ in 0..200 {
                sink = sink.wrapping_add(f());
            }
            black_box(sink);
            best = best.min(start.elapsed());
        }
        best.as_secs_f64() * 1e9 / 200.0 / files.len() as f64
    };
    let typed_ns = best(&mut || {
        heads
            .iter()
            .map(|h| {
                let v = View::<Raw<DatResourceFileHeader>>::at_offset(
                    Image::new(black_box(h.bytes()), 0x10),
                    0,
                )
                .unwrap();
                u64::from(v.version) + u64::from(View::get(&v).info.r#virtual)
            })
            .sum()
    });
    let words_ns = best(&mut || {
        heads
            .iter()
            .map(|h| {
                let b = black_box(h.bytes());
                u64::from(word(b, 4)) + u64::from(word(b, 8))
            })
            .sum()
    });
    println!(
        "{} headers: version and virtual flags typed {typed_ns:.2} ns a header, read as words \
         {words_ns:.2} ns",
        files.len()
    );
}
