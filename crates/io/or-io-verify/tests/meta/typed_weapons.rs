//! The weapon family as typed roots, on documents written here in the shape the game's weapon metadata has: members read by the
//! engine's names, a member the family does not name arriving as it came, an edit that changes one value and nothing else, and a
//! record built from nothing that saves and reads back.
//!
//! ⚠ No game file is quoted: the documents are this file's own, with the names the install's metadata uses. The install is where the
//! family is measured (`the_weapon_metadata_of_the_install_reaches_a_fixed_point` in `or-weapons`), and these hold what a clean checkout can.

use crate::support;
use or_io::decl::File;
use or_io::decl::{Vec2, Vec3};
use or_io::meta::typed::weapon::{
    CAmmoInfo, CWeaponComponentClipInfo, CWeaponComponentInfoBlob, CWeaponInfo, CWeaponInfoBlob,
    CWeaponModelInfoInitData, CWeaponModelInfoInitDataList, ComponentInfoAny, InfoAny,
};

const WEAPONS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>

<CWeaponInfoBlob>
  <SlotNavigateOrder>
    <Item>
      <WeaponSlots>
        <Item>
          <OrderNumber value="10" />
          <Entry>SLOT_UNARMED</Entry>
        </Item>
      </WeaponSlots>
    </Item>
  </SlotNavigateOrder>
  <SlotBestOrder>
    <WeaponSlots />
  </SlotBestOrder>
  <AimingInfos>
    <Item type="CAimingInfo">
      <Name>PISTOL_AIM</Name>
      <HeadingLimit value="90.000000" />
      <SweepPitchMin value="-80.000000" />
      <SweepPitchMax value="42.000000" />
    </Item>
  </AimingInfos>
  <Infos>
    <Item>
      <Infos>
        <Item type="CAmmoInfo">
          <Name>AMMO_PISTOL</Name>
          <Model />
          <Audio />
          <Slot />
          <AmmoMax value="250" />
          <AmmoFlags />
        </Item>
        <Item type="CWeaponInfo">
          <Name>WEAPON_TESTPISTOL</Name>
          <Model>w_pi_testpistol</Model>
          <Audio>AUDIO_ITEM_PISTOL</Audio>
          <Slot>SLOT_PISTOL</Slot>
          <DamageType>BULLET</DamageType>
          <Damage value="26.000000" />
          <ClipSize value="12" />
          <AimOffsetMax x="0.5" y="0.25" z="-0.125" />
          <FlashFxLightRangeMinMax x="0.2.000000" y="1.5" />
          <AimingInfo ref="PISTOL_AIM" />
          <AmmoInfo ref="AMMO_PISTOL" />
          <Explosion>
            <Default value="3" />
            <HitCar>EXP_TAG_BULLET</HitCar>
          </Explosion>
          <OverrideForces>
            <Item>
              <BoneTag>BONETAG_HEAD</BoneTag>
              <ForceBack value="1.000000" />
            </Item>
          </OverrideForces>
          <AttachPoints>
            <Item>
              <AttachBone>WAPClip</AttachBone>
              <Components>
                <Item>
                  <Name>COMPONENT_TESTPISTOL_CLIP_01</Name>
                  <Default value="true" />
                </Item>
                <Item>
                  <Name>COMPONENT_TESTPISTOL_CLIP_02</Name>
                  <Default value="false" />
                </Item>
              </Components>
            </Item>
          </AttachPoints>
        </Item>
      </Infos>
    </Item>
  </Infos>
  <VehicleWeaponInfos />
  <Name>TEST</Name>
</CWeaponInfoBlob>
"#;

const COMPONENTS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CWeaponComponentInfoBlob>
  <Data />
  <Infos>
    <Item type="CWeaponComponentClipInfo">
      <Name>COMPONENT_TESTPISTOL_CLIP_01</Name>
      <Model>w_pi_testpistol_mag1</Model>
      <LocName>WCT_CLIP1</LocName>
      <LocDesc>WCD_CLIP1</LocDesc>
      <AttachBone>WAPClip</AttachBone>
      <ClipSize value="12" />
      <CreateObject value="true" />
      <bShownOnWheel value="true" />
      <AccuracyModifier type="NULL" />
      <DamageModifier type="NULL" />
      <ReloadData ref="RELOAD_PISTOL" />
    </Item>
  </Infos>
  <InfoBlobName>TEST</InfoBlobName>
