//! Nothing writes inside a game install without asking the edit gate.
//!
//! ⚠ A gate keyed on a hand-written list of ~100 famous basenames matched **27
//! of 172** `.rpf` files in a Legacy install and 27 of 169 in Enhanced, missing
//! **82 `dlc.rpf`** and **4 `dlc1.rpf`** under `update/x64/dlcpacks/`: 84% of
//! what it was for only looked protected. The gate classifies by install root.

use std::path::{Path, PathBuf};

use super::prefs::{EditMode, GamePaths, Prefs};
use super::retail::{classify, PathClass};

fn prefs_for(root: &Path, mode: EditMode) -> Prefs {
    let mut game_paths = GamePaths::default();
    game_paths.set(or_title::Game::GtaVLegacy, Some(root.to_path_buf()));
    Prefs {
        game_paths,
        edit_mode: mode,
        ..Default::default()
    }
}

/// Real shapes from a retail Legacy install, all of them shipped content; a
/// basename list matched only the first two.
#[test]
fn the_guard_covers_every_archive_in_an_install() {
    let root = PathBuf::from("C:/Games/GTAV");
    let prefs = prefs_for(&root, EditMode::ReadOnly);

    let shipped = [
        // Matched by a basename list.
        "update/update.rpf",
        "x64a.rpf",
        // Not matched by one. 82 of the install's 172 archives are the first shape.
        "update/x64/dlcpacks/mp2023_01/dlc.rpf",
        "update/x64/dlcpacks/mp2023_01_g9ec/dlc.rpf",
        "update/x64/dlcpacks/patchday27ng/dlc1.rpf",
        // A patch past the end of any list; one title update adds one.
        "patchday32ng.rpf",
        // Not an archive at all.
        "commandline.txt",
        "GTA5.exe",
        // Deep, and named nothing like anything on a list.
        "x64/audio/sfx/WEAPONS_PLAYER.rpf",
    ];

    for rel in shipped {
        let target = root.join(rel);
        assert_eq!(
            classify(&target, &prefs),
            PathClass::ShippedGame,
            "`{rel}` is inside the install and is not under `mods`, so it is the \
             game as shipped. If this says Outside, the gate has gone back to \
             matching names and has a hole the size of whatever is not on the list."
        );
    }
}

/// The overlay is inside the install and is the one place writing is the point.
#[test]
fn the_overlay_is_distinguished_from_the_game_as_shipped() {
    let root = PathBuf::from("C:/Games/GTAV");
    let prefs = prefs_for(&root, EditMode::ReadOnly);

    for rel in [
        "mods/update/update.rpf",
        "mods/x64a.rpf",
        "MODS/update/x64/dlcpacks/mp2023_01/dlc.rpf",
    ] {
        assert_eq!(
            classify(&root.join(rel), &prefs),
            PathClass::Overlay,
            "`{rel}` is under the overlay directory, which is where a user is \
             supposed to write. Classifying it as shipped would refuse the \
             feature the overlay exists for."
        );
    }

    // ⚠ `mods_` is how a user *disables* an overlay; matching it as the overlay
    // would make a deliberately parked mod folder writable.
    for rel in ["mods_/gunskins.rpf", "modsbackup/x64a.rpf", "mods.rpf"] {
        assert_eq!(
            classify(&root.join(rel), &prefs),
            PathClass::ShippedGame,
            "`{rel}` is not the overlay — the match is a whole path component, \
             not a prefix"
        );
    }

    // ⚠ The overlay is the **first** component below the root, and every case
    // above sits there — so an implementation matching `mods` at any depth
    // agrees with all six and is wrong. A user who creates the overlay at
    // `<install>/x64/mods` gets a folder the game ignores entirely; reading it
    // as the overlay makes `EditMode::Overlay` write into it without asking.
    for rel in [
        "x64/mods/foo.rpf",
        "update/mods/y.ydr",
        "x64/audio/mods/sfx.rpf",
    ] {
        assert_eq!(
            classify(&root.join(rel), &prefs),
            PathClass::ShippedGame,
            "`{rel}` has a `mods` component that is not the first one below the \
             install root, so it is shipped content. The defect this catches \
             only ever widens write permission."
        );
    }
}

/// Outside an install, every mode writes freely.
#[test]
fn nothing_outside_an_install_is_ever_gated() {
    let root = PathBuf::from("C:/Games/GTAV");
    for mode in EditMode::ALL {
        let prefs = prefs_for(&root, mode);
        for path in [
            "C:/Users/me/Desktop/thing.ydr",
            "D:/mods/wip/vehicles.meta",
            "D:/Backups/update.rpf",
        ] {
            assert_eq!(
                classify(Path::new(path), &prefs),
                PathClass::Outside,
                "{path} is not in an install. ⚠ A mode that refused this would be \
                 read as the application being broken, which is how a safety \
                 feature gets switched off permanently."
            );
        }
    }
}

