use or_io::desc::value::{self, ScalarValue};
use or_io::desc::{
    BitRange, CountSource, FieldRepr, IntWidth, Pack, PackField, PackSelector, PackType, Pointee,
    RecordError, Scalar, Value, WriteError,
};
use or_io::write::{build, BuildError};

fn field(name: &str, offset: usize, repr: FieldRepr) -> PackField {
    PackField {
        name: name.into(),
        offset,
        repr,
        space: None,
        bits: Vec::new(),
        note: None,
    }
}

fn ty(name: &str, size: usize, fields: Vec<PackField>) -> PackType {
    PackType {
        name: name.into(),
        size,
        fields,
        selector: None,
    }
}

fn int(width: IntWidth) -> FieldRepr {
    FieldRepr::Scalar(Scalar::Int(width))
}

fn u32_in(heap: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(heap[at..at + 4].try_into().expect("four bytes"))
}

fn f32_in(heap: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(heap[at..at + 4].try_into().expect("four bytes"))
}

fn menagerie() -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x30,
        vec![
            field("flags", 0x00, int(IntWidth::U32)),
            field("ratio", 0x04, FieldRepr::Scalar(Scalar::F32)),
            field(
                "kind",
                0x08,
                FieldRepr::Enum {
                    type_name: "Kind".into(),
                    width: IntWidth::U8,
                },
            ),
            field("gap", 0x09, FieldRepr::Padding { size: 3 }),
            field("count", 0x0C, int(IntWidth::U16)),
            field(
                "items",
                0x10,
                FieldRepr::Ptr(Pointee::Many {
                    type_name: Some("Item".into()),
                    count: CountSource::Field("count".into()),
                    element_size: None,
                }),
            ),
            field(
                "nested",
                0x18,
                FieldRepr::Inline {
                    type_name: "Item".into(),
                },
            ),
            field("mystery", 0x20, FieldRepr::Opaque { size: 8 }),
            field(
                "slots",
                0x28,
                FieldRepr::PtrSlots {
                    pointee: Pointee::One {
                        type_name: "Item".into(),
                    },
                    count: 1,
                },
            ),
        ],
    ));
    pack.types.push(ty(
        "Item",
        8,
        vec![
            field("id", 0x00, int(IntWidth::U32)),
            field("weight", 0x04, FieldRepr::Scalar(Scalar::F32)),
        ],
    ));
    pack
}

#[test]
fn a_blank_is_every_field_at_the_zero_its_description_implies() {
    let pack = menagerie();
    let blank = value::empty(&pack, "Root").expect("a described type");

    let record = blank.record().expect("a struct");
    assert_eq!(record.type_name(), "Root");

    let declared: Vec<&str> = pack
        .type_by_name("Root")
        .expect("Root")
        .fields
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    let authored: Vec<&str> = record
        .as_ref()
        .fields()
        .map(|(field, _)| field.name.as_str())
        .collect();
    assert_eq!(
        authored, declared,
        "a blank carries every field, in declaration order — which is allocation \
         order"
    );
    let children: Vec<u32> = record.children().iter().map(|(at, _)| *at).collect();
    assert_eq!(
        children,
        vec![0x10, 0x28],
        "and a child for every pointer field, and for nothing else"
    );

    assert_eq!(blank.get("flags").as_ref().and_then(Value::as_int), Some(0));
    assert_eq!(
        blank.get("ratio").as_ref().and_then(Value::as_f32),
        Some(0.0)
    );
    assert_eq!(blank.get("kind").as_ref().and_then(Value::as_int), Some(0));

    assert_eq!(
        blank.get("gap").as_ref().and_then(Value::as_bytes),
        Some(&[0u8, 0, 0][..])
    );
    assert_eq!(
        blank.get("mystery").as_ref().and_then(Value::as_bytes),
        Some(&[0u8; 8][..])
    );

    assert!(matches!(
        blank.child("items"),
        Some(Value::Ptr { target: None, .. })
    ));
    assert_eq!(
        blank
            .child("slots")
            .and_then(Value::elements)
            .map(<[_]>::len),
        Some(1)
    );

    assert_eq!(
        blank.get("nested.id").as_ref().and_then(Value::as_int),
        Some(0)
    );
}

