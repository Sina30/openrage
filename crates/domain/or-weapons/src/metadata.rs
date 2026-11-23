//! What the weapon metadata of a pack defines: the weapons, the components they attach, and the model and texture dictionary each is drawn with.
//!
//! The metadata is read through the typed family of `or_io::meta::typed::weapon`, whose names are the engine's. This reads the
//! members the rules and the assembled weapon ask for, and writes none: an edit of a member goes through the family
//! ([`crate::edit`]) so that what no member names arrives as it came.

use or_io::decl::File;
use or_io::meta::typed::weapon::{
    root_of, CWeaponComponentInfoBlob, CWeaponInfoBlob, CWeaponModelInfoInitDataList,
    ComponentInfoAny, InfoAny,
};

use crate::layout::data_file_type;

/// One attachment point of a weapon: the bone, and the components that attach there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachPoint {
    pub bone: String,
    /// The names of the components, with whether the weapon wears it by default.
    pub components: Vec<(String, bool)>,
}

/// A weapon as its metadata defines it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinedWeapon {
    pub name: String,
    pub model: String,
    pub audio: String,
    pub slot: String,
    /// The group the weapon is listed under, such as `GROUP_PISTOL`.
    pub group: String,
    pub attach_points: Vec<AttachPoint>,
}

/// A component as its metadata defines it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinedComponent {
    pub name: String,
    pub model: String,
    pub attach_bone: String,
    /// The engine's name of the kind of component it is: `CWeaponComponentClipInfo`.
    pub kind: &'static str,
}

/// The model and texture dictionary an archetype draws a weapon with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinedArchetype {
    pub model_name: String,
    pub txd_name: String,
}

/// What one metadata document defines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Defined {
    /// The root the document states.
    pub root: String,
    pub weapons: Vec<DefinedWeapon>,
    /// The names of the ammunition it defines.
    pub ammo: Vec<String>,
    pub components: Vec<DefinedComponent>,
    pub archetypes: Vec<DefinedArchetype>,
}

impl Defined {
    /// The type the game is told the document is, none for a document this content does not write.
    pub fn data_file_type(&self) -> Option<&'static str> {
        data_file_type(&self.root)
    }
}

