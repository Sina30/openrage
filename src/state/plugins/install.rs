//! Installing a plugin from a folder or a file: its folder is copied into the plugins directory, where the plugin manager lists it.
//!
//! ⚠ A plugin is somebody else's code, so installing only copies: nothing runs, and the consent prompt a plugin raises before its
//! first run is as it was. Nothing is replaced: a plugin of the same id is a refusal that says so.

use std::path::{Path, PathBuf};

use super::InstalledPlugin;

/// How much a plugin folder may hold; a plugin is a manifest, a module and the few files they name.
const MAX_FILES: usize = 200;
const MAX_BYTES: u64 = 64 << 20;

/// The folder `source` names: itself where it is a folder, else the folder that holds it, so the manifest or the module can be picked.
fn folder_of(source: &Path) -> Result<PathBuf, String> {
    if source.is_dir() {
        return Ok(source.to_path_buf());
    }
    source
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("{} is in no folder", source.display()))
}

/// Whether `id` can name a folder of the plugins directory on every host.
fn names_a_folder(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Copy the plugin that `source` names into `plugins`, in a folder named for its id, and read it from there.
pub fn install_into(plugins: &Path, source: &Path) -> Result<InstalledPlugin, String> {
    let folder = folder_of(source)?;
    if !folder.join("plugin.json").is_file() {
        return Err(format!(
            "{} holds no plugin.json: a plugin is a folder holding plugin.json and plugin.wasm",
            folder.display()
        ));
    }
    let found = InstalledPlugin::read(&folder);
    if let Some(why) = &found.refused {
        return Err(format!("{} cannot be installed: {why}", folder.display()));
    }
    let id = &found.manifest.id;
    if !names_a_folder(id) {
        return Err(format!(
            "the id `{id}` cannot name a folder: letters, digits, `-`, `_` and `.` only"
        ));
    }
    let dest = plugins.join(id);
    if dest.exists() {
        return Err(format!(
            "{} is installed already: delete the folder {} to install another",
            found.manifest.name,
            dest.display()
        ));
    }
    let files = files_of(&folder)?;
    std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    for file in &files {
        let relative = file.strip_prefix(&folder).map_err(|e| e.to_string())?;
        let target = dest.join(relative);
        let copied = target
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::copy(file, &target).map(drop));
        if let Err(e) = copied {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(format!("{}: {e}", file.display()));
        }
    }
    Ok(InstalledPlugin::read(&dest))
}

/// The files under `folder`, without a hidden entry or a link, refusing a folder that is too big to be a plugin.
fn files_of(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let (mut files, mut bytes) = (Vec::new(), 0u64);
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() {
                bytes += entry.metadata().map_err(|e| e.to_string())?.len();
                files.push(entry.path());
            }
            if files.len() > MAX_FILES || bytes > MAX_BYTES {
                return Err(format!(
                    "{} holds more than a plugin does: at most {MAX_FILES} files and {} MB",
                    folder.display(),
                    MAX_BYTES >> 20
                ));
            }
        }
    }
    Ok(files)
}

impl crate::state::AppState {
    /// Install the plugin that `source` (its folder, or its `plugin.json` or `plugin.wasm`) names, and list it; its name.
    pub fn install_plugin(&mut self, source: &Path) -> Result<String, String> {
        let plugins = super::plugins_dir().ok_or("there is no configuration directory")?;
        let installed = install_into(&plugins, source)?;
        self.rescan_plugins();
        Ok(installed.manifest.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plugin folder of a manifest and a module that is no module: installing reads the manifest and copies, and runs nothing.
    fn a_plugin(dir: &Path, id: &str) -> PathBuf {
        let folder = dir.join(format!("source-of-{id}"));
        std::fs::create_dir_all(folder.join("data")).unwrap();
        std::fs::write(
            folder.join("plugin.json"),
            format!(
                r#"{{"id":"{id}","name":"Plugin {id}","abi":{},"wants":["cap.read_file_bytes"]}}"#,
                or_plugin::ABI_VERSION
            ),
        )
        .unwrap();
        std::fs::write(folder.join("plugin.wasm"), b"\0asm").unwrap();
        std::fs::write(folder.join("data/table.json"), b"[]").unwrap();
        folder
    }

    #[test]
    fn a_plugin_installed_from_its_folder_or_from_a_file_of_it_is_copied_and_listed_and_never_replaced(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let plugins = dir.path().join("plugins");
        let hat = a_plugin(dir.path(), "hat-tool");

        let installed = install_into(&plugins, &hat).expect("a folder installs");
        assert_eq!(installed.manifest.id, "hat-tool");
        assert!(installed.refused.is_none(), "{:?}", installed.refused);
        assert!(plugins.join("hat-tool/plugin.wasm").is_file());
        assert!(plugins.join("hat-tool/data/table.json").is_file());
        let listed = super::super::installed_in(&plugins);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].manifest.id, "hat-tool");

        // The same plugin again is refused with a sentence and nothing is touched.
        let again = install_into(&plugins, &hat.join("plugin.json")).unwrap_err();
        assert!(again.contains("installed already"), "{again}");

        // A file of another plugin installs the folder it is in.
        let belt = a_plugin(dir.path(), "belt-tool");
        install_into(&plugins, &belt.join("plugin.wasm")).expect("a file installs its folder");
        assert_eq!(super::super::installed_in(&plugins).len(), 2);
    }

    #[test]
    fn what_is_no_plugin_is_refused_by_name_and_leaves_nothing_in_the_plugins_directory() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = dir.path().join("plugins");

        let empty = dir.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let why = install_into(&plugins, &empty).unwrap_err();
        assert!(why.contains("plugin.json"), "{why}");

        let broken = dir.path().join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("plugin.json"), b"not json").unwrap();
        assert!(install_into(&plugins, &broken).is_err());

        let odd = a_plugin(dir.path(), "x");
        std::fs::write(
            odd.join("plugin.json"),
            format!(
                r#"{{"id":"../up","name":"Odd","abi":{},"wants":["cap.read_file_bytes"]}}"#,
                or_plugin::ABI_VERSION
            ),
        )
        .unwrap();
        let why = install_into(&plugins, &odd).unwrap_err();
        assert!(why.contains("cannot name a folder"), "{why}");
        assert!(!plugins.exists() || std::fs::read_dir(&plugins).unwrap().next().is_none());
    }
}
