//! An archive a user opens must never take the process down with it.
//!
//! Each case is an `#[ignore]`d test re-run by name in a child process, because
//! both failures pinned here are **aborts**, not panics: a `Vec` allocation the
//! allocator refuses goes to the allocation-error handler, and a stack overflow
//! aborts at the guard page. `catch_unwind` sees neither, and an in-process
//! assertion would take the whole test binary down.
//!
//! ⚠ The assertion is on the child's **exit status**, never on `is_ok()` of a
//! parse: a refusal and a crash are both "not a successful parse".

use std::process::{Command, Output};

use or_io::container::rpf::RpfArchive;

const RPF7_MAGIC: u32 = 0x5250_4637;
const OPEN_MAGIC: u32 = 0x4E45_504F;
const DIR_MARKER: u64 = (1 << 23) - 1;

fn header(entry_count: u32, name_heap_size: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&RPF7_MAGIC.to_le_bytes());
    out.extend_from_slice(&entry_count.to_le_bytes());
    out.extend_from_slice(&name_heap_size.to_le_bytes());
    out.extend_from_slice(&OPEN_MAGIC.to_le_bytes());
    out
}

fn directory_entry(first_entry_index: u32, entry_count: u32) -> Vec<u8> {
    let packed: u64 = DIR_MARKER << 40;
    let mut out = Vec::new();
    out.extend_from_slice(&packed.to_le_bytes());
    out.extend_from_slice(&first_entry_index.to_le_bytes());
    out.extend_from_slice(&entry_count.to_le_bytes());
    out
}

fn archive_claiming_four_billion_entries() -> Vec<u8> {
    header(u32::MAX, 0x10)
}

fn archive_whose_directory_contains_itself() -> Vec<u8> {
    let mut out = header(2, 0x10);
    out.extend_from_slice(&directory_entry(1, 1));
    out.extend_from_slice(&directory_entry(1, 1));
    out.extend_from_slice(&[0u8; 0x10]);
    out
}

fn archive_nested_this_deep(depth: u32) -> Vec<u8> {
    let entry_count = depth + 1;
    let mut out = header(entry_count, 0x10);
    for i in 0..depth {
        out.extend_from_slice(&directory_entry(i + 1, 1));
    }
    out.extend_from_slice(&directory_entry(0, 0));
    out.extend_from_slice(&[0u8; 0x10]);
    out
}

#[test]
#[ignore = "run in a subprocess by the case that owns it"]
fn child_opens_an_archive_claiming_four_billion_entries() {
    let bytes = archive_claiming_four_billion_entries();
    eprintln!("child: {} byte archive, entry_count=u32::MAX", bytes.len());
    let outcome = RpfArchive::from_bytes(bytes);
    report(outcome.map(|a| a.entries.len()));
}

#[test]
#[ignore = "run in a subprocess by the case that owns it"]
fn child_opens_an_archive_whose_directory_contains_itself() {
    let bytes = archive_whose_directory_contains_itself();
    eprintln!(
        "child: {} byte archive, entry 1 is its own child",
        bytes.len()
    );
    let outcome = RpfArchive::from_bytes(bytes);
    report(outcome.map(|a| a.entries.len()));
}

fn archive_with_a_multibyte_name_across_the_cap() -> Vec<u8> {
    let mut name: Vec<u8> = vec![b'a'; 255];
    name.extend_from_slice("€".as_bytes());
    name.push(0);
    while !name.len().is_multiple_of(16) {
        name.push(0);
    }

    let mut out = header(2, name.len() as u32);
    out.extend_from_slice(&directory_entry(1, 1));
    let packed: u64 = 0;
    out.extend_from_slice(&packed.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&name);
    out
}

/// 100,000 nested directories, in a 1.6 MB archive. Acyclic, so a visited set
/// does not see it.
const NESTING_DEPTH: u32 = 100_000;

