//! A plugin adds a panel, a command and a setting to the editor and draws over
//! the viewport, each only once the user has said yes to the capability that
//! buys it, and what the user does reaches the guest.
//!
//! ⚠ The sample guest is built with `--features surface` by a nested `cargo
//! build` for wasm32-unknown-unknown, as `a_plugin_asks_before_it_runs` builds
//! the others.

use std::path::PathBuf;

use or_plugin::{
    Capability, Event, Manifest, Pick, Primitive, SettingKind, SettingValue, Widget, ABI_VERSION,
};

use crate::state::a_plugin_asks_before_it_runs::built_sample;
use crate::state::{AppState, Document, DocumentKind};

use super::{Consent, InstalledPlugin};

const SURFACE: [Capability; 4] = [
    Capability::AddPanels,
    Capability::AddCommands,
    Capability::AddSettings,
    Capability::DrawOverlays,
];

fn surface_guest() -> PathBuf {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT
        .get_or_init(|| built_sample("surface", "openrage-plugin-sample-surface"))
        .clone()
}

fn surface_plugin(id: &str, wants: &[Capability]) -> InstalledPlugin {
    InstalledPlugin {
        manifest: Manifest {
            id: id.into(),
            name: "Sample".into(),
            abi: ABI_VERSION,
            wants: wants.to_vec(),
        },
        wasm: surface_guest(),
        refused: None,
    }
}

/// A state with `plugin` installed and a blank declared resource open, whose consent is `answered`.
fn state_with(plugin: &InstalledPlugin, answered: bool) -> AppState {
    let mut state = AppState::default();
    state.plugins.scanned = Some(vec![plugin.clone()]);
    if answered {
        state.prefs.plugin_consent.insert(
            plugin.manifest.id.clone(),
            Consent::Granted(
                plugin
                    .manifest
                    .wants
                    .iter()
                    .map(|c| c.key().to_string())
                    .collect(),
            ),
        );
    }
    state.documents.push(blank(1));
    state.active_document = Some(1);
    state
}

/// A blank declared resource, as a document of id `id`.
fn blank(id: u64) -> Document {
    let kind = DocumentKind::Ybn;
    let resource = kind.resource_type().expect("a paged format");
    let handler = crate::state::payload_handlers::handler_for(resource, or_io::Generation::Gen8)
        .expect("a payload handler");
    let payload = (handler.empty)();
    let bytes = handler.save(&payload).expect("a blank serializes");
    let path = std::env::temp_dir().join(format!("surface{id}.ybn"));
    let mut doc = Document::new(id, Some(path), payload);
    doc.set_resource(crate::state::document_io::resource_image_for(kind, &bytes));
    doc
}

fn loaded(id: &str) -> AppState {
    let mut state = state_with(&surface_plugin(id, &SURFACE), true);
    state.load_plugin_surfaces();
    state
}

/// What the first plugin drawing over the open document draws.
fn overlay(state: &AppState) -> Option<&[Primitive]> {
    state.plugin_overlays().into_iter().next()
}

