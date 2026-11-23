//! `openrage-cli rpf scan-open` over GTA VI animation resources built here: a headerless clip
//! dictionary in a zlib run and the same dictionary stored as a version 65 RSC8 each print their
//! root, set, animation, DOF, block and clip, and a root of no known kind is refused.
//!
//! ⚠ The command prints with `println!`, which a test cannot read back in its own process, so it
//! runs in a child of this test binary that calls `or_cli::run` between two markers.

use std::io::Write;
use std::mem::offset_of;
use std::process::{Command, ExitCode};

use or_io::decl::Raw;
use or_io::gta5::CrBlockStream;
use or_io::hash::string_hash;

const CHILD_ENV: &str = "OPENRAGE_ANIM_SCAN_OPEN_ARGS";
const CHILD_TEST: &str = "run_anim_scan_open_for_a_parent";
const SEPARATOR: char = '\u{1f}';
const BEGIN: &str = "\u{1e}stdout begins\u{1e}";
const END: &str = "\u{1e}stdout ends\u{1e}";
const VIRTUAL: u64 = 0x5000_0000;

/// Not a test of its own: the body a parent's child process runs.
#[test]
fn run_anim_scan_open_for_a_parent() {
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

fn hash(name: &str) -> u32 {
    string_hash(name.as_bytes())
}

/// A virtual section laid out object by object, each on 16 bytes.
#[derive(Default)]
struct Bytes(Vec<u8>);

impl Bytes {
    fn alloc(&mut self, len: usize) -> usize {
        let at = self.0.len().next_multiple_of(16);
        self.0.resize(at + len, 0);
        at
    }
    fn put(&mut self, at: usize, bytes: &[u8]) {
        self.0[at..at + bytes.len()].copy_from_slice(bytes);
    }
    fn u32(&mut self, at: usize, v: u32) {
        self.put(at, &v.to_le_bytes());
    }
    fn ptr(&mut self, at: usize, to: usize) {
        self.put(at, &(VIRTUAL + to as u64).to_le_bytes());
    }
    fn inline_name(&mut self, at: usize, s: &str) {
        self.put(at, s.as_bytes());
        self.0[at + 15] = 0x80 | s.len() as u8;
    }
    fn array(&mut self, at: usize, first: usize, count: u16) {
        self.ptr(at, first);
        self.put(at + 8, &count.to_le_bytes());
        self.put(at + 10, &count.to_le_bytes());
    }
    /// A map of one entry whose next pointer is at 0x10, in a table of `slots` buckets.
    fn map(&mut self, at: usize, slots: u32, key: u64, entry: usize) {
        let buckets = self.alloc(slots as usize * 8);
        self.ptr(buckets + (key % u64::from(slots)) as usize * 8, entry);
        self.ptr(at, buckets);
        self.u32(at + 8, slots);
        self.u32(at + 0x10, 1);
        self.0[at + 0x17] = 1;
    }
}

const DICTIONARY: &str = "dict_a";
const ANIMATION: &str = "walk";
const FRAMES: u16 = 31;

/// A clip dictionary of one set holding one animation of one float DOF, a constant in directory
/// section 2, and one clip that plays it whole.
fn clip_dictionary() -> Vec<u8> {
    dictionary_of(hash("SKEL_ROOT"), "genericControl", 4, 2, &[0.5])
}

/// The same with its one DOF of `id`, `track` and `kind` a constant of `constants` in directory
/// section `section`.
fn dictionary_of(id: u32, track: &str, kind: u32, section: usize, constants: &[f32]) -> Vec<u8> {
    let mut b = Bytes::default();
    let root = b.alloc(0xB0);
    b.inline_name(root + 0x38, DICTIONARY);
    b.u32(root + 0x68, hash(DICTIONARY));
    b.u32(root + 0x6C, 2);
    let set = b.alloc(0x80);
    let set_name = format!("{DICTIONARY}-0");
    b.inline_name(set + 0x38, &set_name);
    b.u32(set + 0x68, hash(&set_name));

    let animation = b.alloc(0x70);
    b.ptr(animation + 0x18, set);
    b.put(animation + 0x20, &0x140u16.to_le_bytes());
    b.put(animation + 0x24, &FRAMES.to_le_bytes());
    b.put(animation + 0x26, &32u16.to_le_bytes());
    b.put(animation + 0x28, &1.0f32.to_le_bytes());
    let name = b.alloc(16);
    b.put(name, format!("pack:/{ANIMATION}").as_bytes());
    b.ptr(animation + 0x30, name);
    let dof = b.alloc(12);
    b.u32(dof, id);
    b.u32(dof + 4, hash(track));
    b.u32(dof + 8, kind);
    b.array(animation + 0x60, dof, 1);

    // The constants, then 11 section counts with one entry in `section`, padded to four with 4.
    let mut data: Vec<u8> = constants.iter().flat_map(|c| c.to_le_bytes()).collect();
    let mut words = [0u16; 11].to_vec();
    words[section] = 1;
    words.extend([0, 4, 4, 4]);
    data.extend(words.iter().flat_map(|w| w.to_le_bytes()));
    let block = b.alloc(0x20 + data.len());
    let field = |at: usize| block + at;
    b.u32(
        field(offset_of!(Raw<CrBlockStream>, data_size)),
        data.len() as u32,
    );
    b.u32(
        field(offset_of!(Raw<CrBlockStream>, constant_size)),
        4 * constants.len() as u32,
    );
    b.u32(
        field(offset_of!(Raw<CrBlockStream>, channel_offset)),
        data.len() as u32 + 0x20,
    );
    b.put(
        field(offset_of!(Raw<CrBlockStream>, slop_size)),
        &2u16.to_le_bytes(),
    );
    b.put(
        field(offset_of!(Raw<CrBlockStream>, num_frames)),
        &FRAMES.to_le_bytes(),
    );
    b.0[field(offset_of!(Raw<CrBlockStream>, segment_size))] = 11;
    b.put(block + 0x20, &data);
    let blocks = b.alloc(8);
    b.ptr(blocks, block);
    b.array(animation + 0x50, blocks, 1);
    b.u32(animation + 0x48, data.len() as u32 + 0x20);

    let entry = b.alloc(0x20);
    b.u32(entry, hash(ANIMATION));
    b.ptr(entry + 8, animation);
    b.map(set + 0x48, 3, u64::from(hash(ANIMATION)), entry);
    // The key set: every animation key the clips play.
    let key_entry = b.alloc(0x10);
    b.u32(key_entry, hash(ANIMATION));
    b.map(root + 0x80, 11, u64::from(hash(ANIMATION)), key_entry);

    let key = u64::from(hash(DICTIONARY)) << 32 | u64::from(hash("clip_a"));
    let clip = b.alloc(0xA0);
    b.put(clip + 0x10, &1u64.to_le_bytes());
    b.ptr(clip + 0x18, root);
    b.u32(clip + 0x20, 1);
    b.inline_name(clip + 0x28, "clip_a.clip");
    b.ptr(clip + 0x68, animation);
    b.u32(clip + 0x70, hash(ANIMATION));
    b.put(clip + 0x94, &1.0f32.to_le_bytes());
    b.put(clip + 0x98, &1.0f32.to_le_bytes());
    b.put(clip + 0x58, &key.to_le_bytes());
    let entry = b.alloc(0x20);
    b.put(entry, &key.to_le_bytes());
    b.ptr(entry + 8, clip);
    b.map(root + 0x48, 11, key, entry);
    // The metadata region: the clip's properties object, holding none.
    let properties = b.alloc(0x28);
    b.put(properties + 0x20, &1u64.to_le_bytes());
    b.ptr(clip + 0x48, properties);
    b.ptr(root + 0xA0, properties);
    b.u32(root + 0xA8, 0x28);
    // The end region: the block map's record of one virtual block, a word, the set list and a
    // pointer to the set, to the section's end.
    let map = b.alloc(0x18);
    b.0[map + 8] = 1;
    b.ptr(root + 0x08, map);
    b.alloc(16);
    let list = b.alloc(16);
    b.ptr(list, set);
    b.u32(list + 8, hash(&set_name));
    b.u32(list + 12, 1);
    b.array(root + 0x70, list, 1);
    let last = b.alloc(16);
    b.ptr(last, set);
    b.0
}

/// `data` as a zlib stream of stored deflate blocks, under the `78 DA` head a scan looks for.
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
    out.extend(((b << 16) | a).to_be_bytes());
    out
}

