use super::*;

const SETUP2: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?>
<SSetupData>
  <deviceName>dlc_mpChristmas3</deviceName>
  <datFile>content.xml</datFile>
  <timeStamp>02/10/2022 12:00:00</timeStamp>
  <nameHash>mpchristmas3</nameHash>
  <contentChangeSets />
  <type>EXTRACONTENT_COMPAT_PACK</type>
  <order value="24" />
  <minorOrder value="0" />
  <isLevelPack value="false" />
  <requiredVersion value="0" />
  <subPackCount value="0" />
</SSetupData>"#;

#[test]
fn dlclist_collects_paths_in_order() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<SMandatoryPacksData>
  <Paths>
    <Item>dlcpacks:/mpbeach/</Item>
    <Item>dlcpacks:/mpchristmas3/</Item>
  </Paths>
</SMandatoryPacksData>"#;
    let list = DlcList::parse(xml).unwrap();
    assert_eq!(
        list.paths,
        vec!["dlcpacks:/mpbeach/", "dlcpacks:/mpchristmas3/"]
    );
}

#[test]
fn extra_title_update_data_maps_devices_to_patch_folders() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<SExtraTitleUpdateData>
  <Mounts>
    <Item type="SExtraTitleUpdateMount">
      <deviceName>dlcMPBeach:/</deviceName>
      <path>update:/dlc_patch/mpBeach/</path>
    </Item>
    <Item type="SExtraTitleUpdateMount">
      <deviceName>dlc_mpHeist3:/</deviceName>
      <path>update:/dlc_patch/mpHeist3/</path>
    </Item>
  </Mounts>
</SExtraTitleUpdateData>"#;
    let etud = ExtraTitleUpdateData::parse(xml).unwrap();
    assert_eq!(etud.mounts.len(), 2);
    assert_eq!(etud.mounts[0].device_name, "dlcMPBeach");
    assert_eq!(etud.mounts[0].path, "update:/dlc_patch/mpBeach/");
    assert_eq!(
        etud.mount_for("DLC_MPHEIST3:/"),
        Some("update:/dlc_patch/mpHeist3/")
    );
    assert_eq!(etud.mount_for("dlc_nonexistent"), None);
}

#[test]
fn setup2_parses_fields() {
    let s = SetupData::parse(SETUP2).unwrap();
    assert_eq!(s.name_hash, "mpchristmas3");
    assert_eq!(s.device_name, "dlc_mpChristmas3");
    assert_eq!(s.dat_file, "content.xml");
    assert_eq!(s.content_type, SetupContentType::CompatPack);
    assert_eq!(s.order, 24);
    assert_eq!(s.minor_order, 0);
    assert!(!s.is_level_pack);
    assert_eq!(s.content_device(), "dlc_mpChristmas3CRC");
}

#[test]
fn setup2_carries_the_change_set_groups_that_decide_what_runs() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<SSetupData>
  <deviceName>dlc_mp2023_01</deviceName>
  <nameHash>MP2023_01</nameHash>
  <contentChangeSetGroups>
    <Item>
      <NameHash>GROUP_STARTUP</NameHash>
      <ContentChangeSets>
        <Item>MP2023_01_AUTOGEN</Item>
        <Item>MP2023_01_UNLOCKS_AUTOGEN</Item>
      </ContentChangeSets>
    </Item>
    <Item>
      <NameHash>GROUP_MAP</NameHash>
      <ContentChangeSets>
        <Item>MP2023_01_MAP_UPDATE</Item>
      </ContentChangeSets>
    </Item>
  </contentChangeSetGroups>
  <type>EXTRACONTENT_COMPAT_PACK</type>
</SSetupData>"#;
    let s = SetupData::parse(xml).unwrap();
    assert_eq!(s.content_change_set_groups.len(), 2);
    assert_eq!(s.content_change_set_groups[0].name_hash, "GROUP_STARTUP");
    assert_eq!(
        names(&s.content_change_set_groups[0]),
        vec!["MP2023_01_AUTOGEN", "MP2023_01_UNLOCKS_AUTOGEN"]
    );
    assert_eq!(
        names(&s.content_change_set_groups[1]),
        vec!["MP2023_01_MAP_UPDATE"]
    );
}

fn names(group: &ContentChangeSetGroup) -> Vec<&str> {
    group
        .content_change_sets
        .iter()
        .map(|c| c.value.as_str())
        .collect()
}

