//! A guest that stays loaded registers a panel, a command and a setting, describes
//! widgets each frame, draws over the viewport and is told what the user did, each
//! through the capability that buys it and nothing else.
//!
//! ⚠ The sample guest is built with its `surface` feature by a nested `cargo
//! build` for wasm32-unknown-unknown into this target's scratch directory, so
//! that target must be installed; a failure to build fails the suite rather
//! than skipping it.

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, OnceLock};

use or_plugin::{
    AcceptsAnything, AlwaysWritable, Because, Call, Capability, CommandSpec, Event, Grants,
    HostError, Limits, NeverWritable, PanelSpec, Pick, Primitive, SaveCheck, Session, SettingKind,
    SettingSpec, SettingValue, Status, Widget, WouldBeInvalid,
};

const WASM32: &str = "wasm32-unknown-unknown";

fn wasm(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("the fixture must be valid WAT")
}

fn sample_guest() -> &'static [u8] {
    static BUILT: OnceLock<Vec<u8>> = OnceLock::new();
    BUILT.get_or_init(|| {
        let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
        let target_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugins-surface");
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .current_dir(&plugins)
            .args([
                "build",
                "--release",
                "-p",
                "or-plugin-sample",
                "--features",
                "surface",
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

const SURFACE: [Capability; 4] = [
    Capability::AddPanels,
    Capability::AddCommands,
    Capability::AddSettings,
    Capability::DrawOverlays,
];

fn surface() -> Grants {
    SURFACE
        .into_iter()
        .fold(Grants::none(), |grants, c| grants.granting(c))
}

fn opened() -> Session {
    Session::open(sample_guest(), surface(), Limits::default()).expect("the sample registers")
}

fn nothing() -> Call {
    Call::without_a_document()
}

fn status_line(widgets: &[Widget]) -> String {
    match widgets.last() {
        Some(Widget::Label { text }) => text.clone(),
        other => panic!("the sample ends its panel with a status line: {other:?}"),
    }
}

#[test]
fn a_guest_registers_a_panel_a_command_and_a_setting_when_it_starts() {
    let session = opened();
    let registered = session.registered();

    assert_eq!(
        registered.panels,
        [
            PanelSpec {
                id: "sample".into(),
                title: "Sample".into()
            },
            PanelSpec {
                id: "second".into(),
                title: "Second".into()
            }
        ]
    );
    assert_eq!(
        registered.commands,
        [CommandSpec {
            id: "add-point".into(),
            label: "Sample: add a point".into(),
            shortcut: Some("Ctrl+Alt+P".into())
        }]
    );
    assert_eq!(
        registered.settings,
        [SettingSpec {
            id: "scale".into(),
            label: "Scale".into(),
            kind: SettingKind::Number {
                min: 0.1,
                max: 10.0,
                default: 1.0
            }
        }]
    );
    assert_eq!(
        session.setting("scale"),
        Some(&SettingValue::Number(1.0)),
        "a setting is its default until the user changes it"
    );
    assert!(session.overlay().is_empty() && !session.takes_clicks());
}

#[test]
fn a_panel_describes_every_widget_and_a_click_reaches_the_guest_once() {
    let mut session = opened();

    let first = session.draw_panel(0, nothing()).expect("it draws");
    assert!(first.outcome.refused.is_empty());
    let kinds: Vec<&str> = first
        .widgets
        .iter()
        .map(|w| match w {
            Widget::Button { .. } => "button",
            Widget::Slider { .. } => "slider",
            Widget::Checkbox { .. } => "checkbox",
            Widget::TextField { .. } => "text field",
            Widget::List { .. } => "list",
            Widget::Label { .. } => "label",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "button",
            "slider",
            "checkbox",
            "text field",
            "list",
            "checkbox",
            "label"
        ],
        "the guest's own widgets, in the order it described them"
    );
    assert!(status_line(&first.widgets).starts_with("pressed 0 times"));

    session.deliver(0, Event::Clicked { id: "press".into() });
    let after = session.draw_panel(0, nothing()).expect("it draws");
    assert!(
        status_line(&after.widgets).starts_with("pressed 1 times"),
        "the click reached the guest, which counted it: {:?}",
        after.widgets
    );

    let again = session.draw_panel(0, nothing()).expect("it draws");
    assert!(
        status_line(&again.widgets).starts_with("pressed 1 times"),
        "an event answers one call: {:?}",
        again.widgets
    );

    session.deliver(
        0,
        Event::Slid {
            id: "press".into(),
            value: 1.0,
        },
    );
    let other = session.draw_panel(0, nothing()).expect("it draws");
    assert!(
        status_line(&other.widgets).starts_with("pressed 1 times"),
        "an event of another kind than its widget's is not a click: {:?}",
        other.widgets
    );
}

#[test]
fn each_widget_answers_the_value_the_user_left_it_at() {
    let mut session = opened();
    session.draw_panel(0, nothing()).expect("it draws");
    session.deliver(
        0,
        Event::Slid {
            id: "weight".into(),
            value: 5.0,
        },
    );
    session.deliver(
        0,
        Event::Checked {
            id: "pin".into(),
            checked: true,
        },
    );
    session.deliver(
        0,
        Event::Typed {
            id: "name".into(),
            text: "left".into(),
        },
    );
    session.deliver(
        0,
        Event::Chose {
            id: "choice".into(),
            index: 1,
        },
    );
    session.deliver(
        0,
        Event::Chose {
            id: "choice".into(),
            index: 7,
        },
    );

    let drawn = session.draw_panel(0, nothing()).expect("it draws");
    let said = status_line(&drawn.widgets);
    assert_eq!(
        said, "pressed 0 times, weight 1.00, pinned true, name left, chose None",
        "a slider is kept to its range, and a list item that is not there is not chosen"
    );
    assert!(
        drawn.widgets.iter().any(
            |w| matches!(w, Widget::Slider { id, value, .. } if id == "weight" && *value == 1.0)
        ),
        "and the host draws the value the guest was given back: {:?}",
        drawn.widgets
    );

    session.deliver(
        0,
        Event::Chose {
            id: "choice".into(),
            index: 1,
        },
    );
    let drawn = session.draw_panel(0, nothing()).expect("it draws");
    assert!(status_line(&drawn.widgets).ends_with("chose Some(1)"));
}

#[test]
fn a_command_runs_in_the_guest_which_reads_its_setting_and_draws_an_overlay() {
    let mut session = opened();
    session.run_command(0, nothing()).expect("the command runs");
    let epoch = session.overlay_epoch();
    let Some(Primitive::Point { at, .. }) = session.overlay().last() else {
        panic!("{:?}", session.overlay());
    };
    assert_eq!(at, &[1.0, 0.0, 0.0], "the default scale");

    assert_eq!(
        session.set_setting("scale", SettingValue::Number(3.0)),
        Some(&SettingValue::Number(3.0))
    );
    session.run_command(0, nothing()).expect("it runs again");
    let Some(Primitive::Point { at, .. }) = session.overlay().last() else {
        panic!("{:?}", session.overlay());
    };
    assert_eq!(at, &[3.0, 0.0, 0.0], "the guest read what the user chose");
    assert!(session.overlay_epoch() > epoch);

    let epoch = session.overlay_epoch();
    session
        .run_command(0, nothing())
        .expect("it runs a third time");
    assert_eq!(
        session.overlay_epoch(),
        epoch,
        "an overlay the guest draws again as it was is not a change"
    );

    assert_eq!(
        session.set_setting("scale", SettingValue::Number(99.0)),
        Some(&SettingValue::Number(10.0)),
        "kept to the range the guest declared"
    );
    assert_eq!(session.set_setting("scale", SettingValue::Bool(true)), None);
    assert_eq!(session.set_setting("nope", SettingValue::Bool(true)), None);
    assert!(
        session.run_command(1, nothing()).is_err(),
        "no command is registered at 1"
    );
}

#[test]
fn a_pick_reaches_the_guest_that_asked_for_clicks_as_five_numbers() {
    let mut session = opened();
    assert!(!session.takes_clicks());
    session.draw_panel(0, nothing()).expect("it draws");
    session.deliver(
        0,
        Event::Checked {
            id: "pick".into(),
            checked: true,
        },
    );
    session.draw_panel(0, nothing()).expect("it draws");
    assert!(session.takes_clicks(), "the guest asked for the clicks");

    session
        .picked(
            Pick {
                lod: 1,
                model: 2,
                geometry: 3,
                polygon: 4,
                vertex: 5,
            },
            nothing(),
        )
        .expect("the guest takes the pick");
    assert_eq!(
        session.overlay(),
        [Primitive::Point {
            at: [1.0, 2.0, 3.0],
            size: 4.0,
            colour: [5.0, 0.0, 0.0, 1.0]
        }]
    );
}

/// The WAT arity of each host function a session links, which the table below must cover.
const ARITY: &[(&str, usize)] = &[
    ("register_panel", 2),
    ("draw_widget", 5),
    ("register_command", 2),
    ("declare_setting", 2),
    ("setting_value", 5),
    ("set_overlay", 2),
    ("overlay_picking", 1),
    ("file_size", 1),
    ("read_file", 3),
    ("write_file", 5),
    ("write_files", 5),
];

/// A guest whose `register` calls `name` with zeros, which every function answers with a status.
fn calling(name: &str, arity: usize) -> Vec<u8> {
    let params = vec!["i32"; arity].join(" ");
    let zeros = vec!["(i32.const 0)"; arity].join(" ");
    wasm(&format!(
        r#"(module
  (import "openrage" "{name}" (func $f (param {params}) (result i32)))
  (memory (export "memory") 1)
  (func (export "register") (drop (call $f {zeros}))))"#
    ))
}

#[test]
fn a_guest_holds_only_the_functions_its_grants_buy() {
    let all = Session::functions_for(&Grants::trusted_with_everything());
    for name in &all {
        assert!(
            ARITY.iter().any(|(n, _)| n == name),
            "`{name}` is a session function this test has no arity for"
        );
    }
    assert_eq!(all.len(), ARITY.len());

    for (name, arity) in ARITY {
        let needs = Session::capability_for(name).expect("every function has a capability");
        let guest = calling(name, *arity);

        let refused = Session::open(&guest, Grants::none(), Limits::default())
            .err()
            .unwrap_or_else(|| panic!("`{name}` linked without {needs:?}"));
        assert!(
            matches!(&refused, HostError::Unresolved { import, needs: Some(c) }
                if import == name && *c == needs),
            "`{name}` fails instantiation naming what buys it: {refused}"
        );
        assert!(
            refused.to_string().contains(needs.asks_for()),
            "and the sentence says what to allow: {refused}"
        );

        Session::open(&guest, Grants::none().granting(needs), Limits::default())
            .unwrap_or_else(|e| panic!("`{name}` is bought by {needs:?}: {e}"));

        for other in Capability::ALL.into_iter().filter(|c| *c != needs) {
            let others = Session::open(&guest, Grants::none().granting(other), Limits::default());
            assert!(
                others.is_err() || Session::capability_for(name) == Some(other),
                "`{name}` linked with {other:?}, which does not buy it"
            );
        }
    }
}

/// `bytes` as a WAT data segment at `at`.
fn data(at: usize, bytes: &[u8]) -> String {
    let escaped: String = bytes.iter().map(|b| format!("\\{b:02x}")).collect();
    format!("(data (i32.const {at}) \"{escaped}\")")
}

#[test]
fn a_registration_is_taken_while_the_guest_starts_and_never_replaces_another() {
    let panel = PanelSpec {
        id: "a".into(),
        title: "A".into(),
    }
    .encode();
    let spaced = PanelSpec {
        id: "a b".into(),
        title: "A".into(),
    }
    .encode();
    let guest = wasm(&format!(
        r#"(module
  (import "openrage" "register_panel" (func $register_panel (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  {a}
  {b}
  (func (export "register")
    (if (i32.ne (call $register_panel (i32.const 0) (i32.const {la})) (i32.const 0))
      (then unreachable))
    ;; the same id again is refused, not merged
    (if (i32.ne (call $register_panel (i32.const 0) (i32.const {la})) (i32.const 6))
      (then unreachable))
    ;; an id with a space in it is not a plain one
    (if (i32.ne (call $register_panel (i32.const 128) (i32.const {lb})) (i32.const 6))
      (then unreachable)))
  (func (export "draw_panel") (param i32)
    ;; once the guest has started, there is nothing to register with
    (if (i32.ne (call $register_panel (i32.const 0) (i32.const {la})) (i32.const 6))
      (then unreachable))))"#,
        a = data(0, &panel),
        b = data(128, &spaced),
        la = panel.len(),
        lb = spaced.len(),
    ));

    let mut session = Session::open(
        &guest,
        Grants::none().granting(Capability::AddPanels),
        Limits::default(),
    )
    .expect("it started, its refusals answered as statuses");
    assert_eq!(session.registered().panels.len(), 1);
    let refused: Vec<&str> = (session.started().failures.iter())
        .filter(|f| f.function == "register_panel" && f.status == Status::Refused)
        .filter_map(|f| f.because.as_deref())
        .collect();
    assert_eq!(
        refused.len(),
        2,
        "the two registrations the host refused are kept, each with its sentence: {refused:?}"
    );
    assert!(refused[0].contains("registered twice"), "{refused:?}");

    let drawn = session.draw_panel(0, nothing()).expect("it draws");
    let refusal = drawn
        .outcome
        .failures
        .iter()
        .find(|f| f.function == "register_panel")
        .expect("the late registration is a failure the host keeps");
    assert_eq!(refusal.status, Status::Refused);
    assert!(
        refusal
            .because
            .as_deref()
            .is_some_and(|why| why.contains("while the plugin starts")),
        "{refusal:?}"
    );
}

#[test]
fn a_text_too_long_for_the_guests_buffer_is_left_for_its_retry() {
    let panel = PanelSpec {
        id: "a".into(),
        title: "A".into(),
    }
    .encode();
    let field = Widget::TextField {
        id: "name".into(),
        label: "Name".into(),
        text: "old".into(),
    }
    .encode();
    let guest = wasm(&format!(
        r#"(module
  (import "openrage" "register_panel" (func $register_panel (param i32 i32) (result i32)))
  (import "openrage" "draw_widget"
    (func $draw (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  {panel}
  {field}
  (func (export "register")
    (drop (call $register_panel (i32.const 0) (i32.const {lp}))))
  (func (export "draw_panel") (param i32)
    ;; one byte of room: too small, and the length it needs is reported
    (if (i32.ne (call $draw (i32.const 128) (i32.const {lf})
                  (i32.const 1024) (i32.const 1) (i32.const 1100))
                (i32.const 1))
      (then unreachable))
    (if (i32.ne (i32.load (i32.const 1100)) (i32.const 4)) (then unreachable))
    ;; with room, the answer is what the user typed, which the first call did not consume
    (if (i32.ne (call $draw (i32.const 128) (i32.const {lf})
                  (i32.const 1024) (i32.const 64) (i32.const 1100))
                (i32.const 0))
      (then unreachable))
    (if (i32.ne (i32.load8_u (i32.const 1024)) (i32.const 108)) (then unreachable))))"#,
        panel = data(0, &panel),
        field = data(128, &field),
        lp = panel.len(),
        lf = field.len(),
    ));
    let mut session = Session::open(
        &guest,
        Grants::none().granting(Capability::AddPanels),
        Limits::default(),
    )
    .expect("it started");

    session.deliver(
        0,
        Event::Typed {
            id: "name".into(),
            text: "left".into(),
        },
    );
    let drawn = session.draw_panel(0, nothing()).expect("it draws");
    assert_eq!(
        drawn.widgets,
        [Widget::TextField {
            id: "name".into(),
            label: "Name".into(),
            text: "left".into()
        }],
        "the call that did not fit described nothing, and the retry described the text once"
    );
}

/// Two panels, `a` and `b`, each a button of the id `x` and then a label saying whether it was clicked.
fn two_panels_guest() -> Vec<u8> {
    let spec = |id: &str| {
        PanelSpec {
            id: id.into(),
            title: id.into(),
        }
        .encode()
    };
    let button = Widget::Button {
        id: "x".into(),
        text: "X".into(),
    }
    .encode();
    let label = |text: &str| Widget::Label { text: text.into() }.encode();
    let (pa, pb) = (spec("a"), spec("b"));
    let (a_yes, a_no, b_yes, b_no) = (
        label("a clicked"),
        label("a idle"),
        label("b clicked"),
        label("b idle"),
    );
    wasm(&format!(
        r#"(module
  (import "openrage" "register_panel" (func $register_panel (param i32 i32) (result i32)))
  (import "openrage" "draw_widget"
    (func $draw (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  {pa} {pb} {button} {a_yes} {a_no} {b_yes} {b_no}
  (func (export "register")
    (drop (call $register_panel (i32.const 0) (i32.const {lpa})))
    (drop (call $register_panel (i32.const 64) (i32.const {lpb}))))
  (func $frame (param i32 i32 i32 i32)
    (drop (call $draw (i32.const 128) (i32.const {lbutton})
                (i32.const 1024) (i32.const 8) (i32.const 1100)))
    (if (i32.load8_u (i32.const 1024))
      (then (drop (call $draw (local.get 0) (local.get 1)
                        (i32.const 1024) (i32.const 8) (i32.const 1100))))
      (else (drop (call $draw (local.get 2) (local.get 3)
                        (i32.const 1024) (i32.const 8) (i32.const 1100))))))
  (func (export "draw_panel") (param i32)
    (if (i32.eqz (local.get 0))
      (then (call $frame (i32.const 192) (i32.const {lay}) (i32.const 256) (i32.const {lan})))
      (else (call $frame (i32.const 320) (i32.const {lby}) (i32.const 384) (i32.const {lbn}))))))"#,
        pa = data(0, &pa),
        pb = data(64, &pb),
        button = data(128, &button),
        a_yes = data(192, &a_yes),
        a_no = data(256, &a_no),
        b_yes = data(320, &b_yes),
        b_no = data(384, &b_no),
        lpa = pa.len(),
        lpb = pb.len(),
        lbutton = button.len(),
        lay = a_yes.len(),
        lan = a_no.len(),
        lby = b_yes.len(),
        lbn = b_no.len(),
    ))
}

fn two_panels() -> Session {
    Session::open(
        &two_panels_guest(),
        Grants::none().granting(Capability::AddPanels),
        Limits::default(),
    )
    .expect("it started")
}

#[test]
fn what_the_user_did_in_one_panel_waits_for_that_panel_and_no_other() {
    let mut session = two_panels();
    assert_eq!(session.registered().panels.len(), 2);

    let line = |session: &mut Session, panel| {
        status_line(
            &session
                .draw_panel(panel, nothing())
                .expect("it draws")
                .widgets,
        )
    };

    session.deliver(0, Event::Clicked { id: "x".into() });
    assert_eq!(line(&mut session, 1), "b idle", "a's click is not b's");
    assert_eq!(
        line(&mut session, 0),
        "a clicked",
        "another panel's frame did not drop panel a's click"
    );
    assert_eq!(line(&mut session, 0), "a idle", "an event answers one call");

    session.deliver(1, Event::Clicked { id: "x".into() });
    assert_eq!(line(&mut session, 0), "a idle", "b's click is not a's");
    assert_eq!(line(&mut session, 1), "b clicked");

    session.deliver(0, Event::Clicked { id: "x".into() });
    session.deliver(1, Event::Clicked { id: "x".into() });
    assert_eq!(line(&mut session, 1), "b clicked");
    assert_eq!(
        line(&mut session, 0),
        "a clicked",
        "one id in two panels holds one event for each"
    );

    session.deliver(7, Event::Clicked { id: "x".into() });
    assert_eq!(
        line(&mut session, 1),
        "b idle",
        "no panel is registered at 7, so nothing waits for it"
    );
}

#[test]
fn a_guest_that_traps_ends_its_session_and_not_the_host() {
    let panel = PanelSpec {
        id: "a".into(),
        title: "A".into(),
    }
    .encode();
    let guest = wasm(&format!(
        r#"(module
  (import "openrage" "register_panel" (func $register_panel (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  {panel}
  (func (export "register") (drop (call $register_panel (i32.const 0) (i32.const {lp}))))
  (func (export "draw_panel") (param i32) unreachable))"#,
        panel = data(0, &panel),
        lp = panel.len(),
    ));
    let mut session = Session::open(
        &guest,
        Grants::none().granting(Capability::AddPanels),
        Limits::default(),
    )
    .expect("it started");

    let trapped = session
        .draw_panel(0, nothing())
        .expect_err("a guest trap arrives as an error with the host intact");
    assert!(matches!(trapped, HostError::Guest(_)), "{trapped}");
    assert!(session.stopped().is_some());

    let after = session
        .draw_panel(0, nothing())
        .expect_err("a trapped guest's memory is not run again");
    assert!(after.to_string().contains("stopped earlier"), "{after}");
}

#[test]
fn a_guest_that_spins_in_a_frame_is_stopped_by_the_fuel_of_that_call() {
    let panel = PanelSpec {
        id: "a".into(),
        title: "A".into(),
    }
    .encode();
    let guest = wasm(&format!(
        r#"(module
  (import "openrage" "register_panel" (func $register_panel (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  {panel}
  (func (export "register") (drop (call $register_panel (i32.const 0) (i32.const {lp}))))
  (func (export "draw_panel") (param i32) (loop $forever (br $forever))))"#,
        panel = data(0, &panel),
        lp = panel.len(),
    ));
    let mut session = Session::open(
        &guest,
        Grants::none().granting(Capability::AddPanels),
        Limits::default(),
    )
    .expect("it started");
    assert!(
        session.draw_panel(0, nothing()).is_err(),
        "a frame that never ends is not waited for"
    );
}

/// A command that reads the open file's first byte and writes back `NEW` and that byte.
fn rewriting_guest() -> Vec<u8> {
    let command = CommandSpec {
        id: "rewrite".into(),
        label: "Rewrite".into(),
        shortcut: None,
    }
    .encode();
    wasm(&format!(
        r#"(module
  (import "openrage" "register_command" (func $register_command (param i32 i32) (result i32)))
  (import "openrage" "file_size" (func $file_size (param i32) (result i32)))
  (import "openrage" "read_file" (func $read_file (param i32 i32 i32) (result i32)))
  (import "openrage" "write_file"
    (func $write_file (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  {command}
  (func (export "register")
    (drop (call $register_command (i32.const 0) (i32.const {len}))))
  (func (export "run_command") (param i32)
    (if (i32.eqz (call $file_size (i32.const 600)))
      (then
        (drop (call $read_file (i32.const 0) (i32.const 703) (i32.const 1)))
        (i32.store8 (i32.const 700) (i32.const 78))
        (i32.store8 (i32.const 701) (i32.const 69))
        (i32.store8 (i32.const 702) (i32.const 87))
        (drop (call $write_file (i32.const 700) (i32.const 4)
                 (i32.const 800) (i32.const 128) (i32.const 640)))))))"#,
        command = data(0, &command),
        len = command.len(),
    ))
}

fn files() -> Grants {
    Grants::none()
        .granting(Capability::AddCommands)
        .granting(Capability::ReadFileBytes)
        .granting(Capability::WriteFileBytes)
}

fn serving(gate: impl or_plugin::WriteGate + 'static, check: impl SaveCheck + 'static) -> Call {
    Call::new(gate, check).with_file(Arc::new(b"OLDFILE".to_vec()))
}

#[test]
fn a_call_reads_the_file_it_was_handed_and_the_file_it_writes_is_the_embedders_to_keep() {
    let mut session =
        Session::open(&rewriting_guest(), files(), Limits::default()).expect("it started");

    let outcome = session
        .run_command(0, serving(AlwaysWritable, AcceptsAnything))
        .expect("it runs");
    assert_eq!(
        outcome.replaced.as_deref(),
        Some(&b"NEWO"[..]),
        "it read the first byte of the file it was handed and wrote it behind `NEW`"
    );
    assert!(outcome.touched && outcome.refused.is_empty());

    let outcome = session
        .run_command(0, nothing())
        .expect("a call with no file is not a trap");
    assert_eq!(outcome.replaced, None);
    let no_file = outcome
        .failures
        .iter()
        .find(|f| f.function == "file_size")
        .expect("the refusal is kept");
    assert_eq!(no_file.status, Status::Refused);
    assert!(
        no_file
            .because
            .as_deref()
            .is_some_and(|why| why.contains("no file")),
        "{no_file:?}"
    );
}

#[test]
fn a_write_the_gate_or_the_check_refuses_changes_nothing_and_says_why() {
    let mut session =
        Session::open(&rewriting_guest(), files(), Limits::default()).expect("it started");

    let outcome = session
        .run_command(
            0,
            serving(
                NeverWritable("the editor is read-only".into()),
                AcceptsAnything,
            ),
        )
        .expect("it runs");
    assert_eq!(outcome.replaced, None, "{outcome:?}");
    assert!(
        matches!(
            outcome.refused.as_slice(),
            [refused] if refused.capability == Capability::WriteFileBytes
                && refused.because == Because::EmbedderRefusesWrites("the editor is read-only".into())
        ),
        "the refusal names the capability and the editor's own sentence: {:?}",
        outcome.refused
    );
    assert!(outcome
        .failures
        .iter()
        .any(|f| f.function == "write_file" && f.status == Status::NotWritable));

    let outcome = session
        .run_command(
            0,
            serving(AlwaysWritable, WouldBeInvalid("it would not load".into())),
        )
        .expect("it runs");
    assert_eq!(outcome.replaced, None);
    assert!(
        outcome.failures.iter().any(|f| f.function == "write_file"
            && f.status == Status::Refused
            && f.because.as_deref() == Some("the file was not replaced: it would not load")),
        "{:?}",
        outcome.failures
    );
}
