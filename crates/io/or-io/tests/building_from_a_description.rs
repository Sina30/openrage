use or_io::container::paging::{PgRscBuilder, FIXED_PHYSICAL_BASE, FIXED_VIRTUAL_BASE};
use or_io::desc::value::ScalarValue;
use or_io::desc::TypeName;
use or_io::desc::{
    AddressSpace, CountSource, FieldRepr, Image, ImageMut, IntWidth, Pack, PackEnum, PackField,
    PackSelector, PackType, PackVariant, Pointee, RecordError, Scalar, Value, WriteError,
};
use or_io::write::{build, patch, read, read_with, Edit, ElementTypes, PatchError};

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

fn physical(mut field: PackField) -> PackField {
    field.space = Some(AddressSpace::Physical);
    field
}

fn packed(mut field: PackField) -> PackField {
    field.bits.push(or_io::desc::BitRange {
        name: "primitive_type".into(),
        low: 0,
        width: 3,
        note: None,
    });
    field
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

struct Bytes(Vec<u8>);

impl Bytes {
    fn new(len: usize) -> Self {
        Self(vec![0u8; len])
    }
    fn put(&mut self, at: usize, data: &[u8]) -> &mut Self {
        self.0[at..at + data.len()].copy_from_slice(data);
        self
    }
    fn ptr(&mut self, at: usize, target: usize) -> &mut Self {
        self.put(at, &(FIXED_VIRTUAL_BASE | target as u64).to_le_bytes())
    }
    fn image(&self) -> Image<'_> {
        Image::new(&self.0, self.0.len())
    }
}

fn at(offset: usize) -> u64 {
    FIXED_VIRTUAL_BASE + offset as u64
}

fn pointer_in(heap: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(heap[at..at + 8].try_into().expect("eight bytes"))
}

#[test]
fn a_pointer_allocates_before_an_embedded_structs_children() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x20,
        vec![
            field(
                "inner",
                0x00,
                FieldRepr::Inline {
                    type_name: "Inner".into(),
                },
            ),
            field(
                "tail",
                0x10,
                FieldRepr::Ptr(Pointee::One {
                    type_name: "Leaf".into(),
                }),
            ),
            field("pad", 0x18, FieldRepr::Padding { size: 8 }),
        ],
    ));
    pack.types.push(ty(
        "Inner",
        0x10,
        vec![
            field(
                "items",
                0x00,
                FieldRepr::Ptr(Pointee::Many {
                    type_name: Some("Leaf".into()),
                    count: CountSource::Field("n".into()),
                    element_size: None,
                }),
            ),
            field("n", 0x08, int(IntWidth::U32)),
        ],
    ));
    pack.types.push(ty(
        "Leaf",
        0x08,
        vec![field("value", 0x00, int(IntWidth::U32))],
    ));

    let mut bytes = Bytes::new(0x200);
    bytes.ptr(0x00, 0x100);
    bytes.put(0x08, &1u32.to_le_bytes());
    bytes.ptr(0x10, 0x180);
    bytes.put(0x100, &11u32.to_le_bytes());
    bytes.put(0x180, &22u32.to_le_bytes());

    let value = read(&pack, TypeName("Root"), bytes.image(), 0).expect("read");
    let built = build(&pack, &value).expect("build");
    let heap = &built.virtual_data;

    assert_eq!(
        pointer_in(heap, 0x10),
        at(0x20),
        "tail's target comes first"
    );
    assert_eq!(
        pointer_in(heap, 0x00),
        at(0x28),
        "the embedded struct's run comes after it"
    );
    assert_eq!(heap.len(), 0x30);
    assert_eq!(&heap[0x20..0x24], &22u32.to_le_bytes());
    assert_eq!(&heap[0x28..0x2C], &11u32.to_le_bytes());
}

#[test]
fn a_structs_body_is_allocated_before_its_children() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x10,
        vec![
            field("name", 0x00, FieldRepr::Ptr(Pointee::Str)),
            field("pad", 0x08, FieldRepr::Padding { size: 8 }),
        ],
    ));

    let mut bytes = Bytes::new(0x100);
    bytes.ptr(0x00, 0x40);
    bytes.put(0x40, b"lod0\0");

    let value = read(&pack, TypeName("Root"), bytes.image(), 0).expect("read");
    let built = build(&pack, &value).expect("build");

    assert_eq!(pointer_in(&built.virtual_data, 0), at(0x10));
    assert_eq!(&built.virtual_data[0x10..0x15], b"lod0\0");
    assert_eq!(built.virtual_data.len(), 0x18);
}

#[test]
fn the_pack_decides_which_heap_a_payload_goes_in() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Texture",
        0x18,
        vec![
            physical(field(
                "pixel_data",
                0x00,
                FieldRepr::Ptr(Pointee::Bytes {
                    length: CountSource::Fixed(6),
                }),
            )),
            field(
                "bone_names",
                0x08,
                FieldRepr::Ptr(Pointee::Bytes {
                    length: CountSource::Fixed(6),
                }),
            ),
            field("pad", 0x10, FieldRepr::Padding { size: 8 }),
        ],
    ));

    let mut bytes = Bytes::new(0x100);
    bytes.ptr(0x00, 0x40);
    bytes.ptr(0x08, 0x50);
    bytes.put(0x40, &[1, 2, 3, 4, 5, 6]);
    bytes.put(0x50, &[7, 8, 9, 10, 11, 12]);

    let value = read(&pack, TypeName("Texture"), bytes.image(), 0).expect("read");
    let built = build(&pack, &value).expect("build");

    assert_eq!(
        pointer_in(&built.virtual_data, 0x00),
        FIXED_PHYSICAL_BASE,
        "pixels belong in graphics memory"
    );
    assert_eq!(
        pointer_in(&built.virtual_data, 0x08),
        at(0x18),
        "bone names belong in system memory, right after the struct"
    );
    assert_eq!(&built.physical_data[..6], &[1, 2, 3, 4, 5, 6]);
    assert_eq!(&built.virtual_data[0x18..0x1E], &[7, 8, 9, 10, 11, 12]);

    assert_eq!(built.physical_data.len(), 128);

    assert_eq!(built.physical_pointers.len(), 1);
    assert_eq!(built.physical_pointers[0].write_offset, 0x00);
    assert_eq!(built.virtual_pointers.len(), 1);
    assert_eq!(built.virtual_pointers[0].write_offset, 0x08);
}

