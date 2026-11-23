//! The weapon family as typed roots: a weapon file as [`CWeaponInfoBlob`], a components file as
//! [`CWeaponComponentInfoBlob`] and an archetypes file as [`CWeaponModelInfoInitDataList`], each member found by the
//! name the document spells.
//!
//! ⚠ The weapon metadata the game ships is XML text, and a record keeps the value tree it was read from: what no
//! member names (a `ref` to a named item, a null pointer, a member whose shape differs from file to file) reaches the
//! file as it arrived, and an untouched save is the tree's own canonical text. A save writes OpenRage's canonical XML (no
//! byte order mark, LF), which the game reads as it reads the shipped text.

use std::collections::HashSet;
use std::sync::OnceLock;

use super::*;

pub use super::weapon_ammo::*;
pub use super::weapon_archetypes::*;
pub use super::weapon_blob::*;
pub use super::weapon_components::*;
pub use super::weapon_info::*;

/// An element of an array of pointers: the record its type names, whichever of the family it is.
macro_rules! meta_union {
    (
        $(#[$doc:meta])*
        $name:ident { $($variant:ident,)* }
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq)]
        // A record is held by value, as every typed root is: an element of the family is an owned value, not a box a person unwraps.
        #[allow(clippy::large_enum_variant)]
        pub enum $name {
            $($variant($variant),)*
        }

        impl $name {
            /// The engine's name of the type the element is.
            pub fn type_name(&self) -> &'static str {
                match self {
                    $(Self::$variant(_) => stringify!($variant),)*
                }
            }
        }

        impl Element for $name {
            fn read_element(value: &Value) -> Result<Self, String> {
                let inst = match value {
                    Value::Struct(inst) | Value::Pointer(Some(inst)) => inst,
                    Value::Pointer(None) => return Err("a null pointer".into()),
                    other => return expected("a struct", other),
                };
                $(if is_named(&inst.type_name, stringify!($variant)) {
                    return <$variant as Member>::read(value).map(Self::$variant);
                })*
                Err(format!("a `{}` is none of the types of a `{}`", inst.type_name, stringify!($name)))
            }

            fn write_element(&self, was: Option<&Value>, sibling: Option<&Value>) -> Result<Value, String> {
                let written = match self {
                    $(Self::$variant(v) => v.write_element(was, sibling)?,)*
                };
                // An element of an array of pointers is behind a pointer, whichever way the record would be placed.
                Ok(match written {
                    Value::Struct(inst) => Value::Pointer(Some(inst)),
                    other => other,
                })
            }
        }
    };
}

meta_union! {
    /// An info of a weapon file: a weapon or one of the ammunition it fires.
    InfoAny { CWeaponInfo, CAmmoInfo, CAmmoProjectileInfo, CAmmoRocketInfo, CAmmoThrownInfo, }
}

meta_union! {
    /// An entry of the `Data` of a components file: reload and swap data the components name.
    ComponentDataAny { CWeaponComponentReloadData, CWeaponSwapData, CWeaponComponentReloadLoopedData, }
}

meta_union! {
    /// A component of a components file.
    ComponentInfoAny {
        CWeaponComponentInfo,
        CWeaponComponentClipInfo,
        CWeaponComponentFlashLightInfo,
        CWeaponComponentScopeInfo,
        CWeaponComponentSuppressorInfo,
        CWeaponComponentVariantModelInfo,
    }
}

