//! Struct definitions shipped type tables state and the PSC corpus does not.
//!
//! The canonical import types a document from the schema snapshot, which is
//! the PSC corpus. Where shipped files hold something that corpus never
//! declares, the files' own type tables are the only source, so what they say
//! is transcribed here in the PSC's dialect and a suite holds each entry to
//! the tables it came from.
//!
//! ⚠ **Additive by construction.** A type
//! the snapshot already declares is never replaced, and a member is added to a
//! document's types only when that document carries it — so a file that names
//! nothing here is typed exactly as it would be without this module.

use std::borrow::Cow;
use std::sync::OnceLock;

use crate::meta::instance::{MetaInstance, Value};
use crate::meta::schema::{parse_psc, SchemaItem, SchemaLibrary, StructDef};

/// Types a shipped document is rooted at that the PSC corpus never declares.
///
/// Transcribed from `startupreq.ymt`'s own type table, the only document in
/// the staged corpus that names them; `every_shipped_type_is_what_its_type_table_says`
/// compares every member, in order, with that table.
///
/// ⚠ `Extension` is an embedded `char[8]` in the file. The PSC dialect says
/// so, and the type model reads every `type="member"` string as a pointer —
/// the same mapping the 200 such members the PSC corpus declares get.
pub const SHIPPED_TYPES: &str = r#"<ParserSchema>
  <structdef type="strRequestRecording">
    <array name="Requests" type="atArray"><struct type="strRecordedRequest"/></array>
  </structdef>
  <structdef type="strRecordedRequest">
    <struct name="Resource" type="strResourceReference"/>
    <s32 name="Flags"/>
  </structdef>
  <structdef type="strResourceReference">
    <string name="AssetName" type="atString"/>
    <string name="Extension" type="member" size="8"/>
  </structdef>
</ParserSchema>"#;

/// Members an Enhanced type table appends to a type the PSC corpus declares.
///
/// Each `structdef` here names an existing type and lists only what Enhanced
/// adds, in the order it adds it; derived types inherit them, which is where
/// the files put them. Measured over both retail installs by
/// `every_world_file_reaches_its_xml_fixed_point`: every Enhanced table that
/// carries one of these carries it as stated, and no Legacy table carries any.
///
/// ⚠ **The name is a hash because nothing names it.** A tag spelled
/// `hash_73B8AA83` is read as that number, not hashed as text.
pub const GEN9_MEMBERS: &str = r#"<ParserSchema>
  <structdef type="CEntityDef">
    <u32 name="hash_73B8AA83"/>
  </structdef>
  <structdef type="CBaseArchetypeDef">
    <u32 name="hash_73B8AA83"/>
    <u32 name="padding1"/>
  </structdef>
</ParserSchema>"#;

fn parsed(psc: &'static str) -> Vec<StructDef> {
    parse_psc(psc.as_bytes(), "shipped")
        .expect("the shipped definitions are well-formed PSC")
        .into_iter()
        .filter_map(|item| match item {
            SchemaItem::Struct(s) => Some(s),
            _ => None,
        })
        .collect()
}

/// [`SHIPPED_TYPES`] as struct definitions.
pub fn shipped_types() -> &'static [StructDef] {
    static TYPES: OnceLock<Vec<StructDef>> = OnceLock::new();
    TYPES.get_or_init(|| parsed(SHIPPED_TYPES))
}

/// [`GEN9_MEMBERS`] as struct definitions naming only the added members.
pub fn gen9_members() -> &'static [StructDef] {
    static MEMBERS: OnceLock<Vec<StructDef>> = OnceLock::new();
    MEMBERS.get_or_init(|| parsed(GEN9_MEMBERS))
}

/// `base` with the shipped types it lacks, and Enhanced's members when
/// `gen9` is set.
pub fn extend(base: &SchemaLibrary, gen9: bool) -> SchemaLibrary {
    let mut lib = base.clone();
    for def in shipped_types() {
        if lib.struct_(&def.name).is_none() {
            lib.insert_struct(def.clone());
        }
    }
    if gen9 {
        for added in gen9_members() {
            let Some(def) = lib.structs.get_mut(&added.name) else {
                continue;
            };
            for field in &added.fields {
                if !def.fields.iter().any(|f| f.name == field.name) {
                    def.fields.push(field.clone());
                }
            }
        }
    }
    lib
}

