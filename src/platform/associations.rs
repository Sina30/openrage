//! Windows file associations: OpenRage as a handler for the formats it opens.
//!
//! ⚠ **Nothing here runs unless the user asks**, and everything it writes is
//! journalled so the same click can take it back. Registration lives under the
//! current user's `Software\Classes`, never the machine's, so it needs no
//! elevation and cannot affect another account.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Where a real registration is written, relative to `HKEY_CURRENT_USER`.
#[cfg(windows)]
pub const USER_CLASSES: &str = r"Software\Classes";

/// One extension to offer, and the noun a user reads beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub ext: String,
    pub noun: String,
    /// Checked when the page first opens: game formats yes, generic ones no.
    pub suggested: bool,
}

/// Extensions other programs have a better claim to, so they are offered but
/// never suggested.
const GENERIC: &[&str] = &[
    "xml", "txt", "json", "ini", "csv", "log", "lua", "dat", "cfg", "dds", "png", "jpg", "jpeg",
    "bmp", "tga", "meta", "ide", "ipl",
];

/// Every extension some document kind opens, once each, in name order.
pub fn candidates() -> Vec<Candidate> {
    let mut by_ext: BTreeMap<String, String> = BTreeMap::new();
    for info in crate::state::kinds::all() {
        for ext in info.extensions() {
            by_ext
                .entry(ext.to_ascii_lowercase())
                .or_insert_with(|| info.noun.to_string());
        }
    }
    // ⚠ Not a document kind, since a project is not a file the editor opens as a tab: a launch with
    // one on its command line opens it as a project, which is what this registers.
    by_ext
        .entry(or_project::PROJECT_EXTENSION.to_string())
        .or_insert_with(|| "OpenRage project".to_string());
    by_ext
        .into_iter()
        .map(|(ext, noun)| Candidate {
            suggested: !GENERIC.contains(&ext.as_str()),
            ext,
            noun,
        })
        .collect()
}

/// The formats OpenRage opens, as JSON for a packager: an array of `{"ext", "noun", "suggested"}`.
///
/// The one list a desktop file's MIME types, a macOS bundle's document types and the Settings
/// page's rows are all made from, so a package cannot name a format the app does not open.
pub fn file_types_json() -> String {
    let rows: Vec<serde_json::Value> = candidates()
        .into_iter()
        .map(|c| serde_json::json!({ "ext": c.ext, "noun": c.noun, "suggested": c.suggested }))
        .collect();
    serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".to_string())
}

/// The programmatic identifier OpenRage registers for one extension.
pub fn prog_id(ext: &str) -> String {
    format!("OpenRage.{}", ext.to_ascii_lowercase())
}

/// Access to one registry subtree, keyed by `\`-separated paths below it.
///
/// ⚠ **A trait so the planner is tested against a map**, and the Windows
/// implementation against a scratch key: no test registers an association on
/// the machine running it.
pub trait Registry {
    fn read(&self, key: &str, value: &str) -> Result<Option<String>, String>;
    fn key_exists(&self, key: &str) -> Result<bool, String>;
    /// Write a string value, creating the key and every key above it.
    fn write(&mut self, key: &str, value: &str, data: &str) -> Result<(), String>;
    fn remove_value(&mut self, key: &str, value: &str) -> Result<(), String>;
    /// Remove a key that holds no values and no subkeys; say whether it went.
    fn remove_key_if_empty(&mut self, key: &str) -> Result<bool, String>;
}

/// One value OpenRage set, and what was there before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueChange {
    pub key: String,
    pub value: String,
    pub written: String,
    pub before: Option<String>,
}

/// Everything one registration did, in the order it did it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    pub changes: Vec<ValueChange>,
    /// Keys that did not exist before, in the order they were created.
    pub created_keys: Vec<String>,
}

impl Journal {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.created_keys.is_empty()
    }

    /// The extensions this journal registered, for the page to say so.
    pub fn extensions(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .changes
            .iter()
            .filter_map(|c| c.key.strip_suffix(r"\OpenWithProgids"))
            .filter_map(|k| k.strip_prefix('.'))
            .map(str::to_string)
            .collect();
        out.dedup();
        out
    }
}