/// ⚠ An empty `<requiredVersion />` states no version, and a writer that
/// reads it as `0` puts a number into a file that did not carry one.
#[test]
fn an_empty_element_is_not_a_zero() {
    let stated = SetupData::parse(SETUP2).unwrap();
    assert_eq!(stated.required_version, Some(0));
    assert!(stated.to_xml().contains("<requiredVersion value=\"0\" />"));

    let empty =
        SetupData::parse(br#"<SSetupData><nameHash>x</nameHash><requiredVersion /></SSetupData>"#)
            .unwrap();
    assert_eq!(empty.required_version, None);
    assert!(empty.to_xml().contains("<requiredVersion />"));

    let absent = SetupData::parse(br#"<SSetupData><nameHash>x</nameHash></SSetupData>"#).unwrap();
    assert_eq!(absent.required_version, None);
}

#[test]
fn content_parses_weapon_datafile() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<CDataFileMgr__ContentsOfDataFileXml>
  <disabledFiles />
  <includedXmlFiles />
  <includedDataFiles />
  <dataFiles>
    <Item>
      <filename>dlc_mpChristmas3CRC:/common/data/ai/weapon_railgunxm3.meta</filename>
      <fileType>WEAPONINFO_FILE</fileType>
      <overlay value="false" />
      <disabled value="true" />
      <persistent value="false" />
    </Item>
  </dataFiles>
  <patchFiles />
</CDataFileMgr__ContentsOfDataFileXml>"#;
    let c = ContentXml::parse(xml).unwrap();
    assert_eq!(c.data_files.len(), 1);
    let df = &c.data_files[0];
    assert_eq!(
        df.filename,
        "dlc_mpChristmas3CRC:/common/data/ai/weapon_railgunxm3.meta"
    );
    assert_eq!(df.file_type, "WEAPONINFO_FILE");
    assert!(!df.overlay);
    assert!(df.disabled);
    assert!(!df.persistent);
}

#[test]
fn content_collects_files_to_enable_from_change_sets() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<CDataFileMgr__ContentsOfDataFileXml>
  <dataFiles>
    <Item>
      <filename>dlc_x:/common/data/ai/weapon_enabled.meta</filename>
      <fileType>WEAPONINFO_FILE</fileType>
      <disabled value="true" />
    </Item>
    <Item>
      <filename>dlc_x:/common/data/ai/weapon_orphan.meta</filename>
      <fileType>WEAPONINFO_FILE</fileType>
      <disabled value="true" />
    </Item>
  </dataFiles>
  <contentChangeSets>
    <Item>
      <changeSetName>MP_DLC_STARTUP</changeSetName>
      <filesToEnable>
        <Item>dlc_x:/common/data/ai/weapon_enabled.meta</Item>
      </filesToEnable>
      <mapChangeSetData>
        <Item>
          <filesToEnable>
            <Item>dlc_x:/common/data/map_only.ymap</Item>
          </filesToEnable>
        </Item>
      </mapChangeSetData>
    </Item>
  </contentChangeSets>
</CDataFileMgr__ContentsOfDataFileXml>"#;
    let c = ContentXml::parse(xml).unwrap();
    assert_eq!(c.data_files.len(), 2);
    assert_eq!(c.content_change_sets.len(), 1);
    assert_eq!(c.content_change_sets[0].name, "MP_DLC_STARTUP");
    assert_eq!(
        c.files_to_enable(),
        vec![
            "dlc_x:/common/data/ai/weapon_enabled.meta",
            "dlc_x:/common/data/map_only.ymap",
        ]
    );
}

#[test]
fn content_captures_included_xml_hrefs() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<CDataFileMgr__ContentsOfDataFileXml>
  <includedXmlFiles>
    <Item>
      <xi:include href="commoncrc:/data/common.meta" xpointer="xpointer(/dataFiles)"/>
    </Item>
    <Item>
      <xi:include href="commoncrc:/data/common_anim.meta" xpointer="xpointer(/dataFiles)"/>
    </Item>
  </includedXmlFiles>
  <includedDataFiles>
    <Item>commoncrc:/data/common_cutscene.meta</Item>
  </includedDataFiles>
  <dataFiles>
    <Item>
      <filename>common:/data/materials/procedural.meta</filename>
      <fileType>PROCEDURAL_METADATA_FILE</fileType>
    </Item>
  </dataFiles>
</CDataFileMgr__ContentsOfDataFileXml>"#;
    let c = ContentXml::parse(xml).unwrap();
    assert_eq!(
        c.included_xml_files,
        vec![
            "commoncrc:/data/common.meta",
            "commoncrc:/data/common_anim.meta"
        ]
    );
    assert_eq!(
        c.included_data_files,
        vec!["commoncrc:/data/common_cutscene.meta"]
    );
    assert_eq!(c.data_files.len(), 1);
    assert_eq!(c.data_files[0].file_type, "PROCEDURAL_METADATA_FILE");
}

#[test]
fn content_parses_bare_datafiles_root() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<dataFiles>
  <Item>
    <filename>platform:/models/cdimages/componentpeds_a_c.rpf</filename>
    <fileType>RPF_FILE</fileType>
    <contents>CONTENTS_PEDS</contents>
  </Item>
</dataFiles>"#;
    let c = ContentXml::parse(xml).unwrap();
    assert_eq!(c.data_files.len(), 1);
    assert_eq!(
        c.data_files[0].filename,
        "platform:/models/cdimages/componentpeds_a_c.rpf"
    );
    assert_eq!(c.data_files[0].contents, "CONTENTS_PEDS");
}

#[test]
fn content_defaults_filetype_to_rpf() {
    let xml = br#"<CDataFileMgr__ContentsOfDataFileXml>
  <dataFiles>
    <Item>
      <filename>dlc_x:/x64/foo.rpf</filename>
    </Item>
  </dataFiles>
</CDataFileMgr__ContentsOfDataFileXml>"#;
    let c = ContentXml::parse(xml).unwrap();
    assert_eq!(c.data_files.len(), 1);
    assert_eq!(c.data_files[0].file_type, "RPF_FILE");
}

