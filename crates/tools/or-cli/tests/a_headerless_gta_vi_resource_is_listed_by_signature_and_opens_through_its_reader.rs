//! `openrage-cli rpf scan` names a headerless GTA VI resource by the family its layout places it
//! in, and `rpf scan-open` reads a sized root with the physical block its block map counts, and
//! refuses a resource whose block map counts no block.
//!
//! ⚠ The command prints with `println!`, which a test cannot read back in its own process, so it
//! runs in a child of this test binary that calls `or_cli::run` between two markers.

use std::io::Write;
use std::process::{Command, ExitCode};

const CHILD_ENV: &str = "OPENRAGE_HEADERLESS_ARGS";
const CHILD_TEST: &str = "run_the_cli_for_a_parent";
const SEPARATOR: char = '\u{1f}';
const BEGIN: &str = "\u{1e}stdout begins\u{1e}";
const END: &str = "\u{1e}stdout ends\u{1e}";

/// Not a test of its own: the body a parent's child process runs.
#[test]
fn run_the_cli_for_a_parent() {
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
    let toc_end = 0x1A0u32;
    let mut bytes = vec![0x5Au8; toc_end as usize];
    bytes[..0x40].fill(0);
    bytes[..4].copy_from_slice(b"9FPR");
    bytes[0x04..0x08].copy_from_slice(&toc_end.to_le_bytes());
    bytes[0x08..0x0C].copy_from_slice(&(toc_end - 0x10).to_le_bytes());
    bytes[0x0C..0x10].copy_from_slice(&0x40u32.to_le_bytes());
    bytes[0x1C..0x20].copy_from_slice(&0x150u32.to_le_bytes());
    bytes[0x22..0x24].copy_from_slice(&1u16.to_le_bytes());
    bytes[0x24..0x28].copy_from_slice(&1u32.to_le_bytes());
    bytes[0x30..0x34].copy_from_slice(&0x10u32.to_le_bytes());
    bytes[0x2C] = 1;
    (bytes[0x19], bytes[0x1A], bytes[0x20]) = (0x7A, 4, 5);
    (bytes[0x2D], bytes[0x2E]) = (0x0F, 0x09);
    bytes
}

/// `data` as a zlib stream of one stored deflate block, under the `78 DA` head a scan looks for.
fn zlib(data: &[u8]) -> Vec<u8> {
    let len = u16::try_from(data.len()).expect("one stored block");
    let mut out = vec![0x78, 0xDA, 0x01];
    out.extend(len.to_le_bytes());
    out.extend((!len).to_le_bytes());
    out.extend_from_slice(data);
    let (a, b) = data.iter().fold((1u32, 0u32), |(a, b), &c| {
        let a = (a + u32::from(c)) % 65_521;
        (a, (b + a) % 65_521)
    });
    out.extend(((b << 16) | a).to_be_bytes());
    out
}

/// The bytes of the one instance's serialized structure.
const CONTENT: u32 = 0x200;

/// A 208-byte root of signature `0PhPGPPP` of one instance whose record states [`CONTENT`]
/// bytes of structure, and a block map whose counts state one virtual and one physical block.
fn sized_root() -> Vec<u8> {
    let mut v = vec![0u8; 208];
    let slots: [u64; 8] = [
        0,
        0x5000_0080,
        1,
        0x5000_00A0,
        0x6000_0000,
        0x5000_00B0,
        0x5000_00C0,
        0x5000_0050,
    ];
    for (i, slot) in slots.iter().enumerate() {
        v[8 * i..8 * i + 8].copy_from_slice(&slot.to_le_bytes());
    }
    v[0x88] = 1;
    v[0x89] = 1;
    for k in [0, 5, 10] {
        v[0x50 + 4 * k..0x54 + 4 * k].copy_from_slice(&1f32.to_le_bytes());
    }
    v[0xA0..0xA4].copy_from_slice(&CONTENT.to_le_bytes());
    v[0xA4..0xA8].copy_from_slice(&0x100u32.to_le_bytes());
    v
}

/// The serialized structure of [`CONTENT`] bytes the root's one record states: its head, then nodes.
fn structure() -> Vec<u8> {
    let mut s = vec![0u8; CONTENT as usize];
    let serialized = u64::from(CONTENT) - 0x40;
    s[0x20..0x28].copy_from_slice(&serialized.to_le_bytes());
    s[0x28..0x30].copy_from_slice(&(serialized - 0x100).to_le_bytes());
    for (at, word) in [
        (0x100, 0x8102_0006u32),
        (0x104, 9),
        (0x10C, 7),
        (0x128, 0x80),
    ] {
        s[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    for (k, f) in [-1f32, -1.0, -1.0, 1.0, 1.0, 1.0].iter().enumerate() {
        s[0x110 + 4 * k..0x114 + 4 * k].copy_from_slice(&f.to_le_bytes());
    }
    s[0x140..serialized as usize].fill(0x33);
    s
}

/// A root whose second slot is its only pointer, at a block map that counts no block.
fn unknown_root() -> Vec<u8> {
    let mut v = vec![0u8; 0x100];
    v[8..16].copy_from_slice(&0x5000_0080u64.to_le_bytes());
    v
}

#[test]
fn an_instance_root_is_listed_by_signature_and_reads_its_structures_from_the_physical_run() {
    let mut bytes = rpf9_head();
    bytes.resize(0x1000, 0);
    let root_at = bytes.len();
    bytes.extend(zlib(&sized_root()));
    bytes.resize(bytes.len().next_multiple_of(16), 0);
    bytes.extend(zlib(&structure()));
    bytes.resize(0x2000, 0);
    let unknown_at = bytes.len();
    bytes.extend(zlib(&unknown_root()));
    let dir = tempfile::tempdir().expect("a folder");
    let archive = dir.path().join("core.rpf");
    std::fs::write(&archive, &bytes).expect("written");
    let archive = archive.display().to_string();

    let (listed, ok) = run(&["rpf", "scan", &archive]);
    assert!(ok, "{listed}");
    let line = listed
        .lines()
        .find(|l| l.contains(&format!("{root_at:#012X}")))
        .unwrap_or_else(|| panic!("no line for the root in {listed}"));
    assert!(line.contains("headerless acceleration structure"), "{line}");
    assert!(line.contains("block map v=1 p=1"), "{line}");
    assert!(
        listed.contains("2 start(s), with 1 continuation block(s)"),
        "{listed}"
    );

    let (opened, ok) = run(&["rpf", "scan-open", &archive, &format!("{root_at:#X}")]);
    assert!(ok, "{opened}");
    assert!(
        opened.contains("signature 0PhPGPPP, role acceleration structure"),
        "{opened}"
    );
    assert!(
        opened.contains(&format!("tile {CONTENT} bytes of the physical section")),
        "{opened}"
    );
    assert!(
        opened.contains("serialized 448 bytes, 7 primitives under a count of 9"),
        "{opened}"
    );

    let (refused, ok) = run(&["rpf", "scan-open", &archive, &format!("{unknown_at:#X}")]);
    assert!(!ok, "{refused}");
}