/// The values a registration of `exts` sets, in the order it sets them.
///
/// `make_default` also points each extension's own default at OpenRage;
/// without it OpenRage is only added to the extension's Open With list, which
/// takes nothing away from whatever opens the file today.
pub fn desired(
    exe: &Path,
    exts: &[(String, String)],
    make_default: bool,
) -> Vec<(String, String, String)> {
    let exe = exe.display().to_string();
    let mut out = Vec::new();
    for (ext, noun) in exts {
        let ext = ext.to_ascii_lowercase();
        let id = prog_id(&ext);
        out.push((id.clone(), String::new(), format!("{noun} (OpenRage)")));
        out.push((
            format!(r"{id}\DefaultIcon"),
            String::new(),
            format!("\"{exe}\",0"),
        ));
        out.push((
            format!(r"{id}\shell\open\command"),
            String::new(),
            format!("\"{exe}\" \"%1\""),
        ));
        out.push((
            format!(r".{ext}\OpenWithProgids"),
            id.clone(),
            String::new(),
        ));
        if make_default {
            out.push((format!(".{ext}"), String::new(), id));
        }
    }
    out
}

/// Write `wanted`, recording what each write replaced.
///
/// ⚠ **All or nothing.** A write that fails halfway takes back the ones before
/// it, so a refused key cannot leave half an association behind.
pub fn apply(
    reg: &mut dyn Registry,
    wanted: &[(String, String, String)],
) -> Result<Journal, String> {
    let mut journal = Journal::default();
    for (key, value, data) in wanted {
        if let Err(e) = apply_one(reg, &mut journal, key, value, data) {
            let _ = undo(reg, &journal);
            return Err(e);
        }
    }
    Ok(journal)
}

fn apply_one(
    reg: &mut dyn Registry,
    journal: &mut Journal,
    key: &str,
    value: &str,
    data: &str,
) -> Result<(), String> {
    let mut prefix = String::new();
    for part in key.split('\\') {
        if !prefix.is_empty() {
            prefix.push('\\');
        }
        prefix.push_str(part);
        let known = journal
            .created_keys
            .iter()
            .any(|k| k.eq_ignore_ascii_case(&prefix));
        if !known && !reg.key_exists(&prefix)? {
            journal.created_keys.push(prefix.clone());
        }
    }
    let before = reg.read(key, value)?;
    if before.as_deref() == Some(data) {
        return Ok(());
    }
    reg.write(key, value, data)?;
    journal.changes.push(ValueChange {
        key: key.to_string(),
        value: value.to_string(),
        written: data.to_string(),
        before,
    });
    Ok(())
}

/// What an undo could not put back, because something else changed it since.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct UndoReport {
    pub restored: usize,
    pub left_alone: Vec<String>,
}

/// Put back everything `journal` recorded, newest first.
///
/// ⚠ **A value somebody else has changed since is left alone.** Restoring the
/// old default over a program the user chose afterwards would undo *their*
/// choice, not ours; the report names it instead.
pub fn undo(reg: &mut dyn Registry, journal: &Journal) -> Result<UndoReport, String> {
    let mut report = UndoReport::default();
    for change in journal.changes.iter().rev() {
        let now = reg.read(&change.key, &change.value)?;
        if now.as_deref() != Some(change.written.as_str()) {
            report
                .left_alone
                .push(format!(r"{}\{}", change.key, change.value));
            continue;
        }
        match &change.before {
            Some(before) => reg.write(&change.key, &change.value, before)?,
            None => reg.remove_value(&change.key, &change.value)?,
        }
        report.restored += 1;
    }
    for key in journal.created_keys.iter().rev() {
        reg.remove_key_if_empty(key)?;
    }
    Ok(report)
}

/// Which of `exts` currently open in this executable.
pub fn registered(reg: &dyn Registry, exe: &Path, exts: &[String]) -> Vec<String> {
    let command = format!("\"{}\" \"%1\"", exe.display());
    exts.iter()
        .filter(|ext| {
            let id = prog_id(ext);
            let listed = reg
                .read(&format!(r".{ext}\OpenWithProgids"), &id)
                .ok()
                .flatten()
                .is_some();
            let ours = reg
                .read(&format!(r"{id}\shell\open\command"), "")
                .ok()
                .flatten()
                .is_some_and(|c| c.eq_ignore_ascii_case(&command));
            listed && ours
        })
        .cloned()
        .collect()
}

