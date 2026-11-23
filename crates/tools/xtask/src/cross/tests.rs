use super::*;

#[test]
fn each_os_has_its_rust_target_and_the_c_abi_zig_compiles_for() {
    assert_eq!(
        Os::parse("windows").unwrap().rust_target(),
        "x86_64-pc-windows-msvc"
    );
    assert_eq!(
        Os::parse("windows").unwrap().zig_target(),
        "x86_64-windows-gnu"
    );
    assert_eq!(
        Os::parse("linux").unwrap().rust_target(),
        "x86_64-unknown-linux-gnu"
    );
    assert_eq!(Os::parse("linux").unwrap().zig_target(), "x86_64-linux-gnu");
    assert!(
        Os::parse("macos").is_err(),
        "a Mac builds its own, and is not cross-checked"
    );
}

#[test]
fn the_compiler_variable_cc_reads_spells_the_target_with_underscores() {
    assert_eq!(
        compiler_variable("CC", "x86_64-pc-windows-msvc"),
        "CC_x86_64_pc_windows_msvc"
    );
    assert_eq!(
        compiler_variable("AR", "x86_64-unknown-linux-gnu"),
        "AR_x86_64_unknown_linux_gnu"
    );
}

#[test]
fn zig_is_the_one_named_else_the_environments_else_the_path_else_the_python_package() {
    let pick = pick_zig;
    assert_eq!(
        pick(Some("a".into()), Some("b".into()), true, true).unwrap(),
        "a"
    );
    assert_eq!(pick(None, Some("b".into()), true, true).unwrap(), "b");
    assert_eq!(pick(None, None, true, true).unwrap(), "zig");
    assert_eq!(pick(None, None, false, true).unwrap(), "python3 -m ziglang");
    // An empty name is no name
    assert_eq!(pick(Some("  ".into()), None, true, false).unwrap(), "zig");
    let none = pick(None, None, false, false).unwrap_err().to_string();
    assert!(none.contains("pip install ziglang"), "{none}");
}

#[test]
fn the_scripts_name_the_compiler_the_mode_and_the_target() {
    let cc = compiler_script("zig", "cc", "x86_64-linux-gnu");
    assert!(cc.starts_with("#!/bin/sh\n"));
    assert!(cc.contains("exec zig cc -target x86_64-linux-gnu \"$@\""));
    assert!(cc.contains("--target=*"));
    let cxx = compiler_script("python3 -m ziglang", "c++", "x86_64-windows-gnu");
    assert!(cxx.contains("exec python3 -m ziglang c++ -target x86_64-windows-gnu"));
    assert!(archiver_script("zig", Os::Linux).contains("exec zig ar \"$@\""));
    let windows = archiver_script("zig", Os::Windows);
    assert!(windows.contains("-out:*") && windows.contains("zig ar crs"));
}

#[test]
fn the_scripts_a_check_needs_are_written_and_pkg_config_only_for_linux() {
    let dir = tempfile::tempdir().unwrap();
    let windows = dir.path().join("windows");
    write_scripts(&windows, "zig", Os::Windows).unwrap();
    for name in ["cc", "c++", "ar"] {
        assert!(windows.join(name).is_file(), "{name}");
    }
    assert!(!windows.join("pkg-config").exists());
    let linux = dir.path().join("linux");
    write_scripts(&linux, "zig", Os::Linux).unwrap();
    assert!(linux.join("bin/pkg-config").is_file());
    // The compilers are not beside it, so a path that holds it does not hand the host's links to Zig
    assert!(!linux.join("bin/cc").exists());
}

/// The scripts run as `cc` runs them, with a stand-in for Zig that writes the arguments it was given.
#[cfg(unix)]
mod run_as_cc_runs_them {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::*;

    fn fake_zig(dir: &Path) -> (PathBuf, PathBuf) {
        let seen = dir.join("seen");
        let zig = dir.join("zig");
        std::fs::write(
            &zig,
            format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", seen.display()),
        )
        .unwrap();
        std::fs::set_permissions(&zig, std::fs::Permissions::from_mode(0o755)).unwrap();
        (zig, seen)
    }

    fn arguments(file: &Path) -> Vec<String> {
        std::fs::read_to_string(file)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn the_compiler_drops_the_clang_target_flag_and_keeps_every_other_argument_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let (zig, seen) = fake_zig(dir.path());
        write_scripts(&dir.path().join("w"), zig.to_str().unwrap(), Os::Linux).unwrap();
        let status = Command::new(dir.path().join("w/cc"))
            .args([
                "-O1",
                "--target=x86_64-unknown-linux-gnu",
                "-I",
                "a dir/with space",
                "-DX=\"q\"",
                "-c",
                "x.c",
            ])
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            arguments(&seen),
            [
                "cc",
                "-target",
                "x86_64-linux-gnu",
                "-O1",
                "-I",
                "a dir/with space",
                "-DX=\"q\"",
                "-c",
                "x.c"
            ]
        );
    }

    #[test]
    fn the_windows_archiver_turns_the_libs_spelling_into_ars() {
        let dir = tempfile::tempdir().unwrap();
        let (zig, seen) = fake_zig(dir.path());
        write_scripts(&dir.path().join("w"), zig.to_str().unwrap(), Os::Windows).unwrap();
        let output = dir.path().join("out.a");
        std::fs::write(&output, b"old").unwrap();
        let status = Command::new(dir.path().join("w/ar"))
            .arg(format!("-out:{}", output.display()))
            .args(["-nologo", "a.o", "b b.o"])
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            arguments(&seen),
            ["ar", "crs", output.to_str().unwrap(), "a.o", "b b.o"]
        );
        assert!(
            !output.exists(),
            "the old archive goes first, as lib.exe starts afresh"
        );
    }

    #[test]
    fn the_stand_in_pkg_config_says_every_library_is_there() {
        let dir = tempfile::tempdir().unwrap();
        write_scripts(&dir.path().join("l"), "zig", Os::Linux).unwrap();
        let run = |args: &[&str]| {
            let out = Command::new(dir.path().join("l/bin/pkg-config"))
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success());
            String::from_utf8(out.stdout).unwrap()
        };
        assert_eq!(run(&["alsa", "--modversion"]).trim(), "1.2.9");
        assert_eq!(run(&["--libs", "--cflags", "alsa"]).trim(), "-lasound");
    }
}
