use std::sync::Arc;

use or_io::container::paging::{ResourceHeap, FIXED_VIRTUAL_BASE};
use or_io::desc::value::{self, BlobCodecs, BlobReader, BlobSource, ScalarValue, ValueError};
use or_io::desc::TypeName;
use or_io::desc::{
    CountSource, FieldRepr, Image, IntWidth, Pack, PackField, PackType, Pointee, Scalar, Value,
};
use or_io::write::{
    build_into_with, read_using, BlobHeap, BlobWriter, BlobWriters, BuildError, ElementTypes,
    Owner, Placement,
};

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

fn probe_pack() -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x10,
        vec![
            field("blob", 0x00, FieldRepr::Ptr(Pointee::Opaque)),
            field("blob_size", 0x08, int(IntWidth::U32)),
            field("other", 0x0C, int(IntWidth::U32)),
        ],
    ));
    pack
}

// A 24-byte allocation at 0x40 whose first word points sixteen bytes into itself.
fn probe_bytes() -> Bytes {
    let mut bytes = Bytes::new(0x80);
    bytes.ptr(0x00, 0x40);
    bytes.put(0x08, &24u32.to_le_bytes());
    bytes.put(0x0C, &7u32.to_le_bytes());
    bytes.ptr(0x40, 0x50);
    bytes.put(0x48, &0xABCD_EF01u32.to_le_bytes());
    bytes.put(0x50, &0xC0DEu32.to_le_bytes());
    bytes
}

const BLOB_CODEC: &str = "Root::blob";

fn carried(bytes: Vec<u8>, interior: u64) -> Value {
    Value::Blob {
        codec: BLOB_CODEC.into(),
        parts: vec![
            ("bytes".into(), Value::Opaque(bytes)),
            (
                "interior".into(),
                Value::Scalar(ScalarValue::Int(interior as i128)),
            ),
        ],
    }
}

fn nothing_carried() -> Value {
    Value::Blob {
        codec: BLOB_CODEC.into(),
        parts: Vec::new(),
    }
}

fn blank(pack: &Pack, type_name: &str) -> Value {
    value::empty(pack, type_name).expect("a described type")
}

fn with_child(mut value: Value, path: &str, child: Value) -> Value {
    value
        .record_mut()
        .expect("a struct")
        .set_child(path, child)
        .unwrap_or_else(|e| panic!("`{path}`: {e}"));
    value
}

#[derive(Debug, Clone, Copy)]
struct Carry;

impl BlobReader for Carry {
    fn read(&self, blob: &dyn BlobSource) -> Result<Value, ValueError> {
        let (owner, owner_at) = blob.owner();
        let (address, at) = blob.blob();
        let image = blob.image();

        let size_field = owner.field_by_name("blob_size").expect("declared");
        let size = IntWidth::U32
            .read(image.bytes(), owner_at + size_field.offset)
            .expect("in bounds") as usize;

        let bytes = image.slice_at(at, size).expect("in bounds").to_vec();
        let interior = image.pointer_at(at).expect("in bounds") - address;

        Ok(carried(bytes, interior))
    }
}

impl BlobWriter for Carry {
    fn write(
        &self,
        heap: &mut dyn BlobHeap,
        owner: Owner<'_>,
        value: &Value,
    ) -> Result<Placement, BuildError> {
        let refuse = |m: &str| BuildError::Blob {
            type_name: owner.ty.name.clone(),
            field: "blob".into(),
            message: m.into(),
        };
        let bytes = value
            .part("bytes")
            .and_then(Value::as_bytes)
            .ok_or_else(|| refuse("no bytes"))?
            .to_vec();
        let interior = value
            .part("interior")
            .and_then(Value::as_int)
            .ok_or_else(|| refuse("no interior offset"))? as u64;

        let declared = owner.int("blob_size").ok_or_else(|| refuse("no size"))?;
        if declared != bytes.len() as i128 {
            return Err(refuse("the size the header states is not the run's length"));
        }

        let address = heap.alloc(bytes.len(), 16);
        let base = heap.offset_of(address);
        heap.put(base, &bytes)?;
        let target = address + interior;
        heap.put(base, &target.to_le_bytes())?;
        heap.fixup(base, target)?;
        Ok(Placement {
            address,
            siblings: Vec::new(),
        })
    }
}

