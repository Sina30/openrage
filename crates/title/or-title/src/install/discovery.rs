//! Where Steam, the Rockstar launcher and Epic installed each registered title.

use std::path::{Path, PathBuf};

use crate::game::Game;

/// ⚠ The Rockstar launcher and Epic are found through the registry on Windows, and off it only inside a Wine prefix, where a launcher's default folder and Epic's manifests are read as a Windows drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverySource {
    Steam,
    RockstarLauncher,
    Epic,
    UserConfigured,
}

#[derive(Debug, Clone)]
pub struct DiscoveredInstall {
    pub game: Game,
    pub install_dir: PathBuf,
    pub exe: PathBuf,
    pub source: DiscoverySource,
}

impl DiscoveredInstall {
    /// `game` at `dir`, if one of its executables is there.
    pub fn probe(game: Game, dir: PathBuf, source: DiscoverySource) -> Option<Self> {
        let exe = first_existing_exe(&dir, game.profile().icon_executable_candidates())?;
        Some(Self {
            game,
            install_dir: dir,
            exe,
            source,
        })
    }
}

/// One install per title, the first Steam, the Rockstar launcher or Epic names, in that order.
pub fn discover_installs() -> Vec<DiscoveredInstall> {
    let mut out = Vec::new();
    out.extend(discover_steam());
    out.extend(discover_rockstar_launcher());
    out.extend(discover_epic());
    // Last, so an install the platform's own store names wins over one in a prefix.
    out.extend(discover_prefixes());

    let mut seen = std::collections::HashSet::new();
    out.retain(|i| seen.insert(i.game));
    out
}