#[test]
fn a_physical_blob_is_padded_and_a_physical_run_is_not() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Buffers",
        0x18,
        vec![
            physical(field(
                "indices",
                0x00,
                FieldRepr::Ptr(Pointee::Many {
                    type_name: None,
                    count: CountSource::Fixed(4),
                    element_size: Some(2),
                }),
            )),
            physical(field(
                "pixel_data",
                0x08,
                FieldRepr::Ptr(Pointee::Bytes {
                    length: CountSource::Fixed(8),
                }),
            )),
            field("pad", 0x10, FieldRepr::Padding { size: 8 }),
        ],
    ));

    let mut bytes = Bytes::new(0x100);
    bytes.ptr(0x00, 0x40);
    bytes.ptr(0x08, 0x50);
    bytes.put(0x40, &[1, 2, 3, 4, 5, 6, 7, 8]);
    bytes.put(0x50, &[9, 10, 11, 12, 13, 14, 15, 16]);

    let value = read(&pack, TypeName("Buffers"), bytes.image(), 0).expect("read");
    let built = build(&pack, &value).expect("build");

    assert_eq!(pointer_in(&built.virtual_data, 0x00), FIXED_PHYSICAL_BASE);
    assert_eq!(&built.physical_data[..8], &[1, 2, 3, 4, 5, 6, 7, 8]);

    assert_eq!(
        pointer_in(&built.virtual_data, 0x08),
        FIXED_PHYSICAL_BASE + 256
    );
    assert_eq!(
        &built.physical_data[256..264],
        &[9, 10, 11, 12, 13, 14, 15, 16]
    );
    assert_eq!(built.physical_data.len(), 256 + 128);
}

#[test]
fn an_at_arrays_capacity_survives_a_rebuild() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Composite",
        0x10,
        vec![field(
            "children",
            0x00,
            FieldRepr::AtArray(Pointee::Many {
                type_name: Some("Item".into()),
                count: CountSource::Inline,
                element_size: None,
            }),
        )],
    ));
    pack.types.push(ty(
        "Item",
        0x04,
        vec![field("id", 0x00, int(IntWidth::U32))],
    ));

    let mut bytes = Bytes::new(0x100);
    bytes.ptr(0x00, 0x40);
    bytes.put(0x08, &2u16.to_le_bytes());
    bytes.put(0x0A, &6u16.to_le_bytes());
    bytes.put(0x40, &11u32.to_le_bytes());
    bytes.put(0x44, &22u32.to_le_bytes());

    let value = read(&pack, TypeName("Composite"), bytes.image(), 0).expect("read");
    let built = build(&pack, &value).expect("build");

    assert_eq!(&built.virtual_data[0x08..0x0A], &2u16.to_le_bytes());
    assert_eq!(
        &built.virtual_data[0x0A..0x0C],
        &6u16.to_le_bytes(),
        "the capacity is the file's, not the element count"
    );
}

#[test]
fn an_untyped_runs_count_is_in_elements_and_not_in_bytes() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Dictionary",
        0x10,
        vec![field(
            "codes",
            0x00,
            FieldRepr::AtArray(Pointee::Many {
                type_name: None,
                count: CountSource::Inline,
                element_size: Some(4),
            }),
        )],
    ));

    let mut bytes = Bytes::new(0x100);
    bytes.ptr(0x00, 0x40);
    bytes.put(0x08, &3u16.to_le_bytes());
    bytes.put(0x0A, &3u16.to_le_bytes());
    bytes.put(0x40, &7u32.to_le_bytes());
    bytes.put(0x44, &8u32.to_le_bytes());
    bytes.put(0x48, &9u32.to_le_bytes());

    let value = read(&pack, TypeName("Dictionary"), bytes.image(), 0).expect("read");
    let built = build(&pack, &value).expect("build");

    assert_eq!(
        &built.virtual_data[0x08..0x0A],
        &3u16.to_le_bytes(),
        "three four-byte codes is a count of three, not of twelve"
    );
    let run = pointer_in(&built.virtual_data, 0x00) - FIXED_VIRTUAL_BASE;
    let run = run as usize;
    assert_eq!(&built.virtual_data[run..run + 4], &7u32.to_le_bytes());
    assert_eq!(&built.virtual_data[run + 8..run + 12], &9u32.to_le_bytes());
}

// A fixup on a field holding a non-address would ask the resource layer to
// rebase a number that is not an address.
#[test]
fn a_pointer_field_holding_something_else_is_carried_without_a_fixup() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Thing",
        0x10,
        vec![
            field("name", 0x00, FieldRepr::Ptr(Pointee::Str)),
            field("id", 0x08, int(IntWidth::U32)),
        ],
    ));

    let mut bytes = Bytes::new(0x100);
    bytes.put(0x00, &0x210u64.to_le_bytes());
    bytes.put(0x08, &5u32.to_le_bytes());

    let value = read(&pack, TypeName("Thing"), bytes.image(), 0).expect("read");
    assert_eq!(
        value.child("name"),
        Some(&Value::NotAnAddress(0x210)),
        "neither a null nor an error"
    );

    let built = build(&pack, &value).expect("build");
    assert_eq!(
        pointer_in(&built.virtual_data, 0x00),
        0x210,
        "the bytes go back as they were"
    );
    assert!(
        built.virtual_pointers.is_empty() && built.physical_pointers.is_empty(),
        "a value that is not an address must not be registered for rebasing"
    );
    assert_eq!(
        built.report.carried.len(),
        1,
        "and it is reported, so the count is visible rather than assumed"
    );
    assert_eq!(built.report.carried[0].field, "name");
}

/// A non-address in a **fixed slot array** is carried as it is in a field.
///
/// ⚠ Refusing it here with a `Mismatch` left 21 of 48 `.ydd` files unbuildable:
/// `RmcLodGroup::lods` holds a small integer in a slot exactly as
/// `RmcDrawable::name` does in a field.
#[test]
fn a_slot_of_a_fixed_array_holding_something_else_is_carried_too() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Group",
        0x20,
        vec![field(
            "lods",
            0x00,
            FieldRepr::PtrSlots {
                pointee: Pointee::One {
                    type_name: "Lod".into(),
                },
                count: 4,
            },
        )],
    ));
    pack.types.push(ty(
        "Lod",
        0x08,
        vec![field("value", 0x00, int(IntWidth::U32))],
    ));

    let mut bytes = Bytes::new(0x200);
    bytes.ptr(0x00, 0x100);
    bytes.put(0x08, &0x210u64.to_le_bytes());
    bytes.ptr(0x18, 0x110);
    bytes.put(0x100, &7u32.to_le_bytes());
    bytes.put(0x110, &9u32.to_le_bytes());

    let value = read(&pack, TypeName("Group"), bytes.image(), 0).expect("read");
    let built = build(&pack, &value).expect("a non-address in a slot must not refuse the build");
    let heap = &built.virtual_data;

    assert_eq!(
        pointer_in(heap, 0x08),
        0x210,
        "the slot's bytes go back as they were"
    );
    assert_eq!(pointer_in(heap, 0x10), 0, "and the null slot stays null");
    assert_eq!(
        built.report.carried.len(),
        1,
        "the carried slot is reported once"
    );
    assert_eq!(built.report.carried[0].field, "lods");

    // A fixup on the carried slot would ask the resource layer to rebase a
    // number that is not an address.
    let fixed: Vec<u32> = built
        .virtual_pointers
        .iter()
        .map(|f| f.write_offset)
        .collect();
    assert_eq!(
        fixed,
        vec![0x00, 0x18],
        "only the two addresses are fixed up"
    );
    let first = (pointer_in(heap, 0x00) - FIXED_VIRTUAL_BASE) as usize;
    let last = (pointer_in(heap, 0x18) - FIXED_VIRTUAL_BASE) as usize;
    assert_eq!(&heap[first..first + 4], &7u32.to_le_bytes());
    assert_eq!(&heap[last..last + 4], &9u32.to_le_bytes());
}