/// Whether any record of the family names the member stored under `key`.
pub(super) fn is_member(key: u32) -> bool {
    static KEYS: OnceLock<HashSet<u32>> = OnceLock::new();
    KEYS.get_or_init(|| {
        [
            CWeaponInfoBlob::MEMBERS,
            CWeaponInfoBlobSWeaponSlotList::MEMBERS,
            CWeaponInfoBlobSlotEntry::MEMBERS,
            CWeaponTintSpecValues::MEMBERS,
            CWeaponTintSpecValuesCWeaponSpecValue::MEMBERS,
            CWeaponFiringPatternAliases::MEMBERS,
            CWeaponFiringPatternAliasesCFiringPatternAlias::MEMBERS,
            CWeaponUpperBodyFixupExpressionData::MEMBERS,
            CWeaponUpperBodyFixupExpressionDataData::MEMBERS,
            CAimingInfo::MEMBERS,
            CVehicleWeaponInfo::MEMBERS,
            CWeaponInfoBlobSWeaponGroupArmouredGlassDamage::MEMBERS,
            CWeaponInfoBlobInfos::MEMBERS,
            CWeaponInfo::MEMBERS,
            CWeaponComponentPoint::MEMBERS,
            CWeaponComponentPointSComponent::MEMBERS,
            CWeaponInfoSFx::MEMBERS,
            CWeaponInfoSFrontClearTestParams::MEMBERS,
            CWeaponInfoSFirstPersonScopeAttachmentData::MEMBERS,
            CAmmoInfo::MEMBERS,
            CAmmoProjectileInfo::MEMBERS,
            CAmmoProjectileInfoSExplosion::MEMBERS,
            CAmmoRocketInfo::MEMBERS,
            CAmmoThrownInfo::MEMBERS,
            CWeaponComponentInfoBlob::MEMBERS,
            CWeaponComponentReloadData::MEMBERS,
            CWeaponSwapData::MEMBERS,
            CWeaponComponentReloadLoopedData::MEMBERS,
            CWeaponComponentInfo::MEMBERS,
            CWeaponComponentClipInfo::MEMBERS,
            CWeaponComponentFlashLightInfo::MEMBERS,
            CWeaponComponentScopeInfo::MEMBERS,
            CWeaponComponentSuppressorInfo::MEMBERS,
            CWeaponComponentVariantModelInfo::MEMBERS,
            CWeaponModelInfoInitDataList::MEMBERS,
            CWeaponModelInfoInitData::MEMBERS,
        ]
        .iter()
        .flat_map(|members| members.iter())
        .map(|name| literal_string_hash(name.as_bytes()))
        .collect()
    })
    .contains(&key)
}

/// A text document as the record `type_name`, and how it arrived.
pub(super) fn open_text<T>(
    type_name: &str,
    bytes: &[u8],
    read: fn(&MetaInstance) -> Result<T, String>,
) -> Result<(T, Arrival), Error> {
    const EXT: &str = "meta";
    let doc = MetaDocument::parse(
        &weapon_types::library(crate::meta::default_library()),
        bytes.to_vec(),
    )
    .map_err(|e| Error::Read {
        ext: EXT,
        why: e.to_string(),
    })?;
    if doc.source_format != SourceFormat::Xml {
        return Err(Error::Read {
            ext: EXT,
            why: format!(
                "a {} document: the weapon metadata read here is XML text",
                doc.source_format.label()
            ),
        });
    }
    if !is_named(&doc.instance.type_name, type_name) {
        return Err(Error::WrongType {
            expected: type_name.to_string(),
            found: doc.instance.type_name,
        });
    }
    let root = read(&doc.instance).map_err(|why| Error::Read { ext: EXT, why })?;
    Ok((
        root,
        Arrival {
            layout: None,
            format: SourceFormat::Xml,
        },
    ))
}

/// The canonical XML of a root, which is what a text document is saved as.
pub(super) fn save_text(root: Result<MetaInstance, String>) -> Result<Vec<u8>, Error> {
    let tree = root.map_err(|why| Error::Save { ext: "meta", why })?;
    write_meta(&tree)
        .map(String::into_bytes)
        .map_err(|e| Error::Save {
            ext: "meta",
            why: e.to_string(),
        })
}

/// The name of the root element of an XML text, none for anything else.
pub fn root_of(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut rest = text.trim_start_matches('\u{feff}');
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("<?") {
            rest = &after[after.find("?>")? + 2..];
        } else if let Some(after) = rest.strip_prefix("<!--") {
            rest = &after[after.find("-->")? + 3..];
        } else {
            break;
        }
    }
    let tag = rest.strip_prefix('<')?;
    let end = tag.find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
    Some(tag[..end].to_string())
}

/// A text document of the family read as its root: the root's name and how many elements its lists hold, or why it does not open;
/// none for a document of another root, which this family has no reader for.
pub fn reload(bytes: &[u8]) -> Option<Result<(&'static str, usize), String>> {
    let root = root_of(bytes)?;
    let read = |name, count: Result<usize, Error>| {
        Some(count.map(|count| (name, count)).map_err(|e| e.to_string()))
    };
    if is_named(&root, "CWeaponInfoBlob") {
        return read(
            "CWeaponInfoBlob",
            CWeaponInfoBlob::from_bytes(bytes)
                .map(|blob| blob.infos.iter().map(|group| group.infos.len()).sum()),
        );
    }
    if is_named(&root, "CWeaponComponentInfoBlob") {
        return read(
            "CWeaponComponentInfoBlob",
            CWeaponComponentInfoBlob::from_bytes(bytes)
                .map(|blob| blob.data.len() + blob.infos.len()),
        );
    }
    if is_named(&root, "CWeaponModelInfo__InitDataList") {
        return read(
            "CWeaponModelInfo::InitDataList",
            CWeaponModelInfoInitDataList::from_bytes(bytes).map(|list| list.init_datas.len()),
        );
    }
    None
}