fn first_existing_exe(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    names.iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

#[cfg(windows)]
fn discover_steam() -> Vec<DiscoveredInstall> {
    discover_steam_in(read_steam_root())
}

#[cfg(not(windows))]
fn discover_steam() -> Vec<DiscoveredInstall> {
    match std::env::var_os("HOME") {
        Some(home) => discover_steam_in(steam_roots_under(Path::new(&home))),
        None => Vec::new(),
    }
}

/// Where a Steam can sit under a home folder: native, the runtime's `~/.steam` links, Debian's
/// package, Flatpak, the snap and a Mac's. A Proton install sits under the same `steamapps/common`.
#[cfg(any(not(windows), test))]
fn steam_roots_under(home: &Path) -> Vec<PathBuf> {
    [
        ".local/share/Steam",
        ".steam/steam",
        ".steam/root",
        ".steam/debian-installation",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        "snap/steam/common/.local/share/Steam",
        "Library/Application Support/Steam",
    ]
    .iter()
    .map(|root| home.join(root))
    .collect()
}

fn discover_steam_in(steam_roots: impl IntoIterator<Item = PathBuf>) -> Vec<DiscoveredInstall> {
    let mut found = Vec::new();
    for steam_root in steam_roots.into_iter().filter(|r| r.is_dir()) {
        for lib in parse_steam_libraryfolders(&steam_root) {
            found.extend(steam_installs_in_library(&lib));
        }
    }
    found
}

/// The titles `lib`'s `steamapps/common` holds.
fn steam_installs_in_library(lib: &Path) -> Vec<DiscoveredInstall> {
    let common = lib.join("steamapps").join("common");
    if !common.is_dir() {
        return Vec::new();
    }
    Game::all()
        .filter_map(|game| probe_steam_game(game, &common))
        .collect()
}

#[cfg(windows)]
fn read_steam_root() -> Option<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey(r"Software\Valve\Steam") {
        if let Ok(p) = key.get_value::<String, _>("SteamPath") {
            let path = PathBuf::from(p);
            if path.is_dir() {
                return Some(path);
            }
        }
    }
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for sub in [r"SOFTWARE\WOW6432Node\Valve\Steam", r"SOFTWARE\Valve\Steam"] {
        if let Ok(key) = hklm.open_subkey(sub) {
            if let Ok(p) = key.get_value::<String, _>("InstallPath") {
                let path = PathBuf::from(p);
                if path.is_dir() {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn parse_steam_libraryfolders(steam_root: &Path) -> Vec<PathBuf> {
    steam_libraries(steam_root, |raw| Some(PathBuf::from(raw)))
}

/// A Steam root and the libraries its `libraryfolders.vdf` names, each path read through `locate`:
/// as it stands for a native Steam, and mapped onto the prefix's C: drive for a Windows one.
fn steam_libraries(steam_root: &Path, locate: impl Fn(&str) -> Option<PathBuf>) -> Vec<PathBuf> {
    let mut out = vec![steam_root.to_path_buf()];
    let path = steam_root.join("steamapps").join("libraryfolders.vdf");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return out;
    };
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("\"path\"") else {
            continue;
        };
        let Some(open) = rest.find('"') else { continue };
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else {
            continue;
        };
        let raw = &after[..close];
        let unescaped = raw.replace("\\\\", "\\");
        let Some(p) = locate(&unescaped) else {
            continue;
        };
        if p.is_dir() && !out.iter().any(|existing| existing == &p) {
            out.push(p);
        }
    }
    out
}

fn probe_steam_game(game: Game, common: &Path) -> Option<DiscoveredInstall> {
    game.profile()
        .store_hints()
        .steam_folders
        .iter()
        .find_map(|folder| {
            DiscoveredInstall::probe(game, common.join(folder), DiscoverySource::Steam)
        })
}

#[cfg(windows)]
fn discover_rockstar_launcher() -> Vec<DiscoveredInstall> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let mut out = Vec::new();
    for game in Game::all() {
        for sub in game.profile().store_hints().launcher_registry_keys {
            let Ok(key) = hklm.open_subkey(sub) else {
                continue;
            };
            let raw = key
                .get_value::<String, _>("InstallFolder")
                .or_else(|_| key.get_value::<String, _>("InstallLocation"))
                .ok();
            let Some(raw) = raw else { continue };
            if let Some(install) = DiscoveredInstall::probe(
                game,
                PathBuf::from(raw),
                DiscoverySource::RockstarLauncher,
            ) {
                out.push(install);
                break;
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn discover_rockstar_launcher() -> Vec<DiscoveredInstall> {
    Vec::new()
}

#[cfg(windows)]
fn discover_epic() -> Vec<DiscoveredInstall> {
    let Some(programdata) = std::env::var_os("ProgramData") else {
        return Vec::new();
    };
    epic_installs_in(&epic_manifests(Path::new(&programdata)), |raw| {
        Some(PathBuf::from(raw))
    })
}

/// Where Epic's launcher writes one `.item` manifest per installed title, under `ProgramData`.
fn epic_manifests(program_data: &Path) -> PathBuf {
    program_data
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data")
        .join("Manifests")
}

/// The installs Epic's manifests in `manifests` name, each `InstallLocation` read through `locate`.
fn epic_installs_in(
    manifests: &Path,
    locate: impl Fn(&str) -> Option<PathBuf>,
) -> Vec<DiscoveredInstall> {
    let Ok(rd) = std::fs::read_dir(manifests) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("item") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let display = json
            .get("DisplayName")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let game = Game::all().find(|g| {
            g.profile()
                .store_hints()
                .epic_display_names
                .contains(&display)
        });
        let Some(game) = game else { continue };
        let install_loc = json
            .get("InstallLocation")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if install_loc.is_empty() {
            continue;
        }
        let Some(dir) = locate(install_loc) else {
            continue;
        };
        if let Some(install) = DiscoveredInstall::probe(game, dir, DiscoverySource::Epic) {
            out.push(install);
        }
    }
    out
}

#[cfg(not(windows))]
fn discover_epic() -> Vec<DiscoveredInstall> {
    Vec::new()
}

#[cfg(not(windows))]
fn discover_prefixes() -> Vec<DiscoveredInstall> {
    match std::env::var_os("HOME") {
        Some(home) => discover_in_prefixes_under(Path::new(&home)),
        None => Vec::new(),
    }
}

#[cfg(windows)]
fn discover_prefixes() -> Vec<DiscoveredInstall> {
    Vec::new()
}

/// A path as a Windows program wrote it (`C:\Program Files\x`) on the prefix's C: drive, `drive_c`.
/// Another drive is not inside the prefix, so it maps to nothing.
#[cfg(any(not(windows), test))]
fn map_windows_path(drive_c: &Path, windows: &str) -> Option<PathBuf> {
    let mut chars = windows.chars();
    let (drive, colon) = (chars.next()?, chars.next()?);
    if !drive.eq_ignore_ascii_case(&'c') || colon != ':' {
        return None;
    }
    let rest = windows[2..].replace('\\', "/");
    Some(drive_c.join(rest.trim_start_matches('/')))
}

/// The `drive_c` of every Wine prefix under `home`, whatever tool made it: Wine's own, Lutris,
/// Heroic (`pfx` inside each prefix, for Proton), Bottles, Proton's `compatdata` beside any Steam,
/// CrossOver and Whisky.
#[cfg(any(not(windows), test))]
fn prefix_drives_under(home: &Path) -> Vec<PathBuf> {
    let mut drives: Vec<PathBuf> = Vec::new();
    let mut add = |prefix: &Path| {
        for candidate in [prefix.to_path_buf(), prefix.join("pfx")] {
            let drive_c = candidate.join("drive_c");
            if drive_c.is_dir() && !drives.contains(&drive_c) {
                drives.push(drive_c);
            }
        }
    };

    add(&home.join(".wine"));
    let mut collections: Vec<PathBuf> = [
        "Games",
        "Games/Heroic/Prefixes/default",
        ".local/share/bottles/bottles",
        ".var/app/com.usebottles.bottles/data/bottles/bottles",
        "Library/Application Support/CrossOver/Bottles",
        "Library/Containers/com.isaacmarovitz.Whisky/Bottles",
    ]
    .iter()
    .map(|collection| home.join(collection))
    .collect();
    collections.extend(
        steam_roots_under(home)
            .into_iter()
            .map(|root| root.join("steamapps/compatdata")),
    );
    for collection in collections {
        let Ok(children) = std::fs::read_dir(&collection) else {
            continue;
        };
        let mut children: Vec<PathBuf> = children.flatten().map(|c| c.path()).collect();
        children.sort();
        for child in children {
            add(&child);
        }
    }
    drives
}

/// What one prefix holds: the Rockstar launcher's default folders, Epic's manifests and a Windows
/// Steam with its libraries, each read as the Windows paths they are.
#[cfg(any(not(windows), test))]
fn discover_in_prefix(drive_c: &Path) -> Vec<DiscoveredInstall> {
    let mut out = Vec::new();

    for game in Game::all() {
        let folders = game.profile().store_hints().steam_folders;
        let found = ["Program Files", "Program Files (x86)"]
            .iter()
            .flat_map(|files| folders.iter().map(move |folder| (files, folder)))
            .find_map(|(files, folder)| {
                let dir = drive_c.join(files).join("Rockstar Games").join(folder);
                DiscoveredInstall::probe(game, dir, DiscoverySource::RockstarLauncher)
            });
        out.extend(found);
    }

    out.extend(epic_installs_in(
        &epic_manifests(&drive_c.join("ProgramData")),
        |raw| map_windows_path(drive_c, raw),
    ));

    for steam in ["Program Files (x86)/Steam", "Program Files/Steam"] {
        let root = drive_c.join(steam);
        if !root.is_dir() {
            continue;
        }
        for lib in steam_libraries(&root, |raw| map_windows_path(drive_c, raw)) {
            out.extend(steam_installs_in_library(&lib));
        }
    }
    out
}

/// Every install a Wine prefix under `home` holds, whichever tool made the prefix.
#[cfg(any(not(windows), test))]
fn discover_in_prefixes_under(home: &Path) -> Vec<DiscoveredInstall> {
    prefix_drives_under(home)
        .iter()
        .flat_map(|drive_c| discover_in_prefix(drive_c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_linux_steam_root_and_its_libraries_are_found_under_home() {
        let home = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();

        let native = home.path().join(".local/share/Steam");
        std::fs::create_dir_all(native.join("steamapps")).unwrap();
        let escaped = elsewhere.path().display().to_string().replace('\\', "\\\\");
        std::fs::write(
            native.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{escaped}\"\n\t}}\n}}\n"
            ),
        )
        .unwrap();
        let legacy = elsewhere.path().join("steamapps/common/Grand Theft Auto V");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("GTA5.exe"), b"").unwrap();

        let flatpak = home
            .path()
            .join(".var/app/com.valvesoftware.Steam/.local/share/Steam");
        let enhanced = flatpak.join("steamapps/common/Grand Theft Auto V Enhanced");
        std::fs::create_dir_all(&enhanced).unwrap();
        std::fs::write(enhanced.join("GTA5_Enhanced.exe"), b"").unwrap();

        let found = discover_steam_in(steam_roots_under(home.path()));
        let at = |game| {
            found
                .iter()
                .find(|i| i.game == game && i.source == DiscoverySource::Steam)
                .map(|i| i.install_dir.clone())
        };
        assert_eq!(at(Game::GtaVLegacy), Some(legacy));
        assert_eq!(at(Game::GtaVEnhanced), Some(enhanced));
    }

    /// A file at `path`, with its folders.
    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("a parent")).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    /// The first folder and executable the game's own profile names, never typed by hand.
    fn exe_of(game: Game) -> &'static str {
        game.profile().icon_executable_candidates()[0]
    }

    fn rockstar_folder(game: Game) -> &'static str {
        game.profile().store_hints().steam_folders[0]
    }

    #[test]
    fn more_steam_roots_are_looked_in_on_linux_and_a_mac() {
        for (root, label) in [
            (".steam/root", "the Steam runtime's own link"),
            (".steam/debian-installation", "Debian's package"),
            ("snap/steam/common/.local/share/Steam", "the snap"),
            ("Library/Application Support/Steam", "a Mac"),
        ] {
            let home = tempfile::tempdir().unwrap();
            let game = Game::GtaVLegacy;
            touch(
                &home
                    .path()
                    .join(root)
                    .join("steamapps/common")
                    .join(rockstar_folder(game))
                    .join(exe_of(game)),
            );
            let found = discover_steam_in(steam_roots_under(home.path()));
            assert!(
                found
                    .iter()
                    .any(|i| i.game == game && i.source == DiscoverySource::Steam),
                "a Steam under {root} ({label}) was not found"
            );
        }
    }

    /// A prefix of each kind, each holding one install a different way.
    #[test]
    fn a_wine_prefix_of_any_kind_is_read_as_a_windows_drive() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let (legacy, enhanced) = (Game::GtaVLegacy, Game::GtaVEnhanced);

        // Wine's own default prefix: the Rockstar launcher's default folder.
        let wine = h
            .join(".wine/drive_c/Program Files/Rockstar Games")
            .join(rockstar_folder(legacy));
        touch(&wine.join(exe_of(legacy)));

        // Heroic keeps Proton prefixes as `<name>/pfx`; Epic names its install in a manifest.
        let heroic = h.join("Games/Heroic/Prefixes/default/GTAV/pfx/drive_c");
        let epic_name = legacy.profile().store_hints().epic_display_names[0];
        let manifest = heroic.join("ProgramData/Epic/EpicGamesLauncher/Data/Manifests/GTAV.item");
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::write(
            &manifest,
            format!(
                "{{\"DisplayName\": \"{epic_name}\", \"InstallLocation\": \"C:\\\\Program Files\\\\Epic Games\\\\GTAV\"}}"
            ),
        )
        .unwrap();
        let epic_install = heroic.join("Program Files/Epic Games/GTAV");
        touch(&epic_install.join(exe_of(legacy)));

        // A Windows Steam inside a Lutris prefix, with its library on the same C: drive.
        let lutris = h.join("Games/lutris-gta/drive_c");
        let lutris_game = lutris
            .join("Program Files (x86)/Steam/steamapps/common")
            .join(rockstar_folder(enhanced));
        touch(&lutris_game.join(exe_of(enhanced)));

        // A Bottles bottle, and a Proton prefix Steam keeps beside its own.
        let bottle = h
            .join(".local/share/bottles/bottles/gta/drive_c/Program Files/Rockstar Games")
            .join(rockstar_folder(enhanced));
        touch(&bottle.join(exe_of(enhanced)));
        let proton = h
            .join(".steam/steam/steamapps/compatdata/123/pfx/drive_c/Program Files/Rockstar Games")
            .join(rockstar_folder(legacy));
        touch(&proton.join(exe_of(legacy)));

        // On a Mac: CrossOver's bottles and Whisky's.
        let crossover = h.join("Library/Application Support/CrossOver/Bottles/gta/drive_c/Program Files/Rockstar Games").join(rockstar_folder(legacy));
        touch(&crossover.join(exe_of(legacy)));
        let whisky = h.join("Library/Containers/com.isaacmarovitz.Whisky/Bottles/ABC/drive_c/Program Files/Rockstar Games").join(rockstar_folder(enhanced));
        touch(&whisky.join(exe_of(enhanced)));

        let found = discover_in_prefixes_under(h);
        let dirs = |game| -> Vec<PathBuf> {
            found
                .iter()
                .filter(|i| i.game == game)
                .map(|i| i.install_dir.clone())
                .collect()
        };
        for (game, want, how) in [
            (legacy, &wine, "Wine's default prefix"),
            (
                legacy,
                &epic_install,
                "a Heroic prefix through an Epic manifest",
            ),
            (enhanced, &lutris_game, "a Steam inside a Lutris prefix"),
            (enhanced, &bottle, "a Bottles bottle"),
            (legacy, &proton, "a Proton prefix beside Steam's own"),
            (legacy, &crossover, "a CrossOver bottle"),
            (enhanced, &whisky, "a Whisky bottle"),
        ] {
            assert!(dirs(game).contains(want), "{how} was not found: {found:#?}");
        }
        assert!(
            found.iter().all(|i| i.exe.is_file()),
            "an install named an executable that is not there"
        );
    }

    #[test]
    fn a_windows_path_is_mapped_onto_its_c_drive_and_no_other() {
        let drive_c = Path::new("/p/drive_c");
        assert_eq!(
            map_windows_path(drive_c, r"C:\Program Files\x"),
            Some(drive_c.join("Program Files/x"))
        );
        assert_eq!(
            map_windows_path(drive_c, "c:/a/b"),
            Some(drive_c.join("a/b"))
        );
        assert_eq!(
            map_windows_path(drive_c, r"D:\Games"),
            None,
            "another drive is not in the prefix"
        );
        assert_eq!(map_windows_path(drive_c, "relative/path"), None);
    }
}
