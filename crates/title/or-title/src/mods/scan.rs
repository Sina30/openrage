use std::path::{Path, PathBuf};
use std::sync::Arc;

use or_io::container::rpf::crypto::RpfCryptoProvider;

use super::{InstalledMod, ModKind, ModPack, ModPlace, PARKED_DIR};
use crate::dlc::DlcList;
use crate::install::materials::GtaInstall;
use crate::vfs::game_mount::{dlc_pack_name, open_rpf_file};
use crate::vfs::{
    build_vfs_for_install_with, ArchiveDevice, FileDevice, FolderDevice, MountOptions, OVERLAY_DIR,
};

/// Every mod an install holds: the overlay's, the parked ones, and the game folder's ASIs and scripts.
///
/// A DLC pack's entry table is read through `crypto`, and `dlclist` says whether each pack is listed.
pub fn scan_install(
    root: &Path,
    crypto: &Arc<dyn RpfCryptoProvider>,
    dlclist: Option<&DlcList>,
) -> Vec<InstalledMod> {
    let mut mods = Vec::new();
    for (dir, place) in [
        (OVERLAY_DIR, ModPlace::Overlay),
        (PARKED_DIR, ModPlace::Parked),
    ] {
        for child in children(&root.join(dir)) {
            mods.push(overlay_mod(&child, place, crypto, dlclist));
        }
    }
    for child in children(root).into_iter().filter(|c| c.is_file()) {
        let name = file_name(&child);
        let kind = if is_asi(&name) {
            ModKind::AsiPlugin
        } else if name.eq_ignore_ascii_case("dinput8.dll") {
            ModKind::AsiLoader
        } else {
            continue;
        };
        mods.push(plain_mod(&child, kind, ModPlace::GameFolder));
    }
    for child in children(&root.join("scripts")) {
        let kind = if is_asi(&file_name(&child)) {
            ModKind::AsiPlugin
        } else {
            ModKind::ScriptMod
        };
        mods.push(plain_mod(&child, kind, ModPlace::GameFolder));
    }
    mods
}

fn children(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    out.sort_by_cached_key(|p| file_name(p).to_ascii_lowercase());
    out
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn is_asi(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("asi"))
}

fn plain_mod(path: &Path, kind: ModKind, place: ModPlace) -> InstalledMod {
    let name = file_name(path);
    let mut files = if path.is_dir() {
        FolderDevice::new(path).walk().unwrap_or_default()
    } else {
        vec![name.clone()]
    };
    files.sort_by_cached_key(|f| f.to_ascii_lowercase());
    InstalledMod {
        name,
        kind,
        place,
        root: path.to_path_buf(),
        files,
        packs: Vec::new(),
    }
}

fn overlay_mod(
    path: &Path,
    place: ModPlace,
    crypto: &Arc<dyn RpfCryptoProvider>,
    dlclist: Option<&DlcList>,
) -> InstalledMod {
    let mut found = plain_mod(path, ModKind::LooseFiles, place);
    found.packs = found
        .files
        .iter()
        .filter_map(|file| Some((pack_name_of(file)?, file)))
        .map(|(name, archive)| read_pack(path, name, archive, crypto, dlclist))
        .collect();
    if !found.packs.is_empty() {
        found.kind = ModKind::DlcPack;
    }
    found
}

/// The pack a `…/dlcpacks/<pack>/dlc.rpf` (or `dlc1.rpf`) names.
fn pack_name_of(file: &str) -> Option<&str> {
    let mut parts = file.rsplit('/');
    let leaf = parts.next()?.to_ascii_lowercase();
    let pack = parts.next()?;
    let in_dlcpacks = parts.next()?.eq_ignore_ascii_case("dlcpacks");
    let sub_pack = leaf
        .strip_prefix("dlc")
        .and_then(|rest| rest.strip_suffix(".rpf"))
        .is_some_and(|digits| digits.bytes().all(|b| b.is_ascii_digit()));
    (in_dlcpacks && sub_pack).then_some(pack)
}

fn read_pack(
    mod_root: &Path,
    name: &str,
    archive: &str,
    crypto: &Arc<dyn RpfCryptoProvider>,
    dlclist: Option<&DlcList>,
) -> ModPack {
    let read = open_rpf_file(&mod_root.join(archive), Arc::clone(crypto))
        .map(|opened| ArchiveDevice::new(opened).walk());
    let (entries, unread) = match read {
        Ok(walked) => (
            // ⚠ Every pack carries these two, so they are no claim on a path.
            (walked.unwrap_or_default().into_iter())
                .filter(|e| !matches!(e.as_str(), "setup2.xml" | "content.xml"))
                .collect(),
            None,
        ),
        Err(why) => (Vec::new(), Some(why.to_string())),
    };
    ModPack {
        name: name.to_string(),
        archive: archive.to_string(),
        listed: dlclist.map(|list| {
            (list.paths.iter()).any(|item| dlc_pack_name(&item.value).eq_ignore_ascii_case(name))
        }),
        entries,
        unread,
    }
}

/// The `dlclist.xml` the install's archives hold, read as the mount reads it; `None` when they do not open or hold none.
pub fn read_dlclist(
    root: &Path,
    crypto: Arc<dyn RpfCryptoProvider>,
    options: MountOptions,
) -> Option<DlcList> {
    let install = GtaInstall {
        root: root.to_path_buf(),
    };
    let vfs = build_vfs_for_install_with(&install, crypto, options).ok()?;
    DlcList::parse(&vfs.open("common:/data/dlclist.xml")?).ok()
}
