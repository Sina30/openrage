//! The guest library in `plugins/guest`, held to the host's statuses and
//! encoding, and the sample guest in `plugins/sample`, built from it for
//! wasm32 and run through the host as an author's plugin would be.
//!
//! ⚠ The sample is built by a nested `cargo build` into this target's scratch
//! directory, so the wasm32-unknown-unknown target must be installed; its
//! absence fails the suite rather than skipping it.

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use or_io::desc::value::ScalarValue;
use or_io::desc::{
    FieldRepr, IntWidth, Pack, PackEnum, PackField, PackType, PackVariant, Pointee, Scalar, Value,
};
use or_plugin::{
    abi, AcceptsAnything, AlwaysWritable, Capability, Grants, Limits, NeverWritable, Runtime,
    Status, Subject,
};

/// The guest library's codec, compiled for the host; its host calls exist only on wasm32.
#[allow(dead_code)]
#[path = "../../../../plugins/guest/src/lib.rs"]
mod guest;

const WASM32: &str = "wasm32-unknown-unknown";

fn sample_guest() -> &'static [u8] {
    // Built once: a second build relinks the module under a test that is reading it
    static BUILT: OnceLock<Vec<u8>> = OnceLock::new();
    BUILT.get_or_init(|| {
        let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
        let target_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugins");
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .current_dir(&plugins)
            .args([
                "build",
                "--release",
                "-p",
                "or-plugin-sample",
                "--target",
                WASM32,
            ])
            .arg("--target-dir")
            .arg(&target_dir)
            .status()
            .expect("cargo runs");
        assert!(
            status.success(),
            "the sample guest did not build for {WASM32}; if the target is missing, \
             `rustup target add {WASM32}`"
        );
        std::fs::read(
            target_dir
                .join(WASM32)
                .join("release/or_plugin_sample.wasm"),
        )
        .expect("the build wrote the module")
    })
}

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

/// Two `f32` fields among one of each other shape the sample meets.
fn pack() -> Pack {
    let mut pack = Pack::new("probe", "gen8");
    pack.enums.push(PackEnum {
        name: "BoundType".into(),
        width: IntWidth::U32,
        variants: vec![PackVariant {
            name: "Geometry".into(),
            value: 4,
            note: None,
        }],
    });
    pack.types.push(PackType {
        name: "Root".into(),
        size: 0x20,
        fields: vec![
            field("count", 0x00, FieldRepr::Scalar(Scalar::Int(IntWidth::U32))),
            field("ratio", 0x04, FieldRepr::Scalar(Scalar::F32)),
            field(
                "bound_type",
                0x08,
                FieldRepr::Enum {
                    type_name: "BoundType".into(),
                    width: IntWidth::U32,
                },
            ),
            field("scale", 0x0C, FieldRepr::Scalar(Scalar::F32)),
            field(
                "elsewhere",
                0x10,
                FieldRepr::Ptr(Pointee::One {
                    type_name: "Root".into(),
                }),
            ),
        ],
        selector: None,
    });
    pack
}

