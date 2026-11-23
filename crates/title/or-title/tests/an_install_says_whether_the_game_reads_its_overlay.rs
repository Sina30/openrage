//! Whether a game install will read its `mods` overlay is decided by file names beside the executable.
//!
//! ⚠ Each case builds a synthetic install directory of empty files and varies
//! one name, so a rule that read the wrong edition's list, or the wrong half
//! of the pair, fails one of them. Nothing is run or loaded.

use std::path::Path;

use or_title::game::overlay_loader::{check_overlay_loader, OverlayLoaderReport};
use or_title::Game;

fn install(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("scratch");
    for name in files {
        std::fs::write(dir.path().join(name), b"").expect("a stand-in file");
    }
    dir
}

fn report(game: Game, root: &Path) -> OverlayLoaderReport {
    check_overlay_loader(game.profile(), root).expect("the folder reads")
}

fn legacy(files: &[&str]) -> OverlayLoaderReport {
    report(Game::GtaVLegacy, install(files).path())
}

fn enhanced(files: &[&str]) -> OverlayLoaderReport {
    report(Game::GtaVEnhanced, install(files).path())
}

#[test]
fn a_legacy_install_with_no_loader_does_not_read_the_overlay() {
    let said = legacy(&["GTA5.exe", "common.rpf"]);
    assert!(
        !said.reads(),
        "an install holding no plugin and no ASI loader reads nothing from mods"
    );
    assert!(said.lacks_redirect());
    assert!(said.loader.is_none());
    assert!(said.redirects.is_empty());
}

#[test]
fn a_legacy_plugin_and_the_loader_it_needs_read_the_overlay_in_any_case() {
    for plugin in ["OpenIV.asi", "RageOpenV.asi", "OPENIV.ASI"] {
        let said = legacy(&["GTA5.exe", plugin, "DINPUT8.dll"]);
        assert!(said.reads(), "{plugin} with dinput8.dll");
        assert_eq!(said.loader, Some("dinput8.dll"));
        assert_eq!(said.redirects.len(), 1, "{plugin}");
    }
}

#[test]
fn a_plugin_nothing_loads_is_a_missing_loader_and_a_loader_with_no_plugin_a_missing_plugin() {
    for plugin in ["OpenIV.asi", "RageOpenV.asi"] {
        let plugin_only = legacy(&["GTA5.exe", plugin]);
        assert!(!plugin_only.reads(), "{plugin} alone");
        assert!(plugin_only.lacks_loader() && !plugin_only.lacks_redirect());
        assert!(plugin_only.needs_loader_file());
        assert_eq!(plugin_only.rules.loader_files, ["dinput8.dll"]);
    }

    let loader_only = legacy(&["GTA5.exe", "dinput8.dll"]);
    assert!(!loader_only.reads());
    assert!(loader_only.lacks_redirect() && !loader_only.lacks_loader());
}

#[test]
fn a_plugin_for_the_other_edition_does_nothing_and_is_named() {
    let on_legacy = legacy(&["GTA5.exe", "OpenRPF.asi", "dinput8.dll"]);
    assert!(!on_legacy.reads(), "OpenRPF is Enhanced's plugin");
    assert!(on_legacy.lacks_redirect());
    assert_eq!(on_legacy.for_other_titles, ["OpenRPF.asi"]);

    let on_enhanced = enhanced(&["GTA5_Enhanced.exe", "OpenIV.asi", "xinput1_4.dll"]);
    assert!(!on_enhanced.reads(), "OpenIV.asi never loaded Enhanced");
    assert!(on_enhanced.lacks_redirect());
    assert_eq!(on_enhanced.for_other_titles, ["OpenIV.asi"]);
}

#[test]
fn an_enhanced_plugin_that_carries_its_own_loader_reads_alone() {
    let alone = enhanced(&["GTA5_Enhanced.exe", "OpenRPF.asi"]);
    assert!(alone.reads());
    assert!(alone.loader.is_none(), "no loader file was there to find");
}

#[test]
fn an_enhanced_plugin_without_its_own_loader_needs_the_loader_enhanced_loads() {
    let bare = enhanced(&["GTA5_Enhanced.exe", "RageOpenV.asi"]);
    assert!(!bare.reads() && bare.lacks_loader());

    let legacy_loader = enhanced(&["GTA5_Enhanced.exe", "RageOpenV.asi", "dinput8.dll"]);
    assert!(
        !legacy_loader.reads(),
        "dinput8.dll is the name Legacy loads, not Enhanced"
    );

    for loader in ["xinput1_4.dll", "dsound.dll"] {
        let said = enhanced(&["GTA5_Enhanced.exe", "RageOpenV.asi", loader]);
        assert!(said.reads(), "RageOpenV.asi with {loader}");
    }
}

#[test]
fn a_folder_that_cannot_be_read_is_an_error_and_not_a_verdict() {
    let gone = install(&[]).path().join("not-there");
    assert!(check_overlay_loader(Game::GtaVLegacy.profile(), &gone).is_err());
}

#[test]
fn a_directory_named_like_a_plugin_is_not_one() {
    let dir = install(&["GTA5.exe", "dinput8.dll"]);
    std::fs::create_dir(dir.path().join("OpenIV.asi")).expect("a directory");
    assert!(!report(Game::GtaVLegacy, dir.path()).reads());
}

#[test]
fn a_title_that_states_no_loader_rules_has_no_verdict() {
    let said = report(Game::Rdr2, install(&["RDR2.exe"]).path());
    assert!(!said.known(), "RDR2 declares no overlay loader");
    assert!(!said.reads() && !said.lacks_redirect() && !said.needs_loader_file());
}
