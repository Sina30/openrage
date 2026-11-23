//! A Debian package: an `ar` archive of `debian-binary`, `control.tar.gz` and `data.tar.gz`, written here so that making one needs no `dpkg`.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

/// One file of the package, by its path under `/` and without the leading slash.
pub(super) struct File {
    pub path: String,
    pub mode: u32,
    pub bytes: Vec<u8>,
}

/// The architecture Debian names for the one Rust does.
fn architecture(rust: &str) -> Result<&'static str> {
    match rust {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        other => bail!("no Debian architecture is known for {other}"),
    }
}

fn control(version: &str, architecture: &str, installed_kib: u64) -> String {
    format!(
        "Package: openrage\n\
         Version: {version}\n\
         Architecture: {architecture}\n\
         Maintainer: OpenRage <noreply@openrage.invalid>\n\
         Installed-Size: {installed_kib}\n\
         Depends: libc6, libstdc++6, libasound2t64 | libasound2, libvulkan1\n\
         Section: graphics\n\
         Priority: optional\n\
         Description: Asset editor for Rockstar's RAGE engine\n \
         Reads, edits and writes the files GTA V Legacy and Enhanced ship.\n \
         A Vulkan driver is needed to show a model or a particle effect.\n"
    )
}

fn octal(value: u64, width: usize) -> Vec<u8> {
    let mut text = format!("{value:0digits$o}", digits = width - 1).into_bytes();
    text.push(0);
    text
}

/// One member of a ustar archive: a header, then the bytes to a whole block. A path past what the header holds is refused.
fn tar_member(path: &str, mode: u32, bytes: &[u8], directory: bool, mtime: u64) -> Result<Vec<u8>> {
    let mut header = [0u8; 512];
    let (prefix, name) = if path.len() <= 100 {
        ("", path)
    } else {
        let at = path
            .match_indices('/')
            .map(|(i, _)| i)
            .rfind(|&i| i <= 155 && path.len() - i - 1 <= 100)
            .with_context(|| format!("{path} is too long for a tar header"))?;
        (&path[..at], &path[at + 1..])
    };
    let mut put = |at: usize, text: &[u8]| header[at..at + text.len()].copy_from_slice(text);
    put(0, name.as_bytes());
    put(100, &octal(u64::from(mode), 8));
    put(108, &octal(0, 8));
    put(116, &octal(0, 8));
    put(
        124,
        &octal(if directory { 0 } else { bytes.len() as u64 }, 12),
    );
    put(136, &octal(mtime, 12));
    put(148, b"        ");
    put(156, if directory { b"5" } else { b"0" });
    put(257, b"ustar\0");
    put(263, b"00");
    put(265, b"root");
    put(297, b"root");
    put(345, prefix.as_bytes());
    let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
    header[148..154].copy_from_slice(&octal(u64::from(sum), 7)[..6]);
    header[154] = 0;
    header[155] = b' ';
    let mut out = header.to_vec();
    if !directory {
        out.extend_from_slice(bytes);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    Ok(out)
}

/// A ustar archive of `files` under `./`, every directory above them listed first, all owned by root.
fn tar(files: &[File], mtime: u64) -> Result<Vec<u8>> {
    let mut directories: Vec<String> = Vec::new();
    for file in files {
        let mut at = 0;
        while let Some(i) = file.path[at..].find('/') {
            at += i + 1;
            let directory = format!("./{}", &file.path[..at]);
            if !directories.contains(&directory) {
                directories.push(directory);
            }
        }
    }
    let mut out = tar_member("./", 0o755, &[], true, mtime)?;
    directories.sort();
    for directory in &directories {
        out.extend(tar_member(directory, 0o755, &[], true, mtime)?);
    }
    for file in files {
        out.extend(tar_member(
            &format!("./{}", file.path),
            file.mode,
            &file.bytes,
            false,
            mtime,
        )?);
    }
    out.resize(out.len() + 1024, 0);
    Ok(out)
}

/// `bytes` through `gzip` without the file name and time in its header, so the same input is the same output.
fn gzip(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut child = Command::new("gzip")
        .args(["-n", "-9", "-c"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("could not run gzip")?;
    let mut stdin = child.stdin.take().context("gzip has no input")?;
    // Written from a thread, since gzip answers while it is still being given its input
    let input = bytes.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output()?;
    writer
        .join()
        .map_err(|_| anyhow::anyhow!("writing to gzip panicked"))??;
    if !output.status.success() {
        bail!("gzip failed");
    }
    Ok(output.stdout)
}

fn ar_member(name: &str, mtime: u64, bytes: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "{name:<16}{mtime:<12}{:<6}{:<6}{:<8}{:<10}`\n",
        0,
        0,
        "100644",
        bytes.len()
    )
    .into_bytes();
    out.extend_from_slice(bytes);
    if bytes.len() % 2 == 1 {
        out.push(b'\n');
    }
    out
}

/// The `.deb` of `files` for `version` on the architecture Rust calls `rust_arch`, stamped `mtime` seconds after 1970.
pub(super) fn deb(version: &str, rust_arch: &str, files: &[File], mtime: u64) -> Result<Vec<u8>> {
    let architecture = architecture(rust_arch)?;
    let installed_kib = files
        .iter()
        .map(|f| f.bytes.len() as u64)
        .sum::<u64>()
        .div_ceil(1024);
    let control = File {
        path: "control".into(),
        mode: 0o644,
        bytes: control(version, architecture, installed_kib).into_bytes(),
    };
    let mut out = b"!<arch>\n".to_vec();
    out.extend(ar_member("debian-binary", mtime, b"2.0\n"));
    out.extend(ar_member(
        "control.tar.gz",
        mtime,
        &gzip(&tar(std::slice::from_ref(&control), mtime)?)?,
    ));
    out.extend(ar_member("data.tar.gz", mtime, &gzip(&tar(files, mtime)?)?));
    Ok(out)
}

#[cfg(test)]
mod tests;