#[test]
fn a_blank_filled_in_lands_where_the_description_says() {
    let pack = menagerie();
    let mut root = value::empty(&pack, "Root").expect("a described type");

    let record = root.record_mut().expect("a struct");
    record
        .set_scalar("flags", ScalarValue::Int(0xDEAD_BEEF))
        .expect("flags");
    record
        .set_scalar("ratio", ScalarValue::F32(1.5))
        .expect("ratio");
    record
        .set_structural("count", ScalarValue::Int(2))
        .expect("count");
    record
        .set_scalar("nested.id", ScalarValue::Int(7))
        .expect("nested.id");
    let mystery = record.plan().field("mystery").expect("mystery").offset;
    record.bytes_mut()[mystery..mystery + 8].copy_from_slice(&[0xC0; 8]);

    let mut items = Vec::new();
    for (id, weight) in [(10i128, -1.0f32), (20, 2.0)] {
        let mut item = value::empty(&pack, "Item").expect("Item");
        let fields = item.record_mut().expect("a struct");
        fields.set_scalar("id", ScalarValue::Int(id)).expect("id");
        fields
            .set_scalar("weight", ScalarValue::F32(weight))
            .expect("weight");
        items.push(item);
    }
    record
        .set_child(
            "items",
            Value::ptr_to(Value::Array {
                elements: items,
                capacity: None,
            }),
        )
        .expect("items");

    let built = build(&pack, &root).expect("an authored tree builds");
    let heap = &built.virtual_data;
    let base = (built.root - or_io::container::paging::FIXED_VIRTUAL_BASE) as usize;

    let at = |offset: usize| base + offset;

    assert_eq!(u32_in(heap, at(0x00)), 0xDEAD_BEEF);
    assert_eq!(f32_in(heap, at(0x04)), 1.5);
    assert_eq!(heap[at(0x0C)], 2, "the count field the run is read by");
    assert_eq!(u32_in(heap, at(0x18)), 7, "the embedded struct's id");
    assert_eq!(&heap[at(0x20)..at(0x28)], &[0xC0u8; 8]);

    let items_at = (u64::from_le_bytes(heap[at(0x10)..at(0x18)].try_into().unwrap())
        - or_io::container::paging::FIXED_VIRTUAL_BASE) as usize;
    assert_eq!(u32_in(heap, items_at), 10);
    assert_eq!(f32_in(heap, items_at + 4), -1.0);
    assert_eq!(u32_in(heap, items_at + 8), 20);
    assert_eq!(f32_in(heap, items_at + 12), 2.0);
}

fn root_with(pack: &Pack, items: usize, says: i128) -> Value {
    let mut root = value::empty(pack, "Root").expect("Root");
    let record = root.record_mut().expect("a struct");
    record
        .set_structural("count", ScalarValue::Int(says))
        .expect("count");
    record
        .set_child(
            "items",
            Value::ptr_to(Value::Array {
                elements: (0..items)
                    .map(|_| value::empty(pack, "Item").expect("Item"))
                    .collect(),
                capacity: None,
            }),
        )
        .expect("items");
    root
}

#[test]
fn a_run_longer_than_its_count_field_is_refused() {
    let pack = menagerie();

    build(&pack, &root_with(&pack, 2, 2)).expect("a run and its count agreeing must build");

    let error = build(&pack, &root_with(&pack, 3, 2)).expect_err("three elements, a count of two");
    assert_eq!(
        error,
        BuildError::CountDisagrees {
            type_name: "Root".into(),
            field: "items".into(),
            count_field: "count".into(),
            count_says: 2,
            run_holds: 3,
        }
    );

    let error = build(&pack, &root_with(&pack, 1, 2)).expect_err("one element, a count of two");
    assert!(matches!(
        error,
        BuildError::CountDisagrees {
            count_says: 2,
            run_holds: 1,
            ..
        }
    ));

    let message = error.to_string();
    assert!(
        message.contains("items") && message.contains("count"),
        "{message}"
    );
}

#[test]
fn a_null_pointer_beside_a_non_zero_count_is_left_alone() {
    let pack = menagerie();
    let mut root = value::empty(&pack, "Root").expect("Root");
    root.record_mut()
        .expect("a struct")
        .set_structural("count", ScalarValue::Int(4))
        .expect("count");

    build(&pack, &root).expect("a null run is not a run of the wrong length");
}

fn tagged() -> Pack {
    let mut pack = Pack::new("probe", "gen8");

    let mut base = ty(
        "Base",
        0x10,
        vec![
            field("word", 0x00, int(IntWidth::U32)),
            field("shared", 0x04, int(IntWidth::U32)),
        ],
    );
    base.selector = Some(PackSelector {
        tag: "word".into(),
        tag_bits: Some(BitRange {
            name: "kind".into(),
            low: 8,
            width: 4,
            note: None,
        }),
        variants: vec![(1, "Small".into()), (2, "Large".into())],
    });
    pack.types.push(base);

    pack.types.push(ty(
        "Small",
        0x10,
        vec![field(
            "base",
            0x00,
            FieldRepr::Inline {
                type_name: "Base".into(),
            },
        )],
    ));
    pack.types.push(ty(
        "Large",
        0x20,
        vec![
            field(
                "base",
                0x00,
                FieldRepr::Inline {
                    type_name: "Base".into(),
                },
            ),
            field("extra", 0x10, int(IntWidth::U32)),
        ],
    ));

    pack.types.push(ty(
        "Largest",
        0x30,
        vec![
            field(
                "large",
                0x00,
                FieldRepr::Inline {
                    type_name: "Large".into(),
                },
            ),
            field("more", 0x20, int(IntWidth::U32)),
        ],
    ));
    pack
}