#[test]
fn mount_order_matches_engine_rules() {
    let tu = SetupData {
        content_type: SetupContentType::TitleUpdate,
        order: 100,
        ..Default::default()
    };
    let compat_a = SetupData {
        content_type: SetupContentType::CompatPack,
        order: 1,
        name_hash: "a".into(),
        ..Default::default()
    };
    let compat_b = SetupData {
        content_type: SetupContentType::CompatPack,
        order: 2,
        name_hash: "b".into(),
        ..Default::default()
    };
    let mut packs = [compat_b.clone(), tu.clone(), compat_a.clone()];
    packs.sort_by(|x, y| x.mount_cmp(y));
    assert_eq!(packs[0].content_type, SetupContentType::TitleUpdate);
    assert_eq!(packs[1].order, 1);
    assert_eq!(packs[2].order, 2);

    let lvl_low = SetupData {
        content_type: SetupContentType::LevelPack,
        is_level_pack: true,
        order: 1,
        ..Default::default()
    };
    let lvl_high = SetupData {
        content_type: SetupContentType::LevelPack,
        is_level_pack: true,
        order: 5,
        ..Default::default()
    };
    let mut lvls = [lvl_low.clone(), lvl_high.clone()];
    lvls.sort_by(|x, y| x.mount_cmp(y));
    assert_eq!(lvls[0].order, 5);
    assert_eq!(lvls[1].order, 1);
}

/// ⚠ The defect this pair holds is **asymmetric**: a writer that escapes and a
/// reader that drops entities let a value survive being written and die being
/// read back. Every expectation here is the variable that was written, never a
/// transcription of what the escape should look like.
#[test]
fn every_escaped_value_comes_back_as_itself() {
    let cases = [
        "Zapp & Roger",
        "a < b",
        "a > b",
        "quote \" and apos '",
        "&",
        "&amp;",
        "<>&\"'",
        "dlc_a&b:/x64/thing.rpf",
    ];

    let mut checked = 0;
    for value in cases {
        let list = DlcList {
            paths: vec![value.into()],
        };
        let back = DlcList::parse(list.to_xml().as_bytes()).unwrap();
        assert_eq!(back.paths, vec![ListItem::from(value)], "dlclist path");

        let setup = SetupData {
            name_hash: value.to_string(),
            device_name: value.to_string(),
            time_stamp: value.to_string(),
            ..Default::default()
        };
        let back = SetupData::parse(setup.to_xml().as_bytes()).unwrap();
        assert_eq!(back.name_hash, value, "setup2 nameHash");
        assert_eq!(back.device_name, value, "setup2 deviceName");

        let content = ContentXml {
            data_files: vec![DataFileEntry {
                filename: value.to_string(),
                file_type: "RPF_FILE".into(),
                ..Default::default()
            }],
            included_xml_files: vec![value.to_string()],
            ..Default::default()
        };
        let back = ContentXml::parse(content.to_xml().as_bytes()).unwrap();
        assert_eq!(back.data_files[0].filename, value, "content filename");
        assert_eq!(back.included_xml_files, vec![value.to_string()], "href");

        checked += 3;
    }

    assert_eq!(
        checked,
        cases.len() * 3,
        "every case must reach all three documents"
    );
    eprintln!("checked {checked} escaped value round trips across three documents");
}

/// ⚠ Without this the suite above can pass while the writer escapes nothing:
/// a reader that drops entities and a writer that emits none agree perfectly.
#[test]
fn the_writer_really_does_emit_entity_references() {
    let list = DlcList {
        paths: vec!["Zapp & Roger <x>".into()],
    };
    let xml = list.to_xml();
    assert!(
        xml.contains("&amp;"),
        "expected an escaped ampersand: {xml}"
    );
    assert!(xml.contains("&lt;"), "expected an escaped less-than: {xml}");
    assert!(
        xml.contains("&gt;"),
        "expected an escaped greater-than: {xml}"
    );
    assert!(
        !xml.contains("Zapp & Roger"),
        "the raw ampersand must not survive into the document: {xml}"
    );
}

#[test]
fn a_character_reference_resolves_to_its_character() {
    let parsed = DlcList::parse(
        br#"<SMandatoryPacksData><Paths><Item>a&#38;b&#x26;c</Item></Paths></SMandatoryPacksData>"#,
    )
    .unwrap();
    let expected = format!("a{0}b{0}c", '&');
    assert_eq!(parsed.paths, vec![ListItem::from(expected)]);
}

#[test]
fn an_unresolvable_entity_keeps_its_spelling() {
    let parsed = DlcList::parse(
        br#"<SMandatoryPacksData><Paths><Item>a&nbsp;b</Item></Paths></SMandatoryPacksData>"#,
    )
    .unwrap();
    assert_eq!(parsed.paths, vec![ListItem::from("a&nbsp;b")]);
}

