//! Growing and shrinking a described run, and the two naive edits that build
//! cleanly and are wrong: a whole-field write over a masked count, and a run
//! grown past its declared capacity.
//!
//! Every fixture is a synthetic pack, so this runs with no corpus and no staged
//! pack.

use or_io::desc::value::{self, ScalarValue};
use or_io::desc::{
    CountSource, FieldRepr, Image, IntWidth, Pack, PackField, PackType, Pointee, Scalar, TypeName,
    Value,
};
use or_io::write::{build, edit};

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

fn item_type() -> PackType {
    ty(
        "Item",
        8,
        vec![
            field("id", 0x00, int(IntWidth::U32)),
            field("weight", 0x04, FieldRepr::Scalar(Scalar::F32)),
        ],
    )
}

/// A root whose run counts through a whole field — the common shape.
fn simple_pack() -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x10,
        vec![
            field("count", 0x00, int(IntWidth::U16)),
            field(
                "items",
                0x08,
                FieldRepr::Ptr(Pointee::Many {
                    type_name: Some("Item".into()),
                    count: CountSource::Field("count".into()),
                    element_size: None,
                }),
            ),
        ],
    ));
    pack.types.push(item_type());
    pack
}

/// A blank of `type_name` with `scalars` written, counts included, and `children` put in place.
fn record(
    pack: &Pack,
    type_name: &str,
    scalars: &[(&str, ScalarValue)],
    children: Vec<(&str, Value)>,
) -> Value {
    let mut value = value::empty(pack, type_name).expect("a described type");
    let record = value.record_mut().expect("a struct");
    for (path, scalar) in scalars {
        record
            .set_structural(path, scalar.clone())
            .unwrap_or_else(|e| panic!("`{path}`: {e}"));
    }
    for (path, child) in children {
        record
            .set_child(path, child)
            .unwrap_or_else(|e| panic!("`{path}`: {e}"));
    }
    value
}

fn i(n: i128) -> ScalarValue {
    ScalarValue::Int(n)
}

fn an_item(pack: &Pack, id: i128) -> Value {
    record(
        pack,
        "Item",
        &[("id", i(id)), ("weight", ScalarValue::F32(1.0))],
        vec![],
    )
}

fn run_of(pack: &Pack, ids: &[i128]) -> Value {
    Value::Ptr {
        source: 0,
        target: Some(Box::new(Value::Array {
            elements: ids.iter().map(|id| an_item(pack, *id)).collect(),
            capacity: None,
        })),
    }
}

fn root_with(pack: &Pack, ids: &[i128]) -> Value {
    record(
        pack,
        "Root",
        &[("count", i(ids.len() as i128))],
        vec![("items", run_of(pack, ids))],
    )
}

/// A root whose count lives under `mask` in a word shared with flags.
fn masked_pack(mask: u64) -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x10,
        vec![
            field("packed", 0x00, int(IntWidth::U32)),
            field(
                "items",
                0x08,
                FieldRepr::Ptr(Pointee::Many {
                    type_name: Some("Item".into()),
                    count: CountSource::MaskedField {
                        field: "packed".into(),
                        mask,
                    },
                    element_size: None,
                }),
            ),
        ],
    ));
    pack.types.push(item_type());
    pack
}

/// A root whose run has both a count and an allocated capacity.
fn capped_pack() -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x10,
        vec![
            field("count", 0x00, int(IntWidth::U16)),
            field("cap", 0x02, int(IntWidth::U16)),
            field(
                "items",
                0x08,
                FieldRepr::Ptr(Pointee::Many {
                    type_name: Some("Item".into()),
                    count: CountSource::CountAndCapacity {
                        count: "count".into(),
                        capacity: "cap".into(),
                    },
                    element_size: None,
                }),
            ),
        ],
    ));
    pack.types.push(item_type());
    pack
}

fn int_of(value: &Value, name: &str) -> i128 {
    match value.get(name) {
        Some(Value::Scalar(ScalarValue::Int(n))) => n,
        other => panic!("`{name}` is not an integer: {other:?}"),
    }
}

/// The run's ids, whether it is an authored run of records or a read, packed one.
fn ids_in(value: &mut Value, field: &str) -> Vec<i128> {
    let run = value
        .child(field)
        .expect("the field")
        .target()
        .expect("a pointer with a target");
    (0..run.run_len().expect("a run"))
        .map(|i| run.struct_at(i).and_then(|e| e.int("id")).expect("an id"))
        .collect()
}

fn reread(pack: &Pack, root: &Value) -> Value {
    let built = build(pack, root).expect("build");
    value::read(
        pack,
        TypeName("Root"),
        Image::new(&built.virtual_data, built.virtual_data.len()),
        0,
    )
    .expect("reads back")
}

