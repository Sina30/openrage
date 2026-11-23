//! A sample guest: doubles every `f32` field of the document's root, or, built
//! with the `bytes` feature, reads the open `.ydd` and writes it back with the
//! first level-of-detail distance of every drawable doubled, or, built with the
//! `surface` feature, registers a panel, a command and a setting and draws an overlay, or, built with the
//! `extend` feature, validates and emits for a build, or, built with the `fixes` feature, reads the
//! clothing packs of a build and suggests a fix with a finding, or, built with the `writes` feature,
//! writes two files of the project together.

#[cfg(not(any(feature = "bytes", feature = "surface", feature = "extend", feature = "fixes", feature = "writes")))]
use or_plugin_guest::{
    field_count, field_name, field_value, patch_field, root_type, Status, Value,
};

/// The entry the host runs; a panic traps and ends the run.
#[cfg(not(any(feature = "bytes", feature = "surface", feature = "extend", feature = "fixes", feature = "writes")))]
#[no_mangle]
pub extern "C" fn go() {
    let root = root_type().expect("every document has a root");
    for index in 0..field_count(&root).expect("the root is described") {
        let name = field_name(&root, index).expect("the index is in range");
        match field_value(&root, 0, &name) {
            Ok(Value::F32(v)) => {
                patch_field(&root, 0, &name, &Value::F32(v * 2.0)).expect("a scalar patches")
            }
            Ok(_) | Err(Status::Refused) => {}
            Err(status) => panic!("reading `{name}` answered {status:?}"),
        }
    }
}

/// Reads the open `.ydd` with `or-io` linked into this module and writes it back with the first
/// level-of-detail distance of every drawable doubled.
#[cfg(feature = "bytes")]
fn double_first_lod() -> Result<(), String> {
    use or_io::gta5::{File, GtaDrawable, PgDictionary};

    let file = or_plugin_guest::file_bytes()
        .map_err(|status| format!("the host would not serve the open file: {status:?}"))?;
    let mut dictionary = PgDictionary::<GtaDrawable>::from_bytes(&file)
        .map_err(|e| format!("the open file is not a Legacy drawable dictionary: {e}"))?;
    for drawable in dictionary.entries.iter_mut().flatten() {
        drawable.base.lod_group.lod_distances[0] *= 2.0;
    }
    let bytes = dictionary
        .to_bytes()
        .map_err(|e| format!("the dictionary would not save: {e}"))?;
    or_plugin_guest::write_file(&bytes)
        .map_err(|refusal| format!("the host refused the file: {}", refusal.why))
}

/// The entry the host runs; a refusal traps, which ends the run with the refusal.
#[cfg(all(feature = "bytes", not(feature = "surface")))]
#[no_mangle]
pub extern "C" fn go() {
    if let Err(why) = double_first_lod() {
        panic!("{why}");
    }
}

/// The panel, command, setting and overlay the host holds the surface to: `register` adds one of
/// each and a second panel whose button has the first's id, the first panel describes every widget
/// kind and ends with a line saying what reached the guest,
/// the command draws a line and a point at the `scale` setting, and a pick draws a point whose
/// position, size and colour carry its five numbers. Built with `bytes` as well, a second command
/// doubles the first level-of-detail distance of the open `.ydd`, and a refusal of the file leaves
/// the guest running, for the host to show.
#[cfg(feature = "surface")]
mod surface {
    use std::cell::RefCell;

    use or_plugin_guest::{
        declare_setting, overlay_picking, register_command, register_panel, set_overlay,
        setting_value, ui, Primitive, SettingKind, SettingValue, Vertex,
    };

    #[derive(Default)]
    struct Model {
        pressed: u32,
        tallied: u32,
        weight: f32,
        pinned: bool,
        name: String,
        chose: Option<u32>,
        picking: bool,
    }

    thread_local! {
        static MODEL: RefCell<Model> = RefCell::new(Model::default());
    }

    #[no_mangle]
    pub extern "C" fn register() {
        register_panel("sample", "Sample").expect("the panels grant buys this");
        register_panel("second", "Second").expect("the panels grant buys this");
        register_command("add-point", "Sample: add a point", Some("Ctrl+Alt+P"))
            .expect("the commands grant buys this");
        #[cfg(feature = "bytes")]
        register_command("double-lod", "Sample: double the first LOD distance", None)
            .expect("the commands grant buys this");
        declare_setting(
            "scale",
            "Scale",
            &SettingKind::Number {
                min: 0.1,
                max: 10.0,
                default: 1.0,
            },
        )
        .expect("the settings grant buys this");
    }

    #[no_mangle]
    pub extern "C" fn draw_panel(index: i32) {
        assert!(index <= 1, "the sample registers two panels");
        MODEL.with(|model| {
            let mut model = model.borrow_mut();
            if index == 1 {
                // The same widget id as the first panel's button: an event belongs to its panel.
                if ui::button("press", "Press") {
                    model.tallied += 1;
                }
                ui::label(&format!("tallied {} times", model.tallied));
                return;
            }
            if ui::button("press", "Press") {
                model.pressed += 1;
            }
            model.weight = ui::slider("weight", "Weight", 0.0, 1.0, model.weight);
            model.pinned = ui::checkbox("pin", "Pin", model.pinned);
            let name = ui::text_field("name", "Name", &model.name);
            model.name = name;
            model.chose = ui::list("choice", &["Front", "Back"], model.chose);
            let picking = ui::checkbox("pick", "Pick in the viewport", model.picking);
            if picking != model.picking {
                model.picking = picking;
                overlay_picking(picking).expect("the overlay grant buys this");
            }
            ui::label(&format!(
                "pressed {} times, weight {:.2}, pinned {}, name {}, chose {:?}",
                model.pressed, model.weight, model.pinned, model.name, model.chose
            ));
        });
    }