fn codecs() -> (BlobCodecs, BlobWriters) {
    (
        BlobCodecs::new().at("Root", "blob", Arc::new(Carry)),
        BlobWriters::new().at("Root", "blob", Arc::new(Carry)),
    )
}

fn round_trip(pack: &Pack, bytes: &Bytes) -> (Value, Value, Vec<u8>) {
    let (readers, writers) = codecs();
    let before = read_using(
        pack,
        TypeName("Root"),
        bytes.image(),
        0,
        &ElementTypes::new(),
        &readers,
    )
    .expect("the codec read it");

    let mut heap = ResourceHeap::new();
    build_into_with(&mut heap, pack, &before, &writers).expect("the codec wrote it");
    let (virtual_data, physical_data, _, _) = heap.finish_with_fixups();
    assert!(
        physical_data.is_empty(),
        "this probe is virtual-only and the build put something in graphics memory"
    );

    let image = Image::new(&virtual_data, virtual_data.len());
    let after = read_using(
        pack,
        TypeName("Root"),
        image,
        0,
        &ElementTypes::new(),
        &readers,
    )
    .expect("the codec read what it wrote");
    (before, after, virtual_data)
}

#[test]
fn a_codec_carries_an_allocation_the_description_declines_to_follow() {
    let pack = probe_pack();
    let bytes = probe_bytes();

    let without = read_using(
        &pack,
        TypeName("Root"),
        bytes.image(),
        0,
        &ElementTypes::new(),
        &BlobCodecs::new(),
    )
    .expect("read");
    assert!(
        matches!(without.child("blob"), Some(Value::UndescribedPtr(a)) if *a != 0),
        "with no codec an opaque pointer must still read as an address nobody \
         describes, or this test is not about the codec: {:?}",
        without.child("blob")
    );

    let (before, after, _) = round_trip(&pack, &bytes);
    let payload = |v: &Value| -> Vec<u8> {
        v.child("blob")
            .and_then(Value::target)
            .and_then(|blob| blob.part("bytes"))
            .and_then(Value::as_bytes)
            .expect("the codec's bytes")
            .to_vec()
    };
    assert_eq!(payload(&before).len(), 24, "the declared size was not read");
    assert_eq!(
        payload(&before)[8..],
        payload(&after)[8..],
        "everything past the interior pointer is carried verbatim and must come \
         back identical"
    );
    assert_eq!(
        before.get("other"),
        after.get("other"),
        "an ordinary scalar beside a codec's field must be untouched by it"
    );
}

/// The pointer inside a carried allocation points into the **new** one.
///
/// ⚠ Carrying an allocation verbatim leaves interior pointers addressing a
/// layout that no longer exists, which turns a data loss into pointer corruption.
#[test]
fn an_interior_pointer_is_rebased_rather_than_carried() {
    let pack = probe_pack();
    let bytes = probe_bytes();
    let (_, _, heap) = round_trip(&pack, &bytes);

    let blob = u64::from_le_bytes(heap[0x00..0x08].try_into().unwrap());
    let blob_at = (blob - FIXED_VIRTUAL_BASE) as usize;
    let interior = u64::from_le_bytes(heap[blob_at..blob_at + 8].try_into().unwrap());

    assert_ne!(
        blob,
        FIXED_VIRTUAL_BASE | 0x40,
        "the rebuilt allocation landed where the source one was, so this test \
         cannot tell a rebased pointer from a carried one. Move the probe."
    );
    assert_eq!(
        interior,
        blob + 16,
        "the interior pointer is {interior:#x} where the allocation now begins at \
         {blob:#x}; sixteen bytes in is where it pointed in the source"
    );
    let payload = (interior - FIXED_VIRTUAL_BASE) as usize;
    assert_eq!(
        u32::from_le_bytes(heap[payload..payload + 4].try_into().unwrap()),
        0xC0DE,
        "the rebased pointer does not lead to the bytes it led to before"
    );
}