/// A value the reader resolves out of an attribute, which is the one place
/// quick-xml leaves the escaping in.
#[test]
fn an_escaped_attribute_value_is_unescaped() {
    let parsed = ContentXml::parse(
        br#"<CDataFileMgr__ContentsOfDataFileXml><dataFiles><Item>
              <filename>x</filename><fileType>RPF_FILE</fileType>
              <contents>a&amp;b</contents>
            </Item></dataFiles>
            <includedXmlFiles><Item><xi:include href="c&amp;d:/x.meta" /></Item></includedXmlFiles>
        </CDataFileMgr__ContentsOfDataFileXml>"#,
    )
    .unwrap();
    assert_eq!(parsed.data_files[0].contents, format!("a{}b", '&'));
    assert_eq!(
        parsed.included_xml_files,
        vec![format!("c{}d:/x.meta", '&')]
    );
}

#[test]
fn a_setup_document_round_trips_through_its_own_writer() {
    let original = SetupData::parse(SETUP2).unwrap();
    let again = SetupData::parse(original.to_xml().as_bytes()).unwrap();
    assert_eq!(original, again);
}

#[test]
fn a_content_document_round_trips_through_its_own_writer() {
    let xml = br#"<CDataFileMgr__ContentsOfDataFileXml>
  <dataFiles>
    <Item>
      <filename>dlc_x:/%PLATFORM%/a.rpf</filename>
      <fileType>RPF_FILE</fileType>
      <overlay value="true" />
      <disabled value="true" />
      <persistent value="true" />
      <contents>CONTENTS_PROPS</contents>
    </Item>
  </dataFiles>
  <contentChangeSets>
    <Item>
      <changeSetName>X_AUTOGEN</changeSetName>
      <mapChangeSetData>
        <Item>
          <associatedMap>MO_JIM_L11</associatedMap>
          <filesToEnable><Item>dlc_x:/%PLATFORM%/a.rpf</Item></filesToEnable>
        </Item>
      </mapChangeSetData>
      <filesToEnable><Item>dlc_x:/%PLATFORM%/a.rpf</Item></filesToEnable>
      <requiresLoadingScreen value="true" />
      <loadingScreenContext>LOADINGSCREEN_CONTEXT_LAST_FRAME</loadingScreenContext>
      <useCacheLoader value="true" />
    </Item>
  </contentChangeSets>
</CDataFileMgr__ContentsOfDataFileXml>"#;
    let original = ContentXml::parse(xml).unwrap();
    let again = ContentXml::parse(original.to_xml().as_bytes()).unwrap();
    assert_eq!(original, again);
    assert_eq!(original.content_change_sets[0].map_change_set_data.len(), 1);
}

/// ⚠ Two `<type>` spellings reach one variant, so a writer working from the
/// variant alone renames the pack.
#[test]
fn a_type_spelling_survives_the_variant_it_maps_to() {
    let s = SetupData::parse(
        br#"<SSetupData><nameHash>x</nameHash><type>EXTRACONTENT_EPISODIC_PACK</type></SSetupData>"#,
    )
    .unwrap();
    assert_eq!(s.content_type, SetupContentType::PaidPack);
    assert!(s.to_xml().contains("EXTRACONTENT_EPISODIC_PACK"));

    let authored = SetupData {
        content_type: SetupContentType::PaidPack,
        ..Default::default()
    };
    assert!(authored
        .to_xml()
        .contains(SetupContentType::PaidPack.canonical_name()));
}

#[test]
fn a_generated_pack_enables_every_file_it_registers() {
    let mut spec = DlcPackSpec::new("mymod");
    spec.files.push(PackFile::new("x64/models/a.rpf", "a.rpf"));
    spec.files
        .push(PackFile::new("common/data/b.ityp", "b.ityp"));

    let content = spec.content();
    assert_eq!(content.data_files.len(), 2);
    assert!(
        content.data_files.iter().all(|d| d.disabled),
        "a shipped pack registers every file disabled"
    );

    let enabled = content.files_to_enable();
    assert_eq!(enabled.len(), content.data_files.len());
    for df in &content.data_files {
        assert!(
            enabled.iter().any(|e| e.value == df.filename),
            "{} is registered and never enabled, so the pack would do nothing",
            df.filename
        );
    }

    // The group is what makes the change set run at all.
    let setup = spec.setup();
    let referenced: Vec<&str> = setup
        .content_change_set_groups
        .iter()
        .flat_map(|g| g.content_change_sets.iter())
        .map(|c| c.value.as_str())
        .collect();
    assert!(
        referenced.contains(&content.content_change_sets[0].name.as_str()),
        "the startup group must name the change set: {referenced:?}"
    );
}

#[test]
fn a_generated_pack_writes_its_paths_against_the_device_the_mount_registers() {
    let mut spec = DlcPackSpec::new("mymod");
    spec.files.push(PackFile::new("x64/models/a.rpf", "a.rpf"));

    // minorOrder 0 means the CRC device, which is what `content_device` says.
    assert_eq!(spec.content_device(), "dlc_mymodCRC");
    assert!(spec.content().data_files[0]
        .filename
        .starts_with("dlc_mymodCRC:/"));

    spec.minor_order = 1;
    assert_eq!(spec.content_device(), "dlc_mymod");
    assert!(spec.content().data_files[0]
        .filename
        .starts_with("dlc_mymod:/"));
}

