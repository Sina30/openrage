//! The ABI has a version number, and a plugin built for an older one still loads and runs as it was
//! built. What interface 0 had is frozen here by name, so a change to it fails this suite rather
//! than a plugin somebody already installed. A plugin built for a newer interface is refused with a
//! sentence, and one that declares an older interface may not want what only a newer one has.

use or_io::desc::Pack;
use or_plugin::{
    extension, AlwaysWritable, Capability, Grants, Limits, Manifest, Rejected, Runtime, Session,
    Subject, ABI_VERSION, OLDEST_ABI,
};

fn wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("the fixture must be valid WAT")
}

/// The host functions interface 0 defined for a guest that runs once on a document, by name.
const RUNS_ONCE_AT_0: [&str; 11] = [
    "root_type",
    "field_count",
    "field_name",
    "type_size",
    "field_value",
    "follow",
    "element_count",
    "patch_field",
    "file_size",
    "read_file",
    "write_file",
];

/// The host functions interface 0 defined for a guest that stays loaded.
const STAYS_LOADED_AT_0: [&str; 10] = [
    "register_panel",
    "draw_widget",
    "register_command",
    "declare_setting",
    "setting_value",
    "set_overlay",
    "overlay_picking",
    "file_size",
    "read_file",
    "write_file",
];

/// The host functions interface 0 defined for a plugin that extends a build.
const EXTENDS_AT_0: [&str; 4] = ["plan_size", "read_plan", "report", "add_file"];

/// The keys of the capabilities interface 0 defined.
const CAPABILITIES_AT_0: [&str; 15] = [
    "cap.read_open_document",
    "cap.edit_open_document",
    "cap.read_file_bytes",
    "cap.write_file_bytes",
    "cap.read_install",
    "cap.write_install",
    "cap.read_user_files",
    "cap.network",
    "cap.add_panels",
    "cap.add_commands",
    "cap.add_settings",
    "cap.draw_overlays",
    "cap.read_plan",
    "cap.report_findings",
    "cap.add_files",
];

fn grants_of(since: u32) -> Grants {
    Capability::ALL
        .into_iter()
        .filter(|c| c.since() <= since)
        .fold(Grants::none(), |grants, c| grants.granting(c))
}

fn sorted(mut names: Vec<&'static str>) -> Vec<&'static str> {
    names.sort_unstable();
    names
}

#[test]
fn what_interface_0_defined_is_still_defined_and_a_grant_of_it_buys_nothing_more() {
    let old = grants_of(0);
    assert_eq!(
        sorted(Runtime::functions_for(&old)),
        sorted(RUNS_ONCE_AT_0.to_vec()),
        "a plugin granted only what interface 0 had would otherwise reach a function it was not built for"
    );
    assert_eq!(
        sorted(Session::functions_for(&old)),
        sorted(STAYS_LOADED_AT_0.to_vec())
    );
    assert_eq!(
        sorted(extension::functions_for(&old)),
        sorted(EXTENDS_AT_0.to_vec())
    );

    let keys: Vec<&str> = Capability::ALL
        .into_iter()
        .filter(|c| c.since() == 0)
        .map(Capability::key)
        .collect();
    assert_eq!(
        keys, CAPABILITIES_AT_0,
        "a key interface 0 declared is spelled as it was"
    );
}

#[test]
fn every_capability_added_since_says_which_interface_added_it() {
    let added: Vec<&str> = Capability::ALL
        .into_iter()
        .filter(|c| c.since() > OLDEST_ABI)
        .map(Capability::key)
        .collect();
    assert_eq!(
        added,
        ["cap.write_files", "cap.read_pack", "cap.suggest_fixes"]
    );
    assert!(Capability::ALL
        .into_iter()
        .all(|c| c.since() <= ABI_VERSION));
}

#[test]
fn a_guest_built_for_interface_0_runs_under_this_host_as_it_was_built() {
    let guest = wasm(
        r#"(module
  (import "openrage" "root_type" (func $root_type (param i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "go")
    (if (i32.ne (call $root_type (i32.const 64) (i32.const 32) (i32.const 0)) (i32.const 0))
      (then unreachable))
    (if (i32.ne (i32.load (i32.const 0)) (i32.const 4)) (then unreachable))
    (if (i32.ne (i32.load (i32.const 64)) (i32.const 0x746f6f52)) (then unreachable))))"#,
    );
    let manifest = Manifest {
        id: "old".into(),
        name: "Old".into(),
        abi: 0,
        wants: vec![Capability::ReadOpenDocument],
    };
    assert_eq!(manifest.accept(), Ok(()));
    assert!(manifest.is_older());

    let pack = Pack::new("probe", "gen8");
    Runtime::new(Limits::default())
        .run(
            &guest,
            "go",
            Subject::describing(&pack, "Root"),
            manifest.requested(),
            AlwaysWritable,
            &or_plugin::WouldBeInvalid("read only".into()),
        )
        .expect("the guest found `Root` in its memory");
}

#[test]
fn the_cloth_painter_the_editor_ships_declares_interface_0_and_is_accepted() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../plugins/cloth-painter/plugin.json"),
    )
    .expect("the painter's manifest");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        json["abi"], 0,
        "the painter is built for interface 0 and keeps running as it was"
    );
    let wants: Vec<Capability> = Capability::ALL
        .into_iter()
        .filter(|c| {
            json["wants"]
                .as_array()
                .unwrap()
                .iter()
                .any(|k| *k == c.key())
        })
        .collect();
    let manifest = Manifest {
        id: json["id"].as_str().unwrap().into(),
        name: json["name"].as_str().unwrap().into(),
        abi: 0,
        wants,
    };
    assert_eq!(manifest.accept(), Ok(()));
    assert!(manifest
        .age_note()
        .is_some_and(|n| n.contains("still loads")));
}

#[test]
fn a_plugin_built_for_a_newer_interface_than_the_host_knows_is_refused_with_a_sentence() {
    let later = Manifest {
        id: "later".into(),
        name: "Later".into(),
        abi: ABI_VERSION + 1,
        wants: vec![Capability::ReadOpenDocument],
    };
    let refused = later.accept().expect_err("a newer interface is not run");
    assert_eq!(
        refused,
        Rejected::Newer {
            declared: ABI_VERSION + 1,
            known: ABI_VERSION
        }
    );
    let said = refused.to_string();
    assert!(
        said.contains("update OpenRage") && said.contains("interface"),
        "{said}"
    );

    // How the host that knew only interface 0 answered a plugin built for 1: the same refusal, in the same words.
    let current = Manifest {
        abi: ABI_VERSION,
        ..later
    };
    let from_before = current
        .accept_up_to(0)
        .expect_err("an older host refuses a plugin built later");
    assert!(
        from_before.to_string().contains("update OpenRage"),
        "{from_before}"
    );
}