fn composite_pack() -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Composite",
        0x20,
        vec![
            field(
                "children",
                0x00,
                FieldRepr::Ptr(Pointee::ManyPtr {
                    type_name: Some("Child".into()),
                    count: CountSource::CountAndCapacity {
                        count: "n".into(),
                        capacity: "cap".into(),
                    },
                }),
            ),
            field(
                "matrices",
                0x08,
                FieldRepr::Ptr(Pointee::Many {
                    type_name: Some("Child".into()),
                    count: CountSource::Field("n".into()),
                    element_size: None,
                }),
            ),
            field("n", 0x10, int(IntWidth::U32)),
            field("cap", 0x14, int(IntWidth::U32)),
            field("pad", 0x18, FieldRepr::Padding { size: 8 }),
        ],
    ));
    pack.types.push(ty(
        "Child",
        0x08,
        vec![
            field("id", 0x00, int(IntWidth::U32)),
            field("pad", 0x04, FieldRepr::Padding { size: 4 }),
        ],
    ));
    pack
}

fn composite_bytes() -> Bytes {
    let mut b = Bytes::new(0x400);
    b.ptr(0x00, 0x100);
    b.ptr(0x08, 0x140);
    b.put(0x10, &3u32.to_le_bytes());
    b.put(0x14, &5u32.to_le_bytes());

    b.ptr(0x100, 0x180);
    b.put(0x108, &[0u8; 8]);
    b.ptr(0x110, 0x190);

    b.put(0x180, &11u32.to_le_bytes());
    b.put(0x190, &22u32.to_le_bytes());

    b.put(0x140, &1u32.to_le_bytes());
    b.put(0x148, &2u32.to_le_bytes());
    b.put(0x150, &3u32.to_le_bytes());
    b
}

#[test]
fn a_pointer_table_and_a_value_run_over_one_count_read_differently() {
    let pack = composite_pack();
    let bytes = composite_bytes();
    let value = read(&pack, TypeName("Composite"), bytes.image(), 0).expect("read");

    let Some(Value::Ptr {
        target: Some(children),
        ..
    }) = value.child("children")
    else {
        panic!("children should be a populated pointer");
    };
    let Value::Array { elements, .. } = children.as_ref() else {
        panic!("children should point at an array");
    };
    assert_eq!(elements.len(), 3, "the count says three slots");

    assert!(matches!(elements[1], Value::Ptr { target: None, .. }));
    let ids: Vec<Option<i128>> = elements
        .iter()
        .map(|e| match e {
            Value::Ptr {
                target: Some(child),
                ..
            } => child.get("id").as_ref().and_then(Value::as_int),
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec![Some(11), None, Some(22)]);

    let Some(Value::Ptr {
        target: Some(matrices),
        ..
    }) = value.child("matrices")
    else {
        panic!("matrices should be a populated pointer");
    };
    let Value::Records(run) = matrices.as_ref() else {
        panic!("matrices hold no pointer, so they should be one packed run");
    };
    let ids: Vec<Option<i128>> = run.iter().map(|e| e.int("id")).collect();
    assert_eq!(ids, vec![Some(1), Some(2), Some(3)]);
}

#[test]
fn a_pointer_table_is_rebuilt_with_its_children_before_it() {
    let pack = composite_pack();
    let bytes = composite_bytes();
    let value = read(&pack, TypeName("Composite"), bytes.image(), 0).expect("read");
    let built = build(&pack, &value).expect("build");
    let heap = &built.virtual_data;

    assert_eq!(pointer_in(heap, 0x00), at(0x38), "children -> the table");
    assert_eq!(pointer_in(heap, 0x08), at(0x50), "matrices -> the run");

    assert_eq!(pointer_in(heap, 0x38), at(0x20), "slot 0");
    assert_eq!(pointer_in(heap, 0x40), 0, "slot 1 stays empty");
    assert_eq!(pointer_in(heap, 0x48), at(0x30), "slot 2");

    assert_eq!(&heap[0x20..0x24], &11u32.to_le_bytes());
    assert_eq!(&heap[0x30..0x34], &22u32.to_le_bytes());
    assert_eq!(&heap[0x50..0x54], &1u32.to_le_bytes());
    assert_eq!(&heap[0x58..0x5C], &2u32.to_le_bytes());
    assert_eq!(&heap[0x60..0x64], &3u32.to_le_bytes());
    assert_eq!(heap.len(), 0x68);

    assert_eq!(built.virtual_pointers.len(), 4);
    assert!(built.report.dropped.is_empty());
}

#[test]
fn a_pointer_table_survives_a_read_build_read() {
    let pack = composite_pack();
    let bytes = composite_bytes();
    let before = read(&pack, TypeName("Composite"), bytes.image(), 0).expect("read");
    let built = build(&pack, &before).expect("build");

    let rebuilt = Image::new(&built.virtual_data, built.virtual_data.len());
    let after = read(&pack, TypeName("Composite"), rebuilt, 0).expect("re-read");

    assert_eq!(renumber(&before), renumber(&after));
}

fn renumber(value: &Value) -> Value {
    fn go(value: &Value, seen: &mut std::collections::HashMap<u64, u64>) -> Value {
        let mut ordinal = |address: u64| -> u64 {
            let next = seen.len() as u64 + 1;
            *seen.entry(address).or_insert(next)
        };
        match value {
            Value::Ptr { source, target } => Value::Ptr {
                source: if *source == 0 { 0 } else { ordinal(*source) },
                target: target.as_ref().map(|t| Box::new(go(t, seen))),
            },
            Value::Alias(address) => Value::Alias(ordinal(*address)),
            // ⚠ A record's bytes hold its pointer slots, which a rebuild
            // reassigns; only the ranges a build copies are content.
            Value::Struct(record) => {
                let mut out = record.clone();
                let bytes = out.bytes_mut();
                bytes.fill(0);
                for &(start, end) in record.plan().copy_ranges() {
                    bytes[start..end].copy_from_slice(&record.bytes()[start..end]);
                }
                for (_, child) in out.children_mut() {
                    *child = go(child, seen);
                }
                Value::Struct(out)
            }
            Value::Array { elements, capacity } => Value::Array {
                elements: elements.iter().map(|e| go(e, seen)).collect(),
                capacity: *capacity,
            },
            Value::Records(run) => {
                let mut out = run.clone();
                let stride = out.stride();
                let ranges = run.plan().copy_ranges().to_vec();
                for (element, source) in out
                    .bytes_mut()
                    .chunks_exact_mut(stride)
                    .zip(run.bytes().chunks_exact(stride))
                {
                    element.fill(0);
                    for &(start, end) in &ranges {
                        element[start..end].copy_from_slice(&source[start..end]);
                    }
                }
                Value::Records(out)
            }
            other => other.clone(),
        }
    }
    go(value, &mut Default::default())
}

#[test]
fn a_tagged_base_allocates_at_the_concrete_size() {
    let mut base = ty(
        "Bound",
        0x10,
        vec![field("bound_type", 0x00, int(IntWidth::U8))],
    );
    base.selector = Some(PackSelector {
        tag: "bound_type".into(),
        tag_bits: None,
        variants: vec![(4, "BoundGeometry".into())],
    });

    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(base);
    pack.types.push(ty(
        "BoundGeometry",
        0x20,
        vec![
            field(
                "header",
                0x00,
                FieldRepr::Inline {
                    type_name: "Bound".into(),
                },
            ),
            field("margin", 0x10, FieldRepr::Scalar(Scalar::F32)),
            field("pad", 0x14, FieldRepr::Padding { size: 12 }),
        ],
    ));

    let mut bytes = Bytes::new(0x100);
    bytes.put(0x00, &[4]);
    bytes.put(0x10, &0.04f32.to_le_bytes());

    let value = read(&pack, TypeName("Bound"), bytes.image(), 0).expect("read");
    assert_eq!(value.type_name(), Some("BoundGeometry"));

    let built = build(&pack, &value).expect("build");
    assert_eq!(
        built.virtual_data.len(),
        0x20,
        "0x20 for the concrete layout, not 0x10 for the base"
    );
    assert_eq!(&built.virtual_data[0x10..0x14], &0.04f32.to_le_bytes());
}

fn trailing_pack(element: Option<&str>, element_size: Option<usize>) -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(PackType {
        name: "Root".into(),
        size: 0x10,
        fields: vec![
            field("vft", 0x00, int(IntWidth::U64)),
            field("n", 0x08, int(IntWidth::U32)),
            field("pad", 0x0C, FieldRepr::Padding { size: 4 }),
            field(
                "run",
                0x10,
                FieldRepr::Trailing {
                    type_name: element.map(str::to_string),
                    count: CountSource::Field("n".into()),
                    element_size,
                },
            ),
        ],
        selector: None,
    });
    pack.types.push(ty(
        "Element",
        0x08,
        vec![
            field("a", 0x00, int(IntWidth::U32)),
            field("b", 0x04, int(IntWidth::U32)),
        ],
    ));
    pack
}

