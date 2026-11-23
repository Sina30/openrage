use crate::support;

use or_io::meta::{FieldType, StringKind};

#[test]
fn snapshot_decodes() {
    let Some(lib) = support::schema_or_skip("snapshot_load") else {
        return;
    };
    assert!(
        lib.structs.len() > 100,
        "expected many structs, got {}",
        lib.structs.len()
    );
    assert!(
        lib.enums.len() > 10,
        "expected many enums, got {}",
        lib.enums.len()
    );
}

#[test]
fn ambient_model_set_present() {
    let Some(lib) = support::schema_or_skip("snapshot_load") else {
        return;
    };
    let s = lib.struct_("CAmbientModelSet").expect("CAmbientModelSet");
    let name_member = s
        .fields
        .iter()
        .find(|f| f.name == "Name")
        .expect("Name field");
    assert!(matches!(
        name_member.ty,
        FieldType::String(StringKind::AtHashString)
    ));
    assert!(name_member.ui_key);

    let models_member = s
        .fields
        .iter()
        .find(|f| f.name == "Models")
        .expect("Models field");
    match &models_member.ty {
        FieldType::Array { elem, .. } => match elem.as_ref() {
            FieldType::Struct(name) => assert_eq!(name, "CAmbientModel"),
            other => panic!("expected struct elem, got {other:?}"),
        },
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn anim_scene_flags_is_bitset_enum() {
    let Some(lib) = support::schema_or_skip("snapshot_load") else {
        return;
    };
    let e = lib.enum_("eAnimSceneFlags").expect("eAnimSceneFlags");
    assert!(e.generate_bitset);
    assert!(e.values.iter().any(|v| v.name == "ASF_LOOPING"));
}

#[test]
fn inheritance_resolves_flat_field_list() {
    let Some(lib) = support::schema_or_skip("snapshot_load") else {
        return;
    };
    let resolved = lib
        .resolve("CAmbientVehicleModelVariations")
        .expect("derived struct resolves");
    assert!(
        resolved.chain.len() >= 2,
        "expected base chain, got {}",
        resolved.chain.len()
    );
    assert_eq!(
        resolved.chain.last().unwrap().name,
        "CAmbientVehicleModelVariations"
    );
}

#[test]
fn an_absent_snapshot_yields_an_empty_library_rather_than_a_broken_one() {
    if or_test_support::schema_snapshot().is_some() {
        return;
    }
    let lib = or_io::meta::default_library();
    assert!(lib.structs.is_empty());
    assert!(lib.enums.is_empty());
    assert!(lib.struct_("CAmbientModelSet").is_none());
}