#[test]
fn a_generated_pack_round_trips_through_the_parsers() {
    let mut spec = DlcPackSpec::new("mymod");
    spec.files.push(PackFile::new("x64/models/a.rpf", "a.rpf"));
    let docs = spec.documents();

    assert_eq!(
        SetupData::parse(docs.setup2.as_bytes()).unwrap(),
        spec.setup()
    );
    assert_eq!(
        ContentXml::parse(docs.content.as_bytes()).unwrap(),
        spec.content()
    );
    assert_eq!(docs.dlclist_entry, "dlcpacks:/mymod/");
}

#[test]
fn a_pack_that_would_not_load_is_refused_with_its_reason() {
    let empty = DlcPackSpec::new("mymod");
    assert!(empty
        .problems()
        .iter()
        .any(|p| p.contains("registers nothing")));

    let mut bad_name = DlcPackSpec::new("my mod!");
    bad_name.files.push(PackFile::new("x64/a.rpf", file!()));
    assert!(bad_name
        .problems()
        .iter()
        .any(|p| p.contains("device name")));

    let mut twice = DlcPackSpec::new("mymod");
    twice.files.push(PackFile::new("x64/a.rpf", file!()));
    twice.files.push(PackFile::new("X64/A.RPF", file!()));
    assert!(twice.problems().iter().any(|p| p.contains("added twice")));
}

#[test]
fn a_files_type_follows_its_extension() {
    assert_eq!(file_type_for("x64/models/a.rpf"), Some("RPF_FILE"));
    assert_eq!(
        file_type_for("common/data/a.ityp"),
        Some("DLC_ITYP_REQUEST")
    );
    assert_eq!(file_type_for("a.unknown"), None);
    // ⚠ Not a gap. Shipped `.meta` entries carry dozens of different types,
    // so an author states this one and `problems()` refuses a pack until they do.
    assert_eq!(file_type_for("common/data/weapons.meta"), None);
}

// ⚠ **These cannot say the game loads a pack.** Nothing in this repository can
// run GTA V, so what is checked is that the archive reads back, that the two
// documents inside it parse, and that the pack's own files are where
// `content.xml` says they are. Whether the engine mounts it needs a person
// with the game.

use or_io::container::rpf::{NoOpCryptoProvider, RpfArchive, RpfCryptoProvider, RpfEncryption};
use std::sync::Arc;

fn no_keys() -> Arc<dyn RpfCryptoProvider> {
    Arc::new(NoOpCryptoProvider)
}

/// A folder removed when the first of the pair drops, so hold it as long as the second is used.
fn lane_scratch(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let scratch = tempfile::Builder::new()
        .prefix(&format!("openrage-dlc-pack-tests-{name}-"))
        .tempdir()
        .expect("a scratch directory");
    let dir = scratch.path().to_path_buf();
    (scratch, dir)
}

#[test]
fn a_built_pack_reads_back_as_the_pack_that_was_specified() {
    let (_scratch, dir) = lane_scratch("built");
    let payload = dir.join("models.rpf");
    std::fs::write(&payload, b"not really an archive, but it is a file").unwrap();
    let ityp = dir.join("props.ityp");
    std::fs::write(&ityp, b"nor is this").unwrap();

    let mut spec = DlcPackSpec::new("mymod");
    spec.files
        .push(PackFile::new("x64/levels/gta5/props/models.rpf", &payload));
    spec.files
        .push(PackFile::new("x64/levels/gta5/props/props.ityp", &ityp));

    let out = dir.join("dlc.rpf");
    let docs = build_pack(&spec, &out, RpfEncryption::Open, no_keys()).expect("the pack builds");

    let archive = RpfArchive::open(&out).expect("the archive opens");

    let paths: Vec<String> = archive
        .nodes
        .iter()
        .filter(|n| !n.is_dir())
        .map(|n| n.path().replace('\\', "/").to_ascii_lowercase())
        .collect();

    for want in [
        "setup2.xml",
        "content.xml",
        "x64/levels/gta5/props/models.rpf",
        "x64/levels/gta5/props/props.ityp",
    ] {
        assert!(
            paths.contains(&want.to_string()),
            "{want} missing: {paths:?}"
        );
    }

    let read = |name: &str| {
        archive
            .read(name)
            .unwrap_or_else(|e| panic!("{name} is in the archive and reads: {e}"))
    };

    let setup = SetupData::parse(&read("setup2.xml")).expect("setup2.xml parses");
    assert_eq!(setup, spec.setup());
    assert_eq!(setup.to_xml(), docs.setup2);

    let content = ContentXml::parse(&read("content.xml")).expect("content.xml parses");
    assert_eq!(content, spec.content());

    // ⚠ The registered paths must name files the archive actually holds, with
    // the device prefix stripped. A pack that registers a path it does not
    // ship mounts and then fails one file at a time, at load.
    let device = format!("{}:/", spec.content_device().to_ascii_lowercase());
    let mut checked = 0;
    for df in &content.data_files {
        let lower = df.filename.to_ascii_lowercase();
        let inside = lower
            .strip_prefix(&device)
            .unwrap_or_else(|| panic!("`{}` is not under `{device}`", df.filename));
        assert!(
            paths.contains(&inside.to_string()),
            "`{}` is registered and not shipped",
            df.filename
        );
        checked += 1;
    }
    assert_eq!(
        checked,
        spec.files.len(),
        "every file in the spec must have been checked"
    );

    let payload_bytes = read("x64/levels/gta5/props/models.rpf");
    assert_eq!(payload_bytes, std::fs::read(&payload).unwrap());
}

