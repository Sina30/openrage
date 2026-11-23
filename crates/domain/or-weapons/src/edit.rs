//! An edit of a weapon's metadata through the typed family: the document of a metadata source is opened as its root, the weapon found
//! by its name is changed, and the canonical text is written back, so that what no member names arrives as it came.
//!
//! An edit returns the text before and the text after, so the caller can keep one Undo step: restoring the text before is the undo.

use std::path::{Path, PathBuf};

use or_io::decl::File;
use or_io::meta::typed::weapon::{
    CWeaponComponentInfoBlob, CWeaponInfo, CWeaponInfoBlob, ComponentInfoAny, InfoAny,
};

/// The file of a metadata source the XML is in.
fn xml_of(source: &Path) -> PathBuf {
    source.join(or_source::metadata::XML)
}

/// What an edit changed in a file: its text before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edited {
    pub file: PathBuf,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

impl Edited {
    /// Whether the edit changed anything a save would write.
    pub fn changed(&self) -> bool {
        self.before != self.after
    }
}

fn write(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = file.with_extension("xml.new");
    std::fs::write(&temp, bytes).map_err(|e| format!("{}: {e}", temp.display()))?;
    std::fs::rename(&temp, file).map_err(|e| format!("{}: {e}", file.display()))
}

/// Put `bytes` back as the XML of the metadata source `source`: an Undo, or a Redo.
pub fn restore(source: &Path, bytes: &[u8]) -> Result<(), String> {
    write(&xml_of(source), bytes)
}

/// Change the weapon called `name` in the metadata source `source` with `change`, and save the canonical text.
///
/// Nothing is written when `change` leaves the weapon as it was, and nothing when the weapon is not defined in the document.
pub fn edit_weapon(
    source: &Path,
    name: &str,
    change: impl FnOnce(&mut CWeaponInfo),
) -> Result<Edited, String> {
    let file = xml_of(source);
    let before = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut blob = CWeaponInfoBlob::from_bytes(&before).map_err(|e| e.to_string())?;
    let weapon = blob
        .infos
        .iter_mut()
        .flat_map(|group| &mut group.infos)
        .find_map(|info| match info {
            InfoAny::CWeaponInfo(weapon) if weapon.name.eq_ignore_ascii_case(name) => Some(weapon),
            _ => None,
        })
        .ok_or_else(|| format!("the document defines no weapon called `{name}`"))?;
    change(weapon);
    let after = blob.to_bytes().map_err(|e| e.to_string())?;
    // ⚠ The text the document is in before the first edit is the game's, and the first save is the canonical form of it: only a real change writes.
    let untouched = CWeaponInfoBlob::from_bytes(&before)
        .and_then(|blob| blob.to_bytes())
        .map_err(|e| e.to_string())?;
    if after == untouched {
        return Ok(Edited {
            file,
            after: before.clone(),
            before,
        });
    }
    write(&file, &after)?;
    Ok(Edited {
        file,
        before,
        after,
    })
}

/// Change the component called `name` in the components source `source` with `change`, and save the canonical text.
pub fn edit_component(
    source: &Path,
    name: &str,
    change: impl FnOnce(&mut ComponentInfoAny),
) -> Result<Edited, String> {
    let file = xml_of(source);
    let before = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut blob = CWeaponComponentInfoBlob::from_bytes(&before).map_err(|e| e.to_string())?;
    let component = blob
        .infos
        .iter_mut()
        .find(|c| component_name(c).eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("the document defines no component called `{name}`"))?;
    change(component);
    let after = blob.to_bytes().map_err(|e| e.to_string())?;
    let untouched = CWeaponComponentInfoBlob::from_bytes(&before)
        .and_then(|blob| blob.to_bytes())
        .map_err(|e| e.to_string())?;
    if after == untouched {
        return Ok(Edited {
            file,
            after: before.clone(),
            before,
        });
    }
    write(&file, &after)?;
    Ok(Edited {
        file,
        before,
        after,
    })
}

/// The name of a component of a components file, whichever kind it is.
pub fn component_name(component: &ComponentInfoAny) -> &str {
    match component {
        ComponentInfoAny::CWeaponComponentInfo(c) => &c.name,
        ComponentInfoAny::CWeaponComponentClipInfo(c) => &c.name,
        ComponentInfoAny::CWeaponComponentFlashLightInfo(c) => &c.name,
        ComponentInfoAny::CWeaponComponentScopeInfo(c) => &c.name,
        ComponentInfoAny::CWeaponComponentSuppressorInfo(c) => &c.name,
        ComponentInfoAny::CWeaponComponentVariantModelInfo(c) => &c.name,
    }
}