#[test]
fn a_trailing_run_of_described_elements_is_read_and_rebuilt() {
    let pack = trailing_pack(Some("Element"), None);

    let mut bytes = Bytes::new(0x40);
    bytes.put(0x00, &0x1400_0000u64.to_le_bytes());
    bytes.put(0x08, &3u32.to_le_bytes());
    for i in 0..3usize {
        bytes.put(0x10 + i * 8, &(10 + i as u32).to_le_bytes());
        bytes.put(0x14 + i * 8, &(20 + i as u32).to_le_bytes());
    }

    let value = read(&pack, TypeName("Root"), bytes.image(), 0).expect("read");

    let run = value.child("run").expect("run");
    let Value::Records(run) = run else {
        panic!("a packed run, got {run:?}");
    };
    assert_eq!(run.len(), 3);
    assert_eq!(run.get(2).and_then(|e| e.int("b")), Some(22));

    let built = build(&pack, &value).expect("build");
    assert_eq!(
        built.virtual_data.len(),
        0x10 + 3 * 0x08,
        "the allocation is the header plus the run"
    );
    assert_eq!(&built.virtual_data[..0x28], &bytes.0[..0x28]);
}

#[test]
fn a_packed_run_and_the_same_run_of_records_build_the_same_bytes() {
    let pack = trailing_pack(Some("Element"), None);

    let mut bytes = Bytes::new(0x40);
    bytes.put(0x08, &3u32.to_le_bytes());
    for i in 0..3usize {
        bytes.put(0x10 + i * 8, &(10 + i as u32).to_le_bytes());
        bytes.put(0x14 + i * 8, &(20 + i as u32).to_le_bytes());
    }
    let packed = read(&pack, TypeName("Root"), bytes.image(), 0).expect("read");
    let Some(Value::Records(run)) = packed.child("run") else {
        panic!("a packed run");
    };
    let mut unpacked = packed.clone();
    unpacked
        .record_mut()
        .expect("a struct")
        .set_child(
            "run",
            Value::Array {
                elements: run.records().into_iter().map(Value::Struct).collect(),
                capacity: None,
            },
        )
        .expect("a trailing field");

    assert!(
        or_io::write::compare::same_value(&packed, &unpacked),
        "the comparison takes either"
    );
    assert_eq!(
        build(&pack, &packed).expect("build").virtual_data,
        build(&pack, &unpacked).expect("build").virtual_data
    );
}

#[test]
fn a_trailing_run_of_undescribed_bytes_is_carried_whole() {
    let pack = trailing_pack(None, Some(1));

    let mut bytes = Bytes::new(0x40);
    bytes.put(0x08, &9u32.to_le_bytes());
    bytes.put(
        0x10,
        &[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04, 0x05],
    );

    let value = read(&pack, TypeName("Root"), bytes.image(), 0).expect("read");
    let built = build(&pack, &value).expect("build");

    assert_eq!(built.virtual_data.len(), 0x10 + 9);
    assert_eq!(&built.virtual_data[0x10..0x19], &bytes.0[0x10..0x19]);
}

