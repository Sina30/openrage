//! `openrage-cli ttpf replace` over a GTA VI texture pack built here: the texture is replaced from
//! a PNG into a new pack that `ttpf info` reads and `ttpf plan` finds in its canonical form, and a
//! replacement written over the pack it reads from is refused.
//!
//! ⚠ The command prints with `println!`, which a test cannot read back in its own process, so it
//! runs in a child of this test binary that calls `or_cli::run` between two markers.

use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitCode};

use or_io::codec::texture::{decode_level, save_png, RgbaImage};
use or_io::gta6::TexturePack;

const CHILD_ENV: &str = "OPENRAGE_TTPF_REPLACE_ARGS";
const CHILD_TEST: &str = "run_ttpf_for_a_parent";
const SEPARATOR: char = '\u{1f}';
const BEGIN: &str = "\u{1e}stdout begins\u{1e}";
const END: &str = "\u{1e}stdout ends\u{1e}";

/// Not a test of its own: the body a parent's child process runs.
#[test]
fn run_ttpf_for_a_parent() {
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

fn put(file: &mut [u8], at: usize, word: u32) {
    file[at..at + 4].copy_from_slice(&word.to_le_bytes());
}

/// A pack of one 128x64 BC1 texture, one 4 KiB tile of red blocks stored as row 1, laid out as the
/// container is measured to be: the table at 0x10, the directory at 0x100, the body after it.
fn pack() -> Vec<u8> {
    let mut file = vec![0u8; 0x1100];
    file[..4].copy_from_slice(b"RDS5");
    put(&mut file, 4, 3);
    put(&mut file, 8, 0x2000);
    file[0x0F] = 0x16;
    // Rows: the directory, the body at page 1, the sentinel at page 2.
    for (i, (offset, page)) in [(0x100u32, 0u32), (0x1100, 1), (0x2100, 2)]
        .into_iter()
        .enumerate()
    {
        put(&mut file, 0x10 + 8 * i, offset >> 8 | (page & 3) << 30);
        put(&mut file, 0x14 + 8 * i, page >> 2);
    }
    file[0x100..0x104].copy_from_slice(b"ETPF");
    put(&mut file, 0x104, 0x27A);
    put(&mut file, 0x108, 1);
    put(&mut file, 0x110, 127 | 63 << 14 | 1 << 28);
    put(&mut file, 0x114, 0x238 | 1 << 21 | 1 << 24);
    put(&mut file, 0x118, 1);
    put(&mut file, 0x120, 0x1234);
    file.extend([0x00, 0xF8, 0x00, 0xF8, 0, 0, 0, 0].repeat(0x200));
    file
}

#[test]
fn a_texture_is_replaced_from_a_png_into_a_new_pack_and_never_over_its_input() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let input = dir.path().join("pack.ttpf");
    std::fs::write(&input, pack()).expect("written");
    let image = dir.path().join("blue.png");
    let blue = RgbaImage {
        width: 128,
        height: 64,
        data: [0, 0, 0xFF, 0xFF].repeat(128 * 64),
        is_srgb: false,
    };
    save_png(&image, &blue).expect("a PNG");
    let output = dir.path().join("out.ttpf");
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let (input_s, image_s, output_s) = (text(&input), text(&image), text(&output));

    let (printed, ok) = run(&["ttpf", "replace", &input_s, "#0", &image_s, "-o", &output_s]);
    assert!(ok, "{printed}");
    assert!(printed.contains("1 mip(s)"), "{printed}");
    let replaced = TexturePack::open(&output).expect("the new pack opens");
    let read = decode_level(&replaced.texture(0).expect("reads"), 0).expect("decodes");
    assert!(read.data.chunks(4).all(|pixel| pixel == [0, 0, 0xFF, 0xFF]));
    assert_eq!(replaced.entries[0].key, 0x1234, "the texture keeps its key");
    assert!(
        std::fs::read(&input).expect("read") == pack(),
        "the input is untouched"
    );

    let (printed, ok) = run(&["ttpf", "info", &output_s]);
    assert!(ok && printed.contains("yes, 1 mip(s)"), "{printed}");
    let (printed, ok) = run(&["ttpf", "plan", &output_s]);
    assert!(ok && printed.contains("3 as planned, 0 not"), "{printed}");

    let (printed, ok) = run(&["ttpf", "replace", &input_s, "#0", &image_s, "-o", &input_s]);
    assert!(!ok, "a replacement over its input is refused: {printed}");
    assert!(std::fs::read(&input).expect("read") == pack());
    // A small one-mip texture given a chain of mips takes the flags its tiled pieces give.
    let (printed, ok) = run(&[
        "ttpf", "replace", &input_s, "#0", &image_s, "-o", &output_s, "-f", "DXT5",
    ]);
    assert!(ok, "{printed}");
    let chained = TexturePack::open(&output).expect("the new pack opens");
    let read = decode_level(&chained.texture(0).expect("reads"), 0).expect("decodes");
    assert!(read.data.chunks(4).all(|pixel| pixel == [0, 0, 0xFF, 0xFF]));
    let (printed, ok) = run(&["ttpf", "plan", &output_s]);
    assert!(ok && printed.contains("3 as planned, 0 not"), "{printed}");
    let (printed, ok) = run(&[
        "ttpf", "replace", &input_s, "#0", &image_s, "-o", &output_s, "-f", "DXT5", "-m", "1",
    ]);
    assert!(ok, "{printed}");
    assert!(
        printed.contains("no game has been seen to load"),
        "{printed}"
    );
    let reshaped = TexturePack::open(&output).expect("the new pack opens");
    assert_eq!(reshaped.entries[0].format_code >> 3, 77, "BC3");
    assert_eq!(reshaped.entries[0].key, 0x1234);
    let read = decode_level(&reshaped.texture(0).expect("reads"), 0).expect("decodes");
    assert!(read.data.chunks(4).all(|pixel| pixel == [0, 0, 0xFF, 0xFF]));
    let (printed, ok) = run(&["ttpf", "plan", &output_s]);
    assert!(ok && printed.contains("3 as planned, 0 not"), "{printed}");
}