    #[no_mangle]
    pub extern "C" fn run_command(index: i32) {
        #[cfg(feature = "bytes")]
        if index == 1 {
            // A refusal is the host's to show: it keeps the sentence for the user.
            let _ = crate::double_first_lod();
            return;
        }
        assert_eq!(index, 0, "the sample registers one command to draw with");
        let Ok(SettingValue::Number(scale)) = setting_value("scale") else {
            panic!("the sample declared a number");
        };
        let end = [scale as f32, 0.0, 0.0];
        set_overlay(&[
            Primitive::Line {
                from: Vertex {
                    at: [0.0; 3],
                    colour: [0.0, 0.0, 1.0, 1.0],
                },
                to: Vertex {
                    at: end,
                    colour: [1.0, 0.0, 0.0, 1.0],
                },
            },
            Primitive::Point {
                at: end,
                size: 0.1,
                colour: [1.0, 0.0, 0.0, 1.0],
            },
        ])
        .expect("the overlay grant buys this");
    }

    #[no_mangle]
    pub extern "C" fn picked(lod: i32, model: i32, geometry: i32, polygon: i32, vertex: i32) {
        set_overlay(&[Primitive::Point {
            at: [lod as f32, model as f32, geometry as f32],
            size: polygon as f32,
            colour: [vertex as f32, 0.0, 0.0, 1.0],
        }])
        .expect("the overlay grant buys this");
    }
}

/// A validator and an emitter, built with the `extend` feature: `validate` reports on the paths and sizes
/// of the plan it reads, and `emit` adds a catalogue of the plan's items as a JSON file.
#[cfg(feature = "extend")]
mod extend {
    use or_plugin_guest::{add_file, plan, report, Level};
    use serde_json::{json, Value};

    /// A file over this many bytes earns a warning.
    const LARGE: u64 = 1 << 20;

    fn document() -> Value {
        let bytes = plan().expect("the plan grant buys the plan");
        serde_json::from_slice(&bytes).expect("the plan is the JSON document the command line prints")
    }

    #[no_mangle]
    pub extern "C" fn validate() {
        for file in document()["files"].as_array().into_iter().flatten() {
            let path = file["path"].as_str().unwrap_or_default();
            if path.contains(' ') {
                report(
                    "space-in-path",
                    Level::Error,
                    Some(path),
                    &format!("`{path}` has a space in its path, which a manifest lists awkwardly"),
                )
                .expect("the findings grant buys this");
            }
            let bytes = file["bytes"].as_u64().unwrap_or(0);
            if bytes > LARGE {
                report(
                    "large-file",
                    Level::Warning,
                    Some(path),
                    &format!("`{path}` is {bytes} bytes, over the {LARGE} this sample warns at"),
                )
                .expect("the findings grant buys this");
            }
        }
    }

    #[no_mangle]
    pub extern "C" fn emit() {
        let plan = document();
        let catalogue = json!({
            "resource": plan["resource"],
            "files": plan["totals"]["files"],
            "items": plan["items"],
        });
        let text = serde_json::to_string_pretty(&catalogue).expect("a JSON value serialises") + "\n";
        add_file("config/catalogue.json", &text).expect("the files grant buys this");
    }
}

/// A validator that reads the clothing packs of a build and suggests a fix, built with the `fixes` feature (ABI 1):
/// `validate` finds a garment with no label and suggests the pack file with one.
#[cfg(feature = "fixes")]
mod fixes {
    use or_plugin_guest::{pack, report_with_fix, Level};
    use serde_json::Value;

    #[no_mangle]
    pub extern "C" fn validate() {
        let bytes = pack().expect("the pack grant buys the pack");
        let document: Value =
            serde_json::from_slice(&bytes).expect("the pack is the JSON document the host describes");
        for entry in document["packs"].as_array().into_iter().flatten() {
            let folder = entry["folder"].as_str().unwrap_or_default();
            let mut wardrobe = entry["wardrobe"].clone();
            let unlabelled: Vec<usize> = wardrobe["garments"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .filter(|(_, garment)| garment["label"].as_str().is_none_or(str::is_empty))
                .map(|(at, _)| at)
                .collect();
            if unlabelled.is_empty() {
                continue;
            }
            for at in &unlabelled {
                wardrobe["garments"][*at]["label"] = Value::String(format!("Garment {}", at + 1));
            }
            let text = serde_json::to_string_pretty(&wardrobe).expect("a JSON value serialises");
            report_with_fix(
                "garment-unlabelled",
                Level::Warning,
                entry["content"].as_str(),
                &format!(
                    "{} garment(s) have no label, so a list can only show their files",
                    unlabelled.len()
                ),
                "Give each a label",
                &[(&format!("{folder}/wardrobe.json"), text.as_bytes())],
            )
            .expect("the fixes grant buys this");
        }
    }
}

/// An entry that writes two files of the project together, built with the `writes` feature (ABI 1).
///
/// A refusal leaves the guest running: the host shows its sentence, and the guest has nothing to add.
#[cfg(feature = "writes")]
#[no_mangle]
pub extern "C" fn go() {
    let _ = or_plugin_guest::write_files(&[("notes/first.txt", b"one"), ("notes/second.txt", b"two")]);
}