#[test]
fn a_read_run_is_packed_and_edits_the_same_way() {
    let pack = simple_pack();
    let mut root = reread(&pack, &root_with(&pack, &[10, 20]));
    assert!(
        matches!(
            root.child("items").and_then(Value::target),
            Some(Value::Records(_))
        ),
        "an `Item` holds no pointer, so the reader packs the run"
    );

    edit::insert_at(&pack, &mut root, "items", 1, an_item(&pack, 15)).expect("insert");
    assert_eq!(int_of(&root, "count"), 3);
    let taken = edit::remove(&pack, &mut root, "items", 0).expect("remove");
    assert_eq!(int_of(&taken, "id"), 10);
    assert_eq!(int_of(&root, "count"), 2);
    assert_eq!(ids_in(&mut reread(&pack, &root), "items"), vec![15, 20]);
}

#[test]
fn a_packed_run_refuses_a_foreign_element_before_touching_the_count() {
    let pack = simple_pack();
    let mut root = reread(&pack, &root_with(&pack, &[10, 20]));
    let stranger = record(&pack, "Root", &[], vec![]);

    let refused = edit::insert(&pack, &mut root, "items", stranger).expect_err("not an `Item`");
    assert!(
        matches!(refused, edit::EditError::NotAnElement { .. }),
        "{refused}"
    );
    assert_eq!(int_of(&root, "count"), 2, "the count is untouched");
    assert_eq!(ids_in(&mut root, "items"), vec![10, 20]);
}

fn len_in(value: &mut Value, field: &str) -> usize {
    ids_in(value, field).len()
}

/// A bare `push` is caught by `build`, loudly.
///
/// ⚠ `build` carries a count rather than recomputing it, but that does not mean
/// it writes three items under a header saying two: it **validates** the count
/// against the run and refuses with `CountDisagrees`.
#[test]
fn a_bare_push_is_refused_by_the_builder_rather_than_written() {
    let pack = simple_pack();
    let mut root = root_with(&pack, &[10, 20]);

    root.child_mut("items")
        .expect("items")
        .target_mut()
        .expect("target")
        .elements_mut()
        .expect("array")
        .push(an_item(&pack, 30));

    assert_eq!(len_in(&mut root, "items"), 3, "the run really did grow");
    assert_eq!(
        int_of(&root, "count"),
        2,
        "and the count did not, because `build` carries counts rather than \
         recomputing them"
    );

    let refused = build(&pack, &root).expect_err(
        "a run longer than its count must be refused. If this ever builds, the \
         container layer has stopped validating counts and the silent corruption \
         this test was written expecting becomes real.",
    );
    let said = refused.to_string();
    assert!(
        said.contains("holds 3") && said.contains("says 2"),
        "the refusal must name both numbers so a user can see which is wrong, \
         got {said:?}"
    );
}

#[test]
fn an_insert_records_the_length_and_round_trips() {
    let pack = simple_pack();
    let mut root = root_with(&pack, &[10, 20]);

    edit::insert(&pack, &mut root, "items", an_item(&pack, 30)).expect("insert");

    assert_eq!(int_of(&root, "count"), 3, "the count follows the run");
    assert_eq!(len_in(&mut root, "items"), 3);

    let built = build(&pack, &root).expect("build");
    assert_eq!(
        u16::from_le_bytes(built.virtual_data[0..2].try_into().expect("two bytes")),
        3,
        "and the file agrees"
    );

    let mut reread = value::read(
        &pack,
        TypeName("Root"),
        Image::new(&built.virtual_data, built.virtual_data.len()),
        0,
    )
    .expect("the built file reads back through the same description");
    assert_eq!(
        ids_in(&mut reread, "items"),
        vec![10, 20, 30],
        "the round trip preserves order as well as length — an insert that \
         reordered would silently repoint every index stored elsewhere in the file"
    );
}

#[test]
fn a_remove_records_the_length_and_hands_back_what_it_took() {
    let pack = simple_pack();
    let mut root = root_with(&pack, &[10, 20, 30]);

    let taken = edit::remove(&pack, &mut root, "items", 1).expect("remove");
    assert_eq!(
        int_of(&taken, "id"),
        20,
        "remove hands back what it took, so an undo can put it back"
    );
    assert_eq!(int_of(&root, "count"), 2);

    let built = build(&pack, &root).expect("build");
    let mut reread = value::read(
        &pack,
        TypeName("Root"),
        Image::new(&built.virtual_data, built.virtual_data.len()),
        0,
    )
    .expect("reads back");
    assert_eq!(ids_in(&mut reread, "items"), vec![10, 30]);
}