/// `section` as a stored, unkeyed RSC8 of version 65 with no physical section.
fn rsc8(section: &[u8]) -> Vec<u8> {
    let mut bytes = b"RSC8".to_vec();
    for word in [0x1F41u32, section.len() as u32 | 4, 1] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend_from_slice(section);
    bytes
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

#[test]
fn a_headerless_and_a_stored_animation_resource_print_what_the_reader_reads() {
    let section = clip_dictionary();
    let mut other_root = section.clone();
    other_root[0x6C] = 1;
    let bodies = [zlib(&section), rsc8(&section), zlib(&other_root)];
    let mut bytes = rpf9_head();
    let mut offsets = Vec::new();
    for body in &bodies {
        // A headerless resource follows a zero 16-byte slot at the start of a 4 KiB page.
        bytes.resize(bytes.len().next_multiple_of(0x1000) + 16, 0);
        offsets.push(bytes.len());
        bytes.extend_from_slice(body);
    }
    let dir = tempfile::tempdir().expect("a folder");
    let archive = dir.path().join("anim.rpf");
    std::fs::write(&archive, &bytes).expect("written");
    let archive = archive.display().to_string();
    let open = |i: usize| run(&["rpf", "scan-open", &archive, &format!("{:#X}", offsets[i])]);

    for (i, kind) in [(0, "zlib clip root"), (1, "stored rsc8")] {
        let (printed, ok) = open(i);
        assert!(ok, "{printed}");
        assert!(printed.contains(kind), "{printed}");
        assert!(
            printed.contains(&format!(
                "root          clip dictionary \"{DICTIONARY}\", key {:#010X}",
                hash(DICTIONARY)
            )),
            "{printed}"
        );
        assert!(printed.contains("1 animation(s)"), "{printed}");
        let rate = f32::from(FRAMES - 1);
        assert!(
            printed.contains(&format!("{FRAMES} frames, 1 s, {rate} fps")),
            "{printed}"
        );
        assert!(
            printed.contains(&format!(
                "dof 0  id hash_{:08X}, track genericControl, type 4",
                hash("SKEL_ROOT")
            )),
            "{printed}"
        );
        assert!(
            printed.contains("channels per directory section [0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0]"),
            "{printed}"
        );
        assert!(printed.contains("clips         1"), "{printed}");
        assert!(
            printed.contains("channel encoder is equal to the section as read"),
            "{printed}"
        );
        assert!(
            printed.contains(&format!(
                "\"clip_a.clip\"  set 0 animation 0 (key {:#010X}), 0 to 1 s at rate 1",
                hash(ANIMATION)
            )),
            "{printed}"
        );
    }

    let (printed, ok) = open(2);
    assert!(!ok, "{printed}");
    assert!(!printed.contains("clips"), "{printed}");
}

/// A drawable whose skeleton is `SKEL_ROOT` at (0, 0, 1) and `SKEL_Spine0` under it at (0.5, 0, 0),
/// with its block map, as a headerless resource's virtual section.
fn drawable_with_a_skeleton() -> Vec<u8> {
    let mut b = Bytes::default();
    let root = b.alloc(0x100);
    let map = b.alloc(0x18);
    b.0[map + 8] = 1;
    b.ptr(root + 0x08, map);
    let skeleton = b.alloc(0x70);
    b.ptr(root + 0x18, skeleton);
    let bones = [
        ("SKEL_ROOT", -1i16, [0.0f32, 0.0, 1.0]),
        ("SKEL_Spine0", 0, [0.5, 0.0, 0.0]),
    ];
    let records = b.alloc(0x50 * bones.len());
    let inverses = b.alloc(0x40 * bones.len());
    b.ptr(skeleton + 0x28, records);
    b.ptr(skeleton + 0x30, inverses);
    b.put(skeleton + 0x6C, &(bones.len() as u16).to_le_bytes());
    b.put(skeleton + 0x6E, &(bones.len() as u16).to_le_bytes());
    for (i, (name, parent, translation)) in bones.into_iter().enumerate() {
        let at = records + 0x50 * i;
        b.put(at + 12, &1.0f32.to_le_bytes());
        for (k, x) in translation.into_iter().chain([1.0; 3]).enumerate() {
            let lane = if k < 3 {
                0x10 + 4 * k
            } else {
                0x20 + 4 * (k - 3)
            };
            b.put(at + lane, &x.to_le_bytes());
        }
        b.u32(at + 0x1C, hash(name));
        let text = b.alloc(name.len() + 1);
        b.put(text, name.as_bytes());
        b.ptr(at + 0x30, text);
        b.put(at + 0x44, &0xFFFFu16.to_le_bytes());
        b.put(at + 0x46, &parent.to_le_bytes());
        b.put(at + 0x48, &(i as u16).to_le_bytes());
    }
    b.0
}

/// An archive of `bodies`, each as a headerless resource would be laid out, and where each starts.
fn archive_of(bodies: &[Vec<u8>]) -> (tempfile::TempDir, String, Vec<String>) {
    let mut bytes = rpf9_head();
    let mut offsets = Vec::new();
    for body in bodies {
        bytes.resize(bytes.len().next_multiple_of(0x1000) + 16, 0);
        offsets.push(format!("{:#X}", bytes.len()));
        bytes.extend_from_slice(body);
    }
    let dir = tempfile::tempdir().expect("a folder");
    let archive = dir.path().join("anim.rpf");
    std::fs::write(&archive, &bytes).expect("written");
    let archive = archive.display().to_string();
    (dir, archive, offsets)
}

#[test]
fn scan_anim_reports_an_animation_resource_as_json() {
    let (_dir, archive, at) = archive_of(&[zlib(&clip_dictionary()), rsc8(&clip_dictionary())]);
    for offset in &at {
        let (printed, ok) = run(&["rpf", "scan-anim", &archive, offset, "--json"]);
        assert!(ok, "{printed}");
        let report: serde_json::Value = serde_json::from_str(printed.trim()).expect("JSON");
        assert_eq!(report["root"], "clip dictionary");
        assert_eq!(report["name"], DICTIONARY);
        assert_eq!(report["key"], hash(DICTIONARY));
        let animation = &report["sets"][0]["animations"][0];
        assert_eq!(animation["frames"], FRAMES);
        assert_eq!(animation["flags"], 0x140);
        assert_eq!(animation["dofs"][0]["track"], "genericControl");
        assert_eq!(animation["dofs"][0]["id"], hash("SKEL_ROOT"));
        let play = &report["clips"][0]["play"];
        assert_eq!(play["type"], 1);
        assert_eq!(play["key"], hash(ANIMATION));
        assert_eq!(play["end"], 1.0);
    }
    let (printed, ok) = run(&["rpf", "scan-anim", &archive, &at[0]]);
    assert!(!ok, "{printed}");
}

#[test]
fn scan_anim_binds_an_animation_to_a_scanned_skeleton_and_poses_it() {
    let moved = [1.0f32, 2.0, 3.0];
    let anim = dictionary_of(hash("SKEL_Spine0"), "boneTranslation", 2, 1, &moved);
    let (_dir, archive, at) = archive_of(&[zlib(&anim), zlib(&drawable_with_a_skeleton())]);
    let (printed, ok) = run(&[
        "rpf",
        "scan-anim",
        &archive,
        &at[0],
        "--on",
        &archive,
        &at[1],
        "--at",
        "0.5",
    ]);
    assert!(ok, "{printed}");
    assert!(printed.contains("Skeleton:       2 bones"), "{printed}");
    assert!(printed.contains("Clip:           clip_a.clip"), "{printed}");
    assert!(
        printed.contains("1 of 1 DOF(s) move 1 bone(s); 0 name no bone"),
        "{printed}"
    );
    // The spine's local translation is the DOF's value, under the root at (0, 0, 1).
    let [x, y, z] = [moved[0], moved[1], moved[2] + 1.0];
    let line = format!("SKEL_Spine0              ({x:.4}, {y:.4}, {z:.4})");
    assert!(printed.contains(&line), "{line} in {printed}");
    assert!(!printed.contains("SKEL_ROOT    "), "{printed}");

    let other = dictionary_of(hash("SKEL_Head"), "boneTranslation", 2, 1, &moved);
    let (_dir, archive, at) = archive_of(&[zlib(&other), zlib(&drawable_with_a_skeleton())]);
    let (printed, ok) = run(&[
        "rpf",
        "scan-anim",
        &archive,
        &at[0],
        "--on",
        &archive,
        &at[1],
    ]);
    assert!(ok, "{printed}");
    assert!(
        printed.contains("0 of 1 DOF(s) move 0 bone(s); 1 name no bone"),
        "{printed}"
    );
}
