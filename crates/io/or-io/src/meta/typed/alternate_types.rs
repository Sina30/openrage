//! The alternate-variation types, declared here so a root built from nothing saves where no schema
//! snapshot is installed, as the ped variation family's are.
//!
//! The tables are the engine's own; no staged binary document states them, so there is no signature
//! to carry and the root is built and saved as XML, which is how a resource ships it.

use std::sync::OnceLock;

use crate::meta::schema::{parse_psc, SchemaItem, SchemaLibrary};

pub const ALTERNATE_TYPES: &str = r#"<ParserSchema>
  <structdef type="CAlternateVariations__sAlternateVariationComponent">
    <u8 name="component" init="255"/>
    <u8 name="anchor" init="255"/>
    <u8 name="index"/>
    <string name="dlcNameHash" type="atHashString"/>
  </structdef>
  <structdef type="CAlternateVariations__sAlternateVariationSwitch">
    <u8 name="component"/>
    <u8 name="index"/>
    <u8 name="alt"/>
    <string name="dlcNameHash" type="atHashString"/>
    <array name="sourceAssets" type="atArray"><struct type="CAlternateVariations__sAlternateVariationComponent"/></array>
  </structdef>
  <structdef type="CAlternateVariations__sAlternateVariationPed">
    <string name="name" type="atHashString"/>
    <array name="switches" type="atArray"><struct type="CAlternateVariations__sAlternateVariationSwitch"/></array>
  </structdef>
  <structdef type="CAlternateVariations">
    <array name="peds" type="atArray"><struct type="CAlternateVariations__sAlternateVariationPed"/></array>
  </structdef>
  <structdef type="FirstPersonAlternate">
    <string name="assetName" type="atHashString"/>
    <u8 name="alternate"/>
  </structdef>
  <structdef type="FirstPersonAlternateData">
    <array name="alternates" type="atArray"><struct type="FirstPersonAlternate"/></array>
  </structdef>
</ParserSchema>"#;

/// [`ALTERNATE_TYPES`] as a library.
pub fn declared() -> &'static SchemaLibrary {
    static LIBRARY: OnceLock<SchemaLibrary> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let mut library = SchemaLibrary::new();
        let items = parse_psc(ALTERNATE_TYPES.as_bytes(), "alternate")
            .expect("the alternate-variation types are well-formed PSC");
        for item in items {
            match item {
                SchemaItem::Struct(def) => library.insert_struct(def),
                SchemaItem::Enum(def) => library.insert_enum(def),
                SchemaItem::Skipped => {}
            }
        }
        library
    })
}
