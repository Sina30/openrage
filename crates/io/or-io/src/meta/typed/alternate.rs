//! The alternate variations as a typed root: `pedalternatevariations.meta` as [`CAlternateVariations`],
//! the switches that make a drawable of one slot wear its alternate model while another asset is worn.
//!
//! A switch names the drawable that changes (`component`, `index`, `dlcNameHash`, the collection it
//! is in, none for the game's own), the alternate it wears (`alt`, the number after the model's
//! name: `hair_000_u_1.ydd` is alternate 1) and the assets that make it change (`sourceAssets`).
//! A source is a component (`component`) or a prop (`anchor`), whichever of the two it states.
//!
//! ⚠ The root is read from XML, PSO or RBM and built and saved as XML, which is how a resource ships
//! it; no staged binary document states these types, so none can be synthesised.

use super::*;

/// What a source asset states for the member it does not use: the engine's `init` for `component` and `anchor`.
const UNSET: u8 = 255;

/// The most switches the game collects for one source asset: its buffer holds 160 and asserts below the last place.
pub const MOST_SWITCHES_FOR_ONE_ASSET: usize = 159;

const COMPONENT: &str = "CAlternateVariations::sAlternateVariationComponent";
const SWITCH: &str = "CAlternateVariations::sAlternateVariationSwitch";
const PED: &str = "CAlternateVariations::sAlternateVariationPed";

meta_record! {
    /// `CAlternateVariations::sAlternateVariationComponent`: an asset whose being worn makes a switch happen; a component or, with `anchor`, a prop.
    SAlternateVariationComponent = "CAlternateVariations::sAlternateVariationComponent" {
        component: u8 = "component" => UNSET,
        anchor: u8 = "anchor" => UNSET,
        index: u8 = "index",
        dlc_name_hash: HashedString = "dlcNameHash",
    }
}

meta_record! {
    /// `CAlternateVariations::sAlternateVariationSwitch`: the drawable that wears alternate `alt`, and the assets that make it.
    SAlternateVariationSwitch = "CAlternateVariations::sAlternateVariationSwitch" {
        component: u8 = "component",
        index: u8 = "index",
        alt: u8 = "alt",
        dlc_name_hash: HashedString = "dlcNameHash",
        source_assets: Vec<SAlternateVariationComponent> = "sourceAssets",
    }
}

meta_record! {
    /// `CAlternateVariations::sAlternateVariationPed`: the switches of one ped model.
    SAlternateVariationPed = "CAlternateVariations::sAlternateVariationPed" {
        name: HashedString = "name",
        switches: Vec<SAlternateVariationSwitch> = "switches",
    }
}

meta_record! {
    /// `CAlternateVariations`: a `pedalternatevariations.meta`, the switches of each ped it states.
    CAlternateVariations = "CAlternateVariations" {
        peds: Vec<SAlternateVariationPed> = "peds",
    }
}

meta_record! {
    /// `FirstPersonAlternate`: the alternate model the local player's first-person view streams in for the asset `asset_name` names.
    FirstPersonAlternate = "FirstPersonAlternate" {
        asset_name: HashedString = "assetName",
        alternate: u8 = "alternate",
    }
}

meta_record! {
    /// `FirstPersonAlternateData`: a first-person alternate file, the assets that have one.
    FirstPersonAlternateData = "FirstPersonAlternateData" {
        alternates: Vec<FirstPersonAlternate> = "alternates",
    }
}

meta_origin!(CAlternateVariations);
meta_origin!(FirstPersonAlternateData);

/// A name as its text when it has one, else as the hash a file spells it.
fn spelled(name: &HashedString) -> String {
    match name.raw.is_empty() {
        true => format!("hash_{:08X}", name.hash),
        false => name.raw.clone(),
    }
}

impl CAlternateVariations {
    /// Built here or moved from another type, so no arrival layout has its shape.
    fn reshaped(&self) -> bool {
        built_here(&self.carry, "CAlternateVariations")
            || self.peds.iter().any(|p| {
                built_here(&p.carry, PED)
                    || p.switches.iter().any(|s| {
                        built_here(&s.carry, SWITCH)
                            || s.source_assets
                                .iter()
                                .any(|a| built_here(&a.carry, COMPONENT))
                    })
            })
    }

