//! The cloth painter of R46 is a plugin: it installs from its manifest, draws a panel and an overlay
//! over a `.yld`, paints per-vertex values, and hands the host a file the host reads back before it
//! keeps it. Nothing in OpenRage is its own.
//!
//! ⚠ The guest is built with a nested `cargo build` for wasm32-unknown-unknown, as the sample is.

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, OnceLock};

use or_io::gta5::shared::{AtBitSet, Vec3V};
use or_io::gta5::{
    CharacterCloth, CharacterClothController, ClothBridgeSimGfx, ClothController, File,
    PgDictionary, PhClothData, PhVerletCloth,
};
use or_plugin::{
    AlwaysWritable, Call, Capability, Drawn, Event, HostError, Limits, Manifest, NeverWritable,
    Primitive, SaveCheck, Session, Widget, WouldBeInvalid, WriteGate, ABI_VERSION,
};
use or_test_support::{shared_corpus_root, Corpus, TARGET_GEN8};

const WASM32: &str = "wasm32-unknown-unknown";

type Dictionary = PgDictionary<CharacterCloth>;

fn guest() -> &'static [u8] {
    static BUILT: OnceLock<Vec<u8>> = OnceLock::new();
    BUILT.get_or_init(|| {
        let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
        let target_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugins-cloth-painter");
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .current_dir(&plugins)
            .args([
                "build",
                "--release",
                "-p",
                "or-plugin-cloth-painter",
                "--target",
                WASM32,
            ])
            .arg("--target-dir")
            .arg(&target_dir)
            .status()
            .expect("cargo runs");
        assert!(
            status.success(),
            "the cloth painter did not build for {WASM32}; if the target is missing, `rustup target add {WASM32}`"
        );
        std::fs::read(
            target_dir
                .join(WASM32)
                .join("release/or_plugin_cloth_painter.wasm"),
        )
        .expect("the build wrote the module")
    })
}

/// The manifest the plugin ships, as the app reads `plugin.json`.
fn shipped_manifest() -> Manifest {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/cloth-painter/plugin.json");
    let text = std::fs::read_to_string(&path).expect("the plugin ships a manifest");
    let json: serde_json::Value = serde_json::from_str(&text).expect("the manifest is JSON");
    let keys: Vec<&str> = json["wants"]
        .as_array()
        .expect("it states what it wants")
        .iter()
        .filter_map(|k| k.as_str())
        .collect();
    let wants: Vec<Capability> = keys
        .iter()
        .map(|key| {
            Capability::ALL
                .into_iter()
                .find(|c| c.key() == *key)
                .unwrap_or_else(|| panic!("the manifest names `{key}`, which is no capability"))
        })
        .collect();
    assert_eq!(wants.len(), keys.len());
    Manifest {
        id: json["id"].as_str().expect("an id").to_string(),
        name: json["name"].as_str().expect("a name").to_string(),
        abi: u32::try_from(json["abi"].as_u64().expect("an abi")).unwrap(),
        wants,
    }
}

/// The save check the app has for a `.yld`: the bytes read back as a cloth dictionary and save again.
struct ReadsBackAsAClothDictionary;

impl SaveCheck for ReadsBackAsAClothDictionary {
    fn why_the_document_would_be_invalid(&self, _: &[u8]) -> Option<String> {
        None
    }

    fn why_the_file_would_be_invalid(&self, file: &[u8]) -> Option<String> {
        match Dictionary::from_bytes(file) {
            Ok(dictionary) => dictionary.to_bytes().err().map(|e| e.to_string()),
            Err(e) => Some(format!("the bytes do not read back: {e}")),
        }
    }
}

/// What parsing and saving a cloth with `or-io` inside the guest spends: more than the default allows.
fn parsing() -> Limits {
    Limits {
        fuel: Some(4_000_000_000),
        ..Limits::default()
    }
}

fn open() -> Session {
    let grants = shipped_manifest().requested();
    Session::open(guest(), grants, parsing()).expect("the painter starts under its manifest")
}