</CWeaponComponentInfoBlob>
"#;

const ARCHETYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CWeaponModelInfo__InitDataList>
  <InitDatas>
    <Item>
      <modelName>w_pi_testpistol</modelName>
      <txdName>w_pi_testpistol</txdName>
      <ptfxAssetName>weap_xs_testpistol</ptfxAssetName>
      <lodDist value="200" />
    </Item>
  </InitDatas>
</CWeaponModelInfo__InitDataList>
"#;

fn weapon(blob: &CWeaponInfoBlob) -> &CWeaponInfo {
    blob.infos
        .iter()
        .flat_map(|set| &set.infos)
        .find_map(|info| match info {
            InfoAny::CWeaponInfo(weapon) => Some(weapon),
            _ => None,
        })
        .expect("the document holds a weapon")
}

fn weapon_mut(blob: &mut CWeaponInfoBlob) -> &mut CWeaponInfo {
    blob.infos
        .iter_mut()
        .flat_map(|set| &mut set.infos)
        .find_map(|info| match info {
            InfoAny::CWeaponInfo(weapon) => Some(weapon),
            _ => None,
        })
        .expect("the document holds a weapon")
}

/// The lines of an XML text, trimmed, so two texts are compared line by line.
fn lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

#[test]
fn a_weapon_file_reads_by_the_engines_names() {
    let blob = CWeaponInfoBlob::from_bytes(WEAPONS.as_bytes()).expect("the weapon file opens");
    assert_eq!(blob.name, "TEST");
    assert_eq!(blob.aiming_infos.len(), 1);
    assert_eq!(blob.aiming_infos[0].name, "PISTOL_AIM");
    assert_eq!(blob.aiming_infos[0].sweep_pitch_min, -80.0);
    assert_eq!(blob.slot_navigate_order[0].weapon_slots[0].order_number, 10);
    assert_eq!(
        blob.slot_navigate_order[0].weapon_slots[0].entry,
        "SLOT_UNARMED"
    );

    let pistol = weapon(&blob);
    assert_eq!(pistol.name, "WEAPON_TESTPISTOL");
    assert_eq!(pistol.model, "w_pi_testpistol");
    assert_eq!(pistol.slot, "SLOT_PISTOL");
    assert_eq!(pistol.damage_type, "BULLET");
    assert_eq!(pistol.damage, 26.0);
    assert_eq!(pistol.clip_size, 12);
    assert_eq!(
        pistol.aim_offset_max,
        Vec3 {
            x: 0.5,
            y: 0.25,
            z: -0.125
        }
    );
    let points = &pistol.attach_points;
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].attach_bone, "WAPClip");
    let names: Vec<_> = points[0]
        .components
        .iter()
        .map(|c| (&c.name[..], c.default))
        .collect();
    assert_eq!(
        names,
        [
            ("COMPONENT_TESTPISTOL_CLIP_01", true),
            ("COMPONENT_TESTPISTOL_CLIP_02", false)
        ]
    );
    let ammo = blob
        .infos
        .iter()
        .flat_map(|set| &set.infos)
        .find_map(|info| match info {
            InfoAny::CAmmoInfo(ammo) => Some(ammo),
            _ => None,
        })
        .expect("the document holds ammunition");
    assert_eq!((ammo.name.as_str(), ammo.ammo_max), ("AMMO_PISTOL", 250));
    let _: &CAmmoInfo = ammo;
}