#[test]
#[ignore = "run in a subprocess by the case that owns it"]
fn child_opens_a_deeply_nested_archive() {
    let bytes = archive_nested_this_deep(NESTING_DEPTH);
    eprintln!(
        "child: {} byte archive, {NESTING_DEPTH} directories nested one inside the next",
        bytes.len()
    );
    let outcome = RpfArchive::from_bytes(bytes);
    report(outcome.map(|a| a.entries.len()));
}

const BOMB_INFLATED_BYTES: usize = 256 * 1024 * 1024;

#[test]
#[ignore = "run in a subprocess by the case that owns it"]
fn child_extracts_a_body_that_inflates_past_its_declared_size() {
    use or_io::container::resource::dat_decompress_bounded;

    let bomb = or_io::container::resource::dat_compress_best(&vec![0u8; BOMB_INFLATED_BYTES])
        .expect("compress the bomb");
    eprintln!(
        "child: {} compressed bytes inflate to {} ({}x), entry declares 1024",
        bomb.len(),
        BOMB_INFLATED_BYTES,
        BOMB_INFLATED_BYTES / bomb.len().max(1)
    );
    match dat_decompress_bounded(&bomb, 1024) {
        Ok(v) => eprintln!("child: parsed, {} entries", v.len()),
        Err(e) => eprintln!("child: refused -> {e}"),
    }
}

#[test]
#[ignore = "run in a subprocess by the case that owns it"]
fn child_opens_an_archive_with_a_multibyte_name_across_the_cap() {
    let bytes = archive_with_a_multibyte_name_across_the_cap();
    eprintln!(
        "child: {} byte archive, entry name is 255 ASCII + a 3-byte char at the 256-byte cap",
        bytes.len()
    );
    let outcome = RpfArchive::from_bytes(bytes);
    report(outcome.map(|a| a.entries.len()));
}

fn report(outcome: Result<usize, or_io::error::IoError>) {
    match outcome {
        Ok(n) => eprintln!("child: parsed, {n} entries"),
        Err(e) => eprintln!("child: refused -> {e}"),
    }
}

fn run_child(test_name: &str) -> Output {
    let exe = std::env::current_exe().expect("locate this test binary");
    Command::new(exe)
        .args([
            test_name,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .output()
        .expect("spawn child")
}

fn assert_survived(test_name: &str, what: &str) {
    let out = run_child(test_name);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    for line in stderr.lines().filter(|l| l.starts_with("child: ")) {
        println!("{line}");
    }

    assert!(
        out.status.success(),
        "opening {what} took the process down: {:?}\n\
         A malformed archive must produce an IoError, not a dead editor.\n\
         --- child stdout ---\n{stdout}\n--- child stderr ---\n{stderr}",
        out.status,
    );
}

#[test]
fn a_header_claiming_four_billion_entries_is_refused_not_allocated() {
    assert_survived(
        "child_opens_an_archive_claiming_four_billion_entries",
        "a 16-byte archive claiming u32::MAX entries",
    );
}

#[test]
fn a_directory_that_contains_itself_is_refused_not_recursed() {
    assert_survived(
        "child_opens_an_archive_whose_directory_contains_itself",
        "a 64-byte archive whose directory contains itself",
    );
}

#[test]
fn a_deeply_nested_archive_is_walked_without_running_out_of_stack() {
    assert_survived(
        "child_opens_a_deeply_nested_archive",
        "a 1.6 MB archive nested 100,000 directories deep",
    );
}

#[test]
fn a_name_whose_character_straddles_the_length_cap_is_shortened_not_split() {
    assert_survived(
        "child_opens_an_archive_with_a_multibyte_name_across_the_cap",
        "an archive whose entry name has a multi-byte character across the 256-byte cap",
    );
}

#[test]
fn a_body_that_inflates_past_its_declared_size_is_refused_while_it_is_still_small() {
    assert_survived(
        "child_extracts_a_body_that_inflates_past_its_declared_size",
        "a body declaring 1 KiB whose stream inflates to 256 MiB",
    );
}