/// Read a journal written by an earlier registration.
pub fn load_journal(path: &Path) -> Journal {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save_journal(path: &Path, journal: &Journal) -> Result<(), String> {
    if journal.is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(journal).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

/// Two journals as one, so a second registration's undo also takes back the
/// first's.
pub fn merged(mut older: Journal, newer: Journal) -> Journal {
    older.changes.extend(newer.changes);
    for key in newer.created_keys {
        if !older
            .created_keys
            .iter()
            .any(|k| k.eq_ignore_ascii_case(&key))
        {
            older.created_keys.push(key);
        }
    }
    older
}

/// Tell Explorer the associations changed, so icons and Open With refresh.
pub fn notify_shell() {
    #[cfg(windows)]
    // SAFETY: SHChangeNotify with SHCNF_IDLIST and two null items is the
    // documented form of SHCNE_ASSOCCHANGED and reads no memory of ours.
    unsafe {
        use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        );
    }
}

/// The current user's registry below one path.
#[cfg(windows)]
pub struct WindowsRegistry {
    root: winreg::RegKey,
}

#[cfg(windows)]
impl WindowsRegistry {
    /// `HKEY_CURRENT_USER\<below>`, created only if it is missing.
    pub fn under_current_user(below: &str) -> Result<Self, String> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        if let Ok(root) = hkcu.open_subkey_with_flags(below, winreg::enums::KEY_ALL_ACCESS) {
            return Ok(Self { root });
        }
        let (root, _) = hkcu
            .create_subkey(below)
            .map_err(|e| format!(r"HKCU\{below}: {e}"))?;
        Ok(Self { root })
    }

    fn open(&self, key: &str) -> Option<winreg::RegKey> {
        self.root.open_subkey(key).ok()
    }
}

#[cfg(windows)]
impl Registry for WindowsRegistry {
    fn read(&self, key: &str, value: &str) -> Result<Option<String>, String> {
        let Some(k) = self.open(key) else {
            return Ok(None);
        };
        match k.get_raw_value(value) {
            Ok(raw) => {
                let units: Vec<u16> = raw
                    .bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .take_while(|&u| u != 0)
                    .collect();
                Ok(Some(String::from_utf16_lossy(&units)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!(r"{key}\{value}: {e}")),
        }
    }

    fn key_exists(&self, key: &str) -> Result<bool, String> {
        Ok(self.open(key).is_some())
    }

    fn write(&mut self, key: &str, value: &str, data: &str) -> Result<(), String> {
        let (k, _) = self
            .root
            .create_subkey(key)
            .map_err(|e| format!("{key}: {e}"))?;
        k.set_value(value, &data.to_string())
            .map_err(|e| format!(r"{key}\{value}: {e}"))
    }

    fn remove_value(&mut self, key: &str, value: &str) -> Result<(), String> {
        let Ok(k) = self
            .root
            .open_subkey_with_flags(key, winreg::enums::KEY_ALL_ACCESS)
        else {
            return Ok(());
        };
        match k.delete_value(value) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!(r"{key}\{value}: {e}"))
            }
            _ => Ok(()),
        }
    }

    fn remove_key_if_empty(&mut self, key: &str) -> Result<bool, String> {
        let Some(k) = self.open(key) else {
            return Ok(false);
        };
        if k.enum_keys().next().is_some() || k.enum_values().next().is_some() {
            return Ok(false);
        }
        drop(k);
        self.root
            .delete_subkey(key)
            .map(|()| true)
            .map_err(|e| format!("{key}: {e}"))
    }
}

/// A registry in memory, for the planner's tests.
#[cfg(test)]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MapRegistry {
    pub keys: BTreeMap<String, BTreeMap<String, String>>,
    pub refuse: Option<String>,
}

#[cfg(test)]
impl MapRegistry {
    fn norm(key: &str) -> String {
        key.to_ascii_lowercase()
    }
}

#[cfg(test)]
impl Registry for MapRegistry {
    fn read(&self, key: &str, value: &str) -> Result<Option<String>, String> {
        Ok(self
            .keys
            .get(&Self::norm(key))
            .and_then(|v| v.get(&value.to_ascii_lowercase()))
            .cloned())
    }

    fn key_exists(&self, key: &str) -> Result<bool, String> {
        Ok(self.keys.contains_key(&Self::norm(key)))
    }