#[test]
fn what_no_member_names_arrives_as_it_came() {
    let blob = CWeaponInfoBlob::from_bytes(WEAPONS.as_bytes()).unwrap();
    let saved = blob.to_bytes().unwrap();
    let text = String::from_utf8(saved).unwrap();
    // A reference to a named item, a null pointer, a struct whose members the family does not name, a malformed vector.
    for carried in [
        r#"<AimingInfo ref="PISTOL_AIM" />"#,
        r#"<AmmoInfo ref="AMMO_PISTOL" />"#,
        "<Default value=\"3\"/>",
        "<HitCar>EXP_TAG_BULLET</HitCar>",
        "<BoneTag>BONETAG_HEAD</BoneTag>",
        "<ForceBack value=\"1.000000\"/>",
        "<FlashFxLightRangeMinMax x=\"0.2.000000\" y=\"1.5\"/>",
    ] {
        assert!(
            text.replace(" />", "/>")
                .contains(&carried.replace(" />", "/>")),
            "`{carried}` did not arrive: {text}"
        );
    }
    let components = CWeaponComponentInfoBlob::from_bytes(COMPONENTS.as_bytes()).unwrap();
    let saved = String::from_utf8(components.to_bytes().unwrap()).unwrap();
    for carried in [
        "<AccuracyModifier type=\"NULL\"/>",
        "<DamageModifier type=\"NULL\"/>",
        "<ReloadData ref=\"RELOAD_PISTOL\"/>",
    ] {
        assert!(
            saved.contains(carried),
            "`{carried}` did not arrive: {saved}"
        );
    }
}

#[test]
fn an_untouched_save_is_a_fixed_point_and_keeps_every_member() {
    for (what, bytes) in [
        ("weapons", WEAPONS),
        ("components", COMPONENTS),
        ("archetypes", ARCHETYPES),
    ] {
        let first = match what {
            "weapons" => CWeaponInfoBlob::from_bytes(bytes.as_bytes())
                .unwrap()
                .to_bytes(),
            "components" => CWeaponComponentInfoBlob::from_bytes(bytes.as_bytes())
                .unwrap()
                .to_bytes(),
            _ => CWeaponModelInfoInitDataList::from_bytes(bytes.as_bytes())
                .unwrap()
                .to_bytes(),
        }
        .unwrap();
        let second = match what {
            "weapons" => CWeaponInfoBlob::from_bytes(&first).unwrap().to_bytes(),
            "components" => CWeaponComponentInfoBlob::from_bytes(&first)
                .unwrap()
                .to_bytes(),
            _ => CWeaponModelInfoInitDataList::from_bytes(&first)
                .unwrap()
                .to_bytes(),
        }
        .unwrap();
        assert_eq!(
            first, second,
            "{what}: saving the canonical text again moves it"
        );
    }
}

#[test]
fn an_edit_changes_one_value_and_saves_the_rest_as_it_was() {
    let mut blob = CWeaponInfoBlob::from_bytes(WEAPONS.as_bytes()).unwrap();
    let before = blob.to_bytes().unwrap();
    weapon_mut(&mut blob).damage = 40.5;
    let after = blob.to_bytes().unwrap();
    let (was, is) = (lines(&before), lines(&after));
    assert_eq!(was.len(), is.len());
    let changed: Vec<_> = was.iter().zip(&is).filter(|(a, b)| a != b).collect();
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert!(changed[0].1.contains("40.5"), "{changed:?}");

    let reopened = CWeaponInfoBlob::from_bytes(&after).unwrap();
    assert_eq!(weapon(&reopened).damage, 40.5);
    assert_eq!(weapon(&reopened).clip_size, 12);
}

#[test]
fn a_member_the_file_does_not_state_is_stated_when_it_is_set() {
    let mut blob = CWeaponInfoBlob::from_bytes(WEAPONS.as_bytes()).unwrap();
    assert_eq!(weapon(&blob).endurance_damage, 0.0);
    weapon_mut(&mut blob).endurance_damage = 12.0;
    weapon_mut(&mut blob).fx.flash_fx_light_falloff_min_max = Vec2 { x: 1.0, y: 2.0 };
    let saved = blob.to_bytes().unwrap();
    assert_eq!(
        weapon(&CWeaponInfoBlob::from_bytes(&saved).unwrap()).endurance_damage,
        12.0
    );
    assert!(String::from_utf8(saved)
        .unwrap()
        .contains("EnduranceDamage"));
}