#[test]
fn a_codec_can_point_a_sibling_field_into_what_it_wrote() {
    struct WithSibling;

    impl BlobWriter for WithSibling {
        fn write(
            &self,
            heap: &mut dyn BlobHeap,
            _owner: Owner<'_>,
            _value: &Value,
        ) -> Result<Placement, BuildError> {
            let address = heap.alloc(32, 16);
            let base = heap.offset_of(address);
            heap.put(base, &[0xAA; 32])?;
            Ok(Placement {
                address,
                siblings: vec![("second".into(), address + 16)],
            })
        }
    }

    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x10,
        vec![
            field("blob", 0x00, FieldRepr::Ptr(Pointee::Opaque)),
            field("second", 0x08, FieldRepr::Ptr(Pointee::Opaque)),
        ],
    ));

    let mut bytes = Bytes::new(0x40);
    bytes.ptr(0x00, 0x20);
    bytes.ptr(0x08, 0x30);

    let root = |second: Value| {
        let root = with_child(
            blank(&pack, "Root"),
            "blob",
            Value::ptr_to(nothing_carried()),
        );
        with_child(root, "second", second)
    };

    let value = root(Value::UndescribedPtr(0x5000_0030));
    let writers = BlobWriters::new().at("Root", "blob", Arc::new(WithSibling));
    let mut heap = ResourceHeap::new();
    build_into_with(&mut heap, &pack, &value, &writers).expect("built");
    let (data, _, fixups, _) = heap.finish_with_fixups();

    let blob = u64::from_le_bytes(data[0x00..0x08].try_into().unwrap());
    let second = u64::from_le_bytes(data[0x08..0x10].try_into().unwrap());
    assert_ne!(blob, 0, "the codec's own field was not written");
    assert_eq!(
        second,
        blob + 16,
        "the sibling field holds {second:#x} rather than sixteen bytes into the \
         codec's allocation at {blob:#x} — either the placement was not applied, \
         or the ordinary emit of an undescribed pointer wrote a null over it"
    );
    let at_second = fixups.iter().filter(|f| f.write_offset == 0x08).count();
    assert_eq!(
        at_second, 1,
        "a sibling pointer needs exactly one fixup: none and the game never \
         rebases it, two and it rebases twice, which is the failure the drawable \
         fixup validator exists for"
    );
}

/// A sibling may only name a pointer the pack declines to describe.
///
/// ⚠ A codec placing a field the interpreter also writes leaves two fixups at one
/// offset, and RSC7's rebase walk has no visited set — so the game resolves it
/// twice and rejects the file.
#[test]
fn a_sibling_placement_on_a_described_field_is_refused() {
    struct Claims;

    impl BlobWriter for Claims {
        fn write(
            &self,
            heap: &mut dyn BlobHeap,
            _owner: Owner<'_>,
            _value: &Value,
        ) -> Result<Placement, BuildError> {
            let address = heap.alloc(16, 16);
            Ok(Placement {
                address,
                siblings: vec![("described".into(), address)],
            })
        }
    }

    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x10,
        vec![
            field("blob", 0x00, FieldRepr::Ptr(Pointee::Opaque)),
            field(
                "described",
                0x08,
                FieldRepr::Ptr(Pointee::One {
                    type_name: "Leaf".into(),
                }),
            ),
        ],
    ));
    pack.types
        .push(ty("Leaf", 0x08, vec![field("v", 0x00, int(IntWidth::U32))]));

    let mut leaf = blank(&pack, "Leaf");
    leaf.record_mut()
        .expect("a struct")
        .set_scalar("v", ScalarValue::Int(9))
        .expect("v");
    let value = with_child(
        with_child(
            blank(&pack, "Root"),
            "blob",
            Value::ptr_to(nothing_carried()),
        ),
        "described",
        Value::ptr_to(leaf),
    );

    let writers = BlobWriters::new().at("Root", "blob", Arc::new(Claims));
    let mut heap = ResourceHeap::new();
    let built = build_into_with(&mut heap, &pack, &value, &writers);
    assert!(
        matches!(built, Err(BuildError::Blob { ref field, .. }) if field == "described"),
        "a codec claimed a field the interpreter also writes and the build allowed \
         it: {built:?}"
    );
}