/// ⚠ **A correct count in a clobbered word builds.**
///
/// `build` validates the *count*. A caller maintaining a `MaskedField` count by
/// hand, by writing the length into the whole field, produces a tree whose count
/// is right — so the check passes — and whose other bits are gone. Those bits are
/// flags in practice, so the file loads and the asset behaves differently.
/// [`edit::insert`] does a read-modify-write rather than a set.
#[test]
fn a_whole_field_write_to_a_masked_count_builds_and_is_wrong() {
    const MASK: u64 = 0x0000_FFFF;
    const FLAGS: i128 = 0x00AB_0000;

    let pack = masked_pack(MASK);
    let mut naive = record(
        &pack,
        "Root",
        &[("packed", i(FLAGS | 2))],
        vec![("items", run_of(&pack, &[10, 20]))],
    );

    // The hand-maintained edit: push, then "update the count".
    naive
        .child_mut("items")
        .expect("items")
        .target_mut()
        .expect("target")
        .elements_mut()
        .expect("array")
        .push(an_item(&pack, 30));
    naive
        .record_mut()
        .expect("a struct")
        .set_structural("packed", i(3))
        .expect("packed");

    let built = build(&pack, &naive).expect(
        "⚠ this builds. The count under the mask is 3 and the run holds 3, so \
         `CountDisagrees` is satisfied — and the flags are gone. If this ever \
         fails, the builder has learnt to check the bits outside a mask and this \
         hole has closed.",
    );
    let word = u32::from_le_bytes(built.virtual_data[0..4].try_into().expect("four bytes"));
    assert_eq!(
        i128::from(word) & !(MASK as i128),
        0,
        "the flags were destroyed, and nothing objected"
    );

    // The same edit through `edit::insert`.
    let mut correct = record(
        &pack,
        "Root",
        &[("packed", i(FLAGS | 2))],
        vec![("items", run_of(&pack, &[10, 20]))],
    );
    edit::insert(&pack, &mut correct, "items", an_item(&pack, 30)).expect("insert");
    let built = build(&pack, &correct).expect("build");
    let word = u32::from_le_bytes(built.virtual_data[0..4].try_into().expect("four bytes"));
    assert_eq!(
        i128::from(word) & !(MASK as i128),
        FLAGS,
        "and through the edit module the flags survive"
    );
    assert_eq!(i128::from(word) & (MASK as i128), 3, "with the right count");
}

/// ⚠ **A run past its declared capacity builds.**
///
/// A `CountAndCapacity` run lives in an allocation the capacity sizes. Grow the
/// run and set the count to match and `CountDisagrees` is satisfied, because the
/// count *is* right — the capacity is the field that is now a lie.
#[test]
fn a_run_past_its_declared_capacity_builds_and_is_wrong() {
    let pack = capped_pack();
    let mut naive = record(
        &pack,
        "Root",
        &[("count", i(3)), ("cap", i(3))],
        vec![("items", run_of(&pack, &[10, 20, 30]))],
    );

    naive
        .child_mut("items")
        .expect("items")
        .target_mut()
        .expect("target")
        .elements_mut()
        .expect("array")
        .push(an_item(&pack, 40));
    naive
        .record_mut()
        .expect("a struct")
        .set_structural("count", i(4))
        .expect("count");

    build(&pack, &naive).expect(
        "⚠ this builds: the count says four and the run holds four. The capacity \
         still says three, and nothing compares the two. If this ever fails, the \
         builder has learnt to check a run against its capacity and this hole has \
         closed.",
    );

    // `edit` refuses instead.
    let mut correct = record(
        &pack,
        "Root",
        &[("count", i(3)), ("cap", i(3))],
        vec![("items", run_of(&pack, &[10, 20, 30]))],
    );
    let refused = edit::insert(&pack, &mut correct, "items", an_item(&pack, 40))
        .expect_err("a fourth does not fit a capacity of three");
    assert!(
        refused.to_string().contains("at most 3"),
        "the refusal must name the capacity, got {refused}"
    );
}

