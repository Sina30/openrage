//! Where a synthesised RBM class puts its first member, on a library this file
//! builds, so a clean checkout with no corpus still exercises it.
//!
//! The game places a class by its declared size and member offsets, so a class
//! laid out eight bytes short of the vtable word it carries crashes the load.

use or_io::meta::binary::rbm::schema::RbmClass;
use or_io::meta::binary::synth::{name_hash, rbm_layout_for};
use or_io::meta::{Field, FieldHint, FieldType, SchemaLibrary, StructDef};

const VTABLE: u32 = 8;

fn field(name: &str, ty: FieldType) -> Field {
    Field {
        name: name.to_string(),
        ty,
        init: None,
        min: None,
        max: None,
        step: None,
        ui_key: false,
        hide_widgets: false,
        description: None,
        hint: FieldHint::None,
    }
}

fn struct_def(name: &str, base: Option<&str>, simple: bool, fields: Vec<Field>) -> StructDef {
    StructDef {
        name: name.to_string(),
        base: base.map(str::to_string),
        abstract_: false,
        simple,
        fields,
        source_path: String::new(),
    }
}

fn library() -> SchemaLibrary {
    let mut lib = SchemaLibrary::new();
    lib.insert_struct(struct_def(
        "Plain",
        None,
        true,
        vec![field("a", FieldType::U32)],
    ));
    lib.insert_struct(struct_def(
        "Virtual",
        None,
        false,
        vec![field("a", FieldType::U32)],
    ));
    lib.insert_struct(struct_def(
        "Derived",
        Some("Virtual"),
        false,
        vec![field("b", FieldType::U32)],
    ));
    lib.insert_struct(struct_def(
        "Holder",
        None,
        true,
        vec![
            field("flag", FieldType::U8),
            field("inner", FieldType::Struct("Virtual".into())),
        ],
    ));
    lib
}

fn class_of(root: &str, name: &str) -> RbmClass {
    let layout = rbm_layout_for(&library(), root).expect("the library lays out");
    layout
        .model
        .class_for(name_hash(name))
        .unwrap_or_else(|| panic!("'{name}' is not in {root}'s type table"))
        .clone()
}

fn offsets(class: &RbmClass) -> Vec<(u32, u32)> {
    class
        .entries
        .iter()
        .filter(|e| e.name_hash != 0x100)
        .map(|e| (e.name_hash, e.offset))
        .collect()
}

#[test]
fn a_simple_class_starts_at_zero() {
    let class = class_of("Plain", "Plain");
    assert_eq!(offsets(&class), [(name_hash("a"), 0)]);
    assert_eq!(class.data_size, 4);
}

#[test]
fn a_class_that_is_not_simple_starts_after_its_vtable_word() {
    let class = class_of("Virtual", "Virtual");
    assert_eq!(offsets(&class), [(name_hash("a"), VTABLE)]);
    assert_eq!(
        class.data_size, 16,
        "8 + 4, rounded up to its 8-byte alignment"
    );
    assert_eq!(class.align_power, 3);
}

#[test]
fn a_derived_class_holds_one_vtable_word_before_the_base_members() {
    let class = class_of("Derived", "Derived");
    assert_eq!(
        offsets(&class),
        [(name_hash("a"), VTABLE), (name_hash("b"), VTABLE + 4)]
    );
    assert_eq!(class.data_size, 16);
}

#[test]
fn a_class_held_by_value_keeps_its_vtable_word() {
    let holder = class_of("Holder", "Holder");
    assert_eq!(
        offsets(&holder),
        [(name_hash("flag"), 0), (name_hash("inner"), 8)],
        "the inner class starts on its own 8-byte alignment"
    );
    assert_eq!(holder.data_size, 24);
}