#[test]
fn arrival_outranks_site_when_the_two_disagree() {
    struct Named(&'static str);

    impl BlobReader for Named {
        fn read(&self, _blob: &dyn BlobSource) -> Result<Value, ValueError> {
            Ok(Value::Bytes(self.0.as_bytes().to_vec()))
        }
    }

    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x10,
        vec![
            field(
                "here",
                0x00,
                FieldRepr::Inline {
                    type_name: "Holder".into(),
                },
            ),
            field(
                "there",
                0x08,
                FieldRepr::Ptr(Pointee::One {
                    type_name: "Holder".into(),
                }),
            ),
        ],
    ));
    pack.types.push(ty(
        "Holder",
        0x08,
        vec![field("blob", 0x00, FieldRepr::Ptr(Pointee::Opaque))],
    ));

    let mut bytes = Bytes::new(0x40);
    bytes.ptr(0x00, 0x20);
    bytes.ptr(0x08, 0x10);
    bytes.ptr(0x10, 0x28);

    let readers = BlobCodecs::new()
        .at("Holder", "blob", Arc::new(Named("by site")))
        .reached_through("Root", "there", Arc::new(Named("by arrival")));

    let value = read_using(
        &pack,
        TypeName("Root"),
        bytes.image(),
        0,
        &ElementTypes::new(),
        &readers,
    )
    .expect("read");

    let answer = |path: &str| -> String {
        let blob = match path {
            "here" => value.child("here.blob"),
            _ => match value.child("there").expect("pointer") {
                Value::Ptr {
                    target: Some(inner),
                    ..
                } => inner.child("blob"),
                other => panic!("not a pointer: {other:?}"),
            },
        };
        match blob {
            Some(Value::Ptr {
                target: Some(inner),
                ..
            }) => String::from_utf8(inner.as_bytes().expect("bytes").to_vec()).unwrap(),
            other => panic!("no codec fired: {other:?}"),
        }
    };

    assert_eq!(
        answer("here"),
        "by site",
        "an embedded `Holder` keeps the arrival of whatever reached its enclosing \
         allocation, which is nothing at the root — so only the site can answer"
    );
    assert_eq!(
        answer("there"),
        "by arrival",
        "a `Holder` reached through `Root::there` has an arrival that answers, and \
         it must outrank the site answer that also matches"
    );
}

#[test]
fn a_codec_on_anything_but_a_pointer_is_refused_by_name() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x10,
        vec![field(
            "table",
            0x00,
            FieldRepr::AtArray(Pointee::Many {
                type_name: None,
                count: CountSource::Inline,
                element_size: Some(4),
            }),
        )],
    ));

    let mut bytes = Bytes::new(0x40);
    bytes.ptr(0x00, 0x20);
    bytes.put(0x08, &2u16.to_le_bytes());
    bytes.put(0x0A, &2u16.to_le_bytes());

    let readers = BlobCodecs::new().at("Root", "table", Arc::new(Carry));
    let read = read_using(
        &pack,
        TypeName("Root"),
        bytes.image(),
        0,
        &ElementTypes::new(),
        &readers,
    );
    assert!(
        matches!(
            read,
            Err(ValueError::BlobNotAPointer { ref field, .. }) if field == "table"
        ),
        "the reader accepted a codec on an atArray: {read:?}"
    );

    let value = read_using(
        &pack,
        TypeName("Root"),
        bytes.image(),
        0,
        &ElementTypes::new(),
        &BlobCodecs::new(),
    )
    .expect("reads fine with no codec");
    let writers = BlobWriters::new().at("Root", "table", Arc::new(Carry));
    let mut heap = ResourceHeap::new();
    let built = build_into_with(&mut heap, &pack, &value, &writers);
    assert!(
        matches!(
            built,
            Err(BuildError::BlobNotAPointer { ref field, .. }) if field == "table"
        ),
        "the builder accepted a codec on an atArray: {built:?}"
    );
}

#[test]
fn a_codec_that_refuses_its_value_fails_the_build() {
    let pack = probe_pack();
    let bytes = probe_bytes();
    let (readers, writers) = codecs();
    let mut value = read_using(
        &pack,
        TypeName("Root"),
        bytes.image(),
        0,
        &ElementTypes::new(),
        &readers,
    )
    .expect("read");

    value
        .record_mut()
        .expect("a struct")
        .set_scalar("blob_size", ScalarValue::Int(23))
        .expect("declared");

    let mut heap = ResourceHeap::new();
    let built = build_into_with(&mut heap, &pack, &value, &writers);
    assert!(
        matches!(built, Err(BuildError::Blob { .. })),
        "a codec's refusal did not reach the caller: {built:?}"
    );
}
