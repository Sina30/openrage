//! The masked GTA VI key snapshot unmasks, under the executable that keyed it, to a snapshot the
//! RPF9 reader parses; a non-matching executable is refused.
//!
//! Install-gated and game-derived, so it skips cleanly without the two env vars, which is why a
//! clean checkout passes:
//!   `OPENRAGE_GTA6_EXE`          the game executable the snapshot was keyed on
//!   `OPENRAGE_GTA6_KEYS_MASKED`  the masked snapshot produced beside the plaintext one

#![cfg(feature = "gta6")]

use std::path::PathBuf;

use or_io::container::rpf::{decode_masked, is_masked, GtaViKeys};
use or_test_support::skip_install;

const EXE_ENV: &str = "OPENRAGE_GTA6_EXE";
const MASKED_ENV: &str = "OPENRAGE_GTA6_KEYS_MASKED";

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

#[test]
fn the_masked_snapshot_unmasks_with_the_executable_and_is_inert_without_it() {
    let what = "GTA VI masked key snapshot unmasking";
    let (Some(exe_path), Some(masked_path)) = (env_path(EXE_ENV), env_path(MASKED_ENV)) else {
        skip_install(
            what,
            format_args!("set {EXE_ENV} (the executable) and {MASKED_ENV} (the masked snapshot)"),
        );
        return;
    };

    let exe = std::fs::read(&exe_path).expect("read the executable");
    let masked = std::fs::read(&masked_path).expect("read the masked snapshot");
    assert!(is_masked(&masked), "the file is not a masked container");

    let plain = decode_masked(&masked, &exe).expect("the executable unmasks its snapshot");
    GtaViKeys::parse(&plain).expect("the unmasked snapshot parses as an ORGTAVIK snapshot");

    // Inert without the right executable: a non-matching one is refused, not quietly wrong.
    assert!(
        decode_masked(&masked, b"not the game executable").is_err(),
        "the masked snapshot unmasked without the executable"
    );
}