#[test]
fn withholding_the_description_writes_the_header_and_drops_the_run() {
    let described = trailing_pack(Some("Element"), None);
    let mut withheld = described.clone();
    withheld.types[0].fields.retain(|f| !f.repr.is_trailing());

    let mut bytes = Bytes::new(0x40);
    bytes.put(0x08, &3u32.to_le_bytes());
    for i in 0..3usize {
        bytes.put(0x10 + i * 8, &(10 + i as u32).to_le_bytes());
    }

    let with = build(
        &described,
        &read(&described, TypeName("Root"), bytes.image(), 0).expect("read"),
    )
    .expect("build");
    let without = build(
        &withheld,
        &read(&withheld, TypeName("Root"), bytes.image(), 0).expect("read"),
    )
    .expect("build");

    assert_eq!(with.virtual_data.len(), 0x28);
    assert_eq!(without.virtual_data.len(), 0x10);
    assert_eq!(&without.virtual_data[0x08..0x0C], &3u32.to_le_bytes());
}

#[test]
fn a_trailing_run_whose_count_disagrees_is_refused() {
    let pack = trailing_pack(Some("Element"), None);

    let mut bytes = Bytes::new(0x40);
    bytes.put(0x08, &2u32.to_le_bytes());

    let mut value = read(&pack, TypeName("Root"), bytes.image(), 0).expect("read");
    let run = value.child_mut("run").expect("run");
    let Value::Records(run) = run else {
        panic!("a packed run");
    };
    let first = run.record(0).expect("two elements");
    run.push(&first).expect("the run's own type");

    let error = build(&pack, &value).expect_err("a run of three beside a count of two");
    assert!(
        error.to_string().contains("run"),
        "the message should name the field: {error}"
    );
}

#[test]
fn building_a_trailing_type_by_value_is_refused() {
    let mut pack = trailing_pack(Some("Element"), None);
    pack.types.push(ty(
        "Careless",
        0x10,
        vec![field(
            "inner",
            0x00,
            FieldRepr::Inline {
                type_name: "Root".into(),
            },
        )],
    ));

    let mut bytes = Bytes::new(0x40);
    bytes.put(0x08, &1u32.to_le_bytes());

    let value = read(&pack, TypeName("Careless"), bytes.image(), 0).expect("read");
    let error = build(&pack, &value).expect_err("an embedded type that trails a run");
    assert!(
        error.to_string().contains("size is still open"),
        "the message should say why: {error}"
    );
}

#[test]
fn an_authored_trailing_type_starts_empty() {
    let pack = trailing_pack(Some("Element"), None);
    let value = or_io::desc::value::empty(&pack, "Root").expect("empty");
    let built = build(&pack, &value).expect("build");

    assert_eq!(built.virtual_data.len(), 0x10);
    assert_eq!(&built.virtual_data[0x08..0x0C], &0u32.to_le_bytes());
}

#[test]
fn undescribed_regions_are_carried_verbatim() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x10,
        vec![
            field("known", 0x00, int(IntWidth::U32)),
            field("gap", 0x04, FieldRepr::Padding { size: 4 }),
            field("mystery", 0x08, FieldRepr::Opaque { size: 8 }),
        ],
    ));

    let mut bytes = Bytes::new(0x40);
    bytes.put(0x00, &7u32.to_le_bytes());
    bytes.put(0x04, &[0xA1, 0xA2, 0xA3, 0xA4]);
    bytes.put(0x08, &[0xC0, 0xFF, 0xEE, 0x01, 0x02, 0x03, 0x04, 0x05]);

    let value = read(&pack, TypeName("Root"), bytes.image(), 0).expect("read");
    let built = build(&pack, &value).expect("build");

    assert_eq!(&built.virtual_data[..0x10], &bytes.0[..0x10]);
}

#[test]
fn an_undescribed_pointer_is_nulled_and_reported() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x10,
        vec![
            field("runtime_slot", 0x00, FieldRepr::Ptr(Pointee::Opaque)),
            field("pad", 0x08, FieldRepr::Padding { size: 8 }),
        ],
    ));

    let mut bytes = Bytes::new(0x100);
    bytes.ptr(0x00, 0x40);

    let value = read(&pack, TypeName("Root"), bytes.image(), 0).expect("read");
    let built = build(&pack, &value).expect("build");

    assert_eq!(pointer_in(&built.virtual_data, 0x00), 0);
    let dropped: Vec<_> = built.report.dropped_non_null().collect();
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].field, "runtime_slot");
    assert_eq!(dropped[0].address, FIXED_VIRTUAL_BASE | 0x40);
}

#[test]
fn a_value_that_does_not_fit_is_refused() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x04,
        vec![field("small", 0x00, int(IntWidth::U16))],
    ));

    // ⚠ A record holds a scalar as the field's own bytes, so there is no
    // over-wide value for a build to meet: the write into the record refuses it.
    let mut value = or_io::desc::value::empty(&pack, "Root").expect("Root");
    let record = value.record_mut().expect("a struct");
    let err = record
        .set_scalar("small", ScalarValue::Int(70_000))
        .expect_err("70000 does not fit a u16");
    assert!(
        matches!(
            err,
            RecordError::Write {
                error: WriteError::DoesNotFit { .. },
                ..
            }
        ),
        "{err}"
    );
    assert!(
        err.to_string().contains("small") && err.to_string().contains("U16"),
        "the message should name the field and its width: {err}"
    );
    assert_eq!(record.int("small"), Some(0), "and nothing was written");
}

// The interpreter writes into a builder's heap and never calls its bucket packer.
#[test]
fn nothing_here_reaches_the_bucket_packer() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x04,
        vec![field("value", 0x00, int(IntWidth::U32))],
    ));

    let mut value = or_io::desc::value::empty(&pack, "Root").expect("Root");
    value
        .record_mut()
        .expect("a struct")
        .set_scalar("value", ScalarValue::Int(9))
        .expect("value");

    let mut builder = PgRscBuilder::new_bucketed_default();
    let root = or_io::write::build_into(builder.heap_mut(), &pack, &value).expect("build");
    assert_eq!(root, FIXED_VIRTUAL_BASE);
    assert_eq!(builder.heap().virtual_data().len(), 4);

    let (virtual_buckets, _) = builder.ready_build();
    assert!(
        virtual_buckets > 0,
        "the builder still packs; only the interpreter's reach was narrowed"
    );
}