/// ⚠ **A count is not always a whole field.**
///
/// `CountSource::MaskedField` puts the length in part of a word and leaves the
/// rest to something else — in practice flags. A `set_count` that writes the
/// length into the whole field destroys them, and because the file still reads,
/// what the user sees is the asset behaving differently rather than an error.
#[test]
fn a_masked_count_keeps_the_bits_that_are_not_the_count() {
    const MASK: u64 = 0x0000_FFFF;
    /// Flags in the high half that must survive an edit to the low half.
    const FLAGS: i128 = 0x00AB_0000;

    let pack = masked_pack(MASK);

    let mut root = record(
        &pack,
        "Root",
        &[("packed", i(FLAGS | 2))],
        vec![("items", run_of(&pack, &[10, 20]))],
    );

    edit::insert(&pack, &mut root, "items", an_item(&pack, 30)).expect("insert into a masked run");

    let packed = int_of(&root, "packed");
    assert_eq!(
        packed & (MASK as i128),
        3,
        "the count under the mask must be the new length"
    );
    assert_eq!(
        packed & !(MASK as i128),
        FLAGS,
        "⚠ and everything outside the mask must be untouched. A whole-field write \
         would give {:#x} here instead of {FLAGS:#x} — and the file would still \
         load, with different flags.",
        3
    );
}

/// ⚠ A masked count cannot exceed what its mask can hold.
#[test]
fn a_masked_count_refuses_a_length_its_mask_cannot_hold() {
    const MASK: u64 = 0x0000_0003; // two bits: at most three
    let pack = masked_pack(MASK);

    let mut root = record(
        &pack,
        "Root",
        &[("packed", i(3))],
        vec![("items", run_of(&pack, &[1, 2, 3]))],
    );

    let refused = edit::insert(&pack, &mut root, "items", an_item(&pack, 4));
    assert!(
        refused.is_err(),
        "four does not fit in two bits. Writing it would overflow into whatever \
         the next bits mean, which is the corruption the mask exists to prevent."
    );
    assert_eq!(
        int_of(&root, "packed"),
        3,
        "and a refused insert leaves the value alone"
    );
}

/// ⚠ A capacity is an allocation size, so a run cannot outgrow it — and
/// shrinking the run must not shrink it either.
#[test]
fn a_counted_and_capped_run_stops_at_its_capacity_and_never_lowers_it() {
    let mut pack = Pack::new("probe", "gen8");
    pack.types.push(ty(
        "Root",
        0x10,
        vec![
            field("count", 0x00, int(IntWidth::U16)),
            field("cap", 0x02, int(IntWidth::U16)),
            field(
                "items",
                0x08,
                FieldRepr::Ptr(Pointee::Many {
                    type_name: Some("Item".into()),
                    count: CountSource::CountAndCapacity {
                        count: "count".into(),
                        capacity: "cap".into(),
                    },
                    element_size: None,
                }),
            ),
        ],
    ));
    pack.types.push(item_type());

    let mut root = record(
        &pack,
        "Root",
        &[("count", i(2)), ("cap", i(3))],
        vec![("items", run_of(&pack, &[10, 20]))],
    );

    edit::insert(&pack, &mut root, "items", an_item(&pack, 30)).expect("two into three fits");
    assert_eq!(int_of(&root, "count"), 3);
    assert_eq!(int_of(&root, "cap"), 3, "the capacity did not move");

    let refused = edit::insert(&pack, &mut root, "items", an_item(&pack, 40));
    assert!(
        refused.is_err(),
        "a fourth does not fit in a capacity of three. The allocation is sized by \
         the capacity, so growing past it needs a reallocation the game will not do."
    );
    assert_eq!(len_in(&mut root, "items"), 3, "and nothing was appended");

    edit::remove(&pack, &mut root, "items", 0).expect("remove");
    assert_eq!(int_of(&root, "count"), 2);
    assert_eq!(
        int_of(&root, "cap"),
        3,
        "⚠ and the capacity is still three. R17: a capacity is carried, not \
         recomputed — lowering it to match a shorter run shrinks an allocation \
         the game expects at its declared size."
    );
}

