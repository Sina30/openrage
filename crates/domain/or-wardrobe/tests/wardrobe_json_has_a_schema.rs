//! `wardrobe.json` has a JSON Schema, generated from the types that read and write it and kept
//! with the schemas of the other source files, and every key the file writes is a property of it.
//! A field added to a type fails the first until `OPENRAGE_WRITE_SCHEMAS=1` writes the schema again
//! and the second until the sample states it.

use std::collections::BTreeMap;
use std::path::Path;

use or_source::schemas::undescribed;
use or_wardrobe::alternate::{AlternateSet, Asset, Switch};
use or_wardrobe::model::schema;
use or_wardrobe::query::SavedQuery;
use or_wardrobe::settings::{Dummies, Level, Setting, Settings, Target};
use or_wardrobe::{
    Carried, Component, Expression, Garment, Meta, Ped, Replace, Shop, ShopEntry, Skin, Slot,
    Stated, Variation, Waiver, Wardrobe,
};
use serde_json::json;

fn sample() -> Wardrobe {
    let entry = ShopEntry {
        id: "CLO_X_0".into(),
        from: "shop.meta".into(),
        entry: 3,
        cost: Some("100".into()),
        fields: vec!["<restrictionTags />".into()],
    };
    let variation = Variation {
        texture: Some("jbib_diff_000_a_uni.ytd".into()),
        tint: Some("uni".into()),
        tex_id: Some(0),
        distribution: Some(255),
        inclusions: Some(1),
        exclusions: Some(2),
        inclusion_id: Some(3),
        exclusion_id: Some(4),
        shop: Some(entry.clone()),
        shop_id: Some("mine".into()),
    };
    let meta = Meta {
        prop_mask: Some(1),
        num_alternatives: Some(2),
        ped_xml_audio_id: Some("a".into()),
        ped_xml_audio_id2: Some("b".into()),
        ped_xml_expression_mods: Some([0.0; 5]),
        flags: Some(3),
        inclusions: Some(4),
        exclusions: Some(5),
        ped_xml_vfx_comps: Some(6),
        ped_xml_flags: Some(7),
        audio_id: Some("c".into()),
        expression_mods: Some([1.0; 5]),
        render_flags: Some(8),
        prop_flags: Some(9),
        stickyness: Some(10),
        first_person_alternate: Some(11),
    };
    let garment = Garment {
        id: "jacket".into(),
        label: Some("Jacket".into()),
        tags: vec!["Summer".into(), "Leather".into()],
        group: Some("Winter set".into()),
        ped: Some(Ped::Male),
        slot: Some(Slot::Component(Component::Uppr)),
        position: Some(4),
        skin: Skin::Race,
        model: Some("uppr_004_r.ydd".into()),
        alternates: vec!["uppr_004_r_1.ydd".into()],
        cloth: Some("uppr_004_r.yld".into()),
        missing_cloth: true,
        wind_scale: Some(1.5),
        variations: vec![variation],
        meta,
        expressions: vec![Expression {
            index: 1,
            tracks: vec![1],
            ids: vec![2],
            types: vec![3],
            components: vec![4],
        }],
    };
    Wardrobe {
        format: "openrage-wardrobe".into(),
        schema: or_wardrobe::model::SCHEMA,
        pack: "unity".into(),
        collections: BTreeMap::from([(Ped::Male, "mp_m_unity".to_string())]),
        garments: vec![garment],
        alternates: vec![AlternateSet {
            id: "hood".into(),
            name: "Hair under a hood".into(),
            ped: Ped::Male,
            switches: vec![
                Switch {
                    target: Asset::outside(
                        Some("mp_m_example_hair"),
                        Slot::Component(Component::Hair),
                        4,
                    ),
                    alt: 3,
                },
                Switch {
                    target: Asset::garment("jacket"),
                    alt: 1,
                },
            ],
            sources: vec![
                Asset::garment("jacket"),
                Asset::outside(None, Slot::Prop(or_wardrobe::Prop::Head), 2),
            ],
        }],
        queries: vec![SavedQuery {
            name: "Singleplayer wool".into(),
            query: "tag:singleplayer tag:wool".into(),
        }],
        waivers: vec![Waiver {
            code: "W001".into(),
            garment: Some("jacket".into()),
            reason: "on purpose".into(),
        }],
        shop: Shop {
            durty_hashes: true,
            unplaced: vec![Stated {
                ped: Ped::Female,
                kind: "PV_COMP_UPPR".into(),
                prop: true,
                drawable: 1,
                texture: 2,
                entry: entry.clone(),
            }],
            carried: vec![Carried {
                file: "a.meta".into(),
                directive: "data_file".into(),
                kind: "SHOP_PED_APPAREL_SPAK_FILE".into(),
            }],
        },
        rules: Settings {
            target: Some(Target::Legacy),
            settings: BTreeMap::from([(
                "texture-size".to_string(),
                Setting {
                    on: Some(false),
                    severity: Some(Level::Error),
                    threshold: Some(4096),
                },
            )]),
            freeze: Some(false),
            dummies: Dummies {
                replace_on_add: true,
                leave_on_delete: true,
            },
            replace: BTreeMap::from([(
                Ped::Female,
                Replace {
                    dlc: Some("mp_f_heist".into()),
                },
            )]),
        },
        lock: None,
    }
}

#[test]
fn the_schema_on_disk_is_what_the_types_generate() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../or-source/schemas");
    schema().keep(&dir).unwrap_or_else(|why| panic!("{why}"));
}

#[test]
fn every_key_wardrobe_json_writes_is_a_property_of_its_schema() {
    let document = serde_json::to_value(sample()).unwrap();
    let missing = undescribed(&schema().schema, &document);
    assert!(missing.is_empty(), "no property for {missing:?}");

    // The sample is a pack the model reads back, and the check finds a key that is not a property.
    assert_eq!(
        Wardrobe::from_json(sample().to_json().as_bytes(), "sample").unwrap(),
        sample()
    );
    let mut stray = document;
    stray["garments"][0]["meta"]["bogus"] = json!(1);
    stray["garments"][0]["variations"][0]["bogus"] = json!(1);
    let mut found = undescribed(&schema().schema, &stray);
    found.sort();
    assert_eq!(found, ["garments.meta.bogus", "garments.variations.bogus"]);
}

#[test]
fn the_glob_an_editor_binds_the_wardrobe_schema_by_is_in_the_design_document() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/direction/Source_Format_Design.md");
    let design = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n");
    let schema = schema();
    assert!(design.contains(&format!("`{}`", schema.file)));
    for glob in schema.binds {
        assert!(design.contains(&format!("`{glob}`")), "{glob}");
    }
}
