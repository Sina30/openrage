//! `cargo xtask cross-check`: the workspace type-checked and linted for another OS's target, with Zig compiling the C and C++ that its dependencies bring, so a Mac or a Linux machine sees what a Windows or Linux build would refuse.
//!
//! It compiles and does not link, so what it says is that the code builds there, not that it runs there.

use std::path::Path;
use std::process::{Command, ExitCode};

use anyhow::{bail, Context, Result};

use crate::tiers::target_directory;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Os {
    Windows,
    Linux,
}

impl Os {
    fn parse(name: &str) -> Result<Self> {
        match name {
            "windows" => Ok(Os::Windows),
            "linux" => Ok(Os::Linux),
            other => bail!("cross-check takes windows or linux, not `{other}`"),
        }
    }

    /// The Rust target the check is made for.
    fn rust_target(self) -> &'static str {
        match self {
            Os::Windows => "x86_64-pc-windows-msvc",
            Os::Linux => "x86_64-unknown-linux-gnu",
        }
    }

    /// Zig's name for the C ABI it compiles the dependencies for: mingw's headers stand in for the Windows SDK's.
    fn zig_target(self) -> &'static str {
        match self {
            Os::Windows => "x86_64-windows-gnu",
            Os::Linux => "x86_64-linux-gnu",
        }
    }
}

/// The name of an environment variable `cc` reads for `target`, such as `CC_x86_64_pc_windows_msvc`.
fn compiler_variable(kind: &str, target: &str) -> String {
    format!("{kind}_{}", target.replace('-', "_"))
}

/// The Zig to compile with: the one named, `ZIG` from the environment, `zig` on the path, or the `ziglang` Python package.
fn pick_zig(
    named: Option<String>,
    from_env: Option<String>,
    zig_on_path: bool,
    python_has_zig: bool,
) -> Result<String> {
    if let Some(zig) = named.or(from_env).filter(|z| !z.trim().is_empty()) {
        return Ok(zig);
    }
    if zig_on_path {
        return Ok("zig".into());
    }
    if python_has_zig {
        return Ok("python3 -m ziglang".into());
    }
    bail!(
        "no Zig to compile the C dependencies with: put `zig` on the path, or `pip install ziglang` into a virtual environment and name it with --zig \"/path/to/venv/bin/python -m ziglang\", or set ZIG"
    )
}

fn runs(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// A wrapper `cc` can call as a compiler: it drops the clang-style `--target=` that `cc` adds, which Zig refuses beside its own `-target`.
fn compiler_script(zig: &str, mode: &str, zig_target: &str) -> String {
    format!(
        "#!/bin/sh\n\
         for a in \"$@\"; do\n\
         \x20 shift\n\
         \x20 case \"$a\" in --target=*) ;; *) set -- \"$@\" \"$a\" ;; esac\n\
         done\n\
         exec {zig} {mode} -target {zig_target} \"$@\"\n"
    )
}

/// A wrapper `cc` can call as an archiver. For Windows, `cc` asks for `lib.exe`'s spelling, `-out:FILE -nologo OBJECTS`, which becomes `ar crs FILE OBJECTS`.
fn archiver_script(zig: &str, os: Os) -> String {
    match os {
        Os::Linux => format!("#!/bin/sh\nexec {zig} ar \"$@\"\n"),
        Os::Windows => format!(
            "#!/bin/sh\n\
             out=\"\"\n\
             for a in \"$@\"; do\n\
             \x20 shift\n\
             \x20 case \"$a\" in -out:*) out=\"${{a#-out:}}\" ;; -nologo) ;; *) set -- \"$@\" \"$a\" ;; esac\n\
             done\n\
             rm -f \"$out\"\n\
             exec {zig} ar crs \"$out\" \"$@\"\n"
        ),
    }
}

/// A `pkg-config` that says every library is there, for a type-check that needs `alsa-sys`'s build script satisfied and links nothing.
const PKG_CONFIG_SCRIPT: &str = "#!/bin/sh\n\
    case \"$*\" in\n\
    \x20 *--modversion*) echo 1.2.9 ;;\n\
    \x20 *--libs*) echo \"-lasound\" ;;\n\
    \x20 *) echo \"\" ;;\n\
    esac\n\
    exit 0\n";

fn write_executable(path: &Path, text: &str) -> Result<()> {
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// The scripts a check of `os` needs, written into `dir`.
fn write_scripts(dir: &Path, zig: &str, os: Os) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    write_executable(
        &dir.join("cc"),
        &compiler_script(zig, "cc", os.zig_target()),
    )?;
    write_executable(
        &dir.join("c++"),
        &compiler_script(zig, "c++", os.zig_target()),
    )?;
    write_executable(&dir.join("ar"), &archiver_script(zig, os))?;
    if os == Os::Linux {
        // Alone in a folder of its own, since a `cc` on the path would be the host's linker too
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin)?;
        write_executable(&bin.join("pkg-config"), PKG_CONFIG_SCRIPT)?;
    }
    Ok(())
}

pub fn run(args: &[String]) -> Result<ExitCode> {
    let mut os: Option<Os> = None;
    let mut zig: Option<String> = None;
    let mut check_only = false;
    let mut extra: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--zig" => zig = Some(it.next().context("--zig needs a command")?.clone()),
            "--check" => check_only = true,
            "--" => {
                extra.extend(it.by_ref().cloned());
            }
            other if os.is_none() && !other.starts_with('-') => os = Some(Os::parse(other)?),
            other => bail!("cross-check: unknown argument `{other}`"),
        }
    }
    let os = os.context("cross-check takes windows or linux")?;
    if cfg!(windows) {
        bail!("cross-check writes shell wrappers, so it runs on macOS or Linux");
    }
    let zig = pick_zig(
        zig,
        std::env::var("ZIG").ok(),
        runs("zig", &["version"]),
        runs("python3", &["-c", "import ziglang"]),
    )?;
    let root = crate::git::here()?;
    let target = os.rust_target();
    let dir = target_directory()
        .unwrap_or_else(|_| root.join("target"))
        .join("cross-check")
        .join(target);
    write_scripts(&dir, &zig, os)?;

    // ⚠ Not the folder of the compilers: build scripts and proc macros link with the host's `cc` from the path
    let mut path = std::ffi::OsString::from(dir.join("bin"));
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let mut cargo = Command::new("cargo");
    cargo.current_dir(&root).env("PATH", path);
    cargo.env(compiler_variable("CC", target), dir.join("cc"));
    cargo.env(compiler_variable("CXX", target), dir.join("c++"));
    cargo.env(compiler_variable("AR", target), dir.join("ar"));
    if os == Os::Linux {
        cargo.env("PKG_CONFIG_ALLOW_CROSS", "1");
    }
    cargo.arg(if check_only { "check" } else { "clippy" });
    if extra.is_empty() {
        cargo.arg("--workspace");
    } else {
        cargo.args(&extra);
    }
    cargo.args(["--all-targets", "--target", target]);
    if !check_only {
        cargo.args(["--", "-D", "warnings"]);
    }
    println!(
        "{} for {target}, compiling C with `{zig}`",
        if check_only { "check" } else { "clippy" }
    );
    let status = cargo.status().context("could not run cargo")?;
    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests;