/// Read the metadata document `bytes`, or say why it cannot be.
pub fn read(bytes: &[u8]) -> Result<Defined, String> {
    let root = root_of(bytes).ok_or("it is not an XML document")?;
    let mut defined = Defined {
        root: root.clone(),
        ..Defined::default()
    };
    match data_file_type(&root) {
        Some("WEAPONINFO_FILE") => {
            let blob = CWeaponInfoBlob::from_bytes(bytes).map_err(|e| e.to_string())?;
            for info in blob.infos.iter().flat_map(|group| &group.infos) {
                match info {
                    InfoAny::CWeaponInfo(weapon) => defined.weapons.push(DefinedWeapon {
                        name: weapon.name.clone(),
                        model: weapon.model.clone(),
                        audio: weapon.audio.clone(),
                        slot: weapon.slot.clone(),
                        group: weapon.group.clone(),
                        attach_points: weapon
                            .attach_points
                            .iter()
                            .map(|point| AttachPoint {
                                bone: point.attach_bone.clone(),
                                components: point
                                    .components
                                    .iter()
                                    .map(|c| (c.name.clone(), c.default))
                                    .collect(),
                            })
                            .collect(),
                    }),
                    InfoAny::CAmmoInfo(a) => defined.ammo.push(a.name.clone()),
                    InfoAny::CAmmoProjectileInfo(a) => defined.ammo.push(a.name.clone()),
                    InfoAny::CAmmoRocketInfo(a) => defined.ammo.push(a.name.clone()),
                    InfoAny::CAmmoThrownInfo(a) => defined.ammo.push(a.name.clone()),
                }
            }
        }
        Some("WEAPONCOMPONENTSINFO_FILE") => {
            let blob = CWeaponComponentInfoBlob::from_bytes(bytes).map_err(|e| e.to_string())?;
            for info in &blob.infos {
                let (kind, name, model, attach_bone) = match info {
                    ComponentInfoAny::CWeaponComponentInfo(c) => {
                        ("CWeaponComponentInfo", &c.name, &c.model, &c.attach_bone)
                    }
                    ComponentInfoAny::CWeaponComponentClipInfo(c) => (
                        "CWeaponComponentClipInfo",
                        &c.name,
                        &c.model,
                        &c.attach_bone,
                    ),
                    ComponentInfoAny::CWeaponComponentFlashLightInfo(c) => (
                        "CWeaponComponentFlashLightInfo",
                        &c.name,
                        &c.model,
                        &c.attach_bone,
                    ),
                    ComponentInfoAny::CWeaponComponentScopeInfo(c) => (
                        "CWeaponComponentScopeInfo",
                        &c.name,
                        &c.model,
                        &c.attach_bone,
                    ),
                    ComponentInfoAny::CWeaponComponentSuppressorInfo(c) => (
                        "CWeaponComponentSuppressorInfo",
                        &c.name,
                        &c.model,
                        &c.attach_bone,
                    ),
                    ComponentInfoAny::CWeaponComponentVariantModelInfo(c) => (
                        "CWeaponComponentVariantModelInfo",
                        &c.name,
                        &c.model,
                        &c.attach_bone,
                    ),
                };
                defined.components.push(DefinedComponent {
                    name: name.clone(),
                    model: model.clone(),
                    attach_bone: attach_bone.clone(),
                    kind,
                });
            }
        }
        Some("WEAPON_METADATA_FILE") => {
            let list =
                CWeaponModelInfoInitDataList::from_bytes(bytes).map_err(|e| e.to_string())?;
            defined.archetypes = list
                .init_datas
                .iter()
                .map(|data| DefinedArchetype {
                    model_name: data.model_name.clone(),
                    txd_name: data.txd_name.clone(),
                })
                .collect();
        }
        _ => {}
    }
    Ok(defined)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEAPONS: &str = r#"<CWeaponInfoBlob>
  <Infos>
    <Item>
      <Infos>
        <Item type="CAmmoInfo"><Name>AMMO_X</Name></Item>
        <Item type="CWeaponInfo">
          <Name>WEAPON_X</Name>
          <Model>w_pi_x</Model>
          <Slot>SLOT_PISTOL</Slot>
          <AttachPoints>
            <Item>
              <AttachBone>WAPClip</AttachBone>
              <Components>
                <Item><Name>COMPONENT_X_CLIP_01</Name><Default value="true" /></Item>
              </Components>
            </Item>
          </AttachPoints>
        </Item>
      </Infos>
    </Item>
  </Infos>
  <Name>X</Name>
</CWeaponInfoBlob>"#;

    #[test]
    fn a_weapon_file_defines_its_weapons_ammunition_and_attachment_points() {
        let defined = read(WEAPONS.as_bytes()).unwrap();
        assert_eq!(defined.data_file_type(), Some("WEAPONINFO_FILE"));
        assert_eq!(defined.ammo, ["AMMO_X"]);
        let weapon = &defined.weapons[0];
        assert_eq!(
            (weapon.name.as_str(), weapon.model.as_str()),
            ("WEAPON_X", "w_pi_x")
        );
        assert_eq!(weapon.attach_points[0].bone, "WAPClip");
        assert_eq!(
            weapon.attach_points[0].components,
            [("COMPONENT_X_CLIP_01".to_string(), true)]
        );
    }

    #[test]
    fn a_document_of_another_root_defines_nothing_and_is_not_refused() {
        let defined = read(b"<CPickupDataManager><x/></CPickupDataManager>").unwrap();
        assert_eq!(defined.root, "CPickupDataManager");
        assert_eq!(defined.data_file_type(), None);
        assert!(defined.weapons.is_empty());
        assert!(read(b"not xml").is_err());
    }
}