/// A new weapon called `name` in the metadata source `source`, a `CWeaponInfo` with every member its declaration names and its own model name `model`.
pub fn add_weapon(source: &Path, name: &str, model: &str) -> Result<Edited, String> {
    let file = xml_of(source);
    let before = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut blob = CWeaponInfoBlob::from_bytes(&before).map_err(|e| e.to_string())?;
    let exists =
        blob.infos.iter().flat_map(|g| &g.infos).any(
            |info| matches!(info, InfoAny::CWeaponInfo(w) if w.name.eq_ignore_ascii_case(name)),
        );
    if exists {
        return Err(format!(
            "the document already defines a weapon called `{name}`"
        ));
    }
    let mut weapon = CWeaponInfo::default();
    weapon.name = name.to_string();
    weapon.model = model.to_string();
    match blob.infos.first_mut() {
        Some(group) => group.infos.push(InfoAny::CWeaponInfo(weapon)),
        None => {
            let mut group = or_io::meta::typed::weapon::CWeaponInfoBlobInfos::default();
            group.infos.push(InfoAny::CWeaponInfo(weapon));
            blob.infos.push(group);
        }
    }
    let after = blob.to_bytes().map_err(|e| e.to_string())?;
    write(&file, &after)?;
    Ok(Edited {
        file,
        before,
        after,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<CWeaponInfoBlob>
  <Infos>
    <Item>
      <Infos>
        <Item type="CWeaponInfo">
          <Name>WEAPON_X</Name>
          <Damage value="26.0" />
          <ClipSize value="12" />
          <AimingInfo ref="PISTOL_AIM" />
        </Item>
      </Infos>
    </Item>
  </Infos>
  <Name>X</Name>
</CWeaponInfoBlob>"#;

    fn source(dir: &Path) -> PathBuf {
        let source = dir.join("x.metadata.src");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(xml_of(&source), DOC).unwrap();
        source
    }

    #[test]
    fn an_edit_writes_one_changed_value_and_undo_puts_the_text_back() {
        let dir = tempfile::tempdir().unwrap();
        let source = source(dir.path());
        let edited = edit_weapon(&source, "weapon_x", |w| w.damage = 31.5).unwrap();
        assert!(edited.changed());
        let text = std::fs::read_to_string(xml_of(&source)).unwrap();
        assert!(text.contains("31.5"), "{text}");
        assert!(
            text.contains(r#"ref="PISTOL_AIM""#),
            "the reference arrived: {text}"
        );

        let reopened = CWeaponInfoBlob::from_bytes(text.as_bytes()).unwrap();
        let InfoAny::CWeaponInfo(w) = &reopened.infos[0].infos[0] else {
            panic!("a weapon")
        };
        assert_eq!((w.damage, w.clip_size), (31.5, 12));

        restore(&source, &edited.before).unwrap();
        assert_eq!(std::fs::read_to_string(xml_of(&source)).unwrap(), DOC);
    }

    #[test]
    fn an_edit_that_changes_nothing_writes_nothing_and_a_missing_weapon_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let source = source(dir.path());
        let edited = edit_weapon(&source, "WEAPON_X", |w| w.clip_size = 12).unwrap();
        assert!(!edited.changed());
        assert_eq!(std::fs::read_to_string(xml_of(&source)).unwrap(), DOC);
        assert!(edit_weapon(&source, "WEAPON_NOPE", |_| {}).is_err());
    }

    #[test]
    fn a_weapon_is_added_to_a_document_once() {
        let dir = tempfile::tempdir().unwrap();
        let source = source(dir.path());
        let added = add_weapon(&source, "WEAPON_Y", "w_pi_y").unwrap();
        assert!(added.changed());
        let blob = CWeaponInfoBlob::from_bytes(&std::fs::read(xml_of(&source)).unwrap()).unwrap();
        let names: Vec<_> = blob
            .infos
            .iter()
            .flat_map(|g| &g.infos)
            .filter_map(|i| match i {
                InfoAny::CWeaponInfo(w) => Some(w.name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["WEAPON_X", "WEAPON_Y"]);
        assert!(add_weapon(&source, "weapon_y", "x").is_err());
    }
}