fn patchable() -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Geometry",
        0x20,
        vec![
            field(
                "vertices",
                0x00,
                FieldRepr::Ptr(Pointee::Many {
                    type_name: None,
                    count: CountSource::Field("num_vertices".into()),
                    element_size: Some(2),
                }),
            ),
            field("num_vertices", 0x08, int(IntWidth::U32)),
            field("margin", 0x0C, FieldRepr::Scalar(Scalar::F32)),
            packed(field("area", 0x10, FieldRepr::Scalar(Scalar::F32))),
            field(
                "primitive",
                0x14,
                FieldRepr::Enum {
                    type_name: "Primitive".into(),
                    width: IntWidth::U8,
                },
            ),
            field("rest", 0x15, FieldRepr::Padding { size: 11 }),
        ],
    ));
    pack.enums.push(PackEnum {
        name: "Primitive".into(),
        width: IntWidth::U8,
        variants: vec![
            PackVariant {
                name: "Tris".into(),
                value: 4,
                note: None,
            },
            PackVariant {
                name: "TriStrip".into(),
                value: 5,
                note: None,
            },
        ],
    });
    pack
}

#[test]
fn a_scalar_is_written_and_nothing_else_moves() {
    let pack = patchable();
    let mut bytes = vec![0u8; 0x40];
    bytes[0x0C..0x10].copy_from_slice(&1.0f32.to_le_bytes());
    let before = bytes.clone();

    let mut image = ImageMut::new(&mut bytes, 0x40);
    patch(
        &pack,
        "Geometry",
        &mut image,
        0,
        &[Edit::new("margin", ScalarValue::F32(0.25))],
    )
    .expect("margin is ordinary data");

    assert_eq!(&bytes[0x0C..0x10], &0.25f32.to_le_bytes());
    let mut expected = before;
    expected[0x0C..0x10].copy_from_slice(&0.25f32.to_le_bytes());
    assert_eq!(bytes, expected);
}

#[test]
fn a_count_a_packed_field_and_a_pointer_are_all_refused() {
    let pack = patchable();
    let mut bytes = vec![0u8; 0x40];
    let untouched = bytes.clone();
    let mut image = ImageMut::new(&mut bytes, 0x40);

    type Case = (&'static str, ScalarValue, fn(&PatchError) -> bool);

    let cases: [Case; 3] = [
        ("num_vertices", ScalarValue::Int(3), |e| {
            matches!(e, PatchError::Structural { .. })
        }),
        ("area", ScalarValue::F32(1.0), |e| {
            matches!(e, PatchError::BitPacked { .. })
        }),
        ("vertices", ScalarValue::Int(0), |e| {
            matches!(e, PatchError::NotAScalar { .. })
        }),
    ];

    for (name, value, expected) in cases {
        let err = patch(&pack, "Geometry", &mut image, 0, &[Edit::new(name, value)])
            .expect_err("should refuse");
        assert!(expected(&err), "{name}: unexpected {err:?}");
    }

    assert_eq!(bytes, untouched, "a refused edit writes nothing");
}

#[test]
fn an_enum_may_only_be_set_to_a_value_the_pack_declares() {
    let pack = patchable();
    let mut bytes = vec![0u8; 0x40];
    let mut image = ImageMut::new(&mut bytes, 0x40);

    patch(
        &pack,
        "Geometry",
        &mut image,
        0,
        &[Edit::new("primitive", ScalarValue::Int(5))],
    )
    .expect("TriStrip is declared");
    assert_eq!(bytes[0x14], 5);

    let mut image = ImageMut::new(&mut bytes, 0x40);
    let err = patch(
        &pack,
        "Geometry",
        &mut image,
        0,
        &[Edit::new("primitive", ScalarValue::Int(9))],
    )
    .expect_err("9 is not declared");
    assert!(matches!(
        err,
        PatchError::NotADeclaredValue { value: 9, .. }
    ));
    assert_eq!(bytes[0x14], 5, "the refused value did not land");
}

#[test]
fn one_refused_edit_writes_none_of_them() {
    let pack = patchable();
    let mut bytes = vec![0u8; 0x40];
    let untouched = bytes.clone();
    let mut image = ImageMut::new(&mut bytes, 0x40);

    let err = patch(
        &pack,
        "Geometry",
        &mut image,
        0,
        &[
            Edit::new("margin", ScalarValue::F32(0.5)),
            Edit::new("num_vertices", ScalarValue::Int(3)),
        ],
    )
    .expect_err("the second edit is structural");
    assert!(matches!(err, PatchError::Structural { .. }));
    assert_eq!(bytes, untouched);
}

#[test]
fn a_type_can_be_patched_where_it_actually_lives() {
    let pack = patchable();
    let mut bytes = vec![0u8; 0x80];
    let mut image = ImageMut::new(&mut bytes, 0x80);

    patch(
        &pack,
        "Geometry",
        &mut image,
        0x40,
        &[Edit::new("margin", ScalarValue::F32(2.0))],
    )
    .expect("write");

    assert_eq!(&bytes[0x4C..0x50], &2.0f32.to_le_bytes());
    assert!(bytes[..0x40].iter().all(|&b| b == 0));
}

// A dictionary whose entry table names no element type, so only the caller can.
fn dictionary_pack() -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Dictionary",
        0x20,
        vec![
            field(
                "codes",
                0x00,
                FieldRepr::AtArray(Pointee::Many {
                    type_name: None,
                    count: CountSource::Inline,
                    element_size: Some(4),
                }),
            ),
            field(
                "entries",
                0x10,
                FieldRepr::AtArray(Pointee::ManyPtr {
                    type_name: None,
                    count: CountSource::Inline,
                }),
            ),
        ],
    ));
    pack.types.push(ty(
        "Entry",
        0x08,
        vec![
            field("id", 0x00, int(IntWidth::U32)),
            field("pad", 0x04, FieldRepr::Padding { size: 4 }),
        ],
    ));
    pack
}

fn dictionary_bytes() -> Bytes {
    let mut b = Bytes::new(0x400);
    b.ptr(0x00, 0x100);
    b.put(0x08, &2u16.to_le_bytes());
    b.put(0x0A, &2u16.to_le_bytes());
    b.ptr(0x10, 0x140);
    b.put(0x18, &2u16.to_le_bytes());
    b.put(0x1A, &2u16.to_le_bytes());

    b.put(0x100, &0xAAAA_AAAAu32.to_le_bytes());
    b.put(0x104, &0xBBBB_BBBBu32.to_le_bytes());

    b.ptr(0x140, 0x180);
    b.ptr(0x148, 0x190);

    b.put(0x180, &11u32.to_le_bytes());
    b.put(0x190, &22u32.to_le_bytes());
    b
}

