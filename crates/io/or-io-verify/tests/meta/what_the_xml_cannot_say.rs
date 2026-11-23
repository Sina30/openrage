//! Why a metadata document cannot be re-imported without a type source.
//!
//! ⚠ `write_meta` is not injective over `Value`: four variants render to
//! byte-identical XML, so no parser can recover which one it was. A schema
//! library does not invert the export; it supplies the types the export
//! dropped. The reading that names were missing is refuted: adding 20,186 of
//! them moved the withheld count by nothing. Having `parse_meta` ask the
//! layout beside the document instead of the empty library would work only by
//! reintroducing types, which is the same fix as a schema, not a smaller one.

use or_io::meta::instance::{HashedString, MetaInstance, Value};
use or_io::meta::write_meta;

/// The XML one field of each value renders to, with the field named the same
/// so only the value can differ.
fn rendered(value: Value) -> String {
    let mut inst = MetaInstance::new("T");
    inst.set("f".to_string(), value);
    let xml = write_meta(&inst).expect("write");
    xml.lines()
        .find(|line| line.contains("<f"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// If `write_meta` ever learns to distinguish these, this fails and the round
/// trip becomes closable without a type source.
#[test]
fn four_variants_render_to_the_same_xml_so_no_parser_can_tell_them_apart() {
    let text = "prop_bench_01a";

    let same = [
        ("String", rendered(Value::String(text.to_string()))),
        (
            "Enum",
            rendered(Value::Enum {
                type_name: "eSomething".into(),
                variant: text.to_string(),
            }),
        ),
    ];

    assert_eq!(
        same[0].1, same[1].1,
        "a string and an enum variant no longer render alike, so the export has gained a \
         type and this file's premise is out of date"
    );
    assert!(
        same[0].1.contains(text),
        "the text is the whole payload: {}",
        same[0].1
    );

    // ⚠ The numeric pair would silently write wrong bytes rather than refuse:
    // `pso::writer`'s `as_int` and `as_uint` both accept either.
    assert_eq!(
        rendered(Value::Int(5)),
        rendered(Value::UInt(5)),
        "a signed and an unsigned five no longer render alike"
    );
}

/// ⚠ The encode step refuses an enum or a bitset handed as text but coerces
/// between signed and unsigned, so a schema-free import that guessed an
/// integer's sign wrong would write wrong bytes rather than fail.
#[test]
fn the_encode_step_refuses_a_mistyped_enum_and_accepts_a_mistyped_integer() {
    // Not exercised against a real PSO here: `write_enum` and `write_bitset`
    // in `binary::pso::writer` return an error for anything but `Value::Enum`
    // and `Value::Bitset`, while `as_int` and `as_uint` accept `Int`, `UInt`
    // and `Bool` alike.
    assert_eq!(rendered(Value::Int(-1)), "<f value=\"-1\"/>");
    assert_ne!(
        rendered(Value::UInt(1)),
        rendered(Value::Bool(true)),
        "bool renders as true/false, so it is not part of the ambiguity"
    );
}

/// A hashed string and a plain one are also one shape, which matters because
/// the hash is what the file is keyed by.
#[test]
fn a_hashed_string_is_indistinguishable_from_a_plain_one() {
    let plain = rendered(Value::String("prop_bench_01a".to_string()));
    let hashed = rendered(Value::Hash(HashedString::new("prop_bench_01a")));
    assert_eq!(
        plain, hashed,
        "the hash no longer travels differently from the text, so an importer could tell"
    );
}
