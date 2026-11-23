//! The shared libraries winit and wgpu `dlopen` on Linux, probed before the
//! event loop: a missing one otherwise panics inside winit with no hint.

/// A library's soname and the packages that ship it.
struct Library {
    soname: &'static str,
    debian: &'static str,
    fedora: &'static str,
}

const fn library(soname: &'static str, debian: &'static str, fedora: &'static str) -> Library {
    Library {
        soname,
        debian,
        fedora,
    }
}

// Keyboard handling on either backend, and the Vulkan loader: `Backends::PRIMARY`
// is Vulkan alone on Linux.
const ALWAYS: &[Library] = &[
    library("libxkbcommon.so.0", "libxkbcommon0", "libxkbcommon"),
    library("libvulkan.so.1", "libvulkan1", "vulkan-loader"),
];

const WAYLAND: &[Library] = &[library(
    "libwayland-client.so.0",
    "libwayland-client0",
    "libwayland-client",
)];

const X11: &[Library] = &[
    library("libX11.so.6", "libx11-6", "libX11"),
    library("libX11-xcb.so.1", "libx11-xcb1", "libX11-xcb"),
    library("libXcursor.so.1", "libxcursor1", "libXcursor"),
    library("libXi.so.6", "libxi6", "libXi"),
    library("libxcb.so.1", "libxcb1", "libxcb"),
    library(
        "libxkbcommon-x11.so.0",
        "libxkbcommon-x11-0",
        "libxkbcommon-x11",
    ),
];

/// The libraries the session `env` selects that `loads` cannot load, named with their packages.
pub fn missing(
    env: impl Fn(&str) -> Option<String>,
    loads: impl Fn(&str) -> bool,
) -> Option<String> {
    // winit's own choice: Wayland when either variable is set and not empty, X11 otherwise.
    let wayland = ["WAYLAND_DISPLAY", "WAYLAND_SOCKET"]
        .iter()
        .any(|key| env(key).is_some_and(|value| !value.is_empty()));
    let backend = if wayland { WAYLAND } else { X11 };
    let lines: Vec<String> = ALWAYS
        .iter()
        .chain(backend)
        .filter(|lib| !loads(lib.soname))
        .map(|lib| {
            format!(
                "  {}: install {} (Debian, Ubuntu) or {} (Fedora)",
                lib.soname, lib.debian, lib.fedora
            )
        })
        .collect();
    (!lines.is_empty()).then(|| {
        format!(
            "OpenRage cannot start: this system lacks libraries it loads at run time.\n{}",
            lines.join("\n")
        )
    })
}

/// What this process's session lacks, if anything.
#[cfg(target_os = "linux")]
pub fn missing_here() -> Option<String> {
    missing(|key| std::env::var(key).ok(), loads)
}

#[cfg(target_os = "linux")]
fn loads(soname: &str) -> bool {
    use std::ffi::{c_char, c_int, c_void, CString};

    extern "C" {
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
    }
    const RTLD_LAZY: c_int = 1;

    let Ok(name) = CString::new(soname) else {
        return false;
    };
    // SAFETY: `name` is NUL-terminated and outlives the call, and a non-null
    // handle is closed exactly once.
    unsafe {
        let handle = dlopen(name.as_ptr(), RTLD_LAZY);
        if handle.is_null() {
            return false;
        }
        dlclose(handle);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(wayland: bool) -> impl Fn(&str) -> Option<String> {
        move |key| (wayland && key == "WAYLAND_DISPLAY").then(|| "wayland-0".to_owned())
    }

    #[test]
    fn an_x11_session_without_xkbcommon_x11_names_it_and_its_package() {
        let message = missing(session(false), |soname| soname != "libxkbcommon-x11.so.0")
            .expect("a missing library is reported");
        assert!(message.contains("libxkbcommon-x11.so.0"), "{message}");
        assert!(message.contains("libxkbcommon-x11-0"), "{message}");
        assert!(!message.contains("libvulkan"), "{message}");
    }

    #[test]
    fn a_wayland_session_asks_for_wayland_and_not_for_x11() {
        let message = missing(session(true), |_| false).expect("nothing loads");
        assert!(message.contains("libwayland-client.so.0"), "{message}");
        assert!(message.contains("libxkbcommon.so.0"), "{message}");
        assert!(message.contains("libvulkan.so.1"), "{message}");
        assert!(!message.contains("libX11"), "{message}");
        assert_eq!(missing(session(false), |_| true), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_probe_loads_what_is_there_and_not_what_is_not() {
        assert!(loads("libc.so.6"));
        assert!(!loads("libopenrage-no-such-library.so.0"));
    }
}
