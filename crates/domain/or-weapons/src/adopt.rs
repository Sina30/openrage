//! The kind of content that is weapons: how a project hands a build what a module holds, how a folder of weapons is recognised
//! when a project adopts it, and how an empty module is started.
//!
//! A folder is recognised by its `weapons.json`, and a folder without one is not weapons however many `.meta` files it holds: the
//! engine ships a data file as it is, and a model's name settles nothing about which weapon it is.

use std::path::{Path, PathBuf};

use or_project::adopt::{Recognised, Recogniser};
use or_project::content::Content;
use or_project::{Contribution, Module, ModuleKind};

use crate::model::{Weapons, FILE};
use crate::plan::{contribution, module_folder};

/// The id of the weapons content an adoption makes.
const ID: &str = "weapons";

/// The kind of content that is weapons.
pub struct WeaponPack;

impl Content for WeaponPack {
    fn contribution(&self, project_folder: &Path, module: &Module) -> Contribution {
        contribution(project_folder, module)
    }

    fn start(&self, project_folder: &Path, module: &Module, _name: &str) -> Result<(), String> {
        start_module(project_folder, module)
    }
}

impl Recogniser for WeaponPack {
    fn kind(&self) -> ModuleKind {
        ModuleKind::Weapons
    }

    fn holds(&self, folder: &Path) -> bool {
        folder.join(FILE).is_file()
    }

    fn recognise(&self, folder: &Path, files: &[PathBuf]) -> Result<Option<Recognised>, String> {
        if !self.holds(folder) {
            return Ok(None);
        }
        let model = Weapons::read(folder).map_err(|e| e.to_string())?;
        let sources: Vec<PathBuf> = model
            .weapons
            .iter()
            .flat_map(|w| &w.sources)
            .map(|s| folder.join(s.replace('/', std::path::MAIN_SEPARATOR_STR)))
            .collect();
        // The file of the model, and everything inside a source it names.
        let claimed: Vec<PathBuf> = files
            .iter()
            .filter(|file| {
                **file == folder.join(FILE) || sources.iter().any(|s| file.starts_with(s))
            })
            .cloned()
            .collect();
        let names: Vec<&str> = model.weapons.iter().map(|w| w.name.as_str()).collect();
        let summary = format!("{} weapon(s): {}", model.weapons.len(), names.join(", "));
        Ok(Some(Recognised {
            module: Module::weapons(ID, "."),
            summary,
            claimed,
            unplaced: Vec::new(),
            writes: Vec::new(),
        }))
    }
}

/// Make the folder of the weapons module `module`: the folder, and an empty `weapons.json` in it unless it has one.
pub fn start_module(project_folder: &Path, module: &Module) -> Result<(), String> {
    let folder = module_folder(project_folder, module).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    if folder.join(FILE).exists() {
        return Ok(());
    }
    Weapons::new().write(&folder).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Weapon;

    #[test]
    fn an_empty_module_is_started_once_and_a_folder_with_a_model_is_recognised_by_it() {
        let dir = tempfile::tempdir().unwrap();
        let module = Module::weapons("arsenal", "arsenal");
        start_module(dir.path(), &module).unwrap();
        let folder = dir.path().join("arsenal");
        assert!(WeaponPack.holds(&folder));
        assert_eq!(Weapons::read(&folder).unwrap(), Weapons::new());

        let mut model = Weapons::new();
        let mut weapon = Weapon::new("WEAPON_X");
        weapon.sources.push("x.metadata.src".into());
        model.weapons.push(weapon);
        model.write(&folder).unwrap();
        start_module(dir.path(), &module).unwrap();
        assert_eq!(
            Weapons::read(&folder).unwrap(),
            model,
            "a started module keeps what is there"
        );

        let inside = folder.join("x.metadata.src").join("metadata.xml");
        let outside = folder.join("notes.txt");
        let files = [folder.join(FILE), inside.clone(), outside.clone()];
        let found = WeaponPack.recognise(&folder, &files).unwrap().unwrap();
        assert_eq!(found.module.kind, ModuleKind::Weapons);
        assert!(found.claimed.contains(&inside) && !found.claimed.contains(&outside));
        assert!(WeaponPack.recognise(dir.path(), &[]).unwrap().is_none());
    }
}