fn status(widgets: &[Widget]) -> String {
    widgets
        .iter()
        .rev()
        .find_map(|w| match w {
            Widget::Label { text } if text.starts_with("pressed") => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the sample ends its panel with a status line: {widgets:?}"))
}

#[test]
fn the_sample_guests_registered_panel_draws_and_its_button_event_reaches_the_guest() {
    let mut state = loaded("panel");
    let panels = &state.plugin_registrations("panel").expect("loaded").panels;
    assert_eq!(
        panels.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["sample", "second"]
    );

    let widgets = state.plugin_panel("panel", "sample").expect("it draws");
    assert!(
        matches!(widgets.first(), Some(Widget::Button { id, .. }) if id == "press"),
        "the guest's own widgets are what the host got: {widgets:?}"
    );
    assert!(status(&widgets).starts_with("pressed 0 "), "{widgets:?}");

    state.deliver_plugin_events(
        "panel",
        "sample",
        vec![Event::Clicked { id: "press".into() }],
    );
    let widgets = state.plugin_panel("panel", "sample").expect("it draws");
    assert!(
        status(&widgets).starts_with("pressed 1 "),
        "the click reached the guest, which counted it: {widgets:?}"
    );

    let widgets = state.plugin_panel("panel", "sample").expect("it draws");
    assert!(
        status(&widgets).starts_with("pressed 1 "),
        "an event is delivered once: {widgets:?}"
    );
}

fn has_label(widgets: &[Widget], text: &str) -> bool {
    widgets
        .iter()
        .any(|w| matches!(w, Widget::Label { text: t } if t == text))
}

#[test]
fn a_click_in_one_of_a_plugins_panels_waits_for_that_panel_and_no_other() {
    let mut state = loaded("twin");
    state.deliver_plugin_events(
        "twin",
        "second",
        vec![Event::Clicked { id: "press".into() }],
    );

    let first = state.plugin_panel("twin", "sample").expect("it draws");
    assert!(
        status(&first).starts_with("pressed 0 "),
        "the second panel's click is not the first's: {first:?}"
    );
    let second = state.plugin_panel("twin", "second").expect("it draws");
    assert!(
        has_label(&second, "tallied 1 times"),
        "the first panel's frame did not drop the second's click: {second:?}"
    );

    state.deliver_plugin_events(
        "twin",
        "sample",
        vec![Event::Clicked { id: "press".into() }],
    );
    let second = state.plugin_panel("twin", "second").expect("it draws");
    assert!(has_label(&second, "tallied 1 times"), "{second:?}");
    let first = state.plugin_panel("twin", "sample").expect("it draws");
    assert!(status(&first).starts_with("pressed 1 "), "{first:?}");
}

#[test]
fn a_click_on_a_panels_button_in_a_real_frame_survives_a_frame_of_the_plugins_other_panel() {
    use egui::accesskit::Role;

    let mut state = loaded("sibling");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let first = crate::ui::panels::plugin_panel::panel_id("sibling", "sample");
    let second = crate::ui::panels::plugin_panel::panel_id("sibling", "second");

    let drawn = panel_frame(&ctx, &mut state, &second, Vec::new());
    let press = drawn
        .iter()
        .find(|(role, label, _)| *role == Role::Button && label == "Press")
        .unwrap_or_else(|| panic!("the second panel's button is on screen: {drawn:#?}"));
    let at = press.2.center();
    panel_frame(
        &ctx,
        &mut state,
        &second,
        vec![egui::Event::PointerMoved(at)],
    );
    panel_frame(&ctx, &mut state, &second, vec![pointer(at, true)]);
    panel_frame(&ctx, &mut state, &second, vec![pointer(at, false)]);

    let beside = panel_frame(&ctx, &mut state, &first, Vec::new());
    assert!(
        beside
            .iter()
            .any(|(_, label, _)| label.starts_with("pressed 0 times")),
        "{beside:#?}"
    );
    let after = panel_frame(&ctx, &mut state, &second, Vec::new());
    assert!(
        after.iter().any(|(_, label, _)| label == "tallied 1 times"),
        "the click reached the second panel's guest although the first drew in between: {after:#?}"
    );
}

#[test]
fn a_slider_a_checkbox_a_text_field_and_a_list_each_send_their_value_back() {
    let mut state = loaded("widgets");
    state.plugin_panel("widgets", "sample").expect("it draws");
    state.deliver_plugin_events(
        "widgets",
        "sample",
        vec![
            Event::Slid {
                id: "weight".into(),
                value: 0.5,
            },
            Event::Checked {
                id: "pin".into(),
                checked: true,
            },
            Event::Typed {
                id: "name".into(),
                text: "left".into(),
            },
            Event::Chose {
                id: "choice".into(),
                index: 1,
            },
        ],
    );

    let widgets = state.plugin_panel("widgets", "sample").expect("it draws");
    let said = status(&widgets);
    for part in ["weight 0.50", "pinned true", "name left", "chose Some(1)"] {
        assert!(said.contains(part), "`{part}` is missing from {said}");
    }
    assert!(
        widgets.iter().any(
            |w| matches!(w, Widget::Slider { id, value, .. } if id == "weight" && *value == 0.5)
        ),
        "and the host draws the value the user chose: {widgets:?}"
    );
}

#[test]
fn a_command_a_guest_registers_is_offered_with_its_shortcut_and_runs_the_guest() {
    let mut state = loaded("command");
    let command = crate::ui::commands::all(&state)
        .into_iter()
        .find(|c| c.id == "plugin command: command: add-point")
        .expect("the command registry offers it");
    assert_eq!(command.label, "Sample: add a point");
    assert_eq!(
        command.binding.as_deref(),
        Some(format!("{}+Alt+P", crate::ui::shortcuts::primary_modifier_name()).as_str())
    );
    assert_eq!(command.category, "Automate ▸ Plugin commands");
    assert!(command.enabled.is_ok());

    assert!(overlay(&state).is_none(), "nothing is drawn yet");
    crate::ui::commands::run(&mut state, &egui::Context::default(), &command);

    let drawn = overlay(&state).expect("the command ran the guest");
    assert!(
        matches!(drawn, [Primitive::Line { .. }, Primitive::Point { .. }]),
        "{drawn:?}"
    );
}

#[test]
fn a_setting_a_guest_declares_persists_and_the_guest_reads_it() {
    let mut state = loaded("setting");
    let settings = &state
        .plugin_registrations("setting")
        .expect("loaded")
        .settings;
    assert_eq!(settings.len(), 1);
    assert_eq!(settings[0].id, "scale");
    assert!(matches!(settings[0].kind, SettingKind::Number { .. }));
    assert_eq!(
        state.plugin_setting("setting", "scale"),
        Some(SettingValue::Number(1.0)),
        "the default until the user chooses"
    );

    state.set_plugin_setting("setting", "scale", SettingValue::Number(3.0));
    assert_eq!(
        state.plugin_setting("setting", "scale"),
        Some(SettingValue::Number(3.0))
    );
    state.run_plugin_command("setting", "add-point");
    let Some([_, Primitive::Point { at, .. }]) = overlay(&state) else {
        panic!("{:?}", overlay(&state));
    };
    assert_eq!(at[0], 3.0, "the guest read the setting the user changed");

    let mut next = state_with(&surface_plugin("setting", &SURFACE), true);
    next.prefs.plugin_settings = state.prefs.plugin_settings.clone();
    next.load_plugin_surfaces();
    assert_eq!(
        next.plugin_setting("setting", "scale"),
        Some(SettingValue::Number(3.0)),
        "what the preferences hold comes back in a later session"
    );

    state.set_plugin_setting("setting", "scale", SettingValue::Number(99.0));
    assert_eq!(
        state.plugin_setting("setting", "scale"),
        Some(SettingValue::Number(10.0)),
        "a number the setting's range does not hold is kept to it"
    );
}

#[test]
fn a_click_in_the_viewport_reaches_a_guest_that_asked_for_clicks_and_not_one_that_did_not() {
    let mut state = loaded("pick");
    assert!(
        !state.plugin_takes_clicks(),
        "a plugin asks, once its user does"
    );
    state.plugin_panel("pick", "sample").expect("it draws");
    state.deliver_plugin_events(
        "pick",
        "sample",
        vec![Event::Checked {
            id: "pick".into(),
            checked: true,
        }],
    );
    state.plugin_panel("pick", "sample").expect("it draws");
    assert!(state.plugin_takes_clicks());

    state.plugin_pick(Pick {
        lod: 1,
        model: 2,
        geometry: 3,
        polygon: 4,
        vertex: 5,
    });
    let Some([Primitive::Point { at, size, colour }]) = overlay(&state) else {
        panic!("{:?}", overlay(&state));
    };
    assert_eq!(
        (*at, *size, colour[0]),
        ([1.0, 2.0, 3.0], 4.0, 5.0),
        "the five numbers of the pick reached the guest"
    );
}

#[test]
fn an_overlay_is_drawn_only_over_the_document_it_was_drawn_for() {
    let mut state = loaded("overlay");
    state.run_plugin_command("overlay", "add-point");
    assert!(overlay(&state).is_some());

    state.documents.push(blank(2));
    state.set_active_document(2);
    assert!(
        overlay(&state).is_none(),
        "another document is not what the plugin drew over"
    );
    state.set_active_document(1);
    assert!(overlay(&state).is_some());
}

#[test]
fn nothing_is_loaded_before_the_user_has_answered_and_declining_loads_nothing() {
    let plugin = surface_plugin("asks", &SURFACE);
    let mut state = state_with(&plugin, false);
    state.load_plugin_surfaces();
    assert!(
        state.plugin_registrations("asks").is_none() && state.plugins.pending_consent.is_none(),
        "a plugin nobody has answered about is not run, and startup does not ask"
    );

    state.request_plugin_run("asks");
    let pending = state
        .plugins
        .pending_consent
        .as_ref()
        .expect("running it asks first");
    assert_eq!(pending.manifest.wants, SURFACE);
    state.answer_plugin_consent(false);
    assert!(state.plugin_registrations("asks").is_none());

    state.revoke_plugin_consent("asks");
    state.request_plugin_run("asks");
    state.answer_plugin_consent(true);
    assert!(
        state.plugin_registrations("asks").is_some(),
        "allowed, it is loaded and its panel, command and setting are there"
    );

    state.revoke_plugin_consent("asks");
    assert!(
        state.plugin_registrations("asks").is_none() && overlay(&state).is_none(),
        "revoking takes the plugin out of the editor"
    );
}

#[test]
fn a_plugin_that_did_not_want_a_surface_capability_cannot_add_to_the_surface() {
    let plugin = surface_plugin("narrow", &[Capability::AddPanels]);
    let mut state = state_with(&plugin, true);
    state.load_plugin_surfaces();

    assert!(state.plugin_registrations("narrow").is_none());
    let report = state
        .plugin_report("narrow")
        .expect("a plugin that did not load says why");
    let named: Vec<String> = report
        .iter()
        .filter_map(|d| d.arg("capability").map(ToString::to_string))
        .collect();
    assert!(
        matches!(
            named.as_slice(),
            [one] if ["cap.add_commands", "cap.add_settings", "cap.draw_overlays"].contains(&one.as_str())
        ),
        "the refusal names a capability the plugin did not ask for: {report:?}"
    );
}

/// One frame of the panel `id`, with the UI's accessibility tree: each widget's role, label and bounds.
fn panel_frame(
    ctx: &egui::Context,
    state: &mut AppState,
    id: &str,
    events: Vec<egui::Event>,
) -> Vec<(egui::accesskit::Role, String, egui::Rect)> {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        events,
        ..Default::default()
    };
    let id = crate::ui::panel_registry::intern(id);
    let output = ctx.run_ui(input, |ui| {
        let mut panel = crate::ui::panel_registry::PanelContext {
            state: &mut *state,
            bridge: None,
        };
        crate::ui::panel_registry::draw_panel(id, ui, &mut panel);
    });
    output
        .platform_output
        .accesskit_update
        .map(|update| {
            update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    let bounds = node.bounds()?;
                    let rect = egui::Rect::from_min_max(
                        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                    );
                    let text = node.label().or_else(|| node.value())?;
                    Some((node.role(), text.to_string(), rect))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn pointer(at: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn a_click_on_the_registered_panels_button_in_a_real_frame_reaches_the_guest() {
    use egui::accesskit::Role;

    let mut state = loaded("frame");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let id = crate::ui::panels::plugin_panel::panel_id("frame", "sample");
    assert!(
        crate::ui::panel_registry::get(crate::ui::panel_registry::intern(&id)).is_some(),
        "loading the plugin registered its panel"
    );

    let drawn = panel_frame(&ctx, &mut state, &id, Vec::new());
    let press = drawn
        .iter()
        .find(|(role, label, _)| *role == Role::Button && label == "Press")
        .unwrap_or_else(|| panic!("the guest's button is on screen: {drawn:#?}"));
    assert!(
        drawn
            .iter()
            .any(|(_, label, _)| label.starts_with("pressed 0 times")),
        "{drawn:#?}"
    );
    let at = press.2.center();

    panel_frame(&ctx, &mut state, &id, vec![egui::Event::PointerMoved(at)]);
    panel_frame(&ctx, &mut state, &id, vec![pointer(at, true)]);
    panel_frame(&ctx, &mut state, &id, vec![pointer(at, false)]);
    let after = panel_frame(&ctx, &mut state, &id, Vec::new());
    assert!(
        after
            .iter()
            .any(|(_, label, _)| label.starts_with("pressed 1 times")),
        "the click on the host's button reached the guest, which counted it: {after:#?}"
    );
    let later = panel_frame(&ctx, &mut state, &id, Vec::new());
    assert!(
        later
            .iter()
            .any(|(_, label, _)| label.starts_with("pressed 1 times")),
        "once: {later:#?}"
    );
}

#[test]
fn a_panel_of_a_plugin_that_is_not_loaded_says_so_and_offers_to_load_it() {
    let mut state = loaded("gone");
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let id = crate::ui::panels::plugin_panel::panel_id("gone", "sample");
    state.revoke_plugin_consent("gone");

    let drawn = panel_frame(&ctx, &mut state, &id, Vec::new());
    assert!(
        drawn
            .iter()
            .any(|(_, label, _)| label.contains("panel is not running")),
        "{drawn:#?}"
    );
    assert!(
        drawn.iter().any(|(_, label, _)| label == "Load the plugin"),
        "{drawn:#?}"
    );
}

#[test]
fn a_setting_the_user_changes_is_written_to_the_preferences() {
    let mut state = loaded("written");
    state.set_plugin_setting("written", "scale", SettingValue::Number(4.0));
    state.persist_plugin_settings();

    let reread = crate::state::prefs::load_prefs();
    assert_eq!(
        reread.plugin_settings["written"]["scale"],
        serde_json::json!(4.0),
        "the preferences on disk hold what the user chose"
    );
    let mut next = state_with(&surface_plugin("written", &SURFACE), true);
    next.prefs = reread;
    next.load_plugin_surfaces();
    assert_eq!(
        next.plugin_setting("written", "scale"),
        Some(SettingValue::Number(4.0))
    );
}

#[test]
fn an_unreadable_setting_costs_only_itself() {
    let prefs: crate::state::Prefs = serde_json::from_str(
        r#"{"plugin_settings":{"kept":{"scale":2.5},"garbled":5,"also":{"x":"y"}}}"#,
    )
    .expect("the rest of the preferences is read");
    assert_eq!(prefs.plugin_settings.len(), 2);
    assert_eq!(
        prefs.plugin_settings["kept"]["scale"],
        serde_json::json!(2.5)
    );

    let mut state = state_with(&surface_plugin("kept", &SURFACE), true);
    state.prefs.plugin_settings.insert(
        "kept".into(),
        [("scale".to_string(), serde_json::json!("not a number"))].into(),
    );
    state.load_plugin_surfaces();
    assert_eq!(
        state.plugin_setting("kept", "scale"),
        Some(SettingValue::Number(1.0)),
        "a stored value of another kind is the default"
    );
}

#[test]
fn a_key_press_runs_the_command_a_plugin_bound_and_a_binding_taken_is_not_given() {
    use winit::keyboard::{Key, KeyCode, ModifiersState, PhysicalKey};

    let press = |state: &AppState, mods: ModifiersState| {
        crate::ui::shortcuts::plugin_command_for_press(
            &Key::Character("p".into()),
            PhysicalKey::Code(KeyCode::KeyP),
            mods,
            false,
            state,
        )
    };
    let state = loaded("keys");
    assert_eq!(
        press(
            &state,
            crate::ui::shortcuts::PRIMARY_STATE | ModifiersState::ALT
        ),
        Some(("keys".to_string(), "add-point".to_string()))
    );
    assert_eq!(press(&state, crate::ui::shortcuts::PRIMARY_STATE), None);

    let mut taken = state_with(&surface_plugin("taken", &SURFACE), true);
    crate::ui::shortcuts::force_set_binding(
        &mut taken.prefs,
        crate::ui::commands::Action::ShowGrid,
        crate::ui::shortcuts::ShortcutMods {
            ctrl: true,
            control: false,
            shift: false,
            alt: true,
        },
        egui::Key::P,
    );
    taken.load_plugin_surfaces();
    assert_eq!(
        press(
            &taken,
            crate::ui::shortcuts::PRIMARY_STATE | ModifiersState::ALT
        ),
        None,
        "a key another command holds stays with it"
    );
    let command = crate::ui::commands::all(&taken)
        .into_iter()
        .find(|c| c.id == "plugin command: taken: add-point")
        .expect("the command is still offered");
    assert_eq!(command.binding, None);
    let report = taken.plugin_report("taken").expect("a report");
    assert!(
        report
            .iter()
            .any(|d| d.key == "plugin.shortcut" && d.message.contains("already holds")),
        "{report:?}"
    );
}

/// A line vertex as its position and colour, which compare.
fn raw(lines: &[or_renderer::LineVertex]) -> Vec<([f32; 3], [f32; 4])> {
    lines.iter().map(|v| (v.position, v.colour)).collect()
}

#[test]
fn an_overlay_becomes_the_line_vertices_the_viewport_draws() {
    let mut state = loaded("lines");
    let mut lines = Vec::new();
    crate::render::plugin_overlay::append(&mut lines, &state);
    assert!(lines.is_empty(), "nothing is drawn before the guest draws");

    state.run_plugin_command("lines", "add-point");
    crate::render::plugin_overlay::append(&mut lines, &state);
    let drawn = overlay(&state).expect("the guest drew").to_vec();
    assert_eq!(
        lines.len(),
        2 + 6,
        "a line and the cross of a point: {drawn:?}"
    );
    assert_eq!(
        raw(&lines),
        raw(&crate::render::plugin_overlay::lines_of(&drawn)),
        "what the viewport gets is the guest's primitives"
    );
}

#[test]
#[ignore = "needs a GPU adapter: acquisition can hang, not just fail"]
fn the_render_bridge_draws_what_a_plugin_drew_in_front_of_the_meshes() {
    const WHAT: &str = "the_render_bridge_draws_what_a_plugin_drew_in_front_of_the_meshes";
    let Some((device, queue)) = crate::render::test_device(WHAT) else {
        return;
    };
    let mut bridge = crate::render::RenderBridge::new(
        std::sync::Arc::new(device),
        std::sync::Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut state = loaded("bridge");
    bridge.refresh_lines(&state);
    let before = bridge.line_vertices().len();

    state.run_plugin_command("bridge", "add-point");
    bridge.refresh_lines(&state);
    let drawn = overlay(&state).expect("the guest drew").to_vec();
    let lines = bridge.line_vertices();
    assert_eq!(lines.len(), before + 8);
    assert_eq!(
        raw(&lines[before..]),
        raw(&crate::render::plugin_overlay::lines_of(&drawn)),
        "the primitives are the last lines, which draw in front"
    );
}

#[test]
fn a_plugin_and_its_manifest_dropped_in_the_plugins_directory_add_a_panel_a_command_and_a_setting()
{
    let dir = tempfile::tempdir().expect("a scratch directory");
    let dropped = dir.path().join("dropped");
    std::fs::create_dir_all(&dropped).expect("mkdir");
    std::fs::write(
        dropped.join("plugin.json"),
        r#"{"id":"dropped","name":"Dropped","abi":0,"wants":["cap.add_panels","cap.add_commands","cap.add_settings","cap.draw_overlays"]}"#,
    )
    .expect("write");
    std::fs::copy(surface_guest(), dropped.join("plugin.wasm")).expect("copy");

    let found = super::installed_in(dir.path());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].refused, None);
    assert_eq!(found[0].manifest.wants, SURFACE);

    let mut state = state_with(&found[0], true);
    state.load_plugin_surfaces();
    let registered = state.plugin_registrations("dropped").expect("it loaded");
    assert_eq!(
        (
            registered.panels.len(),
            registered.commands.len(),
            registered.settings.len()
        ),
        (2, 1, 1)
    );
}

/// The sample built with `surface` and `bytes`: its commands draw, and one rewrites the open `.ydd` with `or-io`.
fn rewriting_plugin(id: &str) -> InstalledPlugin {
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let wasm = BUILT
        .get_or_init(|| built_sample("surface,bytes", "openrage-plugin-sample-surface-bytes"))
        .clone();
    InstalledPlugin {
        manifest: Manifest {
            id: id.into(),
            name: "Rewrites".into(),
            abi: ABI_VERSION,
            wants: vec![
                Capability::ReadFileBytes,
                Capability::WriteFileBytes,
                Capability::AddPanels,
                Capability::AddCommands,
                Capability::AddSettings,
                Capability::DrawOverlays,
            ],
        },
        wasm,
        refused: None,
    }
}

#[test]
fn a_command_reads_the_open_file_and_replaces_it_through_the_edit_mode() {
    use crate::state::a_plugin_asks_before_it_runs::{first_distance, state_with_ydd};
    use crate::state::EditMode;

    let root = tempfile::tempdir().expect("a scratch directory");
    let plugin = rewriting_plugin("rewrites");
    let mut state = state_with_ydd(&plugin, root.path().join("mods/thing.ydd"));
    state
        .prefs
        .game_paths
        .set(or_title::Game::GtaVLegacy, Some(root.path().to_path_buf()));
    state.prefs.edit_mode = EditMode::ReadOnly;
    state.retail_edit_session_allow = false;
    state.prefs.plugin_consent.insert(
        "rewrites".into(),
        Consent::Granted(
            plugin
                .manifest
                .wants
                .iter()
                .map(|c| c.key().to_string())
                .collect(),
        ),
    );
    state.load_plugin_surfaces();
    let distance = |state: &AppState| first_distance(state.active_document().expect("open"));

    state.run_plugin_command("rewrites", "double-lod");
    assert_eq!(distance(&state), 100.0, "the edit mode refused the file");
    assert!(!state.active_document().expect("open").dirty);
    let report = state.plugin_report("rewrites").expect("a report");
    assert!(
        report.iter().any(|d| {
            d.arg("capability").map(ToString::to_string).as_deref() == Some("cap.write_file_bytes")
                && d.message.contains(EditMode::ReadOnly.semantics())
        }),
        "the refusal names the capability and carries the editor's own sentence: {report:?}"
    );
    assert!(
        state.plugin_commands().iter().all(|c| c.stopped.is_none()),
        "a refusal is the host's to show, and the guest runs on"
    );

    state.prefs.edit_mode = EditMode::Overlay;
    state.run_plugin_command("rewrites", "double-lod");
    assert_eq!(
        distance(&state),
        200.0,
        "the same grant, once the edit mode permits it: {:?}",
        state.plugin_report("rewrites")
    );
    assert!(state.active_document().expect("open").dirty);

    state.run_plugin_command("rewrites", "double-lod");
    assert_eq!(
        distance(&state),
        400.0,
        "the next call is handed the file the document is now, not the one it was"
    );
}