/// [`extend`], built once for the installed default library and on every call
/// for any other.
pub fn library(base: &SchemaLibrary, gen9: bool) -> Cow<'_, SchemaLibrary> {
    static SHIPPED: OnceLock<SchemaLibrary> = OnceLock::new();
    static SHIPPED_GEN9: OnceLock<SchemaLibrary> = OnceLock::new();
    let Some(installed) = crate::meta::snapshot::installed().filter(|lib| std::ptr::eq(base, *lib))
    else {
        return Cow::Owned(extend(base, gen9));
    };
    let cell = if gen9 { &SHIPPED_GEN9 } else { &SHIPPED };
    Cow::Borrowed(cell.get_or_init(|| extend(installed, gen9)))
}

/// Whether `instance` holds, as a member `library` does not declare, one of
/// Enhanced's.
pub fn carries_gen9_member(library: &SchemaLibrary, instance: &MetaInstance) -> bool {
    fn declares(library: &SchemaLibrary, type_name: &str, tag: &str) -> bool {
        let Some(resolved) = library.resolve_any(type_name) else {
            return false;
        };
        gen9_members().iter().any(|added| {
            resolved.chain.iter().any(|d| d.name == added.name)
                && added.fields.iter().any(|f| f.name == tag)
        })
    }
    fn walk(library: &SchemaLibrary, inst: &MetaInstance) -> bool {
        inst.extras
            .iter()
            .any(|extra| declares(library, &inst.type_name, &extra.tag))
            || inst.fields.iter().any(|(_, v)| walk_value(library, v))
    }
    fn walk_value(library: &SchemaLibrary, value: &Value) -> bool {
        match value {
            Value::Struct(inner) | Value::Pointer(Some(inner)) => walk(library, inner),
            Value::Array(items) => items.iter().any(|v| walk_value(library, v)),
            Value::Map(pairs) => pairs
                .iter()
                .any(|(k, v)| walk_value(library, k) || walk_value(library, v)),
            _ => false,
        }
    }
    walk(library, instance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::schema::build_library;

    const PSC: &str = r#"<ParserSchema>
  <structdef type="CEntityDef"><u32 name="flags"/><u32 name="tintValue"/></structdef>
  <structdef type="CMloInstanceDef" base="CEntityDef"><u32 name="groupId"/></structdef>
  <structdef type="CBaseArchetypeDef"><float name="lodDist"/></structdef>
  <structdef type="strRequestRecording"><u32 name="declaredHere"/></structdef>
</ParserSchema>"#;

    fn names(lib: &SchemaLibrary, ty: &str) -> Vec<String> {
        lib.resolve(ty)
            .unwrap()
            .fields
            .iter()
            .map(|f| f.name.clone())
            .collect()
    }

    #[test]
    fn extending_without_gen9_changes_no_type_the_base_declares() {
        let base = build_library([("t.psc", PSC.as_bytes())]).unwrap();
        let lib = extend(&base, false);
        for (name, def) in &base.structs {
            assert_eq!(lib.struct_(name), Some(def), "{name} changed");
        }
        assert!(lib.struct_("strRecordedRequest").is_some());
        assert_eq!(
            names(&lib, "strRequestRecording"),
            ["declaredHere"],
            "a type the PSC declares is never replaced by a transcription"
        );
    }

    #[test]
    fn enhanced_members_land_on_the_base_and_reach_what_derives_from_it() {
        let base = build_library([("t.psc", PSC.as_bytes())]).unwrap();
        let lib = extend(&base, true);
        assert_eq!(
            names(&lib, "CMloInstanceDef"),
            ["flags", "tintValue", "hash_73B8AA83", "groupId"]
        );
        assert_eq!(
            names(&lib, "CBaseArchetypeDef"),
            ["lodDist", "hash_73B8AA83", "padding1"]
        );
    }

    #[test]
    fn only_a_document_carrying_an_enhanced_member_asks_for_them() {
        let base = build_library([("t.psc", PSC.as_bytes())]).unwrap();
        let mut entity = MetaInstance::new("CMloInstanceDef");
        let legacy = Value::Pointer(Some(Box::new(entity.clone())));
        entity.extras.push(crate::meta::instance::UnknownNode {
            tag: "hash_73B8AA83".into(),
            attrs: vec![("value".into(), "7".into())],
            ..Default::default()
        });
        let enhanced = Value::Pointer(Some(Box::new(entity)));

        let mut root = MetaInstance::new("CMapData");
        root.set("entities", Value::Array(vec![legacy.clone()]));
        assert!(!carries_gen9_member(&base, &root));
        root.set("entities", Value::Array(vec![legacy, enhanced]));
        assert!(carries_gen9_member(&base, &root));

        let mut stranger = MetaInstance::new("CSomethingElse");
        stranger.extras.push(crate::meta::instance::UnknownNode {
            tag: "hash_73B8AA83".into(),
            ..Default::default()
        });
        assert!(
            !carries_gen9_member(&base, &stranger),
            "the same hash on a type Enhanced did not extend selects nothing"
        );
    }
}
