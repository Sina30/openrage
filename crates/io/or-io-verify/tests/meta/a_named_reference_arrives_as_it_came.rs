//! A pointer to a named item (`<AimingInfo ref="PISTOL_AIM" />`) read through a schema that declares it, and written back.
//!
//! ⚠ A schema's `external_named` pointer is a name, and the value tree has no variant for one: reading it as a null pointer
//! dropped it from the text, so a weapon file opened and saved by a developer's machine lost every reference it held.

use or_io::meta::instance::parse_meta as parse;
use or_io::meta::{build_library, write_meta};

const PSC: &str = r#"<ParserSchema>
  <structdef type="CHolder">
    <float name="Damage"/>
    <pointer name="AimingInfo" type="CAimingInfo" policy="external_named"/>
  </structdef>
  <structdef type="CAimingInfo">
    <float name="HeadingLimit"/>
  </structdef>
</ParserSchema>"#;

#[test]
fn a_reference_to_a_named_item_is_written_back_with_its_name() {
    let library = build_library([("t.psc", PSC.as_bytes())]).unwrap();
    let xml = br#"<CHolder>
  <Damage value="12.0" />
  <AimingInfo ref="PISTOL_AIM" />
</CHolder>"#;
    let tree = parse(&library, xml).expect("the document reads");
    let saved = write_meta(&tree).expect("the document writes");
    assert!(
        saved.contains(r#"ref="PISTOL_AIM""#),
        "the reference did not arrive: {saved}"
    );
    assert!(saved.contains("Damage"), "{saved}");
}
