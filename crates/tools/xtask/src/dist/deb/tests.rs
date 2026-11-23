use std::process::Command;

use super::*;

fn files() -> Vec<File> {
    vec![
        File {
            path: "usr/bin/openrage".into(),
            mode: 0o755,
            bytes: b"app".to_vec(),
        },
        File {
            path: "usr/share/applications/openrage.desktop".into(),
            mode: 0o644,
            bytes: vec![b'x'; 700],
        },
    ]
}

/// The members of an `ar` archive: name and bytes.
fn ar_members(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    assert_eq!(&bytes[..8], b"!<arch>\n");
    let mut at = 8;
    let mut out = Vec::new();
    while at < bytes.len() {
        let header = &bytes[at..at + 60];
        assert_eq!(&header[58..60], b"`\n");
        let name = String::from_utf8_lossy(&header[..16])
            .trim_end()
            .to_string();
        let size: usize = String::from_utf8_lossy(&header[48..58])
            .trim()
            .parse()
            .unwrap();
        out.push((name, bytes[at + 60..at + 60 + size].to_vec()));
        at += 60 + size + size % 2;
    }
    out
}

fn through(tool: &[&str], bytes: &[u8]) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("in");
    std::fs::write(&path, bytes).unwrap();
    let out = Command::new(tool[0])
        .args(&tool[1..])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn a_package_is_three_members_in_the_order_dpkg_reads_them() {
    let package = deb("0.1.0", "aarch64", &files(), 1_700_000_000).unwrap();
    let members = ar_members(&package);
    let names: Vec<&str> = members.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["debian-binary", "control.tar.gz", "data.tar.gz"]);
    assert_eq!(members[0].1, b"2.0\n");
}

#[test]
fn the_control_file_names_the_package_its_architecture_and_what_it_needs() {
    let text = control("0.1.0", "arm64", 5);
    for line in [
        "Package: openrage",
        "Version: 0.1.0",
        "Architecture: arm64",
        "Installed-Size: 5",
        "Depends: libc6, libasound2t64 | libasound2, libvulkan1",
    ] {
        assert!(text.lines().any(|l| l == line), "{line}\n{text}");
    }
    // A long description continues on lines that start with a space, and the file ends on a newline
    assert!(text.contains("\n Reads, edits"), "{text}");
    assert!(text.ends_with('\n'));
}

#[test]
fn the_architecture_is_the_one_debian_names_and_an_unknown_one_is_refused() {
    assert_eq!(architecture("x86_64").unwrap(), "amd64");
    assert_eq!(architecture("aarch64").unwrap(), "arm64");
    assert!(architecture("riscv64").is_err());
}

#[test]
fn the_installed_size_is_the_files_in_whole_kilobytes() {
    let package = deb("0.1.0", "x86_64", &files(), 0).unwrap();
    let members = ar_members(&package);
    let control = through(&["tar", "-xzOf"], &members[1].1);
    // Three and seven hundred bytes are one kilobyte
    assert!(control.contains("Installed-Size: 1\n"), "{control}");
}

#[test]
fn the_data_lists_its_directories_then_its_files_owned_by_root_with_their_modes() {
    let package = deb("0.1.0", "x86_64", &files(), 1_700_000_000).unwrap();
    let members = ar_members(&package);
    let listing = through(&["tar", "-tvzf"], &members[2].1);
    let rows: Vec<Vec<&str>> = listing
        .lines()
        .map(|l| l.split_whitespace().collect())
        .collect();
    let row = |path: &str| {
        rows.iter()
            .find(|r| r.last() == Some(&path))
            .unwrap_or_else(|| panic!("{path} is not listed:\n{listing}"))
            .clone()
    };
    assert!(
        row("./usr/bin/openrage")[0].starts_with("-rwxr-xr-x"),
        "{listing}"
    );
    assert!(row("./usr/share/applications/openrage.desktop")[0].starts_with("-rw-r--r--"));
    assert!(row("./usr/share/")[0].starts_with("drwxr-xr-x"));
    assert!(row("./usr/")[0].starts_with("drwxr-xr-x"));
    for r in &rows {
        assert!(
            r.join(" ").contains("root/root") || r.join(" ").contains("root root"),
            "{r:?}"
        );
    }
    let usr = rows
        .iter()
        .position(|r| r.last() == Some(&"./usr/"))
        .unwrap();
    let bin = rows
        .iter()
        .position(|r| r.last() == Some(&"./usr/bin/openrage"))
        .unwrap();
    assert!(usr < bin, "a directory comes before what is in it");
}

#[test]
fn a_file_comes_out_of_the_archive_as_it_went_in_across_a_block() {
    let package = deb("0.1.0", "x86_64", &files(), 0).unwrap();
    let members = ar_members(&package);
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("data.tar.gz");
    std::fs::write(&archive, &members[2].1).unwrap();
    let out = Command::new("tar")
        .arg("-xzOf")
        .arg(&archive)
        .arg("./usr/share/applications/openrage.desktop")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, vec![b'x'; 700]);
}

#[test]
fn an_odd_sized_member_is_padded_and_the_next_one_still_starts_where_it_should() {
    let mut archive = b"!<arch>\n".to_vec();
    archive.extend(ar_member("odd", 0, b"abc"));
    archive.extend(ar_member("even", 0, b"abcd"));
    let members = ar_members(&archive);
    assert_eq!(members[0], ("odd".to_string(), b"abc".to_vec()));
    assert_eq!(members[1], ("even".to_string(), b"abcd".to_vec()));
}

#[test]
fn the_same_inputs_are_the_same_bytes() {
    let one = deb("0.1.0", "x86_64", &files(), 1_700_000_000).unwrap();
    let two = deb("0.1.0", "x86_64", &files(), 1_700_000_000).unwrap();
    assert_eq!(one, two);
}

#[test]
fn a_path_too_long_for_the_header_is_refused_and_one_that_splits_is_not() {
    let long = format!("./usr/{}", "f".repeat(101));
    assert!(tar_member(&long, 0o644, b"x", false, 0).is_err());
    let splits = format!("./{}/{}", "d".repeat(120), "f".repeat(60));
    let member = tar_member(&splits, 0o644, b"x", false, 0).unwrap();
    assert_eq!(
        &member[345..345 + 122],
        format!("./{}", "d".repeat(120)).as_bytes()
    );
}

#[test]
#[ignore = "needs dpkg-deb, which a Debian machine has: cargo test -- --ignored"]
fn dpkg_reads_the_package_and_lists_what_is_in_it() {
    let package = deb("0.1.0", "x86_64", &files(), 1_700_000_000).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("openrage.deb");
    std::fs::write(&path, package).unwrap();
    let info = Command::new("dpkg-deb")
        .arg("--info")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        info.status.success(),
        "{}",
        String::from_utf8_lossy(&info.stderr)
    );
    let contents = Command::new("dpkg-deb")
        .arg("--contents")
        .arg(&path)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&contents.stdout);
    assert!(text.contains("./usr/bin/openrage"), "{text}");
}