    /// ⚠ What the game refuses or overruns: alternate 0 is the drawable itself, and one asset collects at most [`MOST_SWITCHES_FOR_ONE_ASSET`] switches.
    fn broken(&self) -> Option<Broken> {
        for ped in &self.peds {
            let name = spelled(&ped.name);
            if let Some(s) = ped.switches.iter().find(|s| s.alt == 0) {
                return Some(Broken::ZeroAlternate {
                    ped: name,
                    component: s.component,
                    index: s.index,
                });
            }
            let mut held: std::collections::BTreeMap<(u8, u8, u8, u32), usize> =
                std::collections::BTreeMap::new();
            for asset in ped.switches.iter().flat_map(|s| &s.source_assets) {
                let key = (
                    asset.component,
                    asset.anchor,
                    asset.index,
                    asset.dlc_name_hash.hash,
                );
                let count = held.entry(key).or_default();
                *count += 1;
                if *count > MOST_SWITCHES_FOR_ONE_ASSET {
                    return Some(Broken::TooManySwitchesForOneAsset {
                        ped: name,
                        component: asset.component,
                        anchor: asset.anchor,
                        index: asset.index,
                    });
                }
            }
        }
        None
    }
}

impl File for CAlternateVariations {
    const EXTENSION: &'static str = "meta";

    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let (mut root, arrival) =
            open_root(Self::EXTENSION, "CAlternateVariations", bytes, Self::read)?;
        root.carry.arrival = Some(Box::new(arrival));
        Ok(root)
    }

    /// ⚠ A root built here is written as XML.
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let xml = Arrival {
            layout: None,
            format: SourceFormat::Xml,
        };
        save_root_as(
            Self::EXTENSION,
            self.broken(),
            self.write(),
            &self.carry,
            self.reshaped(),
            Some(&xml),
        )
    }
}

impl File for FirstPersonAlternateData {
    const EXTENSION: &'static str = "meta";

    fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let (mut root, arrival) = open_root(
            Self::EXTENSION,
            "FirstPersonAlternateData",
            bytes,
            Self::read,
        )?;
        root.carry.arrival = Some(Box::new(arrival));
        Ok(root)
    }

    /// ⚠ A root built here is written as XML.
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let xml = Arrival {
            layout: None,
            format: SourceFormat::Xml,
        };
        let reshaped = built_here(&self.carry, "FirstPersonAlternateData")
            || self
                .alternates
                .iter()
                .any(|a| built_here(&a.carry, "FirstPersonAlternate"));
        save_root_as(
            Self::EXTENSION,
            None,
            self.write(),
            &self.carry,
            reshaped,
            Some(&xml),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_person_file_built_here_saves_as_xml_and_reads_back_equal() {
        let mut built = FirstPersonAlternateData::default();
        built.alternates.push(FirstPersonAlternate {
            asset_name: HashedString::new("mp_m_freemode_01_mp_m_example/jbib_003_u"),
            alternate: 2,
            ..Default::default()
        });
        let once = built.to_bytes().expect("it saves");
        assert!(once.starts_with(b"<?xml"));
        let back = FirstPersonAlternateData::from_bytes(&once).expect("it reopens");
        assert_eq!(back.alternates.len(), 1);
        assert_eq!(back.alternates[0].alternate, 2);
        assert_eq!(
            back.alternates[0].asset_name.hash,
            crate::string_hash(b"mp_m_freemode_01_mp_m_example/jbib_003_u")
        );
        assert_eq!(back.to_bytes().expect("it saves again"), once);
        let none = FirstPersonAlternateData::from_bytes(
            b"<?xml version=\"1.0\"?><FirstPersonAlternateData><alternates /></FirstPersonAlternateData>",
        )
        .expect("an empty list reads");
        assert!(none.alternates.is_empty());
    }

    /// The shape of a resource's file, with names of this test's own.
    const FILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CAlternateVariations>
  <peds>
    <Item>
      <name>mp_m_freemode_01</name>
      <switches>
        <Item>
          <dlcNameHash>mp_m_example_hair</dlcNameHash>
          <component value="2" />
          <index value="4" />
          <alt value="3" />
          <sourceAssets>
            <Item>
              <dlcNameHash>mp_m_example_clothes</dlcNameHash>
              <component value="11" />
              <index value="7" />
            </Item>
            <Item>
              <dlcNameHash>mp_m_example_clothes</dlcNameHash>
              <anchor value="0" />
              <index value="2" />
            </Item>
          </sourceAssets>
        </Item>
        <Item>
          <component value="2" />
          <index value="0" />
          <alt value="1" />
          <sourceAssets>
            <Item>
              <component value="1" />
              <index value="9" />
            </Item>
          </sourceAssets>
        </Item>
      </switches>
    </Item>
  </peds>
</CAlternateVariations>
"#;

    fn root() -> CAlternateVariations {
        CAlternateVariations::from_bytes(FILE.as_bytes()).expect("the file reads")
    }

    #[test]
    fn a_resources_file_reads_each_member_by_the_name_the_file_spells() {
        let root = root();
        let ped = &root.peds[0];
        assert_eq!(ped.name, "mp_m_freemode_01");
        assert_eq!(ped.switches.len(), 2);
        let switch = &ped.switches[0];
        assert_eq!(
            (switch.component, switch.index, switch.alt),
            (2, 4, 3),
            "the drawable that changes"
        );
        assert_eq!(switch.dlc_name_hash, "mp_m_example_hair");
        assert_eq!(switch.source_assets.len(), 2);
        assert_eq!(switch.source_assets[0].component, 11);
        assert_eq!(
            switch.source_assets[0].anchor, 255,
            "a source that states no anchor is a component"
        );
        assert_eq!(switch.source_assets[1].component, 255);
        assert_eq!(switch.source_assets[1].anchor, 0);
        assert_eq!(
            ped.switches[1].dlc_name_hash.hash, 0,
            "the game's own drawables state no collection"
        );
    }

    #[test]
    fn a_root_saves_and_reads_back_equal_and_saving_that_again_changes_no_byte() {
        let first = root().to_bytes().expect("it saves");
        let again = CAlternateVariations::from_bytes(&first).expect("it reopens");
        assert_eq!(again, root());
        assert_eq!(again.to_bytes().expect("it saves again"), first);
    }

    #[test]
    fn a_root_built_from_nothing_saves_as_xml_with_no_snapshot() {
        assert!(
            !crate::meta::has_default_library(),
            "this test needs no snapshot"
        );
        let mut built = CAlternateVariations::default();
        let mut ped = SAlternateVariationPed {
            name: HashedString::new("mp_f_freemode_01"),
            ..Default::default()
        };
        ped.switches.push(SAlternateVariationSwitch {
            component: 2,
            index: 5,
            alt: 2,
            dlc_name_hash: HashedString::new("mp_f_example_hair"),
            source_assets: vec![
                SAlternateVariationComponent {
                    component: 11,
                    index: 3,
                    dlc_name_hash: HashedString::new("mp_f_example_clothes"),
                    ..Default::default()
                },
                SAlternateVariationComponent {
                    anchor: 0,
                    index: 1,
                    dlc_name_hash: HashedString::new("mp_f_example_clothes"),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        built.peds.push(ped);
        let once = built.to_bytes().expect("a root built here saves");
        assert!(
            once.starts_with(b"<?xml"),
            "it is XML, which is how a resource ships it"
        );
        let back = CAlternateVariations::from_bytes(&once).expect("it reopens");
        assert_eq!(back.peds[0].switches[0].source_assets[0].component, 11);
        assert_eq!(back.peds[0].switches[0].source_assets[0].anchor, 255);
        assert_eq!(back.peds[0].switches[0].source_assets[1].component, 255);
        assert_eq!(back.peds[0].switches[0].source_assets[1].anchor, 0);
        assert_eq!(back.peds[0].switches[0].alt, 2);
        assert_eq!(back.to_bytes().expect("it saves again"), once);
    }

    #[test]
    fn a_switch_with_no_sources_written_as_an_empty_element_reads_as_none() {
        let text = FILE.replace(
            r#"<sourceAssets>
            <Item>
              <component value="1" />
              <index value="9" />
            </Item>
          </sourceAssets>"#,
            "<sourceAssets />",
        );
        let root = CAlternateVariations::from_bytes(text.as_bytes()).expect("it reads");
        assert!(root.peds[0].switches[1].source_assets.is_empty());
    }

    #[test]
    fn alternate_zero_is_refused_before_any_byte_is_written() {
        let mut root = root();
        assert!(root.broken().is_none());
        root.peds[0].switches[0].alt = 0;
        let err = root
            .to_bytes()
            .expect_err("alternate 0 is the drawable itself");
        assert!(err.to_string().contains("alternate 0"), "{err}");
    }

    #[test]
    fn one_asset_that_collects_more_switches_than_the_game_holds_is_refused() {
        let mut root = root();
        let switch = root.peds[0].switches[1].clone();
        for _ in 0..MOST_SWITCHES_FOR_ONE_ASSET {
            root.peds[0].switches.push(switch.clone());
        }
        // The asset is now the source of 1 + MOST switches.
        let err = root
            .to_bytes()
            .expect_err("more switches than the game holds");
        assert!(err.to_string().contains("159"), "{err}");
        root.peds[0].switches.pop();
        assert!(root.broken().is_none(), "exactly the most it holds");
    }
}
