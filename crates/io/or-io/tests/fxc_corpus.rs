//! `.fxc` over a real corpus. There is no writer, so no round trip: the
//! format has no table of contents, and a parse that accounts for every byte
//! is the only evidence it landed on the right fields rather than plausible ones.

use std::path::PathBuf;

use or_io::gta5::shaders::fxc::{FxcArchive, GrcEffect};

fn fixtures_dir() -> PathBuf {
    or_test_support::fixtures_root(env!("CARGO_MANIFEST_DIR"))
}

fn fxc_fixtures() -> Vec<PathBuf> {
    or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "fxc").find()
}

macro_rules! require_fixtures {
    ($name:literal, $fixtures:expr) => {
        if $fixtures.is_empty() {
            or_test_support::skip_corpus(
                $name,
                format_args!("no .fxc fixtures under {:?}", fixtures_dir()),
            );
            return;
        }
    };
}

#[test]
fn every_effect_parses_and_accounts_for_every_byte() {
    let fixtures = fxc_fixtures();
    require_fixtures!("FXC consume", fixtures);

    let mut files = 0usize;
    let mut exact = 0usize;
    let mut trailing = Vec::new();
    let mut failures = Vec::new();

    for path in &fixtures {
        let bytes = std::fs::read(path).unwrap();
        match GrcEffect::parse_counting(&bytes) {
            Ok((_, consumed)) => {
                files += 1;
                if consumed == bytes.len() {
                    exact += 1;
                } else {
                    trailing.push(format!(
                        "{:?}: {} of {} bytes",
                        path.file_name().unwrap_or_default(),
                        consumed,
                        bytes.len()
                    ));
                }
            }
            Err(e) => failures.push(format!("{path:?}: {e}")),
        }
    }

    println!("FXC consume: {files} files, {exact} accounted for exactly");
    for line in trailing.iter().take(10) {
        println!("  short: {line}");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(
        exact, files,
        "a `.fxc` has no table of contents, so a parse that stops short is \
         the only symptom a wrong field width has"
    );
}

/// What the corpus holds.
#[test]
fn what_a_shader_effect_holds() {
    let fixtures = fxc_fixtures();
    require_fixtures!("FXC census", fixtures);

    let mut variables = 0usize;
    let mut programs = 0usize;
    let mut techniques = 0usize;
    let mut cbuffers = 0usize;
    let mut blobs = 0usize;
    let mut blob_bytes = 0usize;
    let mut with_draw_bucket = 0usize;
    let mut unknown_types: std::collections::BTreeMap<u8, usize> = Default::default();

    for path in &fixtures {
        let bytes = std::fs::read(path).unwrap();
        let effect = GrcEffect::parse(&bytes).unwrap();
        // ⚠ The blob count comes from scanning, not from the effect: a
        // program names its bytecode by offset and length into the file, so
        // the effect cannot say how many blobs there are or how big they are.
        let archive = FxcArchive::parse(&bytes).unwrap();

        variables += effect.variables.len();
        programs += effect.programs.len();
        techniques += effect.techniques.len();
        cbuffers += effect.cbuffers.len();
        blobs += archive.len();
        blob_bytes += archive.blobs(&bytes).map(|b| b.size).sum::<usize>();
        if effect.draw_bucket().is_some() {
            with_draw_bucket += 1;
        }
        for var in &effect.variables {
            if let or_io::gta5::shaders::fxc::GrcVariableType::Unknown(code) = var.var_type {
                *unknown_types.entry(code).or_default() += 1;
            }
        }
    }

    println!(
        "FXC census: {} files, {variables} variables, {programs} programs, \
         {techniques} techniques, {cbuffers} constant buffers",
        fixtures.len()
    );
    println!("  DXBC blobs: {blobs} holding {blob_bytes} bytes");
    println!(
        "  effects declaring a draw bucket: {with_draw_bucket}/{}",
        fixtures.len()
    );
    println!("  variable type codes with no name: {unknown_types:?}");

    assert!(variables > 0, "the census reached no variable");
    assert!(programs > 0, "the census reached no program");
    assert!(blobs > 0, "the census reached no DXBC blob");
}

/// Enhanced moved the shader parameter model into the resource and ships no
/// `.fxc`, so one in a Gen9 fixture directory is a Legacy file staged there.
#[test]
fn enhanced_ships_no_shader_effect_archives() {
    let enhanced = or_test_support::Corpus::new(env!("CARGO_MANIFEST_DIR"), "fxc")
        .target(or_test_support::TARGET_GEN9)
        .find();
    assert!(
        enhanced.is_empty(),
        "an Enhanced .fxc was staged, and a retail Enhanced install ships \
         zero: {enhanced:?}"
    );
}