#[test]
fn a_pack_that_would_not_load_is_refused_before_anything_is_written() {
    let (_scratch, dir) = lane_scratch("refused");
    let out = dir.join("dlc.rpf");

    let spec = DlcPackSpec::new("mymod");
    let err = build_pack(&spec, &out, RpfEncryption::Open, no_keys())
        .expect_err("a pack with no files is refused");
    assert!(
        err.to_string().contains("registers nothing"),
        "the refusal should name its cause: {err}"
    );
    assert!(
        !out.exists(),
        "a refused pack must leave no half-written archive behind"
    );
}

/// ⚠ Without this the suite above proves only that `build_pack` returns `Ok`.
#[test]
fn the_archive_would_notice_a_file_that_was_never_added() {
    let (_scratch, dir) = lane_scratch("negative");
    let payload = dir.join("a.rpf");
    std::fs::write(&payload, b"x").unwrap();

    let mut spec = DlcPackSpec::new("mymod");
    spec.files.push(PackFile::new("x64/a.rpf", &payload));
    let out = dir.join("dlc.rpf");
    build_pack(&spec, &out, RpfEncryption::Open, no_keys()).expect("builds");

    let archive = RpfArchive::open(&out).unwrap();
    let has = |want: &str| {
        archive
            .nodes
            .iter()
            .any(|n| !n.is_dir() && n.path().replace('\\', "/").eq_ignore_ascii_case(want))
    };
    assert!(has("x64/a.rpf"));
    assert!(
        !has("x64/never_added.rpf"),
        "the lookup this suite relies on must be able to say no"
    );
}

/// ⚠ A `dlc.rpf` cut short still mounts, as a pack missing what it registers.
#[test]
fn a_build_told_to_stop_leaves_no_archive_behind() {
    let (_scratch, dir) = lane_scratch("stopped");
    let payload = dir.join("a.rpf");
    std::fs::write(&payload, vec![7u8; 4096]).unwrap();
    let mut spec = DlcPackSpec::new("mymod");
    spec.files.push(PackFile::new("x64/a.rpf", &payload));

    let out = dir.join("stopped").join("dlc.rpf");
    let mut heard = Vec::new();
    let built = build_pack_with(
        &spec,
        &out,
        RpfEncryption::Open,
        no_keys(),
        &mut |written| {
            heard.push(written);
            written < 1024
        },
    );
    assert!(
        built.is_err(),
        "a build told to stop says it did not finish"
    );
    assert!(
        heard.len() > 1 && heard.windows(2).all(|pair| pair[0] <= pair[1]),
        "it is told how far it has got, as it gets there: {heard:?}"
    );
    assert!(
        !out.exists(),
        "a stopped build leaves no half-written archive to mount"
    );

    let (whole, plain) = (dir.join("whole.rpf"), dir.join("plain.rpf"));
    build_pack_with(&spec, &whole, RpfEncryption::Open, no_keys(), &mut |_| true)
        .expect("a build told to go on finishes");
    build_pack(&spec, &plain, RpfEncryption::Open, no_keys()).expect("builds");
    assert!(
        std::fs::read(&whole).unwrap() == std::fs::read(&plain).unwrap(),
        "being told how far it has got changes nothing it writes"
    );
}

#[test]
fn an_open_pack_needs_no_keys() {
    let (_scratch, dir) = lane_scratch("open");
    let payload = dir.join("a.rpf");
    std::fs::write(&payload, b"x").unwrap();

    let mut spec = DlcPackSpec::new("mymod");
    spec.files.push(PackFile::new("x64/a.rpf", &payload));
    let out = dir.join("dlc.rpf");

    build_pack(&spec, &out, RpfEncryption::Open, no_keys()).expect("builds with no keys");
    assert!(out.is_file());
}