    fn write(&mut self, key: &str, value: &str, data: &str) -> Result<(), String> {
        if self.refuse.as_deref().is_some_and(|r| key.contains(r)) {
            return Err(format!("{key}: access denied"));
        }
        let mut prefix = String::new();
        for part in key.split('\\') {
            if !prefix.is_empty() {
                prefix.push('\\');
            }
            prefix.push_str(part);
            self.keys.entry(Self::norm(&prefix)).or_default();
        }
        self.keys
            .entry(Self::norm(key))
            .or_default()
            .insert(value.to_ascii_lowercase(), data.to_string());
        Ok(())
    }

    fn remove_value(&mut self, key: &str, value: &str) -> Result<(), String> {
        if let Some(values) = self.keys.get_mut(&Self::norm(key)) {
            values.remove(&value.to_ascii_lowercase());
        }
        Ok(())
    }

    fn remove_key_if_empty(&mut self, key: &str) -> Result<bool, String> {
        let norm = Self::norm(key);
        let child = format!("{norm}\\");
        let has_children = self.keys.keys().any(|k| k.starts_with(&child));
        let empty = self.keys.get(&norm).is_some_and(|v| v.is_empty());
        if empty && !has_children {
            self.keys.remove(&norm);
            return Ok(true);
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Tools\OpenRage\openrage.exe")
    }

    fn ydr() -> Vec<(String, String)> {
        vec![
            ("ydr".to_string(), "Drawable".to_string()),
            ("ytd".to_string(), "Texture dictionary".to_string()),
        ]
    }

    /// A machine where `.ytd` already opens in another tool.
    fn machine() -> MapRegistry {
        let mut reg = MapRegistry::default();
        reg.write(".ytd", "", "OtherTool.ytd").unwrap();
        reg.write(r".ytd\OpenWithProgids", "OtherTool.ytd", "")
            .unwrap();
        reg.write(r"OtherTool.ytd\shell\open\command", "", "other.exe %1")
            .unwrap();
        reg
    }

    #[test]
    fn registering_then_undoing_leaves_the_registry_as_it_was() {
        let mut reg = machine();
        let before = reg.clone();
        let journal = apply(&mut reg, &desired(&exe(), &ydr(), true)).unwrap();
        assert_ne!(reg, before);
        assert_eq!(
            reg.read(".ytd", "").unwrap().as_deref(),
            Some("OpenRage.ytd"),
            "made default on request"
        );
        assert_eq!(
            registered(&reg, &exe(), &["ydr".into(), "ytd".into(), "ybn".into()]),
            ["ydr", "ytd"]
        );
        assert_eq!(journal.extensions(), ["ydr", "ytd"]);

        let report = undo(&mut reg, &journal).unwrap();
        assert!(report.left_alone.is_empty(), "{report:?}");
        assert_eq!(reg, before, "undo is exact, keys included");
    }

    #[test]
    fn without_make_default_nothing_already_there_is_touched() {
        let mut reg = machine();
        apply(&mut reg, &desired(&exe(), &ydr(), false)).unwrap();
        assert_eq!(
            reg.read(".ytd", "").unwrap().as_deref(),
            Some("OtherTool.ytd")
        );
        assert_eq!(
            reg.read(r".ytd\OpenWithProgids", "OtherTool.ytd")
                .unwrap()
                .as_deref(),
            Some("")
        );
        assert!(reg
            .read(r".ytd\OpenWithProgids", "OpenRage.ytd")
            .unwrap()
            .is_some());
    }

    #[test]
    fn a_failed_write_takes_back_what_came_before_it() {
        let mut reg = machine();
        let before = reg.clone();
        reg.refuse = Some(r"OpenRage.ytd\shell".to_string());
        let err = apply(&mut reg, &desired(&exe(), &ydr(), true)).unwrap_err();
        assert!(err.contains("access denied"), "{err}");
        reg.refuse = None;
        assert_eq!(reg, before, "half an association is worse than none");
    }

    #[test]
    fn undo_leaves_a_choice_made_since_and_says_so() {
        let mut reg = machine();
        let journal = apply(&mut reg, &desired(&exe(), &ydr(), true)).unwrap();
        reg.write(".ytd", "", "ThirdTool.ytd").unwrap();
        let report = undo(&mut reg, &journal).unwrap();
        assert_eq!(report.left_alone, [r".ytd\"]);
        assert_eq!(
            reg.read(".ytd", "").unwrap().as_deref(),
            Some("ThirdTool.ytd")
        );
        assert!(registered(&reg, &exe(), &["ydr".into(), "ytd".into()]).is_empty());
    }

    #[test]
    fn registering_twice_journals_nothing_the_second_time() {
        let mut reg = machine();
        let wanted = desired(&exe(), &ydr(), false);
        let first = apply(&mut reg, &wanted).unwrap();
        let second = apply(&mut reg, &wanted).unwrap();
        assert!(second.is_empty());
        let both = merged(first.clone(), second);
        assert_eq!(both, first);
    }

    #[test]
    fn the_journal_survives_a_restart_and_an_empty_one_removes_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file-associations.json");
        let mut reg = machine();
        let journal = apply(&mut reg, &desired(&exe(), &ydr(), true)).unwrap();
        save_journal(&path, &journal).unwrap();
        assert_eq!(load_journal(&path), journal);
        save_journal(&path, &Journal::default()).unwrap();
        assert!(!path.exists());
        assert_eq!(load_journal(&path), Journal::default());
    }

    #[test]
    fn game_formats_are_suggested_and_generic_ones_are_not() {
        let all = candidates();
        let find = |e: &str| all.iter().find(|c| c.ext == e);
        assert!(find("ydr").is_some_and(|c| c.suggested));
        assert!(find("rpf").is_some_and(|c| c.suggested));
        if let Some(xml) = find("xml") {
            assert!(!xml.suggested);
        }
    }

    /// ⚠ **The real registry, below a scratch key that is not `Classes`**, so
    /// the Windows implementation is exercised and nothing is associated.
    #[cfg(windows)]
    #[test]
    fn the_windows_registry_round_trips_below_a_scratch_key() {
        let scratch = format!(
            r"Software\OpenRage-test-scratch\{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        assert!(!scratch.contains("Classes"));
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
                let _ = hkcu.delete_subkey_all(&self.0);
                let _ = hkcu.delete_subkey(r"Software\OpenRage-test-scratch");
            }
        }
        let _cleanup = Cleanup(scratch.clone());

        let mut reg = WindowsRegistry::under_current_user(&scratch).unwrap();
        reg.write(".ytd", "", "OtherTool.ytd").unwrap();
        let journal = apply(&mut reg, &desired(&exe(), &ydr(), true)).unwrap();
        assert_eq!(
            reg.read(".ytd", "").unwrap().as_deref(),
            Some("OpenRage.ytd")
        );
        assert_eq!(
            reg.read(r"OpenRage.ydr\shell\open\command", "")
                .unwrap()
                .as_deref(),
            Some(r#""C:\Tools\OpenRage\openrage.exe" "%1""#)
        );
        assert_eq!(registered(&reg, &exe(), &["ydr".into()]), ["ydr"]);

        undo(&mut reg, &journal).unwrap();
        assert_eq!(
            reg.read(".ytd", "").unwrap().as_deref(),
            Some("OtherTool.ytd")
        );
        assert!(!reg.key_exists("OpenRage.ydr").unwrap(), "created keys go");
        assert!(!reg.key_exists(".ydr").unwrap());
    }

    #[test]
    fn the_formats_a_package_is_told_are_the_ones_the_settings_page_offers() {
        let told: Vec<serde_json::Value> =
            serde_json::from_str(&file_types_json()).expect("a JSON array");
        let offered = candidates();
        assert!(
            offered.len() > 20,
            "the registry offered {} formats",
            offered.len()
        );
        assert_eq!(told.len(), offered.len());
        for (row, candidate) in told.iter().zip(&offered) {
            assert_eq!(row["ext"], candidate.ext);
            assert_eq!(row["noun"], candidate.noun);
            assert_eq!(row["suggested"], candidate.suggested);
        }
        // A game format is suggested and a generic one is offered and not.
        let suggested = |ext: &str| {
            told.iter()
                .find(|r| r["ext"] == ext)
                .map(|r| r["suggested"].as_bool().unwrap_or(false))
        };
        assert_eq!(suggested("ytd"), Some(true));
        assert_eq!(suggested("xml"), Some(false));
    }
}