fn f32_at(bytes: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

#[test]
fn the_sample_guest_doubles_every_f32_of_the_root() {
    let pack = pack();
    let mut bytes = vec![0u8; 0x20];
    bytes[0x00..0x04].copy_from_slice(&7u32.to_le_bytes());
    bytes[0x04..0x08].copy_from_slice(&2.5f32.to_le_bytes());
    bytes[0x08..0x0C].copy_from_slice(&4u32.to_le_bytes());
    bytes[0x0C..0x10].copy_from_slice(&(-0.75f32).to_le_bytes());
    let before = bytes.clone();
    let len = bytes.len();

    let outcome = Runtime::new(Limits::default())
        .run(
            sample_guest(),
            "go",
            Subject::editing(&pack, "Root", &mut bytes, len),
            Grants::default().granting(Capability::EditOpenDocument),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect("the sample ran to the end; a trap means a call answered what it did not expect");

    assert!(outcome.touched, "it patched");
    assert_eq!(f32_at(&bytes, 0x04), 5.0);
    assert_eq!(f32_at(&bytes, 0x0C), -1.5);
    assert_eq!(
        bytes[0x00..0x04],
        before[0x00..0x04],
        "an integer is left alone"
    );
    assert_eq!(bytes[0x08..0x0C], before[0x08..0x0C], "and so is an enum");
    assert_eq!(
        outcome
            .failures
            .iter()
            .map(|f| (f.function, f.status))
            .collect::<Vec<_>>(),
        vec![("field_value", Status::Refused)],
        "the one field that is not a value is refused, and the sample goes on"
    );
}

#[test]
fn the_sample_guest_is_refused_without_the_grant_its_patch_costs() {
    let pack = pack();
    let bytes = vec![0u8; 0x20];
    let refused = Runtime::new(Limits::default())
        .run(
            sample_guest(),
            "go",
            Subject::reading(&pack, "Root", &bytes, bytes.len()),
            Grants::default(),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect_err("it imports `patch_field`");
    assert!(
        matches!(
            &refused,
            or_plugin::HostError::Unresolved {
                needs: Some(Capability::EditOpenDocument),
                ..
            }
        ),
        "{refused}"
    );
}

/// The sample panics on a refused patch; the error names the refusal rather than only the trap.
#[test]
fn a_sample_stopped_by_the_write_gate_names_the_refusal() {
    let pack = pack();
    let mut bytes = vec![0u8; 0x20];
    let len = bytes.len();
    let error = Runtime::new(Limits::default())
        .run(
            sample_guest(),
            "go",
            Subject::editing(&pack, "Root", &mut bytes, len),
            Grants::default().granting(Capability::EditOpenDocument),
            NeverWritable("the editor is in read-only mode".into()),
            &AcceptsAnything,
        )
        .expect_err("the sample expects its patch to land");
    let or_plugin::HostError::StoppedAfterRefusal { refused, .. } = &error else {
        panic!("a trap after a refusal is reported with it: {error:?}");
    };
    assert_eq!(
        refused.iter().map(|r| r.capability).collect::<Vec<_>>(),
        vec![Capability::EditOpenDocument]
    );
    assert!(
        error
            .to_string()
            .contains("the editor is in read-only mode"),
        "{error}"
    );
}

#[test]
fn the_guest_library_speaks_the_hosts_version_and_statuses() {
    assert_eq!(guest::ABI_VERSION, or_plugin::ABI_VERSION);
    for status in Status::ALL {
        match guest::Status::check(status.code()) {
            Ok(()) => assert_eq!(status, Status::Ok),
            Err(seen) => assert_eq!(
                (seen as i32, format!("{seen:?}")),
                (status.code(), format!("{status:?}"))
            ),
        }
    }
}

/// Each shape `field_value` answers, encoded by the host and by the guest.
#[test]
fn the_guest_library_encodes_what_the_host_encodes() {
    let cases = [
        (
            Value::int(-9_000_000_000i64),
            guest::Value::Int(-9_000_000_000),
        ),
        (Value::f32(2.5), guest::Value::F32(2.5)),
        (
            Value::Scalar(ScalarValue::F64(-0.125)),
            guest::Value::F64(-0.125),
        ),
        (Value::boolean(true), guest::Value::Bool(true)),
        (
            Value::Enum {
                enum_name: "BoundType".into(),
                value: 4,
            },
            guest::Value::Enum {
                name: "BoundType".into(),
                value: 4,
            },
        ),
        (
            Value::Array {
                elements: vec![Value::int(1), Value::int(2)],
                capacity: Some(8),
            },
            guest::Value::Array {
                elements: vec![guest::Value::Int(1), guest::Value::Int(2)],
                capacity: Some(8),
            },
        ),
        (
            Value::Array {
                elements: vec![Value::f32(1.0)],
                capacity: None,
            },
            guest::Value::Array {
                elements: vec![guest::Value::F32(1.0)],
                capacity: None,
            },
        ),
    ];
    for (host, guest) in cases {
        let bytes = abi::encode(&host).expect("it fits");
        assert_eq!(guest.encode(), bytes, "{guest:?}");
        assert_eq!(guest::Value::decode(&bytes), Some(guest));
    }
    assert_eq!(
        guest::Value::decode(&[abi::tag::BOOL, 1, 0]),
        None,
        "a byte after a whole value is refused"
    );
}

/// Each shape of the surface, encoded by the guest library and decoded by the host, and the
/// host's setting value decoded by the guest.
#[test]
fn the_guest_library_encodes_the_surface_the_host_decodes() {
    use or_plugin::surface::{self, Primitive, SettingKind, SettingSpec, SettingValue, Widget};

    let items = ["a", "b"];
    for (guest, host) in [
        (
            guest::Widget::Label { text: "hi" },
            Widget::Label { text: "hi".into() },
        ),
        (
            guest::Widget::Button {
                id: "go",
                text: "Go",
            },
            Widget::Button {
                id: "go".into(),
                text: "Go".into(),
            },
        ),
        (
            guest::Widget::Slider {
                id: "w",
                label: "Weight",
                min: 0.0,
                max: 1.0,
                value: 0.25,
            },
            Widget::Slider {
                id: "w".into(),
                label: "Weight".into(),
                min: 0.0,
                max: 1.0,
                value: 0.25,
            },
        ),
        (
            guest::Widget::Checkbox {
                id: "c",
                label: "Pin",
                checked: true,
            },
            Widget::Checkbox {
                id: "c".into(),
                label: "Pin".into(),
                checked: true,
            },
        ),
        (
            guest::Widget::TextField {
                id: "t",
                label: "Name",
                text: "é",
            },
            Widget::TextField {
                id: "t".into(),
                label: "Name".into(),
                text: "é".into(),
            },
        ),
        (
            guest::Widget::List {
                id: "l",
                items: &items,
                selected: Some(1),
            },
            Widget::List {
                id: "l".into(),
                items: vec!["a".into(), "b".into()],
                selected: Some(1),
            },
        ),
        (
            guest::Widget::List {
                id: "l",
                items: &[],
                selected: None,
            },
            Widget::List {
                id: "l".into(),
                items: Vec::new(),
                selected: None,
            },
        ),
    ] {
        assert_eq!(guest.encode(), host.encode(), "{guest:?}");
        assert_eq!(Widget::decode(&guest.encode()), Ok(host));
    }

    assert_eq!(
        guest::encode_panel("p", "P"),
        or_plugin::PanelSpec {
            id: "p".into(),
            title: "P".into()
        }
        .encode()
    );
    for shortcut in [None, Some("Ctrl+K")] {
        assert_eq!(
            guest::encode_command("c", "C", shortcut),
            or_plugin::CommandSpec {
                id: "c".into(),
                label: "C".into(),
                shortcut: shortcut.map(String::from),
            }
            .encode()
        );
    }
    for (guest_kind, host_kind) in [
        (
            guest::SettingKind::Bool { default: true },
            SettingKind::Bool { default: true },
        ),
        (
            guest::SettingKind::Number {
                min: -1.0,
                max: 2.0,
                default: 0.5,
            },
            SettingKind::Number {
                min: -1.0,
                max: 2.0,
                default: 0.5,
            },
        ),
        (
            guest::SettingKind::Text {
                default: "x".into(),
            },
            SettingKind::Text {
                default: "x".into(),
            },
        ),
    ] {
        let spec = SettingSpec {
            id: "s".into(),
            label: "S".into(),
            kind: host_kind,
        };
        assert_eq!(guest::encode_setting("s", "S", &guest_kind), spec.encode());
    }
    for (host, guest_value) in [
        (SettingValue::Bool(true), guest::SettingValue::Bool(true)),
        (SettingValue::Number(3.5), guest::SettingValue::Number(3.5)),
        (
            SettingValue::Text("t".into()),
            guest::SettingValue::Text("t".into()),
        ),
    ] {
        assert_eq!(
            guest::SettingValue::decode(&host.encode()),
            Some(guest_value)
        );
    }
    assert_eq!(guest::SettingValue::decode(&[0x01, 2]), None);

    let at = [1.0, 2.0, 3.0];
    let colour = [1.0, 0.0, 0.0, 1.0];
    let vertex = |v: f32| guest::Vertex {
        at: [v; 3],
        colour: [v, 0.0, 0.0, 1.0],
    };
    let overlay = [
        guest::Primitive::Point {
            at,
            size: 0.5,
            colour,
        },
        guest::Primitive::Line {
            from: vertex(0.0),
            to: vertex(1.0),
        },
    ];
    assert_eq!(
        surface::decode_overlay(&guest::encode_overlay(&overlay)),
        Ok(vec![
            Primitive::Point {
                at,
                size: 0.5,
                colour
            },
            Primitive::Line {
                from: surface::Vertex {
                    at: [0.0; 3],
                    colour: [0.0, 0.0, 0.0, 1.0]
                },
                to: surface::Vertex {
                    at: [1.0; 3],
                    colour: [1.0, 0.0, 0.0, 1.0]
                },
            },
        ])
    );
}

/// A finding's level crosses as a number, and the library's names are the host's.
#[test]
fn the_guest_library_names_the_levels_of_a_finding_by_the_codes_the_host_reads() {
    use or_plugin::extension::Level;
    for (guest, host) in [
        (guest::Level::Note, Level::Note),
        (guest::Level::Warning, Level::Warning),
        (guest::Level::Error, Level::Error),
    ] {
        assert_eq!(guest as i32, host.code());
        assert_eq!(Level::from_code(guest as i32), Some(host));
    }
}

#[test]
fn the_guest_library_encodes_files_written_together_as_the_host_decodes_them() {
    let files: [(&str, &[u8]); 3] = [("a/one.txt", b"1"), ("two.json", b"{}"), ("c.bin", &[])];
    let decoded = or_plugin::Transaction::decode(&guest::encode_files(&files)).expect("it decodes");
    assert_eq!(
        decoded
            .files
            .iter()
            .map(|f| (f.path.as_str(), f.bytes.as_slice()))
            .collect::<Vec<_>>(),
        files
    );
}

/// The sample built with one of its version 1 features, each in a target folder of its own.
fn built_with(feature: &'static str, cell: &'static OnceLock<Vec<u8>>) -> &'static [u8] {
    cell.get_or_init(|| {
        let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
        let target_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("plugins-{feature}"));
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .current_dir(&plugins)
            .args([
                "build",
                "--release",
                "-p",
                "or-plugin-sample",
                "--features",
                feature,
                "--target",
                WASM32,
            ])
            .arg("--target-dir")
            .arg(&target_dir)
            .status()
            .expect("cargo runs");
        assert!(
            status.success(),
            "the sample guest did not build for {WASM32}; if the target is missing, \
             `rustup target add {WASM32}`"
        );
        std::fs::read(
            target_dir
                .join(WASM32)
                .join("release/or_plugin_sample.wasm"),
        )
        .expect("the build wrote the module")
    })
}

#[test]
fn the_sample_validator_reads_a_pack_and_suggests_a_fix_through_the_guest_library() {
    static BUILT: OnceLock<Vec<u8>> = OnceLock::new();
    let pack = br#"{"packs":[{"content":"clothing","folder":"clothing","wardrobe":{"pack":"p","garments":[{"id":"a","label":"Named"},{"id":"b"}]}}]}"#;

    let found = or_plugin::extension::validate_with_pack(
        built_with("fixes", &BUILT),
        &Grants::none()
            .granting(Capability::ReadPack)
            .granting(Capability::SuggestFixes),
        Limits::default(),
        b"{}",
        pack,
    )
    .expect("the validator runs");

    let [finding] = found.findings.as_slice() else {
        panic!(
            "one finding expected: {:?} {:?}",
            found.findings, found.failures
        );
    };
    assert_eq!(finding.code, "garment-unlabelled");
    assert_eq!(finding.subject.as_deref(), Some("clothing"));
    let fix = finding.fix.as_ref().expect("it suggests a fix");
    assert_eq!(fix.label, "Give each a label");
    let [file] = fix.files.files.as_slice() else {
        panic!("one file in the fix");
    };
    assert_eq!(file.path, "clothing/wardrobe.json");
    let fixed: serde_json::Value = serde_json::from_slice(&file.bytes).unwrap();
    assert_eq!(
        fixed["garments"][0]["label"], "Named",
        "a label that is there stays"
    );
    assert_eq!(fixed["garments"][1]["label"], "Garment 2");
}

#[test]
fn the_sample_entry_writes_two_files_together_through_the_guest_library() {
    static BUILT: OnceLock<Vec<u8>> = OnceLock::new();
    let pack = Pack::new("probe", "gen8");
    let outcome = Runtime::new(Limits::default())
        .run(
            built_with("writes", &BUILT),
            "go",
            Subject::describing(&pack, "Root"),
            Grants::none().granting(Capability::WriteFiles),
            AlwaysWritable,
            &AcceptsAnything,
        )
        .expect("it runs");

    let kept = outcome.transaction.expect("the files were kept together");
    assert_eq!(
        kept.paths().collect::<Vec<_>>(),
        ["notes/first.txt", "notes/second.txt"]
    );
}
