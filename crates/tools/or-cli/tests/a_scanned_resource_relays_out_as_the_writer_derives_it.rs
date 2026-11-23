//! `openrage-cli rpf scan-relayout` over a synthetic RPF9 archive: a resource the writer's rules
//! lay out as it is stored, one whose block map counts an empty tail, and a stored RSC8.
//!
//! ⚠ The command prints with `println!`, which a test cannot read back in its own process, so it
//! runs in a child of this test binary that calls `or_cli::run` between two markers.

use std::io::Write;
use std::process::{Command, ExitCode};

const CHILD_ENV: &str = "OPENRAGE_SCAN_RELAYOUT_ARGS";
const CHILD_TEST: &str = "run_scan_relayout_for_a_parent";
const SEPARATOR: char = '\u{1f}';
const BEGIN: &str = "\u{1e}stdout begins\u{1e}";
const END: &str = "\u{1e}stdout ends\u{1e}";

/// Not a test of its own: the body a parent's child process runs.
#[test]
fn run_scan_relayout_for_a_parent() {
    let Some(args) = std::env::var_os(CHILD_ENV) else {
        return;
    };
    let args: Vec<String> = args
        .to_string_lossy()
        .split(SEPARATOR)
        .map(str::to_string)
        .collect();
    let mut out = std::io::stdout();
    let _ = write!(out, "{BEGIN}");
    let _ = out.flush();
    let code = or_cli::run(&args);
    let _ = out.flush();
    let _ = write!(out, "{END}{}", u8::from(code == ExitCode::SUCCESS));
    let _ = out.flush();
}

/// What `openrage-cli <args>` prints, and whether it succeeded.
fn run(args: &[&str]) -> (String, bool) {
    let output = Command::new(std::env::current_exe().expect("this test binary"))
        .args([CHILD_TEST, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, args.join(&SEPARATOR.to_string()))
        .env("OPENRAGE_SCHEMA", "none")
        .output()
        .expect("this test binary runs as a child");
    let text = String::from_utf8_lossy(&output.stdout);
    let (_, rest) = text
        .split_once(BEGIN)
        .unwrap_or_else(|| panic!("the child printed no start marker: {text}"));
    let (printed, tail) = rest.split_once(END).unwrap_or_else(|| {
        panic!(
            "the command panicked: {rest}{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (printed.to_string(), tail.starts_with('1'))
}

/// An RPF9 head of layout 4 that holds every measured rule, and its table to 0x1A0.
fn rpf9_head() -> Vec<u8> {
    let le32 = |v: u32| v.to_le_bytes();
    let toc_end = 0x1A0u32;
    let mut bytes = vec![0x5Au8; toc_end as usize];
    bytes[..0x40].fill(0);
    bytes[..4].copy_from_slice(b"9FPR");
    bytes[0x04..0x08].copy_from_slice(&le32(toc_end));
    bytes[0x08..0x0C].copy_from_slice(&le32(toc_end - 0x10));
    bytes[0x0C..0x10].copy_from_slice(&le32(0x40));
    bytes[0x1C..0x20].copy_from_slice(&le32(0x150));
    bytes[0x22..0x24].copy_from_slice(&1u16.to_le_bytes());
    bytes[0x24..0x28].copy_from_slice(&le32(1));
    bytes[0x30..0x34].copy_from_slice(&le32(0x10));
    bytes[0x2C] = 1;
    (bytes[0x19], bytes[0x1A], bytes[0x20]) = (0x7A, 4, 5);
    (bytes[0x2D], bytes[0x2E]) = (0x0F, 0x09);
    bytes
}

/// `data` as a zlib stream of stored deflate blocks, with the head the archives use.
fn zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0xDA];
    let pieces: Vec<&[u8]> = data.chunks(0xFFFF).collect();
    for (i, piece) in pieces.iter().enumerate() {
        out.push(u8::from(i + 1 == pieces.len()));
        let len = piece.len() as u16;
        out.extend(len.to_le_bytes());
        out.extend((!len).to_le_bytes());
        out.extend_from_slice(piece);
    }
    let (a, b) = data.iter().fold((1u32, 0u32), |(a, b), &c| {
        let a = (a + u32::from(c)) % 65_521;
        (a, (b + a) % 65_521)
    });
    out.extend((b << 16 | a).to_be_bytes());
    out
}

/// A virtual section of `len` bytes whose block map at 0x40 counts `counts`, the rest filler.
fn virtual_section(len: usize, counts: (u8, u8)) -> Vec<u8> {
    let mut v = vec![0x11u8; len];
    v[..8].fill(0);
    v[8..16].copy_from_slice(&0x5000_0040u64.to_le_bytes());
    v[0x40..0x60].fill(0);
    (v[0x48], v[0x49]) = counts;
    v
}

#[test]
fn each_resource_says_whether_its_layout_is_what_the_writer_derives_from_its_sections() {
    let mut bytes = rpf9_head();
    let mut starts = Vec::new();
    let blocks: [&[&[u8]]; 2] = [
        &[&virtual_section(0x200, (1, 1)), &[0x22; 0x500]],
        // A whole 64 KiB that counts a tail it does not write.
        &[&virtual_section(0x1_0000, (2, 0))],
    ];
    for resource in blocks {
        bytes.resize(bytes.len().next_multiple_of(0x1000) + 16, 0);
        starts.push(bytes.len());
        for block in resource {
            bytes.resize(bytes.len().next_multiple_of(16), 0);
            bytes.extend(zlib(block));
        }
    }
    bytes.resize(bytes.len().next_multiple_of(0x1000), 0);
    starts.push(bytes.len());
    let mut rsc8 = b"RSC8\x41\x1F\x00\x00".to_vec();
    rsc8.extend((0x100u32 | 4).to_le_bytes());
    rsc8.extend(1u32.to_le_bytes());
    rsc8.extend(virtual_section(0x100, (1, 0)));
    bytes.extend(rsc8);
    bytes.resize(bytes.len().next_multiple_of(0x1000) + 0x1000, 0);
    let dir = tempfile::tempdir().expect("a folder");
    let archive = dir.path().join("pack.rpf");
    std::fs::write(&archive, &bytes).expect("written");
    let archive = archive.display().to_string();

    let (said, ok) = run(&["rpf", "scan-relayout", &archive]);
    assert!(ok, "{said}");
    let line = |at: usize| {
        (said.lines())
            .find(|l| l.contains(&format!("{at:#012X}")))
            .unwrap_or_else(|| panic!("no line for {at:#X} in {said}"))
            .to_string()
    };
    assert!(line(starts[0]).contains("blocks the same"), "{said}");
    assert!(line(starts[0]).contains("block map the same"), "{said}");
    // The writer keeps the empty tail's count the stored map states.
    let e1 = line(starts[1]);
    assert!(e1.contains("blocks the same"), "{e1}");
    assert!(e1.contains("block map the same"), "{e1}");
    assert!(
        line(starts[2]).contains("stored rsc8  head the same"),
        "{said}"
    );
    assert!(
        said.contains("summary: 3 resources; blocks the same 2 of 2 (100.00%); block map the same 2 of 2 (100.00%); RSC8 head the same 1 of 1 (100.00%)"),
        "{said}"
    );

    let (said, ok) = run(&[
        "rpf",
        "scan-relayout",
        &archive,
        "--every",
        "2",
        "--count",
        "1",
    ]);
    assert!(ok, "{said}");
    assert!(said.contains("summary: 1 resources"), "{said}");
}