/// Every count shape the description can name is either handled or refused by
/// name, and nothing is silently treated as another.
#[test]
fn each_count_shape_is_either_maintained_or_refused_with_a_reason() {
    // (the count source, whether it is resizable, a word its refusal must say)
    let cases: Vec<(CountSource, bool, &str)> = vec![
        (CountSource::Field("count".into()), true, ""),
        (
            CountSource::CountAndCapacity {
                count: "count".into(),
                capacity: "cap".into(),
            },
            true,
            "",
        ),
        (
            CountSource::MaskedField {
                field: "count".into(),
                mask: 0xFFFF,
            },
            true,
            "",
        ),
        (CountSource::Fixed(4), false, "always 4"),
        (CountSource::Inline, false, "inline"),
        (CountSource::Unknown, false, "does not say"),
        (
            CountSource::Product {
                count: "count".into(),
                stride: "cap".into(),
            },
            false,
            "stride",
        ),
    ];

    // ⚠ Counted by hand, not derived from the enum: `PrefixedAboveOne` and
    // `Sum` have no row here, and this assertion does not notice.
    assert_eq!(
        cases.len(),
        7,
        "CountSource has seven variants; every one needs a row here"
    );

    for (count, resizable, expect) in cases {
        let mut pack = Pack::new("probe", "gen8");
        pack.types.push(ty(
            "Root",
            0x10,
            vec![
                field("count", 0x00, int(IntWidth::U16)),
                field("cap", 0x02, int(IntWidth::U16)),
                field(
                    "items",
                    0x08,
                    FieldRepr::Ptr(Pointee::Many {
                        type_name: Some("Item".into()),
                        count: count.clone(),
                        element_size: None,
                    }),
                ),
            ],
        ));
        pack.types.push(item_type());

        let root = record(
            &pack,
            "Root",
            &[("count", i(0)), ("cap", i(8))],
            vec![("items", run_of(&pack, &[]))],
        );

        let why = edit::why_not_resizable(&pack, &root, "items");
        if resizable {
            assert_eq!(why, None, "{count:?} should be resizable");
        } else {
            let why = why.unwrap_or_else(|| panic!("{count:?} should be refused"));
            assert!(
                why.contains(expect),
                "{count:?} must be refused with a reason naming {expect:?}, got {why:?}"
            );
        }
    }
}

/// ⚠ An edit naming something that is not a resizable run is refused, not
/// guessed — and the refusal names which thing it is, because it reaches a user
/// as a greyed-out button.
#[test]
fn a_refusal_names_what_is_wrong_with_it() {
    let pack = simple_pack();
    let root = root_with(&pack, &[10]);

    let scalar = edit::why_not_resizable(&pack, &root, "count").expect("a scalar");
    assert!(scalar.contains("not an array"), "got {scalar:?}");

    let unknown = edit::why_not_resizable(&pack, &root, "nope").expect("an unknown field");
    assert!(unknown.contains("no field"), "got {unknown:?}");

    assert_eq!(
        edit::resizable_fields(&pack, "Root"),
        vec!["items".to_string()],
        "only the run is offerable, so a panel does not have to re-derive the rule"
    );
}

/// ⚠ A null pointer is not an empty run.
#[test]
fn inserting_behind_a_null_pointer_says_to_author_one() {
    let pack = simple_pack();
    let mut root = record(
        &pack,
        "Root",
        &[("count", i(0))],
        vec![(
            "items",
            Value::Ptr {
                source: 0,
                target: None,
            },
        )],
    );

    let e = edit::insert(&pack, &mut root, "items", an_item(&pack, 1)).expect_err("a null pointer");
    assert!(
        e.to_string().contains("Author one"),
        "the refusal must say what to do instead, got {e}"
    );
    assert_eq!(
        int_of(&root, "count"),
        0,
        "and a refused insert changes nothing"
    );
}

/// ⚠ Insert and remove are exact inverses, which is what undo/redo needs: an
/// undo stack stores `(field, index, element)` and replays it, and the inverse
/// has to restore the count and the bits beside it too.
#[test]
fn a_remove_and_an_insert_at_undo_each_other_exactly() {
    const MASK: u64 = 0x0000_FFFF;
    const FLAGS: i128 = 0x00AB_0000;

    let pack = masked_pack(MASK);
    let before = record(
        &pack,
        "Root",
        &[("packed", i(FLAGS | 3))],
        vec![("items", run_of(&pack, &[10, 20, 30]))],
    );

    let mut after = before.clone();
    let taken = edit::remove(&pack, &mut after, "items", 1).expect("remove");
    assert_ne!(after, before, "the remove changed something");

    edit::insert_at(&pack, &mut after, "items", 1, taken).expect("put it back");
    assert_eq!(
        after, before,
        "the tree must be identical after undoing, down to the flags sharing the \
         count's word. An inverse that is merely equivalent leaves an undo stack \
         drifting from the file it describes."
    );
}

/// Out of range is an error rather than a panic, on both operations.
#[test]
fn an_index_past_the_end_is_refused_on_both_operations() {
    let pack = simple_pack();
    let mut root = root_with(&pack, &[10]);

    assert!(edit::insert_at(&pack, &mut root, "items", 9, an_item(&pack, 2)).is_err());
    assert!(edit::remove(&pack, &mut root, "items", 5).is_err());
    assert_eq!(int_of(&root, "count"), 1, "neither changed the count");
    assert_eq!(len_in(&mut root, "items"), 1);

    // Inserting *at* the end is the append case and must work.
    edit::insert_at(&pack, &mut root, "items", 1, an_item(&pack, 2)).expect("append");
    assert_eq!(ids_in(&mut root, "items"), vec![10, 2]);
}
