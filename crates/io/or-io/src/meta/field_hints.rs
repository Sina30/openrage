use crate::meta::schema::normalize::normalize_member_name;
use crate::meta::schema::types::{FieldHint, FieldType, StringKind};

pub fn infer_hint(name: &str, ty: &FieldType) -> FieldHint {
    if !is_hashable_string(ty) {
        return FieldHint::None;
    }
    let lower = normalize_member_name(name).to_ascii_lowercase();

    if contains_any(
        &lower,
        &["texture", "diffuse", "normalmap", "specularmap", "txd"],
    ) {
        return FieldHint::TextureName;
    }
    if contains_any(
        &lower,
        &[
            "clipset",
            "clipsetid",
            "animdict",
            "animdictionary",
            "anim",
            "movenetwork",
        ],
    ) {
        return FieldHint::ClipSetName;
    }
    if contains_any(&lower, &["fxname", "particlerule", "ptfx"]) {
        return FieldHint::EffectRuleName;
    }
    if contains_any(&lower, &["sound", "ambientsound", "audio"]) {
        return FieldHint::SoundName;
    }
    if contains_any(&lower, &["scenario", "scenariotype"]) {
        return FieldHint::ScenarioName;
    }
    if contains_any(&lower, &["bonetag", "boneid"]) {
        return FieldHint::BoneTag;
    }
    if contains_any(&lower, &["modkit", "modtype", "extra"]) && lower.starts_with("extra") {
        return FieldHint::VehicleMod;
    }
    if contains_any(
        &lower,
        &["model", "drawable", "prop", "vehicle", "ped", "weapon"],
    ) {
        return FieldHint::ModelName;
    }
    FieldHint::None
}

fn is_hashable_string(ty: &FieldType) -> bool {
    matches!(
        ty,
        FieldType::String(StringKind::AtHashString)
            | FieldType::String(StringKind::AtFinalHashString)
            | FieldType::String(StringKind::AtHashValue)
    )
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::schema::types::{FieldType, StringKind};

    fn hash_field() -> FieldType {
        FieldType::String(StringKind::AtHashString)
    }

    #[test]
    fn model_fields() {
        assert_eq!(infer_hint("m_Model", &hash_field()), FieldHint::ModelName);
        assert_eq!(infer_hint("m_PropSet", &hash_field()), FieldHint::ModelName);
        assert_eq!(
            infer_hint("m_VehicleName", &hash_field()),
            FieldHint::ModelName
        );
    }

    #[test]
    fn clip_fields() {
        assert_eq!(
            infer_hint("m_ClipSet", &hash_field()),
            FieldHint::ClipSetName
        );
        assert_eq!(
            infer_hint("m_AnimDict", &hash_field()),
            FieldHint::ClipSetName
        );
    }

    #[test]
    fn fx_fields() {
        assert_eq!(
            infer_hint("m_fxName", &hash_field()),
            FieldHint::EffectRuleName
        );
    }

    #[test]
    fn non_hash_field_gets_no_hint() {
        assert_eq!(
            infer_hint("m_Model", &FieldType::String(StringKind::Plain)),
            FieldHint::None
        );
        assert_eq!(infer_hint("m_Model", &FieldType::Float), FieldHint::None);
    }

    #[test]
    fn unrelated_name_returns_none() {
        assert_eq!(infer_hint("m_Probability", &hash_field()), FieldHint::None);
    }
}