fn set_tag(value: &mut Value, path: &str, word: i128) {
    value
        .record_mut()
        .expect("a struct")
        .set_structural(path, ScalarValue::Int(word))
        .unwrap_or_else(|e| panic!("the tag word: {e}"));
}

#[test]
fn a_blank_variant_carries_the_tag_the_variant_table_gives_it() {
    let pack = tagged();

    for (name, tag) in [("Small", 1u32), ("Large", 2)] {
        let blank = value::empty(&pack, name).expect("a described variant");
        let word = blank
            .get("base.word")
            .as_ref()
            .and_then(Value::as_int)
            .expect("the base's tag word");
        assert_eq!(
            word as u32,
            tag << 8,
            "{name}: the tag goes in bits 8..12 and nothing else in the word moves"
        );

        let built = build(&pack, &blank).expect("a blank variant builds");
        let base = (built.root - or_io::container::paging::FIXED_VIRTUAL_BASE) as usize;
        assert_eq!(u32_in(&built.virtual_data, base), tag << 8);
    }
}

#[test]
fn a_tag_that_selects_another_layout_is_refused() {
    let pack = tagged();

    let mut large = value::empty(&pack, "Large").expect("Large");
    build(&pack, &large).expect("the blank as authored must build");

    set_tag(&mut large, "base.word", 1 << 8);
    let error = build(&pack, &large).expect_err("a Large tagged as a Small");

    assert_eq!(
        error,
        BuildError::TagDisagrees {
            type_name: "Large".into(),
            base: "Base".into(),
            tag: 1,
            resolves_to: "Small".into(),
        }
    );
}

#[test]
fn a_tag_nobody_described_is_refused_for_a_variant_and_accepted_for_the_base() {
    let pack = tagged();

    let mut large = value::empty(&pack, "Large").expect("Large");
    set_tag(&mut large, "base.word", 9 << 8);
    let error = build(&pack, &large).expect_err("a Large tagged as nothing");
    assert!(
        matches!(&error, BuildError::TagDisagrees { resolves_to, .. } if resolves_to == "Base"),
        "{error}"
    );

    let mut base = value::empty(&pack, "Base").expect("Base");
    set_tag(&mut base, "word", 9 << 8);
    build(&pack, &base).expect("a base whose tag names no variant is an ordinary base");
}

#[test]
fn a_blank_base_whose_zero_tag_selects_a_variant_is_refused() {
    let mut pack = tagged();
    pack.types
        .iter_mut()
        .find(|t| t.name == "Base")
        .and_then(|t| t.selector.as_mut())
        .expect("Base has a selector")
        .variants
        .push((0, "Small".into()));

    let blank = value::empty(&pack, "Base").expect("Base");
    let error = build(&pack, &blank).expect_err("a bare Base whose tag says Small");
    assert!(
        matches!(&error, BuildError::TagDisagrees { type_name, resolves_to, .. }
            if type_name == "Base" && resolves_to == "Small"),
        "{error}"
    );
}

#[test]
fn a_tag_inside_a_field_embedded_by_value_is_not_checked() {
    let mut pack = tagged();
    pack.types
        .iter_mut()
        .find(|t| t.name == "Base")
        .and_then(|t| t.selector.as_mut())
        .expect("Base has a selector")
        .variants
        .push((3, "Largest".into()));

    let blank = value::empty(&pack, "Largest").expect("Largest");

    assert_eq!(
        blank
            .get("large.base.word")
            .as_ref()
            .and_then(Value::as_int),
        Some(3 << 8)
    );

    let built = build(&pack, &blank).expect("an embedded tag is the enclosing one's");
    assert_eq!(
        built.virtual_data.len()
            - (built.root - or_io::container::paging::FIXED_VIRTUAL_BASE) as usize,
        0x30,
        "and the allocation is the outer type's size"
    );
}

#[test]
fn a_type_the_pack_does_not_describe_is_an_error_rather_than_a_blank() {
    let pack = menagerie();
    let error = value::empty(&pack, "Nonexistent").expect_err("no such type");
    assert!(error.to_string().contains("Nonexistent"), "{error}");
}

#[test]
fn a_scalar_too_wide_for_its_field_is_refused_when_authored_too() {
    let pack = menagerie();
    let mut root = value::empty(&pack, "Root").expect("Root");
    let record = root.record_mut().expect("a struct");

    // ⚠ A record holds a scalar as the field's own bytes, so a value that does
    // not fit is refused at the write rather than at the build.
    let error = record
        .set_structural("count", ScalarValue::Int(70_000))
        .expect_err("70000 does not fit a u16");
    assert!(
        matches!(
            error,
            RecordError::Write {
                error: WriteError::DoesNotFit { .. },
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(
        record.int("count"),
        Some(0),
        "and the refused write left the field alone"
    );
    build(&pack, &root).expect("so what reaches the builder is still a valid blank");
}