/// A session being driven one panel frame at a time over a file.
struct Frames {
    session: Session,
    file: Arc<Vec<u8>>,
    wrote: Option<Vec<u8>>,
}

impl Frames {
    fn over(file: Vec<u8>) -> Self {
        Self {
            session: open(),
            file: Arc::new(file),
            wrote: None,
        }
    }

    fn draw(
        &mut self,
        events: Vec<Event>,
        gate: impl WriteGate + 'static,
        check: impl SaveCheck + 'static,
    ) -> Drawn {
        for event in events {
            self.session.deliver(0, event);
        }
        let call = Call::new(gate, check).with_file(self.file.clone());
        let mut drawn = self.session.draw_panel(0, call).expect("the panel draws");
        // What the app does with a file the host kept: it is the open document from now on.
        if let Some(bytes) = drawn.outcome.replaced.take() {
            self.file = Arc::new(bytes.clone());
            self.wrote = Some(bytes);
        }
        drawn
    }

    fn frame(&mut self, events: Vec<Event>) -> Drawn {
        self.draw(events, AlwaysWritable, ReadsBackAsAClothDictionary)
    }
}

fn clicked(id: &str) -> Event {
    Event::Clicked { id: id.into() }
}

fn slid(id: &str, value: f32) -> Event {
    Event::Slid {
        id: id.into(),
        value,
    }
}

fn typed(id: &str, text: &str) -> Event {
    Event::Typed {
        id: id.into(),
        text: text.into(),
    }
}

fn chose(id: &str, index: u32) -> Event {
    Event::Chose {
        id: id.into(),
        index,
    }
}