/// ⚠ The default refuses, which is the whole of "off by default".
#[test]
fn the_default_mode_permits_neither_the_overlay_nor_the_game() {
    assert_eq!(EditMode::default(), EditMode::ReadOnly);
    assert!(!EditMode::default().permits_overlay());
    assert!(!EditMode::default().permits_shipped_game());

    assert!(EditMode::Overlay.permits_overlay());
    assert!(
        !EditMode::Overlay.permits_shipped_game(),
        "the middle mode is the useful one precisely because it stops at the \
         overlay; if it permitted shipped files it would be `Everything` with a \
         reassuring name"
    );
    assert!(EditMode::Everything.permits_overlay());
    assert!(EditMode::Everything.permits_shipped_game());

    for mode in EditMode::ALL {
        assert!(
            !mode.semantics().is_empty() && mode.semantics().len() > 40,
            "{:?} must state its write semantics in a sentence a user can act \
             on — that is half of what R25 asks for",
            mode
        );
        assert!(!mode.label().is_empty());
    }
}

/// A `Prefs` written by an older build still loads.
///
/// ⚠ An older file's `retail_edit_always_allow: true` was a persisted permanent
/// allow; inheriting it would re-grant a permission the user cannot see.
#[test]
fn an_older_prefs_file_loads_into_the_safe_default() {
    let older = r#"{
        "schema": 1,
        "retail_edit_always_allow": true,
        "rpf_output_mode": "Open"
    }"#;
    let prefs: Prefs = serde_json::from_str(older).expect("an older prefs file must still load");
    assert_eq!(
        prefs.edit_mode,
        EditMode::ReadOnly,
        "an unknown field must not carry the old permission forward. Landing in \
         `Everything` would silently re-grant what that boolean granted, to a \
         user who cannot see it happened."
    );
}

/// ⚠ A missing `read_overlay_first` lands *on*, the opposite of the write gate:
/// without `#[serde(default = "default_true")]` it deserialises to `false` and
/// the app silently ignores the user's own overlay files.
#[test]
fn an_older_prefs_file_still_reads_the_overlay() {
    let older = r#"{ "schema": 1, "rpf_output_mode": "Open" }"#;
    let prefs: Prefs = serde_json::from_str(older).expect("an older prefs file must still load");
    assert!(
        prefs.read_overlay_first,
        "a prefs file written before the overlay was read must still read it"
    );
    assert_eq!(prefs.mount_options(), or_title::MountOptions::default());

    let off = Prefs {
        read_overlay_first: false,
        ..Prefs::default()
    };
    assert_eq!(
        off.mount_options(),
        or_title::MountOptions::install_only(),
        "and the toggle reaches the mount rather than only the settings page"
    );
}