fn entry_ids(value: &Value) -> Vec<Option<i128>> {
    let Some(Value::Ptr {
        target: Some(entries),
        ..
    }) = value.child("entries")
    else {
        panic!("entries should be a populated pointer");
    };
    let Value::Array { elements, .. } = entries.as_ref() else {
        panic!("entries should point at an array");
    };
    elements
        .iter()
        .map(|slot| match slot {
            Value::Ptr {
                target: Some(entry),
                ..
            } => entry.get("id").as_ref().and_then(Value::as_int),
            _ => None,
        })
        .collect()
}

#[test]
fn a_dictionary_read_without_its_element_type_reaches_no_entry() {
    let pack = dictionary_pack();
    let bytes = dictionary_bytes();
    let value = read(&pack, TypeName("Dictionary"), bytes.image(), 0).expect("read");

    let Some(Value::Ptr {
        target: Some(entries),
        ..
    }) = value.child("entries")
    else {
        panic!("entries should be a populated pointer");
    };
    let Value::Array { elements, .. } = entries.as_ref() else {
        panic!("entries should point at an array");
    };
    assert_eq!(elements.len(), 2, "the header's count is still read");
    assert!(
        elements
            .iter()
            .all(|slot| matches!(slot, Value::UndescribedPtr(_))),
        "every slot should be an address the reader will not follow: {elements:?}"
    );
}

#[test]
fn naming_the_element_type_reaches_the_entries() {
    let pack = dictionary_pack();
    let bytes = dictionary_bytes();
    let elements = ElementTypes::new().at("Dictionary", "entries", "Entry");
    let value =
        read_with(&pack, TypeName("Dictionary"), bytes.image(), 0, &elements).expect("read");

    assert_eq!(entry_ids(&value), vec![Some(11), Some(22)]);
}

#[test]
fn an_answer_for_another_site_does_not_reach_this_one() {
    let pack = dictionary_pack();
    let bytes = dictionary_bytes();

    for wrong in [
        ElementTypes::new().at("Dictionary", "codes", "Entry"),
        ElementTypes::new().at("Entry", "entries", "Entry"),
    ] {
        let value =
            read_with(&pack, TypeName("Dictionary"), bytes.image(), 0, &wrong).expect("read");
        assert_eq!(
            entry_ids(&value),
            vec![None, None],
            "an answer at {:?} should leave Dictionary::entries undescribed",
            wrong.sites().collect::<Vec<_>>()
        );
    }
}

#[test]
fn an_element_type_the_pack_does_not_describe_is_an_error() {
    let pack = dictionary_pack();
    let bytes = dictionary_bytes();
    let elements = ElementTypes::new().at("Dictionary", "entries", "Entery");

    let err = read_with(&pack, TypeName("Dictionary"), bytes.image(), 0, &elements)
        .expect_err("the pack has no Entery");
    assert!(
        err.to_string().contains("Entery"),
        "the error should name what could not be found: {err}"
    );
}

#[test]
fn a_dictionary_rebuilds_its_entries_only_once_they_have_been_read() {
    let pack = dictionary_pack();
    let bytes = dictionary_bytes();

    let blind = read(&pack, TypeName("Dictionary"), bytes.image(), 0).expect("read");
    let built = build(&pack, &blind).expect("build");
    let table = pointer_in(&built.virtual_data, 0x10) - FIXED_VIRTUAL_BASE;
    assert_eq!(
        pointer_in(&built.virtual_data, table as usize),
        0,
        "an address the reader would not follow cannot be rebuilt, so it is nulled"
    );

    let elements = ElementTypes::new().at("Dictionary", "entries", "Entry");
    let seeing =
        read_with(&pack, TypeName("Dictionary"), bytes.image(), 0, &elements).expect("read");
    let built = build(&pack, &seeing).expect("build");
    let table = pointer_in(&built.virtual_data, 0x10) - FIXED_VIRTUAL_BASE;

    let ids: Vec<u32> = (0..2)
        .map(|i| {
            let slot = pointer_in(&built.virtual_data, table as usize + i * 8);
            assert!(
                slot >= FIXED_VIRTUAL_BASE,
                "slot {i} should hold an entry's address and holds {slot:#x} — which is what a \
                 rebuild writes when the read never resolved the slot"
            );
            let at = (slot - FIXED_VIRTUAL_BASE) as usize;
            u32::from_le_bytes(
                built.virtual_data[at..at + 4]
                    .try_into()
                    .expect("four bytes"),
            )
        })
        .collect();
    assert_eq!(ids, vec![11, 22], "the entries are in the rebuilt heap");

    let rebuilt = Image::new(&built.virtual_data, built.virtual_data.len());
    let again = read_with(&pack, TypeName("Dictionary"), rebuilt, 0, &elements).expect("read back");
    assert_eq!(entry_ids(&again), entry_ids(&seeing));
}

fn nested_dictionary_pack() -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Dictionary",
        0x20,
        vec![field(
            "body",
            0x00,
            FieldRepr::Inline {
                type_name: "DictBody".into(),
            },
        )],
    ));
    pack.types.push(ty(
        "DictBody",
        0x20,
        vec![
            field(
                "codes",
                0x00,
                FieldRepr::AtArray(Pointee::Many {
                    type_name: None,
                    count: CountSource::Inline,
                    element_size: Some(4),
                }),
            ),
            field(
                "entries",
                0x10,
                FieldRepr::AtArray(Pointee::ManyPtr {
                    type_name: None,
                    count: CountSource::Inline,
                }),
            ),
        ],
    ));
    pack.types.push(ty(
        "Outer",
        0x20,
        vec![
            field(
                "mid",
                0x00,
                FieldRepr::Inline {
                    type_name: "Mid".into(),
                },
            ),
            field("outer_id", 0x10, int(IntWidth::U32)),
            field("pad", 0x14, FieldRepr::Padding { size: 12 }),
        ],
    ));
    pack.types.push(ty(
        "Mid",
        0x10,
        vec![
            field(
                "holder",
                0x00,
                FieldRepr::Ptr(Pointee::One {
                    type_name: "Holder".into(),
                }),
            ),
            field("pad", 0x08, FieldRepr::Padding { size: 8 }),
        ],
    ));
    pack.types.push(ty(
        "Holder",
        0x10,
        vec![field(
            "body",
            0x00,
            FieldRepr::Inline {
                type_name: "HolderBody".into(),
            },
        )],
    ));
    pack.types.push(ty(
        "HolderBody",
        0x10,
        vec![
            field(
                "nested",
                0x00,
                FieldRepr::Ptr(Pointee::One {
                    type_name: "Dictionary".into(),
                }),
            ),
            field("pad", 0x08, FieldRepr::Padding { size: 8 }),
        ],
    ));
    pack.types.push(ty(
        "Leaf",
        0x10,
        vec![
            field("leaf_id", 0x00, int(IntWidth::U32)),
            field("pad", 0x04, FieldRepr::Padding { size: 12 }),
        ],
    ));
    pack
}

