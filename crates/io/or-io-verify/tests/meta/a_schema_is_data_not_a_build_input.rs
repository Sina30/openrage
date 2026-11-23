//! The schema reaches a build at run time, never at compile time.

use or_io::meta::BinaryFormat;

const MAP_DATA: &[u8] = br#"<CMapData>
  <name>probe</name>
  <parent/>
  <flags value="0"/>
  <contentFlags value="1"/>
  <streamingExtentsMin x="0" y="0" z="0"/>
  <streamingExtentsMax x="1" y="1" z="1"/>
  <entitiesExtentsMin x="0" y="0" z="0"/>
  <entitiesExtentsMax x="1" y="1" z="1"/>
  <entities/>
  <containerLods/>
  <boxOccluders/>
  <occludeModels/>
  <physicsDictionaries/>
  <instancedData>
    <ImapLink/>
    <PropInstanceList/>
    <GrassInstanceList/>
  </instancedData>
  <timeCycleModifiers/>
  <carGenerators/>
  <LODLightsSOA>
    <direction/>
    <falloff/>
    <falloffExponent/>
    <timeAndStateFlags/>
    <hash/>
    <coneInnerAngle/>
    <coneOuterAngleOrCapExt/>
    <coronaIntensity/>
  </LODLightsSOA>
  <DistantLODLightsSOA>
    <position/>
    <RGBI/>
    <numStreetLights value="0"/>
    <category value="0"/>
  </DistantLODLightsSOA>
  <block>
    <version value="0"/>
    <flags value="0"/>
    <name>probe</name>
    <exportedBy/>
    <owner/>
    <time/>
  </block>
</CMapData>"#;

/// ⚠ Nothing is installed as the default until this test installs it: a
/// default filled at build time or at process start fails the first assert.
#[test]
fn a_schema_is_data_not_a_build_input() {
    const WHAT: &str = "a_schema_is_data_not_a_build_input";
    assert!(
        !or_io::meta::has_default_library(),
        "a default library was installed before anything installed one"
    );
    assert!(or_io::meta::default_library().structs.is_empty());
    let Some(path) = or_test_support::schema_snapshot() else {
        or_test_support::skip_schema(WHAT);
        return;
    };
    let library = or_io::meta::load_snapshot(&path).expect("the snapshot decodes");
    assert!(library.struct_("CMapData").is_some());

    let installed = or_io::meta::install_default_library(library);
    assert!(std::ptr::eq(installed, or_io::meta::default_library()));
    assert!(or_io::meta::is_known_root(
        or_io::meta::default_library(),
        "CMapData"
    ));
    let rbm = or_io::meta::canonical::import(
        or_io::meta::default_library(),
        MAP_DATA,
        BinaryFormat::Rbm,
        None,
    )
    .expect("an XML CMapData imports through the installed default");
    assert!(!rbm.is_empty());
}