#[test]
fn a_list_grows_by_a_record_built_here_and_a_weapon_built_from_nothing_saves_and_reads_back() {
    let mut blob = CWeaponInfoBlob::from_bytes(WEAPONS.as_bytes()).unwrap();
    let mut made = CWeaponInfo::default();
    made.name = "WEAPON_MADEHERE".to_string();
    made.model = "w_pi_madehere".to_string();
    made.clip_size = 8;
    blob.infos[0].infos.push(InfoAny::CWeaponInfo(made));
    let saved = blob.to_bytes().unwrap();
    let reopened = CWeaponInfoBlob::from_bytes(&saved).unwrap();
    let names: Vec<_> = reopened
        .infos
        .iter()
        .flat_map(|set| &set.infos)
        .filter_map(|info| match info {
            InfoAny::CWeaponInfo(w) => Some(w.name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["WEAPON_TESTPISTOL", "WEAPON_MADEHERE"]);
    let made = reopened
        .infos
        .iter()
        .flat_map(|set| &set.infos)
        .find_map(|info| match info {
            InfoAny::CWeaponInfo(w) if w.name == "WEAPON_MADEHERE" => Some(w),
            _ => None,
        })
        .unwrap();
    assert_eq!((made.model.as_str(), made.clip_size), ("w_pi_madehere", 8));
    // Saving what was just read gives the same text, so the built record reached a fixed point too.
    assert_eq!(reopened.to_bytes().unwrap(), saved);

    let nothing = CWeaponInfoBlob::default();
    let text = nothing.to_bytes().unwrap();
    assert_eq!(CWeaponInfoBlob::from_bytes(&text).unwrap().infos.len(), 0);
}

#[test]
fn components_and_archetypes_read_by_their_names_and_a_clip_is_a_component() {
    let components = CWeaponComponentInfoBlob::from_bytes(COMPONENTS.as_bytes()).unwrap();
    assert_eq!(components.info_blob_name, "TEST");
    let ComponentInfoAny::CWeaponComponentClipInfo(clip) = &components.infos[0] else {
        panic!("the component is a clip");
    };
    let _: &CWeaponComponentClipInfo = clip;
    assert_eq!(clip.name, "COMPONENT_TESTPISTOL_CLIP_01");
    assert_eq!(clip.attach_bone, "WAPClip");
    assert_eq!(clip.clip_size, 12);
    assert!(clip.b_shown_on_wheel);

    let archetypes = CWeaponModelInfoInitDataList::from_bytes(ARCHETYPES.as_bytes()).unwrap();
    let data: &CWeaponModelInfoInitData = &archetypes.init_datas[0];
    assert_eq!(data.model_name, "w_pi_testpistol");
    assert_eq!(data.txd_name, "w_pi_testpistol");
    assert_eq!(data.lod_dist, 200);
}

/// ⚠ A developer's machine has the schema snapshot installed, and a user's has none: the family's declaration is what both read with.
#[test]
fn a_staged_schema_snapshot_changes_nothing_a_weapon_file_reads_or_saves() {
    let Some(snapshot) = support::schema_or_skip(
        "a_staged_schema_snapshot_changes_nothing_a_weapon_file_reads_or_saves",
    ) else {
        return;
    };
    assert!(
        snapshot.structs.contains_key("CWeaponInfoBlob"),
        "the staged snapshot declares the weapon blob, so it could change what is read"
    );
    for bytes in [WEAPONS, COMPONENTS, ARCHETYPES] {
        let family_alone = or_io::meta::MetaDocument::parse(
            or_io::meta::typed::weapon_types::declared(),
            bytes.as_bytes().to_vec(),
        )
        .unwrap()
        .to_bytes()
        .unwrap();
        let typed = match bytes {
            WEAPONS => CWeaponInfoBlob::from_bytes(bytes.as_bytes())
                .unwrap()
                .to_bytes(),
            COMPONENTS => CWeaponComponentInfoBlob::from_bytes(bytes.as_bytes())
                .unwrap()
                .to_bytes(),
            _ => CWeaponModelInfoInitDataList::from_bytes(bytes.as_bytes())
                .unwrap()
                .to_bytes(),
        }
        .unwrap();
        assert_eq!(typed, family_alone);
    }
}

#[test]
fn a_document_that_is_not_the_root_asked_for_is_refused_by_name() {
    let error = CWeaponInfoBlob::from_bytes(ARCHETYPES.as_bytes())
        .unwrap_err()
        .to_string();
    assert!(error.contains("CWeaponInfoBlob"), "{error}");
    let error = CWeaponComponentInfoBlob::from_bytes(b"not xml at all")
        .unwrap_err()
        .to_string();
    assert!(!error.is_empty());
}