#[test]
fn every_reader_refuses_a_document_of_another_kind_by_its_root() {
    let change_set =
        b"<CDataFileMgr__ContentChangeSetDef><id>x</id></CDataFileMgr__ContentChangeSetDef>";
    let content_def = b"<CDataFileMgr__ContentDef><includedSets /></CDataFileMgr__ContentDef>";
    let setup = b"<SSetupData><nameHash>x</nameHash></SSetupData>";
    let refusals = [
        ("dlclist", DlcList::parse(setup).err(), "SSetupData"),
        (
            "setup",
            SetupData::parse(content_def).err(),
            "CDataFileMgr__ContentDef",
        ),
        (
            "content",
            ContentXml::parse(change_set).err(),
            "CDataFileMgr__ContentChangeSetDef",
        ),
        (
            "content",
            ContentXml::parse(content_def).err(),
            "CDataFileMgr__ContentDef",
        ),
        (
            "etud",
            ExtraTitleUpdateData::parse(setup).err(),
            "SSetupData",
        ),
        (
            "change set",
            ContentChangeSetDef::parse(content_def).err(),
            "CDataFileMgr__ContentDef",
        ),
        (
            "content def",
            ContentDef::parse(change_set).err(),
            "CDataFileMgr__ContentChangeSetDef",
        ),
        ("content def", ContentDef::parse(SETUP2).err(), "SSetupData"),
    ];
    for (reader, err, root) in refusals {
        let err = err.unwrap_or_else(|| panic!("the {reader} reader took a `{root}` document"));
        assert!(err.to_string().contains(root), "{reader}: {err}");
    }
    assert!(
        SetupData::parse(b"<?xml version=\"1.0\"?>").is_err(),
        "no root at all"
    );
}

#[test]
fn a_bare_data_files_document_still_reads_as_content() {
    let c = ContentXml::parse(b"<dataFiles><Item><filename>a.rpf</filename></Item></dataFiles>")
        .unwrap();
    assert_eq!(c.data_files.len(), 1);
}

const GTA_VI_CHANGE_SET: &[u8] = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>
<CDataFileMgr__ContentChangeSetDef>
  <!--a comment-->
  <id>pack_map</id>
  <registrationGroup>GROUP_A</registrationGroup>
  <associatedMap>levels/a</associatedMap>
  <fileList>
    <Item>
      <filename>$(target)/a/b.rpf</filename>
      <fileType>RPF_FILE</fileType>
      <flags>Persistent</flags>
      <contents>CONTENTS_MAP</contents>
    </Item>
    <Item>
      <filename>$(root)/c</filename>
    </Item>
  </fileList>
  <flags>MapChange</flags>
  <changesetIncludeFlags>kDefault</changesetIncludeFlags>
  <mapAddAtPhase value=\"0\" />
  <mapRemoveAtPhase value=\"2\" />
  <executionConditions>
    <activeChangesetConditions>$level=a</activeChangesetConditions>
    <genericConditions />
  </executionConditions>
</CDataFileMgr__ContentChangeSetDef>"
    .as_bytes();

#[test]
fn a_gta_vi_change_set_reads_every_field_it_states() {
    let set = ContentChangeSetDef::parse(GTA_VI_CHANGE_SET).unwrap();
    assert_eq!(set.id, "pack_map");
    assert_eq!(set.registration_group, "GROUP_A");
    assert_eq!(set.associated_map, "levels/a");
    assert_eq!(
        set.file_list,
        vec![
            RegisteredFile {
                filename: "$(target)/a/b.rpf".into(),
                file_type: "RPF_FILE".into(),
                flags: vec!["Persistent".into()],
                contents: "CONTENTS_MAP".into(),
            },
            RegisteredFile {
                filename: "$(root)/c".into(),
                ..Default::default()
            },
        ]
    );
    assert_eq!(set.flags, ["MapChange"]);
    assert_eq!(set.changeset_include_flags, ["kDefault"]);
    assert_eq!(set.unexpanded(), [("$(root)", 1), ("$(target)", 1)]);
    assert_eq!(set.map_add_at_phase.as_deref(), Some("0"));
    assert_eq!(set.map_remove_at_phase.as_deref(), Some("2"));
    assert_eq!(
        set.execution_conditions,
        Some(ExecutionConditions {
            active_changeset_conditions: "$level=a".into(),
            generic_conditions: String::new(),
        })
    );
    assert!(set.unread.is_empty(), "{:?}", set.unread);
}

#[test]
fn a_flags_element_that_states_several_names_is_split_at_white_space() {
    let xml = b"<CDataFileMgr__ContentChangeSetDef><fileList><Item><filename>a</filename>\
        <flags> Persistent\n Locked </flags></Item></fileList><flags /></CDataFileMgr__ContentChangeSetDef>";
    let set = ContentChangeSetDef::parse(xml).unwrap();
    assert_eq!(set.file_list[0].flags, ["Persistent", "Locked"]);
    assert!(set.flags.is_empty());
    assert!(set.unexpanded().is_empty(), "no path names a macro");
}

#[test]
fn a_gta_vi_change_set_reports_what_it_does_not_know() {
    let xml = br#"<CDataFileMgr__ContentChangeSetDef novel="1">
  <id>x</id>
  <fileList><Item><filename>a</filename><weight>3</weight></Item><Item value="v" /></fileList>
  <newSection><Item>a</Item></newSection>
  <mapAddAtPhase value="0" />
</CDataFileMgr__ContentChangeSetDef>"#;
    let set = ContentChangeSetDef::parse(xml).unwrap();
    assert_eq!(
        set.unread,
        [
            "@novel",
            "fileList/Item/weight",
            "fileList/Item@value",
            "newSection"
        ]
    );
}

