//! `openrage-cli rpf scan-expressions` lists a GTA VI expression set's tracks group by group, and
//! with `--id` writes one track's id where it lies and saves the set as a loose resource that the
//! verb reads again at offset 0.
//!
//! ⚠ The command prints with `println!`, which a test cannot read back in its own process, so it
//! runs in a child of this test binary that calls `or_cli::run` between two markers.

use std::io::Write;
use std::process::{Command, ExitCode};

const CHILD_ENV: &str = "OPENRAGE_EXPRESSIONS_ARGS";
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

/// What `openrage-cli <args>` prints, its errors after it, and whether it succeeded.
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
    let said = String::from_utf8_lossy(&output.stderr);
    (format!("{printed}{said}"), tail.starts_with('1'))
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

fn put(bytes: &mut [u8], at: usize, word: &[u8]) {
    bytes[at..at + word.len()].copy_from_slice(word);
}

/// A set of signature `0PhhPhPh` reading two facial controls, of 1 and 2 components, and writing
/// nothing, with a block map at 0x3F0 that counts one virtual block.
fn expression_set() -> Vec<u8> {
    let pointer = |to: u64| (0x5000_0000 + to).to_le_bytes();
    // The map's 16 bytes and its one slot end the section.
    let mut bytes = vec![0; 0x408];
    put(&mut bytes, 0x08, &pointer(0x3F0));
    put(
        &mut bytes,
        0x10,
        &[0x31, 0x32, 0x33, 0x34, 0x31, 0x32, 0x33, 0x34],
    );
    for (at, n, to) in [(0x18, 2u32, 0x1C0), (0x28, 3, 0x1D0), (0x38, 2, 0x1D8)] {
        put(&mut bytes, at, &n.to_le_bytes());
        put(&mut bytes, at + 4, &n.to_le_bytes());
        put(&mut bytes, at + 8, &pointer(to));
    }
    put(&mut bytes, 0x4C, &1u32.to_le_bytes());
    put(&mut bytes, 0x50, &1u32.to_le_bytes());
    let facial = or_io::gta6::hash::name_hash32(b"facialTranslation");
    for (k, id) in [0xA1u32, 0xA2].into_iter().enumerate() {
        put(&mut bytes, 0x1C0 + 8 * k, &id.to_le_bytes());
        put(&mut bytes, 0x1C4 + 8 * k, &facial.to_le_bytes());
    }
    put(&mut bytes, 0x1D0, &[0, 0, 1, 0, 2, 0]);
    put(&mut bytes, 0x1D8, &[1, 2]);
    bytes[0x3F8] = 1;
    bytes
}

#[test]
fn an_expression_set_lists_its_tracks_and_saves_an_edited_id_that_reads_back() {
    let mut bytes = rpf9_head();
    bytes.resize(0x1000, 0);
    bytes.extend(zlib(&expression_set()));
    let dir = tempfile::tempdir().expect("a folder");
    let archive = dir.path().join("anim.rpf");
    std::fs::write(&archive, &bytes).expect("written");
    let archive = archive.display().to_string();

    let (listed, ok) = run(&["rpf", "scan-expressions", &archive, "0x1000"]);
    assert!(ok, "{listed}");
    assert!(
        listed.contains("inputs          2 track(s) it reads"),
        "{listed}"
    );
    assert!(listed.contains("hash_000000A2"), "{listed}");
    assert!(listed.contains("slot(s) 1, 2"), "{listed}");
    assert!(listed.contains("outputs         none"), "{listed}");

    let saved = dir.path().join("set.rsc").display().to_string();
    let edit = [
        "rpf",
        "scan-expressions",
        &archive,
        "0x1000",
        "--id",
        "inputs",
        "1",
        "hash_0BADF00D",
        "--out",
        &saved,
    ];
    let (wrote, ok) = run(&edit);
    assert!(ok, "{wrote}");
    assert!(wrote.contains("now names hash_0BADF00D"), "{wrote}");

    let (again, ok) = run(&["rpf", "scan-expressions", &saved, "0"]);
    assert!(ok, "{again}");
    assert!(again.contains("hash_0BADF00D"), "{again}");
    assert!(!again.contains("hash_000000A2"), "{again}");
    assert!(again.contains("hash_000000A1"), "{again}");

    // A group the set does not hold, or a track past its tracks, is refused and writes nothing.
    let refused = dir.path().join("refused.rsc").display().to_string();
    let past = [
        "rpf",
        "scan-expressions",
        &archive,
        "0x1000",
        "--id",
        "inputs",
        "2",
        "7",
        "--out",
        &refused,
    ];
    let (said, ok) = run(&past);
    assert!(!ok, "{said}");
    assert!(!std::path::Path::new(&refused).exists());
}