/// Every write path asks the gate.
///
/// ⚠ `WRITES` names each function that can put bytes on disk inside an install,
/// and each body is read from the source; a new write path must be added to it.
/// `save_active_document`'s loose-file branch and `save_active_document_as`
/// once went straight to `spawn_save` without asking, while a basename list
/// naming `commandline.txt` and `gta5.exe` made them look covered.
#[test]
fn the_gate_stands_in_front_of_every_write() {
    /// (file, function, why it is a write)
    const WRITES: &[(&str, &str, &str)] = &[
        (
            "file_io.rs",
            "save_active_document",
            "Ctrl+S on a loose file, which reaches spawn_save",
        ),
        (
            "file_io.rs",
            "save_active_document_as",
            "Save As to a path the picker offered",
        ),
        (
            "file_io.rs",
            "save_dirty_archives",
            "rebuilding and replacing an archive on disk",
        ),
        (
            "os_drop.rs",
            "bring_paths_into_folder",
            "files and folders dropped from Explorer into an archive folder",
        ),
        (
            "extract_job.rs",
            "extract_rpf_nodes",
            "writing an archive's entries into a directory a picker chose, \
             which is the one write whose target the user aims freely",
        ),
        (
            "files_view/mod.rs",
            "export_rpf_node_as",
            "writing one archive entry to a file a Save dialog named",
        ),
        (
            "convert.rs",
            "write_conversion",
            "writing what a conversion made to the file or folder a dialog named",
        ),
        (
            "dlc_pack.rs",
            "create_dlc_pack",
            "building a DLC pack into the folder a dialog named",
        ),
        (
            "gltf.rs",
            "write_gltf",
            "writing a glTF export, and the .bin and textures beside it, where a dialog named",
        ),
        (
            "save_destination.rs",
            "save_entry_to",
            "saving a document opened from an archive into the archive, a \
             loose file or the overlay, whichever the user chose",
        ),
        (
            "overlay_copy.rs",
            "answer_overlay_copy",
            "copying a shipped archive into the mods overlay",
        ),
        (
            "mod_manager.rs",
            "answer_mod_move",
            "moving a mod's folder between the mods overlay and mods_",
        ),
        ("rpf_tree.rs", "rpf_add_folder", "a folder in an archive"),
        (
            "rpf_tree.rs",
            "rpf_rename_node",
            "renaming an archive entry",
        ),
        (
            "rpf_tree.rs",
            "rpf_delete_node",
            "deleting an archive entry",
        ),
        (
            "delete_prompt.rs",
            "answer_delete",
            "the delete itself, once its question is answered",
        ),
        (
            "rpf_moves.rs",
            "rpf_move_nodes",
            "moving archive entries or files on disk, one row or several",
        ),
        (
            "rpf_tree.rs",
            "rpf_import_file_to_folder",
            "importing a file into an archive",
        ),
        ("mod.rs", "rpf_add_empty_file", "a new file in an archive"),
        (
            "clipboard/mod.rs",
            "paste_rows",
            "pasting copied rows into an archive or a folder, and taking a cut's away",
        ),
        (
            "undo.rs",
            "may_write",
            "an undo or redo of a paste's cut, writing the cut file back or sending it to the bin",
        ),
        (
            "defrag_job.rs",
            "defragment_archive",
            "rebuilding an archive in place without its slack",
        ),
        (
            "defrag_job.rs",
            "answer_defrag",
            "the defragment's write, asked again once the preview is answered",
        ),
    ];

    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/state");
    for (file, function, why) in WRITES {
        let text =
            std::fs::read_to_string(dir.join(file)).unwrap_or_else(|e| panic!("read {file}: {e}"));
        let body = function_body(&text, function)
            .unwrap_or_else(|| panic!("{file} no longer has `{function}` — {why}"));

        // ⚠ Not `retail_guard_for(`: asking it only whether to prompt let a
        // refusal fall through to the write.
        let asks = body.contains("gate_permits(") || body.contains("gate_tree_edit(");
        assert!(
            asks,
            "`{function}` in {file} writes ({why}) and does not consult the edit \
             gate. Call `self.gate_permits(Some(&path))` and route \
             `GateOutcome::Confirm` through `pending_retail_edit`.\n\n\
             Two functions were in exactly this state before R25, and the guard \
             beside them made them look covered."
        );
    }

    // The gate must also be the only thing that decides, so the raw predicate
    // stays out of the write paths.
    let file_io = std::fs::read_to_string(dir.join("file_io.rs")).expect("file_io.rs");
    let stray = file_io.matches("classify(").count();
    assert!(
        stray <= 1,
        "file_io.rs calls `classify` {stray} times. Deciding inline \
         reimplements the mode check — ask `gate_permits` so the answer comes \
         from one place and the refusal is reported one way."
    );
}

/// Every prompt the gate can raise has somewhere to be applied.
///
/// ⚠ A `PendingRetailEdit` variant with no arm in `apply_pending_retail_edit` is
/// a write that the user confirms and that then silently does not happen — worse
/// than an ungated write, because they believe it landed.
#[test]
fn every_confirmable_action_can_be_applied() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/state");
    let prompts =
        std::fs::read_to_string(dir.join("pending_prompts.rs")).expect("pending_prompts.rs");
    let tree = std::fs::read_to_string(dir.join("rpf_tree.rs")).expect("rpf_tree.rs");

    let declared = variants_of(&prompts, "pub enum PendingRetailEdit");
    assert!(
        declared.len() >= 8,
        "found only {} PendingRetailEdit variant(s), which is not this enum: {declared:?}",
        declared.len()
    );

    let applied = function_body(&tree, "apply_pending_retail_edit")
        .expect("rpf_tree.rs must still have apply_pending_retail_edit");
    for variant in &declared {
        assert!(
            applied.contains(&format!("PendingRetailEdit::{variant}")),
            "`{variant}` can be raised and has no arm in \
             `apply_pending_retail_edit`, so confirming it does nothing at all"
        );
    }
}