#[test]
fn a_gta_vi_content_definition_reads_its_sets_and_includes() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<CDataFileMgr__ContentDef>
  <contentSets>
    <Item>
      <id>level_a</id>
      <flags>Legacy</flags>
      <fileList>
        <Item><filename>platform:/levels/a/b</filename><fileType>ELEVATOR_FILE</fileType></Item>
      </fileList>
      <odd />
    </Item>
  </contentSets>
  <includedSets>
    <Item>common:/data/a.meta</Item>
  </includedSets>
  <includedChangesets >
    <Item platform="ps5" subplatform="trinity">
      <loadPath>$(root)/include/a.meta</loadPath>
    </Item>
    <Item>
      <loadPath>$(root)/changesets/ccs_b.meta</loadPath>
      <changesetIncludeFlags>kDefault</changesetIncludeFlags>
    </Item>
  </includedChangesets>
  <later />
</CDataFileMgr__ContentDef>"#;
    let def = ContentDef::parse(xml).unwrap();
    assert_eq!(def.content_sets.len(), 1);
    let set = &def.content_sets[0];
    assert_eq!(
        (set.id.as_str(), set.flags.as_slice()),
        ("level_a", &["Legacy".to_string()][..])
    );
    assert_eq!(set.file_list[0].file_type, "ELEVATOR_FILE");
    assert_eq!(set.unread, ["odd"]);
    assert_eq!(
        def.included_sets,
        vec![ListItem::from("common:/data/a.meta")]
    );
    assert_eq!(
        def.included_changesets,
        vec![
            IncludedChangeset {
                load_path: "$(root)/include/a.meta".into(),
                changeset_include_flags: Vec::new(),
                platform: "ps5".into(),
                subplatform: "trinity".into(),
            },
            IncludedChangeset {
                load_path: "$(root)/changesets/ccs_b.meta".into(),
                changeset_include_flags: vec!["kDefault".into()],
                ..Default::default()
            },
        ]
    );
    assert_eq!(def.unread, ["later"]);
}

#[test]
fn a_gta_vi_setup_reads_platform_data_under_root_and_a_gta_v_one_states_none() {
    let xml = br#"<SSetupData><nameHash>core</nameHash><platformDataUnderRoot value="true" /></SSetupData>"#;
    let s = SetupData::parse(xml).unwrap();
    assert_eq!(s.platform_data_under_root, Some(true));
    let written = SetupData::parse(s.to_xml().as_bytes()).unwrap();
    assert_eq!(written.platform_data_under_root, Some(true));

    let gta_v = SetupData::parse(SETUP2).unwrap();
    assert_eq!(gta_v.platform_data_under_root, None);
    assert!(!gta_v.to_xml().contains("platformDataUnderRoot"));
}

fn a_change_set(id: &str, map: &str) -> ContentChangeSetDef {
    ContentChangeSetDef {
        id: id.into(),
        registration_group: format!("GROUP_{id}"),
        associated_map: map.into(),
        file_list: vec![
            RegisteredFile {
                filename: format!("$(target)/{id}/a & b.rpf"),
                file_type: "RPF_FILE".into(),
                flags: vec!["Persistent".into(), "Locked".into()],
                contents: "CONTENTS_MAP".into(),
            },
            RegisteredFile {
                filename: format!("$(root)/{id}.meta"),
                file_type: "ELEVATOR_FILE".into(),
                ..Default::default()
            },
        ],
        flags: vec!["MapChange".into()],
        changeset_include_flags: vec!["kDefault".into()],
        map_add_at_phase: Some("1".into()),
        map_remove_at_phase: map.is_empty().then(|| "3".into()),
        execution_conditions: (!map.is_empty()).then(|| ExecutionConditions {
            active_changeset_conditions: format!("$level={map}"),
            generic_conditions: "<b>".into(),
        }),
        unread: Vec::new(),
    }
}

#[test]
fn a_gta_vi_change_set_and_content_definition_write_what_they_read_back() {
    for set in [a_change_set("one", "levels/a"), a_change_set("two", "")] {
        let xml = set.to_xml();
        assert_eq!(
            ContentChangeSetDef::parse(xml.as_bytes()).unwrap(),
            set,
            "{xml}"
        );
    }
    let def = ContentDef {
        content_sets: vec![a_change_set("three", "levels/b"), a_change_set("four", "")],
        included_sets: vec![ListItem::from("common:/data/a.meta")],
        included_changesets: vec![
            IncludedChangeset {
                load_path: "$(root)/include/a.meta".into(),
                changeset_include_flags: vec!["kDefault".into(), "kOther".into()],
                platform: "ps5".into(),
                subplatform: "trinity".into(),
            },
            IncludedChangeset {
                load_path: "$(root)/changesets/ccs_b.meta".into(),
                platform: "xbsx".into(),
                ..Default::default()
            },
        ],
        unread: Vec::new(),
    };
    let xml = def.to_xml();
    assert_eq!(ContentDef::parse(xml.as_bytes()).unwrap(), def, "{xml}");
    assert_eq!(
        ContentDef::parse(ContentDef::default().to_xml().as_bytes()).unwrap(),
        ContentDef::default()
    );
}
