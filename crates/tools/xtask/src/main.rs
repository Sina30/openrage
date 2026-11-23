//! Repository tasks, run as `cargo xtask <task>`.
//!
//! Each task replaces a PowerShell script, so one implementation runs on
//! Windows, Linux and macOS and the `justfile` stays a list of one-line calls.
//! No workspace crate is a dependency: the pre-commit hook runs this binary,
//! and a commit never waits on the workspace compiling.

mod affected;
mod baselines;
mod cross;
mod dist;
mod encoding;
mod fmt;
mod gate;
mod git;
mod hooks;
mod janitor;
mod lane;
mod provenance;
mod slow_tier;
mod tiers;

use std::process::ExitCode;

use anyhow::{bail, Result};

const USAGE: &str = "\
cargo xtask <task>

  fmt [--check] [--budget N] [--concurrency N] [--manifest-path P] [--list-targets] [-- rustfmt args]
  lint-encoding [--path DIR] [--staged] [--fix-bom]
  lint-provenance [--history REPO [--expressions FILE]]
  install-hooks            copy the pre-commit hook and this binary into the repository's hooks
  pre-commit               what the hook runs
  require-nextest          fail, saying how to install it, when cargo-nextest is missing
  skip-ledger [--keep | --lite | --not-run FILE]
  strict-tests             the workspace suites with a missing corpus failing, then the ledger
  no-pack-tests            the workspace suites as a clean checkout sees them, then the ledger
  gate [--workspace] [--tests-only] [--since REF] [-- parts of test names]
                           clippy and the default test tier over what differs from origin/main (or REF), or all of it
  slow-tier [--budget SECONDS]
                           rewrite the slow tier in .config/nextest.toml from the last `just test`
  baselines-record [SUITE...]
  scaffold-format MODULE EXT ROOT VERSION
  dist <windows|linux|macos> [--out DIR] [--version V] [--bin-dir DIR] [--file-types FILE] [--icon PNG] [--no-sign]
                           the package of the app for this OS, with its checksum
  cross-check <windows|linux> [--zig CMD] [--check] [-- cargo args]
                           the workspace linted for that OS's target, compiling its C with Zig
  lane-new NAME            an agent lane at ../openrage-lanes/NAME on agent/NAME
  janitor [--apply] [--budget-gib N] [--idle-minutes N] [--reference REF] [--lane-root DIR] [--no-github]
                           remove landed lanes and their build output; a report unless --apply
";

/// Forget what `cargo run` told this process about its own package.
///
/// ⚠ `cargo xtask` is `cargo run`, which sets `CARGO_MANIFEST_DIR` and the
/// `CARGO_PKG_*` family here, and a cargo started from a task would inherit
/// them. A build script that tracks one (`ring` tracks several) then reruns
/// every time a build alternates between a recipe and cargo run by hand, and
/// takes `rustls`, `ureq`, `or-cli` and the app with it: 34 to 65 s each way.
fn forget_the_package_cargo_ran() {
    for (name, _) in std::env::vars_os() {
        let text = name.to_string_lossy();
        if text.starts_with("CARGO_PKG_") || text.starts_with("CARGO_MANIFEST_") {
            std::env::remove_var(&name);
        }
    }
}

/// Have Windows end a crashed process started from here without reporting it.
///
/// ⚠ A test that panics cannot unwind under Cranelift, so its process ends as
/// a crash. Windows Error Reporting then keeps the dead process, and with it
/// the test executable, for minutes: the next build of that executable fails
/// to link with `permission denied`. The mode is inherited by every process a
/// task starts, the test binaries and nextest's among them.
#[cfg(windows)]
fn leave_a_crash_unreported() {
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;
    extern "system" {
        fn SetErrorMode(mode: u32) -> u32;
    }
    // SAFETY: it sets a flag of this process and takes no pointer.
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
    }
}

fn main() -> ExitCode {
    forget_the_package_cargo_ran();
    #[cfg(windows)]
    leave_a_crash_unreported();
    let mut args = std::env::args().skip(1);
    let Some(task) = args.next() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    let rest: Vec<String> = args.collect();

    let outcome: Result<ExitCode> = match task.as_str() {
        "fmt" => fmt::run(&rest),
        "lint-encoding" => encoding::run(&rest),
        "lint-provenance" => provenance::run(&rest),
        "install-hooks" => hooks::install(),
        "pre-commit" => hooks::pre_commit(),
        "require-nextest" => tiers::require_nextest(),
        "skip-ledger" => tiers::skip_ledger(&rest),
        "strict-tests" => tiers::strict_tests(),
        "no-pack-tests" => tiers::no_pack_tests(),
        "gate" => gate::run(&rest),
        "slow-tier" => slow_tier::record(&rest),
        "baselines-record" => baselines::record(&rest),
        "scaffold-format" => baselines::scaffold(&rest),
        "janitor" => janitor::run(&rest),
        "dist" => dist::run(&rest),
        "cross-check" => cross::run(&rest),
        "lane-new" => match rest.as_slice() {
            [name] => lane::new(name),
            _ => Err(anyhow::anyhow!("lane-new takes one name")),
        },
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        other => unknown(other),
    };

    match outcome {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn unknown(task: &str) -> Result<ExitCode> {
    bail!("unknown task `{task}`\n\n{USAGE}")
}