/// ⚠ The banner says something different in every mode, because a banner is
/// only a safety feature while its states are distinguishable at a glance.
#[test]
fn the_banner_distinguishes_every_mode() {
    let menu = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/panels/menu.rs"),
    )
    .expect("menu.rs");
    let body = function_body(&menu, "what_the_banner_says")
        .expect("menu.rs must say what the banner says");

    // Four arms, not three: base-game editing reads differently once a write has
    // actually been confirmed, which is the state a user most needs to see.
    let arms = [
        "EditMode::ReadOnly",
        "EditMode::Overlay",
        "EditMode::Everything if state.retail_edit_session_allow",
        "EditMode::Everything =>",
    ];
    for arm in arms {
        assert!(
            body.contains(arm),
            "the banner has no arm for `{arm}`, so that state renders as another \
             one and a user cannot tell them apart"
        );
    }

    let strings: Vec<&str> = body
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| s.len() > 3 && s.chars().any(char::is_alphabetic))
        .collect();
    let mut unique: Vec<&str> = strings.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        strings.len(),
        unique.len(),
        "two banner states render the same text: {strings:?}. Distinct is the \
         requirement, not merely present."
    );
    assert!(
        unique.len() >= 4,
        "found {} distinct banner string(s), expected one per state: {unique:?}",
        unique.len()
    );
}

/// A refused write to a shipped archive offers the overlay copy instead.
///
/// ⚠ Only where the mode lets the overlay be written: offering a copy the gate
/// would then refuse is a dialog that ends in a second refusal.
#[test]
fn an_edit_outside_the_overlay_offers_the_copy() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let install = tmp.path().join("GTAV");
    std::fs::create_dir_all(install.join("update")).expect("install dirs");
    std::fs::write(install.join("gta5.exe"), b"an install marker").expect("marker");
    let shipped = install.join("update").join("update.rpf");
    std::fs::write(&shipped, b"shipped archive").expect("archive");

    let mut state = super::AppState::new();
    state.retail_edit_session_allow = false;
    state.prefs.edit_mode = EditMode::Overlay;
    assert!(matches!(
        state.gate_permits(Some(&shipped)),
        super::GateOutcome::Refused
    ));
    let offer = state
        .overlay_copy
        .offer
        .clone()
        .expect("a refused write to a shipped archive must offer the overlay copy");
    assert_eq!(offer.source, shipped);
    assert_eq!(
        offer.dest,
        install.join("mods").join("update").join("update.rpf")
    );

    state.overlay_copy.offer = None;
    state.prefs.edit_mode = EditMode::ReadOnly;
    assert!(matches!(
        state.gate_permits(Some(&shipped)),
        super::GateOutcome::Refused
    ));
    assert_eq!(state.overlay_copy.offer, None);
}

/// Saving or editing a shipped archive in the mounted install is refused and offers the copy.
///
/// ⚠ The tree's writes once asked the gate only whether to prompt, so a refusal
/// fell through and the save replaced the shipped archive.
#[test]
fn a_refused_archive_in_the_mounted_install_is_not_written() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let install = tmp.path().join("GTAV");
    std::fs::create_dir_all(install.join("update")).expect("install dirs");
    std::fs::write(install.join("gta5.exe"), b"an install marker").expect("marker");
    let shipped = install.join("update").join("update.rpf");
    let dest = install.join("mods").join("update").join("update.rpf");

    let mut state = super::AppState::new();
    state.retail_edit_session_allow = false;
    state.prefs.edit_mode = EditMode::Overlay;
    let bytes = super::document_io::serialize_archive_records(
        vec![("a.txt".to_string(), b"entry".to_vec(), false)],
        super::RpfOutputMode::Open,
        state.keys.provider(),
    )
    .expect("archive bytes");
    std::fs::write(&shipped, &bytes).expect("archive");
    let archive = super::rpf_archive_open::open_archive_from_disk(&shipped, state.keys.provider())
        .expect("open the archive");

    state.open_game_root(install.clone());
    let doc_id = state.documents[0].id;
    let entry = {
        let Some(super::DocPayload::Rpf(rpf)) = state.document_mut(doc_id).map(|d| &mut d.payload)
        else {
            panic!("the install mounts as a GameRoot");
        };
        let root = rpf.session.root_id();
        let node = rpf
            .session
            .add_unopened_disk_archive(root, "update.rpf".to_string(), shipped.clone())
            .expect("the archive's node");
        let id = rpf
            .session
            .register_archive(archive.clone(), Some(shipped.clone()))
            .expect("an id for the archive");
        if let Some(super::RpfNodeBacking::Archive { archive_id, .. }) =
            rpf.session.node_mut(node).map(|n| &mut n.backing)
        {
            *archive_id = Some(id);
        }
        let listing = super::expander::list_archive(archive.clone()).expect("list");
        rpf.session
            .populate_archive_children(node, id, &listing)
            .expect("populate");
        let entry = *rpf.session.children_of(node).last().expect("the entry");
        rpf.session
            .set_bytes_for(entry, b"edited".to_vec())
            .expect("edit the entry");
        entry
    };

    state.active_document = Some(doc_id);
    state.save_active_document();
    assert!(
        std::fs::read(&shipped).expect("shipped") == bytes,
        "a save the gate refuses must leave the shipped archive alone"
    );
    let offered = state.overlay_copy.offer.take().map(|o| o.dest);
    assert_eq!(offered.as_ref(), Some(&dest), "and offer the overlay copy");

    assert!(state.rpf_rename_node(doc_id, entry, "b.txt").is_err());
    let offered = state.overlay_copy.offer.take().map(|o| o.dest);
    assert_eq!(
        offered.as_ref(),
        Some(&dest),
        "a refused tree edit offers it too"
    );
}