#[test]
fn info_names_a_key_by_a_line_of_a_names_file_or_the_rules_from_a_drawable_s_name() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let names = dir.path().join("names.txt");
    std::fs::write(&names, "zq_lamp.#dr\n\n  zq_other_ab  \n").expect("written");
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let names_s = text(&names);
    let hash = |name: &str| or_io::gta6::hash::name_hash64(name.as_bytes());
    for (name, shown) in [
        ("zq_lamp_ab", "zq_lamp_ab"),
        ("ZQ_Other_AB", "zq_other_ab"),
        ("zq_lantern_ab", "hash_"),
    ] {
        let mut bytes = pack();
        bytes[0x120..0x128].copy_from_slice(&hash(name).to_le_bytes());
        let input = dir.path().join("pack.ttpf");
        std::fs::write(&input, bytes).expect("written");
        let (printed, ok) = run(&["ttpf", "info", &text(&input), "--names", &names_s]);
        assert!(ok, "{printed}");
        let row = (printed.lines()).find(|l| l.trim_start().starts_with("0 "));
        assert!(
            row.is_some_and(|row| row.contains(shown)),
            "{name}: {printed}"
        );
    }
}

#[test]
fn a_replacement_written_to_a_hard_link_of_its_input_leaves_the_input_whole() {
    let dir = tempfile::tempdir().expect("a scratch folder");
    let input = dir.path().join("pack.ttpf");
    std::fs::write(&input, pack()).expect("written");
    let link = dir.path().join("link.ttpf");
    std::fs::hard_link(&input, &link).expect("a hard link to the pack");
    let image = dir.path().join("blue.png");
    let blue = RgbaImage {
        width: 128,
        height: 64,
        data: [0, 0, 0xFF, 0xFF].repeat(128 * 64),
        is_srgb: false,
    };
    save_png(&image, &blue).expect("a PNG");
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let (input_s, image_s, link_s) = (text(&input), text(&image), text(&link));

    let (printed, ok) = run(&["ttpf", "replace", &input_s, "#0", &image_s, "-o", &link_s]);
    assert!(ok, "{printed}");
    assert!(
        std::fs::read(&input).expect("read") == pack(),
        "the input keeps every byte, though the output named the same file"
    );
    let replaced = TexturePack::open(&link).expect("the new pack opens");
    let read = decode_level(&replaced.texture(0).expect("reads"), 0).expect("decodes");
    assert!(read.data.chunks(4).all(|pixel| pixel == [0, 0, 0xFF, 0xFF]));
    assert!(
        !dir.path().join("link.ttpf.tmp").exists(),
        "no temporary file is left"
    );
}