fn labels(drawn: &Drawn) -> Vec<&str> {
    drawn
        .widgets
        .iter()
        .filter_map(|w| match w {
            Widget::Label { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn says(drawn: &Drawn, needle: &str) -> bool {
    labels(drawn).iter().any(|text| text.contains(needle))
}

/// A cloth of eight vertices on a slant, every per-vertex value stored.
fn synthetic() -> Dictionary {
    let at: Vec<Vec3V> = (0..8)
        .map(|i| Vec3V {
            x: i as f32,
            y: (i % 3) as f32,
            z: 0.5 * i as f32,
            ..Default::default()
        })
        .collect();
    let level = PhVerletCloth {
        cloth_data: PhClothData {
            vertex_positions: at.clone(),
            vertex_prev_positions: at,
            vertex_initial_normals: vec![Vec3V::default(); 8],
            ..Default::default()
        },
        ..Default::default()
    };
    let bridge = ClothBridgeSimGfx {
        vertex_weight_0: vec![0.25; 8],
        inflation_scale_0: vec![1.0; 8],
        pin_radius_0: vec![0.5; 8],
        pinnable_list: AtBitSet {
            bits: vec![0],
            bit_size: 8,
            ..Default::default()
        },
        ..Default::default()
    };
    let controller = CharacterClothController {
        base: ClothController {
            bridge: Some(Box::new(bridge)),
            cloth_0: Some(Box::new(level)),
            ..Default::default()
        },
        tri_indices: vec![0, 1, 2, 2, 3, 4, 4, 5, 6],
        ..Default::default()
    };
    let cloth = CharacterCloth {
        cloth_controller: Some(Box::new(controller)),
        ..Default::default()
    };
    Dictionary {
        codes: vec![0x1234],
        entries: vec![Some(Box::new(cloth))],
        ..Default::default()
    }
}

fn bytes_of(dictionary: &Dictionary) -> Vec<u8> {
    dictionary
        .to_bytes()
        .expect("the synthetic cloth saves, so its per-vertex runs agree")
}

fn weights(dictionary: &Dictionary) -> Vec<f32> {
    let cloth = dictionary.entries[0].as_deref().unwrap();
    let bridge = cloth
        .cloth_controller
        .as_deref()
        .unwrap()
        .base
        .bridge
        .as_deref()
        .unwrap();
    bridge.vertex_weight_0.clone()
}

fn bridge_of(dictionary: &Dictionary) -> &ClothBridgeSimGfx {
    dictionary.entries[0]
        .as_deref()
        .unwrap()
        .cloth_controller
        .as_deref()
        .unwrap()
        .base
        .bridge
        .as_deref()
        .unwrap()
}

#[test]
fn the_manifest_the_plugin_ships_buys_every_import_the_guest_makes_and_no_other() {
    let manifest = shipped_manifest();
    assert_eq!(manifest.accept(), Ok(()));
    // Built for interface 0 and declared so: the host runs it as it was built, and says in the manager that it is older.
    assert_eq!(manifest.abi, 0);
    assert!(manifest.is_older() && manifest.abi <= ABI_VERSION);
    assert!(manifest.adds_to_the_editor() && !manifest.extends_a_build());

    Session::open(guest(), manifest.requested(), parsing()).expect("it starts as shipped");

    for dropped in &manifest.wants {
        let mut wants = manifest.wants.clone();
        wants.retain(|c| c != dropped);
        let grants = Manifest {
            wants,
            ..manifest.clone()
        }
        .requested();
        let refused = Session::open(guest(), grants, parsing())
            .err()
            .unwrap_or_else(|| {
                panic!("the painter starts without {dropped:?}, so it asks for more than it uses")
            });
        assert!(
            matches!(&refused, HostError::Unresolved { needs: Some(c), .. } if c == dropped),
            "without {dropped:?} it names what is missing: {refused}"
        );
    }
}

#[test]
fn it_registers_a_panel_a_command_and_two_settings() {
    let session = open();
    let registered = session.registered();
    assert_eq!(
        registered
            .panels
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["painter"]
    );
    assert_eq!(
        registered
            .commands
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["write"]
    );
    assert_eq!(
        registered
            .settings
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["overlay", "size"]
    );
    assert_eq!(session.started().failures, [], "the host refused nothing");
}

#[test]
fn with_no_document_it_says_so_and_with_one_it_shows_the_level_and_its_vertices() {
    let mut session = open();
    let drawn = session
        .draw_panel(0, Call::without_a_document())
        .expect("it draws with nothing open");
    assert!(says(&drawn, "No document is open"), "{:?}", labels(&drawn));
    assert!(session.overlay().is_empty());

    let mut frames = Frames::over(bytes_of(&synthetic()));
    let drawn = frames.frame(vec![]);
    assert!(
        says(&drawn, "8 vertices, 0 selected, 0 pinnable"),
        "{:?}",
        labels(&drawn)
    );
    assert!(
        says(&drawn, "Weight 0.250 to 0.250 over 8"),
        "what the file stores is what the panel says: {:?}",
        labels(&drawn)
    );
    let points = frames
        .session
        .overlay()
        .iter()
        .filter(|p| matches!(p, Primitive::Point { .. }))
        .count();
    assert_eq!(points, 8, "one mark for each vertex of the level");
}

#[test]
fn painting_the_selected_vertices_changes_those_and_nothing_else_in_the_file_it_writes() {
    let original = synthetic();
    let mut frames = Frames::over(bytes_of(&original));
    frames.frame(vec![]);

    let drawn = frames.frame(vec![typed("indices", "1-3, 6"), clicked("these")]);
    let painted = frames.frame(vec![typed("weight", "0.75"), clicked("paint_weight")]);
    assert!(
        says(&painted, "Painted 4 vertices"),
        "{:?} then {:?}",
        labels(&drawn),
        labels(&painted)
    );
    assert!(says(&painted, "Trial export:"), "{:?}", labels(&painted));

    let selected = frames
        .session
        .overlay()
        .iter()
        .filter(|p| matches!(p, Primitive::Point { colour, .. } if colour[0] == 1.0 && colour[1] == 0.9))
        .count();
    assert_eq!(selected, 4, "the selection is drawn apart");

    frames.frame(vec![clicked("write")]);
    let written = frames.wrote.expect("the host kept the file");

    let result = Dictionary::from_bytes(&written).expect("the written file is a cloth dictionary");
    let mut expected = original.clone();
    for v in [1, 2, 3, 6] {
        expected.entries[0]
            .as_deref_mut()
            .unwrap()
            .cloth_controller
            .as_deref_mut()
            .unwrap()
            .base
            .bridge
            .as_deref_mut()
            .unwrap()
            .vertex_weight_0[v] = 0.75;
    }
    assert_eq!(
        weights(&result),
        [0.25, 0.75, 0.75, 0.75, 0.25, 0.25, 0.75, 0.25]
    );
    assert_same(
        &result,
        &canon(&expected),
        "the written file is the original with the weight of four vertices changed, and nothing else",
    );
}

#[test]
fn a_gradient_runs_from_the_low_end_of_the_axis_to_the_high_end() {
    let mut frames = Frames::over(bytes_of(&synthetic()));
    frames.frame(vec![]);
    frames.frame(vec![clicked("all")]);
    // X is axis 0; the vertices sit at x = 0 to 7.
    frames.frame(vec![
        chose("axis", 0),
        typed("from", "0.0"),
        typed("to", "1.0"),
        clicked("paint_gradient"),
    ]);
    frames.frame(vec![clicked("write")]);
    let result = Dictionary::from_bytes(&frames.wrote.expect("kept")).unwrap();

    let got = weights(&result);
    for (v, w) in got.iter().enumerate() {
        let want = v as f32 / 7.0;
        assert!((w - want).abs() < 1e-6, "vertex {v}: {w} against {want}");
    }
}

#[test]
fn inflation_pin_radius_and_pinning_are_painted_where_they_are_stored() {
    let mut frames = Frames::over(bytes_of(&synthetic()));
    frames.frame(vec![]);
    frames.frame(vec![typed("indices", "0, 7"), clicked("these")]);
    frames.frame(vec![typed("inflation", "2.5"), clicked("paint_inflation")]);
    frames.frame(vec![
        typed("pin_radius", "0.125"),
        clicked("paint_pin_radius"),
    ]);
    frames.frame(vec![clicked("pin")]);
    let drawn = frames.frame(vec![]);
    assert!(says(&drawn, "2 pinnable"), "{:?}", labels(&drawn));
    frames.frame(vec![clicked("write")]);
    let result = Dictionary::from_bytes(&frames.wrote.expect("kept")).unwrap();
    let bridge = bridge_of(&result);

    assert_eq!(
        bridge.inflation_scale_0,
        [2.5, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.5]
    );
    assert_eq!(
        bridge.pin_radius_0,
        [0.125, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.125]
    );
    assert!(bridge.is_pinnable(0) && bridge.is_pinnable(7));
    assert!((1..7).all(|v| !bridge.is_pinnable(v)));
}

#[test]
fn a_run_the_cloth_does_not_store_is_not_invented() {
    let mut dictionary = synthetic();
    dictionary.entries[0]
        .as_deref_mut()
        .unwrap()
        .cloth_controller
        .as_deref_mut()
        .unwrap()
        .base
        .bridge
        .as_deref_mut()
        .unwrap()
        .inflation_scale_0 = Vec::new();
    let mut frames = Frames::over(bytes_of(&dictionary));
    frames.frame(vec![]);
    frames.frame(vec![clicked("all")]);
    let drawn = frames.frame(vec![typed("inflation", "2.0"), clicked("paint_inflation")]);
    assert!(
        says(&drawn, "stores no inflation scales"),
        "{:?}",
        labels(&drawn)
    );
    let drawn = frames.frame(vec![clicked("write")]);
    let result = Dictionary::from_bytes(&frames.wrote.expect("kept")).unwrap();
    assert!(says(&drawn, "Wrote"), "{:?}", labels(&drawn));
    assert_same(
        &result,
        &canon(&dictionary),
        "a stroke that could not be made changed nothing",
    );
}

#[test]
fn the_force_of_the_cloth_is_set_from_the_numbers_typed_and_a_word_that_is_no_number_is_said() {
    let original = synthetic();
    let mut frames = Frames::over(bytes_of(&original));
    let drawn = frames.frame(vec![]);
    assert!(
        says(
            &drawn,
            "The cloth's force is [0.0, 0.0, 0.0], negated: false"
        ),
        "{:?}",
        labels(&drawn)
    );

    let drawn = frames.frame(vec![typed("f1", "two"), clicked("set_force")]);
    assert!(
        says(&drawn, "`two`, which is not a number"),
        "{:?}",
        labels(&drawn)
    );
    frames.frame(vec![clicked("write")]);
    let result = Dictionary::from_bytes(&frames.wrote.take().expect("kept")).unwrap();
    assert_same(
        &result,
        &canon(&original),
        "a force that was not a number changed nothing",
    );

    frames.frame(vec![
        typed("f0", "1.5"),
        typed("f1", "-2"),
        typed("f2", "0.25"),
        Event::Checked {
            id: "negate".into(),
            checked: true,
        },
        clicked("set_force"),
    ]);
    frames.frame(vec![clicked("write")]);
    let result = Dictionary::from_bytes(&frames.wrote.take().expect("kept")).unwrap();
    let cloth = result.entries[0].as_deref().unwrap();
    assert_eq!(
        (
            cloth.force.x,
            cloth.force.y,
            cloth.force.z,
            cloth.negate_force.get()
        ),
        (1.5, -2.0, 0.25, true)
    );
}

#[test]
fn a_brush_value_that_is_no_number_paints_nothing_and_says_which() {
    let mut frames = Frames::over(bytes_of(&synthetic()));
    frames.frame(vec![]);
    frames.frame(vec![clicked("all")]);
    let drawn = frames.frame(vec![typed("weight", "0,5"), clicked("paint_weight")]);
    assert!(
        says(&drawn, "the weight is `0,5`, which is not a number"),
        "{:?}",
        labels(&drawn)
    );
    let drawn = frames.frame(vec![typed("weight", "NaN"), clicked("paint_weight")]);
    assert!(
        says(&drawn, "`NaN`, which is not a number"),
        "{:?}",
        labels(&drawn)
    );
    let drawn = frames.frame(vec![clicked("undo")]);
    assert!(
        says(&drawn, "no stroke to take back"),
        "nothing was painted: {:?}",
        labels(&drawn)
    );
}

#[test]
fn a_stroke_is_taken_back() {
    let original = synthetic();
    let mut frames = Frames::over(bytes_of(&original));
    frames.frame(vec![]);
    frames.frame(vec![clicked("all")]);
    frames.frame(vec![typed("weight", "0.9"), clicked("paint_weight")]);
    let drawn = frames.frame(vec![clicked("undo")]);
    assert!(
        says(&drawn, "Took the last stroke back"),
        "{:?}",
        labels(&drawn)
    );
    let drawn = frames.frame(vec![clicked("undo")]);
    assert!(
        says(&drawn, "no stroke to take back"),
        "{:?}",
        labels(&drawn)
    );
    frames.frame(vec![clicked("write")]);
    let result = Dictionary::from_bytes(&frames.wrote.expect("kept")).unwrap();
    assert_same(
        &result,
        &canon(&original),
        "a stroke taken back leaves the original",
    );
}

#[test]
fn a_selection_by_region_takes_the_vertices_inside_the_box() {
    let mut frames = Frames::over(bytes_of(&synthetic()));
    frames.frame(vec![]);
    // The vertices sit at x = 0 to 7: the lower half of the extent holds the first four.
    let drawn = frames.frame(vec![slid("hi0", 0.5), clicked("region")]);
    assert!(
        says(&drawn, "8 vertices, 0 selected"),
        "the count is drawn before the click lands"
    );
    let drawn = frames.frame(vec![]);
    assert!(
        says(&drawn, "8 vertices, 4 selected"),
        "{:?}",
        labels(&drawn)
    );
    frames.frame(vec![clicked("invert")]);
    let drawn = frames.frame(vec![]);
    assert!(
        says(&drawn, "8 vertices, 4 selected"),
        "{:?} {:?}",
        labels(&drawn),
        drawn.widgets.len()
    );
    frames.frame(vec![clicked("clear")]);
    let drawn = frames.frame(vec![]);
    assert!(
        says(&drawn, "8 vertices, 0 selected"),
        "{:?}",
        labels(&drawn)
    );
}

#[test]
fn a_file_the_host_would_not_keep_is_refused_with_its_sentence_and_the_painter_goes_on() {
    let mut frames = Frames::over(bytes_of(&synthetic()));
    frames.frame(vec![]);
    frames.frame(vec![clicked("all")]);
    frames.frame(vec![typed("weight", "0.5"), clicked("paint_weight")]);

    let drawn = frames.draw(
        vec![clicked("write")],
        AlwaysWritable,
        WouldBeInvalid("the check says no".into()),
    );
    assert!(frames.wrote.is_none(), "nothing was kept");
    assert!(
        says(&drawn, "the check says no"),
        "the host's sentence is shown: {:?}",
        labels(&drawn)
    );

    let drawn = frames.draw(
        vec![clicked("write")],
        NeverWritable("the editor is in read-only mode".into()),
        ReadsBackAsAClothDictionary,
    );
    assert!(frames.wrote.is_none());
    assert!(
        says(&drawn, "read-only mode"),
        "the gate's sentence is shown: {:?}",
        labels(&drawn)
    );

    frames.frame(vec![clicked("write")]);
    assert!(frames.wrote.is_some(), "and it writes once it may");
}

#[test]
fn a_file_that_is_no_cloth_dictionary_is_said_so_and_not_trapped_on() {
    let mut frames = Frames::over(b"not a game file".to_vec());
    let drawn = frames.frame(vec![]);
    assert!(
        says(&drawn, "is not a cloth dictionary"),
        "{:?}",
        labels(&drawn)
    );
}

#[test]
fn the_write_command_writes_the_painted_dictionary() {
    let mut frames = Frames::over(bytes_of(&synthetic()));
    frames.frame(vec![]);
    frames.frame(vec![clicked("all")]);
    frames.frame(vec![typed("weight", "0.5"), clicked("paint_weight")]);

    let call =
        Call::new(AlwaysWritable, ReadsBackAsAClothDictionary).with_file(frames.file.clone());
    let outcome = frames
        .session
        .run_command(0, call)
        .expect("the command runs");
    let written = outcome.replaced.expect("the host kept the file");
    let result = Dictionary::from_bytes(&written).unwrap();
    assert_eq!(weights(&result), [0.5; 8]);
}

#[test]
fn every_staged_cloth_dictionary_keeps_everything_but_the_vertices_painted() {
    let Some(files) = Corpus::with_root(shared_corpus_root(env!("CARGO_MANIFEST_DIR")), "yld")
        .target(TARGET_GEN8)
        .limit(6)
        .find_or_skip("a staged .yld through the cloth painter")
    else {
        return;
    };
    let mut painted = 0usize;
    for path in files {
        let bytes = std::fs::read(&path).unwrap();
        let original = Dictionary::from_bytes(&bytes)
            .unwrap_or_else(|e| panic!("{} does not open: {e}", path.display()));
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let mut frames = Frames::over(bytes);
        frames.frame(vec![]);
        frames.frame(vec![clicked("all")]);
        let drawn = frames.frame(vec![typed("weight", "0.5"), clicked("paint_weight")]);
        let first = original.entries.first().and_then(|c| c.as_deref());
        let vertices = first.map_or(0, |c| c.lod_vertex_count(0));
        let stored = first
            .and_then(|c| c.cloth_controller.as_deref())
            .and_then(|c| c.base.bridge.as_deref())
            .map_or(0, |b| b.vertex_weight_0.len());
        if vertices == 0 || stored == 0 {
            assert!(
                !says(&drawn, "Painted "),
                "{name}: it painted a level that stores nothing to paint: {:?}",
                labels(&drawn)
            );
            continue;
        }
        let drawn = frames.frame(vec![clicked("write")]);
        let written = frames
            .wrote
            .unwrap_or_else(|| panic!("{name}: the host kept nothing: {:?}", labels(&drawn)));

        // The expectation: the dictionary the file holds, with the first level's stored weights
        // set from vertex 0 on, and nothing else changed.
        let mut expected = original.clone();
        let weights = &mut expected.entries[0]
            .as_deref_mut()
            .and_then(|c| c.cloth_controller.as_deref_mut())
            .and_then(|c| c.base.bridge.as_deref_mut())
            .unwrap()
            .vertex_weight_0;
        weights.iter_mut().take(vertices).for_each(|w| *w = 0.5);
        assert_same(
            &Dictionary::from_bytes(&written).unwrap(),
            &canon(&expected),
            &format!("{name}: the written file differs from the original in more than the weights of level 0"),
        );
        painted += 1;
    }
    assert!(
        painted > 0,
        "at least one staged cloth stores weights at its first level"
    );
}

/// A dictionary as a save and an open leave it, which is what the host hands a guest and takes back.
fn canon(dictionary: &Dictionary) -> Dictionary {
    Dictionary::from_bytes(&bytes_of(dictionary)).expect("what it saves opens")
}

/// Two dictionaries are the same, or the test says where their debug forms first differ.
///
/// ⚠ Compared as text and not with `==`: a shipped cloth holds NaN in a position's fourth word, and a
/// NaN is not equal to itself.
fn assert_same(got: &Dictionary, want: &Dictionary, what: &str) {
    let (got, want) = (format!("{got:?}"), format!("{want:?}"));
    if got == want {
        return;
    }
    let at = got
        .bytes()
        .zip(want.bytes())
        .position(|(a, b)| a != b)
        .unwrap_or(got.len().min(want.len()));
    let from = at.saturating_sub(80);
    let cut = |text: &str| {
        text.get(from..(at + 80).min(text.len()))
            .unwrap_or("")
            .to_string()
    };
    panic!(
        "{what}: first difference at {at}\n got: ...{}\nwant: ...{}",
        cut(&got),
        cut(&want)
    );
}

#[test]
fn a_file_the_painter_reads_for_the_first_time_is_not_said_to_have_changed_but_one_changed_under_it_is(
) {
    let original = synthetic();
    let mut frames = Frames::over(bytes_of(&original));
    let drawn = frames.frame(vec![]);
    assert!(
        !says(&drawn, "changed"),
        "the first reading is not a change: {:?}",
        labels(&drawn)
    );

    frames.frame(vec![clicked("all")]);
    frames.frame(vec![typed("weight", "0.5"), clicked("paint_weight")]);
    // Another tool, or the editor's own undo, put another cloth in the document.
    let mut other = synthetic();
    other.entries[0]
        .as_deref_mut()
        .unwrap()
        .cloth_controller
        .as_deref_mut()
        .unwrap()
        .base
        .bridge
        .as_deref_mut()
        .unwrap()
        .vertex_weight_0[0] = 0.75;
    frames.file = Arc::new(bytes_of(&other));
    let drawn = frames.frame(vec![]);
    assert!(
        says(&drawn, "The open file changed"),
        "{:?}",
        labels(&drawn)
    );
    let drawn = frames.frame(vec![clicked("undo")]);
    assert!(
        says(&drawn, "no stroke to take back"),
        "the strokes were made on a file that is not the open one any more: {:?}",
        labels(&drawn)
    );
}