fn nested_dictionary_bytes() -> Bytes {
    let mut b = Bytes::new(0x400);
    b.ptr(0x000, 0x100);
    b.put(0x008, &1u16.to_le_bytes())
        .put(0x00A, &1u16.to_le_bytes());
    b.ptr(0x010, 0x140);
    b.put(0x018, &1u16.to_le_bytes())
        .put(0x01A, &1u16.to_le_bytes());
    b.put(0x100, &0xAAAA_AAAAu32.to_le_bytes());
    b.ptr(0x140, 0x180);

    b.ptr(0x180, 0x200);
    b.put(0x190, &77u32.to_le_bytes());
    b.ptr(0x200, 0x240);

    b.ptr(0x240, 0x300);
    b.put(0x248, &2u16.to_le_bytes())
        .put(0x24A, &2u16.to_le_bytes());
    b.ptr(0x250, 0x340);
    b.put(0x258, &2u16.to_le_bytes())
        .put(0x25A, &2u16.to_le_bytes());
    b.put(0x300, &0xBBBB_BBBBu32.to_le_bytes());
    b.put(0x304, &0xCCCC_CCCCu32.to_le_bytes());
    b.ptr(0x340, 0x380);
    b.ptr(0x348, 0x390);
    b.put(0x380, &5u32.to_le_bytes());
    b.put(0x390, &6u32.to_le_bytes());
    b
}

fn bound_entries(dictionary: &Value) -> Vec<Option<String>> {
    let Some(Value::Ptr {
        target: Some(entries),
        ..
    }) = dictionary.child("body.entries")
    else {
        panic!("entries should be a populated pointer");
    };
    let Value::Array { elements, .. } = entries.as_ref() else {
        panic!("entries should point at an array");
    };
    elements
        .iter()
        .map(|slot| match slot {
            Value::Ptr {
                target: Some(entry),
                ..
            } => entry.type_name().map(str::to_string),
            _ => None,
        })
        .collect()
}

fn nested_of(root: &Value) -> &Value {
    let Some(Value::Ptr {
        target: Some(entries),
        ..
    }) = root.child("body.entries")
    else {
        panic!("the root should have entries");
    };
    let Value::Array { elements, .. } = entries.as_ref() else {
        panic!("entries should point at an array");
    };
    let Some(Value::Ptr {
        target: Some(outer),
        ..
    }) = elements.first()
    else {
        panic!("the root's single entry should have resolved");
    };
    let Some(Value::Ptr {
        target: Some(holder),
        ..
    }) = outer.child("mid.holder")
    else {
        panic!("Outer.mid.holder should have resolved");
    };
    let Some(Value::Ptr {
        target: Some(nested),
        ..
    }) = holder.child("body.nested")
    else {
        panic!("Holder.body.nested should have resolved");
    };
    nested
}

#[test]
fn the_generic_key_alone_gives_a_nested_container_the_roots_answer() {
    let pack = nested_dictionary_pack();
    let bytes = nested_dictionary_bytes();
    let only_generic = ElementTypes::new().at("DictBody", "entries", "Outer");

    let root = read_with(
        &pack,
        TypeName("Dictionary"),
        bytes.image(),
        0,
        &only_generic,
    )
    .expect("read");

    assert_eq!(
        bound_entries(&root),
        vec![Some("Outer".to_string())],
        "the root's own entries are what the generic key is for"
    );
    assert_eq!(
        bound_entries(nested_of(&root)),
        vec![Some("Outer".to_string()), Some("Outer".to_string())],
        "and the nested dictionary silently gets the same answer, which is the defect"
    );
}

#[test]
fn a_container_reached_through_a_pointer_is_keyed_on_that_pointer() {
    let pack = nested_dictionary_pack();
    let bytes = nested_dictionary_bytes();
    let both = ElementTypes::new()
        .at("DictBody", "entries", "Outer")
        .reached_through("HolderBody", "nested", "Leaf");

    let root = read_with(&pack, TypeName("Dictionary"), bytes.image(), 0, &both).expect("read");

    assert_eq!(
        bound_entries(&root),
        vec![Some("Outer".to_string())],
        "the root still answers from the generic key"
    );
    assert_eq!(
        bound_entries(nested_of(&root)),
        vec![Some("Leaf".to_string()), Some("Leaf".to_string())],
        "and the nested one answers from the pointer that reached it"
    );

    let Value::Ptr {
        target: Some(entries),
        ..
    } = nested_of(&root).child("body.entries").expect("entries")
    else {
        panic!("entries");
    };
    let Value::Array { elements, .. } = entries.as_ref() else {
        panic!("array");
    };
    let ids: Vec<Option<i128>> = elements
        .iter()
        .map(|slot| match slot {
            Value::Ptr {
                target: Some(leaf), ..
            } => leaf.get("leaf_id").as_ref().and_then(Value::as_int),
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec![Some(5), Some(6)]);
}

#[test]
fn an_arrival_answers_only_where_it_was_named() {
    let pack = nested_dictionary_pack();
    let bytes = nested_dictionary_bytes();

    let wrong_field = ElementTypes::new()
        .at("DictBody", "entries", "Outer")
        .reached_through("HolderBody", "pad", "Leaf");
    let root = read_with(
        &pack,
        TypeName("Dictionary"),
        bytes.image(),
        0,
        &wrong_field,
    )
    .expect("read");
    assert_eq!(
        bound_entries(nested_of(&root)),
        vec![Some("Outer".to_string()), Some("Outer".to_string())],
        "an arrival at a field nothing was reached through must not answer"
    );

    let one_hop_early = ElementTypes::new()
        .at("DictBody", "entries", "Outer")
        .reached_through("Mid", "holder", "Leaf");
    let root = read_with(
        &pack,
        TypeName("Dictionary"),
        bytes.image(),
        0,
        &one_hop_early,
    )
    .expect("read");
    assert_eq!(
        bound_entries(nested_of(&root)),
        vec![Some("Outer".to_string()), Some("Outer".to_string())],
        "`Mid::holder` reaches the Holder, not the dictionary one pointer further on"
    );

    let only_arrival = ElementTypes::new().reached_through("HolderBody", "nested", "Leaf");
    let root = read_with(
        &pack,
        TypeName("Dictionary"),
        bytes.image(),
        0,
        &only_arrival,
    )
    .expect("read");
    assert_eq!(
        bound_entries(&root),
        vec![None],
        "a root has no arrival, so an arrival-only answer leaves it undescribed — and the walk \
         therefore never reaches the nested dictionary the answer was about"
    );
}
