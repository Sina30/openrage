//! The JSON Schemas of the files a source holds are what the types generate, and every key those
//! files write is a property of one. Adding a field to a type fails the first until the schema is
//! written again (`OPENRAGE_WRITE_SCHEMAS=1`) and the second until a sample states it.

use std::collections::BTreeMap;
use std::path::Path;

use or_source::asset::{AssetManifest, How, Origin};
use or_source::drawable_values::{BoneRule, Extent, Material, ModelRule, Parameter, Real, Sphere};
use or_source::models::{Dictionary, EntryRef, ImportRules, Rules as Drawable};
use or_source::schemas::{self, undescribed, Schema};
use or_source::textures::{
    AlphaMode, Channel, ColourSpace, Encode, Entry, Filter, Mips, Pack, Quality, Rules as Textures,
    Taken, Tonemap,
};
use serde_json::{json, Value};

fn dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(schemas::DIR)
}

fn schema(file: &str) -> Schema {
    schemas::sources()
        .into_iter()
        .find(|s| s.file == file)
        .unwrap_or_else(|| panic!("no schema {file}"))
}

#[test]
fn every_schema_on_disk_is_what_the_types_generate() {
    let mut failures = Vec::new();
    for schema in schemas::sources() {
        if let Err(why) = schema.keep(&dir()) {
            failures.push(why);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A manifest that states everything it can, including the `compile` block a model keeps in it.
fn manifest() -> Value {
    let origin = Origin {
        how: How::Extracted,
        from_game: true,
        virtual_path: Some("x64a.rpf/models/a.ydr".into()),
        game_build: Some("3095".into()),
        sha256: Some("aa".into()),
        cooked_sha256: Some("bb".into()),
        extra: BTreeMap::new(),
    };
    let mut manifest = AssetManifest::new("drawable", "a", "gen8", origin).unwrap();
    manifest.notes = "for the crew".into();
    manifest.credit = Some(or_source::asset::Credit {
        author: "Jane Doe".into(),
        url: "https://example.org/a".into(),
        licence: "CC-BY-4.0".into(),
    });
    manifest
        .extra
        .insert("compile".into(), json!({"mode": "edit", "base": "cc"}));
    serde_json::to_value(manifest).unwrap()
}

/// A metadata source's manifest, whose `compile` block is of another shape than a model's.
fn metadata_manifest() -> Value {
    let origin = Origin {
        how: How::Extracted,
        from_game: false,
        virtual_path: None,
        game_build: None,
        sha256: None,
        cooked_sha256: None,
        extra: BTreeMap::new(),
    };
    let mut manifest = AssetManifest::new("metadata", "a", "gen8", origin).unwrap();
    manifest
        .extra
        .insert("compile".into(), json!({"extension": "ymap", "base": "cc"}));
    serde_json::to_value(manifest).unwrap()
}

/// Two entries that state every rule and every header word between them.
fn textures() -> Value {
    let taken = |channel| Taken {
        file: "a.png".into(),
        channel,
    };
    let pack = Pack {
        r: Some(taken(Channel::R)),
        g: Some(taken(Channel::G)),
        b: Some(taken(Channel::B)),
        a: Some(taken(Channel::A)),
    };
    let encode = Encode {
        format: Some("DXT5".into()),
        mips: Some(Mips::To4x4),
        filter: Some(Filter::Box),
        quality: Some(Quality::Normal),
        colour_space: Some(ColourSpace::Linear),
        flip_green: Some(false),
        clamp: Some([0.0, 1.0]),
        tonemap: Some(Tonemap::Reinhard),
        exposure: Some(1.0),
        alpha: Some(AlphaMode::Premultiplied),
        alpha_coverage: Some(0.5),
    };
    let legacy = json!({
        "resource_type_and_conversion_flags": ["processed"], "template_type": ["0x20000054"],
        "extra_flags": ["0x00000010"], "handle_index": 7, "info_bits": ["dynamic"], "stereo_rt_mode": 2,
    });
    let enhanced = json!({
        "flags": ["0x00260208"], "usage_data": ["0x00800254"], "anti_alias_type": 1, "tile_mode": "Auto",
    });
    let entry = |name: &str, header: Value| Entry {
        name: name.into(),
        source: Some("a.png".into()),
        pack: Some(pack.clone()),
        header: Some(header),
        encode: Some(encode.clone()),
    };
    serde_json::to_value(Textures {
        textures: vec![entry("legacy", legacy), entry("enhanced", enhanced)],
    })
    .unwrap()
}

fn drawable() -> Value {
    let parameters = BTreeMap::from([
        (
            "DiffuseSampler".to_string(),
            Parameter::Texture("jbib_diff_000".into()),
        ),
        (
            "specularFalloffMult".to_string(),
            Parameter::Vectors(vec![vec![Real(0.5), Real(2.0), Real(3.0), Real(4.0)]]),
        ),
    ]);
    serde_json::to_value(Drawable {
        base: json!({"geometries": 2}),
        import: ImportRules {
            fill_missing: true,
            keep_skinning: false,
        },
        skeleton: vec!["SKEL_ROOT".into()],
        materials: vec![Material {
            name: "default".into(),
            shader: "default".into(),
            draw_bucket: 0,
            draw_bucket_mask: 1,
            flags: Some(0),
            user_flags: Some(0),
            parameters,
        }],
        models: vec![ModelRule {
            at: "lod0/model0".into(),
            name: "a_lod0_model0".into(),
            flags: 0,
            mask: 255,
        }],
        lod_distances: Some([Real(9998.0); 4]),
        bucket_masks: Some([1, 2, 3, 4]),
        bounds_sphere: Some(Sphere {
            center: [Real(0.0); 3],
            radius: Real(1.0),
        }),
        bounds_box: Some(Extent {
            min: [Real(-1.0); 3],
            max: [Real(1.0); 3],
        }),
        bones: vec![BoneRule {
            name: "SKEL_ROOT".into(),
            tag: 0,
        }],
    })
    .unwrap()
}

fn dictionary() -> Value {
    serde_json::to_value(Dictionary {
        entries: vec![EntryRef {
            code: "0x0000002a".into(),
            folder: "a".into(),
        }],
    })
    .unwrap()
}

#[test]
fn every_key_a_source_file_writes_is_a_property_of_its_schema() {
    for (file, document) in [
        ("asset.schema.json", manifest()),
        ("asset.schema.json", metadata_manifest()),
        ("textures.schema.json", textures()),
        ("drawable.schema.json", drawable()),
        ("dictionary.schema.json", dictionary()),
    ] {
        let missing = undescribed(&schema(file).schema, &document);
        assert!(missing.is_empty(), "{file} has no property for {missing:?}");
    }
}

#[test]
fn a_key_with_no_property_is_found_and_the_check_is_not_empty() {
    let asset = schema("asset.schema.json").schema;
    let mut document = manifest();
    // The manifest carries keys a later build wrote, so a key of its own is free; a block it describes is not.
    document["bogus"] = json!(1);
    document["compile"]["bogus"] = json!(true);
    assert_eq!(undescribed(&asset, &document), ["compile.bogus"]);

    let textures = schema("textures.schema.json").schema;
    let mut document = self::textures();
    document["textures"][0]["encode"]["bogus"] = json!(1);
    document["textures"][1]["header"]["bogus"] = json!(1);
    document["textures"][0]["pack"]["r"]["bogus"] = json!(1);
    let mut found = undescribed(&textures, &document);
    found.sort();
    assert_eq!(
        found,
        [
            "textures.encode.bogus",
            "textures.header.bogus",
            "textures.pack.r.bogus"
        ]
    );
}

#[test]
fn the_globs_an_editor_binds_the_schemas_by_are_in_this_document() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/direction/Source_Format_Design.md");
    let design = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n");
    for schema in schemas::sources() {
        assert!(
            design.contains(&format!("`{}`", schema.file)),
            "{}",
            schema.file
        );
        for glob in schema.binds {
            assert!(
                design.contains(&format!("`{glob}`")),
                "{}: {glob}",
                schema.file
            );
        }
    }
    assert!(design.contains("`**/*.src/*.json`"));
}
