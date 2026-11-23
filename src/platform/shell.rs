//! Showing a file or a folder in the platform's file manager, as Explorer's Show in folder does.

use std::path::Path;

/// Open the folder holding `path` in the file manager, with `path` selected where the platform can.
///
/// ⚠ A test build and a self-check tell [`fake`] instead, so neither opens a window.
pub fn reveal(path: &Path) {
    #[cfg(not(test))]
    if !super::standing_in() {
        return real(path);
    }
    fake::asked("reveal", path);
}

/// Open the folder `path` in the file manager, making it first if it is missing.
pub fn open_folder(path: &Path) {
    #[cfg(not(test))]
    if !super::standing_in() {
        return real_open(path);
    }
    fake::asked("open", path);
}

#[cfg(not(test))]
fn real(path: &Path) {
    #[cfg(windows)]
    let spawned = {
        // ⚠ One argument with no space after the comma: Explorer reads `/select,` and the path as one switch.
        let mut select = std::ffi::OsString::from("/select,");
        select.push(path.as_os_str());
        std::process::Command::new("explorer.exe")
            .arg(select)
            .spawn()
    };
    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let spawned = if select_in_file_manager(path) {
        return;
    } else {
        // No file manager answered on the session bus: open the folder, which cannot select.
        std::process::Command::new("xdg-open")
            .arg(path.parent().unwrap_or(path))
            .spawn()
    };
    if let Err(e) = spawned {
        tracing::warn!(path = %path.display(), error = %e, "the file manager did not start");
    }
}

/// Ask the desktop's file manager, over the session bus, to show `path` selected, as every one that
/// implements `org.freedesktop.FileManager1` does; whether one answered.
#[cfg(all(unix, not(target_os = "macos"), not(test)))]
fn select_in_file_manager(path: &Path) -> bool {
    std::process::Command::new("dbus-send")
        .args(["--session", "--print-reply", "--type=method_call"])
        .args([
            "--dest=org.freedesktop.FileManager1",
            "/org/freedesktop/FileManager1",
        ])
        .arg("org.freedesktop.FileManager1.ShowItems")
        .arg(format!("array:string:{}", file_uri(path)))
        .arg("string:")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// `path` as a `file://` URI, every byte but an unreserved character or a `/` written as `%XX`.
///
/// ⚠ A comma too, because `dbus-send` splits an array argument on commas.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn file_uri(path: &Path) -> String {
    use std::fmt::Write as _;

    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_encoded_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                uri.push(char::from(byte));
            }
            _ => {
                let _ = write!(uri, "%{byte:02X}");
            }
        }
    }
    uri
}

#[cfg(not(test))]
fn real_open(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

/// A stand-in for the file manager: what it was asked to show, kept and logged.
pub mod fake {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    static SHOWN: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    pub(super) fn asked(to: &str, path: &Path) {
        tracing::info!(
            "the file manager's stand-in was asked to {to} {}",
            path.display()
        );
        if let Ok(mut shown) = SHOWN.lock() {
            shown.push(path.to_path_buf());
        }
    }

    /// Whether `path` was revealed or opened since the test binary started.
    #[cfg(test)]
    pub fn was_revealed(path: &Path) -> bool {
        SHOWN.lock().is_ok_and(|r| r.iter().any(|p| p == path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_becomes_a_uri_the_file_manager_reads_back_to_the_same_file() {
        assert_eq!(file_uri(Path::new("/home/a/b.ytd")), "file:///home/a/b.ytd");
        assert_eq!(
            file_uri(Path::new("/home/a b/c,d#e%f é.ytd")),
            "file:///home/a%20b/c%2Cd%23e%25f%20%C3%A9.ytd"
        );
    }
}