/// In `ReadOnly`, saving a dirty archive under the overlay is refused too.
///
/// ⚠ The save once asked the gate only about shipped archives among the dirty
/// ones, so a mounted install whose only dirty archive sat under `mods` asked
/// about nothing and was written.
#[test]
fn read_only_refuses_saving_an_archive_in_the_overlay() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let install = tmp.path().join("GTAV");
    std::fs::create_dir_all(install.join("mods").join("update")).expect("install dirs");
    std::fs::write(install.join("gta5.exe"), b"an install marker").expect("marker");
    let overlay = install.join("mods").join("update").join("update.rpf");

    let mut state = super::AppState::new();
    state.retail_edit_session_allow = false;
    state.prefs.edit_mode = EditMode::ReadOnly;
    let bytes = super::document_io::serialize_archive_records(
        vec![("a.txt".to_string(), b"entry".to_vec(), false)],
        super::RpfOutputMode::Open,
        state.keys.provider(),
    )
    .expect("archive bytes");
    std::fs::write(&overlay, &bytes).expect("archive");
    let archive = super::rpf_archive_open::open_archive_from_disk(&overlay, state.keys.provider())
        .expect("open the archive");

    state.open_game_root(install.clone());
    let doc_id = state.documents[0].id;
    {
        let Some(super::DocPayload::Rpf(rpf)) = state.document_mut(doc_id).map(|d| &mut d.payload)
        else {
            panic!("the install mounts as a GameRoot");
        };
        let root = rpf.session.root_id();
        let node = rpf
            .session
            .add_unopened_disk_archive(root, "update.rpf".to_string(), overlay.clone())
            .expect("the archive's node");
        let id = rpf
            .session
            .register_archive(archive.clone(), Some(overlay.clone()))
            .expect("an id for the archive");
        if let Some(super::RpfNodeBacking::Archive { archive_id, .. }) =
            rpf.session.node_mut(node).map(|n| &mut n.backing)
        {
            *archive_id = Some(id);
        }
        let listing = super::expander::list_archive(archive.clone()).expect("list");
        rpf.session
            .populate_archive_children(node, id, &listing)
            .expect("populate");
        let entry = *rpf.session.children_of(node).last().expect("the entry");
        rpf.session
            .set_bytes_for(entry, b"edited".to_vec())
            .expect("edit the entry");
    }

    state.active_document = Some(doc_id);
    state.save_active_document();
    assert!(
        std::fs::read(&overlay).expect("overlay") == bytes,
        "`ReadOnly` writes nothing inside an install, the overlay included"
    );
}

/// The body of `fn name`, by brace depth.
fn function_body(text: &str, name: &str) -> Option<String> {
    let at = text.find(&format!("fn {name}("))?;
    let rest = &text[at..];
    let open = rest.find('{')?;
    let mut depth = 0usize;
    for (i, c) in rest[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(rest[open..open + i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// The variant names declared by an enum, by brace depth.
fn variants_of(text: &str, header: &str) -> Vec<String> {
    let Some(at) = text.find(header) else {
        return Vec::new();
    };
    let rest = &text[at..];
    let Some(open) = rest.find('{') else {
        return Vec::new();
    };
    let mut depth = 0usize;
    let mut out = Vec::new();
    for line in rest[open..].lines() {
        let trimmed = line.trim();
        let before = depth;
        depth += trimmed.matches('{').count();
        depth = depth.saturating_sub(trimmed.matches('}').count());
        // A variant sits at depth 1: inside the enum, not inside a variant's
        // own field block.
        if before == 1
            && !trimmed.starts_with("//")
            && !trimmed.starts_with('#')
            && trimmed
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase())
        {
            let name: String = trimmed
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                out.push(name);
            }
        }
        if depth == 0 && before > 0 {
            break;
        }
    }
    out
}
